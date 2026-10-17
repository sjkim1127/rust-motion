//! Lens Calibration & Optical Correction Engine.
//!
//! Applies radial distortion (barrel/pincushion), lateral chromatic aberration (TCA),
//! and vignetting falloff compensation based on lens profiles from `rust-motion-lensdb`.

use tiny_skia::{Pixmap, PremultipliedColorU8};

/// Apply radial lens distortion correction, TCA chromatic correction, and vignetting compensation.
pub fn apply_lens_correction(
    pixmap: &mut Pixmap,
    distortion: [f32; 4],
    vignette: [f32; 3],
    tca: [f32; 2],
    center: [f32; 2],
) {
    let width = pixmap.width();
    let height = pixmap.height();
    if width == 0 || height == 0 {
        return;
    }

    let has_warp = distortion.iter().any(|&k| k != 0.0) || tca[0] != 0.0 || tca[1] != 0.0;
    let has_vignette = vignette.iter().any(|&k| k != 0.0);

    if !has_warp && !has_vignette {
        return;
    }

    let w = width as f32;
    let h = height as f32;
    let max_dim = w.max(h);
    let half_diag = (w * w + h * h).sqrt() * 0.5;
    let norm_radius = half_diag / max_dim;

    let cx = center[0] * w;
    let cy = center[1] * h;

    let src_copy = pixmap.clone();

    // Bilinear sample helper from source pixmap
    let sample_bilinear = |sx: f32, sy: f32, channel: usize| -> f32 {
        let x0 = sx.floor() as i32;
        let y0 = sy.floor() as i32;
        let x1 = x0 + 1;
        let y1 = y0 + 1;

        let fx = sx - x0 as f32;
        let fy = sy - y0 as f32;

        let clamp_px = |x: i32, y: i32| -> (f32, f32) {
            let cx = x.clamp(0, width as i32 - 1) as u32;
            let cy = y.clamp(0, height as i32 - 1) as u32;
            let px = src_copy.pixel(cx, cy).unwrap_or(PremultipliedColorU8::TRANSPARENT);
            let a = px.alpha();
            if a == 0 {
                return (0.0, 0.0);
            }
            let inv_a = 1.0 / a as f32;
            let val = match channel {
                0 => px.red() as f32 * inv_a * 255.0,
                1 => px.green() as f32 * inv_a * 255.0,
                2 => px.blue() as f32 * inv_a * 255.0,
                _ => a as f32 / 255.0,
            };
            (val, a as f32 / 255.0)
        };

        let (p00, a00) = clamp_px(x0, y0);
        let (p10, a10) = clamp_px(x1, y0);
        let (p01, a01) = clamp_px(x0, y1);
        let (p11, a11) = clamp_px(x1, y1);

        let top = p00 + (p10 - p00) * fx;
        let bottom = p01 + (p11 - p01) * fx;
        let val = top + (bottom - top) * fy;

        if channel == 3 {
            let atop = a00 + (a10 - a00) * fx;
            let abottom = a01 + (a11 - a01) * fx;
            atop + (abottom - atop) * fy
        } else {
            val
        }
    };

    let k_dist = distortion;
    let [kr_scale, kb_scale] = [1.0 + tca[0], 1.0 + tca[1]];

    for y in 0..height {
        for x in 0..width {
            let dx = (x as f32 - cx) / (norm_radius * max_dim);
            let dy = (y as f32 - cy) / (norm_radius * max_dim);
            let r2 = dx * dx + dy * dy;
            let _r = r2.sqrt();

            // 1. Distortion mapping
            // Poly4 radial displacement: f(r) = 1 + k0*r^2 + k1*r^4 + k2*r^6
            let warp_f = 1.0 + r2 * (k_dist[0] + r2 * (k_dist[1] + r2 * k_dist[2]));

            // Channel coordinates with TCA
            let sx_g = cx + dx * warp_f * (norm_radius * max_dim);
            let sy_g = cy + dy * warp_f * (norm_radius * max_dim);

            let sx_r = cx + dx * warp_f * kr_scale * (norm_radius * max_dim);
            let sy_r = cy + dy * warp_f * kr_scale * (norm_radius * max_dim);

            let sx_b = cx + dx * warp_f * kb_scale * (norm_radius * max_dim);
            let sy_b = cy + dy * warp_f * kb_scale * (norm_radius * max_dim);

            let mut red = sample_bilinear(sx_r, sy_r, 0);
            let mut green = sample_bilinear(sx_g, sy_g, 1);
            let mut blue = sample_bilinear(sx_b, sy_b, 2);
            let alpha = sample_bilinear(sx_g, sy_g, 3);

            // 2. Vignetting compensation
            // Gain factor: 1 + k1*r^2 + k2*r^4 + k3*r^6
            if has_vignette {
                let vig_gain = 1.0 + r2 * (vignette[0] + r2 * (vignette[1] + r2 * vignette[2]));
                red *= vig_gain;
                green *= vig_gain;
                blue *= vig_gain;
            }

            let a_u8 = (alpha * 255.0).clamp(0.0, 255.0).round() as u8;
            let a_f = a_u8 as f32 / 255.0;

            let pr = (red.clamp(0.0, 255.0) * a_f).round().min(255.0) as u8;
            let pg = (green.clamp(0.0, 255.0) * a_f).round().min(255.0) as u8;
            let pb = (blue.clamp(0.0, 255.0) * a_f).round().min(255.0) as u8;

            if let Some(c) = PremultipliedColorU8::from_rgba(pr, pg, pb, a_u8) {
                pixmap.pixels_mut()[(y * width + x) as usize] = c;
            }
        }
    }
}
