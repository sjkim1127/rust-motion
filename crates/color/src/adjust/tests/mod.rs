//! Tests for the adjustment engine.

use super::*;

mod ink;
mod lookup;
mod lut3d_guard;
mod tone;

pub(super) fn near(got: [f32; 3], want: [f32; 3], tol: f32) -> bool {
    got.iter().zip(want).all(|(g, w)| (g - w).abs() <= tol)
}
