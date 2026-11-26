//! 3D colour-lookup (LUT) interpolation.

use super::*;

fn identity(n: usize) -> Vec<f32> {
    let m = (n - 1) as f32;
    (0..n * n * n)
        .flat_map(|i| {
            [
                (i % n) as f32 / m,
                ((i / n) % n) as f32 / m,
                (i / (n * n)) as f32 / m,
            ]
        })
        .collect()
}

#[test]
fn identity_lut_is_identity_both_interpolations() {
    let t = identity(5);
    for c in [[0.1, 0.5, 0.9], [0.33, 0.77, 0.0], [1.0, 1.0, 1.0]] {
        for tet in [false, true] {
            let o = lut3d_sample(&t, 5, c, tet);
            for k in 0..3 {
                assert!((o[k] - c[k]).abs() < 1e-5, "{c:?} {tet} {o:?}");
            }
        }
    }
}

/// Photoshop 27.11, measured: absolute Selective Color on greys (red channel after one range's
/// cyan change).
#[test]
fn absolute_selective_color_matches_photoshop() {
    // (range index, cyan %, grey, Photoshop's red)
    let measured = [
        (7, 50.0, 64u8, 32u8),
        (7, 50.0, 192, 129),
        (7, -50.0, 192, 223),
        (7, -50.0, 64, 128),
        (6, -50.0, 192, 224),
        (6, -50.0, 240, 253),
        (8, 50.0, 64, 32),
        (8, 50.0, 16, 2),
    ];
    for (range, cyan, grey, want) in measured {
        let mut adj = [[0.0f32; 4]; 9];
        adj[range][0] = cyan;
        let g = grey as f32 / 255.0;
        let got = selective_color([g; 3], false, &adj)[0] * 255.0;
        assert!(
            (got - want as f32).abs() <= 1.5,
            "range {range} cyan {cyan} grey {grey}: {got} vs {want}"
        );
    }
}

#[test]
fn selective_color_zero_is_identity_and_ranges_are_local() {
    let zero = [[0.0f32; 4]; 9];
    for c in [[0.9, 0.1, 0.1], [0.5, 0.5, 0.5], [0.2, 0.4, 0.8]] {
        assert_eq!(selective_color(c, true, &zero), c);
    }
    // +100 % cyan on reds only: red loses red, blue is untouched.
    let mut a = zero;
    a[0][0] = 100.0;
    let red = selective_color([0.9, 0.1, 0.1], false, &a);
    assert!(red[0] < 0.2, "{red:?}");
    assert_eq!(selective_color([0.1, 0.1, 0.9], false, &a), [0.1, 0.1, 0.9]);
    // Relative mode can't tint pure white; absolute can (via whites).
    let mut w = zero;
    w[6] = [0.0, 0.0, 50.0, 0.0];
    assert_eq!(selective_color([1.0; 3], true, &w), [1.0; 3]);
    assert!(selective_color([1.0; 3], false, &w)[2] < 0.6);
}

#[test]
fn dither_offsets_are_balanced() {
    let sum: f32 = (0..4).flat_map(|y| (0..4).map(move |x| bayer4(x, y))).sum();
    assert!(sum.abs() < 1e-5);
    assert_eq!(bayer4(-4, -4), bayer4(0, 0));
}
