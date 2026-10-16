//! Advanced After Effects Visual Effects (VFX) Stack.
//!
//! Provides:
//! - **Optical Glow**: High-dynamic-range highlight extraction with multi-pass
//!   exponential falloff pyramid, anamorphic stretch, tinting, and Screen/Add compositing.
//! - **Displacement Map**: Sub-pixel geometric distortion using red, green, blue, alpha,
//!   or luminance channels of a texture or procedural ripple.
//! - **Fractal Noise Generator**: Multi-octave coherent procedural noise (Perlin, Simplex,
//!   Cellular/Worley) with sub-pixel temporal evolution and Basic, Turbulent, and Stringy fractals.

use crate::backend::RasterError;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tiny_skia::Pixmap;

// ─── Optical Glow ─────────────────────────────────────────────────────────────

/// Blend mode for compositing the optical glow over the original surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GlowBlendMode {
    /// Additive blending: $C = \min(255, C_{src} + C_{glow})$
    #[default]
    Add,
    /// Screen blending: $C = 255 - \frac{(255 - C_{src})(255 - C_{glow})}{255}$
    Screen,
}

/// Optical Glow settings matching high-end motion graphics glow plugins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpticalGlow {
    /// Luminance threshold (0.0 to 1.0). Pixels above this brightness contribute to glow. Default `0.5`.
    pub threshold: f32,
    /// Base glow blur radius in pixels. Default `20.0`.
    pub radius: f32,
    /// Multiplier for glow brightness. Default `1.5`.
    pub intensity: f32,
    /// Anamorphic aspect ratio (horizontal streak multiplier). `1.0` = circular, `2.0` = 2x wide anamorphic streak.
    pub anamorphic_ratio: f32,
    /// Optional RGBA tint color applied to the glow.
    pub tint: Option<[u8; 4]>,
    /// Compositing blend mode: Add or Screen.
    pub blend_mode: GlowBlendMode,
}

impl Default for OpticalGlow {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            radius: 20.0,
            intensity: 1.5,
            anamorphic_ratio: 1.0,
            tint: None,
            blend_mode: GlowBlendMode::Add,
        }
    }
}

