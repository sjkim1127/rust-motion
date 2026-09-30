//! Comprehensive E2E Test Suite for Composition, Timeline & Transition Series (Tiers 1-4)
//!
//! Features covered:
//! - Feature 13: Declarative Transition Series & Overlap Calculations (`SceneTransitionSeries`, `calculate_timeline`, `TransitionTiming`)
//! - Feature 17: Seamless Looping & Trail Animations (`SceneLoop`, `SceneTrail`, `SceneSequence`, `SceneFreeze`, `SceneGroup`, `SceneLayer`)
//! - Tier 3: Pairwise cross-feature combinations
//! - Tier 4: Real-world video application scenario (Cinematic Color-Graded Video Reel)

use dioxuscut_composition::{
    NativeComposition, NativeCompositionContext, SceneEmitter, SceneEmitterComposition,
    SceneFrameContext, SceneFreeze, SceneGroup, SceneLayer, SceneLoop, SceneSequence, SceneStack,
    SceneTextBlock, SceneTrail, SceneTransitionSeries, TransitionKind, TransitionTiming,
};
use dioxuscut_rasterizer::{Color, Scene, SceneFilter, SceneNode, SceneShadow, Transform2D};
use serde_json::Value;

fn make_test_context(w: u32, h: u32, duration: u32) -> NativeCompositionContext {
    NativeCompositionContext {
        width: w,
        height: h,
        fps: 30.0,
        duration_in_frames: duration,
    }
}

