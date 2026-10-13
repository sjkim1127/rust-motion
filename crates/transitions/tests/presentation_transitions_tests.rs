use rust_motion_composition::{
    FlipDirection, LinearWipeDirection, NativeCompositionContext, SceneEmitter, SceneFrameContext,
    SceneRect, SceneTransitionSeries, TransitionKind, TransitionTiming,
};
use rust_motion_rasterizer::{Color, Scene, SceneNode};
use rust_motion_transitions::{
    bezier, ease, ease_in, ease_in_out, ease_out, ClockWipe, Dissolve, Flip,
    FlipDirection as FlipDir, Iris, LinearTiming, LinearWipe, SceneClockWipe, SceneDissolve,
    SceneFlip, SceneIris, SceneLinearWipe, SceneZoom, SpringConfig, SpringTiming,
    TransitionContext, TransitionPresentation, TransitionTiming as TimingTrait, WipeDirection,
    Zoom, ZoomMode,
};
use serde_json::Value;

fn test_ctx(w: u32, h: u32) -> NativeCompositionContext {
    NativeCompositionContext {
        width: w,
        height: h,
        fps: 30.0,
        duration_in_frames: 100,
    }
}

#[test]
fn test_clock_wipe_presentation_path_and_emitter() {
    let wipe = ClockWipe::default();
    let ctx = TransitionContext::new(0.5, 200.0, 100.0, 10, 20);
    let visual = wipe.render_entering(&ctx);
    assert!(visual.clip.is_some(), "ClockWipe must generate clip path");

    let path_zero = wipe.build_clip_path(200.0, 100.0, 0.0);
    assert_eq!(path_zero, "M 0 0 Z");

    let path_full = wipe.build_clip_path(200.0, 100.0, 1.0);
    assert!(path_full.contains("200"));

    // SceneClockWipe emitter test
    let rect = SceneRect::new(0.0, 0.0, 100.0, 100.0, Color::WHITE);
    let emitter = SceneClockWipe::new(rect).with_duration(20);
    let mut scene = Scene::new();
    emitter
        .emit(
            SceneFrameContext::new(10, test_ctx(200, 100)),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 1);
    assert!(matches!(
        &scene.nodes[0],
        SceneNode::Layer { clip: Some(_), .. }
    ));
}

#[test]
fn test_linear_wipe_presentation_8_directions() {
    let directions = [
        WipeDirection::FromLeft,
        WipeDirection::FromRight,
        WipeDirection::FromTop,
        WipeDirection::FromBottom,
        WipeDirection::FromTopLeft,
        WipeDirection::FromTopRight,
        WipeDirection::FromBottomLeft,
        WipeDirection::FromBottomRight,
    ];

    for dir in directions {
        let wipe = LinearWipe::new(dir);
        let ctx = TransitionContext::new(0.5, 400.0, 300.0, 10, 20);
        let visual = wipe.render_entering(&ctx);
        assert!(visual.clip.is_some());
    }

    // SceneLinearWipe emitter test
    let rect = SceneRect::new(0.0, 0.0, 50.0, 50.0, Color::WHITE);
    let emitter = SceneLinearWipe::new(rect)
        .with_duration(20)
        .with_direction(WipeDirection::FromLeft);
    let mut scene = Scene::new();
    emitter
        .emit(
            SceneFrameContext::new(10, test_ctx(100, 100)),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 1);
    assert!(matches!(
        &scene.nodes[0],
        SceneNode::Layer { clip: Some(_), .. }
    ));
}

#[test]
fn test_flip_presentation_and_backface_culling() {
    let flip = Flip::new(FlipDir::FromRight);

    // At progress 0.25 (first half):
    // entering is backface-culled (opacity 0)
    let ctx_early = TransitionContext::new(0.25, 200.0, 100.0, 5, 20);
    let entering_early = flip.render_entering(&ctx_early);
    assert_eq!(entering_early.opacity, 0.0);
    // exiting is rotating (opacity 1.0, scale < 1.0)
    let exiting_early = flip.render_exiting(&ctx_early);
    assert_eq!(exiting_early.opacity, 1.0);
    assert!(exiting_early.transform.scale_x < 1.0);

    // At progress 0.75 (second half):
    // exiting is backface-culled (opacity 0)
    let ctx_late = TransitionContext::new(0.75, 200.0, 100.0, 15, 20);
    let exiting_late = flip.render_exiting(&ctx_late);
    assert_eq!(exiting_late.opacity, 0.0);
    // entering is visible (opacity 1.0, scale > 0.0)
    let entering_late = flip.render_entering(&ctx_late);
    assert_eq!(entering_late.opacity, 1.0);
    assert!(entering_late.transform.scale_x > 0.0);

    // SceneFlip emitter test
    let rect = SceneRect::new(0.0, 0.0, 50.0, 50.0, Color::WHITE);
    let emitter = SceneFlip::new(rect).with_duration(20);
    let mut scene = Scene::new();
    emitter
        .emit(
            SceneFrameContext::new(15, test_ctx(200, 100)),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 1);
}