/// Applies Optical Glow to a pixmap in-place.
pub fn apply_optical_glow(pixmap: &mut Pixmap, glow: &OpticalGlow) -> Result<(), RasterError> {
    if !glow.threshold.is_finite() || !glow.radius.is_finite() || !glow.intensity.is_finite() {
        return Err(RasterError::Scene("Optical glow parameters must be finite".into()));
    }
    let threshold = glow.threshold.clamp(0.0, 0.999);
    let radius = glow.radius.clamp(1.0, 200.0);
    let intensity = glow.intensity.max(0.0);
    let anamorphic = glow.anamorphic_ratio.clamp(0.1, 10.0);

    let width = pixmap.width() as usize;
    let height = pixmap.height() as usize;
    let total_pixels = width * height;
    if total_pixels == 0 {
        return Ok(());
    }

    let src = pixmap.data().to_vec();

    // Step 1: Extract bright highlights above luminance threshold
    let mut highlight = vec![0.0f32; total_pixels * 4];
    let thresh_255 = threshold * 255.0;
    let knee_span = 255.0 - thresh_255;

    for i in 0..total_pixels {
        let idx = i * 4;
        let a = src[idx + 3] as f32;
        if a <= 1.0 {
            continue;
        }
        let r = src[idx] as f32;
        let g = src[idx + 1] as f32;
        let b = src[idx + 2] as f32;

        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        if luma > thresh_255 {
            let factor = ((luma - thresh_255) / knee_span).clamp(0.0, 1.0);
            highlight[idx] = r * factor;
            highlight[idx + 1] = g * factor;
            highlight[idx + 2] = b * factor;
            highlight[idx + 3] = a * factor;
        }
    }

    // Step 2: Multi-pass exponential blur pyramid (4 octaves)
    // Octave 0: r, weight 1.0
    // Octave 1: 2r, weight 0.5
    // Octave 2: 4r, weight 0.25
    // Octave 3: 8r, weight 0.125
    let mut accumulated_glow = vec![0.0f32; total_pixels * 4];
    let octaves = [
        (1.0f32, 1.0f32),
        (2.0f32, 0.5f32),
        (4.0f32, 0.25f32),
        (8.0f32, 0.125f32),
    ];

    for &(scale, weight) in &octaves {
        let rx = ((radius * scale * anamorphic).round() as usize).clamp(1, width.saturating_sub(1));
        let ry = ((radius * scale / anamorphic).round() as usize).clamp(1, height.saturating_sub(1));

        let blurred = separable_box_blur_f32(&highlight, width, height, rx, ry);
        for i in 0..accumulated_glow.len() {
            accumulated_glow[i] += blurred[i] * weight;
        }
    }

    // Step 3: Apply tint if specified
    if let Some(tint_color) = glow.tint {
        let tr = tint_color[0] as f32 / 255.0;
        let tg = tint_color[1] as f32 / 255.0;
        let tb = tint_color[2] as f32 / 255.0;
        let ta = tint_color[3] as f32 / 255.0;

        for i in 0..total_pixels {
            let idx = i * 4;
            accumulated_glow[idx] *= tr * ta;
            accumulated_glow[idx + 1] *= tg * ta;
            accumulated_glow[idx + 2] *= tb * ta;
            accumulated_glow[idx + 3] *= ta;
        }
    }

    // Step 4: Blend onto destination (Add or Screen)
    let dst = pixmap.data_mut();
    match glow.blend_mode {
        GlowBlendMode::Add => {
            dst.par_chunks_exact_mut(4)
                .zip(accumulated_glow.par_chunks_exact(4))
                .for_each(|(pixel, glow_px)| {
                    let r = pixel[0] as f32 + glow_px[0] * intensity;
                    let g = pixel[1] as f32 + glow_px[1] * intensity;
                    let b = pixel[2] as f32 + glow_px[2] * intensity;
                    let a = pixel[3] as f32 + glow_px[3] * intensity;

                    pixel[0] = r.round().clamp(0.0, 255.0) as u8;
                    pixel[1] = g.round().clamp(0.0, 255.0) as u8;
                    pixel[2] = b.round().clamp(0.0, 255.0) as u8;
                    pixel[3] = a.round().clamp(0.0, 255.0) as u8;
                });
        }
        GlowBlendMode::Screen => {
            dst.par_chunks_exact_mut(4)
                .zip(accumulated_glow.par_chunks_exact(4))
                .for_each(|(pixel, glow_px)| {
                    let gr = (glow_px[0] * intensity).clamp(0.0, 255.0);
                    let gg = (glow_px[1] * intensity).clamp(0.0, 255.0);
                    let gb = (glow_px[2] * intensity).clamp(0.0, 255.0);

                    let r = 255.0 - (255.0 - pixel[0] as f32) * (255.0 - gr) / 255.0;
                    let g = 255.0 - (255.0 - pixel[1] as f32) * (255.0 - gg) / 255.0;
                    let b = 255.0 - (255.0 - pixel[2] as f32) * (255.0 - gb) / 255.0;
                    let a = (pixel[3] as f32 + glow_px[3] * intensity).clamp(0.0, 255.0);

                    pixel[0] = r.round().clamp(0.0, 255.0) as u8;
                    pixel[1] = g.round().clamp(0.0, 255.0) as u8;
                    pixel[2] = b.round().clamp(0.0, 255.0) as u8;
                    pixel[3] = a.round().clamp(0.0, 255.0) as u8;
                });
        }
    }

    Ok(())
}

