//! Adversarial and degenerate stress tests for `rust-motion-paths`.

use rust_motion_paths::*;
use std::f64::consts::PI;

// ===========================================================================
// 1. Empty and Whitespace Inputs
// ===========================================================================

#[test]
fn test_empty_and_whitespace_paths() {
    let empty_inputs = ["", "   ", "\t\n\r", " , , ,  ", "\n\n\t"];

    for input in &empty_inputs {
        // get_length must not panic and must return 0.0
        assert_eq!(
            get_length(input),
            0.0,
            "get_length failed on empty input: {:?}",
            input
        );

        // approximate_path_length must not panic and must return 0.0
        assert_eq!(
            approximate_path_length(input),
            0.0,
            "approximate_path_length failed on empty input: {:?}",
            input
        );

        // parse_path must return Ok(empty) or handled Err, but never panic
        let parsed = parse_path(input);
        assert!(
            parsed.is_ok(),
            "parse_path should handle whitespace gracefully: {:?}",
            input
        );
        assert_eq!(parsed.unwrap().len(), 0);

        // evolve_path at various progress values
        for &p in &[-1.0, 0.0, 0.5, 1.0, 2.0] {
            let evolved = evolve_path(p, input);
            assert!(
                !evolved.stroke_dasharray.is_empty(),
                "evolved dasharray should not be empty for p={p}, input={input:?}"
            );
            assert!(
                !evolved.stroke_dashoffset.is_nan(),
                "evolved dashoffset should not be NaN for p={p}, input={input:?}"
            );
        }

        // get_point_at_length must return (0, 0)
        let pt = get_point_at_length(input, 50.0);
        assert_eq!(pt, Point { x: 0.0, y: 0.0 });

        // scale_path and translate_path must return empty or valid string without panic
        assert_eq!(scale_path(input, 2.0, 2.0), "");
        assert_eq!(translate_path(input, 10.0, 20.0), "");
    }
}

// ===========================================================================
// 2. Single Point & Degenerate Subpaths
// ===========================================================================

#[test]
fn test_single_point_and_degenerate_subpaths() {
    let degenerate_paths = [
        "M 0 0",
        "M 100 200",
        "M 10 20 Z",
        "m 0 0",
        "m 10 10 z",
        "M 0 0 M 10 10 M 20 20",
        "M 0 0 L 0 0 L 0 0 Z",
        "M 50 50 H 50 V 50",
    ];

    for &path in &degenerate_paths {
        let len = get_length(path);
        assert!(
            len.abs() < 1e-6,
            "Degenerate path {path:?} should have length 0.0, got {len}"
        );

        let approx = approximate_path_length(path);
        assert!(
            approx.abs() < 1e-6,
            "Degenerate path {path:?} approx length should be 0.0, got {approx}"
        );

        let evolved = evolve_path(0.5, path);
        assert!(
            !evolved.stroke_dashoffset.is_nan(),
            "evolved offset is NaN for {path:?}"
        );

        let scaled = scale_path(path, 3.0, 3.0);
        assert!(
            !scaled.is_empty(),
            "scaled output should not be empty for {path:?}"
        );

        let translated = translate_path(path, 5.0, 5.0);
        assert!(
            !translated.is_empty(),
            "translated output should not be empty for {path:?}"
        );
    }
}

// ===========================================================================
// 3. Degenerate and Boundary Arcs
// ===========================================================================

