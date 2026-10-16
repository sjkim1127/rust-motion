use rust_motion_rasterizer::{
    BlendMode, Color, FrameConfig, RasterizerBackend, Scene, SceneFilter, SceneNode,
    TinySkiaBackend,
};

fn frame_cfg(w: u32, h: u32) -> FrameConfig {
    FrameConfig {
        width: w,
        height: h,
        frame: 0,
        fps: 30.0,
    }
}

fn render_single_layer(w: u32, h: u32, fill: Color, filters: Vec<SceneFilter>) -> image::RgbaImage {
    let backend = TinySkiaBackend::headless();
    let mut scene = Scene::new();
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: rust_motion_rasterizer::MaskMode::Alpha,
        filters,
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: w as f32,
            h: h as f32,
            fill,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });
    backend.render_frame(&scene, &frame_cfg(w, h)).unwrap()
}

#[test]
fn test_chromatic_aberration_channel_separation() {
    let backend = TinySkiaBackend::headless();
    let mut scene = Scene::new();
    // A vertical white bar in the center of a black canvas
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: rust_motion_rasterizer::MaskMode::Alpha,
        filters: vec![SceneFilter::ChromaticAberration {
            offset_x: 4.0,
            offset_y: 0.0,
            angle_rad: 0.0,
        }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 20.0,
            y: 0.0,
            w: 20.0,
            h: 40.0,
            fill: Color::WHITE,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let img = backend.render_frame(&scene, &frame_cfg(60, 40)).unwrap();

    // Blue is shifted left (sampled from x + 4, so at x = 16..20 Blue should appear)
    let left_pixel = img.get_pixel(18, 20);
    assert!(left_pixel[2] > 0, "Blue channel should appear at left edge");
    assert_eq!(left_pixel[1], 0, "Green channel should not be shifted");

    // Red is shifted right (sampled from x - 4, so at x = 40..44 Red should appear)
    let right_pixel = img.get_pixel(42, 20);
    assert_eq!(right_pixel[1], 0, "Green channel should not be shifted");
    assert!(
        right_pixel[0] > 0,
        "Red channel should appear at right edge"
    );
}

#[test]
fn test_vignette_radial_falloff_and_center_preservation() {
    let img = render_single_layer(
        100,
        100,
        Color::WHITE,
        vec![SceneFilter::Vignette {
            offset: 0.2,
            darkness: 0.8,
            roundness: 1.0,
        }],
    );

    // Center pixel (50, 50) is inside offset threshold -> full white
    let center = img.get_pixel(50, 50);
    assert_eq!(center[0], 255);
    assert_eq!(center[1], 255);
    assert_eq!(center[2], 255);

    // Corner pixel (0, 0) should be darkened significantly
    let corner = img.get_pixel(0, 0);
    assert!(corner[0] < 150, "Corner should be darkened by vignette");
    assert!(corner[1] < 150);
    assert!(corner[2] < 150);
}

#[test]
fn test_vignette_chebyshev_box_falloff() {
    let img = render_single_layer(
        100,
        100,
        Color::WHITE,
        vec![SceneFilter::Vignette {
            offset: 0.5,
            darkness: 1.0,
            roundness: 0.0, // rectangular falloff
        }],
    );

    let center = img.get_pixel(50, 50);
    assert_eq!(center[0], 255);

    let corner = img.get_pixel(0, 0);
    assert!(corner[0] < 50, "Corner should be dark with roundness=0.0");
}

#[test]
fn test_contrast_adjustment() {
    // Midpoint gray: (128, 128, 128)
    let img_mid = render_single_layer(
        10,
        10,
        Color::rgb(128, 128, 128),
        vec![SceneFilter::Contrast { factor: 2.0 }],
    );
    let px_mid = img_mid.get_pixel(5, 5);
    // 128 with contrast factor 2.0 should remain 128: (128 - 128) * 2 + 128 = 128
    assert!((px_mid[0] as i32 - 128).abs() <= 2);

    // Dark gray: (64, 64, 64) with factor 2.0 -> (64 - 128) * 2 + 128 = 0
    let img_dark = render_single_layer(
        10,
        10,
        Color::rgb(64, 64, 64),
        vec![SceneFilter::Contrast { factor: 2.0 }],
    );
    let px_dark = img_dark.get_pixel(5, 5);
    assert_eq!(px_dark[0], 0);

    // Light gray: (192, 192, 192) with factor 2.0 -> (192 - 128) * 2 + 128 = 256 -> clamped to 255
    let img_light = render_single_layer(
        10,
        10,
        Color::rgb(192, 192, 192),
        vec![SceneFilter::Contrast { factor: 2.0 }],
    );
    let px_light = img_light.get_pixel(5, 5);
    assert_eq!(px_light[0], 255);
}

#[test]
fn test_saturation_adjustment() {
    // Pure red (255, 0, 0) desaturated (factor 0.0) -> Rec.601 luma = 0.299 * 255 = 76.2
    let img_mono = render_single_layer(
        10,
        10,
        Color::rgb(255, 0, 0),
        vec![SceneFilter::Saturation { factor: 0.0 }],
    );
    let px_mono = img_mono.get_pixel(5, 5);
    assert!((px_mono[0] as i32 - 76).abs() <= 2);
    assert!((px_mono[1] as i32 - 76).abs() <= 2);
    assert!((px_mono[2] as i32 - 76).abs() <= 2);

    // Identity saturation
    let img_ident = render_single_layer(
        10,
        10,
        Color::rgb(200, 100, 50),
        vec![SceneFilter::Saturation { factor: 1.0 }],
    );
    let px_ident = img_ident.get_pixel(5, 5);
    assert_eq!(px_ident[0], 200);
    assert_eq!(px_ident[1], 100);
    assert_eq!(px_ident[2], 50);
}

#[test]
fn test_hue_rotate_rotation() {
    // Red (255, 0, 0) hue rotated by 120 degrees should become Green (0, 255, 0)
    let img_green = render_single_layer(
        10,
        10,
        Color::rgb(255, 0, 0),
        vec![SceneFilter::HueRotate { degrees: 120.0 }],
    );
    let px_green = img_green.get_pixel(5, 5);
    assert_eq!(px_green[0], 0);
    assert_eq!(px_green[1], 255);
    assert_eq!(px_green[2], 0);

    // 240 degrees should become Blue (0, 0, 255)
    let img_blue = render_single_layer(
        10,
        10,
        Color::rgb(255, 0, 0),
        vec![SceneFilter::HueRotate { degrees: 240.0 }],
    );
    let px_blue = img_blue.get_pixel(5, 5);
    assert_eq!(px_blue[0], 0);
    assert_eq!(px_blue[1], 0);
    assert_eq!(px_blue[2], 255);

    // 360 degrees should return to Red
    let img_red = render_single_layer(
        10,
        10,
        Color::rgb(255, 0, 0),
        vec![SceneFilter::HueRotate { degrees: 360.0 }],
    );
    let px_red = img_red.get_pixel(5, 5);
    assert_eq!(px_red[0], 255);
    assert_eq!(px_red[1], 0);
    assert_eq!(px_red[2], 0);
}

#[test]
fn test_invert_filter() {
    let img_full = render_single_layer(
        10,
        10,
        Color::rgb(200, 50, 100),
        vec![SceneFilter::Invert { amount: 1.0 }],
    );
    let px_full = img_full.get_pixel(5, 5);
    assert_eq!(px_full[0], 55);
    assert_eq!(px_full[1], 205);
    assert_eq!(px_full[2], 155);

    let img_half = render_single_layer(
        10,
        10,
        Color::rgb(255, 0, 0),
        vec![SceneFilter::Invert { amount: 0.5 }],
    );
    let px_half = img_half.get_pixel(5, 5);
    assert!((px_half[0] as i32 - 128).abs() <= 2);
}

#[test]
fn test_tint_filter() {
    let img = render_single_layer(
        10,
        10,
        Color::rgb(100, 100, 100),
        vec![SceneFilter::Tint {
            color: [255, 0, 0, 255],
            amount: 0.5,
        }],
    );
    let px = img.get_pixel(5, 5);
    // 50% blend between 100 and 255 for R -> ~177
    assert!((px[0] as i32 - 177).abs() <= 2);
    // 50% blend between 100 and 0 for G and B -> ~50
    assert!((px[1] as i32 - 50).abs() <= 2);
    assert!((px[2] as i32 - 50).abs() <= 2);
}

#[test]
fn test_duotone_filter() {
    // Black (0, 0, 0) should map to primary color
    let img_dark = render_single_layer(
        10,
        10,
        Color::rgb(0, 0, 0),
        vec![SceneFilter::Duotone {
            primary: [10, 20, 30, 255],
            secondary: [200, 210, 220, 255],
        }],
    );
    let px_dark = img_dark.get_pixel(5, 5);
    assert_eq!(px_dark[0], 10);
    assert_eq!(px_dark[1], 20);
    assert_eq!(px_dark[2], 30);

    // White (255, 255, 255) should map to secondary color
    let img_light = render_single_layer(
        10,
        10,
        Color::rgb(255, 255, 255),
        vec![SceneFilter::Duotone {
            primary: [10, 20, 30, 255],
            secondary: [200, 210, 220, 255],
        }],
    );
    let px_light = img_light.get_pixel(5, 5);
    assert_eq!(px_light[0], 200);
    assert_eq!(px_light[1], 210);
    assert_eq!(px_light[2], 220);
}

#[test]
fn test_color_grading_filter() {
    let img = render_single_layer(
        10,
        10,
        Color::rgb(100, 150, 200),
        vec![SceneFilter::ColorGrading {
            contrast: 1.2,
            saturation: 1.1,
            gamma: 1.0,
            tint: Some([255, 200, 150, 50]),
        }],
    );
    let px = img.get_pixel(5, 5);
    assert!(px[3] == 255);
}

#[test]
fn test_color_key_green_screen_removal() {
    // Pure green screen pixel (0, 255, 0, 255)
    let img_green = render_single_layer(
        10,
        10,
        Color::rgb(0, 255, 0),
        vec![SceneFilter::ColorKey {
            key_color: [0, 255, 0, 255],
            similarity: 0.3,
            smoothness: 0.1,
            spill_suppression: 0.5,
        }],
    );
    let px_green = img_green.get_pixel(5, 5);
    // Green screen key should remove green and make pixel transparent
    assert_eq!(px_green[3], 0, "Key color pixel should be transparent");

    // Blue object pixel (0, 0, 255, 255)
    let img_blue = render_single_layer(
        10,
        10,
        Color::rgb(0, 0, 255),
        vec![SceneFilter::ColorKey {
            key_color: [0, 255, 0, 255],
            similarity: 0.3,
            smoothness: 0.1,
            spill_suppression: 0.5,
        }],
    );
    let px_blue = img_blue.get_pixel(5, 5);
    // Blue pixel should remain fully opaque and blue
    assert_eq!(px_blue[3], 255);
    assert_eq!(px_blue[2], 255);
}

#[test]
fn test_invalid_filter_parameters_rejected() {
    let backend = TinySkiaBackend::headless();

    // Negative contrast factor
    let mut scene = Scene::new();
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: rust_motion_rasterizer::MaskMode::Alpha,
        filters: vec![SceneFilter::Contrast { factor: -1.0 }],
        shadow: None,
        children: vec![],
    });
    assert!(backend.render_frame(&scene, &frame_cfg(10, 10)).is_err());

    // NaN gamma in ColorGrading
    let mut scene2 = Scene::new();
    scene2.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: rust_motion_rasterizer::MaskMode::Alpha,
        filters: vec![SceneFilter::ColorGrading {
            contrast: 1.0,
            saturation: 1.0,
            gamma: f32::NAN,
            tint: None,
        }],
        shadow: None,
        children: vec![],
    });
    assert!(backend.render_frame(&scene2, &frame_cfg(10, 10)).is_err());
}

