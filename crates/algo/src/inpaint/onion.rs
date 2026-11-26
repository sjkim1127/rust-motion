//! Onion peel: the ring-by-ring initial fill of the coarsest level.

use super::*;
use crate::par;

/// Fill the hole from its edge inwards, one ring at a time: each pixel takes the centre of the
/// source patch that best matches what is already known around it (the first in `sources` of
/// those that match best). `hb` bounds the hole.
///
/// The search is exhaustive, but most sources are given up early (#2771, #2805): the target's
/// known pixels are gathered once, the search starts from where the pixel's filled neighbours
/// took their sources (or every 16th source at the hole's edge), and a source's error stops being
/// added up once it can only lose (it only grows). So it finds the same source as a plain scan in
/// order, sooner.
#[allow(clippy::too_many_arguments)]
pub(super) fn onion_peel(
    w: usize,
    h: usize,
    ch: usize,
    r: i32,
    img: &mut [f32],
    px: &[Px],
    hb: Bounds,
    sources: &[(i32, i32)],
    ctl: &rust_motion_raster::Interrupt,
) -> Result<(), rust_motion_raster::Cancelled> {
    const NONE: u32 = u32::MAX;
    let mut known: Vec<bool> = px
        .iter()
        .map(|k| matches!(k, Px::Known | Px::Source))
        .collect();
    // Each source centre's place in `sources`, and the source each filled pixel took.
    let mut index = vec![NONE; w * h];
    for (j, &(x, y)) in sources.iter().enumerate() {
        index[y as usize * w + x as usize] = j as u32;
    }
    let mut took = vec![NONE; w * h];
    let neighbours = [
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ];
    loop {
        ctl.check()?;
        let mut front: Vec<usize> = Vec::new();
        for y in hb.y0..hb.y1 {
            for x in hb.x0..hb.x1 {
                let i = y * w + x;
                if known[i] || px[i] != Px::Hole {
                    continue;
                }
                let (x, y) = (x as i32, y as i32);
                let edge = (-1..=1).any(|dy: i32| {
                    (-1..=1).any(|dx: i32| {
                        let (nx, ny) = (x + dx, y + dy);
                        nx >= 0
                            && ny >= 0
                            && (nx as usize) < w
                            && (ny as usize) < h
                            && known[ny as usize * w + nx as usize]
                    })
                });
                if edge {
                    front.push(i);
                }
            }
        }
        if front.is_empty() {
            break;
        }
        let (known_now, took_now) = (&known, &took);
        let image = &*img;
        let best = |&i: &usize| -> (Vec<f32>, u32) {
            let (x, y) = ((i % w) as i32, (i / w) as i32);
            // The known pixels of the target patch, in the plain scan's order: each one's offset
            // from the patch centre (in pixels) and its samples (gathered once, #2805).
            let side = (2 * r + 1) as usize;
            let mut offs: Vec<isize> = Vec::with_capacity(side * side);
            let mut tv: Vec<f32> = Vec::with_capacity(side * side * ch);
            for dy in -r..=r {
                for dx in -r..=r {
                    let (tx, ty) = (x + dx, y + dy);
                    if tx >= 0
                        && ty >= 0
                        && (tx as usize) < w
                        && (ty as usize) < h
                        && known_now[ty as usize * w + tx as usize]
                    {
                        let ti = (ty as usize * w + tx as usize) * ch;
                        offs.push(dy as isize * w as isize + dx as isize);
                        tv.extend_from_slice(&image[ti..ti + ch]);
                    }
                }
            }
            // The best so far: its error per sample, its place in `sources` (the first of equals
            // wins, as in a scan in order), and the sums of squares at which a source surely loses
            // to it (`e / n` above the best) or ties it (at least the best).
            let mut best = Best {
                v: f32::INFINITY,
                j: usize::MAX,
                worse: f32::INFINITY,
                equal: f32::INFINITY,
            };
            if !offs.is_empty() {
                let nf = offs.len() as f32;
                let base_of = |j: usize| {
                    sources
                        .get(j)
                        .map_or(0, |&(sx, sy)| sy as isize * w as isize + sx as isize)
                };
                // The sum at which source `j` can't win: a tie loses to an earlier source.
                let limit_of =
                    |j: usize, best: &Best| if j > best.j { best.equal } else { best.worse };
                // Source `j`'s whole sum `e` (it did not reach its limit): does it win?
                let adopt = |j: usize, e: f32, best: &mut Best| {
                    let v = e / nf;
                    if v < f32::INFINITY && (v < best.v || (v == best.v && j < best.j)) {
                        *best = Best {
                            v,
                            j,
                            worse: least_sum(v, nf, true),
                            equal: least_sum(v, nf, false),
                        };
                    }
                };
                let consider = |j: usize, best: &mut Best| {
                    if j >= sources.len() {
                        return;
                    }
                    let (base, limit) = (base_of(j), limit_of(j, best));
                    let e = match ch {
                        1 => known_sse::<1>(image, &offs, &tv, base, limit),
                        3 => known_sse::<3>(image, &offs, &tv, base, limit),
                        4 => known_sse::<4>(image, &offs, &tv, base, limit),
                        _ => known_sse_any(image, ch, &offs, &tv, base, limit),
                    };
                    if let Some(e) = e {
                        adopt(j, e, best);
                    }
                };
                // A close bound early: where the filled neighbours' sources point (shifted as in
                // PatchMatch's propagation), or at the hole's edge, where none is filled yet,
                // every 16th source.
                for (dx, dy) in neighbours {
                    let (qx, qy) = (x + dx, y + dy);
                    if qx < 0 || qy < 0 || qx as usize >= w || qy as usize >= h {
                        continue;
                    }
                    let Some(&(sx, sy)) =
                        sources.get(took_now[qy as usize * w + qx as usize] as usize)
                    else {
                        continue;
                    };
                    let (cx, cy) = (sx - dx, sy - dy);
                    if cx >= 0
                        && cy >= 0
                        && (cx as usize) < w
                        && (cy as usize) < h
                        && index[cy as usize * w + cx as usize] != NONE
                    {
                        consider(index[cy as usize * w + cx as usize] as usize, &mut best);
                    }
                }
                if best.j == usize::MAX {
                    for j in (0..sources.len()).step_by(16) {
                        consider(j, &mut best);
                    }
                }
                // The scan in order, `LANES` sources at a time (their sums are independent, so they
                // overlap), each against its limit from the best before them: the best only gets
                // better, so a source given up would have lost to it anyway.
                let mut j = 0;
                while j + LANES <= sources.len() {
                    if j.is_multiple_of(4096) && j > 0 && ctl.cancelled() {
                        break;
                    }
                    let bases: [isize; LANES] = std::array::from_fn(|l| base_of(j + l));
                    let limits: [f32; LANES] = std::array::from_fn(|l| limit_of(j + l, &best));
                    let sums = match ch {
                        1 => known_sse_lanes::<1>(image, &offs, &tv, bases, limits),
                        3 => known_sse_lanes::<3>(image, &offs, &tv, bases, limits),
                        4 => known_sse_lanes::<4>(image, &offs, &tv, bases, limits),
                        _ => std::array::from_fn(|l| {
                            known_sse_any(image, ch, &offs, &tv, bases[l], limits[l])
                        }),
                    };
                    for (l, e) in sums.into_iter().enumerate() {
                        if let Some(e) = e {
                            adopt(j + l, e, &mut best);
                        }
                    }
                    j += LANES;
                }
                for j in j..sources.len() {
                    consider(j, &mut best);
                }
            }
            let (sx, sy) = sources.get(best.j).copied().unwrap_or((0, 0));
            let si = (sy as usize * w + sx as usize) * ch;
            (
                image[si..si + ch].to_vec(),
                if best.j == usize::MAX {
                    NONE
                } else {
                    best.j as u32
                },
            )
        };
        let vals: Vec<(Vec<f32>, u32)> =
            par::map(front.len(), front.len() * sources.len() >= 4096, |k| {
                best(&front[k])
            });
        ctl.check()?;
        for (&i, (v, j)) in front.iter().zip(vals) {
            img[i * ch..(i + 1) * ch].copy_from_slice(&v);
            known[i] = true;
            took[i] = j;
        }
    }
    Ok(())
}