#[test]
fn test_degenerate_and_boundary_arcs() {
    // 0-radius arcs (rx=0, ry=0) should degenerate to straight lines per SVG spec
    let arc_zero_radii = "M 0 0 A 0 0 0 0 1 10 0";
    let len_zero = get_length(arc_zero_radii);
    assert!(
        (len_zero - 10.0).abs() < 1e-4,
        "0-radius arc should degenerate to straight line length 10.0, got {len_zero}"
    );

    let approx_zero = approximate_path_length(arc_zero_radii);
    assert!(
        (approx_zero - 10.0).abs() < 1e-4,
        "0-radius arc approx length should be 10.0, got {approx_zero}"
    );

    // Negative radii in SVG string should be treated as positive (rx.abs(), ry.abs())
    let arc_neg_radii = "M 0 0 A -50 -50 0 0 1 100 0";
    let len_neg = get_length(arc_neg_radii);
    assert!(
        (len_neg - 50.0 * PI).abs() < 1.0,
        "Negative radii arc should produce valid semi-circle ~157.08, got {len_neg}"
    );

    // Coincident start and end point (x1 == x2, y1 == y2) should have 0 length
    let arc_coincident = "M 10 10 A 50 50 0 0 0 10 10";
    let len_coincident = get_length(arc_coincident);
    assert_eq!(
        len_coincident, 0.0,
        "Coincident start/end arc should have length 0.0, got {len_coincident}"
    );

    // Radii too small for span (lambda > 1.0) -> SVG spec F.6.2 auto-scales rx, ry
    let arc_small_radii = "M 0 0 A 1 1 0 0 1 100 0";
    let len_small = get_length(arc_small_radii);
    assert!(
        len_small > 100.0,
        "Auto-scaled radii arc should have length >= chord 100.0, got {len_small}"
    );

    // Full 360-degree circle in two arcs
    let full_circle = "M 100 0 A 100 100 0 1 0 -100 0 A 100 100 0 1 0 100 0 Z";
    let len_circle = get_length(full_circle);
    let expected_circle = 2.0 * PI * 100.0;
    assert!(
        (len_circle - expected_circle).abs() < 1.0,
        "Full circle length expected ~{expected_circle}, got {len_circle}"
    );

    let approx_circle = approximate_path_length(full_circle);
    assert!(
        (approx_circle - expected_circle).abs() < 2.0,
        "Full circle approx length expected ~{expected_circle}, got {approx_circle}"
    );
}

// ===========================================================================
// 4. Invalid Commands and Malformed Paths
// ===========================================================================

#[test]
fn test_malformed_and_invalid_path_strings() {
    let invalid_paths = [
        "X 10 20",
        "M 10",                // missing y
        "M foo bar",           // non-numeric tokens
        "M 10 20 Q 30",        // incomplete quad curve
        "M 10 20 C 1 2 3 4 5", // incomplete cubic curve
        "M 10 20 A 1 2 3 4",   // incomplete arc
        "??? !!!",
        "12345678",
        "M NaN Inf L -Inf NaN",
    ];

    for &path in &invalid_paths {
        // get_length must safely return 0.0 or valid float without panic
        let len = get_length(path);
        assert!(!len.is_nan(), "get_length returned NaN for {path:?}");

        // approximate_path_length must not panic
        let approx = approximate_path_length(path);
        assert!(
            !approx.is_nan(),
            "approximate_path_length returned NaN for {path:?}"
        );

        // evolve_path must not panic
        let evolved = evolve_path(0.5, path);
        assert!(
            !evolved.stroke_dashoffset.is_nan(),
            "evolve_path returned NaN offset for {path:?}"
        );

        // point_at_length must not panic
        let pt = get_point_at_length(path, 10.0);
        assert!(!pt.x.is_nan() && !pt.y.is_nan());

        // scale_path and translate_path must gracefully return original or empty without panic
        let _ = scale_path(path, 2.0, 2.0);
        let _ = translate_path(path, 10.0, 10.0);
    }
}

// ===========================================================================
// 5. Extreme and Scientific Notation Coordinates
// ===========================================================================

#[test]
fn test_extreme_and_scientific_coordinates() {
    // Scientific notation with positive and negative exponents
    let sci_path = "M 1e2 2e2 L 3e2 4e2 L 1e-1 2e-1";
    let parsed = parse_path(sci_path).expect("Should parse scientific notation");
    assert_eq!(parsed[0], Instruction::MoveTo { x: 100.0, y: 200.0 });
    assert_eq!(parsed[1], Instruction::LineTo { x: 300.0, y: 400.0 });
    assert_eq!(parsed[2], Instruction::LineTo { x: 0.1, y: 0.2 });

    // Compact SVG syntax (adjacent signs and decimals without spaces)
    let compact_path = "M10-20.5L.5.25";
    let parsed_compact = parse_path(compact_path).expect("Should parse compact syntax");
    assert_eq!(parsed_compact[0], Instruction::MoveTo { x: 10.0, y: -20.5 });
    assert_eq!(parsed_compact[1], Instruction::LineTo { x: 0.5, y: 0.25 });

    // Huge coordinates
    let huge_path = "M 1e12 1e12 L 2e12 2e12";
    let len_huge = get_length(huge_path);
    let expected_huge = (1e24 + 1e24_f64).sqrt();
    assert!(
        (len_huge - expected_huge).abs() / expected_huge < 1e-5,
        "Huge path length calculation mismatch"
    );
}

// ===========================================================================
// 6. Multi-Subpath Paths and ClosePath Semantics
// ===========================================================================

