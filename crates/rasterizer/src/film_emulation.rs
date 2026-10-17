//! Procedural Film Emulation & Halation Synthesis Engine.
//!
//! Provides photorealistic analog film grain synthesis and red/orange halation glow
//! emulation as seen in 35mm / 16mm motion picture film stocks (e.g. Kodak Vision3).

use tiny_skia::{Pixmap, PremultipliedColorU8};

/// Integer hash noise function for procedural film grain generation.
#[inline]
fn hash_noise(x: f32, y: f32, seed: u32) -> f32 {
    let mut h = seed.wrapping_add((x as i32 as u32).wrapping_mul(0x9E3779B9));
    h = h.wrapping_add((y as i32 as u32).wrapping_mul(0x85EBCA6B));
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846CA68B);
    h ^= h >> 16;
    (h as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// Multi-octave grain noise with roughness.
#[inline]
fn sample_grain(gx: f32, gy: f32, roughness: f32, seed: u32) -> f32 {
    let n1 = hash_noise(gx, gy, seed);
    let n2 = hash_noise(gx * 2.13, gy * 2.13, seed ^ 0x55555555);
    n1 * (1.0 - roughness * 0.5) + n2 * (roughness * 0.5)
}

/// Apply procedural film grain and halation in-place on a `Pixmap`.
pub fn apply_film_emulation(
    pixmap: &mut Pixmap,
    grain_amount: f32,
    grain_size: f32,
    grain_roughness: f32,
    grain_seed: u32,
    halation_intensity: f32,
    halation_threshold: f32,
    halation_radius: f32,
    halation_color: [u8; 4],
) {
    let width = pixmap.width();
    let height = pixmap.height();
    if width == 0 || height == 0 {
        return;
    }

    // --- 1. Halation Synthesis ---
    if halation_intensity > 0.0 && halation_radius > 0.5 {
        apply_halation(
            pixmap,
            halation_intensity.clamp(0.0, 2.0),
            halation_threshold.clamp(0.0, 1.0),
            halation_radius.clamp(1.0, 100.0),
            halation_color,
        );
    }

    // --- 2. Procedural Film Grain Synthesis ---
    if grain_amount > 0.0 {
        let amt = (grain_amount / 100.0).clamp(0.0, 1.0) * 0.35;
        let scale = 1.0 / (0.6 + (grain_size / 100.0).clamp(0.0, 1.0) * 2.4);
        let rough = (grain_roughness / 100.0).clamp(0.0, 1.0);

        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) as usize;
                let px = pixmap.pixels_mut()[idx];
                let a = px.alpha();
                if a == 0 {
                    continue;
                }
                let inv_a = 1.0 / (a as f32);
                let r = px.red() as f32 * inv_a;
                let g = px.green() as f32 * inv_a;
                let b = px.blue() as f32 * inv_a;

                // Luminance
                let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                // Film grain density response: highest in midtones & quartertones, lower in extreme highlights
                let weight = (1.0 - (lum - 0.45).abs() * 1.6).clamp(0.1, 1.0);

                let g_val = sample_grain(x as f32 * scale, y as f32 * scale, rough, grain_seed);
                let delta = g_val * amt * weight * 255.0;

                let nr = (r * 255.0 + delta).clamp(0.0, 255.0);
                let ng = (g * 255.0 + delta).clamp(0.0, 255.0);
                let nb = (b * 255.0 + delta).clamp(0.0, 255.0);

                let pr = (nr * (a as f32 / 255.0)).round().min(255.0) as u8;
                let pg = (ng * (a as f32 / 255.0)).round().min(255.0) as u8;
                let pb = (nb * (a as f32 / 255.0)).round().min(255.0) as u8;

                if let Some(c) = PremultipliedColorU8::from_rgba(pr, pg, pb, a) {
                    pixmap.pixels_mut()[idx] = c;
                }
            }
        }
    }
}

/// Extract specular highlights, diffuse with a warm glow, and screen blend back onto the pixmap.
fn apply_halation(
    pixmap: &mut Pixmap,
    intensity: f32,
    threshold: f32,
    radius: f32,
    tint: [u8; 4],
) {
    let width = pixmap.width() as usize;
    let height = pixmap.height() as usize;

    // Build isolated highlight buffer
    let mut highlight = vec![0.0f32; width * height];
    for y in 0..height {
        for x in 0..width {
            let px = pixmap.pixels()[y * width + x];
            let a = px.alpha() as f32 / 255.0;
            if a > 0.0 {
                let r = (px.red() as f32 / 255.0) / a;
                let g = (px.green() as f32 / 255.0) / a;
                let b = (px.blue() as f32 / 255.0) / a;
                let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                if lum > threshold {
                    let over = (lum - threshold) / (1.0 - threshold).max(1e-4);
                    highlight[y * width + x] = over * a;
                }
            }
        }
    }

    // 2-pass box blur approximation of halation scatter
    let r_int = (radius.round() as usize).max(1);
    let mut blurred = vec![0.0f32; width * height];

    // Horizontal blur
    for y in 0..height {
        let row_start = y * width;
        let mut sum = 0.0f32;
        let mut count = 0usize;
        for x in 0..r_int.min(width) {
            sum += highlight[row_start + x];
            count += 1;
        }
        for x in 0..width {
            if x + r_int < width {
                sum += highlight[row_start + x + r_int];
                count += 1;
            }
            if x >= r_int + 1 {
                sum -= highlight[row_start + x - r_int - 1];
                count -= 1;
            }
            blurred[row_start + x] = sum / count.max(1) as f32;
        }
    }

    // Vertical blur into highlight buffer
    for x in 0..width {
        let mut sum = 0.0f32;
        let mut count = 0usize;
        for y in 0..r_int.min(height) {
            sum += blurred[y * width + x];
            count += 1;
        }
        for y in 0..height {
            if y + r_int < height {
                sum += blurred[(y + r_int) * width + x];
                count += 1;
            }
            if y >= r_int + 1 {
                sum -= blurred[(y - r_int - 1) * width + x];
                count -= 1;
            }
            highlight[y * width + x] = sum / count.max(1) as f32;
        }
    }

    // Tint and additive/screen blend onto original image
    let tr = tint[0] as f32 / 255.0;
    let tg = tint[1] as f32 / 255.0;
    let tb = tint[2] as f32 / 255.0;

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let h = highlight[idx] * intensity;
            if h <= 0.001 {
                continue;
            }
            let px = pixmap.pixels_mut()[idx];
            let a = px.alpha();
            if a == 0 {
                continue;
            }

            let r = px.red() as f32;
            let g = px.green() as f32;
            let b = px.blue() as f32;

            // Halation glow added proportionally
            let add_r = h * tr * 255.0 * (a as f32 / 255.0);
            let add_g = h * tg * 255.0 * (a as f32 / 255.0);
            let add_b = h * tb * 255.0 * (a as f32 / 255.0);

            let res_r = (r + add_r).min(255.0).round() as u8;
            let res_g = (g + add_g).min(255.0).round() as u8;
            let res_b = (b + add_b).min(255.0).round() as u8;

            if let Some(c) = PremultipliedColorU8::from_rgba(res_r, res_g, res_b, a) {
                pixmap.pixels_mut()[idx] = c;
            }
        }
    }
}
