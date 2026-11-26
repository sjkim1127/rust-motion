//! Source patches: valid copy centres, the targets around the hole, and coarser-level matches.

use super::*;
use crate::par;

/// Valid source centres: the whole `2r+1` patch inside the grid and copyable. Found per row
/// (each pixel's horizontal run), then per column of runs. `None` when cancelled.
pub(super) fn valid_sources(
    w: usize,
    h: usize,
    r: usize,
    px: &[Px],
    ctl: &rust_motion_raster::Interrupt,
) -> Option<Vec<bool>> {
    const BAND: usize = 64;
    let side = 2 * r + 1;
    let par = w * h >= PAR_MIN;
    let mut run = vec![false; w * h];
    if w < side || h < side {
        return Some(run);
    }
    // Per row: is the run of `side` pixels centred on each one all copyable (a sliding count)?
    par::chunks_mut(&mut run, w, par, |y, out| {
        if y % BAND == 0 && ctl.cancelled() {
            return;
        }
        let row = &px[y * w..(y + 1) * w];
        let copyable = |x: usize| usize::from(row[x] == Px::Source);
        let mut count: usize = (0..side).map(copyable).sum();
        out[r] = count == side;
        for (x, o) in out.iter_mut().enumerate().take(w - r).skip(r + 1) {
            count = count + copyable(x + r) - copyable(x - r - 1);
            *o = count == side;
        }
    });
    if ctl.cancelled() {
        return None;
    }
    // Per column, in bands of rows: how many such runs end each row in a row; a centre is valid
    // where `side` of them stack up to `r` rows below it.
    let mut valid = vec![false; w * h];
    par::chunks_mut(&mut valid, BAND * w, par, |band, out| {
        if ctl.cancelled() {
            return;
        }
        let (y0, rows) = (band * BAND, out.len() / w);
        let mut stack = vec![0usize; w];
        for yy in y0.saturating_sub(r)..(y0 + rows + r).min(h) {
            for (x, n) in stack.iter_mut().enumerate() {
                *n = if run[yy * w + x] { *n + 1 } else { 0 };
            }
            // `yy` is the bottom row of centre `y`'s patch.
            if yy < y0 + r || yy < 2 * r {
                continue;
            }
            let y = yy - r;
            for (v, n) in out[(y - y0) * w..(y - y0 + 1) * w]
                .iter_mut()
                .zip(&stack)
                .take(w - r)
                .skip(r)
            {
                *v = *n >= side;
            }
        }
    });
    if ctl.cancelled() {
        return None;
    }
    Some(valid)
}

/// Columns per block of [`Sources`]' index.
const SOURCE_BLOCK: usize = 64;

/// The valid source centres in row-major order, indexed by counts per row and per block of
/// [`SOURCE_BLOCK`] columns (so the `k`-th is found without listing them all).
pub(super) struct Sources {
    /// Valid centres before each row (`h + 1` entries).
    row_start: Vec<usize>,
    /// Per row, the valid centres in it before each block.
    block_start: Vec<u32>,
    pub(super) blocks: usize,
}

