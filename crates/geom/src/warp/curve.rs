//! Cubic Bezier helpers: the Bernstein basis, de Casteljau splitting and the interpolation matrix.

pub(super) fn bernstein(t: f64) -> [f64; 4] {
    let s = 1.0 - t;
    [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t]
}

fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// De Casteljau split of a cubic at `t`: 7 points (left 0..=3, right 3..=6).
pub(super) fn split_cubic(p: [[f64; 2]; 4], t: f64) -> [[f64; 2]; 7] {
    let a = lerp(p[0], p[1], t);
    let b = lerp(p[1], p[2], t);
    let c = lerp(p[2], p[3], t);
    let d = lerp(a, b, t);
    let e = lerp(b, c, t);
    let f = lerp(d, e, t);
    [p[0], a, d, f, e, c, p[3]]
}

/// Inverse of the Bernstein interpolation matrix at t = 0, 1/3, 2/3, 1 (rows: control point,
/// columns: sample). Exact rational values.
pub(super) const BERN_INV: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [-5.0 / 6.0, 3.0, -1.5, 1.0 / 3.0],
    [1.0 / 3.0, -1.5, 3.0, -5.0 / 6.0],
    [0.0, 0.0, 0.0, 1.0],
];
