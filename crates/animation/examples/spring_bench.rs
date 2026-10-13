//! Shared workload with benchmarks/spring-remotion.mjs; build in release mode.
use rust_motion_animation::{measure_spring, spring_with_options, SpringConfig, SpringOptions};
use std::{hint::black_box, time::Instant};

fn sample(index: u32, scenario: &str) -> f64 {
    let config = if scenario == "varied" {
        SpringConfig {
            damping: 6.0 + (index % 32) as f64 * 0.25,
            mass: 1.0 + (index % 4) as f64 * 0.1,
            stiffness: 80.0 + (index % 8) as f64 * 10.0,
            ..Default::default()
        }
    } else {
        SpringConfig::default()
    };
    let (frame, options) = match scenario {
        "varied" => (
            (index % 120) as f64,
            SpringOptions {
                duration_in_frames: Some(90.0),
                ..Default::default()
            },
        ),
        "repeat" => (60.0, SpringOptions::default()),
        "sequential" | "cold" => ((index % 300) as f64, SpringOptions::default()),
        "duration" => (
            (index % 300) as f64,
            SpringOptions {
                duration_in_frames: Some(60.0),
                delay: 12.0,
                ..Default::default()
            },
        ),
        "seek" => (((index * 137) % 600) as f64 + 0.5, SpringOptions::default()),
        "measure" => return measure_spring(30.0, &config, 0.005).unwrap(),
        _ => panic!("unknown scenario"),
    };
    spring_with_options(black_box(frame), 30.0, config, options).unwrap()
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let scenario = &args[1];
    let iterations: u32 = args[2].parse().unwrap();
    let warmup: u32 = args[3].parse().unwrap();
    for index in 0..warmup {
        black_box(sample(index, scenario));
    }
    let start = Instant::now();
    let mut checksum = 0.0;
    for index in 0..iterations {
        checksum += black_box(sample(index, scenario));
    }
    let elapsed = start.elapsed().as_secs_f64() * 1e9;
    println!(
        "{{\"ns_per_call\":{},\"checksum\":{checksum}}}",
        elapsed / iterations as f64
    );
}
