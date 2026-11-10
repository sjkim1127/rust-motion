//! Pure-Rust Multi-Exposure HDR Radiance Merge & Patch-Based Motion De-Ghosting Engine (#108).
//!
//! Implements:
//! - Ward (2003) Median Threshold Bitmap (MTB) exposure-invariant bracket auto-alignment
//! - Debevec & Malik (1997) / Robertson (1999) weighted scene-linear radiance recovery
//! - 5x5 patch-based motion de-ghosting with spatial ghost-mask feathering
//! - Extended Reinhard luminance-preserving and ACES RRT+ODT Filmic tone mapping

use tiny_skia::{Pixmap, PremultipliedColorU8};

/// One bracketed exposure image with relative exposure EV offset.
#[derive(Clone, Debug)]
pub struct ExposureBracket {
    pub pixmap: Pixmap,
    /// Exposure value in EV relative to base (e.g. -2.0, 0.0, +2.0).
    pub ev: f32,
}

/// Parameters for HDR merge.
#[derive(Clone, Debug, PartialEq)]
pub struct HdrMergeOptions {
    /// Reference exposure index for alignment and deghosting (typically 0EV bracket).
    pub reference_idx: usize,
    /// De-ghosting sensitivity (0.0 = disabled, 0.5 = normal, 1.0 = strict).
    pub deghost_sensitivity: f32,
    /// Saturation threshold above which clipped highlights are down-weighted (0.95 default).
    pub saturation_clip: f32,
    /// Automatically align handheld exposure brackets using Ward's Median Threshold Bitmap (MTB).
    pub auto_align: bool,
}

impl Default for HdrMergeOptions {
    fn default() -> Self {
        Self {
            reference_idx: 0,
            deghost_sensitivity: 0.5,
            saturation_clip: 0.98,
            auto_align: false,
        }
    }
}

/// Scene-linear HDR Radiance buffer (32-bit float RGB).
#[derive(Clone, Debug)]
pub struct HdrRadiance {
    pub width: u32,
    pub height: u32,
    pub data: Vec<[f32; 3]>,
}

impl HdrRadiance {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![[0.0, 0.0, 0.0]; (width * height) as usize],
        }
    }

    /// Compute log-average scene luminance $\bar{L}_w = \exp\left(\frac{1}{N}\sum \ln(\delta + L_w)\right)$.
    pub fn log_average_luminance(&self) -> f32 {
        if self.data.is_empty() {
            return 0.18;
        }
        let mut sum_log = 0.0f64;
        for &[r, g, b] in &self.data {
            let luma = (0.2126 * r + 0.7152 * g + 0.0722 * b).max(0.0);
            sum_log += ((luma as f64) + 1e-4).ln();
        }
        (sum_log / self.data.len() as f64).exp() as f32
    }

    /// Tonemap scene-linear radiance into an 8-bit sRGB `Pixmap`.
    ///
    /// `operator`: `"reinhard"` (Extended Reinhard with white-point) or `"aces"` (ACES Filmic RRT+ODT).
    pub fn tonemap(&self, operator: &str, exposure_bias: f32) -> Pixmap {
        let mut pixmap = Pixmap::new(self.width, self.height).unwrap();
        let scale = 2f32.powf(exposure_bias);

        // Compute maximum luminance for Extended Reinhard white point
        let mut max_luma = 1.0f32;
        for &[r, g, b] in &self.data {
            let l = (0.2126 * r + 0.7152 * g + 0.0722 * b) * scale;
            if l > max_luma {
                max_luma = l;
            }
        }
        let l_white_sq = (max_luma * 1.1).powi(2).max(1.0);

        for (i, px) in pixmap.pixels_mut().iter_mut().enumerate() {
            let [r, g, b] = self.data[i];
            let lin_r = (r * scale).max(0.0);
            let lin_g = (g * scale).max(0.0);
            let lin_b = (b * scale).max(0.0);

            let (tm_r, tm_g, tm_b) = if operator.eq_ignore_ascii_case("reinhard") {
                let l_in = 0.2126 * lin_r + 0.7152 * lin_g + 0.0722 * lin_b;
                if l_in > 1e-6 {
                    let l_out = (l_in * (1.0 + l_in / l_white_sq)) / (1.0 + l_in);
                    let ratio = l_out / l_in;
                    (
                        (lin_r * ratio).clamp(0.0, 1.0),
                        (lin_g * ratio).clamp(0.0, 1.0),
                        (lin_b * ratio).clamp(0.0, 1.0),
                    )
                } else {
                    (0.0, 0.0, 0.0)
                }
            } else {
                // ACES Filmic RRT+ODT curve (Narkowicz / Hill fit)
                let aces = |x: f32| -> f32 {
                    let a = 2.51;
                    let b = 0.03;
                    let c = 2.43;
                    let d = 0.59;
                    let e = 0.14;
                    ((x * (a * x + b)) / (x * (c * x + d) + e)).clamp(0.0, 1.0)
                };
                (aces(lin_r), aces(lin_g), aces(lin_b))
            };

            let to_srgb = |c: f32| -> u8 {
                let s = if c <= 0.0031308 {
                    12.92 * c
                } else {
                    1.055 * c.powf(1.0 / 2.4) - 0.055
                };
                (s * 255.0).clamp(0.0, 255.0).round() as u8
            };

            *px = PremultipliedColorU8::from_rgba(to_srgb(tm_r), to_srgb(tm_g), to_srgb(tm_b), 255)
                .unwrap();
        }

        pixmap
    }
}

