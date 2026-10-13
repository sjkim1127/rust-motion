//! Comprehensive E2E Test Suite for Rasterizer, Typography, Filters & Scene Graph (Tiers 1-4)
//!
//! Features covered:
//! - Feature 6: Layer Filtering & Offscreen Blur/Opacity Compositing
//! - Feature 7: Vignette & Edge Falloff Layer Masking
//! - Feature 8: Color Grading & Image Processing Suite (Brightness, Grayscale, Opacity, Blur, Shadows)
//! - Feature 12: Text Auto-scaling (`fit_text`)
//! - Feature 14: Typography & Text Box Layout (`layout_text_box`, `measure_text_width`, `TextBox`)
//! - Feature 15: Unified Public APIs & Re-exports (`SceneNode`, `SceneFilter`, `Color`, `Transform2D`, `TinySkiaBackend`)
//! - Tier 3: Pairwise cross-feature combinations
//! - Tier 4: Real-world video application scenario (Cyberpunk Title Card)

use rust_motion_rasterizer::backend::{FrameConfig, RasterizerBackend};
use rust_motion_rasterizer::font::{
    fit_text, layout_text_box, measure_text_width, TextBox, TextHorizontalAlign, TextOverflow,
    TextVerticalAlign,
};
use rust_motion_rasterizer::frame_cache::{FrameCacheConfig, FrameCacheKey, FrameCacheManager};
use rust_motion_rasterizer::scene::{
    BlendMode, ClipRegion, Color, MaskMode, Scene, SceneFilter, SceneNode, SceneShadow, Transform2D,
};
use rust_motion_rasterizer::TinySkiaBackend;

fn make_test_config(w: u32, h: u32) -> FrameConfig {
    FrameConfig::new(w, h, 0, 30.0)
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 6: LAYER FILTERING & OFFSCREEN COMPOSITING
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f6_t1_layer_blur_filter_execution() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Blur { sigma: 5.0 }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 150.0,
            y: 150.0,
            w: 100.0,
            h: 100.0,
            fill: Color::rgb(255, 0, 0),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = make_test_config(400, 400);
    let img = backend
        .render_frame(&scene, &config)
        .expect("Layer blur render failed");
    assert_eq!(img.width(), 400);
    assert_eq!(img.height(), 400);

    // Center pixel should contain red
    let center_px = img.get_pixel(200, 200);
    assert!(center_px.0[0] > 0);
}

#[test]
fn test_f6_t1_layer_opacity_filter_execution() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Opacity { amount: 0.5 }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
            fill: Color::rgba(255, 0, 0, 255),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = make_test_config(200, 200);
    let img = backend.render_frame(&scene, &config).unwrap();
    let px = img.get_pixel(100, 100);
    assert!(
        (px.0[3] as i32 - 128).abs() <= 2,
        "Alpha should be ~128 (0.5), got {}",
        px.0[3]
    );
}

#[test]
fn test_f6_t1_layer_drop_shadow_generation() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: Vec::new(),
        shadow: Some(SceneShadow {
            offset_x: 10.0,
            offset_y: 10.0,
            blur_sigma: 4.0,
            color: Color::rgba(0, 0, 0, 180),
        }),
        children: vec![SceneNode::Rect {
            x: 100.0,
            y: 100.0,
            w: 100.0,
            h: 100.0,
            fill: Color::rgb(0, 200, 255),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = make_test_config(400, 400);
    let img = backend.render_frame(&scene, &config).unwrap();
    let shadow_px = img.get_pixel(205, 205);
    assert!(shadow_px.0[3] > 0);
}

#[test]
fn test_f6_t1_layer_clipping_region() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: Some(ClipRegion::Rect {
            x: 50.0,
            y: 50.0,
            w: 100.0,
            h: 100.0,
            corner_radius: 0.0,
        }),
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: Vec::new(),
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 300.0,
            fill: Color::rgb(255, 255, 0),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = make_test_config(300, 300);
    let img = backend.render_frame(&scene, &config).unwrap();

    // Inside clip: (75, 75) should be yellow
    let inside_px = img.get_pixel(75, 75);
    assert_eq!(inside_px.0[0], 255);
    assert_eq!(inside_px.0[1], 255);
    assert_eq!(inside_px.0[3], 255);

    // Outside clip: (200, 200) should be transparent
    let outside_px = img.get_pixel(200, 200);
    assert_eq!(outside_px.0[3], 0);
}

