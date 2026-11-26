//! Selection modes, mask/surface conversion and combining with an existing selection.

use super::*;

/// How a new selection combines with the current one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SelectionMode {
    #[default]
    Replace,
    Add,
    Subtract,
    Intersect,
}

impl SelectionMode {
    /// Parses `replace|add|subtract|intersect` (unknown → replace).
    pub fn parse(s: &str) -> Self {
        match s {
            "add" => SelectionMode::Add,
            "subtract" => SelectionMode::Subtract,
            "intersect" => SelectionMode::Intersect,
            _ => SelectionMode::Replace,
        }
    }
}

/// Reads a selection surface over `area` (no selection → all zero).
pub fn mask_from_surface(s: Option<&Surface>, area: Rect) -> Vec<f32> {
    let n = area.width() as usize * area.height() as usize;
    match s {
        None => vec![0.0; n],
        // Selections are GRAY8: copy tile bytes through a table, tile rows in parallel.
        Some(s) if s.format() == PixelFormat::GRAY8 && !area.is_empty() => {
            let w = area.width() as usize;
            let lut: [f32; 256] = std::array::from_fn(|i| i as f32 / 255.0);
            let dp = s.default_pixel().first().copied().unwrap_or(0.0);
            let mut out = vec![0.0f32; n];
            par_rows(&mut out, w, 1, |yy, row| {
                let y = area.y0 + yy as i32;
                let mut x = area.x0;
                while x < area.x1 {
                    let tc = TileCoord::containing(x, y);
                    let x1 = (tc.rect().x1).min(area.x1);
                    let dst = row
                        .get_mut((x - area.x0) as usize..(x1 - area.x0) as usize)
                        .unwrap_or_default();
                    let src = s.tile(tc).and_then(|t| {
                        let base = (y - tc.ty * TILE_SIZE) as usize * TILE_SIZE as usize
                            + (x - tc.tx * TILE_SIZE) as usize;
                        t.bytes().get(base..base + dst.len())
                    });
                    match src {
                        Some(src) => dst
                            .iter_mut()
                            .zip(src)
                            .for_each(|(d, b)| *d = lut[*b as usize]),
                        None => dst.fill(dp),
                    }
                    x = x1;
                }
            });
            out
        }
        Some(s) => {
            let mut v = Vec::new();
            s.read_region_into(area, &mut v);
            let ch = s.channels();
            if ch > 1 {
                v.chunks_exact(ch).map(|p| p[0]).collect()
            } else {
                v
            }
        }
    }
}

/// 8-bit selection value of a coverage (clamped and rounded, as GRAY8 surfaces store it; NaN → 0).
#[inline]
fn coverage8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Builds a GRAY8 selection surface over `area` (masks row-major over it) whose coverage at
/// each sample is `f(prev, new)`, `prev` reading 0 without a `prev` mask and missing samples
/// reading 0. Tiles are filled in parallel straight from the masks, a row at a time (no
/// full-canvas byte buffer); only the tiles that hold something are kept (as [`Surface::prune`]
/// would leave them), and fully selected tiles share one tile (copy-on-write), so the inside of a
/// large selection costs no memory per tile (#211).
fn surface_from_masks(
    area: Rect,
    new: &[f32],
    prev: Option<&[f32]>,
    f: impl Fn(f32, f32) -> f32 + Sync + Send,
) -> Surface {
    let mut s = Surface::new(PixelFormat::GRAY8);
    if area.is_empty() {
        return s;
    }
    let w = area.width() as usize;
    let ts = TILE_SIZE as usize;
    let full = s.solid_tile(&[1.0]);
    let row = |i0: usize, dst: &mut [u8]| {
        let n = dst.len();
        match (new.get(i0..i0 + n), prev.map(|p| p.get(i0..i0 + n))) {
            (Some(sn), None) => dst
                .iter_mut()
                .zip(sn)
                .for_each(|(d, v)| *d = coverage8(f(0.0, *v))),
            (Some(sn), Some(Some(sp))) => dst
                .iter_mut()
                .zip(sn.iter().zip(sp))
                .for_each(|(d, (v, p))| *d = coverage8(f(*p, *v))),
            _ => {
                let at = |m: &[f32], i: usize| m.get(i).copied().unwrap_or(0.0);
                for (i, d) in (i0..).zip(dst.iter_mut()) {
                    *d = coverage8(f(prev.map_or(0.0, |p| at(p, i)), at(new, i)));
                }
            }
        }
    };
    // Every tile starts as one shared blank tile; `Arc::make_mut` gives each task its own copy.
    let blanks = s.take_tiles(area);
    let built = par_map(blanks.len(), |k| {
        let (tc, blank) = blanks.get(k)?;
        let tr = tc.rect().intersect(&area);
        let (lx, span) = ((tr.x0 - area.x0) as usize, tr.width() as usize);
        let mut tile = Arc::clone(blank);
        let data = Arc::make_mut(&mut tile).bytes_mut();
        let (mut lo, mut hi) = (if tr == tc.rect() { u8::MAX } else { 0 }, 0u8);
        for y in tr.y0..tr.y1 {
            let o = (y - tc.ty * TILE_SIZE) as usize * ts + (tr.x0 - tc.tx * TILE_SIZE) as usize;
            let Some(dst) = data.get_mut(o..o + span) else {
                continue;
            };
            row((y - area.y0) as usize * w + lx, dst);
            (lo, hi) = dst
                .iter()
                .fold((lo, hi), |(lo, hi), b| (lo.min(*b), hi.max(*b)));
        }
        (hi > 0).then(|| {
            (
                *tc,
                if lo == u8::MAX {
                    Arc::clone(&full)
                } else {
                    tile
                },
            )
        })
    });
    s.put_tiles(built.into_iter().flatten());
    s
}

/// Builds a GRAY8 selection surface from a mask over `area` (missing samples are 0).
pub fn mask_to_surface(m: &[f32], area: Rect) -> Surface {
    surface_from_masks(area, m, None, |_, v| v)
}

/// Combines `new` with `old` by `mode`. Returns `None` when nothing is selected.
pub fn combine(
    old: Option<&Surface>,
    new: &[f32],
    area: Rect,
    mode: SelectionMode,
) -> Option<Surface> {
    if area.is_empty() {
        return None;
    }
    // Quantize to the 8-bit grid selections are stored on, so combining a mask with itself is
    // exact. Below half an 8-bit step is nothing.
    let q = |v: f32| f32::from(coverage8(v)) / 255.0;
    let s = match mode {
        SelectionMode::Replace => surface_from_masks(area, new, None, |_, v| v),
        _ => {
            let prev = mask_from_surface(old, area);
            match mode {
                SelectionMode::Add => {
                    surface_from_masks(area, new, Some(&prev), |p, v| p.max(q(v)))
                }
                SelectionMode::Subtract => {
                    surface_from_masks(area, new, Some(&prev), |p, v| (p - q(v)).max(0.0))
                }
                _ => surface_from_masks(area, new, Some(&prev), |p, v| p.min(q(v))),
            }
        }
    };
    (s.tile_count() > 0).then_some(s)
}

pub(super) fn close(a: [f32; 4], b: [f32; 4], tol: f32) -> bool {
    (a[3] == 0.0 && b[3] == 0.0) || (0..4).all(|c| (a[c] - b[c]).abs() * 255.0 <= tol + 1e-3)
}
