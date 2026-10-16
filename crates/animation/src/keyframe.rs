//! Adobe After Effects-grade Keyframe & Graph Editor engine.
//!
//! Provides cubic Bézier temporal keyframe interpolation with support for
//! incoming/outgoing speeds and influence percentages, matching After Effects'
//! Speed Graph and Value Graph mathematics.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Keyframe temporal interpolation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum InterpolationType {
    /// Constant linear progression between adjacent keyframes.
    #[default]
    Linear,
    /// Cubic Bézier curve defined by speed and influence tangents.
    Bezier,
    /// Instant step change: maintains current value until next keyframe.
    Hold,
}

/// Tangent ease specification matching After Effects' `KeyframeEase`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct KeyframeEase {
    /// Speed in units per second (e.g. pixels/sec, degrees/sec).
    pub speed: f64,
    /// Influence percentage in (0.0, 100.0] (default 33.333333%).
    pub influence: f64,
}

impl KeyframeEase {
    /// Creates a new keyframe ease with the specified speed and influence.
    pub fn new(speed: f64, influence: f64) -> Self {
        Self {
            speed,
            influence: influence.clamp(0.0001, 100.0),
        }
    }

    /// Normalized influence in (0.0, 1.0].
    #[inline]
    pub fn normalized_influence(&self) -> f64 {
        if self.influence > 1.0 {
            (self.influence / 100.0).clamp(0.0001, 1.0)
        } else {
            self.influence.clamp(0.0001, 1.0)
        }
    }

    /// Standard After Effects Easy Ease preset: speed = 0, influence = 33.333%.
    #[inline]
    pub fn easy_ease() -> Self {
        Self::new(0.0, 33.333333333333336)
    }

    /// Standard linear ease matching linear velocity.
    #[inline]
    pub fn linear() -> Self {
        Self::new(0.0, 0.1)
    }
}

/// A keyframe representing a value at a specific timeline frame.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Keyframe<T> {
    /// Timeline frame position (floating point to allow sub-frame precision).
    pub frame: f64,
    /// Value at this keyframe.
    pub value: T,
    /// Incoming ease tangent (from previous keyframe).
    pub in_ease: Option<KeyframeEase>,
    /// Outgoing ease tangent (to next keyframe).
    pub out_ease: Option<KeyframeEase>,
    /// Incoming interpolation type.
    pub interpolation_in: InterpolationType,
    /// Outgoing interpolation type.
    pub interpolation_out: InterpolationType,
}

impl<T: Clone> Keyframe<T> {
    /// Creates a linear keyframe.
    pub fn linear(frame: f64, value: T) -> Self {
        Self {
            frame,
            value,
            in_ease: None,
            out_ease: None,
            interpolation_in: InterpolationType::Linear,
            interpolation_out: InterpolationType::Linear,
        }
    }

    /// Creates an Easy Ease (F9) keyframe with smooth zero-speed entry and exit.
    pub fn easy_ease(frame: f64, value: T) -> Self {
        Self {
            frame,
            value,
            in_ease: Some(KeyframeEase::easy_ease()),
            out_ease: Some(KeyframeEase::easy_ease()),
            interpolation_in: InterpolationType::Bezier,
            interpolation_out: InterpolationType::Bezier,
        }
    }

    /// Creates an Ease In (Shift+F9) keyframe (smooth deceleration on arrival).
    pub fn ease_in(frame: f64, value: T) -> Self {
        Self {
            frame,
            value,
            in_ease: Some(KeyframeEase::easy_ease()),
            out_ease: None,
            interpolation_in: InterpolationType::Bezier,
            interpolation_out: InterpolationType::Linear,
        }
    }

    /// Creates an Ease Out (Ctrl+Shift+F9) keyframe (smooth acceleration on departure).
    pub fn ease_out(frame: f64, value: T) -> Self {
        Self {
            frame,
            value,
            in_ease: None,
            out_ease: Some(KeyframeEase::easy_ease()),
            interpolation_in: InterpolationType::Linear,
            interpolation_out: InterpolationType::Bezier,
        }
    }