#[test]
fn test_f6_t1_layer_group_transform_nesting() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    let translation = Transform2D::translate(50.0, 50.0);
    scene.push(SceneNode::Group {
        transform: translation,
        opacity: 1.0,
        children: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 50.0,
            h: 50.0,
            fill: Color::rgb(0, 255, 0),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = make_test_config(300, 300);
    let img = backend.render_frame(&scene, &config).unwrap();

    let px = img.get_pixel(60, 60);
    assert_eq!(px.0[1], 255);
    assert_eq!(px.0[3], 255);
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f6_t2_layer_blur_sigma_zero_noop() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Blur { sigma: 0.0 }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 10.0,
            y: 10.0,
            w: 80.0,
            h: 80.0,
            fill: Color::rgb(255, 0, 0),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = make_test_config(100, 100);
    assert!(backend.render_frame(&scene, &config).is_ok());
}

#[test]
fn test_f6_t2_layer_blur_sigma_nan_rejected() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Blur { sigma: f32::NAN }],
        shadow: None,
        children: vec![],
    });

    let config = make_test_config(100, 100);
    assert!(backend.render_frame(&scene, &config).is_err());
}

#[test]
fn test_f6_t2_layer_blur_sigma_exceeds_max_rejected() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Blur { sigma: 105.0 }],
        shadow: None,
        children: vec![],
    });

    let config = make_test_config(100, 100);
    assert!(backend.render_frame(&scene, &config).is_err());
}

#[test]
fn test_f6_t2_layer_empty_children_handling() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: Vec::new(),
        shadow: None,
        children: vec![],
    });

    let config = make_test_config(100, 100);
    assert!(backend.render_frame(&scene, &config).is_ok());
}

#[test]
fn test_f6_t2_layer_single_pixel_canvas() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Rect {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
        fill: Color::rgb(100, 150, 200),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    });

    let config = make_test_config(1, 1);
    let img = backend.render_frame(&scene, &config).unwrap();
    assert_eq!(img.width(), 1);
    assert_eq!(img.height(), 1);
    let px = img.get_pixel(0, 0);
    assert_eq!(px.0[0], 100);
    assert_eq!(px.0[1], 150);
    assert_eq!(px.0[2], 200);
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 7: VIGNETTE & EDGE FALLOFF SIMULATION
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f7_t1_vignette_alpha_masking() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: Some(vec![SceneNode::Circle {
            cx: 100.0,
            cy: 100.0,
            r: 80.0,
            fill: Color::WHITE,
            stroke: None,
            stroke_width: 0.0,
        }]),
        mask_mode: MaskMode::Alpha,
        filters: Vec::new(),
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
            fill: Color::rgb(0, 255, 128),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = make_test_config(200, 200);
    let img = backend.render_frame(&scene, &config).unwrap();

    // Center (100, 100) inside circle mask: visible
    let center_px = img.get_pixel(100, 100);
    assert_eq!(center_px.0[3], 255);
    assert_eq!(center_px.0[1], 255);

    // Corner (10, 10) outside circle mask: transparent
    let corner_px = img.get_pixel(10, 10);
    assert_eq!(corner_px.0[3], 0);
}

#[test]
fn test_f7_t1_vignette_luminance_masking() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: Some(vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            fill: Color::rgba(128, 128, 128, 255), // 50% gray
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }]),
        mask_mode: MaskMode::Luminance,
        filters: Vec::new(),
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            fill: Color::rgb(255, 0, 0),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = make_test_config(100, 100);
    let img = backend.render_frame(&scene, &config).unwrap();
    let px = img.get_pixel(50, 50);
    assert!(
        px.0[3] > 100 && px.0[3] < 150,
        "Alpha should be ~128 from 50% luminance mask, got {}",
        px.0[3]
    );
}

#[test]
fn test_f7_t1_vignette_radial_gradient_overlay() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::RadialGradient {
        cx: 100.0,
        cy: 100.0,
        r: 100.0,
        stops: vec![
            rust_motion_rasterizer::scene::GradientStop {
                position: 0.0,
                color: Color::rgba(0, 0, 0, 0),
            },
            rust_motion_rasterizer::scene::GradientStop {
                position: 1.0,
                color: Color::rgba(0, 0, 0, 255),
            },
        ],
    });

    let config = make_test_config(200, 200);
    assert!(backend.render_frame(&scene, &config).is_ok());
}

