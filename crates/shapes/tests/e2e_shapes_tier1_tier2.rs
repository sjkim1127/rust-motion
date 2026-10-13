//! Comprehensive E2E Test Suite for Procedural Shapes & Multi-corner Paths (Tiers 1-4)
//!
//! Features covered:
//! - Feature 16: Multi-corner Parametric Text Box Paths & Procedural Motion Graphics Shapes
//!   (`make_pie`, `make_star`, `make_callout`, `make_spark`, `make_rect`, `make_circle`, `make_triangle`, `make_polygon`, `make_arrow`, `make_heart`, `SceneShape`)
//! - Tier 3: Pairwise cross-feature combinations (Shapes + Filters + Transitions)
//! - Tier 4: Real-world video application scenario (Streaming Caption Box with Speech Callout)

use rust_motion_composition::{NativeCompositionContext, SceneEmitter, SceneFrameContext};
use rust_motion_rasterizer::{Color, Scene, SceneNode};
use rust_motion_shapes::{
    make_arrow, make_callout, make_circle, make_heart, make_pie, make_polygon, make_rect,
    make_spark, make_star, make_triangle, CalloutDirection, SceneShape,
};
use serde_json::Value;

fn make_test_context(w: u32, h: u32, duration: u32) -> NativeCompositionContext {
    NativeCompositionContext {
        width: w,
        height: h,
        fps: 30.0,
        duration_in_frames: duration,
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 16: PROCEDURAL SHAPES & PARAMETRIC PATHS
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f16_t1_pie_path_generation_half_and_full() {
    let pie_half = make_pie(100.0, 0.5, true, false, 0.0);
    assert_eq!(pie_half.width, 200.0);
    assert_eq!(pie_half.height, 200.0);
    assert!(pie_half.path.starts_with("M 100"));
    assert!(pie_half.path.contains('A'));
    assert!(pie_half.path.ends_with('Z'));

    let pie_full = make_pie(100.0, 1.0, true, false, 0.0);
    assert!(pie_full.path.contains("A 100 100"));
}

#[test]
fn test_f16_t1_star_path_generation() {
    let (star_5, w, h) = make_star(5, 40.0, 100.0);
    assert_eq!(w, 200.0);
    assert_eq!(h, 200.0);
    assert!(star_5.starts_with('M'));
    assert!(star_5.ends_with('Z'));
    let l_count = star_5.matches('L').count();
    assert_eq!(l_count, 9); // 10 vertices: 1 Move + 9 Lines + 1 Close
}

#[test]
fn test_f16_t1_callout_speech_bubble_directions() {
    let directions = [
        CalloutDirection::Down,
        CalloutDirection::Up,
        CalloutDirection::Left,
        CalloutDirection::Right,
    ];

    for dir in directions {
        let callout = make_callout(200.0, 120.0, 30.0, dir);
        assert!(!callout.path.is_empty());
        assert!(callout.path.starts_with('M'));
        assert!(callout.path.ends_with('Z'));
        assert!(callout.width >= 200.0);
        assert!(callout.height >= 120.0);
    }
}

#[test]
fn test_f16_t1_spark_diamond_and_concave_curves() {
    // Edge roundness 0.0 -> diamond with bezier
    let spark_diamond = make_spark(100.0, 100.0, 0.0, 0.0);
    assert_eq!(spark_diamond.width, 100.0);
    assert_eq!(spark_diamond.height, 100.0);
    assert!(spark_diamond.path.contains('C'));

    // Deep concave roundness 0.8
    let spark_concave = make_spark(100.0, 100.0, 0.8, 0.0);
    assert!(spark_concave.path.starts_with("M 50 0"));
}

#[test]
fn test_f16_t1_rect_with_and_without_corner_radius() {
    let (rect_sharp, w1, h1) = make_rect(200.0, 100.0, 0.0);
    assert_eq!(w1, 200.0);
    assert_eq!(h1, 100.0);
    assert_eq!(rect_sharp, "M 0 0 L 200 0 L 200 100 L 0 100 Z");

    let (rect_rounded, w2, h2) = make_rect(200.0, 100.0, 16.0);
    assert_eq!(w2, 200.0);
    assert_eq!(h2, 100.0);
    assert!(rect_rounded.contains('A'));
}

#[test]
fn test_f16_t1_geometric_primitives_circle_triangle_polygon_arrow_heart() {
    let (circle, cw, ch) = make_circle(50.0);
    assert_eq!(cw, 100.0);
    assert_eq!(ch, 100.0);
    assert!(circle.contains('A'));

    let (triangle, tw, th) = make_triangle(100.0);
    assert_eq!(tw, 100.0);
    assert!(th > 0.0);
    assert!(triangle.starts_with('M'));

    let (polygon_6, pw, ph) = make_polygon(6, 60.0);
    assert_eq!(pw, 120.0);
    assert_eq!(ph, 120.0);
    assert!(polygon_6.ends_with('Z'));

    let (_arrow, aw, ah) = make_arrow(120.0, 40.0);
    assert_eq!(aw, 120.0);
    assert_eq!(ah, 100.0);

    let heart = make_heart(100.0, 90.0);
    assert_eq!(heart.width, 100.0);
    assert_eq!(heart.height, 90.0);
    assert!(heart.path.contains('C'));
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f16_t2_pie_zero_and_negative_radius() {
    let pie_zero = make_pie(0.0, 0.5, true, false, 0.0);
    assert_eq!(pie_zero.width, 0.0);
    assert_eq!(pie_zero.height, 0.0);

    let pie_neg = make_pie(-50.0, 0.5, true, false, 0.0);
    assert_eq!(pie_neg.width, 0.0);
}

#[test]
fn test_f16_t2_pie_progress_extremes_and_clamp() {
    let pie_0 = make_pie(50.0, 0.0, true, false, 0.0);
    assert_eq!(pie_0.path, "");

    let pie_over = make_pie(50.0, 1.5, true, false, 0.0);
    assert!(pie_over.path.contains('A'));
}

#[test]
fn test_f16_t2_star_min_points_clamp() {
    // Points < 3 clamped to 3
    let (star_1, _, _) = make_star(1, 20.0, 50.0);
    let l_count = star_1.matches('L').count();
    assert_eq!(l_count, 5); // 6 vertices -> 1 M + 5 L + 1 Z
}

#[test]
fn test_f16_t2_spark_zero_dimensions() {
    let spark_zero = make_spark(0.0, 0.0, 0.5, 0.0);
    assert_eq!(spark_zero.path, "");
    assert_eq!(spark_zero.width, 0.0);
}

#[test]
fn test_f16_t2_rect_corner_radius_exceeds_half_bounds() {
    // If w = 100, max radius = 50. corner_radius = 100 clamped to 50
    let (rect_clamped, _, _) = make_rect(100.0, 100.0, 100.0);
    assert!(rect_clamped.contains("A 50 50"));
}

// ══════════════════════════════════════════════════════════════════════════════
// TIER 3: PAIRWISE CROSS-FEATURE COMBINATIONS
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_pairwise_scene_shape_emitter_rendering() {
    let shape_emitter = SceneShape::star(5, 30.0, 80.0);
    let ctx = make_test_context(500, 500, 30);
    let mut scene = Scene::new();
    assert!(shape_emitter
        .emit(SceneFrameContext::new(0, ctx), &Value::Null, &mut scene)
        .is_ok());

    assert_eq!(scene.nodes.len(), 1);
    match &scene.nodes[0] {
        SceneNode::Path {
            d, stroke_width, ..
        } => {
            assert!(d.starts_with('M'));
            assert_eq!(*stroke_width, 0.0);
        }
        _ => panic!("Expected Path node"),
    }
}

#[test]
fn test_pairwise_spark_shape_inside_layer_with_filter() {
    let spark = make_spark(200.0, 200.0, 0.7, 4.0);
    let node = SceneNode::Path {
        d: spark.path,
        fill: Some(Color::rgb(255, 215, 0)),
        stroke: Some(Color::WHITE),
        stroke_width: 2.0,
        opacity: 1.0,
    };

    let layer = SceneNode::Layer {
        opacity: 1.0,
        blend_mode: rust_motion_rasterizer::scene::BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: rust_motion_rasterizer::scene::MaskMode::Alpha,
        filters: vec![rust_motion_rasterizer::scene::SceneFilter::Brightness { amount: 1.3 }],
        shadow: Some(rust_motion_rasterizer::scene::SceneShadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur_sigma: 6.0,
            color: Color::rgba(255, 215, 0, 180),
        }),
        children: vec![node],
    };

    let mut scene = Scene::new();
    scene.push(layer);
    assert_eq!(scene.nodes.len(), 1);
}

#[test]
fn test_pairwise_callout_and_pie_chart_composition() {
    let callout = SceneShape::callout(250.0, 80.0, 20.0, CalloutDirection::Down);
    let pie = SceneShape::pie(60.0, 0.75, true, false, 0.0);

    let ctx = make_test_context(800, 600, 30);
    let mut scene = Scene::new();

    callout
        .emit(SceneFrameContext::new(0, ctx), &Value::Null, &mut scene)
        .unwrap();
    pie.emit(SceneFrameContext::new(0, ctx), &Value::Null, &mut scene)
        .unwrap();

    assert_eq!(scene.nodes.len(), 2);
}

// ══════════════════════════════════════════════════════════════════════════════
// TIER 4: REAL-WORLD APPLICATION SCENARIOS
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_tier4_scenario_streaming_caption_box_with_callout() {
    // 1920x1080 resolution, animated streaming dialogue caption bubble
    let callout = make_callout(640.0, 140.0, 30.0, CalloutDirection::Down);
    let badge_spark = make_spark(32.0, 32.0, 0.6, 2.0);

    let mut scene = Scene::new();

    // 1. Semi-transparent dark background speech bubble
    scene.push(SceneNode::Path {
        d: callout.path,
        fill: Some(Color::rgba(15, 23, 42, 230)), // #0f172a
        stroke: Some(Color::rgb(56, 189, 248)),   // #38bdf8 cyan border
        stroke_width: 2.0,
        opacity: 1.0,
    });

    // 2. Verified speaker spark badge
    scene.push(SceneNode::Path {
        d: badge_spark.path,
        fill: Some(Color::rgb(250, 204, 21)), // gold spark #facc15
        stroke: None,
        stroke_width: 0.0,
        opacity: 1.0,
    });

    assert_eq!(scene.nodes.len(), 2);
}