    /// Creates a Hold keyframe (value stays fixed until the next keyframe).
    pub fn hold(frame: f64, value: T) -> Self {
        Self {
            frame,
            value,
            in_ease: None,
            out_ease: None,
            interpolation_in: InterpolationType::Hold,
            interpolation_out: InterpolationType::Hold,
        }
    }

    /// Creates a fully custom Bézier keyframe.
    pub fn bezier(frame: f64, value: T, in_ease: KeyframeEase, out_ease: KeyframeEase) -> Self {
        Self {
            frame,
            value,
            in_ease: Some(in_ease),
            out_ease: Some(out_ease),
            interpolation_in: InterpolationType::Bezier,
            interpolation_out: InterpolationType::Bezier,
        }
    }
}

/// Trait for types that can be animated with After Effects keyframes.
pub trait Animatable: Clone + PartialEq {
    /// Linear interpolation between two values.
    fn interpolate_linear(&self, other: &Self, t: f64) -> Self;

    /// 1D cubic Bézier interpolation for After Effects speed/value curves.
    ///
    /// `delta_t` is the time elapsed between keyframes in seconds (`(k1.frame - k0.frame) / fps`).
    fn interpolate_ae_curve(
        &self,
        other: &Self,
        out_ease: Option<KeyframeEase>,
        in_ease: Option<KeyframeEase>,
        t_normalized: f64,
        delta_t_sec: f64,
    ) -> Self;

    /// Calculates instantaneous derivative / velocity (units per second) at `t_normalized`.
    fn velocity_ae_curve(
        &self,
        other: &Self,
        out_ease: Option<KeyframeEase>,
        in_ease: Option<KeyframeEase>,
        t_normalized: f64,
        delta_t_sec: f64,
    ) -> Self;
}

// ─── Mathematical 1D Bézier solver for After Effects Speed/Value Curves ─────────

/// Solves parameter `u ∈ [0, 1]` such that Bézier $X(u) = x$, then computes $Y(u)$.
///
/// Follows After Effects' specification:
/// $X_0 = 0, X_1 = I_{out}, X_2 = 1 - I_{in}, X_3 = 1$.
/// $Y_0 = v_0, Y_1 = v_0 + S_{out} \cdot (I_{out} \cdot \Delta t), Y_2 = v_1 - S_{in} \cdot (I_{in} \cdot \Delta t), Y_3 = v_1$.
#[inline]
pub fn solve_ae_bezier_1d(
    v0: f64,
    v1: f64,
    out_ease: Option<KeyframeEase>,
    in_ease: Option<KeyframeEase>,
    x_target: f64,
    delta_t_sec: f64,
) -> f64 {
    let x_target = x_target.clamp(0.0, 1.0);
    if x_target <= 0.0 {
        return v0;
    }
    if x_target >= 1.0 {
        return v1;
    }

    let linear_speed = (v1 - v0) / delta_t_sec;
    let i_out = out_ease.map_or(0.3333333333333333, |e| e.normalized_influence());
    let s_out = out_ease.map_or(linear_speed, |e| e.speed);

    let i_in = in_ease.map_or(0.3333333333333333, |e| e.normalized_influence());
    let s_in = in_ease.map_or(linear_speed, |e| e.speed);

    // Control points in normalized time [0..1]
    let x1 = i_out;
    let x2 = 1.0 - i_in;

    // Control points in value space
    let y0 = v0;
    let y1 = v0 + s_out * (i_out * delta_t_sec);
    let y2 = v1 - s_in * (i_in * delta_t_sec);
    let y3 = v1;

    // Solve for u where X(u) = x_target using Newton-Raphson + Bisection fallback
    let u = solve_cubic_x(x1, x2, x_target);

    // Evaluate Y(u)
    eval_cubic_bezier(y0, y1, y2, y3, u)
}