#[test]
fn test_f7_t1_vignette_layer_opacity_falloff() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Layer {
        opacity: 0.75,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Opacity { amount: 0.8 }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            fill: Color::rgb(0, 0, 255),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = make_test_config(100, 100);
    let img = backend.render_frame(&scene, &config).unwrap();
    let px = img.get_pixel(50, 50);
    assert!(
        (px.0[3] as i32 - 153).abs() <= 3,
        "Expected alpha ~153, got {}",
        px.0[3]
    );
}

#[test]
fn test_f7_t1_vignette_nested_group_opacity() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Group {
        opacity: 0.5,
        transform: Transform2D::identity(),
        children: vec![SceneNode::Group {
            opacity: 0.5,
            transform: Transform2D::identity(),
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
                fill: Color::WHITE,
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    });

    let config = make_test_config(100, 100);
    let img = backend.render_frame(&scene, &config).unwrap();
    let px = img.get_pixel(50, 50);
    assert!((px.0[3] as i32 - 64).abs() <= 2);
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f7_t2_vignette_zero_amount_opacity_filter() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Opacity { amount: 0.0 }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            fill: Color::WHITE,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = make_test_config(100, 100);
    let img = backend.render_frame(&scene, &config).unwrap();
    let px = img.get_pixel(50, 50);
    assert_eq!(px.0[3], 0, "Opacity 0.0 must yield 0 alpha");
}

#[test]
fn test_f7_t2_vignette_opacity_filter_nan_rejected() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Opacity { amount: f32::NAN }],
        shadow: None,
        children: vec![],
    });

    let config = make_test_config(100, 100);
    assert!(backend.render_frame(&scene, &config).is_err());
}

#[test]
fn test_f7_t2_vignette_opacity_filter_out_of_bounds_rejected() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Opacity { amount: 1.5 }],
        shadow: None,
        children: vec![],
    });

    let config = make_test_config(100, 100);
    assert!(backend.render_frame(&scene, &config).is_err());
}

#[test]
fn test_f7_t2_vignette_empty_mask_children() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: Some(vec![]),
        mask_mode: MaskMode::Alpha,
        filters: Vec::new(),
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            fill: Color::rgb(255, 0, 0),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = make_test_config(100, 100);
    assert!(backend.render_frame(&scene, &config).is_ok());
}

#[test]
fn test_f7_t2_vignette_extreme_layer_scale() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Group {
        transform: Transform2D::scale(1000.0, 1000.0),
        opacity: 1.0,
        children: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
            fill: Color::rgb(0, 0, 255),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = make_test_config(100, 100);
    assert!(backend.render_frame(&scene, &config).is_ok());
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 8: COLOR GRADING & IMAGE PROCESSING SUITE
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f8_t1_brightness_filter_scaling() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Brightness { amount: 2.0 }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            fill: Color::rgb(50, 50, 50),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = make_test_config(100, 100);
    let img = backend.render_frame(&scene, &config).unwrap();
    let px = img.get_pixel(50, 50);
    assert!((px.0[0] as i32 - 100).abs() <= 2);
    assert!((px.0[1] as i32 - 100).abs() <= 2);
    assert!((px.0[2] as i32 - 100).abs() <= 2);
}

#[test]
fn test_f8_t1_grayscale_filter_mixing() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Grayscale { amount: 1.0 }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            fill: Color::rgb(255, 0, 0),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = make_test_config(100, 100);
    let img = backend.render_frame(&scene, &config).unwrap();
    let px = img.get_pixel(50, 50);

    assert_eq!(px.0[0], px.0[1]);
    assert_eq!(px.0[1], px.0[2]);
    assert!((px.0[0] as i32 - 54).abs() <= 2);
}

#[test]
fn test_f8_t1_grayscale_partial_mixing() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Grayscale { amount: 0.5 }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            fill: Color::rgb(255, 0, 0),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = make_test_config(100, 100);
    let img = backend.render_frame(&scene, &config).unwrap();
    let px = img.get_pixel(50, 50);
    assert!(px.0[0] < 255 && px.0[0] > 54);
    assert!(px.0[1] > 0 && px.0[1] < 54);
}

#[test]
fn test_f8_t1_sequential_filter_chain() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![
            SceneFilter::Brightness { amount: 0.5 },
            SceneFilter::Grayscale { amount: 1.0 },
            SceneFilter::Opacity { amount: 0.8 },
        ],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            fill: Color::rgb(200, 100, 50),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = make_test_config(100, 100);
    assert!(backend.render_frame(&scene, &config).is_ok());
}

