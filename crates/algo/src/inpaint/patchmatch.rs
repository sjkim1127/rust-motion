//! PatchMatch passes, match costs and the similarity-weighted vote of the EM step.

use super::*;
use crate::par;

impl Solver<'_> {
    /// One PatchMatch step at target `t` (`k` in the target box) whose match is `cur`: propagation
    /// from the matches of the neighbours visited just before it (`left` in its row, `up` in its
    /// column, in this pass's direction `step`), then random search in exponentially shrinking
    /// windows around the best (two numbers from `rng` per window).
    #[allow(clippy::too_many_arguments)]
    #[inline]
    fn pm_step(
        &self,
        t: (i32, i32),
        k: usize,
        cur: ((i32, i32), f32),
        left: Option<(i32, i32)>,
        up: Option<(i32, i32)>,
        step: i32,
        rng: &mut Rng,
        search_radius: i32,
    ) -> ((i32, i32), f32) {
        let (mut best, mut bc) = cur;
        if let Some(c) = left {
            let cand = (c.0 + step, c.1);
            if self.is_valid(cand.0, cand.1) && cand != best {
                let d = self.dist(t, k, cand, bc);
                if d < bc {
                    (best, bc) = (cand, d);
                }
            }
        }
        if let Some(c) = up {
            let cand = (c.0, c.1 + step);
            if self.is_valid(cand.0, cand.1) && cand != best {
                let d = self.dist(t, k, cand, bc);
                if d < bc {
                    (best, bc) = (cand, d);
                }
            }
        }
        let mut rad = search_radius;
        while rad >= 1 {
            let cand = (best.0 + rng.range(-rad, rad), best.1 + rng.range(-rad, rad));
            if self.is_valid(cand.0, cand.1) && cand != best {
                let d = self.dist(t, k, cand, bc);
                if d < bc {
                    (best, bc) = (cand, d);
                }
            }
            rad /= 2;
        }
        (best, bc)
    }

    /// One PatchMatch pass over all target rows, in bands of the grid's rows: each band is
    /// visited in raster order (backwards on odd passes), propagating within itself, its random
    /// numbers seeded by its index. `nnf` and `cost` are over the target box. The bands run in
    /// parallel; when there are fewer of them than threads, each is also swept in wavefronts of
    /// blocks (a cell depends only on its neighbours before it in its row and its column), which
    /// visits every cell with the same neighbours and random numbers as the raster order.
    pub(super) fn patchmatch(
        &self,
        nnf: &mut [(i32, i32)],
        cost: &mut [f32],
        pass: usize,
        seed: u64,
        search_radius: i32,
    ) {
        const BAND: usize = 16;
        let tb = self.tbox;
        let tw = tb.width();
        if tw == 0 || tb.height() == 0 || nnf.len() < tb.area() || cost.len() < tb.area() {
            return;
        }
        let reverse = pass % 2 == 1;
        let step: i32 = if reverse { -1 } else { 1 };
        let band_seed = |bi: usize| {
            seed ^ (bi as u64).wrapping_mul(0x2545_F491_4F6C_DD1D) ^ ((pass as u64) << 40)
        };
        // The bands crossing the target box: index, first row, rows in the box.
        let mut bands = Vec::new();
        let mut y = tb.y0;
        while y < tb.y1 {
            let bi = y / BAND;
            let end = ((bi + 1) * BAND).min(tb.y1);
            bands.push((bi, y, end - y));
            y = end;
        }
        let threads = par::threads();
        if tb.area() >= PAR_MIN_PATCHES * 8 && !bands.is_empty() && bands.len() < threads {
            self.patchmatch_wavefront(nnf, cost, &bands, reverse, &band_seed, search_radius);
            return;
        }
        let band_fn = |bi: usize, ys: usize, nn: &mut [(i32, i32)], co: &mut [f32]| {
            if self.ctl.cancelled() {
                return;
            }
            let rows = nn.len() / tw;
            let mut rng = Rng::new(band_seed(bi));
            for ry in 0..rows {
                if self.ctl.cancelled() {
                    return;
                }
                let ly = if reverse { rows - 1 - ry } else { ry };
                let y = ys + ly;
                for rx in 0..tw {
                    let lx = if reverse { tw - 1 - rx } else { rx };
                    let li = ly * tw + lx;
                    let k = (y - tb.y0) * tw + lx;
                    if !self.targets[k] {
                        continue;
                    }
                    // Neighbours outside the target box hold no match: nothing to try.
                    let nx = lx as i32 - step;
                    let left = (nx >= 0 && (nx as usize) < tw).then(|| nn[ly * tw + nx as usize]);
                    let nly = ly as i32 - step;
                    let up =
                        (nly >= 0 && (nly as usize) < rows).then(|| nn[nly as usize * tw + lx]);
                    let t = ((tb.x0 + lx) as i32, y as i32);
                    (nn[li], co[li]) = self.pm_step(
                        t,
                        k,
                        (nn[li], co[li]),
                        left,
                        up,
                        step,
                        &mut rng,
                        search_radius,
                    );
                }
            }
        };
        // The target box's rows, band by band.
        let mut parts = Vec::with_capacity(bands.len());
        let (mut nn_rest, mut co_rest) = (&mut nnf[..tb.area()], &mut cost[..tb.area()]);
        for &(bi, ys, rows) in &bands {
            let (nn, nr) = std::mem::take(&mut nn_rest).split_at_mut(rows * tw);
            let (co, cr) = std::mem::take(&mut co_rest).split_at_mut(rows * tw);
            parts.push((bi, ys, nn, co));
            (nn_rest, co_rest) = (nr, cr);
        }
        let par = tb.area() >= PAR_MIN_PATCHES && parts.len() > 1;
        par::each(parts, par, |(bi, ys, nn, co)| band_fn(bi, ys, nn, co));
    }

    /// [`Solver::patchmatch`] swept in wavefronts: the cells of each band in blocks of a row, a
    /// block once the block before it in its row and the one above it (in the pass's direction)
    /// are done. A target's random numbers are those it would draw in raster order (each target
    /// draws the same count).
    fn patchmatch_wavefront(
        &self,
        nnf: &mut [(i32, i32)],
        cost: &mut [f32],
        bands: &[(usize, usize, usize)],
        reverse: bool,
        band_seed: &(dyn Fn(usize) -> u64 + Sync),
        search_radius: i32,
    ) {
        use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};

        let tb = self.tbox;
        let tw = tb.width();
        let step: i32 = if reverse { -1 } else { 1 };
        let pack = |(x, y): (i32, i32)| (u64::from(x as u32) << 32) | u64::from(y as u32);
        let unpack = |v: u64| ((v >> 32) as u32 as i32, v as u32 as i32);
        // Random numbers each target draws.
        let mut draws = 0u64;
        let mut rad = search_radius;
        while rad >= 1 {
            draws += 2;
            rad /= 2;
        }
        // Targets before each column of each row of the box.
        let mut before = vec![0u32; tb.height() * (tw + 1)];
        for (ry, row) in before.chunks_mut(tw + 1).enumerate() {
            for lx in 0..tw {
                row[lx + 1] = row[lx] + u32::from(self.targets[ry * tw + lx]);
            }
        }
        // Targets visited before each of a band's rows (in the pass's order).
        let row_base: Vec<Vec<u64>> = bands
            .iter()
            .map(|&(_, ys, rows)| {
                let mut base = Vec::with_capacity(rows);
                let mut n = 0u64;
                for pr in 0..rows {
                    base.push(n);
                    let ly = if reverse { rows - 1 - pr } else { pr };
                    n += u64::from(before[(ys + ly - tb.y0) * (tw + 1) + tw]);
                }
                base
            })
            .collect();
        let block = (tw / 8).clamp(4, 32);
        let blocks = tw.div_ceil(block);
        let nn: Vec<AtomicU64> = nnf[..tb.area()]
            .iter()
            .map(|m| AtomicU64::new(pack(*m)))
            .collect();
        let co: Vec<AtomicU32> = cost[..tb.area()]
            .iter()
            .map(|c| AtomicU32::new(c.to_bits()))
            .collect();
        let sweep = |(j, pr, b): (usize, usize, usize)| {
            let (bi, ys, rows) = bands[j];
            let ly = if reverse { rows - 1 - pr } else { pr };
            let y = ys + ly;
            let ry = y - tb.y0;
            let (pc0, pc1) = (b * block, ((b + 1) * block).min(tw));
            // The targets visited before this block, in raster order.
            let first = if reverse { tw - 1 - pc0 } else { pc0 };
            let row = &before[ry * (tw + 1)..(ry + 1) * (tw + 1)];
            let in_row = if reverse {
                row[tw] - row[first + 1]
            } else {
                row[first]
            };
            let mut rng = Rng::new(band_seed(bi));
            rng.skip((row_base[j][pr] + u64::from(in_row)) * draws);
            for pc in pc0..pc1 {
                let lx = if reverse { tw - 1 - pc } else { pc };
                let k = ry * tw + lx;
                if !self.targets[k] {
                    continue;
                }
                let nx = lx as i32 - step;
                let left = (nx >= 0 && (nx as usize) < tw)
                    .then(|| unpack(nn[ry * tw + nx as usize].load(Relaxed)));
                let nly = ly as i32 - step;
                let up = (nly >= 0 && (nly as usize) < rows)
                    .then(|| unpack(nn[(ys + nly as usize - tb.y0) * tw + lx].load(Relaxed)));
                let cur = (
                    unpack(nn[k].load(Relaxed)),
                    f32::from_bits(co[k].load(Relaxed)),
                );
                let t = ((tb.x0 + lx) as i32, y as i32);
                let (m, c) = self.pm_step(t, k, cur, left, up, step, &mut rng, search_radius);
                nn[k].store(pack(m), Relaxed);
                co[k].store(c.to_bits(), Relaxed);
            }
        };
        // Wavefront `d`: block `b` of (visited) row `pr` with `pr + b = d`, in every band. Each
        // wave ends before the next starts.
        let longest = bands.iter().map(|b| b.2).max().unwrap_or(0);
        for d in 0..longest + blocks - 1 {
            if self.ctl.cancelled() {
                return;
            }
            let wave: Vec<(usize, usize, usize)> = bands
                .iter()
                .enumerate()
                .flat_map(|(j, &(_, _, rows))| {
                    (0..rows)
                        .filter(move |&pr| pr <= d && d - pr < blocks)
                        .map(move |pr| (j, pr, d - pr))
                })
                .collect();
            par::each(wave, true, sweep);
        }
        for (m, a) in nnf.iter_mut().zip(&nn) {
            *m = unpack(a.load(Relaxed));
        }
        for (c, a) in cost.iter_mut().zip(&co) {
            *c = f32::from_bits(a.load(Relaxed));
        }
    }

    /// Recompute every target's match cost (hole pixels change between iterations), and with
    /// colour adaptation the full target patches' own moments.
    pub(super) fn costs(
        &mut self,
        nnf: &[(i32, i32)],
        cost: &mut [f32],
    ) -> Result<(), rust_motion_raster::Cancelled> {
        let (w, ch, r, tb) = (self.w, self.ch, self.r as usize, self.tbox);
        let par = tb.area() >= PAR_MIN_PATCHES;
        self.ctl.check()?;
        if !self.adapt.is_none() {
            let c8 = ch.min(MOM_CH);
            let side = 2 * r + 1;
            let mut tmom = std::mem::take(&mut self.tmom);
            let (img, full, ctl) = (&*self.img, &self.full, &self.ctl);
            par::chunks_mut(&mut tmom, tb.width() * c8, par, |ty, out| {
                if ctl.cancelled() {
                    return;
                }
                for tx in 0..tb.width() {
                    if full[ty * tb.width() + tx] {
                        patch_moments(
                            img,
                            w,
                            ch,
                            c8,
                            side,
                            tb.x0 + tx - r,
                            tb.y0 + ty - r,
                            &mut out[tx * c8..(tx + 1) * c8],
                        );
                    }
                }
            });
            self.tmom = tmom;
        }
        let this = &*self;
        par::chunks_mut(cost, tb.width(), par, |ty, co| {
            if this.ctl.cancelled() {
                return;
            }
            let y = tb.y0 + ty;
            for (tx, c) in co.iter_mut().enumerate() {
                let k = ty * tb.width() + tx;
                if this.targets[k] {
                    *c = this.dist(((tb.x0 + tx) as i32, y as i32), k, nnf[k], f32::INFINITY);
                }
            }
        });
        self.ctl.check()
    }

    /// Re-estimate hole pixels by a similarity-weighted vote of all overlapping patch matches
    /// (each copy adjusted by its gain and bias).
    pub(super) fn vote(&mut self, nnf: &[(i32, i32)], cost: &[f32]) {
        let (w, h, ch, r) = (self.w, self.h, self.ch, self.r);
        let (tb, hb) = (self.tbox, self.hole_box);
        // A cancelled solve is thrown away.
        if self.ctl.cancelled() {
            return;
        }
        let used = |k: usize| self.targets[k] && cost[k].is_finite();
        // σ² from the 75th percentile of match costs (Wexler et al. use a robust scale like this).
        let cs: Vec<f32> = (0..tb.area())
            .filter(|&k| used(k))
            .map(|k| cost[k])
            .collect();
        let mut cs = cs;
        let sigma2 = if cs.is_empty() {
            1.0
        } else {
            let k = (cs.len() * 3 / 4).min(cs.len() - 1);
            let (_, v, _) = cs.select_nth_unstable_by(k, |a, b| a.total_cmp(b));
            v.max(1e-6)
        };
        drop(cs);
        // Each target's weight and photometric transform (computed once, used by every pixel it
        // covers).
        let adapted = !self.adapt.is_none();
        let par = tb.area() >= PAR_MIN_PATCHES;
        // A weight (zero for a target that doesn't vote) and its match's centre (as a pixel
        // index) per target.
        let mut wt = vec![0.0f32; tb.area()];
        let mut src = vec![0isize; tb.area()];
        par::chunks2_mut(
            &mut wt,
            tb.width(),
            &mut src,
            tb.width(),
            par,
            |ty, wrow, srow| {
                for (tx, (o, sc)) in wrow.iter_mut().zip(srow.iter_mut()).enumerate() {
                    let k = ty * tb.width() + tx;
                    if used(k) {
                        *o = (-cost[k] / (2.0 * sigma2)).exp().max(1e-8);
                        *sc = nnf[k].1 as isize * w as isize + nnf[k].0 as isize;
                    }
                }
            },
        );
        let mut gb: Vec<(f32, f32)> = Vec::new();
        if adapted {
            gb = vec![(1.0f32, 0.0f32); tb.area() * ch];
            let this = &*self;
            par::chunks_mut(&mut gb, tb.width() * ch, par, |ty, out| {
                if this.ctl.cancelled() {
                    return;
                }
                let y = tb.y0 + ty;
                for tx in 0..tb.width() {
                    let k = ty * tb.width() + tx;
                    if used(k) {
                        this.transform(
                            ((tb.x0 + tx) as i32, y as i32),
                            k,
                            nnf[k],
                            &mut out[tx * ch..(tx + 1) * ch],
                        );
                    }
                }
            });
        }
        // The vote, into a buffer over the hole's bounds (every pixel reads the old pixels).
        let votes = Votes {
            w,
            h,
            ch,
            r: r as usize,
            tbox: tb,
            img: &*self.img,
            wt: &wt,
            src: &src,
            gb: &gb,
        };
        let (px, ctl) = (self.px, &self.ctl);
        let mut out = vec![0.0f32; hb.area() * ch];
        let mut got = vec![false; hb.area()];
        let row = |yy: usize, orow: &mut [f32], grow: &mut [bool]| {
            if ctl.cancelled() {
                return;
            }
            let y = hb.y0 + yy;
            for x in hb.x0..hb.x1 {
                if px[y * w + x] != Px::Hole {
                    continue;
                }
                let o = (x - hb.x0) * ch;
                let dst = &mut orow[o..o + ch];
                grow[x - hb.x0] = match (ch, adapted) {
                    (1, false) => votes.pixel::<1, false>(x, y, dst),
                    (3, false) => votes.pixel::<3, false>(x, y, dst),
                    (4, false) => votes.pixel::<4, false>(x, y, dst),
                    (1, true) => votes.pixel::<1, true>(x, y, dst),
                    (3, true) => votes.pixel::<3, true>(x, y, dst),
                    (4, true) => votes.pixel::<4, true>(x, y, dst),
                    _ => votes.pixel_any(x, y, adapted, dst),
                };
            }
        };
        par::chunks2_mut(
            &mut out,
            hb.width() * ch,
            &mut got,
            hb.width(),
            hb.area() >= PAR_MIN_PATCHES,
            row,
        );
        for yy in 0..hb.height() {
            for xx in 0..hb.width() {
                if got[yy * hb.width() + xx] {
                    let (d, s) = (
                        ((hb.y0 + yy) * w + hb.x0 + xx) * ch,
                        (yy * hb.width() + xx) * ch,
                    );
                    self.img[d..d + ch].copy_from_slice(&out[s..s + ch]);
                }
            }
        }
    }
}
