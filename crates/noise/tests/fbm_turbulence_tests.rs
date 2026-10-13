//! Integration tests for Fractional Brownian Motion (fBm) and turbulent flow domain warping.

use rust_motion_noise::{
    domain_warp_2d, fbm_2d, fbm_3d, turbulence_2d, turbulence_warp_2d, warp_points_2d, FbmOptions,
};

#[test]
fn test_fbm_octave_properties() {
    let seed = "fbm-test-seed";

    let opt1 = FbmOptions::new(1, 2.0, 0.5);
    let opt4 = FbmOptions::new(4, 2.0, 0.5);
    let opt8 = FbmOptions::new(8, 2.0, 0.5);

    let v1 = fbm_2d(seed, 1.5, 2.5, &opt1);
    let v4 = fbm_2d(seed, 1.5, 2.5, &opt4);
    let v8 = fbm_2d(seed, 1.5, 2.5, &opt8);

    assert!((-1.0..=1.0).contains(&v1));
    assert!((-1.0..=1.0).contains(&v4));
    assert!((-1.0..=1.0).contains(&v8));

    // Zero octaves returns 0.0
    let opt0 = FbmOptions::new(0, 2.0, 0.5);
    assert_eq!(fbm_2d(seed, 1.5, 2.5, &opt0), 0.0);
    assert_eq!(fbm_3d(seed, 1.5, 2.5, 3.5, &opt0), 0.0);
}

#[test]
fn test_fbm_3d_bounds_and_determinism() {
    let seed = "fbm-3d-test";
    let opts = FbmOptions::default();

    for i in 0..20 {
        let x = i as f64 * 0.4;
        let y = (i * 2) as f64 * 0.3;
        let z = (i * 3) as f64 * 0.2;

        let val1 = fbm_3d(seed, x, y, z, &opts);
        let val2 = fbm_3d(seed, x, y, z, &opts);

        assert_eq!(val1, val2);
        assert!((-1.0..=1.0).contains(&val1));
    }
}

#[test]
fn test_turbulence_2d_bounds() {
    let seed = "turb-test";
    for i in 0..20 {
        let x = i as f64 * 0.5;
        let y = (i * 2) as f64 * 0.5;

        let t = turbulence_2d(seed, x, y, 4);
        assert!((0.0..=1.0).contains(&t));
    }

    assert_eq!(turbulence_2d(seed, 1.0, 2.0, 0), 0.0);
}

#[test]
fn test_turbulence_warp_2d_deformation() {
    let seed = "warp-test";
    let (x, y) = (50.0, 100.0);

    // Zero strength returns original point
    let (wx0, wy0) = turbulence_warp_2d(seed, x, y, 0.0, 0.1);
    assert_eq!(wx0, x);
    assert_eq!(wy0, y);

    // Warping with strength
    let (wx, wy) = turbulence_warp_2d(seed, x, y, 10.0, 0.05);
    assert!((wx - x).abs() <= 10.0);
    assert!((wy - y).abs() <= 10.0);
    assert_ne!(wx, x);
    assert_ne!(wy, y);
}

#[test]
fn test_domain_warp_2d_field() {
    let seed = "domain-warp-field";
    let opts = FbmOptions::default();

    let val = domain_warp_2d(seed, 2.0, 3.0, 2.0, &opts);
    assert!((-1.0..=1.0).contains(&val));
}

#[test]
fn test_warp_points_2d_path_deformation() {
    let points = vec![
        (0.0, 0.0),
        (10.0, 0.0),
        (20.0, 5.0),
        (30.0, 15.0),
        (40.0, 25.0),
    ];

    let warped = warp_points_2d("path-seed", &points, 4.0, 0.02);
    assert_eq!(warped.len(), points.len());

    for (orig, warp) in points.iter().zip(warped.iter()) {
        assert!((warp.0 - orig.0).abs() <= 4.0);
        assert!((warp.1 - orig.1).abs() <= 4.0);
    }
}