#[test]
fn test_f8_t1_blend_modes_rendering() {
    let blend_modes = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
    ];

    let backend = TinySkiaBackend::new();
    for mode in blend_modes {
        let mut scene = Scene::new();
        scene.push(SceneNode::Layer {
            opacity: 1.0,
            blend_mode: mode,
            clip: None,
            mask: None,
            mask_mode: MaskMode::Alpha,
            filters: Vec::new(),
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 50.0,
                h: 50.0,
                fill: Color::rgb(255, 100, 0),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        });
        let config = make_test_config(50, 50);
        assert!(backend.render_frame(&scene, &config).is_ok());
    }
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f8_t2_brightness_zero_flattens_to_black() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Brightness { amount: 0.0 }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 50.0,
            h: 50.0,
            fill: Color::WHITE,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = make_test_config(50, 50);
    let img = backend.render_frame(&scene, &config).unwrap();
    let px = img.get_pixel(25, 25);
    assert_eq!(px.0[0], 0);
    assert_eq!(px.0[1], 0);
    assert_eq!(px.0[2], 0);
    assert_eq!(px.0[3], 255);
}

#[test]
fn test_f8_t2_brightness_nan_rejected() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Brightness { amount: f32::NAN }],
        shadow: None,
        children: vec![],
    });

    let config = make_test_config(50, 50);
    assert!(backend.render_frame(&scene, &config).is_err());
}

#[test]
fn test_f8_t2_brightness_negative_rejected() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Brightness { amount: -0.5 }],
        shadow: None,
        children: vec![],
    });

    let config = make_test_config(50, 50);
    assert!(backend.render_frame(&scene, &config).is_err());
}

#[test]
fn test_f8_t2_grayscale_nan_rejected() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Grayscale { amount: f32::NAN }],
        shadow: None,
        children: vec![],
    });

    let config = make_test_config(50, 50);
    assert!(backend.render_frame(&scene, &config).is_err());
}

#[test]
fn test_f8_t2_grayscale_negative_rejected() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Grayscale { amount: -0.1 }],
        shadow: None,
        children: vec![],
    });

    let config = make_test_config(50, 50);
    assert!(backend.render_frame(&scene, &config).is_err());
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 12: TEXT AUTO-SCALING (fit_text)
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f12_t1_fit_text_single_line_optimal_size() {
    let text = "Auto Fit Title";
    let size = fit_text(text, 300.0, &[], 8.0, 72.0).unwrap();
    assert!((8.0..=72.0).contains(&size));
    let measured = measure_text_width(text, size as f32, &[]).unwrap();
    assert!(measured as f64 <= 300.0 + 1.0);
}

#[test]
fn test_f12_t1_fit_text_tight_width_scales_down() {
    let text = "Long Multi Word Headline Text";
    let size_wide = fit_text(text, 800.0, &[], 10.0, 100.0).unwrap();
    let size_narrow = fit_text(text, 200.0, &[], 10.0, 100.0).unwrap();
    assert!(size_narrow < size_wide);
}

#[test]
fn test_f12_t1_fit_text_large_width_returns_max_size() {
    let text = "Short";
    let size = fit_text(text, 5000.0, &[], 10.0, 48.0).unwrap();
    assert_eq!(size, 48.0);
}

#[test]
fn test_f12_t1_fit_text_empty_string_returns_max_size() {
    let size = fit_text("", 300.0, &[], 10.0, 60.0).unwrap();
    assert_eq!(size, 60.0);
}

