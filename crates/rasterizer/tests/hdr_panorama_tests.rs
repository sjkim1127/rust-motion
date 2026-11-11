//! Comprehensive integration tests for Multi-Exposure HDR Radiance Merge & Multi-Band Laplacian Panorama Stitching (#108).

use rust_motion_rasterizer::hdr_merge::{
    compute_mtb_shift, merge_brackets, ExposureBracket, HdrMergeOptions,
};
use rust_motion_rasterizer::panorama::{
    build_laplacian_pyramid, collapse_laplacian_pyramid, project_image, stitch_sequence,
    stitch_two, FloatImage, PanoramaOptions, ProjectionType,
};
use tiny_skia::{Color, Pixmap, PremultipliedColorU8};

#[test]
fn test_hdr_radiance_merge_bracketed_exposures() {
    let width = 64;
    let height = 64;

    let mut b_under = Pixmap::new(width, height).unwrap();
    let mut b_norm = Pixmap::new(width, height).unwrap();
    let mut b_over = Pixmap::new(width, height).unwrap();

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            let scene_rad = (x as f32 / width as f32) * 4.0;

            let px_under = ((scene_rad * 0.25).min(1.0) * 255.0) as u8;
            let px_norm = ((scene_rad * 1.0).min(1.0) * 255.0) as u8;
            let px_over = ((scene_rad * 4.0).min(1.0) * 255.0) as u8;

            b_under.pixels_mut()[idx] =
                PremultipliedColorU8::from_rgba(px_under, px_under, px_under, 255).unwrap();
            b_norm.pixels_mut()[idx] =
                PremultipliedColorU8::from_rgba(px_norm, px_norm, px_norm, 255).unwrap();
            b_over.pixels_mut()[idx] =
                PremultipliedColorU8::from_rgba(px_over, px_over, px_over, 255).unwrap();
        }
    }

    let brackets = vec![
        ExposureBracket {
            pixmap: b_under,
            ev: -2.0,
        },
        ExposureBracket {
            pixmap: b_norm,
            ev: 0.0,
        },
        ExposureBracket {
            pixmap: b_over,
            ev: 2.0,
        },
    ];

    let options = HdrMergeOptions {
        reference_idx: 1,
        deghost_sensitivity: 0.5,
        saturation_clip: 0.95,
        auto_align: false,
    };

    let radiance = merge_brackets(&brackets, &options).expect("HDR radiance merge failed");
    assert_eq!(radiance.width, width);
    assert_eq!(radiance.height, height);
    assert!(radiance.log_average_luminance() > 0.05);

    // Radiance at rightmost column (high highlight recovered from -2EV bracket) should exceed 1.5
    let right_rad = radiance.data[(width - 1) as usize];
    assert!(
        right_rad[0] > 1.5,
        "Highlight radiance recovered from -2EV should exceed 1.5: {:?}",
        right_rad
    );

    let reinhard = radiance.tonemap("reinhard", 0.0);
    assert_eq!(reinhard.width(), width);
    assert_eq!(reinhard.height(), height);

    let aces = radiance.tonemap("aces", 0.0);
    assert_eq!(aces.width(), width);
    assert_eq!(aces.height(), height);

    let rightmost_px = aces.pixel(width - 1, height / 2).unwrap();
    assert!(
        rightmost_px.red() > 200,
        "ACES tonemapped highlight: {}",
        rightmost_px.red()
    );
}

#[test]
fn test_hdr_mtb_auto_alignment() {
    let w = 48;
    let h = 48;
    let mut ref_pm = Pixmap::new(w, h).unwrap();
    let mut shifted_pm = Pixmap::new(w, h).unwrap();

    // Draw an exposure-scaled pattern shifted by (+3, -2) pixels in target
    let true_dx = 3i32;
    let true_dy = -2i32;
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let pat_ref = if ((x / 8) + (y / 8)) % 2 == 0 {
                160u8
            } else {
                40u8
            };
            ref_pm.pixels_mut()[(y * w as i32 + x) as usize] =
                PremultipliedColorU8::from_rgba(pat_ref, pat_ref, pat_ref, 255).unwrap();

            let sx = x - true_dx;
            let sy = y - true_dy;
            // Brighter exposure (+1 EV) with same spatial pattern shifted by (true_dx, true_dy)
            let pat_tgt = if ((sx.div_euclid(8)) + (sy.div_euclid(8))) % 2 == 0 {
                240u8
            } else {
                80u8
            };
            shifted_pm.pixels_mut()[(y * w as i32 + x) as usize] =
                PremultipliedColorU8::from_rgba(pat_tgt, pat_tgt, pat_tgt, 255).unwrap();
        }
    }

    let (dx, dy) = compute_mtb_shift(&ref_pm, &shifted_pm, 6);
    assert_eq!((dx, dy), (true_dx, true_dy));
}

