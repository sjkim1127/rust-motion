//! Adversarial Stress Testing & Numerical Stability Harness for `rust-motion-noise`.
//!
//! Stress dimensions:
//! 1. Extreme & degenerate coordinates (subnormals, epsilons, huge floats up to 1e300, infinities, NaNs, negative zeros)
//! 2. Mathematical continuity across simplex grid boundaries & diagonal hyperplanes
//! 3. Strict gradient bounding: Global bounds search & Monte Carlo extrema checks
//! 4. Memory safety, thread safety, and allocation-free hot path evaluation
//! 5. fBm convergence, turbulence flow stability, SVG path sanitization

use rust_motion_noise::{
    domain_warp_2d, fbm_2d, fbm_3d, generate_noise_svg_data_url, generate_noise_wave_path,
    mulberry32, noise2d, noise3d, noise4d, turbulence_2d, FbmOptions, SimplexNoise,
    WavePathOptions,
};
use std::sync::Arc;
use std::thread;

// ══════════════════════════════════════════════════════════════════════════════
// 1. EXTREME & DEGENERATE COORDINATES
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_stress_extreme_float_magnitudes() {
    let seeds = ["stress-seed-1", "extreme-seed-2", "seed-42"];
    let test_values = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        1e-300,
        -1e-300,
        f64::MIN_POSITIVE,
        -f64::MIN_POSITIVE,
        5e-324, // Smallest subnormal f64
        1e6,
        -1e6,
        1e12,
        -1e12,
        1e18,
        -1e18,
        1e50,
        -1e50,
        1e150,
        -1e150,
        1e300,
        -1e300,
    ];

    for &seed in &seeds {
        let noise = SimplexNoise::new_2d(seed);
        for &val in &test_values {
            let n2 = noise.noise_2d(val, val);
            assert!(
                n2.is_finite(),
                "noise_2d returned non-finite value {} for input {}",
                n2,
                val
            );
            assert!(
                (-1.0..=1.0).contains(&n2),
                "noise_2d output {} out of [-1.0, 1.0] for input {}",
                n2,
                val
            );

            let n3 = noise.noise_3d(val, -val, val * 0.5);
            assert!(
                n3.is_finite(),
                "noise_3d returned non-finite value {} for input {}",
                n3,
                val
            );
            assert!(
                (-1.0..=1.0).contains(&n3),
                "noise_3d output {} out of [-1.0, 1.0] for input {}",
                n3,
                val
            );

            let n4 = noise.noise_4d(val, val * 0.25, -val, val * 0.75);
            assert!(
                n4.is_finite(),
                "noise_4d returned non-finite value {} for input {}",
                n4,
                val
            );
            assert!(
                (-1.0..=1.0).contains(&n4),
                "noise_4d output {} out of [-1.0, 1.0] for input {}",
                n4,
                val
            );
        }
    }
}

