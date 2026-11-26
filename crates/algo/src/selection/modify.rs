//! Select › Modify: expand, contract, border, smooth and feather.

use super::*;

/// Grows the selection by `r` pixels.
pub fn expand(m: &[f32], w: usize, h: usize, r: f32) -> Vec<f32> {
    let inside: Vec<bool> = m.iter().map(|v| *v >= 0.5).collect();
    let d = edt(&inside, w, h);
    // A pixel whose centre is `d` away from the nearest selected centre is
    // covered when d ≤ r (fractional radii give partial coverage).
    m.iter()
        .zip(d)
        .map(|(v, d)| v.max((r + 1.0 - d).clamp(0.0, 1.0)))
        .collect()
}

/// Shrinks the selection by `r` pixels.
pub fn contract(m: &[f32], w: usize, h: usize, r: f32) -> Vec<f32> {
    let inv: Vec<f32> = m.iter().map(|v| 1.0 - v).collect();
    expand(&inv, w, h, r).into_iter().map(|v| 1.0 - v).collect()
}

/// Select › Modify › Border: a soft band `2r` wide centred on the selection edge, as in
/// Photoshop. A pixel `D` away (centre to centre) from the nearest pixel on the other side of the
/// edge is selected `1 − (D − 1) / r`: fully at the edge, fading to nothing `r` pixels out on
/// either side (measured on Photoshop 27.11, Border 4: 63, 127, 191, 255 | 255, 191, 127, 63
/// across a straight edge, the same along the inside of a corner). It used to be a hard band
/// `r` wide.
pub fn border(m: &[f32], w: usize, h: usize, r: f32) -> Vec<f32> {
    let inside: Vec<bool> = m.iter().map(|v| *v >= 0.5).collect();
    let outside: Vec<bool> = inside.iter().map(|v| !v).collect();
    let (to_inside, to_outside) = (edt(&inside, w, h), edt(&outside, w, h));
    if r <= 0.0 || to_inside.len() != m.len() || to_outside.len() != m.len() {
        return vec![0.0; m.len()];
    }
    inside
        .iter()
        .zip(to_inside.iter().zip(&to_outside))
        .map(|(&sel, (&di, &dout))| {
            (1.0 - ((if sel { dout } else { di }) - 1.0) / r).clamp(0.0, 1.0)
        })
        .collect()
}

/// Smooth: removes specks and rounds corners (box average of radius `round(r)`, edges
/// replicated, then re-thresholded). Parallel and independent of the radius (#211).
pub fn smooth(m: &[f32], w: usize, h: usize, r: f32) -> Vec<f32> {
    crate::selection_blur::smooth(m, w, h, r)
}

/// Feather: Gaussian blur of the mask with sigma = radius, as in Photoshop (kernel truncated at 3σ, zero
/// beyond the canvas). Parallel, confined to the selection's bounds, and independent of the
/// radius (#211).
pub fn feather(m: &[f32], w: usize, h: usize, radius: f32) -> Vec<f32> {
    crate::selection_blur::feather(m, w, h, radius)
}