/// Calculates derivative $dY/dt$ at target time $x$.
#[inline]
pub fn solve_ae_bezier_derivative_1d(
    v0: f64,
    v1: f64,
    out_ease: Option<KeyframeEase>,
    in_ease: Option<KeyframeEase>,
    x_target: f64,
    delta_t_sec: f64,
) -> f64 {
    if delta_t_sec <= 0.0 {
        return 0.0;
    }
    let x_target = x_target.clamp(0.0, 1.0);

    let linear_speed = (v1 - v0) / delta_t_sec;
    let i_out = out_ease.map_or(0.3333333333333333, |e| e.normalized_influence());
    let s_out = out_ease.map_or(linear_speed, |e| e.speed);

    let i_in = in_ease.map_or(0.3333333333333333, |e| e.normalized_influence());
    let s_in = in_ease.map_or(linear_speed, |e| e.speed);

    let x1 = i_out;
    let x2 = 1.0 - i_in;

    let y0 = v0;
    let y1 = v0 + s_out * (i_out * delta_t_sec);
    let y2 = v1 - s_in * (i_in * delta_t_sec);
    let y3 = v1;

    let u = solve_cubic_x(x1, x2, x_target);

    let dy_du = eval_cubic_bezier_derivative(y0, y1, y2, y3, u);
    let dx_du = eval_cubic_bezier_derivative(0.0, x1, x2, 1.0, u);

    if dx_du.abs() < 1e-9 {
        0.0
    } else {
        (dy_du / dx_du) / delta_t_sec
    }
}

#[inline]
fn eval_cubic_bezier(p0: f64, p1: f64, p2: f64, p3: f64, u: f64) -> f64 {
    let inv_u = 1.0 - u;
    let inv_u2 = inv_u * inv_u;
    let u2 = u * u;

    inv_u2 * inv_u * p0 + 3.0 * inv_u2 * u * p1 + 3.0 * inv_u * u2 * p2 + u2 * u * p3
}

#[inline]
fn eval_cubic_bezier_derivative(p0: f64, p1: f64, p2: f64, p3: f64, u: f64) -> f64 {
    let inv_u = 1.0 - u;
    3.0 * inv_u * inv_u * (p1 - p0) + 6.0 * inv_u * u * (p2 - p1) + 3.0 * u * u * (p3 - p2)
}

#[inline]
fn solve_cubic_x(x1: f64, x2: f64, x: f64) -> f64 {
    // Polynomial coefficients for X(u) where X0=0, X3=1:
    // X(u) = 3*u*(1-u)^2*x1 + 3*u^2*(1-u)*x2 + u^3
    //      = u * (3*x1 + u * (3*x2 - 6*x1 + u * (1 - 3*x2 + 3*x1)))
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;

    let mut u = x; // Initial estimate
    for _ in 0..12 {
        let current_x = ((ax * u + bx) * u + cx) * u - x;
        if current_x.abs() < 1e-9 {
            return u.clamp(0.0, 1.0);
        }
        let current_dx = (3.0 * ax * u + 2.0 * bx) * u + cx;
        if current_dx.abs() < 1e-12 {
            break;
        }
        u -= current_x / current_dx;
    }

    // Fallback to binary bisection if Newton-Raphson diverges
    let mut low = 0.0;
    let mut high = 1.0;
    u = x;
    while (high - low) > 1e-7 {
        let current_x = ((ax * u + bx) * u + cx) * u;
        if current_x < x {
            low = u;
        } else {
            high = u;
        }
        u = 0.5 * (low + high);
    }
    u.clamp(0.0, 1.0)
}

// ─── Animatable Implementations ──────────────────────────────────────────────

impl Animatable for f64 {
    #[inline]
    fn interpolate_linear(&self, other: &Self, t: f64) -> Self {
        self + (other - self) * t
    }

    #[inline]
    fn interpolate_ae_curve(
        &self,
        other: &Self,
        out_ease: Option<KeyframeEase>,
        in_ease: Option<KeyframeEase>,
        t_normalized: f64,
        delta_t_sec: f64,
    ) -> Self {
        solve_ae_bezier_1d(*self, *other, out_ease, in_ease, t_normalized, delta_t_sec)
    }

    #[inline]
    fn velocity_ae_curve(
        &self,
        other: &Self,
        out_ease: Option<KeyframeEase>,
        in_ease: Option<KeyframeEase>,
        t_normalized: f64,
        delta_t_sec: f64,
    ) -> Self {
        solve_ae_bezier_derivative_1d(*self, *other, out_ease, in_ease, t_normalized, delta_t_sec)
    }
}

