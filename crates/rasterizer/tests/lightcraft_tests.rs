//! Comprehensive tests for LightCraft RAW development, 3D LUTs, Lens Calibration DB, and Film Emulation (#100).

use rust_motion_rasterizer::backend::{FrameConfig, RasterizerBackend};
use rust_motion_rasterizer::lut3d::Lut3d;
use rust_motion_rasterizer::scene::{Color, Scene, SceneFilter, SceneNode};
use rust_motion_rasterizer::TinySkiaBackend;
use tiny_skia::Pixmap;

#[test]
fn test_lensdb_synthetic_profile_matching_and_correction() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<lensdatabase version="2">
  <mount><name>Sony E</name></mount>
  <camera>
    <maker>Sony</maker>
    <model>ILCE-7M4</model>
    <mount>Sony E</mount>
    <cropfactor>1.0</cropfactor>
  </camera>
  <lens>
    <maker>Sony</maker>
    <model>FE 24-70mm F2.8 GM II</model>
    <mount>Sony E</mount>
    <cropfactor>1.0</cropfactor>
    <calibration>
      <distortion model="poly5" focal="24" k1="-0.03" k2="0.002" />
      <distortion model="poly3" focal="70" k1="0.01" />
      <tca model="linear" focal="24" kr="1.0003" kb="0.9996" />
      <vignetting model="pa" focal="24" aperture="2.8" distance="10" k1="-0.4" k2="0.1" k3="-0.02" />
    </calibration>
  </lens>
</lensdatabase>"#;

    let db = lightcraft_lensdb::LensDb::parse(xml).expect("parse lens database");
    assert_eq!(db.cameras.len(), 1);
    assert_eq!(db.lenses.len(), 1);

    let cam = db.find_camera("Sony", "ILCE-7M4").expect("find camera");
    assert_eq!(cam.crop_factor, 1.0);

    let lens = db
        .find_lens(Some("Sony"), "FE 24-70mm F2.8 GM II", Some(cam))
        .expect("find lens");
    assert_eq!(lens.maker, "Sony");

    let shot = lightcraft_lensdb::Shot {
        focal: 24.0,
        aperture: Some(2.8),
        distance: Some(10.0),
        crop_factor: Some(1.0),
        width: 3840.0,
        height: 2160.0,
    };

    let correction = lens.correction(&shot).expect("compute lens correction");
    assert!(correction.warp.is_some(), "distortion warp computed");
    assert!(correction.vignette.is_some(), "vignette polynomial computed");

    let warp = correction.warp.unwrap();
    assert_eq!(warp.center.x, 0.5);
    assert_eq!(warp.center.y, 0.5);
    assert!(warp.radius > 0.0);
}

#[test]
fn test_cube_lut_parser_and_tetrahedral_interpolation() {
    let cube_str = r#"TITLE "Test Neutral"
LUT_3D_SIZE 2
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
"#;

    let lut = Lut3d::parse_cube(cube_str).expect("parse 2x2x2 identity cube");
    assert_eq!(lut.size, 2);
    assert_eq!(lut.title, Some("Test Neutral".into()));

    // Trilinear identity
    let mid_trilinear = lut.apply_trilinear([0.5, 0.5, 0.5]);
    assert!((mid_trilinear[0] - 0.5).abs() < 1e-4);
    assert!((mid_trilinear[1] - 0.5).abs() < 1e-4);
    assert!((mid_trilinear[2] - 0.5).abs() < 1e-4);

    // Tetrahedral identity
    let mid_tetrahedral = lut.apply_tetrahedral([0.5, 0.5, 0.5]);
    assert!((mid_tetrahedral[0] - 0.5).abs() < 1e-4);
    assert!((mid_tetrahedral[1] - 0.5).abs() < 1e-4);
    assert!((mid_tetrahedral[2] - 0.5).abs() < 1e-4);
}

