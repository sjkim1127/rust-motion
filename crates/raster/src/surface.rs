//! Sparse, copy-on-write tiled surfaces and region transformations.

use std::collections::BTreeMap;
use std::sync::Arc;

use rust_motion_color::{read_sample, write_sample, ColorMode, PixelFormat, SampleType};
use rust_motion_geom::{Rect, TileCoord, TILE_SIZE};

use crate::error::AllocationError;
use crate::pixel::{convert_pixel, decode_pixel, encode_pixel, to_rgba};
use crate::tile::Tile;

#[derive(Clone, Debug)]
pub struct Surface {
    pub(crate) format: PixelFormat,
    pub(crate) default_pixel: Box<[u8]>,
    pub(crate) tiles: BTreeMap<TileCoord, Arc<Tile>>,
}

/// Tiles of one pixel value met while moving a surface ([`Surface::translated`]): each scanned
/// once, and one tile kept per value so the targets share it.
#[derive(Default)]
struct UniformTiles {
    /// Per source tile (by address): the index of its value's tile in `canonical`, or `None`.
    of: std::collections::HashMap<*const Tile, Option<usize>>,
    canonical: Vec<Arc<Tile>>,
}

impl UniformTiles {
    /// The tile to share for a target reading window `r` of `s`, when every tile under it holds
    /// one and the same pixel value (`r` must lie on existing tiles).
    fn window(&mut self, s: &Surface, r: Rect) -> Option<Arc<Tile>> {
        let bpp = s.format.bytes_per_pixel();
        let mut found = None;
        for c in r.tiles() {
            let t = s.tiles.get(&c)?;
            let k = self.value_of(t, bpp)?;
            if found.is_some_and(|f| f != k) {
                return None;
            }
            found = Some(k);
        }
        self.canonical.get(found?).cloned()
    }

    fn value_of(&mut self, t: &Arc<Tile>, bpp: usize) -> Option<usize> {
        if let Some(k) = self.of.get(&Arc::as_ptr(t)) {
            return *k;
        }
        let mut px = t.data.chunks_exact(bpp);
        let k = px.next().filter(|p0| px.all(|p| p == *p0)).map(|p0| {
            match self
                .canonical
                .iter()
                .position(|c| c.data.get(..bpp) == Some(p0))
            {
                Some(k) => k,
                None => {
                    self.canonical.push(t.clone());
                    self.canonical.len() - 1
                }
            }
        });
        self.of.insert(Arc::as_ptr(t), k);
        k
    }
}

impl PartialEq for Surface {
    fn eq(&self, other: &Self) -> bool {
        self.format == other.format
            && self.default_pixel == other.default_pixel
            && self.tiles == other.tiles
    }
}

impl Surface {
    /// Reads a region into a newly allocated buffer, returning an error instead of aborting if
    /// the requested pixel buffer cannot be represented or reserved.
    pub fn try_read_region(&self, r: Rect) -> Result<Vec<f32>, AllocationError> {
        let len = (r.width() as usize)
            .checked_mul(r.height() as usize)
            .and_then(|n| n.checked_mul(self.channels()))
            .ok_or(AllocationError::SizeOverflow)?;
        let bytes = len
            .checked_mul(std::mem::size_of::<f32>())
            .ok_or(AllocationError::SizeOverflow)?;
        let mut out = Vec::new();
        out.try_reserve_exact(len)
            .map_err(|_| AllocationError::NotEnoughMemory { bytes })?;
        out.resize(len, 0.0);
        self.read_region_into(r, &mut out);
        Ok(out)
    }