#[test]
fn test_zoom_presentation_modes() {
    let zoom_in = Zoom::new(ZoomMode::In);
    let ctx = TransitionContext::new(0.5, 200.0, 100.0, 10, 20);
    let v_in = zoom_in.render_entering(&ctx);
    assert!((v_in.transform.scale_x - 0.5).abs() < 1e-4);
    assert!((v_in.opacity - 0.5).abs() < 1e-4);

    let zoom_out = Zoom::new(ZoomMode::Out).with_max_scale(2.0);
    let v_out = zoom_out.render_entering(&ctx);
    assert!((v_out.transform.scale_x - 1.5).abs() < 1e-4);

    // SceneZoom emitter test
    let rect = SceneRect::new(0.0, 0.0, 50.0, 50.0, Color::WHITE);
    let emitter = SceneZoom::new(rect).with_duration(20);
    let mut scene = Scene::new();
    emitter
        .emit(
            SceneFrameContext::new(10, test_ctx(200, 100)),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 1);
    assert!(matches!(&scene.nodes[0], SceneNode::Group { .. }));
}

#[test]
fn test_iris_presentation_and_emitter() {
    let iris = Iris::new();
    let ctx = TransitionContext::new(0.5, 100.0, 100.0, 10, 20);
    let visual = iris.render_entering(&ctx);
    assert!(visual.clip.is_some());

    let rect = SceneRect::new(0.0, 0.0, 50.0, 50.0, Color::WHITE);
    let emitter = SceneIris::new(rect).with_duration(20);
    let mut scene = Scene::new();
    emitter
        .emit(
            SceneFrameContext::new(10, test_ctx(100, 100)),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 1);
}

#[test]
fn test_dissolve_presentation_and_emitter() {
    let dissolve = Dissolve::new();
    let ctx = TransitionContext::new(0.4, 100.0, 100.0, 8, 20);
    let v_enter = dissolve.render_entering(&ctx);
    assert!((v_enter.opacity - 0.4).abs() < 1e-4);
    let v_exit = dissolve.render_exiting(&ctx);
    assert!((v_exit.opacity - 0.6).abs() < 1e-4);

    let rect = SceneRect::new(0.0, 0.0, 50.0, 50.0, Color::WHITE);
    let emitter = SceneDissolve::new(rect).with_duration(20);
    let mut scene = Scene::new();
    emitter
        .emit(
            SceneFrameContext::new(8, test_ctx(100, 100)),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 1);
}

#[test]
fn test_timing_and_easing_curves() {
    let bezier_curve = bezier(0.25, 0.1, 0.25, 1.0);
    assert!((bezier_curve(0.0)).abs() < 1e-6);
    assert!((bezier_curve(1.0) - 1.0).abs() < 1e-6);
    assert!(bezier_curve(0.5) > 0.0 && bezier_curve(0.5) < 1.0);

    let _ = ease();
    let _ = ease_in();
    let _ = ease_out();
    let _ = ease_in_out();

    let linear_t = LinearTiming::new(20);
    assert_eq!(linear_t.duration_in_frames(), 20);
    assert!((linear_t.progress(10) - 0.5).abs() < 1e-5);

    let spring_t = SpringTiming::new(30.0, 30).with_config(SpringConfig::default());
    assert_eq!(spring_t.duration_in_frames(), 30);
    assert!(spring_t.progress(30) > 0.95);
}

#[test]
fn test_transition_series_with_new_transitions() {
    let r1 = SceneRect::new(0.0, 0.0, 100.0, 100.0, Color::WHITE);
    let r2 = SceneRect::new(0.0, 0.0, 100.0, 100.0, Color::BLACK);
    let r3 = SceneRect::new(0.0, 0.0, 100.0, 100.0, Color::rgb(255, 0, 0));
    let r4 = SceneRect::new(0.0, 0.0, 100.0, 100.0, Color::rgb(0, 255, 0));

    let series = SceneTransitionSeries::new()
        .clip(30, r1)
        .transition(TransitionKind::ClockWipe, TransitionTiming::new(10))
        .clip(30, r2)
        .transition(
            TransitionKind::LinearWipe(LinearWipeDirection::FromTopLeft),
            TransitionTiming::new(10),
        )
        .clip(30, r3)
        .transition(
            TransitionKind::Flip(FlipDirection::FromRight),
            TransitionTiming::new(10),
        )
        .clip(30, r4);

    let (starts, overlaps) = series.calculate_timeline();
    assert_eq!(overlaps, vec![10, 10, 10]);
    assert_eq!(starts, vec![0, 20, 40, 60]);
    assert_eq!(series.total_duration(), 90);

    // Render at ClockWipe overlap frame 25
    let mut scene = Scene::new();
    series
        .emit(
            SceneFrameContext::new(25, test_ctx(200, 200)),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 2);
}
