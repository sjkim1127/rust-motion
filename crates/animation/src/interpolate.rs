//! `interpolate()` — Rust port of Remotion's `interpolate()` function.
//!
//! Maps an input value from one numeric range to another, with optional
//! easing and extrapolation control.
//!
//! # Example
//! ```rust
//! use rust_motion_animation::interpolate::{interpolate, InterpolateOptions};
//!
//! // Scale from 0→1 as frame goes 0→30
//! let frame = 15.0_f64;
//! let scale = interpolate(frame, &[0.0, 30.0], &[0.0, 1.0], Default::default());
//! assert!((scale - 0.5).abs() < 1e-10);
//! ```

/// How to handle values outside the input range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtrapolateType {
    /// Extend the linear mapping beyond the range (default).
    #[default]
    Extend,
    /// Clamp to the nearest output value.
    Clamp,
    /// Return the input unchanged when out of range.
    Identity,
    /// Wrap around (modular arithmetic on the input range).
    Wrap,
}

/// Options for [`interpolate`].
#[derive(Clone, Default)]
pub struct InterpolateOptions<'a> {
    /// Optional easing function applied to the normalised `t` inside each segment.
    pub easing: Option<&'a dyn Fn(f64) -> f64>,
    /// Extrapolation strategy for values **below** the first input point.
    pub extrapolate_left: ExtrapolateType,
    /// Extrapolation strategy for values **above** the last input point.
    pub extrapolate_right: ExtrapolateType,
}

/// Maps `input` from `input_range` to `output_range`.
///
/// Equivalent to Remotion's `interpolate(input, inputRange, outputRange, options?)`.
///
/// # Panics
/// Panics if `input_range` and `output_range` have different lengths, or if
/// either has fewer than 2 elements.
pub fn interpolate(
    input: f64,
    input_range: &[f64],
    output_range: &[f64],
    options: InterpolateOptions<'_>,
) -> f64 {
    assert!(
        input_range.len() >= 2,
        "interpolate: input_range must have at least 2 elements"
    );
    assert_eq!(
        input_range.len(),
        output_range.len(),
        "interpolate: input_range and output_range must have the same length"
    );

    let n = input_range.len();

    // ── Find the segment that contains `input` ────────────────────────────────
    let segment = if input <= input_range[0] {
        // Left of first point
        0
    } else if input >= input_range[n - 1] {
        // Right of last point — use the last segment
        n - 2
    } else {
        // Binary search for the segment
        let mut lo = 0usize;
        let mut hi = n - 2;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if input_range[mid + 1] <= input {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    };

    let in_lo = input_range[segment];
    let in_hi = input_range[segment + 1];
    let out_lo = output_range[segment];
    let out_hi = output_range[segment + 1];

    let range = in_hi - in_lo;

    // ── Normalise the input to [0, 1] within the segment ─────────────────────
    let mut t = if range.abs() < f64::EPSILON {
        0.0
    } else {
        (input - in_lo) / range
    };

    // ── Apply extrapolation ───────────────────────────────────────────────────
    let is_left = input < in_lo || (segment == 0 && input <= input_range[0]);
    let is_right = input > in_hi || (segment == n - 2 && input >= input_range[n - 1]);

    let extrapolate = if is_left && t < 0.0 {
        options.extrapolate_left
    } else if is_right && t > 1.0 {
        options.extrapolate_right
    } else {
        ExtrapolateType::Extend
    };

    match extrapolate {
        ExtrapolateType::Clamp => {
            t = t.clamp(0.0, 1.0);
        }
        ExtrapolateType::Identity => {
            return input;
        }
        ExtrapolateType::Wrap => {
            t = t.rem_euclid(1.0);
        }
        ExtrapolateType::Extend => {
            // t stays as-is (can be negative or > 1)
        }
    }

    // ── Apply easing (only within [0, 1]) ────────────────────────────────────
    let t_eased = if let Some(ease) = options.easing {
        if (0.0..=1.0).contains(&t) {
            ease(t)
        } else {
            t
        }
    } else {
        t
    };

    // ── Linear interpolation in output space ─────────────────────────────────
    out_lo + (out_hi - out_lo) * t_eased
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_linear() {
        let v = interpolate(15.0, &[0.0, 30.0], &[0.0, 1.0], Default::default());
        assert!((v - 0.5).abs() < 1e-10, "got {v}");
    }

    #[test]
    fn clamp_left() {
        let opts = InterpolateOptions {
            extrapolate_left: ExtrapolateType::Clamp,
            ..Default::default()
        };
        let v = interpolate(-10.0, &[0.0, 30.0], &[0.0, 1.0], opts);
        assert!((v - 0.0).abs() < 1e-10, "got {v}");
    }

    #[test]
    fn clamp_right() {
        let opts = InterpolateOptions {
            extrapolate_right: ExtrapolateType::Clamp,
            ..Default::default()
        };
        let v = interpolate(60.0, &[0.0, 30.0], &[0.0, 1.0], opts);
        assert!((v - 1.0).abs() < 1e-10, "got {v}");
    }

    #[test]
    fn multi_segment() {
        // Maps 0..30..60 → 0..1..0  (triangle)
        let v = interpolate(
            45.0,
            &[0.0, 30.0, 60.0],
            &[0.0, 1.0, 0.0],
            Default::default(),
        );
        assert!((v - 0.5).abs() < 1e-10, "got {v}");
    }

    #[test]
    fn with_easing() {
        use crate::easing::linear;
        let opts = InterpolateOptions {
            easing: Some(&linear),
            ..Default::default()
        };
        let v = interpolate(15.0, &[0.0, 30.0], &[0.0, 100.0], opts);
        assert!((v - 50.0).abs() < 1e-10, "got {v}");
    }

    #[test]
    fn identity_extrapolate() {
        let opts = InterpolateOptions {
            extrapolate_right: ExtrapolateType::Identity,
            ..Default::default()
        };
        let v = interpolate(999.0, &[0.0, 1.0], &[0.0, 100.0], opts);
        assert!((v - 999.0).abs() < 1e-10, "got {v}");
    }
}