impl Animatable for f32 {
    #[inline]
    fn interpolate_linear(&self, other: &Self, t: f64) -> Self {
        (*self as f64 + (*other as f64 - *self as f64) * t) as f32
    }

    #[inline]
    fn interpolate_ae_curve(
        &self,
        other: &Self,
        out_ease: Option<KeyframeEase>,
        in_ease: Option<KeyframeEase>,
        t_normalized: f64,
        delta_t_sec: f64,
    ) -> Self {
        solve_ae_bezier_1d(*self as f64, *other as f64, out_ease, in_ease, t_normalized, delta_t_sec) as f32
    }

    #[inline]
    fn velocity_ae_curve(
        &self,
        other: &Self,
        out_ease: Option<KeyframeEase>,
        in_ease: Option<KeyframeEase>,
        t_normalized: f64,
        delta_t_sec: f64,
    ) -> Self {
        solve_ae_bezier_derivative_1d(*self as f64, *other as f64, out_ease, in_ease, t_normalized, delta_t_sec) as f32
    }
}

impl<const N: usize> Animatable for [f32; N] {
    #[inline]
    fn interpolate_linear(&self, other: &Self, t: f64) -> Self {
        let mut res = [0.0f32; N];
        for i in 0..N {
            res[i] = self[i].interpolate_linear(&other[i], t);
        }
        res
    }

    #[inline]
    fn interpolate_ae_curve(
        &self,
        other: &Self,
        out_ease: Option<KeyframeEase>,
        in_ease: Option<KeyframeEase>,
        t_normalized: f64,
        delta_t_sec: f64,
    ) -> Self {
        let mut res = [0.0f32; N];
        for i in 0..N {
            res[i] = self[i].interpolate_ae_curve(
                &other[i],
                out_ease,
                in_ease,
                t_normalized,
                delta_t_sec,
            );
        }
        res
    }

    #[inline]
    fn velocity_ae_curve(
        &self,
        other: &Self,
        out_ease: Option<KeyframeEase>,
        in_ease: Option<KeyframeEase>,
        t_normalized: f64,
        delta_t_sec: f64,
    ) -> Self {
        let mut res = [0.0f32; N];
        for i in 0..N {
            res[i] = self[i].velocity_ae_curve(
                &other[i],
                out_ease,
                in_ease,
                t_normalized,
                delta_t_sec,
            );
        }
        res
    }
}

impl<const N: usize> Animatable for [f64; N] {
    #[inline]
    fn interpolate_linear(&self, other: &Self, t: f64) -> Self {
        let mut res = [0.0f64; N];
        for i in 0..N {
            res[i] = self[i].interpolate_linear(&other[i], t);
        }
        res
    }

    #[inline]
    fn interpolate_ae_curve(
        &self,
        other: &Self,
        out_ease: Option<KeyframeEase>,
        in_ease: Option<KeyframeEase>,
        t_normalized: f64,
        delta_t_sec: f64,
    ) -> Self {
        let mut res = [0.0f64; N];
        for i in 0..N {
            res[i] = self[i].interpolate_ae_curve(
                &other[i],
                out_ease,
                in_ease,
                t_normalized,
                delta_t_sec,
            );
        }
        res
    }

    #[inline]
    fn velocity_ae_curve(
        &self,
        other: &Self,
        out_ease: Option<KeyframeEase>,
        in_ease: Option<KeyframeEase>,
        t_normalized: f64,
        delta_t_sec: f64,
    ) -> Self {
        let mut res = [0.0f64; N];
        for i in 0..N {
            res[i] = self[i].velocity_ae_curve(
                &other[i],
                out_ease,
                in_ease,
                t_normalized,
                delta_t_sec,
            );
        }
        res
    }
}

// ─── KeyframeTrack (After Effects Property Track) ──────────────────────────────

