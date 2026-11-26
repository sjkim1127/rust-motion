//! Lab colour range against Photoshop's measured coverage.

use super::*;

/// Photoshop 2024's own Color Range coverage around the sample Lab (60, 25, 19.9961), read
/// from its selections on a 16-bit Lab document: (Lab in Photoshop's internal scale, where
/// a/b = (v16 − 32768)/256, fuzziness, coverage).
const PHOTOSHOP_COVERAGE: &[([f32; 3], f32, f32)] = &[
    ([61.9989, 25.5977, 20.5742], 40.0, 0.9686),
    ([64.9989, 25.5977, 20.5742], 40.0, 0.7961),
    ([67.9988, 25.5977, 20.5742], 40.0, 0.4824),
    ([71.9997, 25.5977, 20.5742], 40.0, 0.1098),
    ([79.9985, 25.5977, 20.5742], 40.0, 0.0),
    ([71.9997, 25.5977, 20.5742], 100.0, 0.8118),
    ([79.9985, 25.5977, 20.5742], 100.0, 0.4824),
    ([60.0, 13.5508, 20.5742], 40.0, 0.0432),
    ([60.0, 19.5742, 20.5742], 40.0, 0.6706),
    ([60.0, 22.582, 20.5742], 40.0, 0.9333),
    ([60.0, 28.6055, 20.5742], 40.0, 0.8549),
    ([60.0, 31.6211, 20.5742], 40.0, 0.5098),
    ([60.0, 37.6445, 20.5742], 40.0, 0.0078),
    ([60.0, 13.5508, 20.5742], 100.0, 0.7647),
    ([60.0, 37.6445, 20.5742], 100.0, 0.7137),
    ([60.0, 25.5977, 10.5352], 40.0, 0.1725),
    ([60.0, 25.5977, 16.5586], 40.0, 0.8667),
    ([60.0, 25.5977, 24.5898], 40.0, 0.7647),
    ([60.0, 25.5977, 30.6133], 40.0, 0.0863),
    ([60.0, 25.5977, 10.5352], 100.0, 0.8392),
    ([60.0, 25.5977, 30.6133], 100.0, 0.8),
];

#[test]
fn lab_range_matches_photoshop() {
    let r = LabRange::point([60.0, 25.0, 19.9961]);
    for &(lab, fz, ps) in PHOTOSHOP_COVERAGE {
        let got = lab_range_coverage(lab, &r, fz);
        assert!(
            (got - ps).abs() <= 2.0 / 255.0,
            "{lab:?} at fuzziness {fz}: {got} vs Photoshop {ps}"
        );
    }
    // Lightness is three times as forgiving as hue: the same 6-unit step along L and along a.
    assert!(
        lab_range_coverage([66.0, 25.0, 20.0], &r, 40.0)
            > lab_range_coverage([60.0, 31.0, 20.0], &r, 40.0)
    );
    // The sampled colour itself is always fully selected.
    for fz in [1.0, 10.0, 40.0] {
        assert_eq!(lab_range_coverage([60.0, 25.0, 19.9961], &r, fz), 1.0);
    }
    assert_eq!(
        quantize_lab8([60.1, 25.4, -19.6]),
        [(153.0f32 / 2.55), 25.0, -20.0]
    );
}

#[test]
fn lab_range_box_and_edges() {
    let r = LabRange::spanning(&[[50.0, 10.0, 0.0], [60.0, 20.0, 5.0]]).unwrap();
    assert_eq!(
        r,
        LabRange {
            lo: [50.0, 10.0, 0.0],
            hi: [60.0, 20.0, 5.0]
        }
    );
    assert!(LabRange::spanning(&[]).is_none());
    // Everything inside the box is fully selected, whatever the fuzziness.
    for fz in [1.0, 40.0, 200.0] {
        assert_eq!(lab_range_coverage([55.0, 15.0, 2.0], &r, fz), 1.0);
    }
    // Coverage falls off outside it and is gone a fuzziness away.
    let near = lab_range_coverage([55.0, 22.0, 2.0], &r, 40.0);
    assert!(near > 0.0 && near < 1.0, "{near}");
    assert_eq!(lab_range_coverage([55.0, 40.0, 2.0], &r, 40.0), 0.0);
    // Fuzziness 0: exact colours only.
    let p = LabRange::point([50.0, 10.0, -5.0]);
    assert_eq!(lab_range_coverage([50.0, 10.0, -5.0], &p, 0.0), 1.0);
    assert_eq!(lab_range_coverage([50.0, 11.0, -5.0], &p, 0.0), 0.0);
    // Hostile numbers never panic and never select.
    for v in [f32::NAN, f32::INFINITY, -f32::INFINITY] {
        for fz in [v, 40.0] {
            let k = lab_range_coverage([v, 0.0, 0.0], &p, fz);
            assert!((0.0..=1.0).contains(&k), "{k}");
        }
    }
}

