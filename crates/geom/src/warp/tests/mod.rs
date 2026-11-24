//! Tests for the warp geometry.

use super::*;

mod mesh;
mod presets;
mod pull;

pub(super) const B: [f64; 4] = [10.0, 20.0, 110.0, 70.0];

pub(super) fn close(a: (f64, f64), b: (f64, f64), eps: f64) -> bool {
    (a.0 - b.0).abs() < eps && (a.1 - b.1).abs() < eps
}
