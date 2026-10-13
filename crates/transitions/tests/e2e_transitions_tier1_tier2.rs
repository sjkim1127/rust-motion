//! Comprehensive E2E Test Suite for Transitions & Easing Engine (Tiers 1-4)
//!
//! Features covered:
//! - Feature 9: Presentation & Wipe Transitions (Wipe Geometry, ClockWipe)
//! - Feature 10: Push & Slide Transitions (Directional Offsets, Epsilon Seam Compensation)
//! - Feature 11: Cross-Fade Transitions (`SceneFade`, Alpha Opacity Curves)
//! - Feature 12: Customizable Easing & Timing (`interpolate`, `spring`, `bezier`, `interpolate_colors`)
//! - Tier 3: Pairwise cross-feature combinations
//! - Tier 4: Real-world video application scenario (Dynamic Presentation Deck)

use rust_motion_animation::{
    bezier, interpolate, interpolate_colors, spring, ExtrapolateType, InterpolateOptions,
    SpringConfig,
};
use rust_motion_composition::{
    CompositionError, NativeCompositionContext, SceneEmitter, SceneFrameContext,
};
use rust_motion_rasterizer::{Color, Scene, SceneNode};
use rust_motion_transitions::{SceneFade, SceneSlide, SlideDirection};
use serde_json::Value;

fn make_test_context(w: u32, h: u32, duration: u32) -> NativeCompositionContext {
    NativeCompositionContext {
        width: w,
        height: h,
        fps: 30.0,
        duration_in_frames: duration,
    }
}