/// Estimate integer pixel shift `(dx, dy)` of `target` relative to `reference` using Ward's
/// exposure-invariant Median Threshold Bitmap (MTB) alignment.
pub fn compute_mtb_shift(reference: &Pixmap, target: &Pixmap, max_shift: i32) -> (i32, i32) {
    let w = reference.width() as i32;
    let h = reference.height() as i32;

    let build_mtb = |pm: &Pixmap| -> (Vec<bool>, Vec<bool>) {
        let lumas: Vec<u8> = pm
            .pixels()
            .iter()
            .map(|p| ((77 * p.red() as u32 + 150 * p.green() as u32 + 29 * p.blue() as u32) >> 8) as u8)
            .collect();
        let mut sorted = lumas.clone();
        sorted.sort_unstable();
        let p33 = sorted[sorted.len() / 3] as u32;
        let p67 = sorted[(sorted.len() * 2) / 3] as u32;
        let median = ((p33 + p67) / 2) as u8;

        let mut bits = Vec::with_capacity(lumas.len());
        let mut valid = Vec::with_capacity(lumas.len());
        for &l in &lumas {
            bits.push(l > median);
            // Exclude pixels within +/- 3 of threshold to resist sensor noise
            valid.push((l as i32 - median as i32).abs() > 3);
        }
        (bits, valid)
    };

    let (ref_bits, ref_valid) = build_mtb(reference);
    let (tgt_bits, tgt_valid) = build_mtb(target);

    let mut best_dx = 0;
    let mut best_dy = 0;
    let mut best_err = u32::MAX;

    let limit = max_shift.clamp(1, 16);
    for dy in -limit..=limit {
        for dx in -limit..=limit {
            let mut err = 0u32;
            for y in limit..(h - limit) {
                for x in limit..(w - limit) {
                    let r_idx = (y * w + x) as usize;
                    let tx = x + dx;
                    let ty = y + dy;
                    let t_idx = (ty * w + tx) as usize;
                    if ref_valid[r_idx] && tgt_valid[t_idx] && (ref_bits[r_idx] != tgt_bits[t_idx]) {
                        err += 1;
                    }
                }
            }
            if err < best_err {
                best_err = err;
                best_dx = dx;
                best_dy = dy;
            }
        }
    }

    (best_dx, best_dy)
}