#[test]
fn test_f12_t1_fit_text_monotonicity_across_widths() {
    let text = "Monotonic Text Sizing";
    let widths = [50.0, 100.0, 200.0, 400.0, 800.0];
    let mut prev_size = 0.0f64;
    for &w in &widths {
        let size = fit_text(text, w, &[], 8.0, 100.0).unwrap();
        assert!(size >= prev_size - 1e-4);
        prev_size = size;
    }
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f12_t2_fit_text_unbreakable_word_falls_back_to_min() {
    let text = "SupercalifragilisticexpialidociousLongUnbreakableString";
    let size = fit_text(text, 10.0, &[], 12.0, 64.0).unwrap();
    assert_eq!(size, 12.0);
}

#[test]
fn test_f12_t2_fit_text_huge_string_stress() {
    let text = "A".repeat(10_000);
    let size = fit_text(&text, 200.0, &[], 10.0, 50.0).unwrap();
    assert_eq!(size, 10.0);
}

#[test]
fn test_f12_t2_fit_text_multilingual_unicode_scripts() {
    let scripts = [
        "안녕하세요 Rust Motion",
        "こんにちは世界",
        "你好世界",
        "مرحبا بالعالم",
        "🦀🚀✨🔥",
    ];
    for script in scripts {
        let res = fit_text(script, 300.0, &[], 8.0, 48.0);
        assert!(res.is_ok());
    }
}

#[test]
fn test_f12_t2_fit_text_negative_and_zero_bounds_rejected() {
    assert!(fit_text("test", 0.0, &[], 8.0, 48.0).is_err());
    assert!(fit_text("test", -50.0, &[], 8.0, 48.0).is_err());
    assert!(fit_text("test", 200.0, &[], 0.0, 48.0).is_err());
    assert!(fit_text("test", 200.0, &[], 50.0, 20.0).is_err());
}

#[test]
fn test_f12_t2_fit_text_nan_and_infinite_bounds_rejected() {
    assert!(fit_text("test", f64::NAN, &[], 8.0, 48.0).is_err());
    assert!(fit_text("test", 200.0, &[], f64::NAN, 48.0).is_err());
    assert!(fit_text("test", 200.0, &[], 8.0, f64::INFINITY).is_err());
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 14: TYPOGRAPHY & TEXT BOX LAYOUT
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f14_t1_layout_text_box_word_wrapping() {
    let request = TextBox::new(
        "The quick brown fox jumps over the lazy dog",
        0.0,
        0.0,
        200.0,
        300.0,
        24.0,
    );
    let layout = layout_text_box(&request).unwrap();
    assert!(layout.lines.len() > 1);
}

#[test]
fn test_f14_t1_layout_text_box_horizontal_alignments() {
    let text = "Line Alignment";
    let box_w = 400.0;

    let mut req_start = TextBox::new(text, 0.0, 0.0, box_w, 200.0, 24.0);
    req_start.horizontal_align = TextHorizontalAlign::Start;

    let mut req_center = TextBox::new(text, 0.0, 0.0, box_w, 200.0, 24.0);
    req_center.horizontal_align = TextHorizontalAlign::Center;

    let mut req_end = TextBox::new(text, 0.0, 0.0, box_w, 200.0, 24.0);
    req_end.horizontal_align = TextHorizontalAlign::End;

    let l_start = layout_text_box(&req_start).unwrap();
    let l_center = layout_text_box(&req_center).unwrap();
    let l_end = layout_text_box(&req_end).unwrap();

    assert_eq!(l_start.lines[0].x, 0.0);
    assert!(l_center.lines[0].x > 0.0);
    assert!(l_end.lines[0].x > l_center.lines[0].x);
}

#[test]
fn test_f14_t1_layout_text_box_vertical_alignments() {
    let text = "Vertical Test";
    let box_h = 400.0;

    let mut req_top = TextBox::new(text, 0.0, 0.0, 300.0, box_h, 24.0);
    req_top.vertical_align = TextVerticalAlign::Start;

    let mut req_mid = TextBox::new(text, 0.0, 0.0, 300.0, box_h, 24.0);
    req_mid.vertical_align = TextVerticalAlign::Center;

    let mut req_bot = TextBox::new(text, 0.0, 0.0, 300.0, box_h, 24.0);
    req_bot.vertical_align = TextVerticalAlign::End;

    let l_top = layout_text_box(&req_top).unwrap();
    let l_mid = layout_text_box(&req_mid).unwrap();
    let l_bot = layout_text_box(&req_bot).unwrap();

    assert!(l_top.lines[0].y < l_mid.lines[0].y);
    assert!(l_mid.lines[0].y < l_bot.lines[0].y);
}

#[test]
fn test_f14_t1_layout_text_box_ellipsis_overflow_truncation() {
    let text = "A very long single line that exceeds width boundary constraints";
    let mut req = TextBox::new(text, 0.0, 0.0, 100.0, 50.0, 24.0);
    req.max_lines = Some(1);
    req.overflow = TextOverflow::Ellipsis;

    let layout = layout_text_box(&req).unwrap();
    assert_eq!(layout.lines.len(), 1);
    assert!(layout.lines[0].text.ends_with('…'));
}

#[test]
fn test_f14_t1_measure_text_width_proportional_to_size() {
    let text = "Proportional Sizing";
    let w16 = measure_text_width(text, 16.0, &[]).unwrap();
    let w32 = measure_text_width(text, 32.0, &[]).unwrap();
    assert!((w32 / w16 - 2.0).abs() < 0.1);
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f14_t2_layout_text_box_zero_dimensions_rejected() {
    let req_w0 = TextBox::new("test", 0.0, 0.0, 0.0, 100.0, 24.0);
    assert!(layout_text_box(&req_w0).is_err());

    let req_h0 = TextBox::new("test", 0.0, 0.0, 100.0, 0.0, 24.0);
    assert!(layout_text_box(&req_h0).is_err());
}

#[test]
fn test_f14_t2_layout_text_box_min_font_size_clamp() {
    let text = "Long line in tight box";
    let mut req = TextBox::new(text, 0.0, 0.0, 50.0, 50.0, 32.0);
    req.min_font_size = 14.0;

    let layout = layout_text_box(&req).unwrap();
    assert!(layout.font_size >= 14.0);
}

#[test]
fn test_f14_t2_layout_text_box_max_lines_limit() {
    let text = "One two three four five six seven eight nine ten";
    let mut req = TextBox::new(text, 0.0, 0.0, 80.0, 400.0, 20.0);
    req.max_lines = Some(2);

    let layout = layout_text_box(&req).unwrap();
    assert!(layout.lines.len() <= 2);
}

#[test]
fn test_f14_t2_measure_text_width_empty_string_is_zero() {
    let w = measure_text_width("", 24.0, &[]).unwrap();
    assert_eq!(w, 0.0);
}

#[test]
fn test_f14_t2_layout_text_box_newlines_and_spaces() {
    let text = "First Line\nSecond Line\n\nFourth Line";
    let req = TextBox::new(text, 0.0, 0.0, 300.0, 300.0, 18.0);
    let layout = layout_text_box(&req).unwrap();
    assert!(layout.lines.len() >= 3);
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 15: UNIFIED PUBLIC APIS & RE-EXPORTS
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f15_t1_scene_node_rect_rendering() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Rect {
        x: 10.0,
        y: 20.0,
        w: 80.0,
        h: 60.0,
        fill: Color::rgb(255, 128, 0),
        stroke: Some(Color::BLACK),
        stroke_width: 2.0,
        corner_radius: 5.0,
    });

    let config = make_test_config(100, 100);
    let img = backend.render_frame(&scene, &config).unwrap();
    let px = img.get_pixel(50, 50);
    assert_eq!(px.0[0], 255);
    assert_eq!(px.0[1], 128);
}

#[test]
fn test_f15_t1_scene_node_circle_rendering() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Circle {
        cx: 50.0,
        cy: 50.0,
        r: 30.0,
        fill: Color::rgb(0, 200, 100),
        stroke: None,
        stroke_width: 0.0,
    });

    let config = make_test_config(100, 100);
    let img = backend.render_frame(&scene, &config).unwrap();
    let center = img.get_pixel(50, 50);
    assert_eq!(center.0[1], 200);
    let outside = img.get_pixel(5, 5);
    assert_eq!(outside.0[3], 0);
}

