//! Magic wand: flood regions, region combining and their anti-aliasing.

use super::*;

/// Pixels similar to the seed pixel (per-channel difference ≤ `tolerance`
/// levels, alpha included; fully transparent pixels match regardless of hidden RGB).
/// `contiguous` limits to the 4-connected region
/// around the seed. `px` covers `area`.
pub fn magic_wand(
    px: &[[f32; 4]],
    area: Rect,
    seed: (i32, i32),
    tolerance: f32,
    contiguous: bool,
    anti_alias: bool,
) -> Vec<f32> {
    let (w, h) = (area.width() as usize, area.height() as usize);
    let Some(n) = w.checked_mul(h).filter(|&n| n <= px.len()) else {
        return Vec::new();
    };
    let mut m = vec![0.0f32; n];
    if !area.contains(seed.0, seed.1) {
        return m;
    }
    let si = (seed.1 - area.y0) as usize * w + (seed.0 - area.x0) as usize;
    let target = px[si];
    if contiguous {
        let mut stack = vec![si];
        m[si] = 1.0;
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            let mut visit = |j: usize| {
                if m[j] == 0.0 && close(px[j], target, tolerance) {
                    m[j] = 1.0;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
    } else {
        for (v, p) in m.iter_mut().zip(px) {
            if close(*p, target, tolerance) {
                *v = 1.0;
            }
        }
    }
    if anti_alias {
        antialias(&mut m, w, h);
    }
    m
}

/// Straight RGBA8 copy of `area` of a surface, read in parallel bands on native.
pub fn rgba8_image(s: &Surface, area: Rect) -> Vec<[u8; 4]> {
    let (w, h) = (area.width() as usize, area.height() as usize);
    let mut img = vec![[0u8; 4]; w * h];
    if w == 0 || h == 0 {
        return img;
    }
    let band = 256usize;
    let read = |bi: usize, chunk: &mut [[u8; 4]]| {
        let y0 = area.y0 + (bi * band) as i32;
        s.read_rgba8_into(
            Rect::new(area.x0, y0, area.x1, y0 + (chunk.len() / w) as i32),
            chunk,
        );
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        img.par_chunks_mut(w * band)
            .enumerate()
            .for_each(|(bi, c)| read(bi, c));
    }
    #[cfg(target_arch = "wasm32")]
    img.chunks_mut(w * band)
        .enumerate()
        .for_each(|(bi, c)| read(bi, c));
    img
}

/// A selected region: 8-bit coverage over `bbox` (row-major), the bounding box of the pixels
/// touched (including the anti-aliased fringe).
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub bbox: Rect,
    pub mask: Vec<u8>,
}

impl Region {
    /// Coverage (0–1) at document pixel `(x, y)`; zero outside the box.
    pub fn at(&self, x: i32, y: i32) -> f32 {
        if !self.bbox.contains(x, y) {
            return 0.0;
        }
        let row = (y.abs_diff(self.bbox.y0) as usize).checked_mul(self.bbox.width() as usize);
        let value = row
            .and_then(|i| i.checked_add(x.abs_diff(self.bbox.x0) as usize))
            .and_then(|i| self.mask.get(i))
            .copied()
            .unwrap_or(0);
        f32::from(value) / 255.0
    }
}

/// Magic wand on 8-bit pixels: a scanline flood fill (or a global match) that only allocates a
/// byte mark per pixel, then anti-aliases just the edges inside the touched bounding box.
/// Matches [`magic_wand`] for 8-bit sources. `None` if the seed is outside `area`.
pub fn wand_region(
    img: &[[u8; 4]],
    area: Rect,
    seed: (i32, i32),
    tolerance: f32,
    contiguous: bool,
    anti_alias: bool,
) -> Option<Region> {
    if !area.contains(seed.0, seed.1) {
        return None;
    }
    let (w, h) = (area.width() as usize, area.height() as usize);
    if w.checked_mul(h).is_none_or(|n| n > img.len()) {
        return None;
    }
    let target = img[(seed.1 - area.y0) as usize * w + (seed.0 - area.x0) as usize];
    let tol = (tolerance + 1e-3).floor().max(0.0) as i32;
    // Copying a selection can leave RGB under zero alpha. Those invisible colours must not
    // split a transparent region into pieces, as if the copied source were still visible.
    let similar = |p: [u8; 4]| {
        (target[3] == 0 && p[3] == 0)
            || (0..4).all(|c| (p[c] as i32 - target[c] as i32).abs() <= tol)
    };
    let mut marks = vec![0u8; w * h];
    let (mut bx0, mut by0, mut bx1, mut by1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    if contiguous {
        // Span fill: fill a run, then scan the rows above and below it for new seeds.
        let mut stack = vec![((seed.0 - area.x0) as usize, (seed.1 - area.y0) as usize)];
        while let Some((x, y)) = stack.pop() {
            let row = y * w;
            if marks[row + x] != 0 {
                continue;
            }
            let (mut l, mut r) = (x, x);
            while l > 0 && marks[row + l - 1] == 0 && similar(img[row + l - 1]) {
                l -= 1;
            }
            while r + 1 < w && marks[row + r + 1] == 0 && similar(img[row + r + 1]) {
                r += 1;
            }
            marks[row + l..=row + r].fill(255);
            (bx0, bx1, by0, by1) = (bx0.min(l), bx1.max(r), by0.min(y), by1.max(y));
            for ny in [y.wrapping_sub(1), y + 1] {
                if ny >= h {
                    continue;
                }
                let nrow = ny * w;
                let mut in_run = false;
                for nx in l..=r {
                    let ok = marks[nrow + nx] == 0 && similar(img[nrow + nx]);
                    if ok && !in_run {
                        stack.push((nx, ny));
                    }
                    in_run = ok;
                }
            }
        }
    } else {
        let scan = |y: usize, row: &mut [u8]| -> Option<(usize, usize)> {
            let mut span: Option<(usize, usize)> = None;
            for (x, m) in row.iter_mut().enumerate() {
                if similar(img[y * w + x]) {
                    *m = 255;
                    span = Some(span.map_or((x, x), |(a, _)| (a, x)));
                }
            }
            span
        };
        #[cfg(not(target_arch = "wasm32"))]
        let spans: Vec<Option<(usize, usize)>> = {
            use rayon::prelude::*;
            marks
                .par_chunks_mut(w)
                .enumerate()
                .map(|(y, row)| scan(y, row))
                .collect()
        };
        #[cfg(target_arch = "wasm32")]
        let spans: Vec<Option<(usize, usize)>> = marks
            .chunks_mut(w)
            .enumerate()
            .map(|(y, row)| scan(y, row))
            .collect();
        for (y, sp) in spans.iter().enumerate() {
            if let Some((a, b)) = sp {
                (bx0, bx1, by0, by1) = (bx0.min(*a), bx1.max(*b), by0.min(y), by1.max(y));
            }
        }
    }
    if bx0 == usize::MAX {
        return None;
    }
    // One pixel of fringe for anti-aliasing, clamped to the area.
    let pad = anti_alias as usize;
    let (x0, y0) = (bx0.saturating_sub(pad), by0.saturating_sub(pad));
    let (x1, y1) = ((bx1 + 1 + pad).min(w), (by1 + 1 + pad).min(h));
    let bw = x1 - x0;
    let at = |x: usize, y: usize| marks[y * w + x];
    let mut mask = vec![0u8; bw * (y1 - y0)];
    let fill_row = |yy: usize, out: &mut [u8]| {
        let y = y0 + yy;
        for (xx, o) in out.iter_mut().enumerate() {
            let x = x0 + xx;
            let v = at(x, y);
            *o = v;
            if !anti_alias {
                continue;
            }
            // Same rule as `antialias`: edge pixels blend with their 3×3 neighbourhood; neighbours
            // outside the area count as the pixel itself.
            let mut sum = 0u32;
            let mut edge = false;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    let n = if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        v
                    } else {
                        at(nx as usize, ny as usize)
                    };
                    edge |= n != v;
                    sum += n as u32;
                }
            }
            if edge {
                let avg = sum as f32 / (9.0 * 255.0);
                let f = if v > 127 { 0.5 + 0.5 * avg } else { 0.5 * avg };
                *o = (f * 255.0).round() as u8;
            }
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        mask.par_chunks_mut(bw)
            .enumerate()
            .for_each(|(yy, row)| fill_row(yy, row));
    }
    #[cfg(target_arch = "wasm32")]
    mask.chunks_mut(bw)
        .enumerate()
        .for_each(|(yy, row)| fill_row(yy, row));
    let bbox = Rect::new(
        area.x0 + x0 as i32,
        area.y0 + y0 as i32,
        area.x0 + x1 as i32,
        area.y0 + y1 as i32,
    );
    Some(Region { bbox, mask })
}