/// An animatable property track composed of zero or more keyframes.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct KeyframeTrack<T> {
    /// Ordered list of keyframes.
    pub keyframes: Vec<Keyframe<T>>,
    /// Static fallback value when no keyframes exist.
    pub default_value: T,
    /// Composition frames per second (fps).
    pub fps: f64,
}

impl<T: Animatable> KeyframeTrack<T> {
    /// Creates a new keyframe track with a default value and composition fps.
    pub fn new(default_value: T, fps: f64) -> Self {
        Self {
            keyframes: Vec::new(),
            default_value,
            fps: if fps > 0.0 { fps } else { 30.0 },
        }
    }

    /// Inserts a keyframe, maintaining ascending order by frame number.
    pub fn insert_keyframe(&mut self, kf: Keyframe<T>) {
        if let Some(pos) = self.keyframes.iter().position(|k| (k.frame - kf.frame).abs() < 1e-6) {
            self.keyframes[pos] = kf;
        } else {
            let pos = self.keyframes.partition_point(|k| k.frame < kf.frame);
            self.keyframes.insert(pos, kf);
        }
    }

    /// Evaluates the property value at the given frame number.
    pub fn evaluate(&self, frame: f64) -> T {
        if self.keyframes.is_empty() {
            return self.default_value.clone();
        }

        if frame <= self.keyframes[0].frame {
            return self.keyframes[0].value.clone();
        }

        let last_idx = self.keyframes.len() - 1;
        if frame >= self.keyframes[last_idx].frame {
            return self.keyframes[last_idx].value.clone();
        }

        // Find the segment [k0, k1] containing `frame`
        let idx = self.keyframes.partition_point(|k| k.frame <= frame);
        let k0 = &self.keyframes[idx - 1];
        let k1 = &self.keyframes[idx];

        let span_frames = k1.frame - k0.frame;
        if span_frames <= 0.0 {
            return k1.value.clone();
        }

        // Check Hold keyframe
        if k0.interpolation_out == InterpolationType::Hold {
            return k0.value.clone();
        }

        let t_norm = (frame - k0.frame) / span_frames;
        let delta_t_sec = span_frames / self.fps;

        if k0.interpolation_out == InterpolationType::Linear
            && k1.interpolation_in == InterpolationType::Linear
        {
            k0.value.interpolate_linear(&k1.value, t_norm)
        } else {
            k0.value.interpolate_ae_curve(
                &k1.value,
                k0.out_ease,
                k1.in_ease,
                t_norm,
                delta_t_sec,
            )
        }
    }

    /// Computes instantaneous velocity (units per second) at the given frame number.
    pub fn velocity(&self, frame: f64) -> T {
        if self.keyframes.len() < 2 {
            return self.default_value.interpolate_linear(&self.default_value, 0.0);
        }

        if frame < self.keyframes[0].frame || frame > self.keyframes.last().unwrap().frame {
            return self.default_value.interpolate_linear(&self.default_value, 0.0);
        }

        let idx = self.keyframes.partition_point(|k| k.frame <= frame);
        let (k0, k1) = if idx == 0 {
            (&self.keyframes[0], &self.keyframes[1])
        } else if idx >= self.keyframes.len() {
            (&self.keyframes[self.keyframes.len() - 2], &self.keyframes[self.keyframes.len() - 1])
        } else {
            (&self.keyframes[idx - 1], &self.keyframes[idx])
        };

        let span_frames = k1.frame - k0.frame;
        if span_frames <= 0.0 {
            return self.default_value.interpolate_linear(&self.default_value, 0.0);
        }

        let t_norm = (frame - k0.frame) / span_frames;
        let delta_t_sec = span_frames / self.fps;

        if k0.interpolation_out == InterpolationType::Hold {
            return self.default_value.interpolate_linear(&self.default_value, 0.0);
        }

        let (out_e, in_e) = if k0.interpolation_out == InterpolationType::Linear
            && k1.interpolation_in == InterpolationType::Linear
        {
            (None, None)
        } else {
            (k0.out_ease, k1.in_ease)
        };
        k0.value.velocity_ae_curve(&k1.value, out_e, in_e, t_norm, delta_t_sec)
    }

