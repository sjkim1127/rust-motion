use rust_motion_paths::{
    get_instructions_length, parse_path, repeat_paths, round_corners, trim_paths, wiggle_paths,
    Instruction, Point, RepeaterConfig, TrimMode, WigglePointType,
};

#[test]
fn test_trim_paths_bezier_curve_precision() {
    // S-curve cubic bezier: M 0 0 C 0 100, 100 100, 100 0
    let path = "M 0 0 C 0 100, 100 100, 100 0";
    let insts = parse_path(path).unwrap();
    let total_len = get_instructions_length(&insts);

    // Trim 25% to 75% (the middle 50% of the S-curve)
    let trimmed = trim_paths(&insts, 0.25, 0.75, 0.0, TrimMode::Simultaneously);
    let trimmed_len = get_instructions_length(&trimmed);

    let expected_len = total_len * 0.5;
    assert!(
        (trimmed_len - expected_len).abs() < 1.0,
        "Trimmed bezier curve length discrepancy: expected {expected_len}, got {trimmed_len}"
    );

    // Must still contain a valid CubicCurveTo
    assert!(trimmed.iter().any(|i| matches!(i, Instruction::CubicCurveTo { .. })));
}

#[test]
fn test_trim_paths_individually_multi_subpath() {
    // Two separate lines: Subpath 1 (length 100) + Subpath 2 (length 100) = Total 200
    let path = "M 0 0 L 100 0 M 0 50 L 100 50";
    let insts = parse_path(path).unwrap();
    let total_len = get_instructions_length(&insts);
    assert!((total_len - 200.0).abs() < 1e-3);

    // In Individually mode, trimming [0.0, 0.75] covers:
    // 100% of Subpath 1 (100px) + 50% of Subpath 2 (50px) = 150px
    let trimmed_indiv = trim_paths(&insts, 0.0, 0.75, 0.0, TrimMode::Individually);
    let indiv_len = get_instructions_length(&trimmed_indiv);
    assert!(
        (indiv_len - 150.0).abs() < 1e-3,
        "Individually mode should yield 150px, got {indiv_len}"
    );

    // In Simultaneously mode, trimming [0.0, 0.75] trims 75% of each subpath:
    // 75px + 75px = 150px, but subpath 2 has its start at (0, 50) and reaches (75, 50)
    let trimmed_simul = trim_paths(&insts, 0.0, 0.75, 0.0, TrimMode::Simultaneously);
    let simul_len = get_instructions_length(&trimmed_simul);
    assert!(
        (simul_len - 150.0).abs() < 1e-3,
        "Simultaneously mode should yield 150px, got {simul_len}"
    );
}

#[test]
fn test_radial_repeater_circle_array() {
    // Single petal pointing up: M 0 0 L 0 -50
    let path = "M 0 0 L 0 -50";
    let insts = parse_path(path).unwrap();

    // 8 copies rotated by 45 degrees each around origin (0, 0)
    let cfg = RepeaterConfig {
        copies: 8,
        offset: 0.0,
        position: Point::new(0.0, 0.0),
        scale: Point::new(1.0, 1.0),
        rotation: 45.0,
        start_opacity: 1.0,
        end_opacity: 1.0,
        anchor_point: Point::new(0.0, 0.0),
    };

    let instances = repeat_paths(&insts, &cfg);
    assert_eq!(instances.len(), 8);

    // Check 2nd copy (index 2: rotated by 90 degrees) -> should point right along X axis
    let inst2 = &instances[2].instructions;
    if let Instruction::LineTo { x, y } = inst2[1] {
        // (0, -50) rotated +90° = (50, 0)
        assert!((x - 50.0).abs() < 1e-3, "Rotated 90 deg line endpoint X: {x}");
        assert!(y.abs() < 1e-3, "Rotated 90 deg line endpoint Y: {y}");
    } else {
        panic!("Expected LineTo instruction");
    }
}

#[test]
fn test_round_corners_triangle_fillets() {
    // Triangle: (0, 0) -> (100, 0) -> (50, 86.6) -> Z
    let path = "M 0 0 L 100 0 L 50 86.6 Z";
    let insts = parse_path(path).unwrap();

    let rounded = round_corners(&insts, 15.0);
    // Should have 3 QuadCurveTo fillets for the 3 vertices
    let fillet_count = rounded
        .iter()
        .filter(|i| matches!(i, Instruction::QuadCurveTo { .. }))
        .count();
    assert_eq!(fillet_count, 3, "Expected 3 rounded corner fillets");
}

#[test]
fn test_wiggle_paths_smooth_vs_corner() {
    let path = "M 0 0 L 100 0 L 200 0";
    let insts = parse_path(path).unwrap();

    let corner_wiggle = wiggle_paths(&insts, 5.0, 1.0, 0.0, WigglePointType::Corner);
    let smooth_wiggle = wiggle_paths(&insts, 5.0, 1.0, 0.0, WigglePointType::Smooth);

    assert!(corner_wiggle.iter().all(|i| !matches!(i, Instruction::QuadCurveTo { .. })));
    assert!(smooth_wiggle.iter().any(|i| matches!(i, Instruction::QuadCurveTo { .. })));
}
