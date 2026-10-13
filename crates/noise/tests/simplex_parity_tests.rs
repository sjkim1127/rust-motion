//! Comprehensive mathematical parity and determinism integration tests for Simplex noise.

use rust_motion_noise::{
    noise2d, noise3d, noise4d, noise_2d, noise_3d, noise_4d, NoiseSeed, SimplexNoise,
};

#[test]
fn test_exact_remotion_spec_parity() {
    // Exact reference values from Remotion v4.0.495 test suite
    let n_origin = noise2d(1, 0.0, 0.0);
    assert_eq!(n_origin, 0.0);

    let n2d = noise2d("my-seed", 0.5, 0.5);
    assert!(
        (n2d - 0.3071565136272162).abs() < 1e-12,
        "Expected 0.3071565136272162, got {}",
        n2d
    );

    let n3d = noise3d("my-seed", 0.7, 0.5, 0.5);
    assert!(
        (n3d - 0.6402128434567901).abs() < 1e-12,
        "Expected 0.6402128434567901, got {}",
        n3d
    );

    let n4d = noise4d("my-seed", 0.7, 0.5, 0.5, 0.9);
    assert!(
        (n4d - 0.2714290963058814).abs() < 1e-12,
        "Expected 0.2714290963058814, got {}",
        n4d
    );
}

#[test]
fn test_aliases_equality() {
    let seed = "alias-seed-test";
    assert_eq!(noise2d(seed, 1.23, 4.56), noise_2d(seed, 1.23, 4.56));
    assert_eq!(
        noise3d(seed, 1.23, 4.56, 7.89),
        noise_3d(seed, 1.23, 4.56, 7.89)
    );
    assert_eq!(
        noise4d(seed, 1.23, 4.56, 7.89, 0.12),
        noise_4d(seed, 1.23, 4.56, 7.89, 0.12)
    );
}

#[test]
fn test_simplex_noise_struct_methods() {
    let noise_gen_2d = SimplexNoise::new_2d("struct-seed");
    let noise_gen_3d = SimplexNoise::new_3d("struct-seed");
    let noise_gen_4d = SimplexNoise::new_4d("struct-seed");

    let val2 = noise_gen_2d.noise_2d(0.4, 0.8);
    assert_eq!(val2, noise_gen_2d.noise2d(0.4, 0.8));
    assert_eq!(val2, noise2d("struct-seed", 0.4, 0.8));

    let val3 = noise_gen_3d.noise_3d(0.4, 0.8, 1.2);
    assert_eq!(val3, noise_gen_3d.noise3d(0.4, 0.8, 1.2));
    assert_eq!(val3, noise3d("struct-seed", 0.4, 0.8, 1.2));

    let val4 = noise_gen_4d.noise_4d(0.4, 0.8, 1.2, 1.6);
    assert_eq!(val4, noise_gen_4d.noise4d(0.4, 0.8, 1.2, 1.6));
    assert_eq!(val4, noise4d("struct-seed", 0.4, 0.8, 1.2, 1.6));
}

#[test]
fn test_noise_bounds_over_grid() {
    let seeds = ["grid-alpha", "grid-beta", "12345"];

    for &seed in &seeds {
        for xi in -10..=10 {
            for yi in -10..=10 {
                let x = xi as f64 * 0.35;
                let y = yi as f64 * 0.35;

                let v2 = noise2d(seed, x, y);
                assert!(
                    (-1.0..=1.0).contains(&v2),
                    "2D noise out of bounds: {} at ({}, {})",
                    v2,
                    x,
                    y
                );

                let v3 = noise3d(seed, x, y, (xi + yi) as f64 * 0.1);
                assert!(
                    (-1.0..=1.0).contains(&v3),
                    "3D noise out of bounds: {} at ({}, {})",
                    v3,
                    x,
                    y
                );

                let v4 = noise4d(seed, x, y, 0.5, 0.7);
                assert!(
                    (-1.0..=1.0).contains(&v4),
                    "4D noise out of bounds: {} at ({}, {})",
                    v4,
                    x,
                    y
                );
            }
        }
    }
}

#[test]
fn test_spatial_continuity_and_smoothness() {
    let seed = "continuity-seed";
    let eps = 1e-4;

    for i in 0..50 {
        let x = i as f64 * 0.2;
        let y = (i * 2) as f64 * 0.15;

        let v1 = noise2d(seed, x, y);
        let v2 = noise2d(seed, x + eps, y + eps);

        let delta = (v2 - v1).abs();
        assert!(
            delta < 0.05,
            "Simplex noise discontinuous: delta {} at ({}, {})",
            delta,
            x,
            y
        );
    }
}

#[test]
fn test_non_finite_inputs_graceful_handling() {
    assert_eq!(noise2d("nan-seed", f64::NAN, 1.0), 0.0);
    assert_eq!(noise2d("inf-seed", 1.0, f64::INFINITY), 0.0);
    assert_eq!(noise3d("nan-seed", 1.0, f64::NAN, 1.0), 0.0);
    assert_eq!(noise4d("nan-seed", 1.0, 1.0, 1.0, f64::NEG_INFINITY), 0.0);
}

#[test]
fn test_seed_conversions() {
    let _s1: NoiseSeed = "str".into();
    let _s2: NoiseSeed = String::from("string").into();
    let _s3: NoiseSeed = 42i32.into();
    let _s4: NoiseSeed = 100i64.into();
    let _s5: NoiseSeed = 200u32.into();
    let _s6: NoiseSeed = 300u64.into();
    let _s7: NoiseSeed = 400usize.into();
    let _s8: NoiseSeed = 12.34f64.into();
    let _s9: NoiseSeed = 56.78f32.into();

    let n_i64 = noise2d(100i64, 0.5, 0.5);
    let n_f64 = noise2d(100.0f64, 0.5, 0.5);
    assert_eq!(n_i64, n_f64);
}
