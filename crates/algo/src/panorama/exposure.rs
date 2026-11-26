//! Photometric compensation: per-image gain and a shared radial vignetting model.

use super::*;
use crate::photo_util::Normal;

// ---------- photometric ----------

/// One image warped into the panorama: an axis-aligned region with colour channels and coverage.
#[derive(Clone, Debug)]
pub struct RoiImage {
    pub x0: i32,
    pub y0: i32,
    pub w: usize,
    pub h: usize,
    /// Colour channels per pixel (no alpha).
    pub ch: usize,
    pub px: Vec<f32>,
    /// Coverage `0..=1`.
    pub alpha: Vec<f32>,
}

impl RoiImage {
    fn at(&self, x: i32, y: i32) -> Option<usize> {
        let (lx, ly) = (x - self.x0, y - self.y0);
        (lx >= 0 && ly >= 0 && (lx as usize) < self.w && (ly as usize) < self.h)
            .then(|| ly as usize * self.w + lx as usize)
    }
}

/// Placements and source sizes (for the vignetting radius).
pub type Geometry<'a> = (&'a [Placement], &'a [(usize, usize)]);

/// Gains (per image, per channel) and the vignetting model found by [`photometric`].
#[derive(Clone, Debug, PartialEq)]
pub struct Photometric {
    pub gains: Vec<Vec<f32>>,
    /// `log V(r) = a·r² + b·r⁴`, `r` = distance from the image centre over the half diagonal.
    pub vignette: [f64; 2],
}

impl Photometric {
    /// Multiplier that corrects a pixel of image `i` at normalised squared radius `r2`.
    pub fn factor(&self, i: usize, c: usize, r2: f64) -> f32 {
        let v = (self.vignette[0] * r2 + self.vignette[1] * r2 * r2).exp();
        (1.0 / (self.gains[i][c] as f64 * v)) as f32
    }
}

/// Estimates exposure gains (`gain`) and a shared vignetting falloff (`vignette`) from the
/// overlaps of `imgs` (`placements`/`sizes` give each panorama pixel's source radius).
/// Without `geometry` (placements and source sizes) the vignetting model is not estimated.
pub fn photometric(
    imgs: &[RoiImage],
    geometry: Option<Geometry>,
    gain: bool,
    vignette: bool,
) -> Photometric {
    let vignette = vignette && geometry.is_some();
    let n = imgs.len();
    let ch = imgs.first().map_or(1, |i| i.ch);
    let identity = Photometric {
        gains: vec![vec![1.0; ch]; n],
        vignette: [0.0, 0.0],
    };
    if n == 0 || (!gain && !vignette) {
        return identity;
    }
    // Overlap samples on a grid: (a, b, colours, radii²).
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for im in imgs {
        x0 = x0.min(im.x0);
        y0 = y0.min(im.y0);
        x1 = x1.max(im.x0 + im.w as i32);
        y1 = y1.max(im.y0 + im.h as i32);
    }
    let step = (((x1 - x0) as i64 * (y1 - y0) as i64 / 40_000).max(1) as f64)
        .sqrt()
        .ceil() as i32;
    let r2_of = |i: usize, u: i32, v: i32| -> Option<f64> {
        let Some((placements, sizes)) = geometry else {
            return Some(0.0);
        };
        let (sx, sy) = placements[i].inverse(u as f64 + 0.5, v as f64 + 0.5)?;
        let (w, h) = (sizes[i].0 as f64, sizes[i].1 as f64);
        let (dx, dy) = (sx - w / 2.0, sy - h / 2.0);
        Some((dx * dx + dy * dy) / (w * w / 4.0 + h * h / 4.0))
    };
    type Sample = (usize, usize, Vec<f32>, Vec<f32>, f64, f64);
    let mut samples: Vec<Sample> = Vec::new();
    let mut y = y0;
    while y < y1 {
        let mut x = x0;
        while x < x1 {
            let cover: Vec<(usize, usize)> = (0..n)
                .filter_map(|i| {
                    imgs[i]
                        .at(x, y)
                        .filter(|&k| imgs[i].alpha[k] > 0.999)
                        .map(|k| (i, k))
                })
                .collect();
            for p in 0..cover.len() {
                for q in p + 1..cover.len() {
                    let ((a, ka), (b, kb)) = (cover[p], cover[q]);
                    let ca = imgs[a].px[ka * ch..(ka + 1) * ch].to_vec();
                    let cb = imgs[b].px[kb * ch..(kb + 1) * ch].to_vec();
                    if ca.iter().chain(&cb).any(|v| !(0.02..=0.98).contains(v)) {
                        continue;
                    }
                    // Disagreeing content (parallax, moving objects) says nothing about exposure.
                    if ca.iter().zip(&cb).any(|(x, y)| (x / y).ln().abs() > 1.0) {
                        continue;
                    }
                    let (Some(ra), Some(rb)) = (r2_of(a, x, y), r2_of(b, x, y)) else {
                        continue;
                    };
                    samples.push((a, b, ca, cb, ra, rb));
                }
            }
            x += step;
        }
        y += step;
    }
    if samples.len() < 20 {
        return identity;
    }
    // Unknowns: log gain per image (per channel), then a, b (shared, from mean log intensity).
    let sigma_n = 0.05;
    let sigma_g = if gain { 0.15 } else { 1e-4 };
    let mut out = identity.clone();
    let nv = if vignette { 2 } else { 0 };
    // Vignetting is solved on the channel mean, then gains per channel with it fixed.
    let mut ne = Normal::new(n + nv);
    for (a, b, ca, cb, ra, rb) in &samples {
        let la = ca.iter().map(|v| (*v as f64).ln()).sum::<f64>() / ch as f64;
        let lb = cb.iter().map(|v| (*v as f64).ln()).sum::<f64>() / ch as f64;
        let mut row = vec![(*a, 1.0), (*b, -1.0)];
        if vignette {
            row.push((n, ra - rb));
            row.push((n + 1, ra * ra - rb * rb));
        }
        ne.add(&row, la - lb, 1.0 / sigma_n);
    }
    for i in 0..n {
        ne.add(&[(i, 1.0)], 0.0, 1.0 / sigma_g);
    }
    if vignette {
        ne.add(&[(n, 1.0)], 0.0, 0.5);
        ne.add(&[(n + 1, 1.0)], 0.0, 0.5);
    }
    let Some(sol) = ne.solve() else { return out };
    if vignette {
        // Vignetting only darkens; clamp a runaway brightening fit.
        out.vignette = [sol[n].clamp(-2.0, 0.5), sol[n + 1].clamp(-2.0, 2.0)];
    }
    if gain {
        for c in 0..ch {
            let mut ne = Normal::new(n);
            for (a, b, ca, cb, ra, rb) in &samples {
                let va = out.vignette[0] * ra + out.vignette[1] * ra * ra;
                let vb = out.vignette[0] * rb + out.vignette[1] * rb * rb;
                ne.add(
                    &[(*a, 1.0), (*b, -1.0)],
                    ((ca[c] as f64).ln() - va) - ((cb[c] as f64).ln() - vb),
                    1.0 / sigma_n,
                );
            }
            for i in 0..n {
                ne.add(&[(i, 1.0)], 0.0, 1.0 / sigma_g);
            }
            if let Some(g) = ne.solve() {
                for i in 0..n {
                    out.gains[i][c] = g[i].exp().clamp(0.2, 5.0) as f32;
                }
            }
        }
    }
    out
}
