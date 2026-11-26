//! Texture synthesis by image quilting with minimum-error boundary cuts.

use super::*;
use crate::poisson::membrane_fill;

/// Create texture: fill the hole with blocks of surrounding texture (see module docs).
pub fn synthesize(
    w: usize,
    h: usize,
    ch: usize,
    img: &[f32],
    hole: &[bool],
    block: usize,
    seed: u64,
) -> Vec<f32> {
    let mut out = img.to_vec();
    let Some((bx0, by0, bx1, by1)) = hole_bbox(w, h, hole) else {
        return out;
    };
    let b = block.max(4) as i32;
    let ov = (b / 3).max(1);
    let hs = integral(w, h, hole);
    // Candidate block origins: whole block (with overlap) inside the grid and outside the hole.
    let mut cands = Vec::new();
    for y in ov..(h as i32 - b - ov) {
        for x in ov..(w as i32 - b - ov) {
            if window_count(&hs, w, h, x - ov, y - ov, x + b + ov, y + b + ov) == 0 {
                cands.push((x, y));
            }
        }
    }
    if cands.is_empty() {
        return membrane_fill(w, h, ch, img, hole);
    }
    let mut filled: Vec<bool> = hole.iter().map(|x| !x).collect();
    let mut rng = Rng::new(seed);
    let mut by = by0 as i32;
    while by < by1 as i32 {
        let mut bx = bx0 as i32;
        while bx < bx1 as i32 {
            if window_count(&hs, w, h, bx, by, bx + b, by + b) > 0 {
                // Pick the candidate whose block best agrees with already-known pixels around/inside.
                let mut best = (cands[0], f32::INFINITY);
                for _ in 0..48 {
                    let c = cands[(rng.next() % cands.len() as u64) as usize];
                    let (mut e, mut n) = (0.0f32, 0u32);
                    for y in (by - ov)..(by + b + ov) {
                        for x in (bx - ov)..(bx + b + ov) {
                            if x < 0
                                || y < 0
                                || x >= w as i32
                                || y >= h as i32
                                || !filled[y as usize * w + x as usize]
                            {
                                continue;
                            }
                            let (a, s) = (
                                (y as usize * w + x as usize) * ch,
                                ((y - by + c.1) as usize * w + (x - bx + c.0) as usize) * ch,
                            );
                            for k in 0..ch {
                                let d = out[a + k] - img[s + k];
                                e += d * d;
                            }
                            n += 1;
                        }
                    }
                    let e = if n == 0 { 0.0 } else { e / n as f32 };
                    if e < best.1 {
                        best = (c, e);
                    }
                }
                let c = best.0;
                let (x0, y0) = ((bx - ov).max(0), (by - ov).max(0));
                let (x1, y1) = ((bx + b + ov).min(w as i32), (by + b + ov).min(h as i32));
                let src =
                    |x: i32, y: i32| ((y - by + c.1) as usize * w + (x - bx + c.0) as usize) * ch;
                // Squared difference between what is there and the block, where both exist.
                let err = |x: i32, y: i32| {
                    let i = y as usize * w + x as usize;
                    if !(hole[i] && filled[i]) {
                        return 0.0;
                    }
                    let (a, s) = (i * ch, src(x, y));
                    (0..ch)
                        .map(|q| (out[a + q] - img[s + q]).powi(2))
                        .sum::<f32>()
                };
                // Minimum-error boundary cuts through the overlaps with the blocks to the left and
                // above: the block is copied right of and below them, untouched pixels are kept
                // on the other side. Copying (rather than averaging) keeps the texture's grain.
                let left = min_cut((x0..(bx + ov).min(x1)).len(), (y0..y1).len(), |u, v| {
                    err(x0 + u as i32, y0 + v as i32)
                });
                let top = min_cut((y0..(by + ov).min(y1)).len(), (x0..x1).len(), |u, v| {
                    err(x0 + v as i32, y0 + u as i32)
                });
                for y in y0..y1 {
                    for x in x0..x1 {
                        let i = y as usize * w + x as usize;
                        if !hole[i] {
                            continue;
                        }
                        let (u, v) = ((x - x0) as usize, (y - y0) as usize);
                        let keep = filled[i]
                            && (left.get(v).is_some_and(|s| u < *s)
                                || top.get(u).is_some_and(|s| v < *s));
                        if !keep {
                            let (a, s) = (i * ch, src(x, y));
                            out[a..a + ch].copy_from_slice(&img[s..s + ch]);
                        }
                        filled[i] = true;
                    }
                }
            }
            bx += b;
        }
        by += b;
    }
    out
}

/// The minimum-error cut through a band `across` wide and `along` long (Efros & Freeman's
/// boundary cut, by dynamic programming): for each step along the band, the offset across it
/// where the cut runs (`err(across, along)` is the cost of a pixel). Pixels before the cut keep
/// what is there. Empty when there is no cost anywhere (nothing to cut through).
fn min_cut(across: usize, along: usize, err: impl Fn(usize, usize) -> f32) -> Vec<usize> {
    if across == 0 || along == 0 {
        return Vec::new();
    }
    // Cumulative cost, step by step along the band; the cut may move by one pixel per step.
    let mut acc: Vec<f32> = (0..along)
        .flat_map(|v| (0..across).map(move |u| (u, v)))
        .map(|(u, v)| err(u, v))
        .collect();
    if acc.iter().all(|x| *x == 0.0) {
        return Vec::new();
    }
    for v in 1..along {
        for u in 0..across {
            let lo = u.saturating_sub(1);
            let hi = (u + 1).min(across - 1);
            let m = (lo..=hi)
                .map(|k| acc[(v - 1) * across + k])
                .fold(f32::INFINITY, f32::min);
            acc[v * across + u] += m;
        }
    }
    let argmin = |v: usize, lo: usize, hi: usize| {
        (lo..=hi)
            .min_by(|a, b| acc[v * across + a].total_cmp(&acc[v * across + b]))
            .unwrap_or(lo)
    };
    let mut cut = vec![0; along];
    let mut u = argmin(along - 1, 0, across - 1);
    for v in (0..along).rev() {
        if v + 1 < along {
            u = argmin(v, u.saturating_sub(1), (u + 1).min(across - 1));
        }
        cut[v] = u;
    }
    cut
}