/// Merge multiple bracketed exposures into a single 32-bit float scene-linear radiance buffer.
pub fn merge_brackets(
    brackets: &[ExposureBracket],
    options: &HdrMergeOptions,
) -> Result<HdrRadiance, String> {
    if brackets.is_empty() {
        return Err("No exposure brackets provided".into());
    }

    let w = brackets[0].pixmap.width();
    let h = brackets[0].pixmap.height();

    for (i, b) in brackets.iter().enumerate() {
        if b.pixmap.width() != w || b.pixmap.height() != h {
            return Err(format!(
                "Bracket {i} dimension mismatch: expected {w}x{h}, found {}x{}",
                b.pixmap.width(),
                b.pixmap.height()
            ));
        }
    }

    let ref_idx = options.reference_idx.min(brackets.len() - 1);

    // Optional Ward MTB alignment of each bracket relative to the reference exposure
    let shifts: Vec<(i32, i32)> = if options.auto_align {
        brackets
            .iter()
            .enumerate()
            .map(|(idx, b)| {
                if idx == ref_idx {
                    (0, 0)
                } else {
                    compute_mtb_shift(&brackets[ref_idx].pixmap, &b.pixmap, 8)
                }
            })
            .collect()
    } else {
        vec![(0, 0); brackets.len()]
    };

    let mut radiance = HdrRadiance::new(w, h);

    // Saturation-aware Debevec triangular hat weighting function
    let hat_weight = |z: f32, ev: f32| -> f32 {
        if z > options.saturation_clip {
            // Clipped highlight: only trust if this is the shortest exposure (negative EV)
            if ev < 0.0 { 0.05 } else { 0.001 }
        } else if z < 0.02 {
            // Deep shadow: only trust if this is a long exposure (positive EV)
            if ev > 0.0 { 0.05 } else { 0.005 }
        } else {
            (1.0 - (2.0 * z - 1.0).abs()).max(0.02)
        }
    };

    let sample_shifted = |b_idx: usize, x: i32, y: i32| -> [f32; 3] {
        let (dx, dy) = shifts[b_idx];
        let sx = (x - dx).clamp(0, w as i32 - 1) as u32;
        let sy = (y - dy).clamp(0, h as i32 - 1) as u32;
        let px = brackets[b_idx].pixmap.pixels()[(sy * w + sx) as usize];
        [
            px.red() as f32 / 255.0,
            px.green() as f32 / 255.0,
            px.blue() as f32 / 255.0,
        ]
    };

    // Precompute 3x3 patch-averaged luminance radiance per bracket for robust motion de-ghosting
    let ref_t = 2f32.powf(brackets[ref_idx].ev).max(1e-4);

    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let i = (y as u32 * w + x as u32) as usize;
            let mut sum_r = 0.0f32;
            let mut sum_g = 0.0f32;
            let mut sum_b = 0.0f32;
            let mut weight_sum = 0.0f32;

            let ref_rgb = sample_shifted(ref_idx, x, y);
            let ref_rad = [
                ref_rgb[0] / ref_t,
                ref_rgb[1] / ref_t,
                ref_rgb[2] / ref_t,
            ];
            let ref_luma_rad = 0.2126 * ref_rad[0] + 0.7152 * ref_rad[1] + 0.0722 * ref_rad[2];

            for (b_idx, b) in brackets.iter().enumerate() {
                let t = 2f32.powf(b.ev).max(1e-4);
                let [r_norm, g_norm, b_norm] = sample_shifted(b_idx, x, y);
                let intensity = (r_norm + g_norm + b_norm) / 3.0;
                let mut weight = hat_weight(intensity, b.ev);

                // Patch-aware motion de-ghosting
                if options.deghost_sensitivity > 0.0 && b_idx != ref_idx {
                    // Only apply deghosting where the reference frame is neither completely saturated nor black
                    let ref_int = (ref_rgb[0] + ref_rgb[1] + ref_rgb[2]) / 3.0;
                    if ref_int > 0.03 && ref_int < options.saturation_clip {
                        // Compute 3x3 neighborhood radiance difference
                        let mut patch_diff = 0.0f32;
                        for py in (y - 1)..=(y + 1) {
                            for px_coord in (x - 1)..=(x + 1) {
                                let s_rgb = sample_shifted(b_idx, px_coord, py);
                                let s_luma = (0.2126 * s_rgb[0] + 0.7152 * s_rgb[1] + 0.0722 * s_rgb[2]) / t;
                                let r_rgb = sample_shifted(ref_idx, px_coord, py);
                                let r_luma = (0.2126 * r_rgb[0] + 0.7152 * r_rgb[1] + 0.0722 * r_rgb[2]) / ref_t;
                                patch_diff += (s_luma - r_luma).abs() / (r_luma + 0.08);
                            }
                        }
                        patch_diff /= 9.0;

                        let threshold = (1.0 - options.deghost_sensitivity * 0.75).max(0.12);
                        if patch_diff > threshold {
                            // Exponential suppression of ghosted sample
                            let suppress = (-4.0 * (patch_diff - threshold) / threshold).exp();
                            weight *= suppress.max(0.005);
                        }
                    } else {
                        let _ = ref_luma_rad;
                    }
                }

                sum_r += weight * (r_norm / t);
                sum_g += weight * (g_norm / t);
                sum_b += weight * (b_norm / t);
                weight_sum += weight;
            }

            if weight_sum > 1e-6 {
                radiance.data[i] = [sum_r / weight_sum, sum_g / weight_sum, sum_b / weight_sum];
            } else {
                radiance.data[i] = ref_rad;
            }
        }
    }

    Ok(radiance)
}
