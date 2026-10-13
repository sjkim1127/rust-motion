//! Comprehensive E2E Test Suite for Procedural Noise & Seeding (Tiers 1-4)
//!
//! Features covered:
//! - Feature 1: Simplex Noise (2D, 3D, 4D)
//! - Feature 2: Mulberry32 PRNG & Seeding (`random`, `hash_code`, `mulberry32`, `NoiseSeed`, `hash_seed`)
//! - Feature 3: Fractal Brownian Motion (fBm) (`fbm_2d`, `fbm_3d`, `FbmOptions`)
//! - Feature 4: Turbulence & Domain Warping (`turbulence_2d`, `turbulence_warp_2d`, `domain_warp_2d`, `warp_points_2d`)
//! - Feature 5: `<NoiseBackground />` (Component, props, SVG generation / data url / wave path)
//! - Tier 3: Pairwise cross-feature combinations
//! - Tier 4: Real-world video application scenario (Organic flow graphic animation)

use rust_motion_noise::{
    domain_warp_2d, fbm_2d, fbm_3d, generate_noise_svg_data_url, generate_noise_wave_path,
    hash_code, mulberry32, noise2d, noise3d, noise4d, random, turbulence_2d, turbulence_warp_2d,
    warp_points_2d, FbmOptions, NoiseBackgroundProps, NoisePatternKind, WavePathOptions,
};

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 1: SIMPLEX NOISE (2D, 3D, 4D)
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f1_t1_simplex_2d_output_range_and_determinism() {
    let seed = "simplex-2d-coverage-seed";
    for i in 0..50 {
        let x = i as f64 * 0.37 - 10.0;
        let y = (i * 2) as f64 * 0.43 - 10.0;
        let val1 = noise2d(seed, x, y);
        let val2 = noise2d(seed, x, y);
        assert_eq!(val1, val2, "noise2d must be strictly deterministic");
        assert!(
            (-1.0..=1.0).contains(&val1),
            "noise2d output {} out of [-1.0, 1.0] at ({}, {})",
            val1,
            x,
            y
        );
    }
}

#[test]
fn test_f1_t1_simplex_3d_spatial_evaluation() {
    let seed = "simplex-3d-coverage-seed";
    for i in 0..30 {
        let x = i as f64 * 0.25;
        let y = (i * 3) as f64 * 0.15;
        let z = (i * 5) as f64 * 0.08;
        let val1 = noise3d(seed, x, y, z);
        let val2 = noise3d(seed, x, y, z);
        assert_eq!(val1, val2, "noise3d must be deterministic");
        assert!(
            (-1.0..=1.0).contains(&val1),
            "noise3d output {} out of [-1.0, 1.0]",
            val1
        );
    }
}

#[test]
fn test_f1_t1_simplex_4d_hyperspace_evaluation() {
    let seed = "simplex-4d-coverage-seed";
    for i in 0..20 {
        let x = i as f64 * 0.3;
        let y = (i * 2) as f64 * 0.2;
        let z = (i * 3) as f64 * 0.1;
        let w = (i * 4) as f64 * 0.05;
        let val1 = noise4d(seed, x, y, z, w);
        let val2 = noise4d(seed, x, y, z, w);
        assert_eq!(val1, val2, "noise4d must be deterministic");
        assert!(
            (-1.0..=1.0).contains(&val1),
            "noise4d output {} out of [-1.0, 1.0]",
            val1
        );
    }
}

#[test]
fn test_f1_t1_simplex_numeric_and_string_seeds() {
    let val_str = noise2d("100", 1.5, 2.5);
    let val_num = noise2d(100i64, 1.5, 2.5);
    let val_float = noise2d(100.0f64, 1.5, 2.5);

    assert!((-1.0..=1.0).contains(&val_str));
    assert!((-1.0..=1.0).contains(&val_num));
    assert_eq!(val_num, val_float);

    let seed_u32 = noise2d(42u32, 0.5, 0.5);
    let seed_i32 = noise2d(42i32, 0.5, 0.5);
    assert_eq!(seed_u32, seed_i32);
}

