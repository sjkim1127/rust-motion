//! # rust-motion-algo
//!
//! CPU image filters (Photoshop's Filter menu): blur, sharpen, noise,
//! pixelate, stylize and distort. Every filter is
//!
//! * **depth-agnostic**: works on normalized `f32` samples read from any
//!   [`Surface`] (8/16-bit integer or 32-bit float) in the surface's own
//!   colour model;
//! * **tile/region aware**: [`FilterParams::halo`] declares how far outside an
//!   output tile the filter reads, so results are identical whatever the
//!   tiling (distortions declare [`Halo::Bounds`] and read the whole
//!   reference area);
//! * **selection aware**: output is mixed with the original by the
//!   selection's coverage.
//!
//! [`apply`] runs a filter over an area, tile by tile (in parallel with
//! rayon on native targets; single-threaded on wasm).
#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unimplemented,
        clippy::todo,
        clippy::unreachable
    )
)]

mod artistic;
mod artistic_fx;
mod blur;
mod blur2;
mod box_apply;
pub mod camera_raw;
pub mod color_to_alpha;
pub mod content_aware;
mod denoise;
pub mod dff;
pub mod direct;
mod dispatch;
mod distort;
mod distort2;
pub mod erase;
pub mod exif;
pub mod features;
mod filter_params;
mod fxutil;
mod gallery;
pub mod hdr;
pub mod histogram;
mod image;
pub mod inpaint;
pub mod lens;
pub mod liquify;
pub mod magnetic;
pub mod matting;
mod mix;
mod noise;
pub mod nonlocal;
mod oil;
mod other;
mod other2;
pub mod paint;
pub mod panorama;
mod par;
pub mod perspective;
mod photo_util;
mod pixelate;
pub mod poisson;
pub mod puppet;
pub mod pyramid;
pub mod quantize;
pub mod redeye;
mod relight;
pub mod remove;
mod render;
pub mod render2;
pub mod resample;
pub mod retouch;
pub mod scancrop;
pub mod seam;
pub mod segment;
pub mod selection;
mod selection_blur;
mod shape_blur;
mod sharpen;
pub mod stack;
mod stylize;
mod stylize2;
mod tiling;
pub mod tone;
pub mod transform;
pub mod trap;
mod types;
pub mod vanishing;
pub mod vectorscope;
mod video;
pub mod warp;
pub mod wideangle;

pub use artistic::{
    GalleryEffect, GalleryFilter, GalleryParam, GalleryParamKind, GALLERY_CATEGORIES,
};
pub use image::{Edge, Image};
pub use params_ext::*;

mod params_ext;

#[allow(unused_imports)]
use rust_motion_color::ColorMode;
#[allow(unused_imports)]
use rust_motion_geom::Rect;
#[allow(unused_imports)]
use rust_motion_raster::Surface;

pub(crate) use box_apply::*;
pub use dispatch::*;
pub use filter_params::*;
pub(crate) use mix::*;
pub use tiling::*;
pub use types::*;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_ext;

#[cfg(test)]
mod tests_mosaic;

#[cfg(test)]
mod tests_selection_mix;