fn separable_box_blur_f32(
    src: &[f32],
    width: usize,
    height: usize,
    rx: usize,
    ry: usize,
) -> Vec<f32> {
    let mut temp = vec![0.0f32; src.len()];
    let kx = (rx * 2 + 1) as f32;

    // Horizontal pass
    temp.par_chunks_exact_mut(width * 4)
        .enumerate()
        .for_each(|(y, row)| {
            let row_offset = y * width * 4;
            let mut sum = [0.0f32; 4];

            // Initial accumulator window
            for x in 0..=rx.min(width - 1) {
                let idx = row_offset + x * 4;
                for c in 0..4 {
                    sum[c] += src[idx + c];
                }
            }

            for x in 0..width {
                let out_idx = x * 4;
                for c in 0..4 {
                    row[out_idx + c] = sum[c] / kx;
                }
                if x >= rx {
                    let remove_idx = row_offset + (x - rx) * 4;
                    for c in 0..4 {
                        sum[c] -= src[remove_idx + c];
                    }
                }
                if x + rx + 1 < width {
                    let add_idx = row_offset + (x + rx + 1) * 4;
                    for c in 0..4 {
                        sum[c] += src[add_idx + c];
                    }
                }
            }
        });

    let mut out = vec![0.0f32; src.len()];
    let ky = (ry * 2 + 1) as f32;

    // Vertical pass
    out.par_chunks_exact_mut(width * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..width {
                let mut sum = [0.0f32; 4];
                let y_start = y.saturating_sub(ry);
                let y_end = (y + ry).min(height - 1);
                for sy in y_start..=y_end {
                    let idx = (sy * width + x) * 4;
                    for c in 0..4 {
                        sum[c] += temp[idx + c];
                    }
                }
                let out_idx = x * 4;
                for c in 0..4 {
                    row[out_idx + c] = sum[c] / ky;
                }
            }
        });

    out
}

// ─── Displacement Map ─────────────────────────────────────────────────────────

/// Channel selector for driving geometric displacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DisplacementChannel {
    #[default]
    Red,
    Green,
    Blue,
    Alpha,
    Luminance,
}

/// Boundary wrap mode when displaced coordinates fall outside the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DisplacementWrap {
    #[default]
    Clamp,
    Wrap,
}

/// Displacement Map parameters matching After Effects Displacement Map effect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplacementMap {
    /// Texture channel driving horizontal X shift.
    pub channel_x: DisplacementChannel,
    /// Texture channel driving vertical Y shift.
    pub channel_y: DisplacementChannel,
    /// Maximum horizontal shift in pixels. Default `20.0`.
    pub max_horizontal: f32,
    /// Maximum vertical shift in pixels. Default `20.0`.
    pub max_vertical: f32,
    /// Wrap mode for coordinates falling outside borders.
    pub wrap_mode: DisplacementWrap,
    /// Optional displacement texture pixel bytes (RGBA8). If None, generates procedural ripples.
    pub map_source: Option<Vec<u8>>,
    /// Width of displacement map image.
    pub map_width: u32,
    /// Height of displacement map image.
    pub map_height: u32,
}

impl Default for DisplacementMap {
    fn default() -> Self {
        Self {
            channel_x: DisplacementChannel::Red,
            channel_y: DisplacementChannel::Green,
            max_horizontal: 20.0,
            max_vertical: 20.0,
            wrap_mode: DisplacementWrap::Clamp,
            map_source: None,
            map_width: 0,
            map_height: 0,
        }
    }
}

