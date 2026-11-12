//! Pixel storage and content boundary scanning for individual tiles.

use crate::error::AllocationError;
use rust_motion_color::PixelFormat;
use rust_motion_geom::TILE_SIZE;

/// Pixel storage for one tile: `TILE_SIZE² × bytes_per_pixel`, row-major, interleaved channels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    pub(crate) data: Box<[u8]>,
}

impl Tile {
    pub(crate) fn try_filled(format: &PixelFormat, pixel: &[u8]) -> Result<Self, AllocationError> {
        let n = (TILE_SIZE * TILE_SIZE) as usize;
        let bytes = n
            .checked_mul(format.bytes_per_pixel())
            .ok_or(AllocationError::SizeOverflow)?;
        let mut data = Vec::new();
        data.try_reserve_exact(bytes)
            .map_err(|_| AllocationError::NotEnoughMemory { bytes })?;
        data.resize(bytes, 0);
        if pixel.iter().any(|&b| b != 0) {
            for px in data.chunks_exact_mut(pixel.len()) {
                px.copy_from_slice(pixel);
            }
        }
        Ok(Tile {
            data: data.into_boxed_slice(),
        })
    }

    /// Clone this tile's pixel buffer with a fallible reservation.
    pub fn try_clone(&self) -> Result<Self, AllocationError> {
        let bytes = self.data.len();
        let mut data = Vec::new();
        data.try_reserve_exact(bytes)
            .map_err(|_| AllocationError::NotEnoughMemory { bytes })?;
        data.extend_from_slice(&self.data);
        Ok(Tile {
            data: data.into_boxed_slice(),
        })
    }

    pub(crate) fn filled(format: &PixelFormat, pixel: &[u8]) -> Self {
        let row_len = TILE_SIZE as usize * format.bytes_per_pixel();
        let mut data = vec![0u8; row_len * TILE_SIZE as usize];
        if pixel.iter().any(|&b| b != 0) {
            // One prebuilt row copied per tile row: 256 bulk copies instead of one
            // copy_from_slice per pixel.
            let mut row = Vec::with_capacity(row_len);
            for _ in 0..TILE_SIZE {
                row.extend_from_slice(pixel);
            }
            for out in data.chunks_mut(row_len) {
                out.copy_from_slice(&row);
            }
        }
        Tile {
            data: data.into_boxed_slice(),
        }
    }
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Tile-local half-open bounds `(x0, y0, x1, y1)` of the pixels that differ from the encoded
    /// pixel `dp` (one pixel of the tile's format), or `None` when every pixel equals it (or `dp`
    /// is empty).
    pub fn content_bounds(&self, dp: &[u8]) -> Option<(u16, u16, u16, u16)> {
        let bpp = dp.len();
        if bpp == 0 {
            return None;
        }
        let ts = TILE_SIZE as usize;
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
        for (y, row) in self.data.chunks_exact(ts * bpp).enumerate() {
            let Some(first) = row.chunks_exact(bpp).position(|px| px != dp) else {
                continue;
            };
            let last = ts
                - 1
                - row
                    .chunks_exact(bpp)
                    .rev()
                    .position(|px| px != dp)
                    .unwrap_or(0);
            x0 = x0.min(first);
            x1 = x1.max(last + 1);
            y0 = y0.min(y);
            y1 = y + 1;
        }
        (x0 != usize::MAX).then_some((x0 as u16, y0 as u16, x1 as u16, y1 as u16))
    }
}