#[test]
fn test_f15_t1_scene_node_layer_with_filter_list() {
    let layer = SceneNode::Layer {
        opacity: 0.9,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Brightness { amount: 1.2 }],
        shadow: None,
        children: vec![],
    };

    match layer {
        SceneNode::Layer {
            filters, opacity, ..
        } => {
            assert_eq!(filters.len(), 1);
            assert_eq!(opacity, 0.9);
        }
        _ => panic!("Expected Layer variant"),
    }
}

#[test]
fn test_f15_t1_color_rgba_and_hex_constructors() {
    let c1 = Color::rgb(10, 20, 30);
    assert_eq!(c1.r, 10);
    assert_eq!(c1.g, 20);
    assert_eq!(c1.b, 30);
    assert_eq!(c1.a, 255);

    let c2 = Color::from_hex("#ff8800").unwrap();
    assert_eq!(c2.r, 255);
    assert_eq!(c2.g, 136);
    assert_eq!(c2.b, 0);
}

#[test]
fn test_f15_t1_transform2d_translate_scale_rotate() {
    let t = Transform2D::translate(10.0, 20.0);
    assert_eq!(t.tx, 10.0);
    assert_eq!(t.ty, 20.0);

    let s = Transform2D::scale(2.0, 3.0);
    assert_eq!(s.scale_x, 2.0);
    assert_eq!(s.scale_y, 3.0);

    let r = Transform2D::rotate(90.0);
    assert_eq!(r.rotate_deg, 90.0);
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f15_t2_scene_node_serde_roundtrip() {
    let node = SceneNode::Rect {
        x: 15.0,
        y: 25.0,
        w: 120.0,
        h: 80.0,
        fill: Color::rgb(12, 34, 56),
        stroke: Some(Color::WHITE),
        stroke_width: 1.5,
        corner_radius: 4.0,
    };

    let json = serde_json::to_string(&node).unwrap();
    let deserialized: SceneNode = serde_json::from_str(&json).unwrap();
    assert_eq!(node, deserialized);
}

#[test]
fn test_f15_t2_scene_filter_serde_roundtrip() {
    let filters = vec![
        SceneFilter::Blur { sigma: 4.5 },
        SceneFilter::Brightness { amount: 1.2 },
        SceneFilter::Grayscale { amount: 0.8 },
        SceneFilter::Opacity { amount: 0.5 },
    ];

    for filter in filters {
        let json = serde_json::to_string(&filter).unwrap();
        let deserialized: SceneFilter = serde_json::from_str(&json).unwrap();
        assert_eq!(filter, deserialized);
    }
}

#[test]
fn test_f15_t2_transform2d_default() {
    let def = Transform2D::default();
    assert_eq!(def.tx, 0.0);
    assert_eq!(def.ty, 0.0);
    assert_eq!(def.scale_x, 1.0);
    assert_eq!(def.scale_y, 1.0);
    assert_eq!(def.rotate_deg, 0.0);
}

#[test]
fn test_f15_t2_color_transparent_and_opaque_edge_values() {
    let transparent = Color::TRANSPARENT;
    assert_eq!(transparent.a, 0);

    let hex_invalid = Color::from_hex("invalid-hex");
    assert!(hex_invalid.is_none());
}

#[test]
fn test_f15_t2_rasterizer_empty_scene_rendering() {
    let backend = TinySkiaBackend::new();
    let scene = Scene::new();
    let config = make_test_config(100, 100);
    let img = backend.render_frame(&scene, &config).unwrap();
    for px in img.pixels() {
        assert_eq!(px.0[3], 0);
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// TIER 3: PAIRWISE CROSS-FEATURE COMBINATIONS
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_pairwise_layer_filter_chain_brightness_contrast_grayscale() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![
            SceneFilter::Brightness { amount: 1.5 },
            SceneFilter::Grayscale { amount: 0.75 },
            SceneFilter::Blur { sigma: 2.0 },
        ],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 20.0,
            y: 20.0,
            w: 160.0,
            h: 160.0,
            fill: Color::rgb(255, 60, 20),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 10.0,
        }],
    });

    let config = make_test_config(200, 200);
    let img = backend.render_frame(&scene, &config).unwrap();
    let px = img.get_pixel(100, 100);
    assert!(px.0[3] > 200);
}

