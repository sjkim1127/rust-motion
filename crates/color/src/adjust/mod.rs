//! Adjustment layers data structures and evaluation engine.
//!
//! Split by concern: [`types`] holds the data, [`apply`] the evaluation entry points, and the
//! remaining modules the per-adjustment maths. Everything public is re-exported here, so callers
//! keep using `adjust::Adjustment`, `adjust::apply_depth`, ….

#[allow(unused_imports)]
use crate::convert::{
    cmyk_to_rgb, lab_to_srgb, linear_to_srgb, mat3_mul, rgb_to_cmyk, rgb_to_gray, srgb_to_lab,
    srgb_to_linear, D50, SRGB_TO_XYZ_D50, XYZ_D50_TO_SRGB,
};
use crate::{ColorMode, SampleType};

mod apply;
mod balance;
mod brightness;
mod curves;
mod gradient;
mod hue;
mod lut;
mod modes;
#[cfg(test)]
mod tests;
mod tone;
mod types;

pub use apply::*;
pub use balance::*;
pub use brightness::*;
pub use curves::*;
pub use gradient::*;
pub use hue::*;
pub use lut::*;
pub use modes::*;
pub use tone::*;
pub use types::*;
