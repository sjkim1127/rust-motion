//! Tests for hole filling.

use super::*;

mod completion;
mod onion;
mod proximity;
mod sources;
mod thin_holes;

/// Smooth gradient plus deterministic fine noise.
pub(super) fn texture(w: usize, h: usize) -> Vec<f32> {
    let mut rng = Rng::new(7);
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = ((i % w) as f32, (i / w) as f32);
            let n = (rng.next() % 1000) as f32 / 1000.0 * 0.04 - 0.02;
            [0.3 + 0.004 * x + n, 0.5 + 0.002 * y + n, 0.4 + n]
        })
        .collect()
}

pub(super) fn disc(w: usize, h: usize, cx: f32, cy: f32, r: f32) -> Vec<bool> {
    (0..w * h)
        .map(|i| ((i % w) as f32 - cx).hypot((i / w) as f32 - cy) < r)
        .collect()
}

pub(super) fn hole_rmse(a: &[f32], b: &[f32], hole: &[bool], ch: usize) -> f32 {
    let (mut e, mut n) = (0.0f32, 0);
    for (i, hh) in hole.iter().enumerate() {
        if *hh {
            for c in 0..ch {
                e += (a[i * ch + c] - b[i * ch + c]).powi(2);
                n += 1;
            }
        }
    }
    (e / n as f32).sqrt()
}

/// Structure, fine noise and flat patches (so that some matches tie exactly), `ch` channels.
pub(super) fn busy(w: usize, h: usize, ch: usize, seed: u64) -> Vec<f32> {
    let mut rng = Rng::new(seed);
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            let flat = (x / 9 + y / 7) % 5 == 0;
            let n = (rng.next() % 1000) as f32 / 1000.0;
            (0..ch).map(move |c| {
                if flat {
                    0.25 + 0.1 * c as f32
                } else {
                    (0.2 + 0.004 * x as f32 + 0.1 * ((x + 2 * y + c) % 6) as f32 + 0.05 * n)
                        .min(1.0)
                }
            })
        })
        .collect()
}

pub(super) fn bits(v: &[f32]) -> Vec<u32> {
    v.iter().map(|x| x.to_bits()).collect()
}

/// Roles: a hole, a band that is context only, a corner that is no image.
pub(super) fn roles(w: usize, h: usize, hole: &[bool]) -> Vec<Px> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if hole[i] {
                Px::Hole
            } else if x < 4 && y < 6 {
                Px::Void
            } else if (w * 3 / 4..w * 3 / 4 + 3).contains(&x) {
                Px::Known
            } else {
                Px::Source
            }
        })
        .collect()
}