fn test_rect(w: f32, h: f32, color: Color) -> SceneNode {
    SceneNode::Rect {
        x: 0.0,
        y: 0.0,
        w,
        h,
        fill: color,
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 13: TRANSITION SERIES & TIMELINE SEQUENCING
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f13_t1_transition_series_timeline_calculation() {
    // 3 clips: Clip 1 (30 frames), Clip 2 (40 frames), Clip 3 (50 frames)
    // Transition 1-2: 10 frames overlap
    // Transition 2-3: 15 frames overlap
    let series = SceneTransitionSeries::new()
        .clip(30, test_rect(100.0, 100.0, Color::rgb(255, 0, 0)))
        .transition(TransitionKind::Fade, TransitionTiming::new(10))
        .clip(40, test_rect(100.0, 100.0, Color::rgb(0, 255, 0)))
        .transition(TransitionKind::SlideLeft, TransitionTiming::new(15))
        .clip(50, test_rect(100.0, 100.0, Color::rgb(0, 0, 255)));

    let (starts, overlaps) = series.calculate_timeline();
    assert_eq!(starts.len(), 3);
    assert_eq!(overlaps.len(), 2);

    assert_eq!(starts[0], 0);
    assert_eq!(starts[1], 20);
    assert_eq!(starts[2], 45);

    assert_eq!(overlaps[0], 10);
    assert_eq!(overlaps[1], 15);
}

#[test]
fn test_f13_t1_transition_series_emission_at_overlap() {
    let series = SceneTransitionSeries::new()
        .clip(20, test_rect(100.0, 100.0, Color::rgb(255, 0, 0)))
        .transition(TransitionKind::Fade, TransitionTiming::new(10))
        .clip(20, test_rect(100.0, 100.0, Color::rgb(0, 255, 0)));

    let ctx = make_test_context(100, 100, 30);

    // Frame 15 is inside the transition window [10..20]
    let frame_ctx = SceneFrameContext::new(15, ctx);
    let mut scene = Scene::new();
    series.emit(frame_ctx, &Value::Null, &mut scene).unwrap();

    // Both incoming and outgoing clips should be emitted during overlap
    assert_eq!(scene.nodes.len(), 2);
}

#[test]
fn test_f13_t1_transition_series_total_duration() {
    let series = SceneTransitionSeries::new()
        .clip(30, test_rect(50.0, 50.0, Color::WHITE))
        .transition(TransitionKind::Fade, TransitionTiming::new(10))
        .clip(30, test_rect(50.0, 50.0, Color::WHITE));

    let (starts, _) = series.calculate_timeline();
    let total_duration = starts[1] + 30;
    assert_eq!(total_duration, 50);
}

#[test]
fn test_f13_t1_transition_kinds_all_variants() {
    let kinds = [
        TransitionKind::Fade,
        TransitionKind::SlideLeft,
        TransitionKind::SlideRight,
        TransitionKind::SlideUp,
        TransitionKind::SlideDown,
    ];

    for kind in kinds {
        let series = SceneTransitionSeries::new()
            .clip(10, test_rect(50.0, 50.0, Color::WHITE))
            .transition(kind, TransitionTiming::new(5))
            .clip(10, test_rect(50.0, 50.0, Color::WHITE));
        let (_, overlaps) = series.calculate_timeline();
        assert_eq!(overlaps[0], 5);
    }
}

#[test]
fn test_f13_t1_scene_sequence_local_frame_offset() {
    let seq =
        SceneSequence::new(20, test_rect(50.0, 50.0, Color::rgb(255, 0, 0))).with_duration(30);
    let ctx = make_test_context(100, 100, 60);

    // Frame 10: outside range -> 0 nodes
    let mut s_before = Scene::new();
    seq.emit(SceneFrameContext::new(10, ctx), &Value::Null, &mut s_before)
        .unwrap();
    assert!(s_before.nodes.is_empty());

    // Frame 25: inside range -> 1 node
    let mut s_inside = Scene::new();
    seq.emit(SceneFrameContext::new(25, ctx), &Value::Null, &mut s_inside)
        .unwrap();
    assert_eq!(s_inside.nodes.len(), 1);

    // Frame 55: outside range -> 0 nodes
    let mut s_after = Scene::new();
    seq.emit(SceneFrameContext::new(55, ctx), &Value::Null, &mut s_after)
        .unwrap();
    assert!(s_after.nodes.is_empty());
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f13_t2_transition_series_empty() {
    let series = SceneTransitionSeries::new();
    assert!(series.is_empty());
    assert_eq!(series.len(), 0);
    let (starts, overlaps) = series.calculate_timeline();
    assert!(starts.is_empty());
    assert!(overlaps.is_empty());
}

#[test]
fn test_f13_t2_transition_series_single_clip_no_transition() {
    let series =
        SceneTransitionSeries::new().clip(50, test_rect(100.0, 100.0, Color::rgb(255, 0, 0)));
    let (starts, overlaps) = series.calculate_timeline();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0], 0);
    assert!(overlaps.is_empty());
}

#[test]
fn test_f13_t2_transition_series_overlap_exceeds_clip_duration_clamped() {
    // Transition duration = 50, but clips are only 20 frames each -> overlap clamped to 20
    let series = SceneTransitionSeries::new()
        .clip(20, test_rect(50.0, 50.0, Color::rgb(255, 0, 0)))
        .transition(TransitionKind::Fade, TransitionTiming::new(50))
        .clip(20, test_rect(50.0, 50.0, Color::rgb(0, 0, 255)));

    let (starts, overlaps) = series.calculate_timeline();
    assert_eq!(overlaps[0], 20);
    assert_eq!(starts[1], 0);
}

#[test]
fn test_f13_t2_transition_series_zero_duration_clips() {
    let series = SceneTransitionSeries::new()
        .clip(0, test_rect(50.0, 50.0, Color::rgb(255, 0, 0)))
        .clip(0, test_rect(50.0, 50.0, Color::rgb(0, 0, 255)));

    let (starts, _) = series.calculate_timeline();
    assert_eq!(starts[0], 0);
    assert_eq!(starts[1], 0);
}

#[test]
fn test_f13_t2_scene_sequence_hidden_flag() {
    let seq = SceneSequence::new(0, test_rect(50.0, 50.0, Color::rgb(255, 0, 0)))
        .with_duration(30)
        .hidden(true);

    let ctx = make_test_context(100, 100, 30);
    let mut scene = Scene::new();
    seq.emit(SceneFrameContext::new(10, ctx), &Value::Null, &mut scene)
        .unwrap();
    assert!(scene.nodes.is_empty());
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 17: SEAMLESS LOOPING & TRAIL ANIMATION
// ══════════════════════════════════════════════════════════════════════════════

// ── Tier 1: Feature Coverage (≥5 tests) ───────────────────────────────────────

#[test]
fn test_f17_t1_scene_loop_times_and_modulo_frames() {
    // Loop of 20 frames duration, repeated 3 times -> active on [0..60]
    let loop_emitter = SceneLoop::with_times(20, 3, test_rect(100.0, 100.0, Color::rgb(0, 255, 0)));
    let ctx = make_test_context(100, 100, 100);

    // Frame 0: active
    let mut s0 = Scene::new();
    loop_emitter
        .emit(SceneFrameContext::new(0, ctx), &Value::Null, &mut s0)
        .unwrap();
    assert_eq!(s0.nodes.len(), 1);

    // Frame 35: active (second loop, local frame 15)
    let mut s35 = Scene::new();
    loop_emitter
        .emit(SceneFrameContext::new(35, ctx), &Value::Null, &mut s35)
        .unwrap();
    assert_eq!(s35.nodes.len(), 1);

    // Frame 65: inactive (past 3 * 20 = 60 frames)
    let mut s65 = Scene::new();
    loop_emitter
        .emit(SceneFrameContext::new(65, ctx), &Value::Null, &mut s65)
        .unwrap();
    assert!(s65.nodes.is_empty());
}

#[test]
fn test_f17_t1_scene_loop_infinite() {
    let loop_infinite = SceneLoop::new(15, test_rect(50.0, 50.0, Color::WHITE));
    let ctx = make_test_context(100, 100, 1000);

    for &frame in &[0, 100, 500, 999] {
        let mut s = Scene::new();
        loop_infinite
            .emit(SceneFrameContext::new(frame, ctx), &Value::Null, &mut s)
            .unwrap();
        assert_eq!(s.nodes.len(), 1);
    }
}

#[test]
fn test_f17_t1_scene_freeze_constant_frame() {
    let freeze = SceneFreeze::new(10, test_rect(50.0, 50.0, Color::rgb(255, 255, 0)));
    let ctx = make_test_context(100, 100, 60);

    for &frame in &[0, 20, 50] {
        let mut s = Scene::new();
        freeze
            .emit(SceneFrameContext::new(frame, ctx), &Value::Null, &mut s)
            .unwrap();
        assert_eq!(s.nodes.len(), 1);
    }
}

#[test]
fn test_f17_t1_scene_trail_copies_and_delays() {
    // 5 trailing copies with 3-frame delay each
    let trail = SceneTrail::new(5, 3, test_rect(40.0, 40.0, Color::WHITE));
    let ctx = make_test_context(100, 100, 60);

    let mut s = Scene::new();
    trail
        .emit(SceneFrameContext::new(20, ctx), &Value::Null, &mut s)
        .unwrap();
    assert_eq!(s.nodes.len(), 5);
}

#[test]
fn test_f17_t1_scene_stack_in_order_emission() {
    let mut stack = SceneStack::new();
    stack.push(test_rect(10.0, 10.0, Color::rgb(255, 0, 0)));
    stack.push(test_rect(20.0, 20.0, Color::rgb(0, 255, 0)));
    stack.push(test_rect(30.0, 30.0, Color::rgb(0, 0, 255)));

    let ctx = make_test_context(100, 100, 30);
    let mut scene = Scene::new();
    stack
        .emit(SceneFrameContext::new(0, ctx), &Value::Null, &mut scene)
        .unwrap();

    assert_eq!(scene.nodes.len(), 3);
}

// ── Tier 2: Boundary & Corner Cases (≥5 tests) ────────────────────────────────

#[test]
fn test_f17_t2_scene_loop_zero_duration_emits_nothing() {
    let loop_z = SceneLoop::new(0, test_rect(50.0, 50.0, Color::rgb(255, 0, 0)));
    let ctx = make_test_context(100, 100, 30);
    for frame in [0, 1] {
        let mut scene = Scene::new();
        assert!(loop_z
            .emit(SceneFrameContext::new(frame, ctx), &Value::Null, &mut scene)
            .is_ok());
        assert!(scene.nodes.is_empty());
    }
}

#[test]
fn test_f17_t2_scene_trail_zero_copies_clamps_to_one() {
    // layers = 0 clamped to 1
    let trail_0 = SceneTrail::new(0, 5, test_rect(50.0, 50.0, Color::rgb(255, 0, 0)));
    let ctx = make_test_context(100, 100, 30);
    let mut scene = Scene::new();
    trail_0
        .emit(SceneFrameContext::new(10, ctx), &Value::Null, &mut scene)
        .unwrap();
    assert_eq!(scene.nodes.len(), 1);
}

#[test]
fn test_f17_t2_scene_trail_lag_clamp_min_one() {
    // lag = 0 clamped to 1
    let trail = SceneTrail::new(3, 0, test_rect(20.0, 20.0, Color::WHITE));
    let ctx = make_test_context(100, 100, 30);
    let mut scene = Scene::new();
    trail
        .emit(SceneFrameContext::new(6, ctx), &Value::Null, &mut scene)
        .unwrap();
    assert_eq!(scene.nodes.len(), 3);
}

#[test]
fn test_f17_t2_scene_freeze_zero_frame() {
    let freeze = SceneFreeze::new(0, test_rect(50.0, 50.0, Color::WHITE));
    let ctx = make_test_context(100, 100, 30);
    let mut scene = Scene::new();
    freeze
        .emit(SceneFrameContext::new(0, ctx), &Value::Null, &mut scene)
        .unwrap();
    assert_eq!(scene.nodes.len(), 1);
}

#[test]
fn test_f17_t2_scene_stack_empty() {
    let stack = SceneStack::new();
    assert!(stack.is_empty());
    assert_eq!(stack.len(), 0);
    let ctx = make_test_context(100, 100, 30);
    let mut scene = Scene::new();
    assert!(stack
        .emit(SceneFrameContext::new(0, ctx), &Value::Null, &mut scene)
        .is_ok());
    assert!(scene.nodes.is_empty());
}

// ══════════════════════════════════════════════════════════════════════════════
// TIER 3: PAIRWISE CROSS-FEATURE COMBINATIONS
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_pairwise_transition_series_with_layer_filters() {
    let clip1 = SceneLayer::new(test_rect(200.0, 200.0, Color::rgb(255, 100, 0)))
        .with_filter(SceneFilter::Brightness { amount: 1.2 });

    let clip2 = SceneLayer::new(test_rect(200.0, 200.0, Color::rgb(0, 100, 255)))
        .with_filter(SceneFilter::Grayscale { amount: 0.8 });

    let series = SceneTransitionSeries::new()
        .clip(30, clip1)
        .transition(TransitionKind::Fade, TransitionTiming::new(10))
        .clip(30, clip2);

    let ctx = make_test_context(400, 400, 60);
    let mut scene = Scene::new();
    assert!(series
        .emit(SceneFrameContext::new(25, ctx), &Value::Null, &mut scene)
        .is_ok());
    assert_eq!(scene.nodes.len(), 2);
}

#[test]
fn test_pairwise_scene_loop_nested_inside_group_transform() {
    let looped_rect = SceneLoop::with_times(15, 4, test_rect(50.0, 50.0, Color::WHITE));
    let group = SceneGroup::new(looped_rect).with_transform(Transform2D::translate(100.0, 100.0));

    let ctx = make_test_context(500, 500, 60);
    let mut scene = Scene::new();
    assert!(group
        .emit(SceneFrameContext::new(20, ctx), &Value::Null, &mut scene)
        .is_ok());
    assert_eq!(scene.nodes.len(), 1);
}

#[test]
fn test_pairwise_sequence_with_scene_text_block() {
    let text_block = SceneTextBlock::new("Sequenced Headline", 50.0, 50.0, 400.0, 100.0, 28.0)
        .with_color(Color::rgb(255, 220, 0));

    let sequence = SceneSequence::new(10, text_block).with_duration(40);
    let ctx = make_test_context(600, 200, 60);

    let mut scene = Scene::new();
    assert!(sequence
        .emit(SceneFrameContext::new(20, ctx), &Value::Null, &mut scene)
        .is_ok());
    assert!(!scene.nodes.is_empty());
}

// ══════════════════════════════════════════════════════════════════════════════
// TIER 4: REAL-WORLD APPLICATION SCENARIOS
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_tier4_scenario_cinematic_color_graded_video_reel() {
    let intro_clip = SceneLayer::new(test_rect(1920.0, 1080.0, Color::rgb(18, 24, 38)))
        .with_filter(SceneFilter::Brightness { amount: 1.1 })
        .with_shadow(SceneShadow {
            offset_x: 0.0,
            offset_y: 4.0,
            blur_sigma: 10.0,
            color: Color::rgba(0, 0, 0, 160),
        });

    let main_clip = SceneLayer::new(test_rect(1920.0, 1080.0, Color::rgb(30, 41, 59)))
        .with_filter(SceneFilter::Grayscale { amount: 0.2 })
        .with_opacity(0.95);

    let outro_clip = SceneLayer::new(test_rect(1920.0, 1080.0, Color::rgb(15, 23, 42)))
        .with_filter(SceneFilter::Opacity { amount: 0.9 });

    let reel_series = SceneTransitionSeries::new()
        .clip(45, intro_clip)
        .transition(TransitionKind::Fade, TransitionTiming::new(15))
        .clip(60, main_clip)
        .transition(TransitionKind::SlideRight, TransitionTiming::new(20))
        .clip(45, outro_clip);

    let ctx = make_test_context(1920, 1080, 150);
    let composition = SceneEmitterComposition::new("cinematic_reel", reel_series);

    // Active timeline is [0..115] (45 + 60 + 45 - 15 - 20 = 115)
    for frame in [0, 20, 40, 70, 95, 110] {
        let scene = composition.render(frame, &Value::Null, ctx).unwrap();
        assert!(
            !scene.nodes.is_empty(),
            "Frame {frame} must emit valid scene nodes"
        );
    }
}