    /// Samples the track value at time in seconds (`time_sec * fps`).
    #[inline]
    pub fn value_at_time(&self, time_sec: f64) -> T {
        self.evaluate(time_sec * self.fps)
    }

    /// Samples the instantaneous velocity at time in seconds (`time_sec * fps`).
    #[inline]
    pub fn velocity_at_time(&self, time_sec: f64) -> T {
        self.velocity(time_sec * self.fps)
    }
}

impl KeyframeTrack<f64> {
    /// Evaluates After Effects `loopOut(type, numKeyframes)` on this track.
    pub fn loop_out(&self, loop_type: crate::expressions::LoopType, num_keyframes: usize, time_sec: f64) -> f64 {
        crate::expressions::loop_out(self, loop_type, num_keyframes, time_sec)
    }

    /// Evaluates After Effects `loopIn(type, numKeyframes)` on this track.
    pub fn loop_in(&self, loop_type: crate::expressions::LoopType, num_keyframes: usize, time_sec: f64) -> f64 {
        crate::expressions::loop_in(self, loop_type, num_keyframes, time_sec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_track_evaluation() {
        let mut track = KeyframeTrack::new(0.0f64, 30.0);
        track.insert_keyframe(Keyframe::linear(0.0, 0.0));
        track.insert_keyframe(Keyframe::linear(30.0, 100.0));

        assert_eq!(track.evaluate(0.0), 0.0);
        assert_eq!(track.evaluate(30.0), 100.0);
        assert!((track.evaluate(15.0) - 50.0).abs() < 1e-6);
        assert_eq!(track.evaluate(-5.0), 0.0);
        assert_eq!(track.evaluate(40.0), 100.0);
    }

    #[test]
    fn test_hold_keyframe() {
        let mut track = KeyframeTrack::new(10.0f64, 30.0);
        track.insert_keyframe(Keyframe::hold(0.0, 10.0));
        track.insert_keyframe(Keyframe::linear(30.0, 50.0));

        assert_eq!(track.evaluate(0.0), 10.0);
        assert_eq!(track.evaluate(15.0), 10.0);
        assert_eq!(track.evaluate(29.99), 10.0);
        assert_eq!(track.evaluate(30.0), 50.0);
    }

    #[test]
    fn test_easy_ease_smoothness() {
        let mut track = KeyframeTrack::new(0.0f64, 60.0);
        track.insert_keyframe(Keyframe::easy_ease(0.0, 0.0));
        track.insert_keyframe(Keyframe::easy_ease(60.0, 1000.0));

        // Start and end values
        assert_eq!(track.evaluate(0.0), 0.0);
        assert_eq!(track.evaluate(60.0), 1000.0);

        // Symmetric midpoint should be exactly 50%
        let mid = track.evaluate(30.0);
        assert!((mid - 500.0).abs() < 1.0);

        // Slow start (first 10 frames advance much less than 1/6th of total)
        let early = track.evaluate(10.0);
        let linear_early = 1000.0 * (10.0 / 60.0);
        assert!(early < linear_early, "Easy ease should start slower than linear");

        // Velocity at start and end should be zero
        let v_start = track.velocity(0.0);
        let v_end = track.velocity(60.0);
        assert!(v_start.abs() < 1.0, "Start velocity should be near zero: {v_start}");
        assert!(v_end.abs() < 1.0, "End velocity should be near zero: {v_end}");

        // Midpoint velocity should be peak
        let v_mid = track.velocity(30.0);
        assert!(v_mid > 1000.0, "Midpoint velocity should peak above average: {v_mid}");
    }

    #[test]
    fn test_multidimensional_2d_position_animation() {
        let mut track = KeyframeTrack::new([0.0f32, 0.0f32], 30.0);
        track.insert_keyframe(Keyframe::easy_ease(0.0, [100.0, 200.0]));
        track.insert_keyframe(Keyframe::easy_ease(30.0, [500.0, 800.0]));

        let mid = track.evaluate(15.0);
        assert!((mid[0] - 300.0).abs() < 1.0);
        assert!((mid[1] - 500.0).abs() < 1.0);
    }
}
