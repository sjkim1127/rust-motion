//! Tests for selection algorithms.

use super::*;

mod color_range;
mod lab_range;
mod mask;
mod modify;
mod polygon;
mod wand;

pub(super) fn img(w: usize, h: usize, f: impl Fn(usize, usize) -> [f32; 4]) -> Vec<[f32; 4]> {
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| f(x, y))
        .collect()
}

pub(super) fn square(w: usize, h: usize) -> Vec<f32> {
    (0..w * h)
        .map(|i| {
            if (5..15).contains(&(i % w)) && (5..15).contains(&(i / w)) {
                1.0
            } else {
                0.0
            }
        })
        .collect()
}
