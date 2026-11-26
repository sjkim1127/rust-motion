//! Selection algorithms on coverage masks (`w × h` row-major `f32` in
//! `0..=1` over an area): magic wand / flood regions, colour range,
//! modify (expand, contract, border, smooth, feather), anti-aliased lasso,
//! and combining with an existing selection.

use std::sync::Arc;

use rust_motion_color::PixelFormat;
use rust_motion_geom::{Rect, TileCoord, TILE_SIZE};
use rust_motion_raster::Surface;
use serde::{Deserialize, Serialize};

use crate::photo_util::{par_map, par_rows};

mod color_range;
mod distance;
mod lab_range;
mod marquee;
mod mask;
mod modify;
mod polygon;
#[cfg(test)]
mod tests;
mod wand;

pub use color_range::*;
pub use distance::edt;
pub use lab_range::*;
pub use marquee::{combine_surfaces, ellipse_surface};
pub use mask::*;
pub use modify::*;
pub use polygon::*;
pub use wand::*;