/// Boxes Photoshop 2024's Color Range dialog returned after Subtract from Sample, at
/// fuzziness 40 unless noted: (lo, hi, subtracted value, fuzziness, new lo, new hi) on one axis.
const PHOTOSHOP_SUBTRACT_A: &[(f32, f32, f32, f32, f32, f32)] = &[
    (0.0, 20.0, 5.0, 40.0, 5.0, 20.0),
    (0.0, 20.0, 10.0, 40.0, 0.0, 10.0),
    (0.0, 20.0, 15.0, 40.0, 0.0, 15.0),
    (0.0, 20.0, 0.0, 40.0, 2.68, 20.0),
    (0.0, 20.0, -2.0, 40.0, 0.68, 20.0),
    (0.0, 20.0, -3.0, 40.0, 2.36, 20.0),
    (0.0, 20.0, -8.0, 40.0, 0.04, 20.0),
    (0.0, 20.0, -11.0, 40.0, 2.4, 20.0),
    (0.0, 20.0, -13.0, 40.0, 0.4, 20.0),
    (0.0, 20.0, -14.0, 40.0, 0.0, 20.0),
    (0.0, 20.0, 20.0, 40.0, 0.0, 17.32),
    (0.0, 20.0, 23.0, 40.0, 0.0, 17.64),
    (0.0, 20.0, 33.0, 40.0, 0.0, 19.6),
    (0.0, 20.0, 0.0, 80.0, 5.35, 20.0),
    (0.0, 20.0, -12.0, 80.0, 4.05, 20.0),
    (0.0, 20.0, -22.0, 80.0, 4.76, 20.0),
    (0.0, 20.0, -30.0, 80.0, 0.0, 20.0),
    (0.0, 6.0, 4.0, 40.0, 0.0, 4.0),
    (0.0, 2.0, 0.0, 40.0, 2.0, 2.0),
    (0.0, 2.0, 3.0, 40.0, 0.0, 0.32),
];

#[test]
fn subtract_reshapes_the_box_like_photoshop() {
    for &(lo, hi, m, fz, want_lo, want_hi) in PHOTOSHOP_SUBTRACT_A {
        let r = LabRange {
            lo: [50.0, lo, 0.0],
            hi: [50.0, hi, 0.0],
        }
        .subtract(&[50.0, m, 0.0], fz);
        assert!(
            (r.lo[1] - want_lo).abs() < 0.03 && (r.hi[1] - want_hi).abs() < 0.03,
            "a [{lo}, {hi}] − {m} at {fz}: [{}, {}] vs Photoshop [{want_lo}, {want_hi}]",
            r.lo[1],
            r.hi[1]
        );
    }
    // Lightness (box 40–60, fuzziness 40): Photoshop gave 43.14, 41.17, 40.79 and 45.1.
    for (m, want) in [
        (40.0, 43.14),
        (34.902, 41.17),
        (25.098, 40.79),
        (45.098, 45.1),
    ] {
        let r = LabRange {
            lo: [40.0, 0.0, 0.0],
            hi: [60.0, 0.0, 0.0],
        }
        .subtract(&[m, 0.0, 0.0], 40.0);
        assert!(
            (r.lo[0] - want).abs() < 0.05,
            "L − {m}: {} vs Photoshop {want}",
            r.lo[0]
        );
    }
    // All three axes at once (Photoshop: two samples, then one colour outside the box).
    let r = LabRange {
        lo: [53.58, 0.0, 0.0],
        hi: [59.05, 30.56, 25.03],
    }
    .subtract(&[53.33, -28.71, 36.45], 40.0);
    let want = LabRange {
        lo: [56.47, 0.0, 0.0],
        hi: [59.05, 30.56, 23.05],
    };
    for k in 0..3 {
        assert!(
            (r.lo[k] - want.lo[k]).abs() < 0.05 && (r.hi[k] - want.hi[k]).abs() < 0.05,
            "{r:?} vs {want:?}"
        );
    }
    // A single sample can't be subtracted from itself; hostile numbers change nothing.
    let p = LabRange::point([50.0, 10.0, 10.0]);
    assert_eq!(p.subtract(&[50.0, 10.0, 10.0], 40.0), p);
    assert_eq!(
        p.subtract(&[f32::NAN, f32::INFINITY, -f32::INFINITY], f32::NAN),
        p
    );
    assert_eq!(
        LabRange::spanning(&[[50.0, 0.0, 0.0]])
            .unwrap()
            .grow(&[60.0, -5.0, 3.0]),
        LabRange {
            lo: [50.0, -5.0, 0.0],
            hi: [60.0, 0.0, 3.0]
        }
    );
}