/// Applies Displacement Map warping to a pixmap in-place.
pub fn apply_displacement_map(pixmap: &mut Pixmap, map: &DisplacementMap) -> Result<(), RasterError> {
    if !map.max_horizontal.is_finite() || !map.max_vertical.is_finite() {
        return Err(RasterError::Scene("Displacement parameters must be finite".into()));
    }

    let width = pixmap.width();
    let height = pixmap.height();
    if width == 0 || height == 0 {
        return Ok(());
    }

    let src = pixmap.data().to_vec();
    let map_w = if map.map_width > 0 { map.map_width } else { width };
    let map_h = if map.map_height > 0 { map.map_height } else { height };

    let sample_channel = |raw: &[u8], ch: DisplacementChannel, idx: usize| -> f32 {
        let r = raw[idx] as f32 / 255.0;
        let g = raw[idx + 1] as f32 / 255.0;
        let b = raw[idx + 2] as f32 / 255.0;
        let a = raw[idx + 3] as f32 / 255.0;
        match ch {
            DisplacementChannel::Red => r,
            DisplacementChannel::Green => g,
            DisplacementChannel::Blue => b,
            DisplacementChannel::Alpha => a,
            DisplacementChannel::Luminance => 0.2126 * r + 0.7152 * g + 0.0722 * b,
        }
    };

    let sample_bilinear_src = |x: f32, y: f32| -> [u8; 4] {
        let (sx, sy) = match map.wrap_mode {
            DisplacementWrap::Clamp => (
                x.clamp(0.0, (width - 1) as f32),
                y.clamp(0.0, (height - 1) as f32),
            ),
            DisplacementWrap::Wrap => {
                let w = width as f32;
                let h = height as f32;
                let wx = x.rem_euclid(w);
                let wy = y.rem_euclid(h);
                (wx, wy)
            }
        };

        let x0 = sx.floor() as u32;
        let y0 = sy.floor() as u32;
        let x1 = (x0 + 1).min(width - 1);
        let y1 = (y0 + 1).min(height - 1);
        let fx = sx - x0 as f32;
        let fy = sy - y0 as f32;

        let mut out = [0u8; 4];
        for c in 0..4 {
            let p00 = src[((y0 * width + x0) * 4) as usize + c] as f32;
            let p10 = src[((y0 * width + x1) * 4) as usize + c] as f32;
            let p01 = src[((y1 * width + x0) * 4) as usize + c] as f32;
            let p11 = src[((y1 * width + x1) * 4) as usize + c] as f32;

            let top = p00 * (1.0 - fx) + p10 * fx;
            let bot = p01 * (1.0 - fx) + p11 * fx;
            let val = top * (1.0 - fy) + bot * fy;
            out[c] = val.round().clamp(0.0, 255.0) as u8;
        }
        out
    };

    let dst = pixmap.data_mut();

    // Parallel displacement mapping per scanline
    dst.par_chunks_exact_mut((width * 4) as usize)
        .enumerate()
        .for_each(|(y, row)| {
            let fy = y as f32;
            let v = fy / height as f32;

            for x in 0..width {
                let fx = x as f32;
                let u = fx / width as f32;

                // Obtain displacement vector (vx, vy) where 0.5 is neutral 0 offset
                let (val_x, val_y) = if let Some(ref map_bytes) = map.map_source {
                    let mx = ((u * map_w as f32) as u32).min(map_w - 1);
                    let my = ((v * map_h as f32) as u32).min(map_h - 1);
                    let midx = ((my * map_w + mx) * 4) as usize;
                    if midx + 3 < map_bytes.len() {
                        let vx = sample_channel(map_bytes, map.channel_x, midx);
                        let vy = sample_channel(map_bytes, map.channel_y, midx);
                        (vx, vy)
                    } else {
                        (0.5, 0.5)
                    }
                } else {
                    // Procedural wave ripples if no map provided
                    let wave_x = (u * 20.0 + v * 10.0).sin() * 0.5 + 0.5;
                    let wave_y = (v * 20.0 + u * 10.0).cos() * 0.5 + 0.5;
                    (wave_x, wave_y)
                };

                let offset_x = (val_x - 0.5) * 2.0 * map.max_horizontal;
                let offset_y = (val_y - 0.5) * 2.0 * map.max_vertical;

                let sampled = sample_bilinear_src(fx + offset_x, fy + offset_y);
                let out_idx = (x * 4) as usize;
                row[out_idx..out_idx + 4].copy_from_slice(&sampled);
            }
        });

    Ok(())
}

// ─── Fractal Noise Generator ──────────────────────────────────────────────────

/// Underlying noise algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FractalNoiseType {
    /// Classic Perlin gradient noise.
    Perlin,
    /// Higher performance simplex lattice noise.
    #[default]
    Simplex,
    /// Cellular / Worley distance noise for organic voronoi textures.
    Cellular,
}

/// Fractal accumulation style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FractalType {
    /// Standard smooth fBm accumulation.
    #[default]
    Basic,
    /// Absolute value billowy turbulence: $\sum |\text{noise}|$.
    Turbulent,
    /// Inverted ridge stringy turbulence: $\sum (1 - |\text{noise}|)$.
    Stringy,
}