#[test]
fn test_multi_subpaths_and_close_path() {
    // 3 distinct closed triangles at different offsets
    // Triangle 1: (0,0) -> (3,0) -> (3,4) -> Z (chord to 0,0 is 5) -> perimeter 12
    // Triangle 2: (10,10) -> (13,10) -> (13,14) -> Z -> perimeter 12
    // Triangle 3: (100,100) -> (103,100) -> (103,104) -> Z -> perimeter 12
    let multi_path =
        "M 0 0 L 3 0 L 3 4 Z M 10 10 L 13 10 L 13 14 Z M 100 100 L 103 100 L 103 104 Z";
    let len = get_length(multi_path);
    assert!(
        (len - 36.0).abs() < 0.1,
        "Multi-subpath length expected ~36.0, got {len}"
    );

    let approx = approximate_path_length(multi_path);
    assert!(
        (approx - 36.0).abs() < 0.1,
        "Multi-subpath approx length expected ~36.0, got {approx}"
    );

    // Verify point at length walks through subpaths correctly
    let pt_start = get_point_at_length(multi_path, 0.0);
    assert_eq!(pt_start, Point { x: 0.0, y: 0.0 });

    let pt_second_subpath = get_point_at_length(multi_path, 15.0); // 12 + 3 -> (13, 10)
    assert!(
        (pt_second_subpath.x - 13.0).abs() < 0.5 && (pt_second_subpath.y - 10.0).abs() < 0.5,
        "Point at length 15 should be near (13, 10), got {:?}",
        pt_second_subpath
    );
}

// ===========================================================================
// 7. Path Interpolation Stress & Mismatch Fallbacks
// ===========================================================================

#[test]
fn test_interpolate_path_stress_and_mismatches() {
    let from_rect = "M 0 0 L 100 0 L 100 100 L 0 100 Z";
    let to_rect = "M 10 10 L 110 10 L 110 110 L 10 110 Z";

    // Standard interpolation at boundary progresses
    assert_eq!(interpolate_path(from_rect, to_rect, 0.0), from_rect);
    assert_eq!(interpolate_path(from_rect, to_rect, 1.0), to_rect);

    // Clamping on out-of-range progress
    assert_eq!(interpolate_path(from_rect, to_rect, -10.0), from_rect);
    assert_eq!(interpolate_path(from_rect, to_rect, 10.0), to_rect);

    // Exact midpoint interpolation
    let mid = interpolate_path(from_rect, to_rect, 0.5);
    assert_eq!(mid, "M 5 5 L 105 5 L 105 105 L 5 105 Z");

    // Mismatched token counts fall back to `from` (< 0.5) and `to` (>= 0.5)
    let short_path = "M 0 0 L 10 10";
    let long_path = "M 0 0 L 10 10 L 20 20 L 30 30";

    assert_eq!(interpolate_path(short_path, long_path, 0.2), short_path);
    assert_eq!(interpolate_path(short_path, long_path, 0.499), short_path);
    assert_eq!(interpolate_path(short_path, long_path, 0.5), long_path);
    assert_eq!(interpolate_path(short_path, long_path, 0.8), long_path);

    // Empty path interpolations
    assert_eq!(interpolate_path("", short_path, 0.3), "");
    assert_eq!(interpolate_path("", short_path, 0.7), short_path);
    assert_eq!(interpolate_path(short_path, "", 0.3), short_path);
    assert_eq!(interpolate_path(short_path, "", 0.7), "");
}

// ===========================================================================
// 8. Evolve Path Boundary Conditions
// ===========================================================================

#[test]
fn test_evolve_path_boundary_conditions() {
    let path = "M 0 0 L 100 0"; // length 100

    // Extreme progress values
    let ev_neg_huge = evolve_path(-1000.0, path);
    assert_eq!(ev_neg_huge.stroke_dashoffset, 150.0);
    assert_eq!(ev_neg_huge.stroke_dasharray, "150.0000 150.0000");

    let ev_pos_huge = evolve_path(1000.0, path);
    assert_eq!(ev_pos_huge.stroke_dashoffset, 0.0);
    assert_eq!(ev_pos_huge.stroke_dasharray, "100.0000 100.0000");

    // evolve_path_with_length zero length
    let ev_zero_len = evolve_path_with_length(0.5, 0.0);
    assert_eq!(ev_zero_len.stroke_dashoffset, 0.0);
    assert_eq!(ev_zero_len.stroke_dasharray, "0.0000 0.0000");
}
