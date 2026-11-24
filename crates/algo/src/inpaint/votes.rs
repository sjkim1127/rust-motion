//! Per-pixel voting, patch moments and the exhaustive known-pixel distance scans.

use super::*;

/// What [`Solver::vote`] votes with: per target (over the target box) its weight (zero when it
/// doesn't vote), its match's centre as a pixel index, and with colour adaptation its gain and
/// bias per channel.
pub(super) struct Votes<'a> {
    pub(super) w: usize,
    pub(super) h: usize,
    pub(super) ch: usize,
    pub(super) r: usize,
    pub(super) tbox: Bounds,
    pub(super) img: &'a [f32],
    pub(super) wt: &'a [f32],
    pub(super) src: &'a [isize],
    pub(super) gb: &'a [(f32, f32)],
}

impl Votes<'_> {
    /// Hole pixel `(x, y)`'s vote into `out` (`CH` channels): the weighted mean of what the
    /// matches of the patches covering it put there, summed patch by patch in raster order of
    /// their centres. False when no patch votes.
    #[inline(always)]
    pub(super) fn pixel<const CH: usize, const ADAPT: bool>(
        &self,
        x: usize,
        y: usize,
        out: &mut [f32],
    ) -> bool {
        let (w, r, tb) = (self.w, self.r, self.tbox);
        let mut acc = [0.0f32; CH];
        let mut wsum = 0.0f32;
        for ty in y.saturating_sub(r)..=(y + r).min(self.h - 1) {
            let dy = ty as isize - y as isize;
            let krow = (ty - tb.y0) * tb.width();
            for tx in x.saturating_sub(r)..=(x + r).min(w - 1) {
                let k = krow + tx - tb.x0;
                let wt = self.wt[k];
                if wt == 0.0 {
                    continue;
                }
                // The pixel of the match's patch that lands here.
                let si = (self.src[k] - dy * w as isize - (tx as isize - x as isize)) as usize * CH;
                let s = &self.img[si..si + CH];
                for c in 0..CH {
                    let v = if ADAPT {
                        let (g, b) = self.gb[k * CH + c];
                        g * s[c] + b
                    } else {
                        s[c]
                    };
                    acc[c] += v * wt;
                }
                wsum += wt;
            }
        }
        if wsum > 0.0 {
            for (d, a) in out.iter_mut().zip(&acc) {
                *d = a / wsum;
            }
            return true;
        }
        false
    }

    /// [`Votes::pixel`] for any number of channels.
    pub(super) fn pixel_any(&self, x: usize, y: usize, adapted: bool, out: &mut [f32]) -> bool {
        let (w, r, tb, ch) = (self.w, self.r, self.tbox, self.ch);
        let mut acc = vec![0.0f32; ch];
        let mut wsum = 0.0f32;
        for ty in y.saturating_sub(r)..=(y + r).min(self.h - 1) {
            let dy = ty as isize - y as isize;
            let krow = (ty - tb.y0) * tb.width();
            for tx in x.saturating_sub(r)..=(x + r).min(w - 1) {
                let k = krow + tx - tb.x0;
                let wt = self.wt[k];
                if wt == 0.0 {
                    continue;
                }
                let si = (self.src[k] - dy * w as isize - (tx as isize - x as isize)) as usize * ch;
                for (c, a) in acc.iter_mut().enumerate() {
                    let v = if adapted {
                        let (g, b) = self.gb[k * ch + c];
                        g * self.img[si + c] + b
                    } else {
                        self.img[si + c]
                    };
                    *a += v * wt;
                }
                wsum += wt;
            }
        }
        if wsum > 0.0 {
            for (d, a) in out.iter_mut().zip(&acc) {
                *d = a / wsum;
            }
            return true;
        }
        false
    }
}

/// `Σv, Σv²` per channel (the first `out.len()`) of the `side × side` patch with top-left
/// `(x0, y0)`, summed row by row, pixel by pixel, as [`Solver::moments`] does.
#[allow(clippy::too_many_arguments)]
pub(super) fn patch_moments(
    img: &[f32],
    w: usize,
    ch: usize,
    c8: usize,
    side: usize,
    x0: usize,
    y0: usize,
    out: &mut [[f32; 2]],
) {
    out.iter_mut().for_each(|v| *v = [0.0; 2]);
    for y in y0..y0 + side {
        let row = &img[(y * w + x0) * ch..(y * w + x0 + side) * ch];
        for p in row.chunks_exact(ch) {
            for (c, o) in out.iter_mut().enumerate().take(c8) {
                let a = p[c];
                o[0] += a;
                o[1] += a * a;
            }
        }
    }
}

