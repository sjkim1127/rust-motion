//! Image filter pipeline, color grading, shadows, blur, and photographic filters.

use crate::backend::RasterError;
use crate::scene::{SceneFilter, SceneShadow};
use rayon::prelude::*;
use tiny_skia::{Pixmap, PixmapPaint, Transform};

use super::color_blend::{
    apply_adjustment_to_pixmap, composite_custom_psd_blend, tiny_skia_blend_mode,
};

pub(crate) fn apply_filter(pixmap: &mut Pixmap, filter: &SceneFilter) -> Result<(), RasterError> {
    match filter.clone() {
        SceneFilter::Blur { sigma } => {
            if !sigma.is_finite() || !(0.0..=100.0).contains(&sigma) {
                return Err(RasterError::Scene(format!(
                    "blur sigma must be finite and between 0 and 100, got {sigma}"
                )));
            }
            let radius = (sigma * 1.5).ceil().clamp(0.0, 128.0) as usize;
            box_blur(pixmap, radius);
        }
        SceneFilter::Brightness { amount } => {
            if !amount.is_finite() || !(0.0..=10.0).contains(&amount) {
                return Err(RasterError::Scene(format!(
                    "brightness must be finite and between 0 and 10, got {amount}"
                )));
            }
            pixmap.data_mut().par_chunks_exact_mut(4).for_each(|pixel| {
                let alpha = pixel[3];
                for channel in &mut pixel[..3] {
                    *channel = (f32::from(*channel) * amount)
                        .round()
                        .clamp(0.0, f32::from(alpha)) as u8;
                }
            });
        }
        SceneFilter::Grayscale { amount } => {
            if !amount.is_finite() || !(0.0..=1.0).contains(&amount) {
                return Err(RasterError::Scene(format!(
                    "grayscale must be finite and between 0 and 1, got {amount}"
                )));
            }
            pixmap.data_mut().par_chunks_exact_mut(4).for_each(|pixel| {
                let alpha = pixel[3];
                let gray = 0.2126 * f32::from(pixel[0])
                    + 0.7152 * f32::from(pixel[1])
                    + 0.0722 * f32::from(pixel[2]);
                for channel in &mut pixel[..3] {
                    *channel = (f32::from(*channel) + (gray - f32::from(*channel)) * amount)
                        .round()
                        .clamp(0.0, f32::from(alpha)) as u8;
                }
            });
        }
        SceneFilter::Opacity { amount } => {
            if !amount.is_finite() || !(0.0..=1.0).contains(&amount) {
                return Err(RasterError::Scene(format!(
                    "filter opacity must be finite and between 0 and 1, got {amount}"
                )));
            }
            for channel in pixmap.data_mut() {
                *channel = (f32::from(*channel) * amount).round() as u8;
            }
        }
        SceneFilter::ChromaticAberration {
            offset_x,
            offset_y,
            angle_rad,
        } => {
            if !offset_x.is_finite() || !offset_y.is_finite() || !angle_rad.is_finite() {
                return Err(RasterError::Scene(
                    "chromatic aberration parameters must be finite".into(),
                ));
            }
            let cos_a = angle_rad.cos();
            let sin_a = angle_rad.sin();
            let dx = offset_x * cos_a - offset_y * sin_a;
            let dy = offset_x * sin_a + offset_y * cos_a;

            if dx.abs() < 1e-6 && dy.abs() < 1e-6 {
                return Ok(());
            }

            let width = pixmap.width() as i32;
            let height = pixmap.height() as i32;
            let src = pixmap.data().to_vec();

            let sample_channel = |x: f32, y: f32, channel: usize| -> f32 {
                if x < 0.0 || x > (width - 1) as f32 || y < 0.0 || y > (height - 1) as f32 {
                    return 0.0;
                }
                let x0 = x.floor() as i32;
                let y0 = y.floor() as i32;
                let x1 = (x0 + 1).min(width - 1);
                let y1 = (y0 + 1).min(height - 1);
                let fx = x - x.floor();
                let fy = y - y.floor();

                let p00 = src[((y0 * width + x0) * 4) as usize + channel] as f32;
                let p10 = src[((y0 * width + x1) * 4) as usize + channel] as f32;
                let p01 = src[((y1 * width + x0) * 4) as usize + channel] as f32;
                let p11 = src[((y1 * width + x1) * 4) as usize + channel] as f32;

                let top = p00 * (1.0 - fx) + p10 * fx;
                let bot = p01 * (1.0 - fx) + p11 * fx;
                top * (1.0 - fy) + bot * fy
            };

            let dst = pixmap.data_mut();
            for y in 0..height {
                for x in 0..width {
                    let idx = ((y * width + x) * 4) as usize;
                    let fx = x as f32;
                    let fy = y as f32;

                    let a_r = sample_channel(fx - dx, fy - dy, 3);
                    let a_g = src[idx + 3] as f32;
                    let a_b = sample_channel(fx + dx, fy + dy, 3);
                    let alpha = a_r.max(a_g).max(a_b);

                    if alpha == 0.0 {
                        continue;
                    }

                    let r = sample_channel(fx - dx, fy - dy, 0);
                    let g = src[idx + 1] as f32;
                    let b = sample_channel(fx + dx, fy + dy, 2);

                    dst[idx] = r.round().clamp(0.0, alpha) as u8;
                    dst[idx + 1] = g.round().clamp(0.0, alpha) as u8;
                    dst[idx + 2] = b.round().clamp(0.0, alpha) as u8;
                    dst[idx + 3] = alpha.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        SceneFilter::Vignette {
            offset,
            darkness,
            roundness,
        } => {
            if !offset.is_finite() || !darkness.is_finite() || !roundness.is_finite() {
                return Err(RasterError::Scene(
                    "vignette parameters must be finite".into(),
                ));
            }
            let off = offset.clamp(0.0, 2.0);
            let dark = darkness.clamp(0.0, 1.0);
            let rnd = roundness.clamp(0.0, 1.0);

            let width = pixmap.width() as f32;
            let height = pixmap.height() as f32;
            let max_diag = (1.0 - rnd) * 1.0 + rnd * 2.0f32.sqrt();
            let span = (max_diag - off).max(1e-5);
            let row_len = pixmap.width() as usize * 4;
            let px_by_x: Vec<f32> = (0..pixmap.width())
                .map(|x| {
                    let u = (x as f32 + 0.5) / width;
                    2.0 * (u - 0.5).abs()
                })
                .collect();

            pixmap
                .data_mut()
                .par_chunks_exact_mut(row_len)
                .enumerate()
                .for_each(|(y, row)| {
                    let v = (y as f32 + 0.5) / height;
                    let py = 2.0 * (v - 0.5).abs();
                    for (x, pixel) in row.chunks_exact_mut(4).enumerate() {
                        let px = px_by_x[x];

                        let d_rect = px.max(py);
                        let d_ellipse = (px * px + py * py).sqrt();
                        let d = (1.0 - rnd) * d_rect + rnd * d_ellipse;

                        let factor = if d <= off {
                            1.0
                        } else {
                            let t = ((d - off) / span).clamp(0.0, 1.0);
                            let s = t * t * (3.0 - 2.0 * t);
                            1.0 - dark * s
                        };

                        let alpha = pixel[3] as f32;
                        for ch in pixel[0..3].iter_mut() {
                            *ch = (*ch as f32 * factor).round().clamp(0.0, alpha) as u8;
                        }
                    }
                });
        }
        SceneFilter::Contrast { factor } => {
            if !factor.is_finite() || factor < 0.0 {
                return Err(RasterError::Scene(
                    "contrast factor must be non-negative and finite".into(),
                ));
            }
            pixmap.data_mut().par_chunks_exact_mut(4).for_each(|pixel| {
                let alpha = pixel[3] as f32;
                if alpha == 0.0 {
                    return;
                }
                for channel in &mut pixel[..3] {
                    let unpre = (*channel as f32 / alpha) * 255.0;
                    let contrasted = (unpre - 128.0) * factor + 128.0;
                    *channel = ((contrasted / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
                }
            });
        }
        SceneFilter::Saturation { factor } => {
            if !factor.is_finite() || factor < 0.0 {
                return Err(RasterError::Scene(
                    "saturation factor must be non-negative and finite".into(),
                ));
            }
            pixmap.data_mut().par_chunks_exact_mut(4).for_each(|pixel| {
                let alpha = pixel[3] as f32;
                if alpha == 0.0 {
                    return;
                }
                let r_unpre = (pixel[0] as f32 / alpha) * 255.0;
                let g_unpre = (pixel[1] as f32 / alpha) * 255.0;
                let b_unpre = (pixel[2] as f32 / alpha) * 255.0;

                let luma = 0.299 * r_unpre + 0.587 * g_unpre + 0.114 * b_unpre;

                let r_sat = (luma + (r_unpre - luma) * factor).clamp(0.0, 255.0);
                let g_sat = (luma + (g_unpre - luma) * factor).clamp(0.0, 255.0);
                let b_sat = (luma + (b_unpre - luma) * factor).clamp(0.0, 255.0);

                pixel[0] = ((r_sat / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
                pixel[1] = ((g_sat / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
                pixel[2] = ((b_sat / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
            });
        }
        SceneFilter::HueRotate { degrees } => {
            if !degrees.is_finite() {
                return Err(RasterError::Scene("hue degrees must be finite".into()));
            }
            let shift = (degrees % 360.0 + 360.0) % 360.0;
            if shift.abs() < 1e-4 {
                return Ok(());
            }

            pixmap.data_mut().par_chunks_exact_mut(4).for_each(|pixel| {
                let alpha = pixel[3] as f32;
                if alpha == 0.0 {
                    return;
                }
                let r_u = pixel[0] as f32 / alpha;
                let g_u = pixel[1] as f32 / alpha;
                let b_u = pixel[2] as f32 / alpha;

                let max = r_u.max(g_u).max(b_u);
                let min = r_u.min(g_u).min(b_u);
                let delta = max - min;
                let v = max;
                let s = if max > 0.0 { delta / max } else { 0.0 };
                let mut h = if delta.abs() < 1e-6 {
                    0.0
                } else if (max - r_u).abs() < 1e-6 {
                    60.0 * (((g_u - b_u) / delta) % 6.0)
                } else if (max - g_u).abs() < 1e-6 {
                    60.0 * (((b_u - r_u) / delta) + 2.0)
                } else {
                    60.0 * (((r_u - g_u) / delta) + 4.0)
                };
                if h < 0.0 {
                    h += 360.0;
                }

                h = (h + shift) % 360.0;

                let c = v * s;
                let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
                let m = v - c;

                let (r_norm, g_norm, b_norm) = match (h / 60.0) as u32 {
                    0 => (c, x, 0.0),
                    1 => (x, c, 0.0),
                    2 => (0.0, c, x),
                    3 => (0.0, x, c),
                    4 => (x, 0.0, c),
                    _ => (c, 0.0, x),
                };

                pixel[0] = ((r_norm + m) * alpha).round().clamp(0.0, alpha) as u8;
                pixel[1] = ((g_norm + m) * alpha).round().clamp(0.0, alpha) as u8;
                pixel[2] = ((b_norm + m) * alpha).round().clamp(0.0, alpha) as u8;
            });
        }
        SceneFilter::Invert { amount } => {
            if !amount.is_finite() || !(0.0..=1.0).contains(&amount) {
                return Err(RasterError::Scene(format!(
                    "invert amount must be finite and between 0 and 1, got {amount}"
                )));
            }
            pixmap.data_mut().par_chunks_exact_mut(4).for_each(|pixel| {
                let alpha = pixel[3] as f32;
                if alpha == 0.0 {
                    return;
                }
                for channel in &mut pixel[..3] {
                    let unpre = (*channel as f32 / alpha) * 255.0;
                    let inverted = unpre + (255.0 - 2.0 * unpre) * amount;
                    *channel = ((inverted / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
                }
            });
        }
        SceneFilter::Tint { color, amount } => {
            if !amount.is_finite() || !(0.0..=1.0).contains(&amount) {
                return Err(RasterError::Scene(format!(
                    "tint amount must be finite and between 0 and 1, got {amount}"
                )));
            }
            let tr = color[0] as f32;
            let tg = color[1] as f32;
            let tb = color[2] as f32;
            let ta = (color[3] as f32 / 255.0) * amount;

            pixmap.data_mut().par_chunks_exact_mut(4).for_each(|pixel| {
                let alpha = pixel[3] as f32;
                if alpha == 0.0 {
                    return;
                }
                let r_unpre = (pixel[0] as f32 / alpha) * 255.0;
                let g_unpre = (pixel[1] as f32 / alpha) * 255.0;
                let b_unpre = (pixel[2] as f32 / alpha) * 255.0;

                let r_tint = r_unpre * (1.0 - ta) + tr * ta;
                let g_tint = g_unpre * (1.0 - ta) + tg * ta;
                let b_tint = b_unpre * (1.0 - ta) + tb * ta;

                pixel[0] = ((r_tint / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
                pixel[1] = ((g_tint / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
                pixel[2] = ((b_tint / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
            });
        }
        SceneFilter::Duotone { primary, secondary } => {
            let pr = primary[0] as f32;
            let pg = primary[1] as f32;
            let pb = primary[2] as f32;
            let sr = secondary[0] as f32;
            let sg = secondary[1] as f32;
            let sb = secondary[2] as f32;

            for pixel in pixmap.data_mut().chunks_exact_mut(4) {
                let alpha = pixel[3] as f32;
                if alpha == 0.0 {
                    continue;
                }
                let r_unpre = (pixel[0] as f32 / alpha) * 255.0;
                let g_unpre = (pixel[1] as f32 / alpha) * 255.0;
                let b_unpre = (pixel[2] as f32 / alpha) * 255.0;

                let luma = (0.299 * r_unpre + 0.587 * g_unpre + 0.114 * b_unpre) / 255.0;
                let luma = luma.clamp(0.0, 1.0);

                let r_duo = pr * (1.0 - luma) + sr * luma;
                let g_duo = pg * (1.0 - luma) + sg * luma;
                let b_duo = pb * (1.0 - luma) + sb * luma;

                pixel[0] = ((r_duo / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
                pixel[1] = ((g_duo / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
                pixel[2] = ((b_duo / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
            }
        }
        SceneFilter::ColorGrading {
            contrast,
            saturation,
            gamma,
            tint,
        } => {
            if !contrast.is_finite()
                || !saturation.is_finite()
                || !gamma.is_finite()
                || gamma <= 0.0
                || contrast < 0.0
                || saturation < 0.0
            {
                return Err(RasterError::Scene(
                    "color grading parameters must be positive/finite".into(),
                ));
            }
            let inv_gamma = 1.0 / gamma;

            for pixel in pixmap.data_mut().chunks_exact_mut(4) {
                let alpha = pixel[3] as f32;
                if alpha == 0.0 {
                    continue;
                }
                let r_unpre = (pixel[0] as f32 / alpha) * 255.0;
                let g_unpre = (pixel[1] as f32 / alpha) * 255.0;
                let b_unpre = (pixel[2] as f32 / alpha) * 255.0;

                // 1. Gamma
                let mut r = 255.0 * (r_unpre / 255.0).clamp(0.0, 1.0).powf(inv_gamma);
                let mut g = 255.0 * (g_unpre / 255.0).clamp(0.0, 1.0).powf(inv_gamma);
                let mut b = 255.0 * (b_unpre / 255.0).clamp(0.0, 1.0).powf(inv_gamma);

                // 2. Contrast
                r = (r - 128.0) * contrast + 128.0;
                g = (g - 128.0) * contrast + 128.0;
                b = (b - 128.0) * contrast + 128.0;

                // 3. Saturation (Rec.601)
                let luma = 0.299 * r + 0.587 * g + 0.114 * b;
                r = luma + (r - luma) * saturation;
                g = luma + (g - luma) * saturation;
                b = luma + (b - luma) * saturation;

                // 4. Tint
                if let Some(tint_col) = tint {
                    let tr = tint_col[0] as f32;
                    let tg = tint_col[1] as f32;
                    let tb = tint_col[2] as f32;
                    let ta = (tint_col[3] as f32) / 255.0;
                    r = r * (1.0 - ta) + tr * ta;
                    g = g * (1.0 - ta) + tg * ta;
                    b = b * (1.0 - ta) + tb * ta;
                }

                pixel[0] = ((r.clamp(0.0, 255.0) / 255.0) * alpha)
                    .round()
                    .clamp(0.0, alpha) as u8;
                pixel[1] = ((g.clamp(0.0, 255.0) / 255.0) * alpha)
                    .round()
                    .clamp(0.0, alpha) as u8;
                pixel[2] = ((b.clamp(0.0, 255.0) / 255.0) * alpha)
                    .round()
                    .clamp(0.0, alpha) as u8;
            }
        }
        SceneFilter::ColorKey {
            key_color,
            similarity,
            smoothness,
            spill_suppression,
        } => {
            if !similarity.is_finite()
                || !smoothness.is_finite()
                || !spill_suppression.is_finite()
                || similarity < 0.0
                || smoothness < 0.0
            {
                return Err(RasterError::Scene(
                    "color key parameters must be non-negative and finite".into(),
                ));
            }
            let kr = key_color[0] as f32 / 255.0;
            let kg = key_color[1] as f32 / 255.0;
            let kb = key_color[2] as f32 / 255.0;
            let sqrt3 = 3.0f32.sqrt();

            let edge0 = (similarity - smoothness).max(0.0);
            let edge1 = (similarity + smoothness).min(1.0);
            let ss = spill_suppression.clamp(0.0, 1.0);

            for pixel in pixmap.data_mut().chunks_exact_mut(4) {
                let alpha = pixel[3] as f32;
                if alpha <= 0.25 {
                    continue;
                }
                let mut r_u = pixel[0] as f32 / alpha;
                let mut g_u = pixel[1] as f32 / alpha;
                let mut b_u = pixel[2] as f32 / alpha;

                let dr = r_u - kr;
                let dg = g_u - kg;
                let db = b_u - kb;
                let dist = ((dr * dr + dg * dg + db * db).sqrt() / sqrt3).clamp(0.0, 1.0);

                let mask = if edge1 > edge0 {
                    let t = ((dist - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
                    t * t * (3.0 - 2.0 * t)
                } else if dist >= similarity {
                    1.0
                } else {
                    0.0
                };

                if ss > 0.0 {
                    if kg > kr && kg > kb {
                        let max_g = (r_u + b_u) / 2.0;
                        if g_u > max_g {
                            g_u = g_u * (1.0 - ss) + max_g * ss;
                        }
                    } else if kb > kr && kb > kg {
                        let max_b = (r_u + g_u) / 2.0;
                        if b_u > max_b {
                            b_u = b_u * (1.0 - ss) + max_b * ss;
                        }
                    } else if kr > kg && kr > kb {
                        let max_r = (g_u + b_u) / 2.0;
                        if r_u > max_r {
                            r_u = r_u * (1.0 - ss) + max_r * ss;
                        }
                    }
                }

                let new_alpha = (alpha * mask).round().clamp(0.0, 255.0);
                pixel[0] = (r_u * new_alpha).round().clamp(0.0, new_alpha) as u8;
                pixel[1] = (g_u * new_alpha).round().clamp(0.0, new_alpha) as u8;
                pixel[2] = (b_u * new_alpha).round().clamp(0.0, new_alpha) as u8;
                pixel[3] = new_alpha as u8;
            }
        }
        SceneFilter::CameraMotionBlur {
            shutter_angle,
            samples,
            shutter_phase,
            angle_deg,
            kernel,
        } => {
            if !shutter_angle.is_finite() || !shutter_phase.is_finite() {
                return Err(RasterError::Scene(
                    "CameraMotionBlur parameters must be finite".into(),
                ));
            }
            let angle_clamped = shutter_angle.clamp(0.0, 360.0);
            if angle_clamped < 0.01 || samples <= 1 {
                return Ok(());
            }

            let angle = angle_deg.unwrap_or(0.0);
            let w = pixmap.width() as f32;
            let exposure = angle_clamped / 360.0;
            let length = (w * 0.2 * exposure).max(4.0);

            crate::motion_blur::apply_directional_motion_blur(
                pixmap, angle, length, samples, kernel,
            )?;
        }
        SceneFilter::DirectionalMotionBlur {
            angle_deg,
            length,
            samples,
            kernel,
        } => {
            crate::motion_blur::apply_directional_motion_blur(
                pixmap, angle_deg, length, samples, kernel,
            )?;
        }
        SceneFilter::OpticalGlow {
            threshold,
            radius,
            intensity,
            anamorphic_ratio,
            tint,
            blend_mode,
        } => {
            let glow = crate::vfx::OpticalGlow {
                threshold,
                radius,
                intensity,
                anamorphic_ratio,
                tint,
                blend_mode,
            };
            crate::vfx::apply_optical_glow(pixmap, &glow)?;
        }
        SceneFilter::DisplacementMap {
            channel_x,
            channel_y,
            max_horizontal,
            max_vertical,
            wrap_mode,
            ref map_source,
            map_width,
            map_height,
        } => {
            let disp = crate::vfx::DisplacementMap {
                channel_x,
                channel_y,
                max_horizontal,
                max_vertical,
                wrap_mode,
                map_source: map_source.clone(),
                map_width,
                map_height,
            };
            crate::vfx::apply_displacement_map(pixmap, &disp)?;
        }
        SceneFilter::FractalNoise {
            noise_type,
            fractal_type,
            contrast,
            brightness,
            complexity,
            scale_x,
            scale_y,
            evolution,
            seed,
            blend_mode,
            opacity,
        } => {
            let config = crate::vfx::FractalNoise {
                noise_type,
                fractal_type,
                contrast,
                brightness,
                complexity,
                scale_x,
                scale_y,
                evolution,
                seed,
            };
            let noise_pixmap =
                crate::vfx::generate_fractal_noise_pixmap(pixmap.width(), pixmap.height(), &config);
            if let Some(sk_mode) = tiny_skia_blend_mode(blend_mode) {
                let paint = PixmapPaint {
                    opacity: opacity.clamp(0.0, 1.0),
                    blend_mode: sk_mode,
                    ..Default::default()
                };
                pixmap.draw_pixmap(
                    0,
                    0,
                    noise_pixmap.as_ref(),
                    &paint,
                    Transform::identity(),
                    None,
                );
            } else {
                composite_custom_psd_blend(
                    pixmap,
                    &noise_pixmap,
                    blend_mode.to_color_blend_mode(),
                    opacity.clamp(0.0, 1.0),
                    0,
                    0,
                );
            }
        }
        SceneFilter::Curves {
            master,
            red,
            green,
            blue,
        } => {
            let to_pts = |pts: &[[f32; 2]]| {
                pts.iter()
                    .map(|p| rust_motion_color::adjust::CurvePoint {
                        input: p[0],
                        output: p[1],
                    })
                    .collect()
            };
            let adj = rust_motion_color::adjust::Adjustment::Curves {
                master: to_pts(&master),
                per_channel: [to_pts(&red), to_pts(&green), to_pts(&blue)],
                space: rust_motion_color::adjust::ToneSpace::Rgb,
                black: Vec::new(),
                pencil: false,
            };
            apply_adjustment_to_pixmap(pixmap, &adj);
        }
        SceneFilter::Levels {
            in_black,
            in_white,
            gamma,
            out_black,
            out_white,
        } => {
            let ch = rust_motion_color::adjust::LevelsChannel {
                in_black,
                in_white,
                gamma,
                out_black,
                out_white,
            };
            let adj = rust_motion_color::adjust::Adjustment::Levels {
                master: ch,
                per_channel: Default::default(),
                space: rust_motion_color::adjust::ToneSpace::Rgb,
                black: Default::default(),
            };
            apply_adjustment_to_pixmap(pixmap, &adj);
        }
        SceneFilter::Vibrance {
            vibrance,
            saturation,
        } => {
            let adj = rust_motion_color::adjust::Adjustment::Vibrance {
                vibrance,
                saturation,
            };
            apply_adjustment_to_pixmap(pixmap, &adj);
        }
        SceneFilter::ColorBalance {
            shadows,
            midtones,
            highlights,
            preserve_luminosity,
        } => {
            let adj = rust_motion_color::adjust::Adjustment::ColorBalance {
                shadows,
                midtones,
                highlights,
                preserve_luminosity,
            };
            apply_adjustment_to_pixmap(pixmap, &adj);
        }
        SceneFilter::Adjustment(adj) => {
            apply_adjustment_to_pixmap(pixmap, &adj);
        }
        SceneFilter::BoxBlur { radius } => {
            if !radius.is_finite() || radius < 0.0 {
                return Err(RasterError::Scene(
                    "box blur radius must be non-negative finite".into(),
                ));
            }
            let params = rust_motion_algo::FilterParams::BoxBlur { radius };
            apply_photo_filter(pixmap, &params)?;
        }
        SceneFilter::RadialBlur {
            amount,
            spin,
            center_x,
            center_y,
        } => {
            if !amount.is_finite() || amount < 0.0 {
                return Err(RasterError::Scene(
                    "radial blur amount must be non-negative finite".into(),
                ));
            }
            let method = if spin {
                rust_motion_algo::RadialMethod::Spin
            } else {
                rust_motion_algo::RadialMethod::Zoom
            };
            let params = rust_motion_algo::FilterParams::RadialBlur {
                amount: amount.clamp(1.0, 100.0),
                method,
                quality: rust_motion_algo::RadialQuality::Good,
                center_x,
                center_y,
            };
            apply_photo_filter(pixmap, &params)?;
        }
        SceneFilter::SurfaceBlur { radius, threshold } => {
            if !radius.is_finite() || radius < 0.0 || !threshold.is_finite() || threshold < 0.0 {
                return Err(RasterError::Scene(
                    "surface blur parameters must be non-negative finite".into(),
                ));
            }
            let params = rust_motion_algo::FilterParams::SurfaceBlur { radius, threshold };
            apply_photo_filter(pixmap, &params)?;
        }
        SceneFilter::UnsharpMask {
            amount,
            radius,
            threshold,
        } => {
            if !amount.is_finite() || !radius.is_finite() || !threshold.is_finite() {
                return Err(RasterError::Scene(
                    "unsharp mask parameters must be finite".into(),
                ));
            }
            let params = rust_motion_algo::FilterParams::UnsharpMask {
                amount,
                radius,
                threshold,
            };
            apply_photo_filter(pixmap, &params)?;
        }
        SceneFilter::SmartSharpen {
            amount,
            radius,
            reduce_noise,
        } => {
            if !amount.is_finite() || !radius.is_finite() {
                return Err(RasterError::Scene(
                    "smart sharpen parameters must be finite".into(),
                ));
            }
            let params = rust_motion_algo::FilterParams::SmartSharpen {
                amount,
                radius,
                reduce_noise,
            };
            apply_photo_filter(pixmap, &params)?;
        }
        SceneFilter::HighPass { radius } => {
            if !radius.is_finite() || radius <= 0.0 {
                return Err(RasterError::Scene(
                    "high pass radius must be positive finite".into(),
                ));
            }
            let params = rust_motion_algo::FilterParams::HighPass { radius };
            apply_photo_filter(pixmap, &params)?;
        }
        SceneFilter::Twirl { angle } => {
            if !angle.is_finite() {
                return Err(RasterError::Scene("twirl angle must be finite".into()));
            }
            let params = rust_motion_algo::FilterParams::Twirl { angle };
            apply_photo_filter(pixmap, &params)?;
        }
        SceneFilter::Spherize { amount } => {
            if !amount.is_finite() {
                return Err(RasterError::Scene("spherize amount must be finite".into()));
            }
            let params = rust_motion_algo::FilterParams::Spherize {
                amount: amount.clamp(-100.0, 100.0),
                mode: rust_motion_algo::SpherizeMode::Normal,
            };
            apply_photo_filter(pixmap, &params)?;
        }
        SceneFilter::Pinch { amount } => {
            if !amount.is_finite() {
                return Err(RasterError::Scene("pinch amount must be finite".into()));
            }
            let params = rust_motion_algo::FilterParams::Pinch {
                amount: amount.clamp(-100.0, 100.0),
            };
            apply_photo_filter(pixmap, &params)?;
        }
        SceneFilter::Ripple { amount, size } => {
            if !amount.is_finite() {
                return Err(RasterError::Scene("ripple amount must be finite".into()));
            }
            let ripple_size = if size < 5.0 {
                rust_motion_algo::RippleSize::Small
            } else if size > 20.0 {
                rust_motion_algo::RippleSize::Large
            } else {
                rust_motion_algo::RippleSize::Medium
            };
            let params = rust_motion_algo::FilterParams::Ripple {
                amount,
                size: ripple_size,
            };
            apply_photo_filter(pixmap, &params)?;
        }
        SceneFilter::PhotoFilter(params) => {
            apply_photo_filter(pixmap, &params)?;
        }
        SceneFilter::WasmPlugin {
            plugin_name,
            wasm_bytes,
            params,
        } => {
            let bytes = if let Some(b) = wasm_bytes {
                b.clone()
            } else {
                let candidate_path = std::path::Path::new("plugins").join(&plugin_name);
                std::fs::read(&candidate_path).map_err(|e| {
                    RasterError::Scene(format!(
                        "failed to load wasm plugin '{plugin_name}' from {candidate_path:?}: {e}"
                    ))
                })?
            };
            let plugin =
                rust_motion_plugins::Plugin::load(&bytes, rust_motion_plugins::Limits::default())
                    .map_err(|e| RasterError::Scene(format!("wasm plugin validation error: {e}")))?;
            let w = pixmap.width();
            let h = pixmap.height();
            if w > 0 && h > 0 {
                let rect = rust_motion_geom::Rect::new(0, 0, w as i32, h as i32);
                let surf = pixmap_to_surface(pixmap);
                let out_surf = plugin.apply(&surf, rect, None, &params).map_err(|e| {
                    RasterError::Scene(format!("wasm plugin execution failed: {e}"))
                })?;
                let new_pixmap = surface_to_pixmap(&out_surf, w, h)?;
                pixmap.data_mut().copy_from_slice(new_pixmap.data());
            }
        }
        SceneFilter::Lut3D {
            cube_content,
            amount,
            tetrahedral,
        } => {
            let lut = if let Some(registered) = crate::lut3d::get(&cube_content) {
                (*registered).clone()
            } else if cube_content.starts_with("3DMESH") || cube_content.contains("Mesh") {
                crate::lut3d::Lut3d::parse_3dl(&cube_content)
                    .map_err(|e| RasterError::Scene(format!("failed to parse .3dl LUT: {e}")))?
            } else {
                crate::lut3d::Lut3d::parse_cube(&cube_content)
                    .map_err(|e| RasterError::Scene(format!("failed to parse .cube LUT: {e}")))?
            };
            lut.apply_to_pixmap(pixmap, amount, tetrahedral);
        }
        SceneFilter::LensCorrection {
            distortion,
            vignette,
            tca,
            center,
        } => {
            crate::lens_correction::apply_lens_correction(
                pixmap, distortion, vignette, tca, center,
            );
        }
        SceneFilter::FilmEmulation {
            grain_amount,
            grain_size,
            grain_roughness,
            grain_seed,
            halation_intensity,
            halation_threshold,
            halation_radius,
            halation_color,
        } => {
            crate::film_emulation::apply_film_emulation(
                pixmap,
                grain_amount,
                grain_size,
                grain_roughness,
                grain_seed,
                halation_intensity,
                halation_threshold,
                halation_radius,
                halation_color,
            );
        }
        SceneFilter::Matte {
            threshold,
            temporal_smooth: _,
        } => {
            let matte = crate::ai_vision::VideoMatting::extract_matte(pixmap, threshold);
            crate::ai_vision::VideoMatting::apply_matte(pixmap, &matte);
        }
        SceneFilter::Inpaint {
            hole_rect,
            patch_radius,
        } => {
            let w = pixmap.width();
            let h = pixmap.height();
            let hx = (hole_rect[0] * w as f32).round().max(0.0) as u32;
            let hy = (hole_rect[1] * h as f32).round().max(0.0) as u32;
            let hw = (hole_rect[2] * w as f32).round().max(1.0) as u32;
            let hh = (hole_rect[3] * h as f32).round().max(1.0) as u32;

            let mut hole_mask = vec![false; (w * h) as usize];
            for y in hy..(hy + hh).min(h) {
                for x in hx..(hx + hw).min(w) {
                    hole_mask[(y * w + x) as usize] = true;
                }
            }
            crate::ai_vision::Inpainter::inpaint(pixmap, &hole_mask, patch_radius);
        }
        SceneFilter::TemporalDenoise {
            strength: _,
            motion_threshold: _,
        } => {}
    }
    Ok(())
}

pub(crate) fn pixmap_to_surface(pixmap: &Pixmap) -> rust_motion_raster::Surface {
    use rust_motion_color::{ColorMode, PixelFormat, SampleType};
    use rust_motion_geom::Rect;
    let w = pixmap.width() as i32;
    let h = pixmap.height() as i32;
    let rect = Rect::new(0, 0, w, h);
    let mut straight = vec![0u8; (w * h * 4) as usize];
    for (src_px, dst_px) in pixmap.pixels().iter().zip(straight.chunks_exact_mut(4)) {
        let a = src_px.alpha();
        if a > 0 {
            let inv_a = 255.0 / a as f32;
            dst_px[0] = (src_px.red() as f32 * inv_a).round().min(255.0) as u8;
            dst_px[1] = (src_px.green() as f32 * inv_a).round().min(255.0) as u8;
            dst_px[2] = (src_px.blue() as f32 * inv_a).round().min(255.0) as u8;
            dst_px[3] = a;
        } else {
            dst_px.fill(0);
        }
    }
    rust_motion_raster::Surface::from_interleaved(
        PixelFormat::new(ColorMode::Rgb, SampleType::U8, true),
        rect,
        &straight,
    )
}

pub(crate) fn surface_to_pixmap(
    surface: &rust_motion_raster::Surface,
    width: u32,
    height: u32,
) -> Result<Pixmap, RasterError> {
    use rust_motion_geom::Rect;
    let rect = Rect::new(0, 0, width as i32, height as i32);
    let bytes = surface.to_interleaved(rect);
    let mut pixmap = Pixmap::new(width, height)
        .ok_or_else(|| RasterError::Scene("failed to allocate Pixmap".into()))?;
    for (src_px, dst_px) in bytes.chunks_exact(4).zip(pixmap.pixels_mut()) {
        let a = src_px[3] as f32 / 255.0;
        let r = ((src_px[0] as f32 / 255.0) * a * 255.0).round() as u8;
        let g = ((src_px[1] as f32 / 255.0) * a * 255.0).round() as u8;
        let b = ((src_px[2] as f32 / 255.0) * a * 255.0).round() as u8;
        *dst_px = tiny_skia::PremultipliedColorU8::from_rgba(r, g, b, src_px[3])
            .unwrap_or(tiny_skia::PremultipliedColorU8::TRANSPARENT);
    }
    Ok(pixmap)
}

pub fn apply_photo_filter(
    pixmap: &mut Pixmap,
    params: &rust_motion_algo::FilterParams,
) -> Result<(), RasterError> {
    use rust_motion_geom::Rect;
    let w = pixmap.width();
    let h = pixmap.height();
    if w == 0 || h == 0 {
        return Ok(());
    }
    let rect = Rect::new(0, 0, w as i32, h as i32);
    let surf = pixmap_to_surface(pixmap);
    let out_surf = rust_motion_algo::apply_in(&surf, params, rect, rect, None, rect);
    let new_pixmap = surface_to_pixmap(&out_surf, w, h)?;
    pixmap.data_mut().copy_from_slice(new_pixmap.data());
    Ok(())
}

pub(crate) fn make_shadow(source: &Pixmap, shadow: &SceneShadow) -> Result<Pixmap, RasterError> {
    if !shadow.blur_sigma.is_finite() || !(0.0..=100.0).contains(&shadow.blur_sigma) {
        return Err(RasterError::Scene(format!(
            "shadow blur sigma must be finite and between 0 and 100, got {}",
            shadow.blur_sigma
        )));
    }
    if !shadow.offset_x.is_finite() || !shadow.offset_y.is_finite() {
        return Err(RasterError::Scene("shadow offsets must be finite".into()));
    }
    let mut output = Pixmap::new(source.width(), source.height())
        .ok_or_else(|| RasterError::Scene("failed to allocate shadow surface".into()))?;
    for (source_pixel, shadow_pixel) in source
        .data()
        .chunks_exact(4)
        .zip(output.data_mut().chunks_exact_mut(4))
    {
        let alpha = (u16::from(source_pixel[3]) * u16::from(shadow.color.a) / 255) as u8;
        shadow_pixel[0] = (u16::from(shadow.color.r) * u16::from(alpha) / 255) as u8;
        shadow_pixel[1] = (u16::from(shadow.color.g) * u16::from(alpha) / 255) as u8;
        shadow_pixel[2] = (u16::from(shadow.color.b) * u16::from(alpha) / 255) as u8;
        shadow_pixel[3] = alpha;
    }
    let radius = (shadow.blur_sigma * 1.5).ceil().clamp(0.0, 128.0) as usize;
    box_blur(&mut output, radius);
    Ok(output)
}

pub(crate) fn box_blur(pixmap: &mut Pixmap, radius: usize) {
    if radius == 0 {
        return;
    }
    let width = pixmap.width() as usize;
    let height = pixmap.height() as usize;
    let kernel = (radius * 2 + 1) as u32;
    let source = pixmap.data().to_vec();
    let mut horizontal = vec![0_u8; source.len()];

    horizontal
        .par_chunks_exact_mut(width * 4)
        .enumerate()
        .for_each(|(y, row)| {
            let mut sums = [0_u32; 4];
            for x in 0..=radius.min(width - 1) {
                let index = (y * width + x) * 4;
                for channel in 0..4 {
                    sums[channel] += u32::from(source[index + channel]);
                }
            }
            for x in 0..width {
                let offset = x * 4;
                for channel in 0..4 {
                    row[offset + channel] = (sums[channel] / kernel) as u8;
                }
                if x >= radius {
                    let remove = (y * width + x - radius) * 4;
                    for channel in 0..4 {
                        sums[channel] -= u32::from(source[remove + channel]);
                    }
                }
                if x + radius + 1 < width {
                    let add = (y * width + x + radius + 1) * 4;
                    for channel in 0..4 {
                        sums[channel] += u32::from(source[add + channel]);
                    }
                }
            }
        });

    let output = pixmap.data_mut();
    output
        .par_chunks_exact_mut(width * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..width {
                let mut sums = [0_u32; 4];
                let start_y = y.saturating_sub(radius);
                let end_y = y.saturating_add(radius).min(height - 1);
                for sample_y in start_y..=end_y {
                    let index = (sample_y * width + x) * 4;
                    for channel in 0..4 {
                        sums[channel] += u32::from(horizontal[index + channel]);
                    }
                }
                let offset = x * 4;
                for channel in 0..4 {
                    row[offset + channel] = (sums[channel] / kernel) as u8;
                }
                if y >= radius {
                    let remove = ((y - radius) * width + x) * 4;
                    for channel in 0..4 {
                        sums[channel] -= u32::from(horizontal[remove + channel]);
                    }
                }
                if y + radius + 1 < height {
                    let add = (y + radius + 1) * width + x;
                    for channel in 0..4 {
                        sums[channel] += u32::from(horizontal[add + channel]);
                    }
                }
            }
        });
}