/// Fractal Noise generator parameters matching Adobe After Effects Fractal Noise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FractalNoise {
    pub noise_type: FractalNoiseType,
    pub fractal_type: FractalType,
    /// Contrast multiplier (1.0 = normal, >1.0 = high contrast). Default `1.0`.
    pub contrast: f32,
    /// Brightness offset (-1.0 to 1.0). Default `0.0`.
    pub brightness: f32,
    /// Number of harmonic octaves (1 to 8). Default `4`.
    pub complexity: u32,
    /// Horizontal scale in pixels. Default `100.0`.
    pub scale_x: f32,
    /// Vertical scale in pixels. Default `100.0`.
    pub scale_y: f32,
    /// Temporal evolution angle in radians. Controls organic morphing without sliding.
    pub evolution: f32,
    /// Random seed. Default `42`.
    pub seed: u64,
}

impl Default for FractalNoise {
    fn default() -> Self {
        Self {
            noise_type: FractalNoiseType::Simplex,
            fractal_type: FractalType::Basic,
            contrast: 1.0,
            brightness: 0.0,
            complexity: 4,
            scale_x: 100.0,
            scale_y: 100.0,
            evolution: 0.0,
            seed: 42,
        }
    }
}

/// Generates a grayscale/luminance Pixmap of fractal noise.
pub fn generate_fractal_noise_pixmap(width: u32, height: u32, config: &FractalNoise) -> Pixmap {
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).unwrap();
    let octaves = config.complexity.clamp(1, 8);
    let scale_x = config.scale_x.max(1.0);
    let scale_y = config.scale_y.max(1.0);
    let evolution = config.evolution;
    let contrast = config.contrast.max(0.0);
    let brightness = config.brightness;
    let seed = config.seed;

    pixmap
        .data_mut()
        .par_chunks_exact_mut((width * 4) as usize)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..width {
                let mut sum = 0.0f32;
                let mut amp = 1.0f32;
                let mut freq = 1.0f32;
                let mut max_amp = 0.0f32;

                for oct in 0..octaves {
                    let nx = (x as f32 / scale_x) * freq + (seed as f32 * 0.173);
                    let ny = (y as f32 / scale_y) * freq + (seed as f32 * 0.311);
                    let nz = evolution * 0.5 + (oct as f32 * 1.618);

                    let raw_n = match config.noise_type {
                        FractalNoiseType::Perlin => perlin_3d(nx, ny, nz),
                        FractalNoiseType::Simplex => simplex_3d(nx, ny, nz),
                        FractalNoiseType::Cellular => cellular_3d(nx, ny, nz),
                    };

                    let sample = match config.fractal_type {
                        FractalType::Basic => raw_n,
                        FractalType::Turbulent => raw_n.abs() * 2.0 - 1.0,
                        FractalType::Stringy => (1.0 - raw_n.abs()) * 2.0 - 1.0,
                    };

                    sum += sample * amp;
                    max_amp += amp;
                    amp *= 0.5;
                    freq *= 2.0;
                }

                let normalized = sum / max_amp.max(1e-4); // [-1, 1]
                let val = (normalized * 0.5 + 0.5) * contrast + brightness;
                let byte = (val * 255.0).round().clamp(0.0, 255.0) as u8;

                let out_idx = (x * 4) as usize;
                row[out_idx] = byte;
                row[out_idx + 1] = byte;
                row[out_idx + 2] = byte;
                row[out_idx + 3] = 255;
            }
        });

    pixmap
}

// ─── Procedural Noise Mathematics ─────────────────────────────────────────────

fn hash_u32(n: u32) -> u32 {
    let mut x = n;
    x = ((x >> 16) ^ x).wrapping_mul(0x45d9f3b);
    x = ((x >> 16) ^ x).wrapping_mul(0x45d9f3b);
    (x >> 16) ^ x
}

fn hash_vec3(x: i32, y: i32, z: i32) -> f32 {
    let n = (x.wrapping_mul(73856093)) ^ (y.wrapping_mul(19349663)) ^ (z.wrapping_mul(83492791));
    (hash_u32(n as u32) & 0x00ffffff) as f32 / 16777215.0
}

