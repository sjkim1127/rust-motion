//! Brown-Conrady Lens Calibration & Optical Aberration Correction Engine.
//!
//! Implements:
//! - Radial distortion: $r' = r(1 + k_1 r^2 + k_2 r^4 + k_3 r^6)$
//! - Tangential decentering distortion:
//!   $\Delta x = 2 p_1 x y + p_2(r^2 + 2x^2)$, $\Delta y = p_1(r^2 + 2y^2) + 2 p_2 x y$
//! - Lateral chromatic aberration (TCA) per-channel R/G/B polynomial radial scaling
//! - Optical vignetting compensation: $\cos^4(\theta)$ natural falloff + 6th-order polynomial

use serde::{Deserialize, Serialize};
use tiny_skia::{Pixmap, PremultipliedColorU8};

/// Full Brown-Conrady optical calibration model parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrownConradyModel {
    /// Radial distortion coefficients `[k1, k2, k3]`.
    pub radial_k: [f32; 3],
    /// Tangential decentering coefficients `[p1, p2]`.
    pub tangential_p: [f32; 2],
    /// Lateral chromatic aberration `[kr_shift, kb_shift]` (relative to green channel).
    pub tca_shift: [f32; 2],
    /// Vignette polynomial coefficients `[v1, v2, v3]` and natural $\cos^4(\theta)$ weight.
    pub vignette_poly: [f32; 3],
    /// Natural $\cos^4(\theta)$ vignetting compensation strength (`0.0..=1.0`).
    pub cos4_vignette_strength: f32,
    /// Normalized optical center `[cx, cy]` in `0.0..=1.0` (default `[0.5, 0.5]`).
    pub optical_center: [f32; 2],
}

impl Default for BrownConradyModel {
    fn default() -> Self {
        Self {
            radial_k: [0.0, 0.0, 0.0],
            tangential_p: [0.0, 0.0],
            tca_shift: [0.0, 0.0],
            vignette_poly: [0.0, 0.0, 0.0],
            cos4_vignette_strength: 0.0,
            optical_center: [0.5, 0.5],
        }
    }
}

/// Apply full Brown-Conrady radial + tangential distortion, TCA, and $\cos^4(\theta)$ + polynomial vignetting.
pub fn apply_brown_conrady_correction(pixmap: &mut Pixmap, model: &BrownConradyModel) {
    let width = pixmap.width();
    let height = pixmap.height();
    if width == 0 || height == 0 {
        return;
    }

    let has_warp = model.radial_k.iter().any(|&k| k != 0.0)
        || model.tangential_p.iter().any(|&p| p != 0.0)
        || model.tca_shift.iter().any(|&t| t != 0.0);
    let has_vignette =
        model.vignette_poly.iter().any(|&v| v != 0.0) || model.cos4_vignette_strength > 0.0;

    if !has_warp && !has_vignette {
        return;
    }

    let w = width as f32;
    let h = height as f32;
    let max_dim = w.max(h);
    let half_diag = (w * w + h * h).sqrt() * 0.5;
    let norm_radius = half_diag / max_dim;
    let inv_scale = 1.0 / (norm_radius * max_dim);
    let scale = norm_radius * max_dim;

    let cx = model.optical_center[0] * w;
    let cy = model.optical_center[1] * h;

    let src_copy = pixmap.clone();

    let sample_bilinear = |sx: f32, sy: f32, channel: usize| -> f32 {
        let x0 = sx.floor() as i32;
        let y0 = sy.floor() as i32;
        let x1 = x0 + 1;
        let y1 = y0 + 1;

        let fx = sx - x0 as f32;
        let fy = sy - y0 as f32;

        let clamp_px = |x: i32, y: i32| -> (f32, f32) {
            let cx_i = x.clamp(0, width as i32 - 1) as u32;
            let cy_i = y.clamp(0, height as i32 - 1) as u32;
            let px = src_copy
                .pixel(cx_i, cy_i)
                .unwrap_or(PremultipliedColorU8::TRANSPARENT);
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

    let [k1, k2, k3] = model.radial_k;
    let [p1, p2] = model.tangential_p;
    let [kr_scale, kb_scale] = [1.0 + model.tca_shift[0], 1.0 + model.tca_shift[1]];
    let [v1, v2, v3] = model.vignette_poly;

    for y in 0..height {
        for x in 0..width {
            let xn = (x as f32 - cx) * inv_scale;
            let yn = (y as f32 - cy) * inv_scale;
            let r2 = xn * xn + yn * yn;

            // Brown-Conrady radial factor: 1 + k1*r^2 + k2*r^4 + k3*r^6
            let radial = 1.0 + r2 * (k1 + r2 * (k2 + r2 * k3));

            // Brown-Conrady tangential decentering displacement
            let tx = 2.0 * p1 * xn * yn + p2 * (r2 + 2.0 * xn * xn);
            let ty = p1 * (r2 + 2.0 * yn * yn) + 2.0 * p2 * xn * yn;

            let x_dist = xn * radial + tx;
            let y_dist = yn * radial + ty;

            // Per-channel TCA scaling
            let sx_g = cx + x_dist * scale;
            let sy_g = cy + y_dist * scale;

            let sx_r = cx + x_dist * kr_scale * scale;
            let sy_r = cy + y_dist * kr_scale * scale;

            let sx_b = cx + x_dist * kb_scale * scale;
            let sy_b = cy + y_dist * kb_scale * scale;

            let mut red = sample_bilinear(sx_r, sy_r, 0);
            let mut green = sample_bilinear(sx_g, sy_g, 1);
            let mut blue = sample_bilinear(sx_b, sy_b, 2);
            let alpha = sample_bilinear(sx_g, sy_g, 3);

            // Vignetting polynomial + cos^4(theta) law
            if has_vignette {
                let poly_gain = (1.0 + r2 * (v1 + r2 * (v2 + r2 * v3))).max(0.0);
                // cos(theta) = 1 / sqrt(1 + r^2) -> 1 / cos^4(theta) = (1 + r^2)^2
                let cos4_gain =
                    1.0 + model.cos4_vignette_strength * ((1.0 + r2 * 0.35).powi(2) - 1.0);
                let vig_gain = poly_gain * cos4_gain;
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

/// Apply radial lens distortion correction, TCA chromatic correction, and vignetting compensation.
/// Note: `distortion[3]` can optionally supply tangential $p_1$ decentering.
pub fn apply_lens_correction(
    pixmap: &mut Pixmap,
    distortion: [f32; 4],
    vignette: [f32; 3],
    tca: [f32; 2],
    center: [f32; 2],
) {
    let model = BrownConradyModel {
        radial_k: [distortion[0], distortion[1], distortion[2]],
        tangential_p: [distortion[3], 0.0],
        tca_shift: tca,
        vignette_poly: vignette,
        cos4_vignette_strength: 0.0,
        optical_center: center,
    };
    apply_brown_conrady_correction(pixmap, &model);
}