#[test]
fn test_hdr_deghosting_moving_object() {
    let width = 32;
    let height = 32;

    let mut ref_pixmap = Pixmap::new(width, height).unwrap();
    ref_pixmap.fill(Color::from_rgba8(110, 110, 110, 255));

    let mut ghost_pixmap = Pixmap::new(width, height).unwrap();
    ghost_pixmap.fill(Color::from_rgba8(220, 220, 220, 255));
    for y in 10..22 {
        for x in 10..22 {
            let idx = (y * width + x) as usize;
            ghost_pixmap.pixels_mut()[idx] = PremultipliedColorU8::from_rgba(8, 8, 8, 255).unwrap();
        }
    }

    let brackets = vec![
        ExposureBracket {
            pixmap: ref_pixmap,
            ev: 0.0,
        },
        ExposureBracket {
            pixmap: ghost_pixmap,
            ev: 1.0,
        },
    ];

    let options = HdrMergeOptions {
        reference_idx: 0,
        deghost_sensitivity: 0.9,
        saturation_clip: 0.98,
        auto_align: false,
    };

    let radiance = merge_brackets(&brackets, &options).expect("deghost merge");
    let center_rad = radiance.data[(16 * width + 16) as usize];
    assert!(
        center_rad[0] > 0.35,
        "Deghosted radiance should match reference (~0.43): {:?}",
        center_rad
    );
}

#[test]
fn test_laplacian_pyramid_exact_reconstruction() {
    let w = 64;
    let h = 48;
    let mut pm = Pixmap::new(w, h).unwrap();
    for y in 0..h {
        for x in 0..w {
            let r = ((x * 4) % 256) as u8;
            let g = ((y * 5) % 256) as u8;
            let b = (((x + y) * 3) % 256) as u8;
            pm.pixels_mut()[(y * w + x) as usize] =
                PremultipliedColorU8::from_rgba(r, g, b, 255).unwrap();
        }
    }

    let orig = FloatImage::from_pixmap(&pm);
    let lap_pyr = build_laplacian_pyramid(&orig, 4);
    assert_eq!(lap_pyr.len(), 5);

    let reconstructed = collapse_laplacian_pyramid(&lap_pyr);
    assert_eq!(reconstructed.width, w);
    assert_eq!(reconstructed.height, h);

    for (a, b) in orig.pixels.iter().zip(reconstructed.pixels.iter()) {
        for c in 0..4 {
            assert!(
                (a[c] - b[c]).abs() < 1e-5,
                "Laplacian pyramid reconstruction mismatch: {} vs {}",
                a[c],
                b[c]
            );
        }
    }
}

#[test]
fn test_panorama_projections() {
    let mut img = Pixmap::new(100, 100).unwrap();
    img.fill(Color::from_rgba8(200, 150, 100, 255));

    let f = 500.0;
    let cylindrical = project_image(&img, ProjectionType::Cylindrical, f);
    assert_eq!(cylindrical.width(), 100);
    assert_eq!(cylindrical.height(), 100);

    let spherical = project_image(&img, ProjectionType::Spherical, f);
    assert_eq!(spherical.width(), 100);
    assert_eq!(spherical.height(), 100);

    let perspective = project_image(&img, ProjectionType::Perspective, f);
    assert_eq!(perspective.width(), 100);
    assert_eq!(perspective.height(), 100);
}

#[test]
fn test_panorama_stitching_multi_band_blend() {
    let w = 80;
    let h = 60;

    let mut left = Pixmap::new(w, h).unwrap();
    left.fill(Color::from_rgba8(220, 40, 40, 255)); // Red

    let mut right = Pixmap::new(w, h).unwrap();
    right.fill(Color::from_rgba8(40, 40, 220, 255)); // Blue

    let options = PanoramaOptions {
        projection: ProjectionType::Perspective,
        overlap: 0.25, // 20px overlap
        blend_bands: 4,
        focal_length_px: 1000.0,
        auto_align: false,
    };

    let stitched = stitch_two(&left, &right, &options).expect("stitch two images");
    let expected_w = w + w - 20; // 140
    assert_eq!(stitched.width(), expected_w);
    assert_eq!(stitched.height(), h);

    // Far left should be predominantly red
    let left_px = stitched.pixel(5, 30).unwrap();
    assert!(left_px.red() > 200);
    assert!(left_px.blue() < 55);

    // Far right should be predominantly blue
    let right_px = stitched.pixel(expected_w - 5, 30).unwrap();
    assert!(right_px.blue() > 200);
    assert!(right_px.red() < 55);

    // Overlap center should be smoothly blended purple (red + blue)
    let seam_center_x = 60 + 10; // overlap is 60..80, center is 70
    let mid_px = stitched.pixel(seam_center_x, 30).unwrap();
    assert!(
        mid_px.red() > 80 && mid_px.red() < 180,
        "Mid red: {}",
        mid_px.red()
    );
    assert!(
        mid_px.blue() > 80 && mid_px.blue() < 180,
        "Mid blue: {}",
        mid_px.blue()
    );

    // Test sequence stitching (3 images)
    let mut mid = Pixmap::new(w, h).unwrap();
    mid.fill(Color::from_rgba8(40, 220, 40, 255)); // Green

    let seq = vec![left, mid, right];
    let seq_stitched = stitch_sequence(&seq, &options).expect("stitch sequence");
    assert!(seq_stitched.width() > w * 2);
}