fn cellular_3d(x: f32, y: f32, z: f32) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let zi = z.floor() as i32;
    let mut min_dist = 10.0f32;

    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let cx = xi + dx;
                let cy = yi + dy;
                let cz = zi + dz;

                let px = cx as f32 + hash_vec3(cx, cy, cz);
                let py = cy as f32 + hash_vec3(cx + 17, cy + 31, cz + 43);
                let pz = cz as f32 + hash_vec3(cx + 59, cy + 71, cz + 89);

                let dist = ((x - px).powi(2) + (y - py).powi(2) + (z - pz).powi(2)).sqrt();
                if dist < min_dist {
                    min_dist = dist;
                }
            }
        }
    }

    (min_dist.min(1.4) / 1.4) * 2.0 - 1.0
}

fn perlin_3d(x: f32, y: f32, z: f32) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let zi = z.floor() as i32;

    let xf = x - x.floor();
    let yf = y - y.floor();
    let zf = z - z.floor();

    let u = xf * xf * (3.0 - 2.0 * xf);
    let v = yf * yf * (3.0 - 2.0 * yf);
    let w = zf * zf * (3.0 - 2.0 * zf);

    let grad = |ix: i32, iy: i32, iz: i32, fx: f32, fy: f32, fz: f32| -> f32 {
        let h = (hash_vec3(ix, iy, iz) * 16.0) as i32;
        let u = if h < 8 { fx } else { fy };
        let v = if h < 4 { fy } else if h == 12 || h == 14 { fx } else { fz };
        (if (h & 1) == 0 { u } else { -u }) + (if (h & 2) == 0 { v } else { -v })
    };

    let c000 = grad(xi, yi, zi, xf, yf, zf);
    let c100 = grad(xi + 1, yi, zi, xf - 1.0, yf, zf);
    let c010 = grad(xi, yi + 1, zi, xf, yf - 1.0, zf);
    let c110 = grad(xi + 1, yi + 1, zi, xf - 1.0, yf - 1.0, zf);

    let c001 = grad(xi, yi, zi + 1, xf, yf, zf - 1.0);
    let c101 = grad(xi + 1, yi, zi + 1, xf - 1.0, yf, zf - 1.0);
    let c011 = grad(xi, yi + 1, zi + 1, xf, yf - 1.0, zf - 1.0);
    let c111 = grad(xi + 1, yi + 1, zi + 1, xf - 1.0, yf - 1.0, zf - 1.0);

    let x00 = c000 * (1.0 - u) + c100 * u;
    let x10 = c010 * (1.0 - u) + c110 * u;
    let x01 = c001 * (1.0 - u) + c101 * u;
    let x11 = c011 * (1.0 - u) + c111 * u;

    let y0 = x00 * (1.0 - v) + x10 * v;
    let y1 = x01 * (1.0 - v) + x11 * v;

    (y0 * (1.0 - w) + y1 * w).clamp(-1.0, 1.0)
}