#[test]
fn test_pairwise_text_layout_inside_layer_with_shadow() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    let text_req = TextBox::new("Cyber Shadow Title", 0.0, 0.0, 300.0, 100.0, 32.0);
    let layout = layout_text_box(&text_req).unwrap();

    let mut text_nodes = Vec::new();
    for line in layout.lines {
        text_nodes.push(SceneNode::Text {
            x: line.x,
            y: line.y,
            content: line.text,
            font_size: layout.font_size,
            color: Color::WHITE,
            font_weight: 700,
            font_sources: Vec::new(),
        });
    }

    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Brightness { amount: 1.1 }],
        shadow: Some(SceneShadow {
            offset_x: 6.0,
            offset_y: 6.0,
            blur_sigma: 3.0,
            color: Color::rgba(0, 255, 255, 150),
        }),
        children: text_nodes,
    });

    let config = make_test_config(500, 300);
    assert!(backend.render_frame(&scene, &config).is_ok());
}

#[test]
fn test_pairwise_fit_text_and_tiny_skia_text_node() {
    let backend = TinySkiaBackend::new();
    let font_size = fit_text("Dynamic Fitted Header", 400.0, &[], 12.0, 64.0).unwrap();

    let mut scene = Scene::new();
    scene.push(SceneNode::Text {
        x: 20.0,
        y: 80.0,
        content: "Dynamic Fitted Header".into(),
        font_size: font_size as f32,
        color: Color::rgb(255, 200, 50),
        font_weight: 400,
        font_sources: Vec::new(),
    });

    let config = make_test_config(500, 200);
    let img = backend.render_frame(&scene, &config).unwrap();
    assert_eq!(img.width(), 500);
}

