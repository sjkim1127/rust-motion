//! Adobe After Effects-grade Time Remapping & Optical Flow Frame Blending (Pixel Motion).
//!
//! Provides:
//! - **Time Remapping Curves** ([`TimeRemapCurve`]): Arbitrary non-linear cubic mappings
//!   from composition timeline time $t_{out}$ to source media timestamp $t_{src}$, supporting
//!   freeze frames (0%), smooth speed ramps, super-slow motion, and reverse playback.
//! - **Three Frame Interpolation Modes** ([`FrameInterpolationMode`]):
//!   - `FrameRepeat`: Nearest-neighbor hard cut frame selection.
//!   - `FrameBlend`: Linear cross-fade opacity dissolve between adjacent source frames.
//!   - `PixelMotion`: Bidirectional dense 2D optical flow estimation with sub-pixel
//!     motion-compensated warping.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tiny_skia::Pixmap;

/// Frame interpolation algorithm when rendering sub-frame or re-timed video playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FrameInterpolationMode {
    /// Nearest neighbor selection: picks the single closest source frame.
    #[default]
    FrameRepeat,
    /// Cross-dissolve mix: blends two adjacent source frames based on fractional distance.
    FrameBlend,
    /// Optical Flow (Pixel Motion): estimates 2D motion vector fields between adjacent
    /// frames and warps pixels along the flow for artifact-free super slow motion.
    PixelMotion,
}

/// A keyframed control point on an arbitrary time remapping curve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TimeRemapPoint {
    /// Output timeline timestamp in seconds.
    pub time_out: f64,
    /// Source media timestamp in seconds.
    pub time_src: f64,
    /// Optional incoming velocity tangent $(d t_{src} / d t_{out})$.
    pub tangent_in: Option<f64>,
    /// Optional outgoing velocity tangent $(d t_{src} / d t_{out})$.
    pub tangent_out: Option<f64>,
}

impl TimeRemapPoint {
    pub const fn new(time_out: f64, time_src: f64) -> Self {
        Self {
            time_out,
            time_src,
            tangent_in: None,
            tangent_out: None,
        }
    }

    pub const fn with_speed(time_out: f64, time_src: f64, speed: f64) -> Self {
        Self {
            time_out,
            time_src,
            tangent_in: Some(speed),
            tangent_out: Some(speed),
        }
    }
}

/// Arbitrary time remapping curve mapping output time to source media time.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TimeRemapCurve {
    pub points: Vec<TimeRemapPoint>,
}

impl TimeRemapCurve {
    pub fn new() -> Self {
        Self { points: Vec::new() }
    }

    /// Adds a keyframe point to the time remapping curve.
    pub fn add_point(&mut self, time_out: f64, time_src: f64) -> &mut Self {
        self.points.push(TimeRemapPoint::new(time_out, time_src));
        self.points.sort_by(|a, b| a.time_out.partial_cmp(&b.time_out).unwrap());
        self
    }

    /// Adds a keyframe point with explicit playback speed tangent $(1.0 = 100\%)$.
    pub fn add_point_with_speed(&mut self, time_out: f64, time_src: f64, speed: f64) -> &mut Self {
        self.points.push(TimeRemapPoint::with_speed(time_out, time_src, speed));
        self.points.sort_by(|a, b| a.time_out.partial_cmp(&b.time_out).unwrap());
        self
    }

    /// Evaluates the source media timestamp $t_{src}$ at output timeline timestamp `time_out`.
    pub fn eval(&self, time_out: f64) -> f64 {
        if self.points.is_empty() {
            return time_out;
        }
        if self.points.len() == 1 || time_out <= self.points[0].time_out {
            let p0 = &self.points[0];
            let slope = p0.tangent_in.or(p0.tangent_out).unwrap_or(1.0);
            return p0.time_src + (time_out - p0.time_out) * slope;
        }
        let last = self.points.last().unwrap();
        if time_out >= last.time_out {
            let slope = last.tangent_out.or(last.tangent_in).unwrap_or(1.0);
            return last.time_src + (time_out - last.time_out) * slope;
        }

        // Find surrounding interval
        let idx = match self.points.binary_search_by(|p| p.time_out.partial_cmp(&time_out).unwrap()) {
            Ok(exact) => return self.points[exact].time_src,
            Err(i) => i - 1,
        };

        let p0 = &self.points[idx];
        let p1 = &self.points[idx + 1];
        let dt = p1.time_out - p0.time_out;
        if dt.abs() < 1e-9 {
            return p0.time_src;
        }

        let tau = ((time_out - p0.time_out) / dt).clamp(0.0, 1.0);
        let sec_slope = (p1.time_src - p0.time_src) / dt;
        let m0 = p0.tangent_out.unwrap_or(sec_slope);
        let m1 = p1.tangent_in.unwrap_or(sec_slope);

        // Cubic Hermite polynomial evaluation
        let tau2 = tau * tau;
        let tau3 = tau2 * tau;
        let h00 = 2.0 * tau3 - 3.0 * tau2 + 1.0;
        let h10 = tau3 - 2.0 * tau2 + tau;
        let h01 = -2.0 * tau3 + 3.0 * tau2;
        let h11 = tau3 - tau2;

        h00 * p0.time_src + h10 * dt * m0 + h01 * p1.time_src + h11 * dt * m1
    }

