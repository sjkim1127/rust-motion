//! Optimization harness to find the theoretical global extrema of Simplex noise.

use rust_motion_noise::SimplexNoise;

#[test]
fn test_find_global_extrema_2d() {
    let mut global_max = -1.0f64;
    let mut global_min = 1.0f64;

    // Test 100 different seed permutations
    for seed_idx in 0..100 {
        let noise = SimplexNoise::new_2d(seed_idx as i64);

        // Sweep unit simplex cells with high resolution
        let steps = 40;
        for i in 0..=steps {
            for j in 0..=steps {
                let mut x = i as f64 / steps as f64;
                let mut y = j as f64 / steps as f64;

                // Gradient ascent / descent local optimization (15 iterations)
                let lr = 0.01;
                let h = 1e-5;
                for _ in 0..15 {
                    let v = noise.noise_2d(x, y);
                    let vx = noise.noise_2d(x + h, y);
                    let vy = noise.noise_2d(x, y + h);
                    let gx = (vx - v) / h;
                    let gy = (vy - v) / h;

                    x += gx * lr;
                    y += gy * lr;

                    let v_opt = noise.noise_2d(x, y);
                    if v_opt > global_max {
                        global_max = v_opt;
                    }
                    if v_opt < global_min {
                        global_min = v_opt;
                    }
                }
            }
        }
    }

    println!(
        "2D Simplex Global Extrema: min={}, max={}",
        global_min, global_max
    );
    assert!(
        global_max <= 1.0,
        "2D Simplex maximum {} exceeded 1.0!",
        global_max
    );
    assert!(
        global_min >= -1.0,
        "2D Simplex minimum {} went below -1.0!",
        global_min
    );
}

#[test]
fn test_find_global_extrema_3d() {
    let mut global_max = -1.0f64;
    let mut global_min = 1.0f64;

    for seed_idx in 0..50 {
        let noise = SimplexNoise::new_3d(seed_idx as i64);

        let steps = 15;
        for i in 0..=steps {
            for j in 0..=steps {
                for k in 0..=steps {
                    let mut x = i as f64 / steps as f64;
                    let mut y = j as f64 / steps as f64;
                    let mut z = k as f64 / steps as f64;

                    let lr = 0.01;
                    let h = 1e-5;
                    for _ in 0..10 {
                        let v = noise.noise_3d(x, y, z);
                        let vx = noise.noise_3d(x + h, y, z);
                        let vy = noise.noise_3d(x, y + h, z);
                        let vz = noise.noise_3d(x, y, z + h);
                        let gx = (vx - v) / h;
                        let gy = (vy - v) / h;
                        let gz = (vz - v) / h;

                        x += gx * lr;
                        y += gy * lr;
                        z += gz * lr;

                        let v_opt = noise.noise_3d(x, y, z);
                        if v_opt > global_max {
                            global_max = v_opt;
                        }
                        if v_opt < global_min {
                            global_min = v_opt;
                        }
                    }
                }
            }
        }
    }

    println!(
        "3D Simplex Global Extrema: min={}, max={}",
        global_min, global_max
    );
    assert!(
        global_max <= 1.0,
        "3D Simplex maximum {} exceeded 1.0!",
        global_max
    );
    assert!(
        global_min >= -1.0,
        "3D Simplex minimum {} went below -1.0!",
        global_min
    );
}

#[test]
fn test_find_global_extrema_4d() {
    let mut global_max = -1.0f64;
    let mut global_min = 1.0f64;

    for seed_idx in 0..30 {
        let noise = SimplexNoise::new_4d(seed_idx as i64);

        let steps = 8;
        for i in 0..=steps {
            for j in 0..=steps {
                for k in 0..=steps {
                    for l in 0..=steps {
                        let mut x = i as f64 / steps as f64;
                        let mut y = j as f64 / steps as f64;
                        let mut z = k as f64 / steps as f64;
                        let mut w = l as f64 / steps as f64;

                        let lr = 0.01;
                        let h = 1e-5;
                        for _ in 0..8 {
                            let v = noise.noise_4d(x, y, z, w);
                            let vx = noise.noise_4d(x + h, y, z, w);
                            let vy = noise.noise_4d(x, y + h, z, w);
                            let vz = noise.noise_4d(x, y, z + h, w);
                            let vw = noise.noise_4d(x, y, z, w + h);
                            let gx = (vx - v) / h;
                            let gy = (vy - v) / h;
                            let gz = (vz - v) / h;
                            let gw = (vw - v) / h;

                            x += gx * lr;
                            y += gy * lr;
                            z += gz * lr;
                            w += gw * lr;

                            let v_opt = noise.noise_4d(x, y, z, w);
                            if v_opt > global_max {
                                global_max = v_opt;
                            }
                            if v_opt < global_min {
                                global_min = v_opt;
                            }
                        }
                    }
                }
            }
        }
    }

    println!(
        "4D Simplex Global Extrema: min={}, max={}",
        global_min, global_max
    );
    assert!(
        global_max <= 1.0,
        "4D Simplex maximum {} exceeded 1.0!",
        global_max
    );
    assert!(
        global_min >= -1.0,
        "4D Simplex minimum {} went below -1.0!",
        global_min
    );
}
