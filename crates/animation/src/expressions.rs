//! After Effects Expression Engine.
//!
//! Provides core built-in functions:
//! - `wiggle(freq, amp, octaves, amp_mult, time)`: multi-harmonic organic noise jitter.
//! - `loopOut(track, loop_type, num_keyframes, time)`: infinite keyframe cycling (`Cycle`, `PingPong`, `Offset`, `Continue`).
//! - `loopIn(track, loop_type, num_keyframes, time)`: pre-timeline keyframe cycling.
//! - `valueAtTime(track, time)`: arbitrary temporal property sampling.
//! - `linear(t, t_min, t_max, v1, v2)` / `ease(...)`: value remapping and smooth transitions.
//! - `lookAt(from_point, to_point)`: 3D orientation angles facing target point.
//! - Expression Controllers: Slider, Angle, Point, Color, Checkbox.

use crate::keyframe::{Animatable, KeyframeTrack};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Mode for `loopOut` and `loopIn` After Effects expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LoopType {
    /// Repeats the keyframe range cyclically from the beginning.
    #[default]
    Cycle,
    /// Plays keyframes forward, then backward, then forward, alternating endlessly.
    PingPong,
    /// Repeats cyclically, but with each repetition shifted by the delta value between start and end.
    Offset,
    /// Extrapolates into the future linearly using the final keyframe's instantaneous velocity.
    Continue,
}

// ─── 1D / 2D / 3D Noise Generator for `wiggle()` ─────────────────────────────

