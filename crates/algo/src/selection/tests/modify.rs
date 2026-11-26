//! Expand, contract, border, smooth and feather tests.

use super::*;

#[test]
fn expand_contract_border() {
    let m = square(20, 20);
    let e = expand(&m, 20, 20, 2.0);
    assert_eq!(e[10 * 20 + 3], 1.0);
    assert_eq!(e[10 * 20 + 2], 0.0);
    let c = contract(&m, 20, 20, 2.0);
    assert_eq!(c[10 * 20 + 6], 0.0);
    assert_eq!(c[10 * 20 + 7], 1.0);
    let b = border(&m, 20, 20, 4.0);
    assert_eq!(b[10 * 20 + 10], 0.0);
    assert!(b[10 * 20 + 5] > 0.5 && b[10 * 20 + 4] > 0.5);
}

/// Photoshop 27.11, measured: a 32×32 rectangle at (16,16) in 64×64, Select › Modify ›
/// Border 4 px, filled white. Row y = 32 for x = 8..=24, and the diagonal (16,16)..(24,24).
#[test]
fn border_matches_photoshop() {
    let (w, h) = (64, 64);
    let m: Vec<f32> = (0..w * h)
        .map(|i| {
            if (16..48).contains(&(i % w)) && (16..48).contains(&(i / w)) {
                1.0
            } else {
                0.0
            }
        })
        .collect();
    let b = border(&m, w, h, 4.0);
    let v = |x: usize, y: usize| (b[y * w + x] * 255.0).round() as i32;
    let row: Vec<i32> = (8..=24).map(|x| v(x, 32)).collect();
    let diag: Vec<i32> = (16..=24).map(|d| v(d, d)).collect();
    let near = |got: &[i32], want: &[i32]| got.iter().zip(want).all(|(g, w)| (g - w).abs() <= 1);
    let want_row = [
        0, 0, 0, 0, 63, 127, 191, 255, 255, 191, 127, 63, 0, 0, 0, 0, 0,
    ];
    assert!(near(&row, &want_row), "{row:?}");
    assert!(near(&diag, &[255, 191, 127, 63, 0, 0, 0, 0, 0]), "{diag:?}");
}

#[test]
fn smooth_removes_specks_and_feather_softens() {
    let mut m = square(20, 20);
    m[2 * 20 + 2] = 1.0;
    let s = smooth(&m, 20, 20, 1.0);
    assert_eq!(s[2 * 20 + 2], 0.0);
    assert_eq!(s[10 * 20 + 10], 1.0);
    // Radius 2 (σ 2) keeps the blur inside the 20×20 canvas, so no coverage is lost.
    let f = feather(&square(20, 20), 20, 20, 2.0);
    let edge = f[10 * 20 + 5];
    assert!(edge > 0.3 && edge < 0.8, "{edge}");
    let sum: f32 = f.iter().sum();
    assert!((sum - 100.0).abs() < 1.0);
}