/// The adapted distance from the moments `[Σt, Σt², Σs, Σs², Σts]` of each channel over `n`
/// samples: what is left of the target after the source is fitted to it, per sample.
#[inline]
pub(super) fn adapted_cost(a: Adapt, nf: f32, m: &[[f32; 5]]) -> f32 {
    let mut sum = 0.0f32;
    for mc in m {
        let (g, b) = fit(a, nf, mc[0], mc[1], mc[2], mc[3]);
        // Σ (t − g s − b)², expanded.
        sum += mc[1] + g * g * mc[3] + nf * b * b - 2.0 * g * mc[4] - 2.0 * b * mc[0]
            + 2.0 * g * b * mc[2];
    }
    sum.max(0.0) / (nf * m.len() as f32)
}

/// `Σ (t − s)²` over a target patch's known pixels (offsets `offs` from its centre, samples
/// `tv`, `CH` per pixel) against the source patch centred at pixel `base`, summed in that order;
/// `None` once the sum reaches `limit` (it only grows).
#[inline(always)]
pub(super) fn known_sse<const CH: usize>(
    image: &[f32],
    offs: &[isize],
    tv: &[f32],
    base: isize,
    limit: f32,
) -> Option<f32> {
    let mut e = 0.0f32;
    for (o, t) in offs.iter().zip(tv.as_chunks::<CH>().0) {
        let si = (base + o) as usize * CH;
        let s = &image[si..si + CH];
        for c in 0..CH {
            let d = t[c] - s[c];
            e += d * d;
        }
        if e >= limit {
            return None;
        }
    }
    Some(e)
}

/// [`known_sse`] of [`LANES`] sources at once, each against its own limit (summed in step: the
/// sums are independent, so the processor overlaps them).
#[inline(always)]
pub(super) fn known_sse_lanes<const CH: usize>(
    image: &[f32],
    offs: &[isize],
    tv: &[f32],
    bases: [isize; LANES],
    limits: [f32; LANES],
) -> [Option<f32>; LANES] {
    let mut e = [0.0f32; LANES];
    for (o, t) in offs.iter().zip(tv.as_chunks::<CH>().0) {
        for l in 0..LANES {
            let si = (bases[l] + o) as usize * CH;
            let s = &image[si..si + CH];
            for c in 0..CH {
                let d = t[c] - s[c];
                e[l] += d * d;
            }
        }
        // Sums only grow: once all of them reached their limits, none can win.
        if (0..LANES).all(|l| e[l] >= limits[l]) {
            break;
        }
    }
    std::array::from_fn(|l| (e[l] < limits[l]).then_some(e[l]))
}

/// Sources [`known_sse_lanes`] sums at once.
pub(super) const LANES: usize = 4;

/// [`known_sse`] for any number of channels.
pub(super) fn known_sse_any(
    image: &[f32],
    ch: usize,
    offs: &[isize],
    tv: &[f32],
    base: isize,
    limit: f32,
) -> Option<f32> {
    let mut e = 0.0f32;
    for (o, t) in offs.iter().zip(tv.chunks_exact(ch.max(1))) {
        let si = (base + o) as usize * ch;
        for (tc, sc) in t.iter().zip(&image[si..si + ch]) {
            let d = tc - sc;
            e += d * d;
        }
        if e >= limit {
            return None;
        }
    }
    Some(e)
}

/// The best source so far of [`onion_peel`]'s search.
#[derive(Clone, Copy)]
pub(super) struct Best {
    /// Error per sample, and the place in the sources.
    pub(super) v: f32,
    pub(super) j: usize,
    /// Sums of squared errors from which `sum / n` is above `v`, and at least `v`.
    pub(super) worse: f32,
    pub(super) equal: f32,
}

/// The least sum `e ≥ 0` with `e / n > v` (`strict`) or `e / n ≥ v` in float arithmetic (the
/// quotient only grows with `e`); infinity for an infinite `v`.
pub(super) fn least_sum(v: f32, n: f32, strict: bool) -> f32 {
    let holds = |e: f32| if strict { e / n > v } else { e / n >= v };
    if !v.is_finite() || !n.is_finite() || n <= 0.0 {
        return f32::INFINITY;
    }
    let mut e = (v * n).max(0.0);
    while e > 0.0 && holds(e.next_down()) {
        e = e.next_down();
    }
    while !holds(e) && e < f32::INFINITY {
        e = e.next_up();
    }
    e
}
