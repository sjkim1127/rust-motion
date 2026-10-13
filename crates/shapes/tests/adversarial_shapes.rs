//! Adversarial and degenerate stress tests for `rust-motion-shapes`.

use rust_motion_shapes::*;
use std::f64::consts::PI;

// ===========================================================================
// 1. make_heart Edge & Stress Cases
// ===========================================================================

#[test]
fn test_make_heart_adversarial() {
    // 0 dimensions
    let h0 = make_heart(0.0, 0.0);
    assert_eq!(h0.path, "");
    assert_eq!(h0.width, 0.0);
    assert_eq!(h0.height, 0.0);
    assert_eq!(h0.transform_origin, "0 0");

    // Negative dimensions clamped to 0.0
    let h_neg = make_heart(-100.0, -50.0);
    assert_eq!(h_neg.path, "");
    assert_eq!(h_neg.width, 0.0);
    assert_eq!(h_neg.height, 0.0);

    // One zero dimension
    let h_w0 = make_heart(0.0, 100.0);
    assert_eq!(h_w0.path, "");
    assert_eq!(h_w0.width, 0.0);
    assert_eq!(h_w0.height, 100.0);

    let h_h0 = make_heart(100.0, 0.0);
    assert_eq!(h_h0.path, "");
    assert_eq!(h_h0.width, 100.0);
    assert_eq!(h_h0.height, 0.0);

    // Huge dimensions
    let h_huge = make_heart(1e8, 1e8);
    assert_eq!(h_huge.width, 1e8);
    assert_eq!(h_huge.height, 1e8);
    assert!(h_huge.path.starts_with("M 50000000 100000000"));
    assert!(h_huge.path.ends_with("Z"));

    // Very small positive dimensions
    let h_tiny = make_heart(0.001, 0.001);
    assert!(h_tiny.path.starts_with("M 0.0005 0.001"));
    assert!(h_tiny.path.ends_with("Z"));
}

// ===========================================================================
// 2. make_callout Edge & Stress Cases
// ===========================================================================

#[test]
fn test_make_callout_adversarial() {
    let directions = [
        CalloutDirection::Down,
        CalloutDirection::Up,
        CalloutDirection::Left,
        CalloutDirection::Right,
    ];

    for &dir in &directions {
        // Zero dimensions
        let c0 = make_callout(0.0, 0.0, 30.0, dir);
        assert_eq!(c0.path, "", "Empty path expected for 0x0 callout: {dir:?}");
        match dir {
            CalloutDirection::Down | CalloutDirection::Up => {
                assert_eq!(c0.width, 0.0);
                assert_eq!(c0.height, 30.0);
            }
            CalloutDirection::Left | CalloutDirection::Right => {
                assert_eq!(c0.width, 30.0);
                assert_eq!(c0.height, 0.0);
            }
        }

        // Negative dimensions clamped to 0.0
        let c_neg = make_callout(-50.0, -50.0, -10.0, dir);
        assert_eq!(c_neg.path, "");
        assert_eq!(c_neg.width, 0.0);
        assert_eq!(c_neg.height, 0.0);

        // Zero pointer length -> valid callout box without pointer extension
        let c_no_ptr = make_callout(100.0, 60.0, 0.0, dir);
        assert_eq!(c_no_ptr.width, 100.0);
        assert_eq!(c_no_ptr.height, 60.0);
        assert!(!c_no_ptr.path.is_empty());
        assert!(c_no_ptr.path.ends_with("Z"));

        // Huge pointer length
        let c_huge_ptr = make_callout(100.0, 60.0, 1000.0, dir);
        match dir {
            CalloutDirection::Down | CalloutDirection::Up => {
                assert_eq!(c_huge_ptr.width, 100.0);
                assert_eq!(c_huge_ptr.height, 1060.0);
            }
            CalloutDirection::Left | CalloutDirection::Right => {
                assert_eq!(c_huge_ptr.width, 1100.0);
                assert_eq!(c_huge_ptr.height, 60.0);
            }
        }

        // Extremely small non-zero callout box (e.g. 2x2 px)
        let c_tiny = make_callout(2.0, 2.0, 1.0, dir);
        assert!(!c_tiny.path.is_empty());
        assert!(c_tiny.path.ends_with("Z"));
    }
}

// ===========================================================================
// 3. make_spark Edge & Stress Cases
// ===========================================================================

