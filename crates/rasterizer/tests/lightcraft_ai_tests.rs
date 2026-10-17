//! Comprehensive tests for LightCraft AI Segmentation, Inpainting, and Video Matting (#101).

use rust_motion_rasterizer::ai_vision::{
    FaceDetector, Inpainter, TemporalDenoiser, VideoMatting,
};
use rust_motion_rasterizer::backend::{FrameConfig, RasterizerBackend};
use rust_motion_rasterizer::scene::{Color, Scene, SceneFilter, SceneNode};
use rust_motion_rasterizer::TinySkiaBackend;
use tiny_skia::{Color as SkiaColor, Pixmap, PremultipliedColorU8};

#[test]
fn test_face_detection_and_tracking() {
    let mut pixmap = Pixmap::new(200, 200).unwrap();
    // Fill background with blue
    pixmap.fill(SkiaColor::from_rgba8(20, 40, 100, 255));

    // Draw a face-like skin tone block in the center: (x: 70..130, y: 60..140)
    for y in 60..140 {
        for x in 70..130 {
            pixmap.pixels_mut()[y * 200 + x] =
                PremultipliedColorU8::from_rgba(215, 160, 130, 255).unwrap();
        }
    }

    let mut detector = FaceDetector::new();
    let detected_frame1 = detector.detect_faces(&pixmap, 0.4);
    assert!(!detected_frame1.is_empty(), "face detected");
    let tracked_frame1 = detector.track_faces(detected_frame1);
    let face_id = tracked_frame1[0].id;
    assert!(face_id > 0);

    // Frame 2: slightly shifted face
    let mut pixmap2 = Pixmap::new(200, 200).unwrap();
    pixmap2.fill(SkiaColor::from_rgba8(20, 40, 100, 255));
    for y in 62..142 {
        for x in 72..132 {
            pixmap2.pixels_mut()[y * 200 + x] =
                PremultipliedColorU8::from_rgba(215, 160, 130, 255).unwrap();
        }
    }
    let detected_frame2 = detector.detect_faces(&pixmap2, 0.4);
    assert!(!detected_frame2.is_empty());
    let tracked_frame2 = detector.track_faces(detected_frame2);
    // Face ID must persist across frames due to IoU temporal association
    assert_eq!(tracked_frame2[0].id, face_id, "persistent face ID across frames");
}

#[test]
fn test_video_matting_and_temporal_smoothing() {
    let mut pixmap = Pixmap::new(100, 100).unwrap();
    // Greenish background
    pixmap.fill(SkiaColor::from_rgba8(30, 180, 50, 255));

    // Foreground subject in center (bright red)
    for y in 30..70 {
        for x in 30..70 {
            pixmap.pixels_mut()[y * 100 + x] =
                PremultipliedColorU8::from_rgba(230, 40, 40, 255).unwrap();
        }
    }

    let matte = VideoMatting::extract_matte(&pixmap, 0.4);
    assert_eq!(matte.len(), 100 * 100);

    // Center should have high opacity
    let center_alpha = matte[50 * 100 + 50];
    let bg_alpha = matte[5 * 100 + 5];
    assert!(center_alpha > 200, "foreground has high alpha");
    assert_eq!(bg_alpha, 0, "background has zero alpha");

    // Test temporal smoothing across frames
    let prev_matte = vec![255u8; 100 * 100];
    let smoothed = VideoMatting::temporal_smooth(&matte, Some(&prev_matte), 0.5);
    // Background pixel smoothed with previous frame
    assert!(smoothed[5 * 100 + 5] > 0);

    // Apply matte to pixmap
    VideoMatting::apply_matte(&mut pixmap, &matte);
    let bg_px = pixmap.pixel(5, 5).unwrap();
    assert_eq!(bg_px.alpha(), 0, "background cleared by matte");
}

#[test]
fn test_content_aware_inpainting() {
    let mut pixmap = Pixmap::new(64, 64).unwrap();
    // Fill with pattern
    for y in 0..64 {
        for x in 0..64 {
            let v = ((x / 8 + y / 8) % 2) * 200 + 30;
            pixmap.pixels_mut()[y * 64 + x] =
                PremultipliedColorU8::from_rgba(v as u8, v as u8, v as u8, 255).unwrap();
        }
    }

    // Mask a small 8x8 hole in the center
    let mut hole_mask = vec![false; 64 * 64];
    for y in 28..36 {
        for x in 28..36 {
            hole_mask[y * 64 + x] = true;
            pixmap.pixels_mut()[y * 64 + x] = PremultipliedColorU8::TRANSPARENT;
        }
    }

    let success = Inpainter::inpaint(&mut pixmap, &hole_mask, 2);
    assert!(success, "inpainting succeeded");

    // Check that hole pixels have been reconstructed
    let inpainted_px = pixmap.pixel(32, 32).unwrap();
    assert_eq!(inpainted_px.alpha(), 255, "hole is filled");
    assert!(inpainted_px.red() > 0, "pixel filled from surrounding texture");
}

#[test]
fn test_temporal_denoiser() {
    let mut frame1 = Pixmap::new(50, 50).unwrap();
    frame1.fill(SkiaColor::from_rgba8(100, 100, 100, 255));

    let mut frame2 = Pixmap::new(50, 50).unwrap();
    // Add small random noise (+/- 10)
    for (i, px) in frame2.pixels_mut().iter_mut().enumerate() {
        let noise = (i % 7) as i32 - 3;
        let v = (100 + noise) as u8;
        *px = PremultipliedColorU8::from_rgba(v, v, v, 255).unwrap();
    }

    TemporalDenoiser::denoise(&mut frame2, Some(&frame1), 0.5, 15.0);

    // Noise should be attenuated towards 100
    for px in frame2.pixels() {
        let diff = (px.red() as i32 - 100).abs();
        assert!(diff <= 2, "temporal denoising reduced noise variance");
    }
}

#[test]
fn test_rasterizer_inpaint_and_matte_filters() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: rust_motion_rasterizer::scene::BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: rust_motion_rasterizer::scene::MaskMode::Alpha,
        filters: vec![
            SceneFilter::Inpaint {
                hole_rect: [0.45, 0.45, 0.1, 0.1],
                patch_radius: 2,
            },
        ],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            fill: Color::rgb(150, 150, 150),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = FrameConfig::new(100, 100, 0, 30.0);
    let img = backend.render_frame(&scene, &config).expect("render frame");
    let px = img.get_pixel(50, 50);
    assert_eq!(px[3], 255, "inpainted region is fully opaque");
}
