//! Performance throughput and zero-allocation hot path verification.

use rust_motion_noise::{FbmOptions, SimplexNoise};
use std::time::Instant;

#[test]
fn test_simplex_2d_throughput() {
    let noise = SimplexNoise::new_2d(12345);
    let iterations = 1_000_000;

    let start = Instant::now();
    let mut sum = 0.0;
    for i in 0..iterations {
        let x = (i as f64) * 0.001;
        let y = (i as f64) * 0.002;
        sum += noise.noise_2d(x, y);
    }
    let elapsed = start.elapsed();
    let ops_per_sec = (iterations as f64) / elapsed.as_secs_f64();

    println!(
        "2D Simplex: {} evaluations in {:.4}s -> {:.2} M ops/sec (sum={:.2})",
        iterations,
        elapsed.as_secs_f64(),
        ops_per_sec / 1e6,
        sum
    );
    assert!(
        ops_per_sec > 500_000.0,
        "Throughput too low: {} ops/sec",
        ops_per_sec
    );
}

#[test]
fn test_simplex_3d_throughput() {
    let noise = SimplexNoise::new_3d(12345);
    let iterations = 1_000_000;

    let start = Instant::now();
    let mut sum = 0.0;
    for i in 0..iterations {
        let x = (i as f64) * 0.001;
        let y = (i as f64) * 0.002;
        let z = (i as f64) * 0.003;
        sum += noise.noise_3d(x, y, z);
    }
    let elapsed = start.elapsed();
    let ops_per_sec = (iterations as f64) / elapsed.as_secs_f64();

    println!(
        "3D Simplex: {} evaluations in {:.4}s -> {:.2} M ops/sec (sum={:.2})",
        iterations,
        elapsed.as_secs_f64(),
        ops_per_sec / 1e6,
        sum
    );
    assert!(
        ops_per_sec > 500_000.0,
        "Throughput too low: {} ops/sec",
        ops_per_sec
    );
}

#[test]
fn test_simplex_4d_throughput() {
    let noise = SimplexNoise::new_4d(12345);
    let iterations = 1_000_000;

    let start = Instant::now();
    let mut sum = 0.0;
    for i in 0..iterations {
        let x = (i as f64) * 0.001;
        let y = (i as f64) * 0.002;
        let z = (i as f64) * 0.003;
        let w = (i as f64) * 0.004;
        sum += noise.noise_4d(x, y, z, w);
    }
    let elapsed = start.elapsed();
    let ops_per_sec = (iterations as f64) / elapsed.as_secs_f64();

    println!(
        "4D Simplex: {} evaluations in {:.4}s -> {:.2} M ops/sec (sum={:.2})",
        iterations,
        elapsed.as_secs_f64(),
        ops_per_sec / 1e6,
        sum
    );
    assert!(
        ops_per_sec > 300_000.0,
        "Throughput too low: {} ops/sec",
        ops_per_sec
    );
}

#[test]
fn test_fbm_2d_throughput() {
    let opts = FbmOptions::new(4, 2.0, 0.5);
    let noise = SimplexNoise::new_2d(12345);
    let iterations = 200_000;

    let start = Instant::now();
    let mut sum = 0.0;
    for i in 0..iterations {
        let x = (i as f64) * 0.001;
        let y = (i as f64) * 0.002;
        sum += rust_motion_noise::fbm_2d_with_noise(&noise, x, y, &opts);
    }
    let elapsed = start.elapsed();
    let ops_per_sec = (iterations as f64) / elapsed.as_secs_f64();

    println!(
        "4-Octave fBm 2D (reused noise): {} evaluations in {:.4}s -> {:.2} M ops/sec (sum={:.2})",
        iterations,
        elapsed.as_secs_f64(),
        ops_per_sec / 1e6,
        sum
    );
    assert!(
        ops_per_sec > 200_000.0,
        "Throughput too low: {} ops/sec",
        ops_per_sec
    );
}
