//! Adobe After Effects-grade Camera Shutter & Subframe Motion Blur Engine.
//!
//! Provides:
//! - Camera shutter physics simulation ([`ShutterConfig`]) with `shutter_angle` and `shutter_phase`.
//! - Weighted temporal filter kernels ([`TemporalFilterKernel`]: Box, Triangle, Gaussian)
//!   to eliminate high-velocity strobing artifacts.
//! - Subframe temporal time and weight distribution ([`subframe_time_samples`]).
//! - Multi-sample subframe accumulation ([`accumulate_subframes`]).
//! - 2D Directional motion blur filter ([`apply_directional_motion_blur`]) along arbitrary angles.

use crate::backend::RasterError;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tiny_skia::Pixmap;

/// Weighting kernel used to filter subframe samples across the shutter exposure window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TemporalFilterKernel {
    /// Uniform box filter: all subframes receive equal weight (standard After Effects).
    #[default]
    Box,
    /// Bartlett / triangular filter: weights peak linearly at the shutter center.
    Triangle,
    /// Gaussian filter: bell-curve weighting for ultra-smooth cinematic motion trails.
    Gaussian,
}

/// Camera shutter parameters governing motion blur exposure.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShutterConfig {
    /// Shutter opening angle in degrees (0.0 to 360.0).
    ///
    /// - `180.0°` = standard cinema half-frame exposure (e.g. 1/60s at 30fps).
    /// - `360.0°` = full-frame maximum exposure.
    /// - `0.0°` = shutter closed (no motion blur).
    pub shutter_angle: f32,

    /// Shutter phase offset in degrees relative to the current frame timestamp.
    ///
    /// - `-90.0°` = default After Effects behavior (exposure centered evenly around frame).
    /// - `0.0°` = trailing blur (exposure starts at frame).
    /// - `-180.0°` = leading blur (exposure ends at frame).
    pub shutter_phase: f32,

    /// Number of subframe temporal samples to accumulate (typically 8 to 32).
    pub samples: u32,

    /// Temporal weighting kernel.
    pub kernel: TemporalFilterKernel,
}

impl Default for ShutterConfig {
    fn default() -> Self {
        Self {
            shutter_angle: 180.0,
            shutter_phase: -90.0,
            samples: 16,
            kernel: TemporalFilterKernel::Box,
        }
    }
}

impl ShutterConfig {
    /// Creates a new camera shutter configuration.
    pub fn new(shutter_angle: f32, shutter_phase: f32, samples: u32, kernel: TemporalFilterKernel) -> Self {
        Self {
            shutter_angle: shutter_angle.clamp(0.0, 360.0),
            shutter_phase,
            samples: samples.max(1),
            kernel,
        }
    }

    /// Standard cinematic 180° shutter centered at current frame (-90° phase).
    pub fn cinematic_180(samples: u32) -> Self {
        Self::new(180.0, -90.0, samples, TemporalFilterKernel::Box)
    }

    /// Smooth Gaussian shutter eliminating strobing on rapid movements.
    pub fn smooth_gaussian(samples: u32) -> Self {
        Self::new(180.0, -90.0, samples, TemporalFilterKernel::Gaussian)
    }
}

/// Generates discrete `(sample_time_sec, weight)` pairs for a frame according to shutter physics.
///
/// Weights are normalized so that $\sum w_i = 1.0$.
pub fn subframe_time_samples(
    frame: f64,
    fps: f64,
    config: &ShutterConfig,
) -> Vec<(f64, f32)> {
    let n = config.samples.max(1) as usize;
    let valid_fps = if fps > 0.0 { fps } else { 30.0 };
    let frame_duration = 1.0 / valid_fps;

    let exposure_duration = (config.shutter_angle.clamp(0.0, 360.0) as f64 / 360.0) * frame_duration;
    let phase_offset = (config.shutter_phase as f64 / 360.0) * frame_duration;

    let base_time = (frame / valid_fps) + phase_offset;

    if n == 1 || exposure_duration <= 1e-9 {
        return vec![(base_time + 0.5 * exposure_duration, 1.0)];
    }

    let mut raw_weights = Vec::with_capacity(n);
    let mut times = Vec::with_capacity(n);

    for i in 0..n {
        let u = i as f64 / (n - 1) as f64; // in [0.0, 1.0]
        let t = base_time + u * exposure_duration;
        times.push(t);

        // Normalized coordinate in [-1.0, 1.0] centered at 0.5
        let centered = (u - 0.5) * 2.0;

        let w = match config.kernel {
            TemporalFilterKernel::Box => 1.0f32,
            TemporalFilterKernel::Triangle => (1.0 - centered.abs() as f32).max(0.001),
            TemporalFilterKernel::Gaussian => {
                // 3-sigma cutoff at edges
                let sigma = 0.5f32;
                let x = centered as f32;
                (-0.5 * (x / sigma).powi(2)).exp()
            }
        };
        raw_weights.push(w);
    }

    let sum_w: f32 = raw_weights.iter().sum();
    let norm_factor = if sum_w > 0.0 { 1.0 / sum_w } else { 1.0 / n as f32 };

    times
        .into_iter()
        .zip(raw_weights)
        .map(|(t, w)| (t, w * norm_factor))
        .collect()
}

