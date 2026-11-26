//! Level geometry: downsampling, the hole's bounds and thickness, and its integral image.

use super::*;
use crate::par;
use std::borrow::Cow;

/// Half-size level: a cell is a hole if any of its pixels is; else it averages its non-void
/// pixels, and may be copied from only if all of them may.
pub(super) fn downsample(l: &Level, ch: usize) -> Level<'static> {
    let (w, h) = (l.w.div_ceil(2), l.h.div_ceil(2));
    let mut img = vec![0.0f32; w * h * ch];
    let mut px = vec![Px::Void; w * h];
    let (lw, lh, limg, lpx) = (l.w, l.h, &*l.img, &*l.px);
    let row = |y: usize, irow: &mut [f32], prow: &mut [Px]| {
        for (x, p) in prow.iter_mut().enumerate() {
            let mut n = 0.0f32;
            let o = x * ch;
            let (mut hole, mut all_source, mut any) = (false, true, false);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (sx, sy) = (x * 2 + dx, y * 2 + dy);
                if sx >= lw || sy >= lh {
                    continue;
                }
                let si = sy * lw + sx;
                match lpx[si] {
                    Px::Hole => hole = true,
                    Px::Void => all_source = false,
                    k => {
                        any = true;
                        all_source &= k == Px::Source;
                        for c in 0..ch {
                            irow[o + c] += limg[si * ch + c];
                        }
                        n += 1.0;
                    }
                }
            }
            if n > 0.0 {
                for c in 0..ch {
                    irow[o + c] /= n;
                }
            }
            *p = if hole {
                Px::Hole
            } else if !any {
                Px::Void
            } else if all_source {
                Px::Source
            } else {
                Px::Known
            };
        }
    };
    par::chunks2_mut(&mut img, w * ch, &mut px, w, w * h >= PAR_MIN, row);
    Level {
        w,
        h,
        img: Cow::Owned(img),
        px: Cow::Owned(px),
    }
}

/// Summed-area table of a boolean grid (`(w+1) × (h+1)`). Not inlined: in [`synthesize`] it made
/// Create Texture's candidate loop slower (measured).
#[inline(never)]
pub(super) fn integral(w: usize, h: usize, m: &[bool]) -> Vec<u32> {
    let mut s = vec![0u32; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0u32;
        for x in 0..w {
            row += u32::from(m[y * w + x]);
            s[(y + 1) * (w + 1) + x + 1] = s[y * (w + 1) + x + 1] + row;
        }
    }
    s
}

/// Count of set cells in the clipped window `[x0, x1) × [y0, y1)`.
pub(super) fn window_count(
    s: &[u32],
    gw: usize,
    gh: usize,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
) -> u32 {
    let (x0, y0) = (x0.max(0) as usize, y0.max(0) as usize);
    let (x1, y1) = ((x1.max(0) as usize).min(gw), (y1.max(0) as usize).min(gh));
    if x1 <= x0 || y1 <= y0 {
        return 0;
    }
    let w1 = gw + 1;
    s[y1 * w1 + x1] + s[y0 * w1 + x0] - s[y0 * w1 + x1] - s[y1 * w1 + x0]
}

pub(super) fn hole_bbox(w: usize, h: usize, hole: &[bool]) -> Option<(usize, usize, usize, usize)> {
    let mut b: Option<(usize, usize, usize, usize)> = None;
    for y in 0..h {
        for x in 0..w {
            if hole[y * w + x] {
                b = Some(match b {
                    None => (x, y, x + 1, y + 1),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
                });
            }
        }
    }
    b
}

/// A hole at most `1 / THIN_RATIO` as thick as its bounds are long counts as thin (a wire, a
/// string, a hair, a long brush stroke): see the module docs.
pub(super) const THIN_RATIO: usize = 4;

/// How thick the hole within bounds `hb` of a `w`-wide role grid is: twice the largest distance
/// from a hole pixel to the nearest pixel that is not one (a long, thin hole is thin however long
/// its bounds).
pub(super) fn hole_thickness(w: usize, hb: Bounds, px: &[Px]) -> usize {
    // The bounds and a ring of non-hole cells around them (also past the grid's edge).
    let (gw, gh) = (hb.width() + 2, hb.height() + 2);
    let outside: Vec<bool> = par::map(gw * gh, gw * gh >= PAR_MIN, |i| {
        let (gx, gy) = (i % gw, i / gw);
        if gx == 0 || gy == 0 || gx > hb.width() || gy > hb.height() {
            return true;
        }
        px.get((hb.y0 + gy - 1) * w + hb.x0 + gx - 1) != Some(&Px::Hole)
    });
    let deepest = crate::selection::edt(&outside, gw, gh)
        .into_iter()
        .fold(0.0f32, f32::max);
    (2.0 * deepest).ceil() as usize
}

/// Most target pixels × sources the onion peel of a thin hole may compare (its search is
/// exhaustive, though most comparisons stop early: about a second at this many).
const PEEL_BUDGET: usize = 1 << 28;

/// Whether the onion peel of the hole cells within `hb` of a `w`-wide role grid from `sources`
/// source centres stays within [`PEEL_BUDGET`].
pub(super) fn peel_affordable(w: usize, px: &[Px], hb: Bounds, sources: usize) -> bool {
    let holes: usize = (hb.y0..hb.y1)
        .map(|y| {
            px.get(y * w + hb.x0..y * w + hb.x1)
                .map_or(0, |row| row.iter().filter(|k| **k == Px::Hole).count())
        })
        .sum();
    holes.saturating_mul(sources) <= PEEL_BUDGET
}

/// Bounds of the hole pixels of a role grid.
pub(super) fn hole_bounds(w: usize, h: usize, px: &[Px]) -> Option<Bounds> {
    // Per row the first and last hole column; the union of the rows' spans.
    let rows: Vec<Option<(usize, usize)>> = par::map(h, w * h >= PAR_MIN, |y| {
        let row = px.get(y * w..(y + 1) * w)?;
        let x0 = row.iter().position(|k| *k == Px::Hole)?;
        let x1 = row.iter().rposition(|k| *k == Px::Hole).unwrap_or(x0) + 1;
        Some((x0, x1))
    });
    let mut b: Option<Bounds> = None;
    for (y, span) in rows.into_iter().enumerate() {
        if let Some((x0, x1)) = span {
            b = Some(match b {
                None => Bounds {
                    x0,
                    y0: y,
                    x1,
                    y1: y + 1,
                },
                Some(b) => Bounds {
                    x0: b.x0.min(x0),
                    y0: b.y0,
                    x1: b.x1.max(x1),
                    y1: y + 1,
                },
            });
        }
    }
    b
}