#[test]
fn test_make_spark_adversarial() {
    // 0 dimensions
    let s0 = make_spark(0.0, 0.0, 0.5, 0.0);
    assert_eq!(s0.path, "");
    assert_eq!(s0.width, 0.0);
    assert_eq!(s0.height, 0.0);

    // Negative dimensions clamped to 0.0
    let s_neg = make_spark(-100.0, -100.0, 0.5, 0.0);
    assert_eq!(s_neg.path, "");
    assert_eq!(s_neg.width, 0.0);

    // Edge roundness extreme values: < 0 (clamped to 0.0) and > 1 (clamped to 1.0)
    let s_round_neg = make_spark(100.0, 100.0, -2.0, 0.0);
    let s_round_zero = make_spark(100.0, 100.0, 0.0, 0.0);
    assert_eq!(
        s_round_neg.path, s_round_zero.path,
        "Negative roundness should clamp to 0.0"
    );

    let s_round_huge = make_spark(100.0, 100.0, 10.0, 0.0);
    let s_round_one = make_spark(100.0, 100.0, 1.0, 0.0);
    assert_eq!(
        s_round_huge.path, s_round_one.path,
        "Roundness > 1.0 should clamp to 1.0"
    );

    // Corner radius extreme values: negative (clamped to 0.0), huge (clamped to (hx/2).min(hy/2))
    let s_cr_neg = make_spark(100.0, 100.0, 0.5, -5.0);
    let s_cr_zero = make_spark(100.0, 100.0, 0.5, 0.0);
    assert_eq!(
        s_cr_neg.path, s_cr_zero.path,
        "Negative corner radius should clamp to 0.0"
    );

    // Huge corner radius clamped to 25.0 (for 100x100 spark: hx=50, max r = 25)
    let s_cr_huge = make_spark(100.0, 100.0, 0.5, 1000.0);
    let s_cr_max = make_spark(100.0, 100.0, 0.5, 25.0);
    assert_eq!(
        s_cr_huge.path, s_cr_max.path,
        "Corner radius should clamp to max allowed cap radius"
    );

    // Huge spark
    let s_huge = make_spark(1e6, 1e6, 0.5, 50.0);
    assert_eq!(s_huge.width, 1e6);
    assert_eq!(s_huge.height, 1e6);
    assert!(s_huge.path.ends_with("Z"));
}

// ===========================================================================
// 4. make_pie Edge & Stress Cases
// ===========================================================================

#[test]
fn test_make_pie_adversarial() {
    // 0 radius
    let p0 = make_pie(0.0, 0.5, true, false, 0.0);
    assert_eq!(p0.path, "");
    assert_eq!(p0.width, 0.0);
    assert_eq!(p0.height, 0.0);

    // Negative radius clamped to 0.0
    let p_neg_r = make_pie(-50.0, 0.5, true, false, 0.0);
    assert_eq!(p_neg_r.path, "");
    assert_eq!(p_neg_r.width, 0.0);

    // 0 progress or negative progress -> empty path
    let p_zero_p = make_pie(100.0, 0.0, true, false, 0.0);
    assert_eq!(p_zero_p.path, "");

    let p_neg_p = make_pie(100.0, -0.5, true, false, 0.0);
    assert_eq!(p_neg_p.path, "");

    // Progress > 1.0 clamped to 1.0
    let p_over = make_pie(100.0, 2.5, true, false, 0.0);
    let p_full = make_pie(100.0, 1.0, true, false, 0.0);
    assert_eq!(p_over.path, p_full.path);

    // Arc splitting boundary at progress = 0.5 vs 0.5001
    let p_half = make_pie(100.0, 0.5, true, false, 0.0);
    // Count arc commands 'A'
    let arc_count_half = p_half.path.matches("A ").count();
    assert_eq!(arc_count_half, 1, "Half pie should use single arc");

    let p_half_plus = make_pie(100.0, 0.51, true, false, 0.0);
    let arc_count_split = p_half_plus.path.matches("A ").count();
    assert_eq!(
        arc_count_split, 2,
        "Progress > 0.5 should split into two arcs"
    );

    // Counter-clockwise full circle and half circle
    let p_ccw = make_pie(100.0, 0.25, true, true, 0.0);
    assert!(
        p_ccw.path.contains("0 0 0"),
        "Counter-clockwise arc should have sweep_flag=0"
    );

    // Extreme rotations: 2*PI, 100*PI, -100*PI
    let p_rot_full = make_pie(100.0, 0.25, true, false, 2.0 * PI);
    let p_rot_0 = make_pie(100.0, 0.25, true, false, 0.0);
    assert_eq!(
        p_rot_full.path, p_rot_0.path,
        "2*PI rotation should be visually identical"
    );

    let p_rot_huge = make_pie(100.0, 0.5, true, false, 100.0 * PI);
    assert!(!p_rot_huge.path.is_empty());
}

// ===========================================================================
// 5. General Shape Robustness on All Shapes
// ===========================================================================

