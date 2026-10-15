//! # rust-motion-animation
//!
//! Remotion-compatible animation primitives for Rust Motion.
//!
//! Provides:
//! - [`interpolate`] — maps a value from one range to another (Remotion `interpolate()`)
//! - [`spring`]       — physics-based spring animation (Remotion `spring()`)
//! - [`easing`]       — common easing functions (Remotion `Easing`)
//! - [`interpolate_colors`] — color interpolation
//! - [`transform`]    — CSS transform builder (`makeTransform`, `interpolateStyles`)

pub mod chop;
pub mod easing;
pub mod interpolate;
pub mod interpolate_colors;
pub mod keyframe;
pub mod random;
pub mod spring;
pub mod transform;
pub mod typography;

pub use chop::{AudioEnvelopeChop, LagChop, LfoChop, LfoWave, MathChop, MathOp};
pub use easing::{bezier, EasingFn};
pub use interpolate::{interpolate, ExtrapolateType, InterpolateOptions};
pub use interpolate_colors::{interpolate_colors, interpolate_colors_range};
pub use keyframe::{
    solve_ae_bezier_1d, solve_ae_bezier_derivative_1d, Animatable, InterpolationType, Keyframe,
    KeyframeEase, KeyframeTrack,
};
pub use random::{random, RandomSeed};
pub use spring::{
    measure_spring, spring, spring_with_options, SpringConfig, SpringError, SpringOptions,
};
pub use transform::{
    interpolate_styles, make_transform, matrix, matrix3d, perspective, rotate, rotate3d, rotate_x,
    rotate_y, rotate_z, scale, scale3d, scale_x, scale_y, scale_z, skew, skew_x, skew_y, translate,
    translate3d, translate_x, translate_y, translate_z, StyleMap, StyleValue, TransformOp,
};
pub use typography::{scramble_text, typewriter_text, typewriter_words};
