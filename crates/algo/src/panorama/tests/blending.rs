//! Tests for photometric compensation, seams and multi-band blending.

use super::*;
use crate::transform::Homography;

#[test]
fn photometric_recovers_gain_and_vignette() {
    // Two half-overlapping views of a flat-ish scene: the second one 30 % brighter.
    let (w, h) = (200usize, 100usize);
    let base =
        |x: i32, y: i32| 0.3 + 0.2 * ((x as f32 * 0.05).sin() * (y as f32 * 0.07).cos()).abs();
    let mut imgs = Vec::new();
    let mut pls = Vec::new();
    for (k, (x0, g)) in [(0, 1.0f32), (100, 1.3f32)].into_iter().enumerate() {
        let mut px = Vec::new();
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                px.push(base(x + x0, y) * g);
            }
        }
        imgs.push(RoiImage {
            x0,
            y0: 0,
            w,
            h,
            ch: 1,
            px,
            alpha: vec![1.0; w * h],
        });
        pls.push(Placement {
            projection: Projection::Plane,
            focal: 200.0,
            center: [100.0, 50.0],
            k1: 0.0,
            h: Homography([1.0, 0.0, 100.0 + x0 as f64, 0.0, 1.0, 50.0, 0.0, 0.0, 1.0]),
        });
        let _ = k;
    }
    let ph = photometric(&imgs, Some((&pls, &[(w, h), (w, h)])), true, false);
    let ratio = ph.gains[1][0] / ph.gains[0][0];
    assert!((ratio - 1.3).abs() < 0.03, "{ph:?}");
    // With vignetting on, the flat scene (no falloff) gives a near-zero model.
    let pv = photometric(&imgs, Some((&pls, &[(w, h), (w, h)])), true, true);
    assert!(pv.vignette[0].abs() < 0.1, "{pv:?}");
}

#[test]
fn seams_avoid_disagreement_and_multiband_is_seamless() {
    // Two images over a 120×40 strip, overlapping in 40..80; the second has a bright
    // "moving object" at x 50..58 that the first doesn't.
    let (w, h) = (120usize, 40usize);
    let mut luma = vec![vec![0.5f32; w * h], vec![0.5f32; w * h]];
    let cover: Vec<Vec<bool>> = vec![
        (0..w * h).map(|i| i % w < 80).collect(),
        (0..w * h).map(|i| i % w >= 40).collect(),
    ];
    for y in 0..h {
        for x in 50..58 {
            luma[1][y * w + x] = 1.0;
        }
    }
    let labels = seam_labels(w, h, &luma, &cover, &[0, 1]);
    // The object column belongs to image 0 (the seam passes to its right), or entirely to 1.
    for y in 0..h {
        let owner: Vec<i32> = (50..58).map(|x| labels[y * w + x]).collect();
        assert!(
            owner.iter().all(|o| *o == owner[0]),
            "row {y} split the object: {owner:?}"
        );
    }
    assert_eq!(labels[5], 0);
    assert_eq!(labels[w - 1], 1);
    // Multi-band blend of two flat images at different levels joins smoothly.
    let mk = |x0: i32, v: f32| RoiImage {
        x0,
        y0: 0,
        w: 80,
        h,
        ch: 1,
        px: vec![v; 80 * h],
        alpha: vec![1.0; 80 * h],
    };
    let imgs = [mk(0, 0.3), mk(40, 0.7)];
    let wts: Vec<Vec<f32>> = (0..2)
        .map(|k| {
            (0..80 * h)
                .map(|i| {
                    let x = (i % 80) as i32 + imgs[k].x0;
                    if (x < 60) == (k == 0) {
                        1.0
                    } else {
                        0.0
                    }
                })
                .collect()
        })
        .collect();
    let (out, cov) = multiband(w, h, &imgs, &wts, 4);
    let row: Vec<f32> = (0..w).map(|x| out[20 * w + x]).collect();
    assert!(
        (row[2] - 0.3).abs() < 0.02 && (row[w - 3] - 0.7).abs() < 0.02,
        "{row:?}"
    );
    let max_jump = row
        .windows(2)
        .map(|p| (p[1] - p[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(max_jump < 0.1, "max step {max_jump}");
    assert!(cov.iter().all(|c| *c == 1.0));
}
