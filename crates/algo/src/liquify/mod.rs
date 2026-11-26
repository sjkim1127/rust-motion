//! Filter › Liquify: a dense displacement field edited by brush strokes, then used to resample
//! the layer once.
//!
//! The field stores, for every node of a regular grid (one node per `cell` document pixels), the
//! offset `d` such that output pixel `p` shows the source at `p + d(p)` (an inverse map, so
//! applying it never leaves holes). Strokes are plain data ([`LiquifyStroke`]: tool, brush and
//! points), applied in order by [`LiquifyField::apply_stroke`]; replaying the same strokes on the
//! same bounds gives the same field bit for bit, which is what makes the filter a replayable
//! command and a smart filter.
//!
//! Every tool is written as "where does the new output at `p` come from in the current output":
//! `q = M(p)` (Forward Warp: `p - kΔ`, Twirl: a rotation about the brush centre, Pucker/Bloat: a
//! scaling about it), so the new field is `d'(p) = q + d(q) - p`, sampled bilinearly from the
//! field as it was before the dab. Reconstruct scales `d` toward zero and Smooth relaxes it toward
//! its local mean. Freeze/Thaw paint a mask that scales every tool's strength down.
//!
//! [`apply_liquify`] upsamples the field with Catmull-Rom (bicubic) per pixel and samples the
//! source (premultiplied, bicubic), tile by tile in parallel; tiles where the field is zero are
//! copied untouched. [`ProxyImage::render`] is the cheap preview path: bilinear field and
//! bilinear source on a downsampled RGBA copy, re-rendered only where a dab changed things.

use rust_motion_geom::Rect;
use rust_motion_raster::Surface;
use serde::{Deserialize, Serialize};

mod apply;
mod field;
mod proxy;
#[cfg(test)]
mod tests;
mod types;

pub use apply::*;
pub use field::*;
pub use proxy::*;
pub use types::*;
