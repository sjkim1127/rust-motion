//! Renderer Conformance and Determinism Test Suite
//!
//! Verifies Rust Motion 1.0 renderer conformance requirements (#89):
//! 1. Frame Evaluation Invariance:
//!    - Sequential forward evaluation
//!    - Sequential reverse evaluation
//!    - Sparse / random access evaluation
//!    - Repeated frame requests
//!    - Concurrent multi-threaded evaluation
//! 2. State Isolation & Pure Functions (no mutable state across frames)
//! 3. Historical rendering bug regression fixtures

use dioxuscut_composition::{Composition, NativeCompositionContext, SpringRectsComposition};
use dioxuscut_rasterizer::backend::{FrameConfig, RasterizerBackend};
use dioxuscut_rasterizer::font::{
    layout_text_box, TextBox, TextHorizontalAlign, TextVerticalAlign,
};
use dioxuscut_rasterizer::scene::{
    BlendMode, ClipRegion, Color, MaskMode, Scene, SceneFilter, SceneNode,
};
use dioxuscut_rasterizer::TinySkiaBackend;
use std::collections::BTreeMap;

fn test_context() -> NativeCompositionContext {
    NativeCompositionContext {
        width: 320,
        height: 240,
        fps: 30.0,
        duration_in_frames: 60,
    }
}