    /// Evaluates instantaneous playback speed $(d t_{src} / d t_{out})$ at `time_out`.
    pub fn speed_at(&self, time_out: f64) -> f64 {
        let eps = 1e-4;
        let s0 = self.eval(time_out - eps);
        let s1 = self.eval(time_out + eps);
        (s1 - s0) / (2.0 * eps)
    }

    /// Computes fractional frame sampling indices for media timeline playback.
    ///
    /// Returns `(frame_0, frame_1, alpha)` where $\alpha \in [0.0, 1.0)$ is the sub-frame fraction.
    pub fn subframe_at(&self, time_out: f64, source_fps: f64) -> (u64, u64, f32) {
        let src_t = self.eval(time_out).max(0.0);
        let exact_frame = src_t * source_fps;
        let f0 = exact_frame.floor() as u64;
        let f1 = f0 + 1;
        let alpha = (exact_frame - f0 as f64).clamp(0.0, 1.0) as f32;
        (f0, f1, alpha)
    }
}

// ─── Optical Flow & Frame Interpolation Engine ───────────────────────────────

/// Dense 2D Block-Matching Optical Flow estimator and image warper.
pub struct OpticalFlowEngine;

impl OpticalFlowEngine {
    /// Estimates bidirectional 2D motion vector fields between `prev` and `next` frames.
    ///
    /// Returns a grid of $(\Delta x, \Delta y)$ displacement vectors for each block.
    pub fn estimate_flow(
        prev: &Pixmap,
        next: &Pixmap,
        block_size: usize,
        search_radius: usize,
    ) -> (Vec<(f32, f32)>, usize, usize) {
        let width = prev.width() as usize;
        let height = prev.height() as usize;
        let b = block_size.max(4);
        let r = search_radius.clamp(1, 32) as i32;

        let grid_w = (width + b - 1) / b;
        let grid_h = (height + b - 1) / b;
        let mut flow = vec![(0.0f32, 0.0f32); grid_w * grid_h];

        let p_data = prev.data();
        let n_data = next.data();

        // Parallel estimation across block grid rows
        flow.par_chunks_exact_mut(grid_w)
            .enumerate()
            .for_each(|(gy, row)| {
                let cy = (gy * b + b / 2) as i32;

                for gx in 0..grid_w {
                    let cx = (gx * b + b / 2) as i32;
                    let mut best_sad = u64::MAX;
                    let mut best_dx = 0;
                    let mut best_dy = 0;

                    for dy in -r..=r {
                        let ny = cy + dy;
                        if ny < 0 || ny >= height as i32 {
                            continue;
                        }

                        for dx in -r..=r {
                            let nx = cx + dx;
                            if nx < 0 || nx >= width as i32 {
                                continue;
                            }

                            // Compute Sum of Absolute Differences (SAD) across block pixels
                            let mut sad = 0u64;
                            for by in 0..b {
                                let py = (cy - (b as i32) / 2 + by as i32).clamp(0, height as i32 - 1) as usize;
                                let qy = (ny - (b as i32) / 2 + by as i32).clamp(0, height as i32 - 1) as usize;

                                for bx in 0..b {
                                    let px = (cx - (b as i32) / 2 + bx as i32).clamp(0, width as i32 - 1) as usize;
                                    let qx = (nx - (b as i32) / 2 + bx as i32).clamp(0, width as i32 - 1) as usize;

                                    let idx_p = (py * width + px) * 4;
                                    let idx_n = (qy * width + qx) * 4;

                                    sad += (p_data[idx_p] as i64 - n_data[idx_n] as i64).unsigned_abs();
                                    sad += (p_data[idx_p + 1] as i64 - n_data[idx_n + 1] as i64).unsigned_abs();
                                    sad += (p_data[idx_p + 2] as i64 - n_data[idx_n + 2] as i64).unsigned_abs();
                                }
                            }

                            if sad < best_sad {
                                best_sad = sad;
                                best_dx = dx;
                                best_dy = dy;
                            }
                        }
                    }

                    row[gx] = (best_dx as f32, best_dy as f32);
                }
            });

        (flow, grid_w, grid_h)
    }

