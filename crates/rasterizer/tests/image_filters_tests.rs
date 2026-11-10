use rust_motion_rasterizer::backend::{FrameConfig, RasterizerBackend};
use rust_motion_rasterizer::scene::{BlendMode, Color, Scene, SceneFilter, SceneNode};
use rust_motion_rasterizer::tiny_skia_backend::TinySkiaBackend;
use rust_motion_rasterizer::algo::FilterParams;

#[test]
fn test_photocraft_filter_serde_roundtrip() {
    let filters = vec![
        SceneFilter::BoxBlur { radius: 5.0 },
        SceneFilter::RadialBlur {
            amount: 25.0,
            spin: true,
            center_x: 0.5,
            center_y: 0.5,
        },
        SceneFilter::SurfaceBlur {
            radius: 8.0,
            threshold: 15.0,
        },
        SceneFilter::UnsharpMask {
            amount: 150.0,
            radius: 2.0,
            threshold: 3.0,
        },
        SceneFilter::SmartSharpen {
            amount: 100.0,
            radius: 1.5,
            reduce_noise: 10.0,
        },
        SceneFilter::HighPass { radius: 10.0 },
        SceneFilter::Twirl { angle: 45.0 },
        SceneFilter::Spherize { amount: 50.0 },
        SceneFilter::Pinch { amount: -30.0 },
        SceneFilter::Ripple {
            amount: 100.0,
            size: 15.0,
        },
    ];

    for filter in &filters {
        let json = serde_json::to_string(filter).expect("serialize filter");
        let deserialized: SceneFilter = serde_json::from_str(&json).expect("deserialize filter");
        assert_eq!(filter, &deserialized);
    }
}

fn create_test_layer(filter: SceneFilter) -> Scene {
    let mut scene = Scene::new();
    // Inner rectangle
    let inner_rect = SceneNode::Rect {
        x: 20.0,
        y: 20.0,
        w: 60.0,
        h: 60.0,
        fill: Color::rgb(255, 100, 50),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    };
    // Background canvas
    let bg_rect = SceneNode::Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
        fill: Color::rgb(10, 20, 40),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    };

    let layer = SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: rust_motion_rasterizer::scene::MaskMode::Alpha,
        filters: vec![filter],
        shadow: None,
        children: vec![bg_rect, inner_rect],
    };

    scene.push(layer);
    scene
}

#[test]
fn test_box_blur_filter_rendering() {
    let backend = TinySkiaBackend::new();
    let config = FrameConfig::new(100, 100, 0, 30.0);
    let scene = create_test_layer(SceneFilter::BoxBlur { radius: 4.0 });

    let img = backend.render_frame(&scene, &config).expect("render box blur");
    assert_eq!(img.width(), 100);
    assert_eq!(img.height(), 100);

    // Pixel at edge of inner rectangle (x=20, y=50) should be smoothed/blurred
    let edge_px = img.get_pixel(20, 50);
    assert!(edge_px[0] > 10 && edge_px[0] < 255);
}

#[test]
fn test_surface_blur_filter_rendering() {
    let backend = TinySkiaBackend::new();
    let config = FrameConfig::new(100, 100, 0, 30.0);
    let scene = create_test_layer(SceneFilter::SurfaceBlur {
        radius: 5.0,
        threshold: 20.0,
    });

    let img = backend.render_frame(&scene, &config).expect("render surface blur");
    assert_eq!(img.width(), 100);
    assert_eq!(img.height(), 100);
}

#[test]
fn test_unsharp_mask_and_high_pass_rendering() {
    let backend = TinySkiaBackend::new();
    let config = FrameConfig::new(100, 100, 0, 30.0);

    // Unsharp mask
    let scene_unsharp = create_test_layer(SceneFilter::UnsharpMask {
        amount: 100.0,
        radius: 2.0,
        threshold: 2.0,
    });
    let img_unsharp = backend
        .render_frame(&scene_unsharp, &config)
        .expect("render unsharp mask");
    assert_eq!(img_unsharp.width(), 100);

    // High pass
    let scene_highpass = create_test_layer(SceneFilter::HighPass { radius: 5.0 });
    let img_highpass = backend
        .render_frame(&scene_highpass, &config)
        .expect("render high pass");
    assert_eq!(img_highpass.width(), 100);
}

#[test]
fn test_distortions_twirl_spherize_pinch_ripple() {
    let backend = TinySkiaBackend::new();
    let config = FrameConfig::new(100, 100, 0, 30.0);

    // Twirl
    let scene_twirl = create_test_layer(SceneFilter::Twirl { angle: 90.0 });
    let img_twirl = backend.render_frame(&scene_twirl, &config).expect("render twirl");
    assert_eq!(img_twirl.width(), 100);

    // Spherize
    let scene_spherize = create_test_layer(SceneFilter::Spherize { amount: 50.0 });
    let img_spherize = backend
        .render_frame(&scene_spherize, &config)
        .expect("render spherize");
    assert_eq!(img_spherize.width(), 100);

    // Pinch
    let scene_pinch = create_test_layer(SceneFilter::Pinch { amount: -40.0 });
    let img_pinch = backend.render_frame(&scene_pinch, &config).expect("render pinch");
    assert_eq!(img_pinch.width(), 100);

    // Ripple
    let scene_ripple = create_test_layer(SceneFilter::Ripple {
        amount: 80.0,
        size: 15.0,
    });
    let img_ripple = backend.render_frame(&scene_ripple, &config).expect("render ripple");
    assert_eq!(img_ripple.width(), 100);
}

#[test]
fn test_photofilter_direct_algorithm_execution() {
    let backend = TinySkiaBackend::new();
    let config = FrameConfig::new(100, 100, 0, 30.0);

    // Direct PhotoCraft algo execution: Median filter
    let scene = create_test_layer(SceneFilter::PhotoFilter(Box::new(
        FilterParams::Median { radius: 3.0 },
    )));
    let img = backend
        .render_frame(&scene, &config)
        .expect("render photo filter median");
    assert_eq!(img.width(), 100);
}