fn frame_config(width: u32, height: u32, frame: u32) -> FrameConfig {
    FrameConfig::new(width, height, frame, 30.0)
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. DETERMINISM ACROSS EVALUATION PATTERNS
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_conformance_deterministic_frame_evaluation_across_access_patterns() {
    let comp = SpringRectsComposition;
    let ctx = test_context();
    let prepared = comp
        .prepare(&serde_json::Value::Null, ctx)
        .expect("prepares");
    let backend = TinySkiaBackend::new();

    // 1. Render sequential forward frames 0..30 as ground truth
    let test_frames: Vec<u32> = vec![0, 1, 5, 12, 17, 23, 29];
    let mut baseline_images: BTreeMap<u32, Vec<u8>> = BTreeMap::new();

    for &f in &test_frames {
        let scene = prepared.render(f).expect("sequential frame renders");
        let img = backend
            .render_frame(&scene, &frame_config(ctx.width, ctx.height, f))
            .expect("scene rasterizes");
        baseline_images.insert(f, img.into_raw());
    }

    // 2. Reverse evaluation: render from 29 down to 0
    for &f in test_frames.iter().rev() {
        let scene = prepared.render(f).expect("reverse frame renders");
        let img = backend
            .render_frame(&scene, &frame_config(ctx.width, ctx.height, f))
            .expect("scene rasterizes");
        let raw = img.into_raw();
        assert_eq!(
            raw, baseline_images[&f],
            "Frame {} in reverse order must be byte-identical to sequential baseline",
            f
        );
    }

    // 3. Sparse / pseudo-random evaluation
    let sparse_frames: Vec<u32> = vec![17, 0, 29, 5, 23, 1, 12];
    for &f in &sparse_frames {
        let scene = prepared.render(f).expect("sparse frame renders");
        let img = backend
            .render_frame(&scene, &frame_config(ctx.width, ctx.height, f))
            .expect("scene rasterizes");
        let raw = img.into_raw();
        assert_eq!(
            raw, baseline_images[&f],
            "Frame {} in sparse order must be byte-identical to sequential baseline",
            f
        );
    }

    // 4. Repeated evaluation of identical frame
    for repetition in 0..5 {
        let target_frame = 12;
        let scene = prepared
            .render(target_frame)
            .expect("repeated frame renders");
        let img = backend
            .render_frame(&scene, &frame_config(ctx.width, ctx.height, target_frame))
            .expect("scene rasterizes");
        let raw = img.into_raw();
        assert_eq!(
            raw, baseline_images[&target_frame],
            "Frame {} repetition {} must be byte-identical to baseline",
            target_frame, repetition
        );
    }

    // 5. Concurrent multi-threaded evaluation
    let prep = &prepared;
    std::thread::scope(|s| {
        for &f in &test_frames {
            let expected_bytes = &baseline_images[&f];
            s.spawn(move || {
                let local_backend = TinySkiaBackend::new();
                let scene = prep.render(f).expect("concurrent render");
                let img = local_backend
                    .render_frame(&scene, &frame_config(320, 240, f))
                    .expect("concurrent rasterize");
                assert_eq!(
                    &img.into_raw(),
                    expected_bytes,
                    "Frame {} evaluated concurrently must match sequential baseline",
                    f
                );
            });
        }
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. STATE ISOLATION & PURITY TESTS
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_conformance_state_isolation_between_render_passes() {
    let comp = SpringRectsComposition;
    let ctx = test_context();
    let prepared1 = comp.prepare(&serde_json::json!({}), ctx).unwrap();
    let prepared2 = comp.prepare(&serde_json::json!({}), ctx).unwrap();

    let backend = TinySkiaBackend::new();

    // Pass A renders frame 20 after frame 0
    let _ = prepared1.render(0).unwrap();
    let scene_a = prepared1.render(20).unwrap();
    let img_a = backend
        .render_frame(&scene_a, &frame_config(320, 240, 20))
        .unwrap();

    // Pass B renders frame 20 directly without rendering frame 0 first
    let scene_b = prepared2.render(20).unwrap();
    let img_b = backend
        .render_frame(&scene_b, &frame_config(320, 240, 20))
        .unwrap();

    assert_eq!(
        img_a.into_raw(),
        img_b.into_raw(),
        "Frame 20 output must be completely invariant to prior frame evaluations"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. HISTORICAL REGRESSION FIXTURES
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_regression_zero_dimension_node_rendering() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    // Zero width and zero height rectangle should not panic or corrupt image
    scene.push(SceneNode::Rect {
        x: 10.0,
        y: 10.0,
        w: 0.0,
        h: 0.0,
        fill: Color::rgba(255, 0, 0, 255),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    });
    scene.push(SceneNode::Rect {
        x: 20.0,
        y: 20.0,
        w: 50.0,
        h: 50.0,
        fill: Color::rgba(0, 255, 0, 255),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    });

    let config = frame_config(100, 100, 0);
    let img = backend
        .render_frame(&scene, &config)
        .expect("Zero dimension rect renders safely");
    assert_eq!(img.width(), 100);
    assert_eq!(img.height(), 100);
}

#[test]
fn test_regression_out_of_bounds_clip_region() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    // Completely out-of-bounds clip region
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: Some(ClipRegion::Rect {
            x: -500.0,
            y: -500.0,
            w: 100.0,
            h: 100.0,
            corner_radius: 0.0,
        }),
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![],
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

    let config = frame_config(100, 100, 0);
    let img = backend
        .render_frame(&scene, &config)
        .expect("Out-of-bounds clip renders safely without underflow");
    // All pixels should remain completely transparent
    for pixel in img.pixels() {
        assert_eq!(
            pixel[3], 0,
            "Out-of-bounds clipped pixel must be transparent"
        );
    }
}

#[test]
fn test_regression_high_sigma_blur_on_alpha_boundary() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();
    scene.push(SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Blur { sigma: 50.0 }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 40.0,
            y: 40.0,
            w: 20.0,
            h: 20.0,
            fill: Color::rgb(255, 255, 255),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let config = frame_config(100, 100, 0);
    let img = backend
        .render_frame(&scene, &config)
        .expect("High sigma blur completes safely without numerical divergence");
    assert_eq!(img.width(), 100);
}

#[test]
fn test_regression_text_layout_zero_and_infinite_bounds() {
    let mut tb = TextBox::new("Rust Motion 1.0 Conformance Test", 0.0, 0.0, 0.0, 0.0, 16.0);
    tb.horizontal_align = TextHorizontalAlign::Center;
    tb.vertical_align = TextVerticalAlign::Center;

    let res = layout_text_box(&tb);
    // Should safely return error without panic or infinite loop
    assert!(
        res.is_err(),
        "Zero-dimension text box must return graceful error"
    );

    // Positive dimension text box layouts cleanly
    let valid_tb = TextBox::new(
        "Rust Motion 1.0 Conformance Test",
        0.0,
        0.0,
        400.0,
        100.0,
        16.0,
    );
    let valid_res =
        layout_text_box(&valid_tb).expect("Positive dimension text box layouts successfully");
    assert_eq!(valid_res.lines.len(), 1);
}