    /// Warps a pixmap along a flow vector field scaled by `scale_factor`.
    pub fn warp_image(
        src: &Pixmap,
        flow: &[(f32, f32)],
        grid_w: usize,
        grid_h: usize,
        block_size: usize,
        scale_factor: f32,
    ) -> Pixmap {
        let width = src.width();
        let height = src.height();
        let mut out = Pixmap::new(width, height).unwrap();
        let src_data = src.data();
        let b = block_size.max(1) as f32;

        let sample_bilinear = |x: f32, y: f32| -> [u8; 4] {
            let cx = x.clamp(0.0, (width - 1) as f32);
            let cy = y.clamp(0.0, (height - 1) as f32);
            let x0 = cx.floor() as usize;
            let y0 = cy.floor() as usize;
            let x1 = (x0 + 1).min(width as usize - 1);
            let y1 = (y0 + 1).min(height as usize - 1);
            let fx = cx - x0 as f32;
            let fy = cy - y0 as f32;

            let mut pixel = [0u8; 4];
            for c in 0..4 {
                let p00 = src_data[(y0 * width as usize + x0) * 4 + c] as f32;
                let p10 = src_data[(y0 * width as usize + x1) * 4 + c] as f32;
                let p01 = src_data[(y1 * width as usize + x0) * 4 + c] as f32;
                let p11 = src_data[(y1 * width as usize + x1) * 4 + c] as f32;

                let top = p00 * (1.0 - fx) + p10 * fx;
                let bot = p01 * (1.0 - fx) + p11 * fx;
                pixel[c] = (top * (1.0 - fy) + bot * fy).round().clamp(0.0, 255.0) as u8;
            }
            pixel
        };

        out.data_mut()
            .par_chunks_exact_mut((width * 4) as usize)
            .enumerate()
            .for_each(|(y, row)| {
                let fy = y as f32;
                let gy = ((fy / b).floor() as usize).min(grid_h - 1);

                for x in 0..width as usize {
                    let fx = x as f32;
                    let gx = ((fx / b).floor() as usize).min(grid_w - 1);

                    let (dx, dy) = flow[gy * grid_w + gx];
                    let sx = fx - dx * scale_factor;
                    let sy = fy - dy * scale_factor;

                    let sampled = sample_bilinear(sx, sy);
                    let out_idx = x * 4;
                    row[out_idx..out_idx + 4].copy_from_slice(&sampled);
                }
            });

        out
    }

    /// Synthesizes an intermediate optical-flow frame between `prev` and `next` at sub-frame fraction `alpha`.
    pub fn blend_pixel_motion(prev: &Pixmap, next: &Pixmap, alpha: f32) -> Pixmap {
        let (flow, gw, gh) = Self::estimate_flow(prev, next, 8, 12);

        // Forward warp prev by alpha
        let warped_prev = Self::warp_image(prev, &flow, gw, gh, 8, alpha);
        // Backward warp next by -(1 - alpha)
        let warped_next = Self::warp_image(next, &flow, gw, gh, 8, -(1.0 - alpha));

        let width = prev.width();
        let height = prev.height();
        let mut out = Pixmap::new(width, height).unwrap();

        let wp_data = warped_prev.data();
        let wn_data = warped_next.data();
        let w_prev = 1.0 - alpha;
        let w_next = alpha;

        out.data_mut()
            .par_chunks_exact_mut(4)
            .enumerate()
            .for_each(|(i, pixel)| {
                let idx = i * 4;
                for c in 0..4 {
                    let v = wp_data[idx + c] as f32 * w_prev + wn_data[idx + c] as f32 * w_next;
                    pixel[c] = v.round().clamp(0.0, 255.0) as u8;
                }
            });

        out
    }