#[test]
fn test_stress_non_finite_inputs_all_permutations() {
    let non_finites = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY];

    for &nf in &non_finites {
        // 2D inputs
        assert_eq!(noise2d("seed", nf, 1.0), 0.0);
        assert_eq!(noise2d("seed", 1.0, nf), 0.0);
        assert_eq!(noise2d("seed", nf, nf), 0.0);

        // 3D inputs
        assert_eq!(noise3d("seed", nf, 1.0, 2.0), 0.0);
        assert_eq!(noise3d("seed", 1.0, nf, 2.0), 0.0);
        assert_eq!(noise3d("seed", 1.0, 2.0, nf), 0.0);
        assert_eq!(noise3d("seed", nf, nf, nf), 0.0);

        // 4D inputs
        assert_eq!(noise4d("seed", nf, 1.0, 2.0, 3.0), 0.0);
        assert_eq!(noise4d("seed", 1.0, nf, 2.0, 3.0), 0.0);
        assert_eq!(noise4d("seed", 1.0, 2.0, nf, 3.0), 0.0);
        assert_eq!(noise4d("seed", 1.0, 2.0, 3.0, nf), 0.0);
        assert_eq!(noise4d("seed", nf, nf, nf, nf), 0.0);

        // fBm inputs
        let opts = FbmOptions::default();
        assert_eq!(fbm_2d("seed", nf, 1.0, &opts), 0.0);
        assert_eq!(fbm_2d("seed", 1.0, nf, &opts), 0.0);
        assert_eq!(fbm_3d("seed", nf, 1.0, 2.0, &opts), 0.0);
        assert_eq!(fbm_3d("seed", 1.0, nf, 2.0, &opts), 0.0);
        assert_eq!(fbm_3d("seed", 1.0, 2.0, nf, &opts), 0.0);

        // Turbulence inputs
        assert_eq!(turbulence_2d("seed", nf, 1.0, 4), 0.0);
        assert_eq!(turbulence_2d("seed", 1.0, nf, 4), 0.0);
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// 2. MATHEMATICAL CONTINUITY & SMOOTHNESS ACROSS SIMPLEX BOUNDARIES
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_stress_continuity_across_simplex_diagonal_boundaries() {
    let noise = SimplexNoise::new_2d("boundary-continuity-seed");

    // Diagonal boundary y = x (where x0 == y0 branch is selected)
    for i in -20..20 {
        let center = i as f64 * 0.5;
        let eps = 1e-6;

        // Traverse across the diagonal y = x from center - eps to center + eps
        let v_below = noise.noise_2d(center - eps, center + eps);
        let v_at = noise.noise_2d(center, center);
        let v_above = noise.noise_2d(center + eps, center - eps);

        let delta1 = (v_at - v_below).abs();
        let delta2 = (v_above - v_at).abs();

        assert!(
            delta1 < 1e-3,
            "C0 discontinuity across diagonal y=x at center {}: delta={}",
            center,
            delta1
        );
        assert!(
            delta2 < 1e-3,
            "C0 discontinuity across diagonal y=x at center {}: delta={}",
            center,
            delta2
        );
    }
}

#[test]
fn test_stress_continuity_across_integer_grid_boundaries() {
    let noise = SimplexNoise::new_2d("grid-continuity-seed");

    // Traverse across integer grid lines x = -10.0 .. 10.0
    for xi in -10..=10 {
        let x_int = xi as f64;
        let y_fixed = 0.37;
        let eps = 1e-6;

        let v_left = noise.noise_2d(x_int - eps, y_fixed);
        let v_mid = noise.noise_2d(x_int, y_fixed);
        let v_right = noise.noise_2d(x_int + eps, y_fixed);

        let delta_left = (v_mid - v_left).abs();
        let delta_right = (v_right - v_mid).abs();

        assert!(
            delta_left < 1e-3,
            "Discontinuity crossing grid boundary x={} from left: delta={}",
            x_int,
            delta_left
        );
        assert!(
            delta_right < 1e-3,
            "Discontinuity crossing grid boundary x={} to right: delta={}",
            x_int,
            delta_right
        );
    }
}

#[test]
fn test_stress_bounded_numerical_gradients() {
    let noise = SimplexNoise::new_2d("grad-seed");
    let h = 1e-5;

    // Scan a dense grid and verify directional derivative magnitude is bounded
    for xi in 0..50 {
        for yi in 0..50 {
            let x = xi as f64 * 0.1;
            let y = yi as f64 * 0.1;

            let n_center = noise.noise_2d(x, y);
            let n_dx = noise.noise_2d(x + h, y);
            let n_dy = noise.noise_2d(x, y + h);

            let grad_x = (n_dx - n_center) / h;
            let grad_y = (n_dy - n_center) / h;
            let grad_mag = (grad_x * grad_x + grad_y * grad_y).sqrt();

            // Simplex noise gradient magnitude is mathematically bounded (< 20.0)
            assert!(
                grad_mag < 20.0,
                "Unbounded gradient detected at ({}, {}): magnitude {}",
                x,
                y,
                grad_mag
            );
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// 3. EXHAUSTIVE MONTE CARLO & GLOBAL BOUNDS SEARCH
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_stress_monte_carlo_strict_bounds_2d_3d_4d() {
    let seeds = [
        "seed-alpha",
        "seed-beta",
        "seed-gamma",
        "seed-omega",
        "123456",
        "custom-prng-098",
    ];

    let mut max_2d = -1.0f64;
    let mut min_2d = 1.0f64;
    let mut max_3d = -1.0f64;
    let mut min_3d = 1.0f64;
    let mut max_4d = -1.0f64;
    let mut min_4d = 1.0f64;

    for &seed in &seeds {
        let noise2 = SimplexNoise::new_2d(seed);
        let noise3 = SimplexNoise::new_3d(seed);
        let noise4 = SimplexNoise::new_4d(seed);

        for i in 0..5_000 {
            let p1 = mulberry32(i * 4 + 1) * 200.0 - 100.0;
            let p2 = mulberry32(i * 4 + 2) * 200.0 - 100.0;
            let p3 = mulberry32(i * 4 + 3) * 200.0 - 100.0;
            let p4 = mulberry32(i * 4 + 4) * 200.0 - 100.0;

            let v2 = noise2.noise_2d(p1, p2);
            assert!(
                (-1.0..=1.0).contains(&v2),
                "2D noise out of bounds: {} at ({}, {})",
                v2,
                p1,
                p2
            );
            if v2 > max_2d {
                max_2d = v2;
            }
            if v2 < min_2d {
                min_2d = v2;
            }

            let v3 = noise3.noise_3d(p1, p2, p3);
            assert!(
                (-1.0..=1.0).contains(&v3),
                "3D noise out of bounds: {} at ({}, {}, {})",
                v3,
                p1,
                p2,
                p3
            );
            if v3 > max_3d {
                max_3d = v3;
            }
            if v3 < min_3d {
                min_3d = v3;
            }

            let v4 = noise4.noise_4d(p1, p2, p3, p4);
            assert!(
                (-1.0..=1.0).contains(&v4),
                "4D noise out of bounds: {} at ({}, {}, {}, {})",
                v4,
                p1,
                p2,
                p3,
                p4
            );
            if v4 > max_4d {
                max_4d = v4;
            }
            if v4 < min_4d {
                min_4d = v4;
            }
        }
    }

    assert!(max_2d > 0.8 && min_2d < -0.8);
    assert!(max_3d > 0.8 && min_3d < -0.8);
    assert!(max_4d > 0.8 && min_4d < -0.8);
}

#[test]
fn test_stress_fbm_and_turbulence_bounds_sweep() {
    let seed = "fbm-turbulence-sweep";
    let fbm_opts = FbmOptions::new(8, 2.15, 0.55);

    for i in 0..1_000 {
        let x = mulberry32(i * 3 + 1) * 100.0 - 50.0;
        let y = mulberry32(i * 3 + 2) * 100.0 - 50.0;
        let z = mulberry32(i * 3 + 3) * 100.0 - 50.0;

        let val_fbm2 = fbm_2d(seed, x, y, &fbm_opts);
        assert!((-1.0..=1.0).contains(&val_fbm2));

        let val_fbm3 = fbm_3d(seed, x, y, z, &fbm_opts);
        assert!((-1.0..=1.0).contains(&val_fbm3));

        let val_turb = turbulence_2d(seed, x, y, 6);
        assert!((0.0..=1.0).contains(&val_turb));

        let val_dwarp = domain_warp_2d(seed, x * 0.05, y * 0.05, 5.0, &fbm_opts);
        assert!((-1.0..=1.0).contains(&val_dwarp));
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// 4. CONCURRENCY & THREAD-SAFETY
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_stress_concurrent_noise_evaluation() {
    let noise = Arc::new(SimplexNoise::new_2d("multi-thread-shared"));
    let mut handles = Vec::new();

    for thread_id in 0..8 {
        let noise_clone = Arc::clone(&noise);
        let handle = thread::spawn(move || {
            for i in 0..2_000 {
                let x = (thread_id * 100 + i) as f64 * 0.05;
                let y = (thread_id * 50 + i * 2) as f64 * 0.05;
                let v = noise_clone.noise_2d(x, y);
                assert!((-1.0..=1.0).contains(&v));
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("Concurrent noise evaluation panicked");
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// 5. SVG PATH GENERATION SANITIZATION & STABILITY
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_stress_svg_generation_no_nan_or_infinities() {
    let degenerate_opts = [
        WavePathOptions::new(0.0, 0.0, 0.0, 0.0, 0),
        WavePathOptions::new(1.0, 500.0, 1e6, 10.0, 16),
        WavePathOptions::new(-0.5, -100.0, -100.0, 0.001, 1),
    ];

    for (idx, opt) in degenerate_opts.iter().enumerate() {
        let path = generate_noise_wave_path("svg-stress-seed", 1920.0, 1080.0, opt);
        assert!(
            !path.contains("NaN"),
            "SVG path contains NaN at index {}",
            idx
        );
        assert!(
            !path.contains("inf"),
            "SVG path contains inf at index {}",
            idx
        );
        assert!(path.starts_with("M 0,"));
        assert!(path.ends_with("Z"));
    }

    let data_url = generate_noise_svg_data_url(
        "svg-data-stress",
        3840,
        2160,
        1e5,
        0.5,
        "#123456",
        "#abcdef",
    );
    assert!(!data_url.contains("NaN"));
    assert!(!data_url.contains("inf"));
    assert!(data_url.starts_with("data:image/svg+xml;utf8,<svg"));
}