#[test]
fn test_all_shapes_boundary_values() {
    // Circle
    let (c0_p, _, _) = make_circle(0.0);
    assert!(c0_p.contains("M 0 0"));
    let (c_neg_p, _, _) = make_circle(-50.0);
    assert!(c_neg_p.contains("M 0 0"));
    let (c_p, _, _) = make_circle(100.0);
    assert!(c_p.ends_with("Z"));

    // Rect
    let (r0_p, _, _) = make_rect(0.0, 10.0, 0.0);
    assert!(r0_p.contains("M 0 0"));
    let (r_neg_p, _, _) = make_rect(-10.0, -10.0, -5.0);
    assert!(r_neg_p.contains("M 0 0"));
    // Corner radius clamped to min(w/2, h/2)
    let (rect_huge_r, _, _) = make_rect(100.0, 60.0, 1000.0);
    let (rect_max_r, _, _) = make_rect(100.0, 60.0, 30.0);
    assert_eq!(rect_huge_r, rect_max_r);

    // Triangle
    let (t0_p, _, _) = make_triangle(0.0);
    assert!(t0_p.contains("M 0.0000 0"));
    let (t_neg_p, _, _) = make_triangle(-10.0);
    assert!(t_neg_p.contains("M 0.0000 0"));

    // Star
    let (s0_p, _, _) = make_star(0, 50.0, 25.0);
    assert!(s0_p.ends_with("Z"));
    let (s100_p, _, _) = make_star(100, 50.0, 25.0);
    assert!(s100_p.ends_with("Z"));

    // Polygon
    let (p0_p, _, _) = make_polygon(0, 50.0);
    assert!(p0_p.ends_with("Z"));
    let (p100_p, _, _) = make_polygon(100, 50.0);
    assert!(p100_p.ends_with("Z"));

    // Arrow
    let (a0_p, w0, h0) = make_arrow(0.0, 0.0);
    assert_eq!(w0, 20.0); // Clamped minimum length
    assert_eq!(h0, 10.0); // Clamped minimum thickness * 2.5
    assert!(a0_p.ends_with("Z"));
}

// ===========================================================================
// 6. Cross-Crate Parametric Fuzzing (Paths + Shapes)
// ===========================================================================

#[test]
fn test_cross_crate_parametric_fuzzing() {
    use rust_motion_paths::{approximate_path_length, evolve_path, get_length, parse_path};

    // Heart sweep
    let test_dims = [-50.0, 0.0, 0.001, 1.0, 10.0, 50.0, 100.0, 1000.0];
    for &w in &test_dims {
        for &h in &test_dims {
            let heart = make_heart(w, h);
            if w <= 0.0 || h <= 0.0 {
                assert_eq!(heart.path, "");
            } else {
                let insts = parse_path(&heart.path).expect("Heart path must parse");
                assert!(!insts.is_empty());
                let len = get_length(&heart.path);
                assert!(len.is_finite() && len > 0.0);
                let approx = approximate_path_length(&heart.path);
                assert!(approx.is_finite() && approx > 0.0);
                let ev = evolve_path(0.5, &heart.path);
                assert!(ev.stroke_dashoffset.is_finite() && ev.stroke_dashoffset > 0.0);
            }
        }
    }

    // Callout sweep
    let directions = [
        CalloutDirection::Down,
        CalloutDirection::Up,
        CalloutDirection::Left,
        CalloutDirection::Right,
    ];
    let ptr_lens = [-10.0, 0.0, 5.0, 30.0, 100.0];
    for &dir in &directions {
        for &w in &test_dims {
            for &h in &test_dims {
                for &ptr in &ptr_lens {
                    let callout = make_callout(w, h, ptr, dir);
                    if w <= 0.0 || h <= 0.0 {
                        assert_eq!(callout.path, "");
                    } else {
                        let insts = parse_path(&callout.path).expect("Callout path must parse");
                        assert!(!insts.is_empty());
                        let len = get_length(&callout.path);
                        assert!(len.is_finite() && len > 0.0);
                    }
                }
            }
        }
    }

    // Spark sweep
    let roundnesses = [-1.0, 0.0, 0.25, 0.5, 0.75, 1.0, 2.0];
    let corner_radii = [-5.0, 0.0, 2.0, 10.0, 50.0, 200.0];
    for &w in &test_dims {
        for &h in &test_dims {
            for &roundness in &roundnesses {
                for &cr in &corner_radii {
                    let spark = make_spark(w, h, roundness, cr);
                    if w <= 0.0 || h <= 0.0 {
                        assert_eq!(spark.path, "");
                    } else {
                        let insts = parse_path(&spark.path).expect("Spark path must parse");
                        assert!(!insts.is_empty());
                        let len = get_length(&spark.path);
                        assert!(len.is_finite() && len > 0.0);
                    }
                }
            }
        }
    }

    // Pie sweep
    let radii = [-10.0, 0.0, 1.0, 50.0, 100.0, 500.0];
    let progresses = [-0.5, 0.0, 0.01, 0.25, 0.5, 0.51, 0.75, 1.0, 1.5];
    let rotations = [-PI, 0.0, PI / 4.0, PI / 2.0, PI, 2.0 * PI, 5.0 * PI];
    for &r in &radii {
        for &p in &progresses {
            for &close in &[true, false] {
                for &ccw in &[true, false] {
                    for &rot in &rotations {
                        let pie = make_pie(r, p, close, ccw, rot);
                        if r <= 0.0 || p <= 0.0 {
                            assert_eq!(pie.path, "");
                        } else {
                            let insts = parse_path(&pie.path).expect("Pie path must parse");
                            assert!(!insts.is_empty());
                            let len = get_length(&pie.path);
                            assert!(len.is_finite() && len > 0.0);
                            let ev = evolve_path(0.5, &pie.path);
                            assert!(ev.stroke_dashoffset.is_finite());
                        }
                    }
                }
            }
        }
    }
}
