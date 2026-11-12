//! Sparse, copy-on-write tiled surfaces in any [`PixelFormat`].
//!
//! A [`Surface`] is an infinite plane of 256×256 tiles. Missing tiles read as the surface's
//! default pixel (transparent for layers, white for "reveal all" masks). Tiles are `Arc`-shared,
//! so cloning a surface is O(tiles) and mutation copies only the touched tiles. That makes
//! undo snapshots, background jobs and autosave cheap.
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

mod error;
mod image;
mod interrupt;
mod pixel;
mod surface;
mod tile;

#[cfg(test)]
mod tests;

pub use error::AllocationError;
pub use image::{is_hdr, Rgba8Image};
pub use interrupt::{Cancelled, Interrupt};
pub use pixel::{convert_pixel, decode_pixel, encode_pixel, from_rgba, from_rgba_into, to_rgba};
pub use surface::Surface;
pub use tile::Tile;