/// Combines a [`Region`] with the current selection, touching only the region's box (outside it
/// the new mask is zero). Returns `None` when nothing remains selected.
pub fn combine_region(
    old: Option<&Surface>,
    new: Option<&Region>,
    mode: SelectionMode,
) -> Option<Surface> {
    let empty = Region {
        bbox: Rect::EMPTY,
        mask: Vec::new(),
    };
    let new = new.unwrap_or(&empty);
    let b = new.bbox;
    let prev_u8 = |s: &Surface| -> Vec<u8> {
        let mut v = Vec::new();
        s.read_region_into(b, &mut v);
        let ch = s.channels();
        v.chunks_exact(ch)
            .map(|p| (p[0].clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect()
    };
    let from_bytes = |bytes: &[u8]| -> Option<Surface> {
        if b.is_empty() || bytes.iter().all(|v| *v == 0) {
            return None;
        }
        let mut s = Surface::from_interleaved(PixelFormat::GRAY8, b, bytes);
        s.prune();
        Some(s)
    };
    let nonempty =
        |s: Surface| -> Option<Surface> { (!s.content_bounds().is_empty()).then_some(s) };
    match (mode, old) {
        (SelectionMode::Replace, _) | (SelectionMode::Add, None) => from_bytes(&new.mask),
        (SelectionMode::Subtract | SelectionMode::Intersect, None) => None,
        (SelectionMode::Intersect, Some(o)) => {
            let p = prev_u8(o);
            from_bytes(
                &p.iter()
                    .zip(&new.mask)
                    .map(|(a, b)| *a.min(b))
                    .collect::<Vec<_>>(),
            )
        }
        (SelectionMode::Add | SelectionMode::Subtract, Some(o)) => {
            if b.is_empty() {
                return nonempty(o.clone());
            }
            let p = prev_u8(o);
            let out: Vec<f32> =
                p.iter().zip(&new.mask).map(|(a, n)| if mode == SelectionMode::Add { *a.max(n) } else { a.saturating_sub(*n) } as f32 / 255.0).collect();
            let mut s = o.clone();
            s.write_region(b, &out);
            s.prune();
            nonempty(s)
        }
    }
}

/// Softens hard mask edges: edge pixels become the 3×3 average.
fn antialias(m: &mut [f32], w: usize, h: usize) {
    let src = m.to_vec();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut sum = 0.0;
            let mut cnt = 0.0;
            let mut edge = false;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let (xx, yy) = (x as i32 + dx, y as i32 + dy);
                    let v = if xx < 0 || yy < 0 || xx >= w as i32 || yy >= h as i32 {
                        src[i]
                    } else {
                        src[yy as usize * w + xx as usize]
                    };
                    edge |= v != src[i];
                    sum += v;
                    cnt += 1.0;
                }
            }
            if edge {
                // Keep selected pixels mostly selected, add a soft fringe.
                m[i] = if src[i] > 0.5 {
                    0.5 + 0.5 * sum / cnt
                } else {
                    0.5 * sum / cnt
                };
            }
        }
    }
}
