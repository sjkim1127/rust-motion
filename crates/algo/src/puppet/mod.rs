//! Edit › Puppet Warp: pins on a triangle mesh over the layer's opaque region, deformed
//! as-rigidly-as-possible.
//!
//! * **Mesh.** The layer's coverage (alpha above a threshold) becomes a signed distance field,
//!   offset by `expansion` to grow or shrink the region. A square grid at the density's spacing,
//!   split into triangles, is fitted to that outline (2D isosurface stuffing): vertices near the
//!   outline snap onto it and the triangles it crosses are cut along it, so the mesh edge follows
//!   the shape, and separate shapes get separate meshes. Inside, the mesh stays a uniform grid,
//!   which is all ARAP needs.
//! * **Deformation.** The per-triangle ARAP energy of Liu et al. 2008 (the triangle form of
//!   Sorkine & Alexa 2007), `E = Σ_t Σ_(i,j)∈t w_ij |(v_i − v_j) − R_t (x_i − x_j)|²` with
//!   cotangent weights, minimized by alternating a local step (best rotation per triangle from
//!   the 2×2 covariance: `atan2(S10 − S01, S00 + S11)`) and a global step (one sparse SPD solve
//!   per axis, Jacobi-preconditioned conjugate gradients, warm-started). Pins are stiff soft
//!   constraints on the barycentric point under them. The first global step uses `R = I`
//!   (a harmonic deformation), which is the classic initialisation. Modes change the local
//!   step: Rigid fits rotations, Distort fits similarities (rotation × scale, i.e. ASAP), and
//!   Normal fits rotation × √scale, in between. A pin's rotation fixes `R` on the triangles
//!   around it. Mesh components without a pin stay where they are.
//! * **Rendering.** Triangles are rasterized with [`crate::warp::warp_triangles`] (tile-parallel,
//!   premultiplied, bilinear/bicubic), back to front by the depth of their nearest pin.

use std::collections::{HashMap, HashSet};

use rust_motion_geom::Rect;
use rust_motion_raster::Surface;
use serde::{Deserialize, Serialize};

use crate::transform::Interp;

mod field;
mod geometry;
mod mesh;
mod solver;
mod sparse;
#[cfg(test)]
mod tests;
mod types;
mod warp;

use field::*;
use geometry::*;
pub use mesh::*;
pub use solver::*;
use sparse::*;
pub use types::*;
pub use warp::*;