#[test]
fn test_3dl_lut_parser_and_application() {
    let d3l_str = r#"# 3DL Format
3DMESH
Mesh 2 2 2
0 4095
0 0 0
4095 0 0
0 4095 0
4095 4095 0
0 0 4095
4095 0 4095
0 4095 4095
4095 4095 4095
"#;

    let lut = Lut3d::parse_3dl(d3l_str).expect("parse 3dl format");
    assert_eq!(lut.size, 2);
    let sample = lut.apply([1.0, 1.0, 1.0], true);
    assert!((sample[0] - 1.0).abs() < 1e-3);
    assert!((sample[1] - 1.0).abs() < 1e-3);
    assert!((sample[2] - 1.0).abs() < 1e-3);
}

#[test]
fn test_builtin_cinema_profiles() {
    let portra = Lut3d::kodak_portra_400();
    assert_eq!(portra.size, 17);
    let sample_skin = portra.apply([0.7, 0.5, 0.4], true);
    // Portra enhances warm tones
    assert!(sample_skin[0] > 0.0);

    let provia = Lut3d::fujifilm_provia_100();
    assert_eq!(provia.size, 17);

    let teal_orange = Lut3d::cinematic_teal_orange();
    assert_eq!(teal_orange.size, 17);
}

#[test]
fn test_rasterizer_lut3d_filter_rendering() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    let cube_str = r#"TITLE "Tint Red"
LUT_3D_SIZE 2
0.5 0.0 0.0
1.0 0.0 0.0
0.5 0.0 0.0
1.0 0.0 0.0
0.5 0.0 0.0
1.0 0.0 0.0
0.5 0.0 0.0
1.0 0.0 0.0
"#;

    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: rust_motion_rasterizer::scene::BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: rust_motion_rasterizer::scene::MaskMode::Alpha,
        filters: vec![SceneFilter::Lut3D {
            cube_content: cube_str.to_string(),
            amount: 1.0,
            tetrahedral: true,
        }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            fill: Color::rgb(100, 100, 100),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = FrameConfig::new(100, 100, 0, 30.0);
    let img = backend.render_frame(&scene, &config).expect("render frame");
    let px = img.get_pixel(50, 50);
    // Red channel boosted by LUT, green/blue zeroed
    assert!(px[0] > 100);
    assert_eq!(px[1], 0);
    assert_eq!(px[2], 0);
}

#[test]
fn test_rasterizer_film_emulation_and_halation() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: rust_motion_rasterizer::scene::BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: rust_motion_rasterizer::scene::MaskMode::Alpha,
        filters: vec![SceneFilter::FilmEmulation {
            grain_amount: 50.0,
            grain_size: 25.0,
            grain_roughness: 50.0,
            grain_seed: 42,
            halation_intensity: 1.0,
            halation_threshold: 0.5,
            halation_radius: 10.0,
            halation_color: [255, 60, 20, 255],
        }],
        shadow: None,
        children: vec![
            // Dark background
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
                fill: Color::rgb(20, 20, 20),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            // Bright highlight in center (should trigger halation)
            SceneNode::Rect {
                x: 40.0,
                y: 40.0,
                w: 20.0,
                h: 20.0,
                fill: Color::WHITE,
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
        ],
    });

    let config = FrameConfig::new(100, 100, 0, 30.0);
    let img = backend.render_frame(&scene, &config).expect("render frame");

    // Halation warm glow should be visible around the bright box (e.g. at (35, 50))
    let halo_px = img.get_pixel(35, 50);
    assert!(halo_px[0] > halo_px[2], "warm halation red channel exceeds blue");
}

#[test]
fn test_rasterizer_lens_correction_filter() {
    let mut pix = Pixmap::new(100, 100).unwrap();
    pix.fill(tiny_skia::Color::from_rgba8(200, 200, 200, 255));

    // Apply distortion and vignette falloff
    rust_motion_rasterizer::lens_correction::apply_lens_correction(
        &mut pix,
        [-0.05, 0.01, 0.0, 0.0],
        [-0.3, 0.05, 0.0],
        [0.001, -0.001],
        [0.5, 0.5],
    );

    // Center should be brighter than edge due to vignette falloff
    let center_px = pix.pixel(50, 50).unwrap();
    let corner_px = pix.pixel(5, 5).unwrap();
    assert!(corner_px.red() < center_px.red(), "vignette darkens outer edge");
}
