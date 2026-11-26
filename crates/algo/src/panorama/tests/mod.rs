//! Tests for panorama stitching.

use super::*;
use crate::photo_util::Rng;

mod blending;
mod registration;

/// Smooth random texture: blobs over a gradient (bounded detail so Harris finds corners).
pub(crate) fn scene(w: usize, h: usize, seed: u64) -> Vec<f32> {
    let mut rng = Rng(seed);
    let mut img: Vec<f32> = (0..w * h)
        .map(|i| 0.25 + 0.2 * ((i % w) as f32 / w as f32))
        .collect();
    for _ in 0..(w * h / 900).max(40) {
        let cx = (rng.next() % w as u64) as f64;
        let cy = (rng.next() % h as u64) as f64;
        let r = 3.0 + (rng.next() % 9) as f64;
        let v = (rng.next() % 1000) as f32 / 1000.0;
        let sq = rng.next().is_multiple_of(2);
        let (x0, x1) = ((cx - r).max(0.0) as usize, ((cx + r) as usize + 1).min(w));
        let (y0, y1) = ((cy - r).max(0.0) as usize, ((cy + r) as usize + 1).min(h));
        for y in y0..y1 {
            for x in x0..x1 {
                let (dx, dy) = (x as f64 - cx, y as f64 - cy);
                if if sq {
                    dx.abs() < r && dy.abs() < r * 0.6
                } else {
                    dx.hypot(dy) < r
                } {
                    img[y * w + x] = v;
                }
            }
        }
    }
    img
}

fn sample(img: &[f32], w: usize, h: usize, x: f64, y: f64) -> Option<f32> {
    if x < 0.0 || y < 0.0 || x > (w - 1) as f64 || y > (h - 1) as f64 {
        return None;
    }
    Some(crate::photo_util::bilinear(
        img, w, h, 1, 0, x as f32, y as f32,
    ))
}

/// Crops `tw × th` views of a scene through `map` (view px → scene px).
pub(super) fn view(
    scene: &[f32],
    sw: usize,
    sh: usize,
    tw: usize,
    th: usize,
    map: impl Fn(f64, f64) -> (f64, f64),
) -> Vec<f32> {
    (0..tw * th)
        .map(|i| {
            let (x, y) = map((i % tw) as f64, (i / tw) as f64);
            sample(scene, sw, sh, x, y).unwrap_or(0.0)
        })
        .collect()
}