    /// Reads a region as encoded interleaved bytes with fallible allocation.
    pub fn try_to_interleaved(&self, r: Rect) -> Result<Vec<u8>, AllocationError> {
        let bpp = self.format.bytes_per_pixel();
        let len = (r.width() as usize)
            .checked_mul(r.height() as usize)
            .and_then(|n| n.checked_mul(bpp))
            .ok_or(AllocationError::SizeOverflow)?;
        let mut out = Vec::new();
        out.try_reserve_exact(len)
            .map_err(|_| AllocationError::NotEnoughMemory { bytes: len })?;
        out.resize(len, 0);
        let w = r.width() as usize;
        for tc in r.tiles() {
            let tr = tc.rect().intersect(&r);
            let span = tr.width() as usize * bpp;
            for y in tr.y0..tr.y1 {
                let dst = (((y - r.y0) as usize) * w + (tr.x0 - r.x0) as usize) * bpp;
                match self.tiles.get(&tc) {
                    Some(t) => {
                        let src = (((y - tc.ty * TILE_SIZE) as usize) * TILE_SIZE as usize
                            + (tr.x0 - tc.tx * TILE_SIZE) as usize)
                            * bpp;
                        out[dst..dst + span].copy_from_slice(&t.data[src..src + span]);
                    }
                    None => {
                        for px in out[dst..dst + span].chunks_exact_mut(bpp) {
                            px.copy_from_slice(&self.default_pixel);
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// Transparent/zero surface.
    pub fn new(format: PixelFormat) -> Self {
        Self {
            format,
            default_pixel: vec![0u8; format.bytes_per_pixel()].into_boxed_slice(),
            tiles: BTreeMap::new(),
        }
    }

    /// Surface whose untouched pixels read as `pixel` (normalised channel values).
    pub fn with_default(format: PixelFormat, pixel: &[f32]) -> Self {
        let mut s = Self::new(format);
        encode_pixel(&format, pixel, &mut s.default_pixel);
        s
    }

    pub fn format(&self) -> PixelFormat {
        self.format
    }
    pub fn channels(&self) -> usize {
        self.format.channels()
    }
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }
    pub fn tiles(&self) -> impl Iterator<Item = (&TileCoord, &Arc<Tile>)> {
        self.tiles.iter()
    }
    pub fn tile(&self, c: TileCoord) -> Option<&Arc<Tile>> {
        self.tiles.get(&c)
    }
    pub fn default_pixel(&self) -> Vec<f32> {
        decode_pixel(&self.format, &self.default_pixel)
    }

    /// Mutable access to a tile, allocating it (filled with the default pixel) or un-sharing it.
    pub fn tile_mut(&mut self, c: TileCoord) -> &mut Tile {
        let fmt = self.format;
        let dp = self.default_pixel.clone();
        let arc = self
            .tiles
            .entry(c)
            .or_insert_with(|| Arc::new(Tile::filled(&fmt, &dp)));
        Arc::make_mut(arc)
    }

    /// Fallible counterpart to [`Surface::tile_mut`]. Pixel storage is reserved before changing
    /// the surface, and a shared tile is copied only after its replacement has been allocated.
    pub fn try_tile_mut(&mut self, c: TileCoord) -> Result<&mut Tile, AllocationError> {
        if !self.tiles.contains_key(&c) {
            let tile = Arc::new(Tile::try_filled(&self.format, &self.default_pixel)?);
            self.tiles.insert(c, tile);
        } else if self.tiles.get(&c).is_some_and(|t| Arc::strong_count(t) > 1) {
            let original = self.tiles.get(&c).ok_or(AllocationError::SizeOverflow)?;
            let tile = Arc::new(original.try_clone()?);
            self.tiles.insert(c, tile);
        }
        self.tiles
            .get_mut(&c)
            .and_then(Arc::get_mut)
            .ok_or(AllocationError::SizeOverflow)
    }

    /// Writes a region using fallible pixel-tile allocations. Callers that need transactional
    /// document updates should perform this on their staged document copy and commit on success.
    pub fn try_write_region(&mut self, r: Rect, data: &[f32]) -> Result<(), AllocationError> {
        let n = self.channels();
        let sample = self.format.sample;
        let expected = (r.width() as usize)
            .checked_mul(r.height() as usize)
            .and_then(|v| v.checked_mul(n))
            .ok_or(AllocationError::SizeOverflow)?;
        if data.len() != expected {
            return Err(AllocationError::SizeOverflow);
        }
        for tc in r.tiles() {
            let tr = tc.rect().intersect(&r);
            let t = self.try_tile_mut(tc)?;
            let span = tr.width() as usize * n;
            for y in tr.y0..tr.y1 {
                let src =
                    (((y - r.y0) as usize) * r.width() as usize + (tr.x0 - r.x0) as usize) * n;
                let dst = (((y - tc.ty * TILE_SIZE) as usize) * TILE_SIZE as usize
                    + (tr.x0 - tc.tx * TILE_SIZE) as usize)
                    * n;
                for (i, v) in data[src..src + span].iter().enumerate() {
                    write_sample(&mut t.data, sample, dst + i, *v);
                }
            }
        }
        Ok(())
    }

    /// Coarse bounds: union of allocated tile rects.
    pub fn tile_bounds(&self) -> Rect {
        self.tiles
            .keys()
            .fold(Rect::EMPTY, |r, c| r.union(&c.rect()))
    }

    /// Exact bounds of pixels that differ from the default pixel.
    pub fn content_bounds(&self) -> Rect {
        if self.tiles.is_empty() {
            return Rect::EMPTY;
        }
        // Scan tiles from the outside of the tile grid inwards; a tile lying entirely inside the
        // bounds found so far cannot extend them and is skipped (so a full-canvas selection scans
        // roughly its outer ring of tiles instead of every pixel).
        let (mut gx0, mut gy0, mut gx1, mut gy1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for c in self.tiles.keys() {
            (gx0, gy0, gx1, gy1) = (gx0.min(c.tx), gy0.min(c.ty), gx1.max(c.tx), gy1.max(c.ty));
        }
        let mut order: Vec<(&TileCoord, &Arc<Tile>)> = self.tiles.iter().collect();
        order.sort_by_key(|(c, _)| (c.tx - gx0).min(gx1 - c.tx).min(c.ty - gy0).min(gy1 - c.ty));
        let mut out = Rect::EMPTY;
        for (c, t) in order {
            let origin = c.rect();
            if !out.is_empty() && out.contains_rect(&origin) {
                continue;
            }
            if let Some((x0, y0, x1, y1)) = t.content_bounds(&self.default_pixel) {
                out = out.union(
                    &Rect::new(x0.into(), y0.into(), x1.into(), y1.into())
                        .translate(origin.x0, origin.y0),
                );
            }
        }
        out
    }

    /// Drop tiles that are entirely the default pixel.
    pub fn prune(&mut self) {
        let dp = self.default_pixel.clone();
        let bpp = dp.len();
        self.tiles
            .retain(|_, t| t.data.chunks_exact(bpp).any(|px| px != &*dp));
    }

    #[inline]
    fn locate(&self, x: i32, y: i32) -> (TileCoord, usize) {
        let c = TileCoord::containing(x, y);
        let lx = x.rem_euclid(TILE_SIZE) as usize;
        let ly = y.rem_euclid(TILE_SIZE) as usize;
        (c, (ly * TILE_SIZE as usize + lx) * self.format.channels())
    }

    /// Read one pixel as normalised channel values (`channels()` floats).
    pub fn pixel(&self, x: i32, y: i32) -> Vec<f32> {
        let mut out = vec![0.0; self.channels()];
        self.read_pixel(x, y, &mut out);
        out
    }

    pub fn read_pixel(&self, x: i32, y: i32, out: &mut [f32]) {
        let (c, base) = self.locate(x, y);
        let n = self.channels();
        match self.tiles.get(&c) {
            Some(t) => {
                for (i, o) in out.iter_mut().enumerate().take(n) {
                    *o = read_sample(&t.data, self.format.sample, base + i);
                }
            }
            None => {
                for (i, o) in out.iter_mut().enumerate().take(n) {
                    *o = read_sample(&self.default_pixel, self.format.sample, i);
                }
            }
        }
    }

    pub fn write_pixel(&mut self, x: i32, y: i32, px: &[f32]) {
        let (c, base) = self.locate(x, y);
        let n = self.channels();
        let sample = self.format.sample;
        let t = self.tile_mut(c);
        for (i, v) in px.iter().enumerate().take(n) {
            write_sample(&mut t.data, sample, base + i, *v);
        }
    }

    /// One channel of one pixel, without allocating.
    #[inline]
    pub fn sample_channel(&self, x: i32, y: i32, c: usize) -> f32 {
        let (tc, base) = self.locate(x, y);
        match self.tiles.get(&tc) {
            Some(t) => read_sample(&t.data, self.format.sample, base + c),
            None => read_sample(&self.default_pixel, self.format.sample, c),
        }
    }

    /// Channel `c` at `(x0 + i·step, y)` for `i in 0..out.len()`, looking each tile up once per run
    /// (much faster than per-sample [`Surface::sample_channel`] for strided scans).
    pub fn sample_row_strided(&self, y: i32, x0: i32, step: i32, c: usize, out: &mut [f32]) {
        let step = step.max(1);
        let dp = read_sample(&self.default_pixel, self.format.sample, c);
        let n = self.format.channels();
        let ty = y.div_euclid(TILE_SIZE);
        let ly = y.rem_euclid(TILE_SIZE) as usize;
        let mut i = 0;
        while i < out.len() {
            let x = x0 + i as i32 * step;
            let tx = x.div_euclid(TILE_SIZE);
            // Samples that fall in this tile.
            let tile_end = (tx + 1) * TILE_SIZE;
            let count = (((tile_end - x) + step - 1) / step).max(1) as usize;
            let end = (i + count).min(out.len());
            match self.tiles.get(&TileCoord { tx, ty }) {
                None => out[i..end].fill(dp),
                Some(t) => {
                    for (k, o) in out[i..end].iter_mut().enumerate() {
                        let lx = (x + k as i32 * step - tx * TILE_SIZE) as usize;
                        *o = read_sample(
                            &t.data,
                            self.format.sample,
                            (ly * TILE_SIZE as usize + lx) * n + c,
                        );
                    }
                }
            }
            i = end;
        }
    }

    /// [`Surface::read_region`] into a reusable buffer (resized to fit).
    pub fn read_region_into(&self, r: Rect, out: &mut Vec<f32>) {
        let n = self.channels();
        let w = r.width() as usize;
        out.clear();
        out.resize(w * r.height() as usize * n, 0.0);
        let dp = self.default_pixel();
        for tc in r.tiles() {
            let tr = tc.rect().intersect(&r);
            let tile = self.tiles.get(&tc);
            for y in tr.y0..tr.y1 {
                let o = (((y - r.y0) as usize) * w + (tr.x0 - r.x0) as usize) * n;
                let span = tr.width() as usize * n;
                let dst = &mut out[o..o + span];
                match tile {
                    Some(t) => {
                        let base = (((y - tc.ty * TILE_SIZE) as usize) * TILE_SIZE as usize
                            + (tr.x0 - tc.tx * TILE_SIZE) as usize)
                            * n;
                        for (i, d) in dst.iter_mut().enumerate() {
                            *d = read_sample(&t.data, self.format.sample, base + i);
                        }
                    }
                    None => {
                        for px in dst.chunks_exact_mut(n) {
                            px.copy_from_slice(&dp);
                        }
                    }
                }
            }
        }
    }

    /// Reads `r` as straight RGBA floats (converting from the surface's
    /// model) into `out` (`w*h` entries), without per-pixel allocation.
    /// Fast paths for RGB(A) and gray(A) at 8/16/32 bits read tile bytes
    /// directly; missing tiles are filled with the default pixel.
    pub fn read_rgba_into(&self, r: Rect, out: &mut [[f32; 4]]) {
        let w = r.width() as usize;
        debug_assert_eq!(out.len(), w * r.height() as usize);
        let fmt = self.format;
        let n = fmt.channels();
        let dp = to_rgba(&fmt, &self.default_pixel());
        for tc in r.tiles() {
            let tr = tc.rect().intersect(&r);
            let tile = self.tiles.get(&tc);
            for y in tr.y0..tr.y1 {
                let o = ((y - r.y0) as usize) * w + (tr.x0 - r.x0) as usize;
                let dst = &mut out[o..o + tr.width() as usize];
                let Some(t) = tile else {
                    dst.fill(dp);
                    continue;
                };
                let base = (((y - tc.ty * TILE_SIZE) as usize) * TILE_SIZE as usize
                    + (tr.x0 - tc.tx * TILE_SIZE) as usize)
                    * n;
                match (fmt.mode, fmt.sample, fmt.alpha) {
                    (ColorMode::Rgb, SampleType::U8, true) => {
                        let src = &t.data[base..base + dst.len() * 4];
                        for (d, s) in dst.iter_mut().zip(src.as_chunks::<4>().0) {
                            *d = [
                                s[0] as f32 / 255.0,
                                s[1] as f32 / 255.0,
                                s[2] as f32 / 255.0,
                                s[3] as f32 / 255.0,
                            ];
                        }
                    }
                    (ColorMode::Grayscale, SampleType::U8, true) => {
                        let src = &t.data[base..base + dst.len() * 2];
                        for (d, s) in dst.iter_mut().zip(src.as_chunks::<2>().0) {
                            let g = s[0] as f32 / 255.0;
                            *d = [g, g, g, s[1] as f32 / 255.0];
                        }
                    }
                    (ColorMode::Cmyk, SampleType::U8, _) => {
                        // Identical ink bytes share the exact ICC result; alpha is independent.
                        let mut previous: Option<([u8; 4], [f32; 3])> = None;
                        let src = &t.data[base..base + dst.len() * n];
                        for (d, s) in dst.iter_mut().zip(src.chunks_exact(n)) {
                            let ink = [s[0], s[1], s[2], s[3]];
                            let rgb = match previous {
                                Some((key, rgb)) if key == ink => rgb,
                                _ => {
                                    let rgb = rust_motion_color::convert::cmyk_to_rgb(
                                        ink.map(|v| v as f32 / 255.0),
                                    );
                                    previous = Some((ink, rgb));
                                    rgb
                                }
                            };
                            *d = [
                                rgb[0],
                                rgb[1],
                                rgb[2],
                                s.get(4).map_or(1.0, |v| *v as f32 / 255.0),
                            ];
                        }
                    }
                    _ => {
                        let mut px = [0.0f32; 8];
                        for (i, d) in dst.iter_mut().enumerate() {
                            for (c, v) in px.iter_mut().enumerate().take(n) {
                                *v = read_sample(&t.data, fmt.sample, base + i * n + c);
                            }
                            *d = to_rgba(&fmt, &px[..n]);
                        }
                    }
                }
            }
        }
    }

    /// Reads `r` as straight 8-bit RGBA into `out` (`w*h` entries). RGBA8 surfaces copy tile bytes
    /// directly; other formats convert through [`Surface::read_rgba_into`] one tile row at a time.
    pub fn read_rgba8_into(&self, r: Rect, out: &mut [[u8; 4]]) {
        let w = r.width() as usize;
        debug_assert_eq!(out.len(), w * r.height() as usize);
        let fmt = self.format;
        let rgba8 = matches!(
            (fmt.mode, fmt.sample, fmt.alpha),
            (ColorMode::Rgb, SampleType::U8, true)
        );
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        let dp = to_rgba(&fmt, &self.default_pixel());
        let dp8 = [q(dp[0]), q(dp[1]), q(dp[2]), q(dp[3])];
        let mut row = Vec::new();
        for tc in r.tiles() {
            let tr = tc.rect().intersect(&r);
            let tile = self.tiles.get(&tc);
            for y in tr.y0..tr.y1 {
                let o = ((y - r.y0) as usize) * w + (tr.x0 - r.x0) as usize;
                let dst = &mut out[o..o + tr.width() as usize];
                match tile {
                    None => dst.fill(dp8),
                    Some(t) if rgba8 => {
                        let base = (((y - tc.ty * TILE_SIZE) as usize) * TILE_SIZE as usize
                            + (tr.x0 - tc.tx * TILE_SIZE) as usize)
                            * 4;
                        let len = dst.len();
                        for (d, s) in dst
                            .iter_mut()
                            .zip(t.data[base..base + len * 4].as_chunks::<4>().0)
                        {
                            *d = [s[0], s[1], s[2], s[3]];
                        }
                    }
                    Some(_) => {
                        row.resize(dst.len(), [0.0f32; 4]);
                        self.read_rgba_into(Rect::new(tr.x0, y, tr.x1, y + 1), &mut row);
                        for (d, p) in dst.iter_mut().zip(&row) {
                            *d = [q(p[0]), q(p[1]), q(p[2]), q(p[3])];
                        }
                    }
                }
            }
        }
    }

    /// `true` if any allocated tile intersects `r` (else `r` reads as the
    /// default pixel everywhere).
    pub fn has_tiles_in(&self, r: Rect) -> bool {
        r.tiles().any(|tc| self.tiles.contains_key(&tc))
    }

    /// Read a rectangle as interleaved normalised floats (`w*h*channels`).
    pub fn read_region(&self, r: Rect) -> Vec<f32> {
        let n = self.channels();
        let w = r.width() as usize;
        let mut out = vec![0.0f32; w * r.height() as usize * n];
        let dp = self.default_pixel();
        for tc in r.tiles() {
            let tr = tc.rect().intersect(&r);
            let tile = self.tiles.get(&tc);
            for y in tr.y0..tr.y1 {
                for x in tr.x0..tr.x1 {
                    let o = (((y - r.y0) as usize) * w + (x - r.x0) as usize) * n;
                    match tile {
                        Some(t) => {
                            let base = (((y - tc.ty * TILE_SIZE) as usize) * TILE_SIZE as usize
                                + (x - tc.tx * TILE_SIZE) as usize)
                                * n;
                            for i in 0..n {
                                out[o + i] = read_sample(&t.data, self.format.sample, base + i);
                            }
                        }
                        None => out[o..o + n].copy_from_slice(&dp),
                    }
                }
            }
        }
        out
    }

    /// Write interleaved normalised floats into a rectangle.
    pub fn write_region(&mut self, r: Rect, data: &[f32]) {
        let n = self.channels();
        let w = r.width() as usize;
        assert_eq!(
            data.len(),
            w * r.height() as usize * n,
            "region data length mismatch"
        );
        let sample = self.format.sample;
        for tc in r.tiles() {
            let tr = tc.rect().intersect(&r);
            let t = self.tile_mut(tc);
            // Convert contiguous channel samples a row at a time. Dispatching on depth here
            // removes the per-pixel channel loop and lets the compiler vectorise conversion.
            let span = tr.width() as usize * n;
            for y in tr.y0..tr.y1 {
                let o = (((y - r.y0) as usize) * w + (tr.x0 - r.x0) as usize) * n;
                let base = (((y - tc.ty * TILE_SIZE) as usize) * TILE_SIZE as usize
                    + (tr.x0 - tc.tx * TILE_SIZE) as usize)
                    * n;
                let src = &data[o..o + span];
                let bytes = sample.bytes();
                let dst = &mut t.data[base * bytes..(base + span) * bytes];
                match sample {
                    SampleType::U8 => {
                        for (d, v) in dst.iter_mut().zip(src) {
                            *d = (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                        }
                    }
                    SampleType::U16 => {
                        for (d, v) in dst.as_chunks_mut::<2>().0.iter_mut().zip(src) {
                            let q = (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
                            d.copy_from_slice(&q.to_ne_bytes());
                        }
                    }
                    SampleType::F32 => {
                        for (d, v) in dst.as_chunks_mut::<4>().0.iter_mut().zip(src) {
                            d.copy_from_slice(&v.to_ne_bytes());
                        }
                    }
                }
            }
        }
    }

    /// Take the tiles covering `r` out of the surface (missing ones filled with the default
    /// pixel), so they can be edited elsewhere, e.g. in parallel: `Arc::unwrap_or_clone` copies a
    /// tile only if an undo snapshot still shares it. Give them back with [`Surface::put_tiles`].
    pub fn take_tiles(&mut self, r: Rect) -> Vec<(TileCoord, Arc<Tile>)> {
        let fmt = self.format;
        let mut blank: Option<Arc<Tile>> = None;
        r.tiles()
            .map(|c| {
                let t = self.tiles.remove(&c).unwrap_or_else(|| {
                    blank
                        .get_or_insert_with(|| Arc::new(Tile::filled(&fmt, &self.default_pixel)))
                        .clone()
                });
                (c, t)
            })
            .collect()
    }

    /// Fallible counterpart to [`Surface::take_tiles`]. Reserves the complete result and any
    /// default tile before removing a tile, so a reservation failure leaves the surface intact.
    pub fn try_take_tiles(
        &mut self,
        r: Rect,
    ) -> Result<Vec<(TileCoord, Arc<Tile>)>, AllocationError> {
        let count = r.tiles().count();
        let bytes = count
            .checked_mul(std::mem::size_of::<(TileCoord, Arc<Tile>)>())
            .ok_or(AllocationError::SizeOverflow)?;
        let mut out = Vec::new();
        out.try_reserve_exact(count)
            .map_err(|_| AllocationError::NotEnoughMemory { bytes })?;
        let has_missing = r.tiles().any(|c| !self.tiles.contains_key(&c));
        let blank = if has_missing {
            Some(Arc::new(Tile::try_filled(
                &self.format,
                &self.default_pixel,
            )?))
        } else {
            None
        };
        for c in r.tiles() {
            let tile = self.tiles.remove(&c).or_else(|| blank.clone());
            if let Some(tile) = tile {
                out.push((c, tile));
            }
        }
        Ok(out)
    }

    /// Put tiles back (see [`Surface::take_tiles`]); each replaces the tile at its coordinate.
    pub fn put_tiles(&mut self, tiles: impl IntoIterator<Item = (TileCoord, Arc<Tile>)>) {
        for (c, t) in tiles {
            self.tiles.insert(c, t);
        }
    }

    /// A tile filled with the default pixel (what a missing tile reads as), allocated fallibly.
    pub fn try_default_tile(&self) -> Result<Tile, AllocationError> {
        Tile::try_filled(&self.format, &self.default_pixel)
    }

    /// A tile of this surface's format filled with `px` (normalised channel values). Sharing one
    /// across many coordinates fills them without copying (tiles are copy-on-write).
    pub fn solid_tile(&self, px: &[f32]) -> Arc<Tile> {
        let mut enc = vec![0u8; self.format.bytes_per_pixel()];
        encode_pixel(&self.format, px, &mut enc);
        Arc::new(Tile::filled(&self.format, &enc))
    }

    /// The default pixel's encoded bytes.
    pub fn default_bytes(&self) -> &[u8] {
        &self.default_pixel
    }

    /// Fill a rectangle with one pixel value.
    pub fn fill_rect(&mut self, r: Rect, px: &[f32]) {
        let n = self.channels();
        let mut enc = vec![0u8; self.format.bytes_per_pixel()];
        encode_pixel(&self.format, &px[..n], &mut enc);
        let bpp = enc.len();
        // Fully covered tiles share one Arc: a solid fill costs one tile however large `r` is,
        // and copy-on-write splits a tile off when it is edited (#705).
        let mut full: Option<Arc<Tile>> = None;
        for tc in r.tiles() {
            let tr = tc.rect().intersect(&r);
            if tr == tc.rect() {
                let fmt = self.format;
                let t = full.get_or_insert_with(|| Arc::new(Tile::filled(&fmt, &enc)));
                self.tiles.insert(tc, t.clone());
                continue;
            }
            let t = self.tile_mut(tc);
            for y in tr.y0..tr.y1 {
                let ly = (y - tc.ty * TILE_SIZE) as usize;
                let lx0 = (tr.x0 - tc.tx * TILE_SIZE) as usize;
                let lx1 = (tr.x1 - tc.tx * TILE_SIZE) as usize;
                let row = &mut t.data
                    [(ly * TILE_SIZE as usize + lx0) * bpp..(ly * TILE_SIZE as usize + lx1) * bpp];
                for p in row.chunks_exact_mut(bpp) {
                    p.copy_from_slice(&enc);
                }
            }
        }
    }

    /// Build from interleaved *encoded* bytes of `format` covering `r`.
    pub fn from_interleaved(format: PixelFormat, r: Rect, bytes: &[u8]) -> Self {
        let mut s = Surface::new(format);
        s.write_interleaved(r, bytes);
        s
    }

    /// Write interleaved encoded bytes (same format as the surface) into `r`.
    pub fn write_interleaved(&mut self, r: Rect, bytes: &[u8]) {
        let bpp = self.format.bytes_per_pixel();
        let w = r.width() as usize;
        assert_eq!(
            bytes.len(),
            w * r.height() as usize * bpp,
            "interleaved length mismatch"
        );
        for tc in r.tiles() {
            let tr = tc.rect().intersect(&r);
            let t = self.tile_mut(tc);
            let span = tr.width() as usize * bpp;
            for y in tr.y0..tr.y1 {
                let src = (((y - r.y0) as usize) * w + (tr.x0 - r.x0) as usize) * bpp;
                let dst = (((y - tc.ty * TILE_SIZE) as usize) * TILE_SIZE as usize
                    + (tr.x0 - tc.tx * TILE_SIZE) as usize)
                    * bpp;
                t.data[dst..dst + span].copy_from_slice(&bytes[src..src + span]);
            }
        }
    }

    /// Read `r` as interleaved encoded bytes in the surface's format.
    pub fn to_interleaved(&self, r: Rect) -> Vec<u8> {
        let bpp = self.format.bytes_per_pixel();
        let w = r.width() as usize;
        let mut out = vec![0u8; w * r.height() as usize * bpp];
        for tc in r.tiles() {
            let tr = tc.rect().intersect(&r);
            let span = tr.width() as usize * bpp;
            for y in tr.y0..tr.y1 {
                let dst = (((y - r.y0) as usize) * w + (tr.x0 - r.x0) as usize) * bpp;
                match self.tiles.get(&tc) {
                    Some(t) => {
                        let src = (((y - tc.ty * TILE_SIZE) as usize) * TILE_SIZE as usize
                            + (tr.x0 - tc.tx * TILE_SIZE) as usize)
                            * bpp;
                        out[dst..dst + span].copy_from_slice(&t.data[src..src + span]);
                    }
                    None => {
                        for p in out[dst..dst + span].chunks_exact_mut(bpp) {
                            p.copy_from_slice(&self.default_pixel);
                        }
                    }
                }
            }
        }
        out
    }

    /// The surface moved by whole pixels: its encoded samples copied tile to tile (no float
    /// round trip, no intermediate buffer); moves by whole tiles share the tiles themselves, and
    /// a target tile read entirely from tiles of one single pixel value shares one such tile (the
    /// inside of a solid fill stays one copy-on-write tile however often it moves). A tile it
    /// copies that ends up holding only the default pixel isn't stored.
    /// `content` must hold every pixel that isn't the default one (`content_bounds`, say): only
    /// tiles it reaches are kept.
    pub fn translated(&self, dx: i32, dy: i32, content: Rect) -> Surface {
        let mut out = Surface {
            format: self.format,
            default_pixel: self.default_pixel.clone(),
            tiles: BTreeMap::new(),
        };
        let sources = || {
            self.tiles
                .iter()
                .filter(move |(c, _)| !c.rect().intersect(&content).is_empty())
        };
        if dx % TILE_SIZE == 0 && dy % TILE_SIZE == 0 {
            let (tx, ty) = (dx / TILE_SIZE, dy / TILE_SIZE);
            out.tiles = sources()
                .map(|(c, t)| {
                    (
                        TileCoord::new(c.tx.saturating_add(tx), c.ty.saturating_add(ty)),
                        t.clone(),
                    )
                })
                .collect();
            return out;
        }
        let bpp = self.format.bytes_per_pixel();
        let moved = content.translate(dx, dy);
        let targets: std::collections::BTreeSet<TileCoord> = sources()
            .flat_map(|(c, _)| c.rect().translate(dx, dy).intersect(&moved).tiles())
            .collect();
        let mut uniform = UniformTiles::default();
        // The fill row: one tile row of the default pixel, so the regions a shift vacates
        // fill with bulk copies (empty when the default is zero — the tile starts zeroed).

        // One row of the default pixel, so the regions a shift vacates fill with bulk copies.
        let fill_row: Vec<u8> = if self.default_pixel.iter().any(|&b| b != 0) {
            let mut row = Vec::with_capacity(TILE_SIZE as usize * bpp);
            for _ in 0..TILE_SIZE {
                row.extend_from_slice(&self.default_pixel);
            }
            row
        } else {
            Vec::new()
        };

        // One tile row of the default pixel, to compare a finished tile with a row at a time.
        let default_row = self.default_pixel.repeat(TILE_SIZE.unsigned_abs() as usize);
        for dc in targets {
            let dr = dc.rect();
            let sr = dr.translate(dx.saturating_neg(), dy.saturating_neg());
            // Every pixel of the target comes from a source tile (none from the default pixel).
            // `sr` must be the exact preimage: an offset of `i32::MIN` saturates its negation.
            let covered = sr.width() == TILE_SIZE.unsigned_abs()
                && sr.height() == TILE_SIZE.unsigned_abs()
                && sr.translate(dx, dy) == dr
                && content.contains_rect(&sr)
                && sr.tiles().all(|sc| self.tiles.contains_key(&sc));
            if covered {
                if let Some(shared) = uniform.window(self, sr) {
                    // A tile of only the default pixel is left out, as `prune` would.
                    if shared.data.get(..bpp) != Some(&*self.default_pixel) {
                        out.tiles.insert(dc, shared);
                    }
                    continue;
                }
            }

            let mut tile = Tile {
                data: vec![0u8; (TILE_SIZE * TILE_SIZE) as usize * bpp].into_boxed_slice(),
            };
            let (tx0, ty0) = (
                i64::from(dc.tx) * i64::from(TILE_SIZE),
                i64::from(dc.ty) * i64::from(TILE_SIZE),
            );
            let row_len = TILE_SIZE as usize * bpp;
            // Per stored row: copy the source span that falls inside the content in as few
            // memcpys as the tile grid allows, and fill the vacated edges from the default
            // row — no whole-tile pre-fill, which rewrote nearly every byte twice.
            for local_y in 0..i64::from(TILE_SIZE) {
                let y = ty0 + local_y;
                let Some(row) = tile
                    .data
                    .get_mut(local_y as usize * row_len..(local_y as usize + 1) * row_len)
                else {
                    continue;
                };
                let in_content_y = y - i64::from(dy) >= i64::from(content.y0)
                    && y - i64::from(dy) < i64::from(content.y1);
                // The row's target columns whose SOURCE lies inside the content are copied
                // (the copy loop crosses source tiles as needed); the rest stays default —
                // exactly the pixels the shift vacated.
                let cx0 = (i64::from(content.x0) + i64::from(dx)).max(tx0);
                let cx1 = (i64::from(content.x1) + i64::from(dx)).min(tx0 + i64::from(TILE_SIZE));
                if !in_content_y || cx1 <= cx0 {
                    if !fill_row.is_empty() {
                        row.copy_from_slice(&fill_row);
                    }
                    continue;
                }
                let left = (cx0.max(tx0) - tx0) as usize * bpp;
                let copied_end = (cx1.min(tx0 + i64::from(TILE_SIZE)) - tx0) as usize * bpp;
                if !fill_row.is_empty() {
                    if left > 0 {
                        row[..left].copy_from_slice(&fill_row[..left]);
                    }
                    if copied_end < row_len {
                        row[copied_end..].copy_from_slice(&fill_row[copied_end..]);
                    }
                }
                let mut x = cx0.max(tx0);
                let x_end = cx1.min(tx0 + i64::from(TILE_SIZE));
                // Rows run over the same one or two source tiles: cache the last lookup.
                let mut cached: Option<(i64, i64, &std::sync::Arc<Tile>)> = None;
                while x < x_end {
                    // The source document position this target column reads, and the source
                    // tile it lives in (the shift crosses source tiles as x runs).
                    let (sx, sy) = (x - i64::from(dx), y - i64::from(dy));
                    let (tx, ty) = (
                        sx.div_euclid(i64::from(TILE_SIZE)),
                        sy.div_euclid(i64::from(TILE_SIZE)),
                    );
                    // A missing source tile inside the content holds the default pixel, not
                    // zeros: fill the span from the default row instead of skipping it
                    // (echelon's white-mask-with-sparse-tiles case).
                    let st = match cached.take() {
                        Some((ctx, cty, t)) if ctx == tx && cty == ty => {
                            cached = Some((ctx, cty, t));
                            t
                        }
                        _ => match self.tiles.get(&TileCoord::new(tx as i32, ty as i32)) {
                            Some(t) => {
                                cached = Some((tx, ty, t));
                                t
                            }
                            None => {
                                let span_end = ((tx + 1) * i64::from(TILE_SIZE) + i64::from(dx))
                                    .min(x_end)
                                    .max(x + 1);
                                if !fill_row.is_empty() {
                                    let at = (x - tx0) as usize * bpp;
                                    let len = (span_end - x) as usize * bpp;
                                    if let Some(d) = row.get_mut(at..at + len) {
                                        d.copy_from_slice(&fill_row[at..at + len]);
                                    }
                                }
                                x = span_end;
                                continue;
                            }
                        },
                    };
                    let span_end =
                        (((tx + 1) * i64::from(TILE_SIZE) + i64::from(dx)).min(x_end)).max(x + 1);
                    let span = (span_end - x) as usize * bpp;
                    let src = ((sy - ty * i64::from(TILE_SIZE)) * row_len as i64
                        + (sx - tx * i64::from(TILE_SIZE)) * bpp as i64)
                        as usize;
                    let dst = (x - tx0) as usize * bpp;
                    if let (Some(d), Some(sr)) =
                        (row.get_mut(dst..dst + span), st.data.get(src..src + span))
                    {
                        d.copy_from_slice(sr);
                    }
                    x = span_end;
                }
            }
            // A target that got only default pixels isn't stored, as `prune` would drop it. Checked
            // after the copy, a row at a time: a tile with content stops at its first such row.
            if tile
                .data
                .chunks(default_row.len())
                .any(|row| row != default_row)
            {
                out.tiles.insert(dc, Arc::new(tile));
            }
        }
        out
    }

    /// Applies `f` to every colour sample (not alpha), including the default pixel: a tone-curve
    /// change such as linearising when a document becomes 32-bit.
    pub fn map_color(&mut self, f: impl Fn(f32) -> f32) {
        let format = self.format;
        let color = format.mode.color_channels();
        let n = format.channels();
        let mut dp = decode_pixel(&format, &self.default_pixel);
        for v in dp.iter_mut().take(color) {
            *v = f(*v);
        }
        encode_pixel(&format, &dp, &mut self.default_pixel);
        let coords: Vec<TileCoord> = self.tiles.keys().copied().collect();
        let px_count = (TILE_SIZE * TILE_SIZE) as usize;
        // A tile shared by several coordinates (a filled background) is mapped once and stays
        // shared (#4531). Keys are the original tiles, all alive until their coordinates are
        // reached, so a freed address can't be mistaken for another original.
        let mut done: std::collections::HashMap<*const Tile, Arc<Tile>> =
            std::collections::HashMap::new();
        for c in coords {
            let Some(key) = self.tiles.get(&c).map(Arc::as_ptr) else {
                continue;
            };
            if let Some(mapped) = done.get(&key) {
                self.tiles.insert(c, mapped.clone());
                continue;
            }
            let tile = self.tile_mut(c);
            for i in 0..px_count {
                for k in 0..color {
                    let at = i * n + k;
                    let v = read_sample(&tile.data, format.sample, at);
                    write_sample(&mut tile.data, format.sample, at, f(v));
                }
            }
            if let Some(mapped) = self.tiles.get(&c) {
                done.insert(key, mapped.clone());
            }
        }
    }

    /// Convert to another pixel format (depth and/or model; colour models via [`convert_pixel`]).
    pub fn convert(&self, to: PixelFormat) -> Surface {
        let mut out =
            Surface::with_default(to, &convert_pixel(&self.format, &to, &self.default_pixel()));
        let from_n = self.channels();
        let mut src = vec![0.0f32; from_n];
        // Each distinct tile converts once; coordinates sharing it share the result (#4531).
        let mut done: std::collections::HashMap<*const Tile, Arc<Tile>> =
            std::collections::HashMap::new();
        for (c, t) in &self.tiles {
            if let Some(converted) = done.get(&Arc::as_ptr(t)) {
                out.tiles.insert(*c, converted.clone());
                continue;
            }
            let dst = out.tile_mut(*c);
            let px_count = (TILE_SIZE * TILE_SIZE) as usize;
            for i in 0..px_count {
                for (k, v) in src.iter_mut().enumerate() {
                    *v = read_sample(&t.data, self.format.sample, i * from_n + k);
                }
                let px = convert_pixel(&self.format, &to, &src);
                for (k, v) in px.iter().enumerate() {
                    write_sample(&mut dst.data, to.sample, i * to.channels() + k, *v);
                }
            }
            if let Some(converted) = out.tiles.get(c) {
                done.insert(Arc::as_ptr(t), converted.clone());
            }
        }
        out
    }

    /// Convert a surface while reserving each replacement tile fallibly.
    pub fn try_convert(&self, to: PixelFormat) -> Result<Surface, AllocationError> {
        let mut out =
            Surface::with_default(to, &convert_pixel(&self.format, &to, &self.default_pixel()));
        let from_n = self.channels();
        let mut src = Vec::new();
        src.try_reserve_exact(from_n)
            .map_err(|_| AllocationError::NotEnoughMemory {
                bytes: from_n * std::mem::size_of::<f32>(),
            })?;
        src.resize(from_n, 0.0f32);
        let mut done: std::collections::HashMap<*const Tile, Arc<Tile>> =
            std::collections::HashMap::new();
        for (c, t) in &self.tiles {
            if let Some(converted) = done.get(&Arc::as_ptr(t)) {
                out.tiles.insert(*c, converted.clone());
                continue;
            }
            let dst = out.try_tile_mut(*c)?;
            let px_count = (TILE_SIZE * TILE_SIZE) as usize;
            for i in 0..px_count {
                for (k, v) in src.iter_mut().enumerate() {
                    *v = read_sample(&t.data, self.format.sample, i * from_n + k);
                }
                let px = convert_pixel(&self.format, &to, &src);
                for (k, v) in px.iter().enumerate() {
                    write_sample(&mut dst.data, to.sample, i * to.channels() + k, *v);
                }
            }
            if let Some(converted) = out.tiles.get(c) {
                done.insert(Arc::as_ptr(t), converted.clone());
            }
        }
        Ok(out)
    }

    /// Composite-friendly RGBA read (converts from the surface's model).
    pub fn rgba(&self, x: i32, y: i32) -> [f32; 4] {
        let p = self.pixel(x, y);
        to_rgba(&self.format, &p)
    }
}
