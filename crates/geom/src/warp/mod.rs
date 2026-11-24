//! Warp geometry shared by Edit › Transform › Warp, smart-object warps and Warp Text.
//!
//! Two kinds of warp, both forward maps from a source box to document space:
//! - **Styles** ([`WarpStyle`], Photoshop's fifteen presets plus bend and horizontal/vertical
//!   distortion, the PSD `warp` descriptor's `warpStyle`/`warpValue`/`warpPerspective*`). Each
//!   is a single bicubic Bezier patch whose control grid follows the bend: measured from
//!   Photoshop's renders (see [`preset_grid`]), not taken from Adobe's code. Every style is the
//!   identity at bend 0 and continuous in the bend.
//! - **Custom meshes** ([`BezierMesh`]): a grid of bicubic Bezier patches. The default is one
//!   patch (a 4×4 control grid); split warps add patch rows/columns by exact de Casteljau
//!   subdivision, so splitting never changes the shape.
//!
//! Pure geometry: resampling lives in `rust-motion-algo::warp`.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

mod curve;
mod edit;
mod map;
mod mesh;
mod style;
mod style_warp;
#[cfg(test)]
mod tests;

use curve::*;
pub use map::*;
pub use mesh::*;
pub use style::*;
pub use style_warp::*;