/// Computes a continuous 1D Perlin-style gradient noise value in `[-1.0, 1.0]`.
///
/// Uses quintic Hermite smoothing curve $6t^5 - 15t^4 + 10t^3$ for $C^2$ continuous derivatives.
#[inline]
pub fn perlin_noise_1d(x: f64, seed: u64) -> f64 {
    let phase = (seed as f64) * 0.31718281828;
    let xp = x + phase;
    let xi = xp.floor() as i64;
    let xf = xp - xp.floor();

    let grad = |idx: i64| -> f64 {
        let mut h = (idx as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ seed;
        h = (h ^ (h >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        h = (h ^ (h >> 27)).wrapping_mul(0x94D049BB133111EB);
        h ^= h >> 31;
        // Map pseudo-random hash to gradient in [-1.0, 1.0]
        ((h & 0xFFFF) as f64 / 32767.5) - 1.0
    };

    let g0 = grad(xi);
    let g1 = grad(xi + 1);

    let v0 = g0 * xf;
    let v1 = g1 * (xf - 1.0);

    // Quintic Hermite interpolant: 6*t^5 - 15*t^4 + 10*t^3
    let u = xf * xf * xf * (xf * (xf * 6.0 - 15.0) + 10.0);

    (v0 + u * (v1 - v0)) * 2.0
}

/// After Effects 1D `wiggle(freq, amp, octaves, amp_mult, time)`.
///
/// Evaluates multi-harmonic fractal noise:
/// - `freq`: Frequency in oscillations/second.
/// - `amp`: Amplitude of variation.
/// - `octaves`: Number of noise octaves / detail levels (default 1).
/// - `amp_mult`: Scaling factor for each octave's amplitude (default 0.5).
/// - `time`: Current composition time in seconds.
pub fn wiggle(freq: f64, amp: f64, octaves: usize, amp_mult: f64, time: f64) -> f64 {
    wiggle_seeded(freq, amp, octaves, amp_mult, time, 1337)
}

/// Seeded 1D `wiggle`.
pub fn wiggle_seeded(
    freq: f64,
    amp: f64,
    octaves: usize,
    amp_mult: f64,
    time: f64,
    seed: u64,
) -> f64 {
    let n_octaves = octaves.max(1);
    let mut total = 0.0;
    let mut cur_freq = freq.abs();
    let mut cur_amp = amp;

    for oct in 0..n_octaves {
        let val = perlin_noise_1d(time * cur_freq, seed.wrapping_add((oct as u64) * 31));
        total += val * cur_amp;
        cur_freq *= 2.0;
        cur_amp *= amp_mult;
    }

    total
}

/// After Effects 2D `wiggle` producing independent X and Y jitter.
pub fn wiggle_2d(
    freq: f64,
    amp: f64,
    octaves: usize,
    amp_mult: f64,
    time: f64,
) -> (f64, f64) {
    let x = wiggle_seeded(freq, amp, octaves, amp_mult, time, 101);
    let y = wiggle_seeded(freq, amp, octaves, amp_mult, time, 202);
    (x, y)
}

/// After Effects 3D `wiggle` producing independent X, Y, and Z jitter.
pub fn wiggle_3d(
    freq: f64,
    amp: f64,
    octaves: usize,
    amp_mult: f64,
    time: f64,
) -> (f64, f64, f64) {
    let x = wiggle_seeded(freq, amp, octaves, amp_mult, time, 101);
    let y = wiggle_seeded(freq, amp, octaves, amp_mult, time, 202);
    let z = wiggle_seeded(freq, amp, octaves, amp_mult, time, 303);
    (x, y, z)
}

// ─── Value Remapping & Interpolation Expressions ─────────────────────────────

/// Linear interpolation and range remapping matching After Effects `linear(t, tMin, tMax, value1, value2)`.
///
/// Clamps `t` to `[t_min, t_max]`.
#[inline]
pub fn linear(t: f64, t_min: f64, t_max: f64, value1: f64, value2: f64) -> f64 {
    if (t_max - t_min).abs() < 1e-12 {
        return value1;
    }
    let u = ((t - t_min) / (t_max - t_min)).clamp(0.0, 1.0);
    value1 + (value2 - value1) * u
}

/// Smooth cubic ease interpolation matching After Effects `ease(t, tMin, tMax, value1, value2)`.
///
/// Uses Hermite smoothstep $S(u) = 3u^2 - 2u^3$ with zero tangent velocity at both endpoints.
#[inline]
pub fn ease(t: f64, t_min: f64, t_max: f64, value1: f64, value2: f64) -> f64 {
    if (t_max - t_min).abs() < 1e-12 {
        return value1;
    }
    let u = ((t - t_min) / (t_max - t_min)).clamp(0.0, 1.0);
    let smooth = u * u * (3.0 - 2.0 * u);
    value1 + (value2 - value1) * smooth
}

/// Quadratic ease-in interpolation matching After Effects `easeIn(t, tMin, tMax, value1, value2)`.
#[inline]
pub fn ease_in(t: f64, t_min: f64, t_max: f64, value1: f64, value2: f64) -> f64 {
    if (t_max - t_min).abs() < 1e-12 {
        return value1;
    }
    let u = ((t - t_min) / (t_max - t_min)).clamp(0.0, 1.0);
    let smooth = u * u;
    value1 + (value2 - value1) * smooth
}

/// Quadratic ease-out interpolation matching After Effects `easeOut(t, tMin, tMax, value1, value2)`.
#[inline]
pub fn ease_out(t: f64, t_min: f64, t_max: f64, value1: f64, value2: f64) -> f64 {
    if (t_max - t_min).abs() < 1e-12 {
        return value1;
    }
    let u = ((t - t_min) / (t_max - t_min)).clamp(0.0, 1.0);
    let smooth = u * (2.0 - u);
    value1 + (value2 - value1) * smooth
}

/// Calculates 3D orientation angles in degrees facing from `from_point` to `to_point`.
///
/// Returns `[rx, ry, rz]` (Pitch, Yaw, Roll) in Euler degrees matching After Effects 3D Camera / Layer orientations.
pub fn look_at(from_point: [f32; 3], to_point: [f32; 3]) -> [f32; 3] {
    let dx = to_point[0] - from_point[0];
    let dy = to_point[1] - from_point[1];
    let dz = to_point[2] - from_point[2];

    let xz_dist = (dx * dx + dz * dz).sqrt();

    // Pitch (X rotation): angle up or down
    let rx = (-dy).atan2(xz_dist).to_degrees();

    // Yaw (Y rotation): azimuth angle in horizontal plane
    let ry = dx.atan2(dz).to_degrees();

    // Roll (Z rotation): default upright (0.0)
    let rz = 0.0f32;

    [rx, ry, rz]
}

// ─── Keyframe Looping Expressions: loopOut / loopIn ──────────────────────────

/// Samples a keyframe track at an arbitrary time in seconds.
///
/// Matches After Effects `valueAtTime(t)`.
#[inline]
pub fn value_at_time<T: Animatable>(track: &KeyframeTrack<T>, time_sec: f64) -> T {
    let frame = time_sec * track.fps;
    track.evaluate(frame)
}

/// Samples the instantaneous velocity of a keyframe track at `time_sec` (units per second).
#[inline]
pub fn velocity_at_time<T: Animatable>(track: &KeyframeTrack<T>, time_sec: f64) -> T {
    let frame = time_sec * track.fps;
    track.velocity(frame)
}

/// Evaluates After Effects `loopOut(type, numKeyframes)` on an animatable track.
///
/// - `loop_type`: `Cycle`, `PingPong`, `Offset`, or `Continue`.
/// - `num_keyframes`: Number of keyframes to include in the loop cycle. `0` means all keyframes.
/// - `time_sec`: Current composition time in seconds.
pub fn loop_out(
    track: &KeyframeTrack<f64>,
    loop_type: LoopType,
    num_keyframes: usize,
    time_sec: f64,
) -> f64 {
    let n = track.keyframes.len();
    if n < 2 {
        return value_at_time(track, time_sec);
    }

    let count = if num_keyframes == 0 || num_keyframes >= n {
        n
    } else {
        num_keyframes.max(2)
    };

    let start_idx = n - count;
    let k_start = &track.keyframes[start_idx];
    let k_end = &track.keyframes[n - 1];

    let t_start = k_start.frame / track.fps;
    let t_end = k_end.frame / track.fps;
    let duration = t_end - t_start;

    if duration <= 1e-9 || time_sec <= t_end {
        return value_at_time(track, time_sec);
    }

    let delta = time_sec - t_end;

    match loop_type {
        LoopType::Cycle => {
            let offset = delta % duration;
            value_at_time(track, t_start + offset)
        }
        LoopType::PingPong => {
            let cycles = (delta / duration).floor() as i64;
            let remainder = delta - (cycles as f64) * duration;
            if cycles % 2 == 0 {
                // Moving backward from end to start
                value_at_time(track, t_end - remainder)
            } else {
                // Moving forward from start to end
                value_at_time(track, t_start + remainder)
            }
        }
        LoopType::Offset => {
            let cycles = (delta / duration).floor() as i64 + 1;
            let remainder = delta % duration;
            let base_val = value_at_time(track, t_start + remainder);
            let val_step = k_end.value - k_start.value;
            base_val + (cycles as f64) * val_step
        }
        LoopType::Continue => {
            let v_final = track.velocity(k_end.frame);
            k_end.value + delta * v_final
        }
    }
}

/// Evaluates After Effects `loopIn(type, numKeyframes)` on an animatable track.
///
/// Loops keyframes before the first keyframe time.
pub fn loop_in(
    track: &KeyframeTrack<f64>,
    loop_type: LoopType,
    num_keyframes: usize,
    time_sec: f64,
) -> f64 {
    let n = track.keyframes.len();
    if n < 2 {
        return value_at_time(track, time_sec);
    }

    let count = if num_keyframes == 0 || num_keyframes >= n {
        n
    } else {
        num_keyframes.max(2)
    };

    let end_idx = count - 1;
    let k_start = &track.keyframes[0];
    let k_end = &track.keyframes[end_idx];

    let t_start = k_start.frame / track.fps;
    let t_end = k_end.frame / track.fps;
    let duration = t_end - t_start;

    if duration <= 1e-9 || time_sec >= t_start {
        return value_at_time(track, time_sec);
    }

    let delta = t_start - time_sec;

    match loop_type {
        LoopType::Cycle => {
            let offset = delta % duration;
            value_at_time(track, t_end - offset)
        }
        LoopType::PingPong => {
            let cycles = (delta / duration).floor() as i64;
            let remainder = delta - (cycles as f64) * duration;
            if cycles % 2 == 0 {
                value_at_time(track, t_start + remainder)
            } else {
                value_at_time(track, t_end - remainder)
            }
        }
        LoopType::Offset => {
            let cycles = (delta / duration).floor() as i64 + 1;
            let remainder = delta % duration;
            let base_val = value_at_time(track, t_end - remainder);
            let val_step = k_end.value - k_start.value;
            base_val - (cycles as f64) * val_step
        }
        LoopType::Continue => {
            let v_start = track.velocity(k_start.frame);
            k_start.value - delta * v_start
        }
    }
}

// ─── Expression Controllers ──────────────────────────────────────────────────

/// Standard After Effects Expression Controllers.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExpressionControllers {
    pub sliders: HashMap<String, f32>,
    pub angles: HashMap<String, f32>,
    pub points: HashMap<String, [f32; 3]>,
    pub colors: HashMap<String, [f32; 4]>,
    pub checkboxes: HashMap<String, bool>,
}

impl ExpressionControllers {
    /// Creates an empty controllers collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or updates a Slider Control.
    pub fn with_slider(mut self, name: impl Into<String>, value: f32) -> Self {
        self.sliders.insert(name.into(), value);
        self
    }

    /// Adds or updates an Angle Control in degrees.
    pub fn with_angle(mut self, name: impl Into<String>, deg: f32) -> Self {
        self.angles.insert(name.into(), deg);
        self
    }

    /// Adds or updates a 2D/3D Point Control.
    pub fn with_point(mut self, name: impl Into<String>, point: [f32; 3]) -> Self {
        self.points.insert(name.into(), point);
        self
    }

    /// Adds or updates a Color Control `[r, g, b, a]`.
    pub fn with_color(mut self, name: impl Into<String>, color: [f32; 4]) -> Self {
        self.colors.insert(name.into(), color);
        self
    }

    /// Adds or updates a Checkbox Control.
    pub fn with_checkbox(mut self, name: impl Into<String>, checked: bool) -> Self {
        self.checkboxes.insert(name.into(), checked);
        self
    }

    /// Retrieves slider value, falling back to default if not present.
    pub fn get_slider(&self, name: &str, default: f32) -> f32 {
        self.sliders.get(name).copied().unwrap_or(default)
    }

    /// Retrieves angle value in degrees.
    pub fn get_angle(&self, name: &str, default: f32) -> f32 {
        self.angles.get(name).copied().unwrap_or(default)
    }

    /// Retrieves point value.
    pub fn get_point(&self, name: &str, default: [f32; 3]) -> [f32; 3] {
        self.points.get(name).copied().unwrap_or(default)
    }

    /// Retrieves color value.
    pub fn get_color(&self, name: &str, default: [f32; 4]) -> [f32; 4] {
        self.colors.get(name).copied().unwrap_or(default)
    }

    /// Retrieves checkbox value.
    pub fn get_checkbox(&self, name: &str, default: bool) -> bool {
        self.checkboxes.get(name).copied().unwrap_or(default)
    }
}

/// Evaluation execution context passed to After Effects expressions.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpressionContext {
    /// Current composition time in seconds.
    pub time: f64,
    /// Current composition frame number.
    pub frame: f64,
    /// Composition frames per second.
    pub fps: f64,
    /// Controller parameters.
    pub controllers: ExpressionControllers,
}

impl ExpressionContext {
    /// Creates a new expression execution context.
    pub fn new(time: f64, fps: f64) -> Self {
        let valid_fps = if fps > 0.0 { fps } else { 30.0 };
        Self {
            time,
            frame: time * valid_fps,
            fps: valid_fps,
            controllers: ExpressionControllers::new(),
        }
    }

    /// Shortcut for 1D `wiggle` at current time.
    pub fn wiggle(&self, freq: f64, amp: f64) -> f64 {
        wiggle(freq, amp, 1, 0.5, self.time)
    }

    /// Shortcut for 2D `wiggle` at current time.
    pub fn wiggle_2d(&self, freq: f64, amp: f64) -> (f64, f64) {
        wiggle_2d(freq, amp, 1, 0.5, self.time)
    }

    /// Shortcut for `linear` evaluated at current time.
    pub fn linear(&self, t_min: f64, t_max: f64, v1: f64, v2: f64) -> f64 {
        linear(self.time, t_min, t_max, v1, v2)
    }

    /// Shortcut for `ease` evaluated at current time.
    pub fn ease(&self, t_min: f64, t_max: f64, v1: f64, v2: f64) -> f64 {
        ease(self.time, t_min, t_max, v1, v2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyframe::Keyframe;

    #[test]
    fn test_perlin_noise_bounds_and_continuity() {
        for i in 0..100 {
            let t = i as f64 * 0.1;
            let val = perlin_noise_1d(t, 42);
            assert!(val >= -2.0 && val <= 2.0, "noise val {} out of bounds", val);
        }

        // Check continuity across integer boundaries
        let v0 = perlin_noise_1d(0.999, 42);
        let v1 = perlin_noise_1d(1.001, 42);
        assert!((v0 - v1).abs() < 0.05, "noise should be smooth across integers");
    }

    #[test]
    fn test_wiggle_amplitude_and_octaves() {
        let val1 = wiggle(5.0, 50.0, 1, 0.5, 1.25);
        assert!(val1.abs() <= 100.0, "wiggle amplitude respected");

        let (x, y) = wiggle_2d(2.0, 100.0, 2, 0.5, 0.5);
        // Independent channels should not be identical
        assert_ne!(x, y);
    }

    #[test]
    fn test_linear_and_ease_interpolations() {
        assert_eq!(linear(0.0, 0.0, 10.0, 100.0, 200.0), 100.0);
        assert_eq!(linear(5.0, 0.0, 10.0, 100.0, 200.0), 150.0);
        assert_eq!(linear(10.0, 0.0, 10.0, 100.0, 200.0), 200.0);
        // Clamping outside bounds
        assert_eq!(linear(-5.0, 0.0, 10.0, 100.0, 200.0), 100.0);
        assert_eq!(linear(15.0, 0.0, 10.0, 100.0, 200.0), 200.0);

        // Ease mid-point is also 50%, but starts and ends with zero speed
        assert_eq!(ease(5.0, 0.0, 10.0, 100.0, 200.0), 150.0);
        // At 20% time, ease is slower than linear
        assert!(ease(2.0, 0.0, 10.0, 100.0, 200.0) < linear(2.0, 0.0, 10.0, 100.0, 200.0));
    }

    #[test]
    fn test_look_at_angles() {
        // Pointing straight along Z axis: Yaw 0, Pitch 0
        let angles_z = look_at([0.0, 0.0, 0.0], [0.0, 0.0, 100.0]);
        assert!((angles_z[0]).abs() < 1e-4);
        assert!((angles_z[1]).abs() < 1e-4);

        // Pointing straight along X axis: Yaw +90
        let angles_x = look_at([0.0, 0.0, 0.0], [100.0, 0.0, 0.0]);
        assert!((angles_x[1] - 90.0).abs() < 1e-4);

        // Pointing straight up along Y axis: Pitch -90
        let angles_y = look_at([0.0, 0.0, 0.0], [0.0, 100.0, 0.0]);
        assert!((angles_y[0] - (-90.0)).abs() < 1e-4);
    }

    #[test]
    fn test_loop_out_cycle() {
        let mut track = KeyframeTrack::new(0.0, 30.0);
        track.insert_keyframe(Keyframe::linear(0.0, 0.0));   // t = 0.0s, val = 0
        track.insert_keyframe(Keyframe::linear(30.0, 100.0)); // t = 1.0s, val = 100

        // In-range evaluations
        assert_eq!(loop_out(&track, LoopType::Cycle, 0, 0.5), 50.0);
        assert_eq!(loop_out(&track, LoopType::Cycle, 0, 1.0), 100.0);

        // Cycle loops every 1.0 second:
        assert!((loop_out(&track, LoopType::Cycle, 0, 1.5) - 50.0).abs() < 1e-6);
        assert!((loop_out(&track, LoopType::Cycle, 0, 2.0) - 0.0).abs() < 1e-6);
        assert!((loop_out(&track, LoopType::Cycle, 0, 2.25) - 25.0).abs() < 1e-6);
    }

    #[test]
    fn test_loop_out_ping_pong() {
        let mut track = KeyframeTrack::new(0.0, 30.0);
        track.insert_keyframe(Keyframe::linear(0.0, 0.0));   // t = 0.0s
        track.insert_keyframe(Keyframe::linear(30.0, 100.0)); // t = 1.0s

        // t = 1.0s: 100
        // t = 1.5s: pinging back -> 50
        // t = 2.0s: back at start -> 0
        // t = 2.5s: ponging forward -> 50
        // t = 3.0s: forward at end -> 100
        assert!((loop_out(&track, LoopType::PingPong, 0, 1.5) - 50.0).abs() < 1e-6);
        assert!((loop_out(&track, LoopType::PingPong, 0, 2.0) - 0.0).abs() < 1e-6);
        assert!((loop_out(&track, LoopType::PingPong, 0, 2.5) - 50.0).abs() < 1e-6);
        assert!((loop_out(&track, LoopType::PingPong, 0, 3.0) - 100.0).abs() < 1e-6);
    }

    #[test]
    fn test_loop_out_offset() {
        let mut track = KeyframeTrack::new(0.0, 30.0);
        track.insert_keyframe(Keyframe::linear(0.0, 0.0));
        track.insert_keyframe(Keyframe::linear(30.0, 100.0)); // delta = +100

        // Loop 1 (t = 1.0..2.0): starts at 100, reaches 200
        assert!((loop_out(&track, LoopType::Offset, 0, 1.5) - 150.0).abs() < 1e-6);
        assert!((loop_out(&track, LoopType::Offset, 0, 2.0) - 200.0).abs() < 1e-6);
        // Loop 2 (t = 2.0..3.0): reaches 300
        assert!((loop_out(&track, LoopType::Offset, 0, 2.5) - 250.0).abs() < 1e-6);
        assert!((loop_out(&track, LoopType::Offset, 0, 3.0) - 300.0).abs() < 1e-6);
    }

    #[test]
    fn test_loop_out_continue() {
        let mut track = KeyframeTrack::new(0.0, 30.0);
        track.insert_keyframe(Keyframe::linear(0.0, 0.0));
        track.insert_keyframe(Keyframe::linear(30.0, 100.0)); // velocity = 100 units / sec

        // Continues linearly into the future at 100 units/sec
        assert!((loop_out(&track, LoopType::Continue, 0, 2.0) - 200.0).abs() < 1e-6);
        assert!((loop_out(&track, LoopType::Continue, 0, 3.5) - 350.0).abs() < 1e-6);
    }

    #[test]
    fn test_expression_controllers() {
        let ctrl = ExpressionControllers::new()
            .with_slider("Intensity", 42.5)
            .with_angle("Rotation", 180.0)
            .with_checkbox("EnableBloom", true)
            .with_color("Tint", [1.0, 0.5, 0.0, 1.0]);

        assert_eq!(ctrl.get_slider("Intensity", 0.0), 42.5);
        assert_eq!(ctrl.get_slider("NonExistent", 10.0), 10.0);
        assert_eq!(ctrl.get_angle("Rotation", 0.0), 180.0);
        assert!(ctrl.get_checkbox("EnableBloom", false));
        assert_eq!(ctrl.get_color("Tint", [0.0; 4]), [1.0, 0.5, 0.0, 1.0]);
    }
}