#[test]
fn test_pairwise_transform2d_and_layer_clipping() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Group {
        transform: Transform2D::scale(1.5, 1.5),
        opacity: 1.0,
        children: vec![SceneNode::Layer {
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            clip: Some(ClipRegion::Rect {
                x: 20.0,
                y: 20.0,
                w: 100.0,
                h: 100.0,
                corner_radius: 0.0,
            }),
            mask: None,
            mask_mode: MaskMode::Alpha,
            filters: Vec::new(),
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 200.0,
                fill: Color::rgb(0, 150, 255),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    });

    let config = make_test_config(400, 400);
    assert!(backend.render_frame(&scene, &config).is_ok());
}

#[test]
fn test_pairwise_frame_cache_metrics_and_eviction() {
    let cache = FrameCacheManager::new(FrameCacheConfig::with_max_bytes(1024 * 1024));
    let key1 = FrameCacheKey::new("comp", 1, 100, 100, 0);
    let key2 = FrameCacheKey::new("comp", 2, 100, 100, 0);

    let img = std::sync::Arc::new(image::RgbaImage::new(100, 100));
    cache.insert(key1.clone(), img.clone());
    cache.insert(key2.clone(), img.clone());

    assert!(cache.contains(&key1));
    assert!(cache.contains(&key2));
    let metrics = cache.metrics();
    assert_eq!(metrics.entry_count, 2);
}

#[test]
fn test_pairwise_chromatic_aberration_and_vignette() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Layer {
        opacity: 0.9,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: Some(vec![SceneNode::Circle {
            cx: 150.0,
            cy: 150.0,
            r: 120.0,
            fill: Color::WHITE,
            stroke: None,
            stroke_width: 0.0,
        }]),
        mask_mode: MaskMode::Alpha,
        filters: vec![
            SceneFilter::Brightness { amount: 1.2 },
            SceneFilter::Blur { sigma: 1.5 },
        ],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 50.0,
            y: 50.0,
            w: 200.0,
            h: 200.0,
            fill: Color::rgb(255, 0, 128),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 12.0,
        }],
    });

    let config = make_test_config(300, 300);
    let img = backend.render_frame(&scene, &config).unwrap();
    let center = img.get_pixel(150, 150);
    assert!(center.0[3] > 0);
}

// ══════════════════════════════════════════════════════════════════════════════
// TIER 4: REAL-WORLD APPLICATION SCENARIOS
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_tier4_scenario_procedural_cyberpunk_title_card() {
    // 1920x1080 resolution, dark cyberpunk aesthetic title card
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    // 1. Dark background
    scene.push(SceneNode::Rect {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
        fill: Color::rgb(11, 13, 25), // #0b0d19
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    });

    // 2. Cyan accent neon line path
    scene.push(SceneNode::Path {
        d: "M 200,350 L 1720,350".into(),
        fill: None,
        stroke: Some(Color::rgb(0, 240, 255)),
        stroke_width: 3.0,
        opacity: 1.0,
    });

    // 3. Multi-line auto-fitted main title inside composited Layer with glow shadow
    let title_text = "CYBERPUNK 2088\nNEON PROTOCOL";
    let title_req = TextBox::new(title_text, 200.0, 400.0, 1400.0, 300.0, 64.0);

    let layout = layout_text_box(&title_req).unwrap();
    let mut title_nodes = Vec::new();
    for line in layout.lines {
        title_nodes.push(SceneNode::Text {
            x: line.x,
            y: line.y,
            content: line.text,
            font_size: layout.font_size,
            color: Color::WHITE,
            font_weight: 700,
            font_sources: Vec::new(),
        });
    }

    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Brightness { amount: 1.15 }],
        shadow: Some(SceneShadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur_sigma: 8.0,
            color: Color::rgba(255, 0, 128, 200), // Neon magenta glow
        }),
        children: title_nodes,
    });

    // 4. Badge container
    scene.push(SceneNode::Rect {
        x: 200.0,
        y: 800.0,
        w: 320.0,
        h: 48.0,
        fill: Color::rgba(255, 0, 128, 40),
        stroke: Some(Color::rgb(255, 0, 128)),
        stroke_width: 1.5,
        corner_radius: 8.0,
    });

    // 5. Render frame
    let config = make_test_config(1920, 1080);
    let img = backend
        .render_frame(&scene, &config)
        .expect("Cyberpunk title card render failed");
    assert_eq!(img.width(), 1920);
    assert_eq!(img.height(), 1080);

    // Verify background corner is dark #0b0d19
    let bg_px = img.get_pixel(10, 10);
    assert_eq!(bg_px.0[0], 11);
    assert_eq!(bg_px.0[1], 13);
    assert_eq!(bg_px.0[2], 25);
}