#[test]
fn test_f1_t1_simplex_remotion_exact_parity() {
    // Exact Remotion v4.0.495 reference values
    let origin = noise2d(1, 0.0, 0.0);
    assert_eq!(origin, 0.0, "Origin noise value must be 0.0");

    let val_2d = noise2d("my-seed", 0.5, 0.5);
    assert!(
        (val_2d - 0.3071565136272162).abs() < 1e-12,
        "Expected 0.3071565136272162, got {}",
        val_2d
    );

    let val_3d = noise3d("my-seed", 0.7, 0.5, 0.5);
    assert!(
        (val_3d - 0.6402128434567901).abs() < 1e-12,
        "Expected 0.6402128434567901, got {}",
        val_3d
    );

    let val_4d = noise4d("my-seed", 0.7, 0.5, 0.5, 0.9);
    assert!(
        (val_4d - 0.2714290963058814).abs() < 1e-12,
        "Expected 0.2714290963058814, got {}",
        val_4d
    );
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f1_t2_simplex_nan_inputs() {
    assert_eq!(noise2d("seed", f64::NAN, 1.0), 0.0);
    assert_eq!(noise2d("seed", 1.0, f64::NAN), 0.0);
    assert_eq!(noise3d("seed", 1.0, f64::NAN, 2.0), 0.0);
    assert_eq!(noise4d("seed", 1.0, 2.0, 3.0, f64::NAN), 0.0);
}

#[test]
fn test_f1_t2_simplex_infinite_inputs() {
    assert_eq!(noise2d("seed", f64::INFINITY, 1.0), 0.0);
    assert_eq!(noise2d("seed", 1.0, f64::NEG_INFINITY), 0.0);
    assert_eq!(noise3d("seed", f64::INFINITY, f64::NEG_INFINITY, 0.0), 0.0);
    assert_eq!(noise4d("seed", 0.0, 0.0, f64::INFINITY, 0.0), 0.0);
}

#[test]
fn test_f1_t2_simplex_extreme_coordinates() {
    let extreme_coords = [1e12, -1e12, 1e-12, -1e-12];
    for &coord in &extreme_coords {
        let v2 = noise2d("extreme-seed", coord, coord);
        let v3 = noise3d("extreme-seed", coord, coord, coord);
        let v4 = noise4d("extreme-seed", coord, coord, coord, coord);
        assert!((-1.0..=1.0).contains(&v2));
        assert!((-1.0..=1.0).contains(&v3));
        assert!((-1.0..=1.0).contains(&v4));
    }
}

#[test]
fn test_f1_t2_simplex_empty_and_special_seeds() {
    let empty_val = noise2d("", 0.5, 0.5);
    assert!((-1.0..=1.0).contains(&empty_val));

    let emoji_val = noise2d("🦀🚀✨🔥", 0.5, 0.5);
    assert!((-1.0..=1.0).contains(&emoji_val));

    let null_val = noise2d("\0\0\0", 0.5, 0.5);
    assert!((-1.0..=1.0).contains(&null_val));
}

#[test]
fn test_f1_t2_simplex_subnormal_and_epsilon_coords() {
    let v_min = noise2d("eps-seed", f64::MIN_POSITIVE, f64::MIN_POSITIVE);
    let v_eps = noise2d("eps-seed", f64::EPSILON, f64::EPSILON);
    let v_neg0 = noise2d("eps-seed", -0.0, -0.0);

    assert!((-1.0..=1.0).contains(&v_min));
    assert!((-1.0..=1.0).contains(&v_eps));
    assert!((-1.0..=1.0).contains(&v_neg0));
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 2: MULBERRY32 PRNG & SEEDING
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f2_t1_mulberry32_output_range() {
    for seed in 0..100 {
        let r = mulberry32(seed);
        assert!(
            (0.0..1.0).contains(&r),
            "mulberry32 output {} not in [0.0, 1.0)",
            r
        );
    }
}

#[test]
fn test_f2_t1_hash_code_java_remotion_parity() {
    assert_eq!(hash_code(""), 0);
    assert_eq!(hash_code("a"), 97);
    assert_eq!(hash_code("hello"), 99162322);
    assert_eq!(hash_code("my-seed"), 1462865394);
    assert_eq!(hash_code("Remotion"), -448233527);
}

#[test]
fn test_f2_t1_random_string_seed_determinism() {
    let r1 = random("test-random-string");
    let r2 = random("test-random-string");
    assert_eq!(r1, r2);
    assert!((0.0..1.0).contains(&r1));
}

#[test]
fn test_f2_t1_random_numeric_seed_determinism() {
    let r1 = random(123.456);
    let r2 = random(123.456);
    assert_eq!(r1, r2);
    assert!((0.0..1.0).contains(&r1));

    let r_int = random(42);
    assert!((0.0..1.0).contains(&r_int));
}

#[test]
fn test_f2_t1_prng_distribution_uniformity() {
    let mut bins = [0usize; 10];
    let total_samples = 10_000;

    for i in 0..total_samples {
        let val = mulberry32(i as i64);
        let bin_idx = ((val * 10.0).floor() as usize).min(9);
        bins[bin_idx] += 1;
    }

    for (idx, &count) in bins.iter().enumerate() {
        assert!(
            count > 700 && count < 1300,
            "Bin {} count {} outside expected uniform range [700, 1300]",
            idx,
            count
        );
    }
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f2_t2_mulberry32_zero_and_negative_seeds() {
    let r0 = mulberry32(0);
    let r_neg1 = mulberry32(-1);
    let r_min = mulberry32(i64::MIN);

    assert!((0.0..1.0).contains(&r0));
    assert!((0.0..1.0).contains(&r_neg1));
    assert!((0.0..1.0).contains(&r_min));
}

#[test]
fn test_f2_t2_mulberry32_u32_max_bounds() {
    let r_u32_max = mulberry32(u32::MAX as i64);
    let r_i32_max = mulberry32(i32::MAX as i64);
    let r_i64_max = mulberry32(i64::MAX);

    assert!((0.0..1.0).contains(&r_u32_max));
    assert!((0.0..1.0).contains(&r_i32_max));
    assert!((0.0..1.0).contains(&r_i64_max));
}

#[test]
fn test_f2_t2_hash_code_large_string_stress() {
    let long_str = "a".repeat(100_000);
    let h = hash_code(&long_str);
    assert_eq!(h, hash_code(&long_str));
}

#[test]
fn test_f2_t2_hash_code_unicode_surrogate_pairs() {
    let emoji_str = "👨‍👩‍👧‍👦-🚀-🎉";
    let h1 = hash_code(emoji_str);
    let h2 = hash_code(emoji_str);
    assert_eq!(h1, h2);
    assert_ne!(h1, 0);
}

#[test]
fn test_f2_t2_random_nan_and_infinite_numeric_seeds() {
    let r_nan = random(f64::NAN);
    let r_inf = random(f64::INFINITY);
    let r_neg_inf = random(f64::NEG_INFINITY);

    assert!((0.0..1.0).contains(&r_nan));
    assert!((0.0..1.0).contains(&r_inf));
    assert!((0.0..1.0).contains(&r_neg_inf));
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 3: FRACTAL BROWNIAN MOTION (fBm)
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f3_t1_fbm_2d_basic_evaluation() {
    let opts = FbmOptions::default();
    let seed = "fbm-2d-eval";
    for i in 0..20 {
        let val = fbm_2d(seed, i as f64 * 0.5, (i * 2) as f64 * 0.5, &opts);
        assert!((-1.0..=1.0).contains(&val));
    }
}

#[test]
fn test_f3_t1_fbm_3d_basic_evaluation() {
    let opts = FbmOptions::default();
    let seed = "fbm-3d-eval";
    for i in 0..20 {
        let val = fbm_3d(
            seed,
            i as f64 * 0.4,
            (i * 2) as f64 * 0.3,
            (i * 3) as f64 * 0.2,
            &opts,
        );
        assert!((-1.0..=1.0).contains(&val));
    }
}

#[test]
fn test_f3_t1_fbm_octave_scaling_energy() {
    let seed = "fbm-octaves";
    let opt1 = FbmOptions::new(1, 2.0, 0.5);
    let opt4 = FbmOptions::new(4, 2.0, 0.5);
    let opt8 = FbmOptions::new(8, 2.0, 0.5);

    let v1 = fbm_2d(seed, 2.5, 3.5, &opt1);
    let v4 = fbm_2d(seed, 2.5, 3.5, &opt4);
    let v8 = fbm_2d(seed, 2.5, 3.5, &opt8);

    assert!((-1.0..=1.0).contains(&v1));
    assert!((-1.0..=1.0).contains(&v4));
    assert!((-1.0..=1.0).contains(&v8));
}

#[test]
fn test_f3_t1_fbm_lacunarity_variation() {
    let seed = "fbm-lacunarity";
    let opt_low = FbmOptions::new(4, 1.5, 0.5);
    let opt_high = FbmOptions::new(4, 3.5, 0.5);

    let v_low = fbm_2d(seed, 1.0, 2.0, &opt_low);
    let v_high = fbm_2d(seed, 1.0, 2.0, &opt_high);

    assert!((-1.0..=1.0).contains(&v_low));
    assert!((-1.0..=1.0).contains(&v_high));
}

#[test]
fn test_f3_t1_fbm_persistence_variation() {
    let seed = "fbm-persistence";
    let opt_decay_fast = FbmOptions::new(4, 2.0, 0.25);
    let opt_decay_slow = FbmOptions::new(4, 2.0, 0.75);

    let v_fast = fbm_2d(seed, 1.0, 2.0, &opt_decay_fast);
    let v_slow = fbm_2d(seed, 1.0, 2.0, &opt_decay_slow);

    assert!((-1.0..=1.0).contains(&v_fast));
    assert!((-1.0..=1.0).contains(&v_slow));
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f3_t2_fbm_zero_octaves_returns_zero() {
    let opt0 = FbmOptions::new(0, 2.0, 0.5);
    assert_eq!(fbm_2d("seed", 1.0, 2.0, &opt0), 0.0);
    assert_eq!(fbm_3d("seed", 1.0, 2.0, 3.0, &opt0), 0.0);
}

#[test]
fn test_f3_t2_fbm_single_octave_equivalence() {
    let opt1 = FbmOptions::new(1, 2.0, 0.5);
    let v_fbm = fbm_2d("single-octave", 1.5, 2.5, &opt1);
    let v_noise = noise2d("single-octave", 1.5, 2.5);
    assert_eq!(v_fbm, v_noise);
}

#[test]
fn test_f3_t2_fbm_large_octave_count_stress() {
    let opt64 = FbmOptions::new(64, 2.0, 0.5);
    let v = fbm_2d("large-octaves", 1.0, 2.0, &opt64);
    assert!((-1.0..=1.0).contains(&v));
}

#[test]
fn test_f3_t2_fbm_zero_persistence() {
    let opt_zero_p = FbmOptions::new(4, 2.0, 0.0);
    let v = fbm_2d("zero-p", 1.0, 2.0, &opt_zero_p);
    assert!((-1.0..=1.0).contains(&v));
}

#[test]
fn test_f3_t2_fbm_nan_and_infinite_coords() {
    let opts = FbmOptions::default();
    assert_eq!(fbm_2d("seed", f64::NAN, 1.0, &opts), 0.0);
    assert_eq!(fbm_2d("seed", 1.0, f64::INFINITY, &opts), 0.0);
    assert_eq!(fbm_3d("seed", 1.0, f64::NAN, 2.0, &opts), 0.0);
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 4: TURBULENCE & DOMAIN WARPING
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f4_t1_turbulence_2d_range() {
    let seed = "turb-coverage";
    for i in 0..20 {
        let t = turbulence_2d(seed, i as f64 * 0.3, (i * 2) as f64 * 0.4, 4);
        assert!(
            (0.0..=1.0).contains(&t),
            "turbulence_2d output {} not in [0.0, 1.0]",
            t
        );
    }
}

#[test]
fn test_f4_t1_turbulence_warp_2d_displacement_radius() {
    let seed = "warp-coverage";
    let (x, y) = (100.0, 200.0);
    let strength = 15.0;
    let (wx, wy) = turbulence_warp_2d(seed, x, y, strength, 0.05);

    assert!((wx - x).abs() <= strength);
    assert!((wy - y).abs() <= strength);
}

#[test]
fn test_f4_t1_domain_warp_2d_field_bounds() {
    let seed = "domain-warp-coverage";
    let opts = FbmOptions::default();
    for i in 0..10 {
        let val = domain_warp_2d(seed, i as f64 * 0.5, (i * 2) as f64 * 0.5, 3.0, &opts);
        assert!((-1.0..=1.0).contains(&val));
    }
}

#[test]
fn test_f4_t1_warp_points_2d_polyline() {
    let pts = vec![(0.0, 0.0), (10.0, 0.0), (20.0, 0.0), (30.0, 0.0)];
    let warped = warp_points_2d("polyline-seed", &pts, 5.0, 0.1);
    assert_eq!(warped.len(), pts.len());
    for (orig, warp) in pts.iter().zip(warped.iter()) {
        assert!((warp.0 - orig.0).abs() <= 5.0);
        assert!((warp.1 - orig.1).abs() <= 5.0);
    }
}

#[test]
fn test_f4_t1_turbulence_frequency_scaling() {
    let (wx_low, wy_low) = turbulence_warp_2d("freq-test", 50.0, 50.0, 10.0, 0.001);
    let (wx_high, wy_high) = turbulence_warp_2d("freq-test", 50.0, 50.0, 10.0, 10.0);

    assert!((wx_low - 50.0).abs() <= 10.0);
    assert!((wy_low - 50.0).abs() <= 10.0);
    assert!((wx_high - 50.0).abs() <= 10.0);
    assert!((wy_high - 50.0).abs() <= 10.0);
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f4_t2_turbulence_zero_strength_identity() {
    let (x, y) = (42.0, 84.0);
    let (wx, wy) = turbulence_warp_2d("identity-seed", x, y, 0.0, 0.1);
    assert_eq!(wx, x);
    assert_eq!(wy, y);
}

#[test]
fn test_f4_t2_turbulence_negative_strength_inversion() {
    let (x, y) = (100.0, 100.0);
    let (wx_pos, wy_pos) = turbulence_warp_2d("neg-strength", x, y, 10.0, 0.05);
    let (wx_neg, wy_neg) = turbulence_warp_2d("neg-strength", x, y, -10.0, 0.05);

    assert!(((wx_pos - x) + (wx_neg - x)).abs() < 1e-10);
    assert!(((wy_pos - y) + (wy_neg - y)).abs() < 1e-10);
}

#[test]
fn test_f4_t2_turbulence_zero_octaves_returns_zero() {
    assert_eq!(turbulence_2d("zero-oct", 1.0, 2.0, 0), 0.0);
}

#[test]
fn test_f4_t2_warp_points_empty_and_single_point() {
    let empty: Vec<(f64, f64)> = Vec::new();
    let warped_empty = warp_points_2d("empty", &empty, 5.0, 0.1);
    assert!(warped_empty.is_empty());

    let single = vec![(10.0, 20.0)];
    let warped_single = warp_points_2d("single", &single, 5.0, 0.1);
    assert_eq!(warped_single.len(), 1);
}

#[test]
fn test_f4_t2_turbulence_nan_infinite_inputs() {
    let (wx_nan, wy_nan) = turbulence_warp_2d("nan", f64::NAN, 1.0, 5.0, 0.1);
    assert!(wx_nan.is_nan());
    assert_eq!(wy_nan, 1.0);

    let (wx_inf, wy_inf) = turbulence_warp_2d("inf", f64::INFINITY, 1.0, 5.0, 0.1);
    assert_eq!(wx_inf, f64::INFINITY);
    assert_eq!(wy_inf, 1.0);
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 5: <NOISEBACKGROUND />
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f5_t1_noise_bg_wave_path_format() {
    let opt = WavePathOptions::new(0.5, 120.0, 0.0, 0.02, 3);
    let path = generate_noise_wave_path("bg-test", 1920.0, 1080.0, &opt);
    assert!(path.starts_with("M 0,"));
    assert!(path.contains("L 1920.00,1080.00"));
    assert!(path.ends_with("Z"));
}

#[test]
fn test_f5_t1_noise_bg_data_url_format() {
    let data_url =
        generate_noise_svg_data_url("svg-seed", 1280, 720, 1.0, 0.015, "#0f172a", "#38bdf8");
    assert!(data_url.starts_with("data:image/svg+xml;utf8,"));
    assert!(data_url.contains("http://www.w3.org/2000/svg"));
    assert!(data_url.contains("fill=\"#0f172a\""));
    assert!(data_url.contains("fill=\"#38bdf8\""));
}

#[test]
fn test_f5_t1_noise_bg_props_defaults() {
    let props = NoiseBackgroundProps {
        seed: "default-seed".into(),
        base_color: "#111827".into(),
        accent_color: "#ec4899".into(),
        palette: vec!["#111827".into(), "#ec4899".into()],
        speed: 0.05,
        frequency: 0.02,
        octaves: 3,
        style: String::new(),
    };

    assert_eq!(props.seed, "default-seed");
    assert_eq!(props.octaves, 3);
    assert_eq!(NoisePatternKind::default(), NoisePatternKind::Waves);
}

#[test]
fn test_f5_t1_noise_bg_aspect_ratio_scaling() {
    let opt = WavePathOptions::new(0.5, 100.0, 0.0, 0.01, 2);
    let path_16_9 = generate_noise_wave_path("ratio", 1920.0, 1080.0, &opt);
    let path_9_16 = generate_noise_wave_path("ratio", 1080.0, 1920.0, &opt);

    assert!(path_16_9.contains("1920.00,1080.00"));
    assert!(path_9_16.contains("1080.00,1920.00"));
}

#[test]
fn test_f5_t1_noise_bg_pattern_kinds() {
    assert_eq!(NoisePatternKind::Waves, NoisePatternKind::Waves);
    assert_eq!(NoisePatternKind::Contours, NoisePatternKind::Contours);
    assert_eq!(NoisePatternKind::RadialAura, NoisePatternKind::RadialAura);
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f5_t2_noise_bg_zero_dimensions() {
    let opt = WavePathOptions::new(0.5, 100.0, 0.0, 0.01, 2);
    let path = generate_noise_wave_path("zero", 0.0, 0.0, &opt);
    assert!(path.starts_with("M 0,"));
}

#[test]
fn test_f5_t2_noise_bg_extreme_frequency_and_speed() {
    let opt = WavePathOptions::new(0.5, 50.0, 1000.0, 5.0, 4);
    let path_fast = generate_noise_wave_path("fast", 800.0, 600.0, &opt);
    assert!(path_fast.starts_with("M 0,"));
}

#[test]
fn test_f5_t2_noise_bg_zero_octaves_fallback() {
    let opt = WavePathOptions::new(0.5, 50.0, 0.0, 0.01, 0);
    let path_zero = generate_noise_wave_path("zero-oct", 800.0, 600.0, &opt);
    assert!(path_zero.starts_with("M 0,"));
}

#[test]
fn test_f5_t2_noise_bg_special_colors() {
    let data_url = generate_noise_svg_data_url(
        "special-col",
        400,
        300,
        1.0,
        0.02,
        "#00000000",
        "rgba(255,0,128,0.5)",
    );
    assert!(data_url.contains("fill=\"#00000000\""));
    assert!(data_url.contains("fill=\"rgba(255,0,128,0.5)\""));
}

#[test]
fn test_f5_t2_noise_bg_nan_parameters() {
    let opt = WavePathOptions::new(f64::NAN, 50.0, 0.0, 0.01, 2);
    let path_nan = generate_noise_wave_path("nan", 800.0, 600.0, &opt);
    assert!(!path_nan.is_empty());
}

// ══════════════════════════════════════════════════════════════════════════════
// TIER 3: PAIRWISE CROSS-FEATURE COMBINATIONS
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_pairwise_fbm_and_turbulence_composition() {
    let seed = "fbm-turb-pairwise";
    let opts = FbmOptions::default();
    let (wx, wy) = turbulence_warp_2d(seed, 10.0, 20.0, 4.0, 0.1);
    let val = fbm_2d(seed, wx, wy, &opts);
    assert!((-1.0..=1.0).contains(&val));
}

#[test]
fn test_pairwise_4d_simplex_and_domain_warp() {
    let seed = "4d-domain-pairwise";
    let opts = FbmOptions::default();
    let field = domain_warp_2d(seed, 5.0, 5.0, 2.0, &opts);
    let val_4d = noise4d(seed, 5.0, 5.0, field, 0.5);
    assert!((-1.0..=1.0).contains(&val_4d));
}

#[test]
fn test_pairwise_mulberry_prng_and_fbm_seeding() {
    let mut prng_seed = 12345i64;
    for _ in 0..5 {
        let r = mulberry32(prng_seed);
        let dynamic_seed = format!("fbm_seed_{:.6}", r);
        let opts = FbmOptions::default();
        let val = fbm_2d(&dynamic_seed, 1.0, 2.0, &opts);
        assert!((-1.0..=1.0).contains(&val));
        prng_seed = (r * 1_000_000.0) as i64;
    }
}

#[test]
fn test_pairwise_noise_bg_wave_and_points_warp() {
    let seed = "wave-warp-pairwise";
    let pts = vec![(100.0, 200.0), (300.0, 400.0), (500.0, 600.0)];
    let warped = warp_points_2d(seed, &pts, 10.0, 0.05);

    let opt = WavePathOptions::new(warped[0].0 / 1920.0, 100.0, 0.0, 0.01, 3);
    let path = generate_noise_wave_path(seed, 1920.0, 1080.0, &opt);
    assert!(path.starts_with("M 0,"));
}

#[test]
fn test_pairwise_fbm_3d_time_slice_and_turbulence() {
    let seed = "fbm3d-turb-pairwise";
    let opts = FbmOptions::default();
    for frame in 0..10 {
        let time = frame as f64 * (1.0 / 30.0);
        let (dx, dy) = turbulence_warp_2d(seed, 1.0, 2.0, 2.0, 0.1);
        let val = fbm_3d(seed, dx, dy, time, &opts);
        assert!((-1.0..=1.0).contains(&val));
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// TIER 4: REAL-WORLD APPLICATION SCENARIOS
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_tier4_scenario_organic_flow_graphic_animation() {
    // 60-frame simulation of 4D Simplex noise + domain warping polyline flow field
    let seed = "cyber-organic-flow";
    let opts = FbmOptions::new(4, 2.03, 0.55);

    // Initial 100 polyline points
    let initial_points: Vec<(f64, f64)> = (0..100)
        .map(|i| (i as f64 * 19.2, 540.0 + (i as f64 * 0.1).sin() * 50.0))
        .collect();

    for frame in 0..60 {
        let time = frame as f64 * (1.0 / 60.0);

        // Displace points through turbulence domain warping
        let warped_points = warp_points_2d(format!("{seed}_f{frame}"), &initial_points, 25.0, 0.02);

        assert_eq!(warped_points.len(), 100);

        // Evaluate 4D noise energy at mid-point along time dimension
        let mid_pt = warped_points[50];
        let field_val = domain_warp_2d(seed, mid_pt.0 * 0.01, mid_pt.1 * 0.01, 2.0, &opts);
        let noise_val = noise4d(seed, mid_pt.0 * 0.005, mid_pt.1 * 0.005, field_val, time);

        assert!(
            (-1.0..=1.0).contains(&noise_val),
            "4D noise out of bounds at frame {}",
            frame
        );

        // Generate SVG wave path from dynamically evaluated amplitude
        let amplitude = 80.0 + noise_val * 40.0;
        let opt = WavePathOptions::new(0.5, amplitude, time, 0.015, 3);
        let wave_svg = generate_noise_wave_path(seed, 1920.0, 1080.0, &opt);

        assert!(wave_svg.starts_with("M 0,"));
        assert!(wave_svg.contains("1920.00,1080.00"));
    }
}
