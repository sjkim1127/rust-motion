//! Tests for the proximity match and texture synthesis.

use super::*;

#[test]
fn proximity_finds_matching_offset() {
    let (w, h, ch) = (80, 60, 1);
    // Periodic pattern with period 10 in x: a match is any multiple-of-10 displacement.
    let img: Vec<f32> = (0..w * h)
        .map(|i| ((i % w) as f32 * std::f32::consts::TAU / 10.0).sin() * 0.4 + 0.5)
        .collect();
    let hole = disc(w, h, 40.0, 30.0, 5.0);
    let (dx, dy) = best_offset(w, h, ch, &img, &hole, 3, 20).unwrap();
    assert_eq!(dx.rem_euclid(10), 0, "dx {dx} dy {dy}");
    assert!((dx, dy) != (0, 0));
}

#[test]
fn proximity_search_clamps_hostile_radii_without_overflow_or_hanging() {
    let (w, h, ch) = (24, 20, 1);
    let img: Vec<f32> = (0..w * h).map(|i| (i % w) as f32 / w as f32).collect();
    let hole = disc(w, h, 12.0, 10.0, 2.0);
    // User-supplied values used to overflow i32 loops, cast a huge usize
    // ring into a negative number, or take billions of search iterations.
    let result = best_offset(w, h, ch, &img, &hole, usize::MAX, i32::MAX);
    if let Some((dx, dy)) = result {
        assert!(dx.abs() <= w as i32 && dy.abs() <= h as i32);
    }
    assert_eq!(best_offset(w, h, ch, &img, &hole, 2, -1), None);
    assert_eq!(best_offset(w, h, ch, &img, &hole, 2, 0), None);
}

#[test]
fn synthesis_fills_hole_with_plausible_values() {
    let (w, h, ch) = (64, 64, 3);
    let clean = texture(w, h);
    let hole = disc(w, h, 32.0, 32.0, 6.0);
    let out = synthesize(w, h, ch, &clean, &hole, 8, 3);
    let err = hole_rmse(&out, &clean, &hole, ch);
    assert!(err < 0.15, "{err}");
    assert!(out.iter().all(|v| v.is_finite()));
}
