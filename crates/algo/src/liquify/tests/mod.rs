//! Tests for Liquify.

use super::*;
use rust_motion_color::{ColorMode, PixelFormat, SampleType};

mod lasso;
mod render;
mod tools;
mod walk;

pub(super) fn bounds() -> Rect {
    Rect::new(0, 0, 96, 64)
}