impl Sources {
    pub(super) fn new(valid: &[bool], w: usize) -> Sources {
        let h = valid.len().checked_div(w).unwrap_or(0);
        let blocks = w.div_ceil(SOURCE_BLOCK);
        let mut block_start = vec![0u32; h * blocks];
        // Per row: the block prefix, and the row's count.
        let counts: Vec<usize> = par::map(h, valid.len() >= PAR_MIN, |y| {
            valid[y * w..(y + 1) * w].iter().filter(|v| **v).count()
        });
        par::chunks_mut(
            &mut block_start,
            blocks,
            valid.len() >= PAR_MIN,
            |y, out| {
                let mut n = 0u32;
                for (b, o) in out.iter_mut().enumerate() {
                    *o = n;
                    n += valid[y * w + b * SOURCE_BLOCK..y * w + ((b + 1) * SOURCE_BLOCK).min(w)]
                        .iter()
                        .filter(|v| **v)
                        .count() as u32;
                }
            },
        );
        let mut row_start = Vec::with_capacity(counts.len() + 1);
        let mut total = 0usize;
        row_start.push(0);
        for c in counts {
            total += c;
            row_start.push(total);
        }
        Sources {
            row_start,
            block_start,
            blocks,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.row_start.last().copied().unwrap_or(0)
    }

    /// The `k`-th valid centre in row-major order (`k < len`).
    pub(super) fn nth(&self, k: usize, valid: &[bool], w: usize) -> (i32, i32) {
        let y = self
            .row_start
            .partition_point(|&s| s <= k)
            .saturating_sub(1);
        let left = k.saturating_sub(self.row_start.get(y).copied().unwrap_or(0));
        let row_blocks = self
            .block_start
            .get(y * self.blocks..(y + 1) * self.blocks)
            .unwrap_or(&[]);
        let b = row_blocks
            .partition_point(|&s| s as usize <= left)
            .saturating_sub(1);
        let mut left = left.saturating_sub(row_blocks.get(b).copied().unwrap_or(0) as usize);
        let x0 = b * SOURCE_BLOCK;
        for (x, v) in valid
            .iter()
            .skip(y * w + x0)
            .take(w.saturating_sub(x0))
            .enumerate()
        {
            if *v {
                if left == 0 {
                    return ((x0 + x) as i32, y as i32);
                }
                left -= 1;
            }
        }
        (0, 0)
    }

    /// All of them, in row-major order.
    pub(super) fn all(&self, valid: &[bool], w: usize) -> Vec<(i32, i32)> {
        let mut out = Vec::with_capacity(self.len());
        for (i, v) in valid.iter().enumerate() {
            if *v {
                out.push(((i % w) as i32, (i / w) as i32));
            }
        }
        out
    }
}

/// Per cell of `tb` (the hole's bounds `hb` grown by the patch radius, where all of them lie): is
/// it a target centre (its patch overlaps the hole)?
pub(super) fn targets_around(w: usize, r: usize, px: &[Px], hb: Bounds, tb: Bounds) -> Vec<bool> {
    // Hole counts summed over the hole's bounds: every hole pixel lies inside them.
    let (bw, bh) = (hb.width(), hb.height());
    let mut s = vec![0u32; (bw + 1) * (bh + 1)];
    for y in 0..bh {
        let mut row = 0u32;
        for x in 0..bw {
            row += u32::from(px[(hb.y0 + y) * w + hb.x0 + x] == Px::Hole);
            s[(y + 1) * (bw + 1) + x + 1] = s[y * (bw + 1) + x + 1] + row;
        }
    }
    let mut targets = vec![false; tb.area()];
    let ri = r as i32;
    let (ox, oy) = (hb.x0 as i32, hb.y0 as i32);
    for y in tb.y0..tb.y1 {
        for x in tb.x0..tb.x1 {
            let k = (y - tb.y0) * tb.width() + (x - tb.x0);
            let (x, y) = (x as i32, y as i32);
            targets[k] = window_count(
                &s,
                bw,
                bh,
                x - ri - ox,
                y - ri - oy,
                x + ri + 1 - ox,
                y + ri + 1 - oy,
            ) > 0;
        }
    }
    targets
}

/// A finished coarser level: its size, pixels and match field (the source centre per cell of
/// its target box, `i32::MIN` where there is none; empty when it had nothing to copy).
pub(super) struct Coarser {
    pub(super) w: usize,
    pub(super) h: usize,
    pub(super) img: Vec<f32>,
    pub(super) field: Vec<(i32, i32)>,
    pub(super) tbox: Bounds,
}

impl Coarser {
    /// The match of cell `(x, y)` (`i32::MIN` outside the target box).
    pub(super) fn field_at(&self, x: usize, y: usize) -> (i32, i32) {
        let t = self.tbox;
        if x < t.x0 || x >= t.x1 || y < t.y0 || y >= t.y1 {
            return (i32::MIN, i32::MIN);
        }
        self.field
            .get((y - t.y0) * t.width() + (x - t.x0))
            .copied()
            .unwrap_or((i32::MIN, i32::MIN))
    }
}
