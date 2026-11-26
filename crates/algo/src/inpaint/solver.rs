//! The per-level PatchMatch/EM state, patch distances and the colour-adaptation fit.

use super::*;
use crate::par;

/// Most channels whose moments the colour adaptation fits (the rest are copied as they are).
pub(super) const MOM_CH: usize = 8;

/// Per-level PatchMatch/EM state.
pub(super) struct Solver<'a> {
    pub(super) w: usize,
    pub(super) h: usize,
    pub(super) ch: usize,
    pub(super) r: i32,
    pub(super) img: &'a mut Vec<f32>,
    pub(super) px: &'a [Px],
    /// Valid source centres: the whole patch is inside the grid and may be copied from.
    pub(super) valid: Vec<bool>,
    /// The hole's bounds and the targets' (the hole's grown by the patch radius): nothing outside
    /// them changes or is matched. The match field, its costs and these per-target tables are
    /// kept over `tbox`, row-major.
    pub(super) hole_box: Bounds,
    pub(super) tbox: Bounds,
    /// Target centres (the patch overlaps the hole).
    pub(super) targets: Vec<bool>,
    /// Per cell of `tbox`: the whole patch is inside the grid and not void (so its distances
    /// need no bounds checks and its own moments can be kept).
    pub(super) full: Vec<bool>,
    /// With colour adaptation, per cell of `tbox` and channel (at most [`MOM_CH`]): `Σt, Σt²` of
    /// the patch, for full patches, as of the last [`Solver::costs`].
    pub(super) tmom: Vec<[f32; 2]>,
    /// With colour adaptation where it pays: per source centre and channel, `Σs, Σs²` of its
    /// patch (source pixels never change within a level). Empty otherwise.
    pub(super) smom: Vec<[f32; 2]>,
    pub(super) adapt: Adapt,
    /// Cancellation: checked per row band; a cancelled solve's output is discarded.
    pub(super) ctl: rust_motion_raster::Interrupt<'a>,
}

/// A channel deviation under which a patch counts as flat for [`fit`] (about 5 levels of 255).
const FLAT: f32 = 0.02;

/// Gain and bias of one channel for target moments `(Σt, Σt²)` and source moments `(Σs, Σs²)`
/// over `n` samples, within the limits.
#[inline]
pub(crate) fn fit(a: Adapt, n: f32, st: f32, stt: f32, ss: f32, sss: f32) -> (f32, f32) {
    let (mt, ms) = (st / n, ss / n);
    let vt = (stt / n - mt * mt).max(0.0);
    let vs = (sss / n - ms * ms).max(0.0);
    let g = if vs > 1e-10 { (vt / vs).sqrt() } else { 1.0 };
    let (mut lo, mut hi) = a.gain;
    // Nearly flat patches of about the same colour (their means within the bias of each other):
    // the ratio of their deviations is noise, so match the mean first, keeping the gain where the
    // bias can still bring the source's mean to the target's. Gained about zero, a gain at its
    // limit moved bright means further than the bias could bring back, and each channel by a
    // different amount: a tinted blotch in a smooth gradient. Textured patches, and flat ones
    // whose colours differ more (Content-Aware Move's Color), are fitted as before.
    if vt.max(vs) < FLAT * FLAT && (mt - ms).abs() <= a.bias && ms > 1e-6 {
        let (glo, ghi) = ((mt - a.bias) / ms, (mt + a.bias) / ms);
        if glo.max(lo) <= ghi.min(hi) {
            (lo, hi) = (glo.max(lo), ghi.min(hi));
        }
    }
    let g = g.clamp(lo, hi);
    let b = (mt - g * ms).clamp(-a.bias, a.bias);
    (g, b)
}