/// Accumulates multiple subframe pixmaps with individual weights into a final composited pixmap.
pub fn accumulate_subframes(subframes: &[(Pixmap, f32)]) -> Result<Pixmap, RasterError> {
    if subframes.is_empty() {
        return Err(RasterError::Scene("no subframes provided for accumulation".into()));
    }

    let (first_pixmap, _) = &subframes[0];
    let w = first_pixmap.width();
    let h = first_pixmap.height();

    for (p, _) in subframes.iter() {
        if p.width() != w || p.height() != h {
            return Err(RasterError::Scene("subframe dimension mismatch in motion blur accumulation".into()));
        }
    }

    let mut output = Pixmap::new(w, h)
        .ok_or_else(|| RasterError::Scene("failed to allocate accumulation buffer".into()))?;

    let total_pixels = (w * h) as usize;
    let mut acc = vec![[0.0f32; 4]; total_pixels];

    for (pixmap, weight) in subframes {
        let data = pixmap.data();
        let w_val = *weight;
        for i in 0..total_pixels {
            let offset = i * 4;
            acc[i][0] += data[offset] as f32 * w_val;
            acc[i][1] += data[offset + 1] as f32 * w_val;
            acc[i][2] += data[offset + 2] as f32 * w_val;
            acc[i][3] += data[offset + 3] as f32 * w_val;
        }
    }

    let out_data = output.data_mut();
    for i in 0..total_pixels {
        let offset = i * 4;
        out_data[offset] = acc[i][0].round().clamp(0.0, 255.0) as u8;
        out_data[offset + 1] = acc[i][1].round().clamp(0.0, 255.0) as u8;
        out_data[offset + 2] = acc[i][2].round().clamp(0.0, 255.0) as u8;
        out_data[offset + 3] = acc[i][3].round().clamp(0.0, 255.0) as u8;
    }

    Ok(output)
}