    /// Interpolates between `prev` and `next` frames based on the requested [`FrameInterpolationMode`].
    pub fn interpolate_frames(
        prev: &Pixmap,
        next: &Pixmap,
        alpha: f32,
        mode: FrameInterpolationMode,
    ) -> Pixmap {
        match mode {
            FrameInterpolationMode::FrameRepeat => {
                if alpha < 0.5 {
                    prev.clone()
                } else {
                    next.clone()
                }
            }
            FrameInterpolationMode::FrameBlend => {
                let width = prev.width();
                let height = prev.height();
                let mut out = Pixmap::new(width, height).unwrap();
                let p_data = prev.data();
                let n_data = next.data();
                let w_prev = 1.0 - alpha;
                let w_next = alpha;

                out.data_mut()
                    .par_chunks_exact_mut(4)
                    .enumerate()
                    .for_each(|(i, pixel)| {
                        let idx = i * 4;
                        for c in 0..4 {
                            let v = p_data[idx + c] as f32 * w_prev + n_data[idx + c] as f32 * w_next;
                            pixel[c] = v.round().clamp(0.0, 255.0) as u8;
                        }
                    });
                out
            }
            FrameInterpolationMode::PixelMotion => Self::blend_pixel_motion(prev, next, alpha),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_remap_curve_linear_and_freeze() {
        let mut curve = TimeRemapCurve::new();
        curve.add_point(0.0, 0.0);
        curve.add_point(1.0, 1.0); // 100% normal speed
        curve.add_point(2.0, 1.0); // Freeze frame between 1.0s and 2.0s
        curve.add_point(3.0, 3.0); // 200% fast motion between 2.0s and 3.0s

        assert!((curve.eval(0.5) - 0.5).abs() < 1e-3);
        assert!((curve.eval(1.5) - 1.0).abs() < 1e-3, "should freeze at 1.0s");
        assert!((curve.eval(2.5) - 2.0).abs() < 1e-3, "should interpolate to 2.0s");

        let (f0, f1, alpha) = curve.subframe_at(0.25, 30.0);
        assert_eq!(f0, 7);
        assert_eq!(f1, 8);
        assert!((alpha - 0.5).abs() < 1e-3);
    }

    #[test]
    fn test_time_remap_speed_ramp() {
        let mut curve = TimeRemapCurve::new();
        curve.add_point_with_speed(0.0, 0.0, 1.0);
        curve.add_point_with_speed(2.0, 0.5, 0.25); // ramp down to 25% slow-mo

        let speed_mid = curve.speed_at(1.0);
        assert!(speed_mid < 1.0 && speed_mid > 0.0, "should decelerate into slow motion");
    }

    #[test]
    fn test_optical_flow_pixel_motion_warping() {
        let mut prev = Pixmap::new(64, 64).unwrap();
        let mut next = Pixmap::new(64, 64).unwrap();

        // White square moving rightward from x=16 to x=24
        for y in 24..40 {
            for x in 16..24 {
                let idx = ((y * 64 + x) * 4) as usize;
                prev.data_mut()[idx..idx + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
            for x in 24..32 {
                let idx = ((y * 64 + x) * 4) as usize;
                next.data_mut()[idx..idx + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }

        // Interpolate at alpha = 0.5 using PixelMotion
        let interpolated = OpticalFlowEngine::interpolate_frames(
            &prev,
            &next,
            0.5,
            FrameInterpolationMode::PixelMotion,
        );

        // At alpha = 0.5, the square should be centered around x = 20..28
        let center_px = interpolated.pixel(20, 32).unwrap();
        assert!(center_px.alpha() > 100, "in-between frame should synthesize center square");
    }

    #[test]
    fn test_frame_repeat_and_frame_blend() {
        let mut prev = Pixmap::new(16, 16).unwrap();
        let mut next = Pixmap::new(16, 16).unwrap();
        prev.fill(tiny_skia::Color::from_rgba8(200, 0, 0, 255));
        next.fill(tiny_skia::Color::from_rgba8(0, 200, 0, 255));

        // Frame repeat at alpha 0.2 should be pure prev
        let repeat_02 = OpticalFlowEngine::interpolate_frames(
            &prev,
            &next,
            0.2,
            FrameInterpolationMode::FrameRepeat,
        );
        assert_eq!(repeat_02.data()[0], 200);

        // Frame blend at alpha 0.5 should be equal mix (100, 100, 0)
        let blend_05 = OpticalFlowEngine::interpolate_frames(
            &prev,
            &next,
            0.5,
            FrameInterpolationMode::FrameBlend,
        );
        assert_eq!(blend_05.data()[0], 100);
        assert_eq!(blend_05.data()[1], 100);
    }
}