#[test]
fn test_camera_motion_blur_shutter_angle_and_phase() {
    let backend = TinySkiaBackend::headless();
    let mut scene = Scene::new();
    // A vertical white block in the center of 64x64 canvas
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: rust_motion_rasterizer::MaskMode::Alpha,
        filters: vec![SceneFilter::CameraMotionBlur {
            shutter_angle: 180.0,
            samples: 16,
            shutter_phase: -90.0,
            angle_deg: Some(0.0),
            kernel: rust_motion_rasterizer::TemporalFilterKernel::Gaussian,
        }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 28.0,
            y: 20.0,
            w: 8.0,
            h: 24.0,
            fill: Color::WHITE,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let img = backend.render_frame(&scene, &frame_cfg(64, 64)).unwrap();
    // Center pixel should be white
    let center = img.get_pixel(32, 32);
    assert!(center[3] > 100);

    // Trail should spread horizontally to left and right
    let left = img.get_pixel(26, 32);
    assert!(left[3] > 0, "motion blur should produce horizontal trail");
    let right = img.get_pixel(38, 32);
    assert!(right[3] > 0, "motion blur should produce horizontal trail");
}

#[test]
fn test_directional_motion_blur_diagonal() {
    let backend = TinySkiaBackend::headless();
    let mut scene = Scene::new();
    // 8x8 white square at (28, 28)
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: rust_motion_rasterizer::MaskMode::Alpha,
        filters: vec![SceneFilter::DirectionalMotionBlur {
            angle_deg: 45.0, // 45° diagonal
            length: 16.0,
            samples: 16,
            kernel: rust_motion_rasterizer::TemporalFilterKernel::Triangle,
        }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 28.0,
            y: 28.0,
            w: 8.0,
            h: 8.0,
            fill: Color::WHITE,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let img = backend.render_frame(&scene, &frame_cfg(64, 64)).unwrap();
    // Diagonal blur spreads along dx=dy: pixel (38, 38) should receive blur
    let diag = img.get_pixel(38, 38);
    assert!(diag[3] > 0, "diagonal blur should spread along x=y");
}