/// Applies high-quality 2D directional motion blur to a pixmap along an arbitrary angle.
///
/// - `angle_deg`: Direction of motion blur in degrees (0 = horizontal right, 90 = vertical down).
/// - `length`: Blur distance in pixels.
/// - `samples`: Number of sub-samples along the trail (clamped to 3..=64).
/// - `kernel`: Temporal weighting kernel (Box, Triangle, or Gaussian).
pub fn apply_directional_motion_blur(
    pixmap: &mut Pixmap,
    angle_deg: f32,
    length: f32,
    samples: u32,
    kernel: TemporalFilterKernel,
) -> Result<(), RasterError> {
    if !angle_deg.is_finite() || !length.is_finite() {
        return Err(RasterError::Scene("motion blur parameters must be finite".into()));
    }

    let length = length.max(0.0);
    if length < 0.25 || samples <= 1 {
        return Ok(());
    }

    let n = samples.clamp(3, 64) as usize;
    let rad = angle_deg.to_radians();
    let dx_total = rad.cos() * length;
    let dy_total = rad.sin() * length;

    let w = pixmap.width() as i32;
    let h = pixmap.height() as i32;
    let src = pixmap.data().to_vec();

    // Compute sample weights
    let mut raw_weights = Vec::with_capacity(n);
    for i in 0..n {
        let u = i as f32 / (n - 1) as f32; // [0.0, 1.0]
        let centered = (u - 0.5) * 2.0;    // [-1.0, 1.0]
        let w = match kernel {
            TemporalFilterKernel::Box => 1.0f32,
            TemporalFilterKernel::Triangle => (1.0 - centered.abs()).max(0.001),
            TemporalFilterKernel::Gaussian => {
                let sigma = 0.5f32;
                (-0.5 * (centered / sigma).powi(2)).exp()
            }
        };
        raw_weights.push(w);
    }
    let weight_sum: f32 = raw_weights.iter().sum();
    let norm_factor = 1.0 / weight_sum;
    let weights: Vec<f32> = raw_weights.iter().map(|w| w * norm_factor).collect();

    let row_stride = (w * 4) as usize;
    let mut out_data = vec![0u8; src.len()];

    // Parallel process by row
    out_data
        .par_chunks_exact_mut(row_stride)
        .enumerate()
        .for_each(|(y_usize, row)| {
            let y = y_usize as i32;
            for x in 0..w {
                let mut acc = [0.0f32; 4];

                for i in 0..n {
                    let u = (i as f32 / (n - 1) as f32) - 0.5; // [-0.5, 0.5]
                    let sx = (x as f32 + u * dx_total).round() as i32;
                    let sy = (y as f32 + u * dy_total).round() as i32;

                    let clamped_x = sx.clamp(0, w - 1) as usize;
                    let clamped_y = sy.clamp(0, h - 1) as usize;

                    let src_idx = (clamped_y * (w as usize) + clamped_x) * 4;
                    let w_i = weights[i];

                    acc[0] += src[src_idx] as f32 * w_i;
                    acc[1] += src[src_idx + 1] as f32 * w_i;
                    acc[2] += src[src_idx + 2] as f32 * w_i;
                    acc[3] += src[src_idx + 3] as f32 * w_i;
                }

                let dst_idx = (x as usize) * 4;
                row[dst_idx] = acc[0].round().clamp(0.0, 255.0) as u8;
                row[dst_idx + 1] = acc[1].round().clamp(0.0, 255.0) as u8;
                row[dst_idx + 2] = acc[2].round().clamp(0.0, 255.0) as u8;
                row[dst_idx + 3] = acc[3].round().clamp(0.0, 255.0) as u8;
            }
        });

    pixmap.data_mut().copy_from_slice(&out_data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::{Color, Paint, Rect, Transform};

    #[test]
    fn test_subframe_time_samples_sum_to_one() {
        let cfg = ShutterConfig::cinematic_180(16);
        let samples = subframe_time_samples(10.0, 30.0, &cfg);
        assert_eq!(samples.len(), 16);

        let sum: f32 = samples.iter().map(|(_, w)| w).sum();
        assert!((sum - 1.0).abs() < 1e-5, "weights must normalize to 1.0");

        // First sample should be before frame center (due to -90° phase)
        let frame_time = 10.0 / 30.0;
        assert!(samples[0].0 < frame_time);
        assert!(samples.last().unwrap().0 > frame_time);
    }

    #[test]
    fn test_triangle_and_gaussian_kernels() {
        let cfg_tri = ShutterConfig::new(180.0, -90.0, 9, TemporalFilterKernel::Triangle);
        let samples_tri = subframe_time_samples(0.0, 30.0, &cfg_tri);

        // Center sample (idx 4) should have highest weight
        let center_w = samples_tri[4].1;
        assert!(center_w > samples_tri[0].1);
        assert!(center_w > samples_tri[8].1);
        // Symmetry
        assert!((samples_tri[0].1 - samples_tri[8].1).abs() < 1e-5);
        assert!((samples_tri[2].1 - samples_tri[6].1).abs() < 1e-5);

        let cfg_gauss = ShutterConfig::smooth_gaussian(11);
        let samples_gauss = subframe_time_samples(0.0, 30.0, &cfg_gauss);
        assert!(samples_gauss[5].1 > samples_gauss[0].1);
        assert!((samples_gauss[0].1 - samples_gauss[10].1).abs() < 1e-5);
    }

    #[test]
    fn test_directional_motion_blur_horizontal_and_diagonal() {
        let mut pixmap = Pixmap::new(64, 64).unwrap();
        // Draw a solid 8x8 white square in the center
        let mut paint = Paint::default();
        paint.set_color(Color::WHITE);
        pixmap.fill_rect(
            Rect::from_xywh(28.0, 28.0, 8.0, 8.0).unwrap(),
            &paint,
            Transform::identity(),
            None,
        );

        // Apply horizontal blur (angle 0°, length 16px)
        apply_directional_motion_blur(
            &mut pixmap,
            0.0,
            16.0,
            16,
            TemporalFilterKernel::Gaussian,
        )
        .expect("blur should succeed");

        // The center should still be bright
        let center = pixmap.pixel(32, 32).unwrap();
        assert!(center.alpha() > 100);

        // The blur should have spread horizontally to x=20 and x=44
        let left_trail = pixmap.pixel(22, 32).unwrap();
        assert!(left_trail.alpha() > 0, "trail should spread horizontally");

        // But vertically outside y=28..36 it should still be empty
        let top_outside = pixmap.pixel(32, 10).unwrap();
        assert_eq!(top_outside.alpha(), 0, "no vertical spread on horizontal blur");
    }

    #[test]
    fn test_accumulate_subframes() {
        let mut p1 = Pixmap::new(16, 16).unwrap();
        p1.fill(Color::from_rgba8(100, 0, 0, 255));

        let mut p2 = Pixmap::new(16, 16).unwrap();
        p2.fill(Color::from_rgba8(0, 100, 0, 255));

        let combined = accumulate_subframes(&[(p1, 0.5), (p2, 0.5)]).unwrap();
        let px = combined.pixel(8, 8).unwrap();
        assert_eq!(px.red(), 50);
        assert_eq!(px.green(), 50);
        assert_eq!(px.blue(), 0);
        assert_eq!(px.alpha(), 255);
    }
}