#[test]
fn eight_bit_grid_steps_are_uneven_like_photoshop() {
    let r = LabRange::point([50.0, 10.0, -20.0]);
    // The sampled colour itself is fully selected at any fuzziness.
    for fz in [1.0, 5.0, 10.0, 40.0] {
        assert_eq!(lab_range_coverage_8bit([50.0, 10.0, -20.0], &r, fz), 1.0);
    }
    // One step above on a counts as 1.5 units, one below as 0.5 (×3 weight): at fuzziness 40
    // (width 40.16) that is t = 4.5/40.16 and 1.5/40.16.
    let up = lab_range_coverage_8bit([50.0, 11.0, -20.0], &r, 40.0);
    let down = lab_range_coverage_8bit([50.0, 9.0, -20.0], &r, 40.0);
    let p = |d: f32| {
        (((1.0 - 2.0 * (d / (40.0 * 256.0 / 255.0)).powi(2)) * 256.0)
            .floor()
            .min(255.0))
            / 255.0
    };
    assert_eq!((up, down), (p(4.5), p(1.5)));
    assert!(down > up);
    // Lightness has no such offset; the exact (non-8-bit) coverage is symmetric.
    assert_eq!(
        lab_range_coverage_8bit([51.0, 10.0, -20.0], &r, 40.0),
        lab_range_coverage_8bit([49.0, 10.0, -20.0], &r, 40.0)
    );
    assert_eq!(
        lab_range_coverage([50.0, 11.0, -20.0], &r, 40.0),
        lab_range_coverage([50.0, 9.0, -20.0], &r, 40.0)
    );
    let px = [[50.0, 11.0, -20.0, 1.0], [50.0, 9.0, -20.0, 1.0]];
    assert_eq!(
        color_range_lab(&px, 2, &r, 40.0, true, None),
        vec![up, down]
    );
}

#[test]
fn color_range_lab_alpha_and_localized() {
    let r = LabRange::point([50.0, 0.0, 0.0]);
    let px = img(10, 1, |x, _| {
        [50.0, 0.0, 0.0, if x == 0 { 0.0 } else { 1.0 }]
    });
    let m = color_range_lab(&px, 10, &r, 40.0, false, None);
    assert_eq!(m[0], 0.0, "transparent pixels are never selected");
    assert!(m[1..].iter().all(|v| *v > 0.9));
    let at = [(1.0, 0.0)];
    let l = color_range_lab(&px, 10, &r, 40.0, false, Some((4.0, &at)));
    assert!(l[1] > l[3] && l[3] > 0.0 && l[6] == 0.0, "{l:?}");
    // No positions: localized clusters have nothing to centre on and change nothing.
    assert_eq!(
        color_range_lab(&px, 10, &r, 40.0, false, Some((4.0, &[]))),
        m
    );
    assert!(color_range_lab(&[], 0, &r, 40.0, false, None).is_empty());
}