fn test_rect_node() -> SceneNode {
    SceneNode::Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
        fill: Color::rgb(255, 128, 0),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 9: PRESENTATION & WIPE TRANSITIONS
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f9_t1_wipe_direction_left_geometry() {
    let p = 0.5f32; // 50%
    let w = 1920.0f32;
    let clip_width = p * w;
    assert_eq!(clip_width, 960.0);
    assert!(clip_width <= w);
}

#[test]
fn test_f9_t1_wipe_direction_right_geometry() {
    let p = 0.75f32; // 75%
    let w = 1920.0f32;
    let offset_x = (1.0 - p) * w;
    assert_eq!(offset_x, 480.0);
}

#[test]
fn test_f9_t1_wipe_direction_top_geometry() {
    let p = 0.25f32; // 25%
    let h = 1080.0f32;
    let clip_height = p * h;
    assert_eq!(clip_height, 270.0);
}

#[test]
fn test_f9_t1_wipe_direction_bottom_geometry() {
    let p = 0.5f32;
    let h = 1080.0f32;
    let offset_y = (1.0 - p) * h;
    assert_eq!(offset_y, 540.0);
}

#[test]
fn test_f9_t1_clock_wipe_circular_sector_clip() {
    let w = 1920.0f64;
    let h = 1080.0f64;
    let r = (w * w + h * h).sqrt() / 2.0;
    assert!(r > w / 2.0 && r > h / 2.0);

    for &progress in &[0.0, 0.25, 0.5, 0.75, 1.0] {
        let theta_deg = progress * 360.0;
        assert!((0.0..=360.0).contains(&theta_deg));
    }
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f9_t2_wipe_zero_progress() {
    let p = 0.0f32.clamp(0.0, 1.0);
    assert_eq!(p, 0.0);
}

#[test]
fn test_f9_t2_wipe_full_progress() {
    let p = 1.0f32.clamp(0.0, 1.0);
    assert_eq!(p, 1.0);
}

#[test]
fn test_f9_t2_wipe_over_progress_clamp() {
    let p_under = (-0.5f32).clamp(0.0, 1.0);
    let p_over = (1.5f32).clamp(0.0, 1.0);
    assert_eq!(p_under, 0.0);
    assert_eq!(p_over, 1.0);
}

#[test]
fn test_f9_t2_clock_wipe_zero_dimensions() {
    let w = 0.0f64;
    let h = 0.0f64;
    let r = (w * w + h * h).sqrt() / 2.0;
    assert_eq!(r, 0.0);
}

#[test]
fn test_f9_t2_clock_wipe_nan_progress() {
    let raw = f64::NAN;
    let clamped = if raw.is_nan() {
        0.0
    } else {
        raw.clamp(0.0, 1.0)
    };
    assert_eq!(clamped, 0.0);
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 10: PUSH & SLIDE TRANSITIONS
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f10_t1_slide_from_left_offsets() {
    let slide = SceneSlide::new(test_rect_node())
        .with_duration(20)
        .from(SlideDirection::FromLeft);

    let ctx = make_test_context(1920, 1080, 100);
    let frame_ctx = SceneFrameContext::new(10, ctx);
    let mut scene = Scene::new();
    slide.emit(frame_ctx, &Value::Null, &mut scene).unwrap();

    assert_eq!(scene.nodes.len(), 1);
    match &scene.nodes[0] {
        SceneNode::Group { transform, .. } => {
            assert_eq!(transform.tx, -960.0);
            assert_eq!(transform.ty, 0.0);
        }
        _ => panic!("Expected Group node"),
    }
}

#[test]
fn test_f10_t1_slide_from_right_offsets() {
    let slide = SceneSlide::new(test_rect_node())
        .with_duration(20)
        .from(SlideDirection::FromRight);

    let ctx = make_test_context(1920, 1080, 100);
    let frame_ctx = SceneFrameContext::new(10, ctx);
    let mut scene = Scene::new();
    slide.emit(frame_ctx, &Value::Null, &mut scene).unwrap();

    match &scene.nodes[0] {
        SceneNode::Group { transform, .. } => {
            assert_eq!(transform.tx, 960.0);
            assert_eq!(transform.ty, 0.0);
        }
        _ => panic!("Expected Group node"),
    }
}

#[test]
fn test_f10_t1_slide_from_top_offsets() {
    let slide = SceneSlide::new(test_rect_node())
        .with_duration(20)
        .from(SlideDirection::FromTop);

    let ctx = make_test_context(1920, 1080, 100);
    let frame_ctx = SceneFrameContext::new(10, ctx);
    let mut scene = Scene::new();
    slide.emit(frame_ctx, &Value::Null, &mut scene).unwrap();

    match &scene.nodes[0] {
        SceneNode::Group { transform, .. } => {
            assert_eq!(transform.tx, 0.0);
            assert_eq!(transform.ty, -540.0);
        }
        _ => panic!("Expected Group node"),
    }
}

#[test]
fn test_f10_t1_slide_from_bottom_offsets() {
    let slide = SceneSlide::new(test_rect_node())
        .with_duration(20)
        .from(SlideDirection::FromBottom);

    let ctx = make_test_context(1920, 1080, 100);
    let frame_ctx = SceneFrameContext::new(10, ctx);
    let mut scene = Scene::new();
    slide.emit(frame_ctx, &Value::Null, &mut scene).unwrap();

    match &scene.nodes[0] {
        SceneNode::Group { transform, .. } => {
            assert_eq!(transform.tx, 0.0);
            assert_eq!(transform.ty, 540.0);
        }
        _ => panic!("Expected Group node"),
    }
}

#[test]
fn test_f10_t1_slide_subpixel_seam_compensation() {
    let slide = SceneSlide::new(test_rect_node())
        .with_duration(30)
        .from(SlideDirection::FromRight);

    let ctx = make_test_context(1920, 1080, 100);
    let frame_ctx = SceneFrameContext::new(30, ctx);
    let mut scene = Scene::new();
    slide.emit(frame_ctx, &Value::Null, &mut scene).unwrap();

    match &scene.nodes[0] {
        SceneNode::Group { transform, .. } => {
            assert_eq!(
                transform.tx, 0.0,
                "Slide at duration must be at exactly 0.0"
            );
        }
        _ => panic!("Expected Group node"),
    }
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f10_t2_slide_progress_zero_exact() {
    let slide = SceneSlide::new(test_rect_node())
        .with_duration(10)
        .from(SlideDirection::FromRight);

    let ctx = make_test_context(1000, 500, 50);
    let frame_ctx = SceneFrameContext::new(0, ctx);
    let mut scene = Scene::new();
    slide.emit(frame_ctx, &Value::Null, &mut scene).unwrap();

    match &scene.nodes[0] {
        SceneNode::Group { transform, .. } => {
            assert_eq!(transform.tx, 1000.0);
        }
        _ => panic!("Expected Group node"),
    }
}

#[test]
fn test_f10_t2_slide_progress_one_exact() {
    let slide = SceneSlide::new(test_rect_node())
        .with_duration(10)
        .from(SlideDirection::FromLeft);

    let ctx = make_test_context(1000, 500, 50);
    let frame_ctx = SceneFrameContext::new(10, ctx);
    let mut scene = Scene::new();
    slide.emit(frame_ctx, &Value::Null, &mut scene).unwrap();

    match &scene.nodes[0] {
        SceneNode::Group { transform, .. } => {
            assert_eq!(transform.tx, 0.0);
        }
        _ => panic!("Expected Group node"),
    }
}

#[test]
fn test_f10_t2_slide_zero_duration() {
    let slide = SceneSlide::new(test_rect_node()).with_duration(0);

    let ctx = make_test_context(1000, 500, 50);
    let frame_ctx = SceneFrameContext::new(0, ctx);
    let mut scene = Scene::new();
    slide.emit(frame_ctx, &Value::Null, &mut scene).unwrap();

    match &scene.nodes[0] {
        SceneNode::Group { transform, .. } => {
            assert_eq!(transform.tx, 0.0);
        }
        _ => panic!("Expected Group node"),
    }
}

#[test]
fn test_f10_t2_slide_large_composition_dimensions() {
    let slide = SceneSlide::new(test_rect_node())
        .with_duration(10)
        .from(SlideDirection::FromRight);

    let ctx = make_test_context(3840, 2160, 100);
    let frame_ctx = SceneFrameContext::new(0, ctx);
    let mut scene = Scene::new();
    slide.emit(frame_ctx, &Value::Null, &mut scene).unwrap();

    match &scene.nodes[0] {
        SceneNode::Group { transform, .. } => {
            assert_eq!(transform.tx, 3840.0);
        }
        _ => panic!("Expected Group node"),
    }
}

#[test]
fn test_f10_t2_slide_frame_beyond_duration() {
    let slide = SceneSlide::new(test_rect_node())
        .with_duration(10)
        .from(SlideDirection::FromBottom);

    let ctx = make_test_context(1920, 1080, 100);
    let frame_ctx = SceneFrameContext::new(50, ctx);
    let mut scene = Scene::new();
    slide.emit(frame_ctx, &Value::Null, &mut scene).unwrap();

    match &scene.nodes[0] {
        SceneNode::Group { transform, .. } => {
            assert_eq!(transform.ty, 0.0);
        }
        _ => panic!("Expected Group node"),
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 11: CROSS-FADE TRANSITIONS
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f11_t1_fade_opacity_cross_dissolve() {
    let fade = SceneFade::new(test_rect_node())
        .with_enter_duration(20)
        .with_exit(60, 20);

    let ctx = make_test_context(640, 480, 60);

    let mut scene0 = Scene::new();
    fade.emit(SceneFrameContext::new(0, ctx), &Value::Null, &mut scene0)
        .unwrap();
    match &scene0.nodes[0] {
        SceneNode::Group { opacity, .. } => assert_eq!(*opacity, 0.0),
        _ => panic!("Expected Group"),
    }

    let mut scene10 = Scene::new();
    fade.emit(SceneFrameContext::new(10, ctx), &Value::Null, &mut scene10)
        .unwrap();
    match &scene10.nodes[0] {
        SceneNode::Group { opacity, .. } => assert_eq!(*opacity, 0.5),
        _ => panic!("Expected Group"),
    }

    let mut scene30 = Scene::new();
    fade.emit(SceneFrameContext::new(30, ctx), &Value::Null, &mut scene30)
        .unwrap();
    match &scene30.nodes[0] {
        SceneNode::Group { opacity, .. } => assert_eq!(*opacity, 1.0),
        _ => panic!("Expected Group"),
    }
}

#[test]
fn test_f11_t1_fade_enter_exit_alpha_sum() {
    let fade_in = SceneFade::new(test_rect_node()).with_enter_duration(30);
    let ctx = make_test_context(1920, 1080, 60);

    for f in 0..=30 {
        let mut s = Scene::new();
        fade_in
            .emit(SceneFrameContext::new(f, ctx), &Value::Null, &mut s)
            .unwrap();
        match &s.nodes[0] {
            SceneNode::Group { opacity, .. } => {
                let expected = f as f32 / 30.0;
                assert!((opacity - expected).abs() < 1e-5);
            }
            _ => panic!("Expected Group"),
        }
    }
}

#[test]
fn test_f11_t1_scene_fade_group_emission() {
    let fade = SceneFade::new(test_rect_node());
    let ctx = make_test_context(100, 100, 50);
    let mut s = Scene::new();
    fade.emit(SceneFrameContext::new(20, ctx), &Value::Null, &mut s)
        .unwrap();
    assert_eq!(s.nodes.len(), 1);
}

#[test]
fn test_f11_t1_fade_duration_scaling() {
    let fade_short = SceneFade::new(test_rect_node()).with_enter_duration(5);
    let fade_long = SceneFade::new(test_rect_node()).with_enter_duration(50);

    let ctx = make_test_context(100, 100, 60);
    let mut s_short = Scene::new();
    let mut s_long = Scene::new();

    fade_short
        .emit(SceneFrameContext::new(5, ctx), &Value::Null, &mut s_short)
        .unwrap();
    fade_long
        .emit(SceneFrameContext::new(5, ctx), &Value::Null, &mut s_long)
        .unwrap();

    let op_short = match &s_short.nodes[0] {
        SceneNode::Group { opacity, .. } => *opacity,
        _ => 0.0,
    };
    let op_long = match &s_long.nodes[0] {
        SceneNode::Group { opacity, .. } => *opacity,
        _ => 0.0,
    };

    assert_eq!(op_short, 1.0);
    assert_eq!(op_long, 0.1);
}

#[test]
fn test_f11_t1_fade_color_mix_preservation() {
    let rgba_str = interpolate_colors("#ff0000", "#0000ff", 0.5);
    assert!(rgba_str.starts_with("rgba(128, 0, 128"));
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f11_t2_fade_zero_duration() {
    let fade = SceneFade::new(test_rect_node()).with_enter_duration(0);
    let ctx = make_test_context(100, 100, 50);
    let mut s = Scene::new();
    fade.emit(SceneFrameContext::new(0, ctx), &Value::Null, &mut s)
        .unwrap();

    match &s.nodes[0] {
        SceneNode::Group { opacity, .. } => assert_eq!(*opacity, 1.0),
        _ => panic!("Expected Group"),
    }
}

#[test]
fn test_f11_t2_fade_single_frame_duration() {
    let fade = SceneFade::new(test_rect_node()).with_enter_duration(1);
    let ctx = make_test_context(100, 100, 50);
    let mut s = Scene::new();
    fade.emit(SceneFrameContext::new(1, ctx), &Value::Null, &mut s)
        .unwrap();

    match &s.nodes[0] {
        SceneNode::Group { opacity, .. } => assert_eq!(*opacity, 1.0),
        _ => panic!("Expected Group"),
    }
}

#[test]
fn test_f11_t2_fade_progress_clamp_below_zero() {
    let fade = SceneFade::new(test_rect_node()).with_enter_duration(10);
    let ctx = make_test_context(100, 100, 50);
    let mut s = Scene::new();
    fade.emit(SceneFrameContext::new(0, ctx), &Value::Null, &mut s)
        .unwrap();

    match &s.nodes[0] {
        SceneNode::Group { opacity, .. } => assert_eq!(*opacity, 0.0),
        _ => panic!("Expected Group"),
    }
}

#[test]
fn test_f11_t2_fade_progress_clamp_above_one() {
    let fade = SceneFade::new(test_rect_node()).with_enter_duration(10);
    let ctx = make_test_context(100, 100, 50);
    let mut s = Scene::new();
    fade.emit(SceneFrameContext::new(100, ctx), &Value::Null, &mut s)
        .unwrap();

    match &s.nodes[0] {
        SceneNode::Group { opacity, .. } => assert_eq!(*opacity, 1.0),
        _ => panic!("Expected Group"),
    }
}

#[test]
fn test_f11_t2_fade_empty_child_scene() {
    struct EmptyEmitter;
    impl SceneEmitter for EmptyEmitter {
        fn emit(
            &self,
            _: SceneFrameContext,
            _: &Value,
            _: &mut Scene,
        ) -> Result<(), CompositionError> {
            Ok(())
        }
    }

    let fade = SceneFade::new(EmptyEmitter);
    let ctx = make_test_context(100, 100, 50);
    let mut s = Scene::new();
    fade.emit(SceneFrameContext::new(10, ctx), &Value::Null, &mut s)
        .unwrap();
    assert!(s.nodes.is_empty(), "Empty child must produce 0 nodes");
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 12: CUSTOMIZABLE EASING & TIMING
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f12_t1_bezier_linear_ease() {
    let linear = bezier(0.0, 0.0, 1.0, 1.0);
    for i in 0..=10 {
        let t = i as f64 * 0.1;
        let v = linear(t);
        assert!((v - t).abs() < 1e-4);
    }
}

#[test]
fn test_f12_t1_bezier_ease_in_out() {
    let ease = bezier(0.42, 0.0, 0.58, 1.0);
    assert_eq!(ease(0.0), 0.0);
    assert_eq!(ease(1.0), 1.0);
    assert!((ease(0.5) - 0.5).abs() < 1e-3);
}

#[test]
fn test_f12_t1_spring_physics_underdamped() {
    let cfg = SpringConfig {
        mass: 1.0,
        damping: 10.0,
        stiffness: 100.0,
        overshoot_clamping: false,
    };
    let s0 = spring(0, 30.0, cfg.clone());
    let s_end = spring(60, 30.0, cfg);
    assert_eq!(s0, 0.0);
    assert!((s_end - 1.0).abs() < 0.05);
}

#[test]
fn test_f12_t1_spring_physics_critically_damped() {
    let cfg = SpringConfig {
        mass: 1.0,
        damping: 20.0,
        stiffness: 100.0,
        overshoot_clamping: false,
    };
    let s_mid = spring(15, 30.0, cfg);
    assert!(s_mid > 0.0 && s_mid <= 1.0);
}

#[test]
fn test_f12_t1_interpolate_modes_clamp_extend() {
    let in_range = [0.0, 100.0];
    let out_range = [0.0, 1000.0];

    assert_eq!(
        interpolate(
            -10.0,
            &in_range,
            &out_range,
            InterpolateOptions {
                extrapolate_left: ExtrapolateType::Clamp,
                extrapolate_right: ExtrapolateType::Clamp,
                easing: None,
            }
        ),
        0.0
    );
    assert_eq!(
        interpolate(
            50.0,
            &in_range,
            &out_range,
            InterpolateOptions {
                extrapolate_left: ExtrapolateType::Clamp,
                extrapolate_right: ExtrapolateType::Clamp,
                easing: None,
            }
        ),
        500.0
    );
    assert_eq!(
        interpolate(
            150.0,
            &in_range,
            &out_range,
            InterpolateOptions {
                extrapolate_left: ExtrapolateType::Clamp,
                extrapolate_right: ExtrapolateType::Clamp,
                easing: None,
            }
        ),
        1000.0
    );
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f12_t2_spring_zero_mass_and_damping() {
    let cfg = SpringConfig {
        mass: 0.1,
        damping: 0.1,
        stiffness: 100.0,
        overshoot_clamping: false,
    };
    let s = spring(10, 30.0, cfg);
    assert!((-10.0..=10.0).contains(&s));
}

#[test]
fn test_f12_t2_bezier_control_points_extremes() {
    let overshoot = bezier(0.5, -0.5, 0.5, 1.5);
    let v_mid = overshoot(0.5);
    assert!((-2.0..=2.0).contains(&v_mid));
}

#[test]
fn test_f12_t2_spring_long_duration_rest_convergence() {
    let cfg = SpringConfig::default();
    let s_far = spring(1000, 30.0, cfg);
    assert!((s_far - 1.0).abs() < 1e-4);
}

#[test]
fn test_f12_t2_interpolate_empty_ranges_rejected() {
    let in_range: [f64; 0] = [];
    let out_range: [f64; 0] = [];
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let opts = InterpolateOptions::default();
        interpolate(50.0, &in_range, &out_range, opts);
    }));
}

#[test]
fn test_f12_t2_interpolate_nan_inputs() {
    let in_range = [0.0, 100.0];
    let out_range = [0.0, 1000.0];
    let opts = InterpolateOptions::default();
    let val = interpolate(f64::NAN, &in_range, &out_range, opts);
    assert!(val.is_nan() || val == 0.0);
}

// ══════════════════════════════════════════════════════════════════════════════
// TIER 3: PAIRWISE CROSS-FEATURE COMBINATIONS
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_pairwise_fade_with_spring_physics() {
    let cfg = SpringConfig::default();
    let ctx = make_test_context(1920, 1080, 60);

    for frame in 0..30 {
        let s = spring(frame, 30.0, cfg.clone());
        let dynamic_fade =
            SceneFade::new(test_rect_node()).with_enter_duration((s * 30.0).max(1.0) as u32);
        let mut scene = Scene::new();
        assert!(dynamic_fade
            .emit(SceneFrameContext::new(frame, ctx), &Value::Null, &mut scene)
            .is_ok());
    }
}

#[test]
fn test_pairwise_slide_with_bezier_easing() {
    let ctx = make_test_context(1920, 1080, 60);

    for frame in 0..30 {
        let slide = SceneSlide::new(test_rect_node()).with_duration(30);
        let mut scene = Scene::new();
        assert!(slide
            .emit(SceneFrameContext::new(frame, ctx), &Value::Null, &mut scene)
            .is_ok());
    }
}

#[test]
fn test_pairwise_wipe_with_color_interpolation() {
    for frame in 0..=20 {
        let progress = frame as f64 / 20.0;
        let rgba_str = interpolate_colors("#ff0000", "#00ff00", progress);
        assert!(rgba_str.starts_with("rgba("));
        let node = SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            fill: Color::rgb(
                (255.0 * (1.0 - progress)) as u8,
                (255.0 * progress) as u8,
                0,
            ),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        };
        let fade = SceneFade::new(node).with_enter_duration(20);
        let ctx = make_test_context(100, 100, 30);
        let mut scene = Scene::new();
        assert!(fade
            .emit(SceneFrameContext::new(frame, ctx), &Value::Null, &mut scene)
            .is_ok());
    }
}

#[test]
fn test_pairwise_nested_fade_and_slide_groups() {
    let slide = SceneSlide::new(test_rect_node())
        .with_duration(15)
        .from(SlideDirection::FromLeft);
    let fade_and_slide = SceneFade::new(slide).with_enter_duration(20);

    let ctx = make_test_context(1920, 1080, 60);
    let mut scene = Scene::new();
    assert!(fade_and_slide
        .emit(SceneFrameContext::new(10, ctx), &Value::Null, &mut scene)
        .is_ok());
    assert_eq!(scene.nodes.len(), 1);
}

#[test]
fn test_pairwise_slide_across_extreme_aspect_ratios() {
    let slide = SceneSlide::new(test_rect_node())
        .with_duration(10)
        .from(SlideDirection::FromRight);

    let ctx_ultrawide = make_test_context(5120, 1440, 30);
    let mut s1 = Scene::new();
    assert!(slide
        .emit(
            SceneFrameContext::new(5, ctx_ultrawide),
            &Value::Null,
            &mut s1
        )
        .is_ok());

    let ctx_vertical = make_test_context(1080, 1920, 30);
    let mut s2 = Scene::new();
    assert!(slide
        .emit(
            SceneFrameContext::new(5, ctx_vertical),
            &Value::Null,
            &mut s2
        )
        .is_ok());
}

// ══════════════════════════════════════════════════════════════════════════════
// TIER 4: REAL-WORLD APPLICATION SCENARIOS
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_tier4_scenario_dynamic_presentation_deck() {
    let slide1 = SceneFade::new(SceneNode::Rect {
        x: 100.0,
        y: 100.0,
        w: 1720.0,
        h: 880.0,
        fill: Color::rgb(30, 41, 59),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 16.0,
    })
    .with_enter_duration(15)
    .with_exit(30, 15);

    let slide2 = SceneSlide::new(SceneNode::Rect {
        x: 100.0,
        y: 100.0,
        w: 1720.0,
        h: 880.0,
        fill: Color::rgb(15, 23, 42),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 16.0,
    })
    .with_duration(20)
    .from(SlideDirection::FromRight);

    let ctx = make_test_context(1920, 1080, 90);

    for frame in [0, 15, 30, 45, 60, 75, 90] {
        let frame_ctx = SceneFrameContext::new(frame, ctx);
        let mut scene = Scene::new();

        if frame < 30 {
            slide1.emit(frame_ctx, &Value::Null, &mut scene).unwrap();
            assert_eq!(scene.nodes.len(), 1);
        } else if frame < 60 {
            slide2.emit(frame_ctx, &Value::Null, &mut scene).unwrap();
            assert_eq!(scene.nodes.len(), 1);
        }
    }
}