fn simplex_3d(x: f32, y: f32, z: f32) -> f32 {
    let s = (x + y + z) * (1.0 / 3.0);
    let i = (x + s).floor() as i32;
    let j = (y + s).floor() as i32;
    let k = (z + s).floor() as i32;

    let g = (i + j + k) as f32 * (1.0 / 6.0);
    let x0 = x - (i as f32 - g);
    let y0 = y - (j as f32 - g);
    let z0 = z - (k as f32 - g);

    let (i1, j1, k1, i2, j2, k2) = if x0 >= y0 {
        if y0 >= z0 {
            (1, 0, 0, 1, 1, 0)
        } else if x0 >= z0 {
            (1, 0, 0, 1, 0, 1)
        } else {
            (0, 0, 1, 1, 0, 1)
        }
    } else if y0 < z0 {
        (0, 0, 1, 0, 1, 1)
    } else if x0 < z0 {
        (0, 1, 0, 0, 1, 1)
    } else {
        (0, 1, 0, 1, 1, 0)
    };

    let x1 = x0 - i1 as f32 + (1.0 / 6.0);
    let y1 = y0 - j1 as f32 + (1.0 / 6.0);
    let z1 = z0 - k1 as f32 + (1.0 / 6.0);

    let x2 = x0 - i2 as f32 + (2.0 / 6.0);
    let y2 = y0 - j2 as f32 + (2.0 / 6.0);
    let z2 = z0 - k2 as f32 + (2.0 / 6.0);

    let x3 = x0 - 1.0 + (3.0 / 6.0);
    let y3 = y0 - 1.0 + (3.0 / 6.0);
    let z3 = z0 - 1.0 + (3.0 / 6.0);

    let contrib = |ix: i32, iy: i32, iz: i32, px: f32, py: f32, pz: f32| -> f32 {
        let t = 0.6 - px * px - py * py - pz * pz;
        if t <= 0.0 {
            0.0
        } else {
            let t2 = t * t;
            let h = (hash_vec3(ix, iy, iz) * 12.0) as i32;
            let u = if h < 4 { px } else if h == 12 || h == 13 { px } else { py };
            let v = if h < 2 { py } else if h == 12 || h == 13 { pz } else { pz };
            let grad = (if (h & 1) == 0 { u } else { -u }) + (if (h & 2) == 0 { v } else { -v });
            t2 * t2 * grad
        }
    };

    let n0 = contrib(i, j, k, x0, y0, z0);
    let n1 = contrib(i + i1, j + j1, k + k1, x1, y1, z1);
    let n2 = contrib(i + i2, j + j2, k + k2, x2, y2, z2);
    let n3 = contrib(i + 1, j + 1, k + 1, x3, y3, z3);

    ((n0 + n1 + n2 + n3) * 32.0).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optical_glow_enhances_bright_regions() {
        let mut pixmap = Pixmap::new(64, 64).unwrap();
        // Draw a bright square in the center
        for y in 24..40 {
            for x in 24..40 {
                let idx = ((y * 64 + x) * 4) as usize;
                pixmap.data_mut()[idx..idx + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }

        let glow = OpticalGlow {
            threshold: 0.6,
            radius: 10.0,
            intensity: 2.0,
            anamorphic_ratio: 1.0,
            tint: None,
            blend_mode: GlowBlendMode::Add,
        };

        apply_optical_glow(&mut pixmap, &glow).unwrap();

        // Pixel outside the original square (e.g. x=20, y=32) should now have glow luminance
        let idx_glow = ((32 * 64 + 20) * 4) as usize;
        assert!(
            pixmap.data()[idx_glow] > 0,
            "glow halo should extend beyond the bright core"
        );
    }

    #[test]
    fn test_displacement_map_shifts_pixels() {
        let mut pixmap = Pixmap::new(64, 64).unwrap();
        // Solid vertical split: left half black, right half white
        for y in 0..64 {
            for x in 32..64 {
                let idx = ((y * 64 + x) * 4) as usize;
                pixmap.data_mut()[idx..idx + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }

        // Displacement map that pushes pixels rightward (val = 1.0)
        let map_data = vec![255u8; 64 * 64 * 4];
        let disp = DisplacementMap {
            channel_x: DisplacementChannel::Red,
            channel_y: DisplacementChannel::Green,
            max_horizontal: 10.0,
            max_vertical: 0.0,
            wrap_mode: DisplacementWrap::Clamp,
            map_source: Some(map_data),
            map_width: 64,
            map_height: 64,
        };

        apply_displacement_map(&mut pixmap, &disp).unwrap();

        // At x = 25 (which was black before), displaced pixel should sample from x = 25 + 10 = 35 (white)
        let idx = ((32 * 64 + 25) * 4) as usize;
        assert_eq!(pixmap.data()[idx], 255, "pixel at x=25 should have shifted from x=35");
    }

    #[test]
    fn test_fractal_noise_generation() {
        let config = FractalNoise {
            noise_type: FractalNoiseType::Simplex,
            fractal_type: FractalType::Basic,
            contrast: 1.0,
            brightness: 0.0,
            complexity: 4,
            scale_x: 20.0,
            scale_y: 20.0,
            evolution: 0.0,
            seed: 42,
        };

        let noise_pm = generate_fractal_noise_pixmap(64, 64, &config);
        assert_eq!(noise_pm.width(), 64);
        assert_eq!(noise_pm.height(), 64);

        // Check variance across pixels (it should not be uniform)
        let data = noise_pm.data();
        let p0 = data[0];
        let p_half = data[((32 * 64 + 32) * 4) as usize];
        assert_ne!(p0, p_half, "fractal noise should vary spatially");
    }
}