impl Solver<'_> {
    #[inline]
    pub(super) fn is_valid(&self, x: i32, y: i32) -> bool {
        x >= 0
            && y >= 0
            && (x as usize) < self.w
            && (y as usize) < self.h
            && self.valid[y as usize * self.w + x as usize]
    }

    /// Mark the full target patches (`full`), and keep the source moments where they pay off.
    pub(super) fn prepare(&mut self, keep_source_moments: bool) {
        let (w, h, r, tb) = (self.w, self.h, self.r as usize, self.tbox);
        let side = 2 * r + 1;
        let mut full = vec![false; tb.area()];
        let px = self.px;
        par::chunks_mut(
            &mut full,
            tb.width(),
            tb.area() >= PAR_MIN_PATCHES,
            |ty, out| {
                let y = tb.y0 + ty;
                if y < r || y + r >= h {
                    return;
                }
                for (tx, f) in out.iter_mut().enumerate() {
                    let x = tb.x0 + tx;
                    if x < r || x + r >= w {
                        continue;
                    }
                    *f = (y - r..=y + r).all(|yy| {
                        px[yy * w + x - r..yy * w + x - r + side]
                            .iter()
                            .all(|k| *k != Px::Void)
                    });
                }
            },
        );
        self.full = full;
        if self.adapt.is_none() {
            return;
        }
        let c8 = self.ch.min(MOM_CH);
        self.tmom = vec![[0.0; 2]; tb.area() * c8];
        if !keep_source_moments {
            return;
        }
        let (ch, img, valid, ctl) = (self.ch, &*self.img, &self.valid, &self.ctl);
        let mut smom = vec![[0.0f32; 2]; w * h * c8];
        par::chunks_mut(&mut smom, w * c8, w * h >= PAR_MIN_PATCHES, |y, out| {
            if ctl.cancelled() {
                return;
            }
            for x in 0..w {
                if valid[y * w + x] {
                    patch_moments(
                        img,
                        w,
                        ch,
                        c8,
                        side,
                        x - r,
                        y - r,
                        &mut out[x * c8..(x + 1) * c8],
                    );
                }
            }
        });
        self.smom = smom;
    }

    /// Distance between the target patch at `t` (its pixels inside the grid and not void; `k` its
    /// index in the `tbox` tables) and the source patch at `s`, per sample; with colour adaptation
    /// the source is fitted first.
    #[inline]
    pub(super) fn dist(&self, t: (i32, i32), k: usize, s: (i32, i32), cutoff: f32) -> f32 {
        let full = self.full[k];
        if !self.adapt.is_none() {
            return if full {
                self.dist_adapted_full(k, t, s)
            } else {
                self.dist_adapted(t, s)
            };
        }
        if full {
            self.dist_full(t, s, cutoff)
        } else {
            self.dist_clipped(t, s, cutoff)
        }
    }

    /// [`Solver::dist`] without adaptation, of a patch that may be clipped.
    pub(super) fn dist_clipped(&self, t: (i32, i32), s: (i32, i32), cutoff: f32) -> f32 {
        let (w, h, ch, r) = (self.w as i32, self.h as i32, self.ch, self.r);
        let full = ((2 * r + 1) * (2 * r + 1)) as f32 * ch as f32;
        let mut sum = 0.0f32;
        let mut n = 0usize;
        for dy in -r..=r {
            let ty = t.1 + dy;
            if ty < 0 || ty >= h {
                continue;
            }
            let sy = s.1 + dy;
            for dx in -r..=r {
                let tx = t.0 + dx;
                if tx < 0 || tx >= w || self.px[ty as usize * self.w + tx as usize] == Px::Void {
                    continue;
                }
                let ti = (ty as usize * self.w + tx as usize) * ch;
                let si = (sy as usize * self.w + (s.0 + dx) as usize) * ch;
                for c in 0..ch {
                    let d = self.img[ti + c] - self.img[si + c];
                    sum += d * d;
                }
                n += ch;
            }
            // Early out: the mean can only be ≥ sum / (full patch size).
            if sum > cutoff * full {
                return f32::INFINITY;
            }
        }
        if n == 0 {
            f32::INFINITY
        } else {
            sum / n as f32
        }
    }

    /// [`Solver::dist_clipped`] of a full patch: the same sums, in the same order. It gives up
    /// (infinity) as soon as the sum shows the patch is worse than `cutoff`, which the sum, only
    /// growing, can show after any pixel: the caller only asks whether it is better.
    pub(super) fn dist_full(&self, t: (i32, i32), s: (i32, i32), cutoff: f32) -> f32 {
        let (w, ch, r) = (self.w, self.ch, self.r);
        let side = (2 * r + 1) as usize;
        let full = ((2 * r + 1) * (2 * r + 1)) as f32 * ch as f32;
        let (span, limit) = (side * ch, cutoff * full);
        let mut sum = 0.0f32;
        for dy in -r..=r {
            let ti = ((t.1 + dy) as usize * w + (t.0 - r) as usize) * ch;
            let si = ((s.1 + dy) as usize * w + (s.0 - r) as usize) * ch;
            for (tp, sp) in self.img[ti..ti + span]
                .chunks_exact(ch)
                .zip(self.img[si..si + span].chunks_exact(ch))
            {
                for (a, b) in tp.iter().zip(sp) {
                    let d = a - b;
                    sum += d * d;
                }
                if sum > limit {
                    return f32::INFINITY;
                }
            }
        }
        sum / (side * side * ch) as f32
    }

    /// Moments of the target patch at `t` and the source patch at `s` per channel:
    /// `[Σt, Σt², Σs, Σs², Σts]`, and the sample count.
    pub(super) fn moments(&self, t: (i32, i32), s: (i32, i32), m: &mut [[f32; 5]]) -> usize {
        let (w, h, ch, r) = (self.w as i32, self.h as i32, self.ch, self.r);
        m.iter_mut().for_each(|v| *v = [0.0; 5]);
        let mut n = 0usize;
        for dy in -r..=r {
            let ty = t.1 + dy;
            if ty < 0 || ty >= h {
                continue;
            }
            let sy = s.1 + dy;
            for dx in -r..=r {
                let tx = t.0 + dx;
                if tx < 0 || tx >= w || self.px[ty as usize * self.w + tx as usize] == Px::Void {
                    continue;
                }
                let ti = (ty as usize * self.w + tx as usize) * ch;
                let si = (sy as usize * self.w + (s.0 + dx) as usize) * ch;
                for (c, mc) in m.iter_mut().enumerate().take(ch) {
                    let (a, b) = (self.img[ti + c], self.img[si + c]);
                    mc[0] += a;
                    mc[1] += a * a;
                    mc[2] += b;
                    mc[3] += b * b;
                    mc[4] += a * b;
                }
                n += 1;
            }
        }
        n
    }

    /// [`Solver::moments`] of a full target patch (`k` in the `tbox` tables): the target's own
    /// sums kept from [`Solver::costs`], the source's from [`Solver::prepare`] when kept, each
    /// summed in the same order as there.
    pub(super) fn moments_full(&self, k: usize, t: (i32, i32), s: (i32, i32), m: &mut [[f32; 5]]) {
        let (w, ch, r) = (self.w, self.ch, self.r);
        let c8 = m.len();
        let side = (2 * r + 1) as usize;
        let tm = &self.tmom[k * c8..(k + 1) * c8];
        let sm = self.smom.get(
            (s.1 as usize * w + s.0 as usize) * c8..(s.1 as usize * w + s.0 as usize + 1) * c8,
        );
        let mut acc = [[0.0f32; 3]; MOM_CH];
        for dy in -r..=r {
            let ti = ((t.1 + dy) as usize * w + (t.0 - r) as usize) * ch;
            let si = ((s.1 + dy) as usize * w + (s.0 - r) as usize) * ch;
            let (trow, srow) = (&self.img[ti..ti + side * ch], &self.img[si..si + side * ch]);
            if sm.is_some() {
                for (tp, sp) in trow.chunks_exact(ch).zip(srow.chunks_exact(ch)) {
                    for c in 0..c8 {
                        acc[c][2] += tp[c] * sp[c];
                    }
                }
            } else {
                for (tp, sp) in trow.chunks_exact(ch).zip(srow.chunks_exact(ch)) {
                    for c in 0..c8 {
                        let (a, b) = (tp[c], sp[c]);
                        acc[c][0] += b;
                        acc[c][1] += b * b;
                        acc[c][2] += a * b;
                    }
                }
            }
        }
        for (c, mc) in m.iter_mut().enumerate() {
            let [ss, sss] = match sm {
                Some(sm) => sm[c],
                None => [acc[c][0], acc[c][1]],
            };
            *mc = [tm[c][0], tm[c][1], ss, sss, acc[c][2]];
        }
    }

    pub(super) fn dist_adapted(&self, t: (i32, i32), s: (i32, i32)) -> f32 {
        let mut m = [[0.0f32; 5]; MOM_CH];
        let ch = self.ch.min(MOM_CH);
        let n = self.moments(t, s, &mut m[..ch]);
        if n == 0 {
            return f32::INFINITY;
        }
        adapted_cost(self.adapt, n as f32, &m[..ch])
    }

    pub(super) fn dist_adapted_full(&self, k: usize, t: (i32, i32), s: (i32, i32)) -> f32 {
        let mut m = [[0.0f32; 5]; MOM_CH];
        let ch = self.ch.min(MOM_CH);
        self.moments_full(k, t, s, &mut m[..ch]);
        let side = (2 * self.r + 1) as usize;
        adapted_cost(self.adapt, (side * side) as f32, &m[..ch])
    }

    /// Gain and bias per channel for the match `t → s` (identity without adaptation).
    pub(super) fn transform(&self, t: (i32, i32), k: usize, s: (i32, i32), out: &mut [(f32, f32)]) {
        if self.adapt.is_none() {
            out.iter_mut().for_each(|v| *v = (1.0, 0.0));
            return;
        }
        let mut m = [[0.0f32; 5]; MOM_CH];
        let ch = self.ch.min(MOM_CH);
        let n = if self.full[k] {
            self.moments_full(k, t, s, &mut m[..ch]);
            let side = (2 * self.r + 1) as usize;
            side * side
        } else {
            self.moments(t, s, &mut m[..ch])
        };
        for (c, o) in out.iter_mut().enumerate() {
            *o = if n == 0 || c >= ch {
                (1.0, 0.0)
            } else {
                fit(self.adapt, n as f32, m[c][0], m[c][1], m[c][2], m[c][3])
            };
        }
    }
}
