use super::*;
use crate::{NativeComposition, NativeCompositionContext};
use rust_motion_rasterizer::{
    BlendMode, ClipRegion, Color, ImageFit, Scene, SceneFilter, SceneNode, SceneShadow,
    TextHorizontalAlign, TextOverflow, TextVerticalAlign,
};
use serde_json::Value;

fn context() -> NativeCompositionContext {
    NativeCompositionContext {
        width: 320,
        height: 180,
        fps: 30.0,
        duration_in_frames: 90,
    }
}

fn frame_text() -> impl SceneEmitter {
    |context: SceneFrameContext, _props: &Value, scene: &mut Scene| {
        scene.push(SceneNode::Text {
            x: 0.0,
            y: 20.0,
            content: context.frame.to_string(),
            font_size: 20.0,
            color: Color::WHITE,
            font_weight: 400,
            font_sources: Vec::new(),
        });
        Ok(())
    }
}

#[test]
fn sequence_uses_local_frames_and_duration() {
    let composition = SceneEmitterComposition::new(
        "sequence",
        SceneSequence::new(10, frame_text()).with_duration(5),
    );
    let active = composition.render(12, &Value::Null, context()).unwrap();
    let inactive = composition.render(15, &Value::Null, context()).unwrap();

    assert!(matches!(
        &active.nodes[0],
        SceneNode::Text { content, .. } if content == "2"
    ));
    assert!(inactive.nodes.is_empty());
}

#[test]
fn series_keeps_later_entries_outside_the_u32_frame_domain() {
    let series = SceneSeries::new()
        .sequence(u32::MAX, frame_text())
        .sequence_with_offset(1, 1, frame_text())
        .sequence_with_offset(1, -1, frame_text());
    let mut scene = Scene::new();

    series
        .emit(
            SceneFrameContext::new(0, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();

    assert_eq!(scene.nodes.len(), 1);
    assert!(matches!(
        &scene.nodes[0],
        SceneNode::Text { content, .. } if content == "0"
    ));
}

#[test]
fn series_does_not_wrap_after_large_positive_offset() {
    let series = SceneSeries::new()
        .sequence(i32::MAX as u32 + 2, frame_text())
        .sequence_with_offset(1, i32::MAX, frame_text());
    let mut scene = Scene::new();

    series
        .emit(
            SceneFrameContext::new(0, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();

    assert_eq!(scene.nodes.len(), 1);
}

#[test]
fn series_can_render_a_one_frame_entry_at_u32_max() {
    let series = SceneSeries::new()
        .sequence(u32::MAX, frame_text())
        .sequence_with_offset(1, 0, frame_text());
    let mut scene = Scene::new();

    series
        .emit(
            SceneFrameContext::new(u32::MAX, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();

    assert_eq!(scene.nodes.len(), 1);
    assert!(matches!(
        &scene.nodes[0],
        SceneNode::Text { content, .. } if content == "0"
    ));
}

#[test]
fn freeze_replaces_only_the_local_frame() {
    let composition = SceneEmitterComposition::new("freeze", SceneFreeze::new(7, frame_text()));
    let scene = composition.render(42, &Value::Null, context()).unwrap();
    assert!(matches!(
        &scene.nodes[0],
        SceneNode::Text { content, .. } if content == "7"
    ));
}

#[test]
fn stack_and_group_preserve_primitive_order() {
    let image = SceneNode::Image {
        src: "card.png".into(),
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 50.0,
        fit: ImageFit::Contain,
        opacity: 1.0,
    };
    let stack = SceneStack::new().with(image).with(frame_text());
    let composition =
        SceneEmitterComposition::new("group", SceneGroup::new(stack).with_opacity(0.5));
    let scene = composition.render(3, &Value::Null, context()).unwrap();

    assert!(matches!(
        &scene.nodes[0],
        SceneNode::Group { opacity, children, .. }
            if (*opacity - 0.5).abs() < f32::EPSILON && children.len() == 2
    ));
}

#[test]
fn layer_collects_compositing_options_and_children() {
    let layer = SceneLayer::new(frame_text())
        .with_opacity(0.75)
        .with_blend_mode(BlendMode::Multiply)
        .with_clip(ClipRegion::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            corner_radius: 8.0,
        })
        .with_filter(SceneFilter::Grayscale { amount: 1.0 })
        .with_shadow(SceneShadow {
            offset_x: 4.0,
            offset_y: 6.0,
            blur_sigma: 3.0,
            color: Color::rgba(0, 0, 0, 128),
        });
    let composition = SceneEmitterComposition::new("layer", layer);
    let scene = composition.render(3, &Value::Null, context()).unwrap();

    assert!(matches!(
        &scene.nodes[0],
        SceneNode::Layer {
            opacity,
            blend_mode: BlendMode::Multiply,
            clip: Some(ClipRegion::Rect { .. }),
            filters,
            shadow: Some(_),
            children,
            ..
        } if (*opacity - 0.75).abs() < f32::EPSILON
            && filters.len() == 1
            && children.len() == 1
    ));
}

#[test]
fn text_block_resolves_wrapping_fitting_and_alignment() {
    let font_cache = rust_motion_rasterizer::FontCache::load();
    let Some(font) = font_cache.font_path() else {
        return;
    };
    let block = SceneTextBlock::new("one two three four five six", 10.0, 20.0, 120.0, 52.0, 32.0)
        .with_min_font_size(14.0)
        .with_max_lines(2)
        .with_alignment(TextHorizontalAlign::Center, TextVerticalAlign::Center)
        .with_overflow(TextOverflow::Ellipsis)
        .with_font_sources([font.to_string()]);
    let composition = SceneEmitterComposition::new("text-block", block);
    let scene = composition.render(0, &Value::Null, context()).unwrap();

    assert!(!scene.nodes.is_empty());
    assert!(scene.nodes.len() <= 2);
    assert!(scene.nodes.iter().all(|node| matches!(
        node,
        SceneNode::Text { x, font_size, .. } if *x >= 10.0 && *font_size <= 32.0
    )));
}

#[test]
fn scene_rect_emits_correct_rect_node() {
    let rect = SceneRect::new(10.0, 20.0, 100.0, 50.0, Color::rgb(255, 0, 0))
        .with_stroke(Color::WHITE, 2.0)
        .with_corner_radius(8.0);
    let composition = SceneEmitterComposition::new("rect", rect);
    let scene = composition.render(0, &Value::Null, context()).unwrap();

    assert_eq!(scene.nodes.len(), 1);
    assert!(matches!(
        &scene.nodes[0],
        SceneNode::Rect { x, y, w, h, fill, stroke: Some(_), corner_radius, .. }
            if (*x - 10.0).abs() < f32::EPSILON
                && (*y - 20.0).abs() < f32::EPSILON
                && (*w - 100.0).abs() < f32::EPSILON
                && (*h - 50.0).abs() < f32::EPSILON
                && *fill == Color::rgb(255, 0, 0)
                && (*corner_radius - 8.0).abs() < f32::EPSILON
    ));
}

#[test]
fn scene_rect_clamps_negative_dimensions_to_zero() {
    let rect = SceneRect::new(0.0, 0.0, -10.0, -5.0, Color::BLACK);
    let composition = SceneEmitterComposition::new("rect-clamp", rect);
    let scene = composition.render(0, &Value::Null, context()).unwrap();

    assert!(matches!(
        &scene.nodes[0],
        SceneNode::Rect { w, h, .. } if *w == 0.0 && *h == 0.0
    ));
}

#[test]
fn scene_text_emits_correct_text_node() {
    let text = SceneText::new("Hello", 5.0, 30.0, 24.0, Color::WHITE).with_font_weight(700);
    let composition = SceneEmitterComposition::new("text", text);
    let scene = composition.render(0, &Value::Null, context()).unwrap();

    assert_eq!(scene.nodes.len(), 1);
    assert!(matches!(
        &scene.nodes[0],
        SceneNode::Text { x, y, content, font_size, font_weight, .. }
            if (x - 5.0).abs() < f32::EPSILON
                && (y - 30.0).abs() < f32::EPSILON
                && content == "Hello"
                && (font_size - 24.0).abs() < f32::EPSILON
                && *font_weight == 700
    ));
}

#[test]
fn scene_text_emits_nothing_for_empty_content() {
    let text = SceneText::new("", 0.0, 0.0, 16.0, Color::WHITE);
    let composition = SceneEmitterComposition::new("text-empty", text);
    let scene = composition.render(0, &Value::Null, context()).unwrap();
    assert!(scene.nodes.is_empty());
}

#[test]
fn scene_linear_gradient_emits_correct_gradient_node() {
    use rust_motion_rasterizer::GradientStop;
    let gradient = SceneLinearGradient::new(
        0.0,
        0.0,
        320.0,
        180.0,
        vec![
            GradientStop {
                position: 0.0,
                color: Color::BLACK,
            },
            GradientStop {
                position: 1.0,
                color: Color::WHITE,
            },
        ],
    )
    .with_angle(135.0);
    let composition = SceneEmitterComposition::new("grad", gradient);
    let scene = composition.render(0, &Value::Null, context()).unwrap();

    assert_eq!(scene.nodes.len(), 1);
    assert!(matches!(
        &scene.nodes[0],
        SceneNode::LinearGradient { w, h, angle_deg, stops, .. }
            if (w - 320.0).abs() < f32::EPSILON
                && (h - 180.0).abs() < f32::EPSILON
                && (angle_deg - 135.0).abs() < f32::EPSILON
                && stops.len() == 2
    ));
}

#[test]
fn scene_loop_wraps_frame_at_duration() {
    let captured_frames = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    let frames_clone = captured_frames.clone();
    let child = move |ctx: SceneFrameContext, _props: &Value, _scene: &mut Scene| {
        frames_clone.lock().unwrap().push(ctx.frame);
        Ok(())
    };
    let looper = SceneLoop::new(10, child);
    let ctx15 = SceneFrameContext {
        frame: 15,
        global_frame: 15,
        composition: context(),
    };
    let mut scene = Scene::new();
    looper.emit(ctx15, &Value::Null, &mut scene).unwrap();
    assert_eq!(*captured_frames.lock().unwrap(), vec![5]); // 15 % 10 = 5
}

#[test]
fn scene_loop_preserves_global_frame() {
    let captured = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    let captured_clone = captured.clone();
    let child = move |ctx: SceneFrameContext, _props: &Value, _scene: &mut Scene| {
        captured_clone
            .lock()
            .unwrap()
            .push((ctx.frame, ctx.global_frame));
        Ok(())
    };
    let looper = SceneLoop::new(10, child);
    for frame in [0, 5, 9, 10, 15, 29, 99] {
        let ctx = SceneFrameContext {
            frame,
            global_frame: frame,
            composition: context(),
        };
        let mut scene = Scene::new();
        looper.emit(ctx, &Value::Null, &mut scene).unwrap();
    }
    assert_eq!(
        *captured.lock().unwrap(),
        vec![(0, 0), (5, 5), (9, 9), (0, 10), (5, 15), (9, 29), (9, 99),]
    );
}

#[test]
fn scene_loop_bounded_repetitions_builder_and_total_duration() {
    let call_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let count_clone = call_count.clone();
    let child = move |_: SceneFrameContext, _: &Value, _: &mut Scene| {
        count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    };

    let infinite_loop = SceneLoop::new(10, child.clone());
    assert_eq!(infinite_loop.total_duration(), None);

    let bounded_loop = SceneLoop::new(10, child).times(3);
    assert_eq!(bounded_loop.total_duration(), Some(30));

    // Frames 0..30 should emit
    for frame in 0..30 {
        let ctx = SceneFrameContext {
            frame,
            global_frame: frame,
            composition: context(),
        };
        let mut scene = Scene::new();
        bounded_loop.emit(ctx, &Value::Null, &mut scene).unwrap();
    }
    assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 30);

    // Frames >= 30 should not emit
    for frame in [30, 31, 50, 100] {
        let ctx = SceneFrameContext {
            frame,
            global_frame: frame,
            composition: context(),
        };
        let mut scene = Scene::new();
        bounded_loop.emit(ctx, &Value::Null, &mut scene).unwrap();
    }
    assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 30);
}

#[test]
fn scene_loop_zero_duration_emits_nothing_at_frames_zero_and_one() {
    let captured = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    let captured_clone = captured.clone();
    let child = move |ctx: SceneFrameContext, _props: &Value, _scene: &mut Scene| {
        captured_clone.lock().unwrap().push(ctx.frame);
        Ok(())
    };
    let looper = SceneLoop::new(0, child);
    for frame in [0, 1] {
        let ctx = SceneFrameContext {
            frame,
            global_frame: frame,
            composition: context(),
        };
        let mut scene = Scene::new();
        looper.emit(ctx, &Value::Null, &mut scene).unwrap();
        assert!(scene.nodes.is_empty());
    }
    assert!(captured.lock().unwrap().is_empty());
}

#[test]
fn scene_transition_series_empty_and_single_clip() {
    let empty = SceneTransitionSeries::new();
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
    assert_eq!(empty.total_duration(), 0);
    assert_eq!(empty.duration_in_frames(), 0);
    let mut scene = Scene::new();
    empty
        .emit(
            SceneFrameContext::new(0, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert!(scene.nodes.is_empty());

    let rect = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
    let single = SceneTransitionSeries::new().clip(45, rect);
    assert!(!single.is_empty());
    assert_eq!(single.len(), 1);
    assert_eq!(single.total_duration(), 45);

    let mut scene = Scene::new();
    single
        .emit(
            SceneFrameContext::new(20, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 1);
    // Single clip without transition is emitted directly, not in a Group
    assert!(matches!(&scene.nodes[0], SceneNode::Rect { .. }));
}

#[test]
fn scene_transition_series_timeline_calculation_and_clamping() {
    let rect = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
    let series = SceneTransitionSeries::new()
        .clip(60, rect.clone())
        .transition(TransitionKind::Fade, TransitionTiming::new(20))
        .clip(40, rect.clone())
        .transition(TransitionKind::SlideLeft, TransitionTiming::new(100)) // 100 > min(40, 50) -> clamped to 40
        .clip(50, rect);

    let (starts, overlaps) = series.calculate_timeline();
    assert_eq!(overlaps, vec![20, 40]);
    assert_eq!(starts, vec![0, 40, 40]);
    assert_eq!(series.total_duration(), 40 + 50); // 90
}

#[test]
fn scene_transition_series_fade_crossfade() {
    let rect1 = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
    let rect2 = SceneRect::new(0.0, 0.0, 20.0, 20.0, Color::BLACK);
    let series = SceneTransitionSeries::new()
        .clip(60, rect1)
        .transition(TransitionKind::Fade, TransitionTiming::new(20))
        .clip(60, rect2);

    // Timeline:
    // Clip 1: starts at 0, duration 60 -> active [0, 60)
    // Clip 2: starts at 40 (60 - 20), duration 60 -> active [40, 100)
    // Total duration: 100

    // Frame 20: Only clip 1 active (no transition yet, local frame 20 < 40)
    let mut scene = Scene::new();
    series
        .emit(
            SceneFrameContext::new(20, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 1);
    assert!(
        matches!(&scene.nodes[0], SceneNode::Rect { w, .. } if (*w - 10.0).abs() < f32::EPSILON)
    );

    // Frame 50 (midpoint of overlap [40, 60)): p = (50 - 40) / 20 = 0.5
    // Clip 1: alpha = 1.0 - 0.5 = 0.5
    // Clip 2: alpha = 0.5
    let mut scene = Scene::new();
    series
        .emit(
            SceneFrameContext::new(50, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 2);

    if let SceneNode::Group {
        opacity, children, ..
    } = &scene.nodes[0]
    {
        assert!((*opacity - 0.5).abs() < 1e-4);
        assert!(
            matches!(&children[0], SceneNode::Rect { w, .. } if (*w - 10.0).abs() < f32::EPSILON)
        );
    } else {
        panic!("Expected SceneNode::Group for clip 1");
    }

    if let SceneNode::Group {
        opacity, children, ..
    } = &scene.nodes[1]
    {
        assert!((*opacity - 0.5).abs() < 1e-4);
        assert!(
            matches!(&children[0], SceneNode::Rect { w, .. } if (*w - 20.0).abs() < f32::EPSILON)
        );
    } else {
        panic!("Expected SceneNode::Group for clip 2");
    }

    // Frame 70: Only clip 2 active (local frame 30 >= 20, no group needed)
    let mut scene = Scene::new();
    series
        .emit(
            SceneFrameContext::new(70, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 1);
    assert!(
        matches!(&scene.nodes[0], SceneNode::Rect { w, .. } if (*w - 20.0).abs() < f32::EPSILON)
    );
}

#[test]
fn scene_transition_series_slide_left() {
    let rect1 = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
    let rect2 = SceneRect::new(0.0, 0.0, 20.0, 20.0, Color::BLACK);
    let series = SceneTransitionSeries::new()
        .clip(60, rect1)
        .transition(TransitionKind::SlideLeft, TransitionTiming::new(20))
        .clip(60, rect2);

    // Frame 50 (midpoint of overlap [40, 60)): p = 0.5
    // Composition width = 320
    // Outgoing (clip 1): tx = -p * width = -0.5 * 320 = -160.0
    // Incoming (clip 2): tx = (1 - p) * width = 0.5 * 320 = +160.0
    let mut scene = Scene::new();
    series
        .emit(
            SceneFrameContext::new(50, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 2);

    if let SceneNode::Group {
        transform, opacity, ..
    } = &scene.nodes[0]
    {
        assert!((*opacity - 1.0).abs() < 1e-4);
        assert!((transform.tx - -160.0).abs() < 1e-4);
        assert!(transform.ty.abs() < 1e-4);
    } else {
        panic!("Expected SceneNode::Group for clip 1");
    }

    if let SceneNode::Group {
        transform, opacity, ..
    } = &scene.nodes[1]
    {
        assert!((*opacity - 1.0).abs() < 1e-4);
        assert!((transform.tx - 160.0).abs() < 1e-4);
        assert!(transform.ty.abs() < 1e-4);
    } else {
        panic!("Expected SceneNode::Group for clip 2");
    }
}

#[test]
fn scene_transition_series_slide_right() {
    let rect1 = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
    let rect2 = SceneRect::new(0.0, 0.0, 20.0, 20.0, Color::BLACK);
    let series = SceneTransitionSeries::new()
        .clip(60, rect1)
        .transition(TransitionKind::SlideRight, TransitionTiming::new(20))
        .clip(60, rect2);

    // Frame 50 (midpoint of overlap [40, 60)): p = 0.5
    // Composition width = 320
    // Outgoing (clip 1): tx = p * width = +160.0
    // Incoming (clip 2): tx = -(1 - p) * width = -160.0
    let mut scene = Scene::new();
    series
        .emit(
            SceneFrameContext::new(50, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 2);

    if let SceneNode::Group { transform, .. } = &scene.nodes[0] {
        assert!((transform.tx - 160.0).abs() < 1e-4);
        assert!(transform.ty.abs() < 1e-4);
    } else {
        panic!("Expected SceneNode::Group for clip 1");
    }

    if let SceneNode::Group { transform, .. } = &scene.nodes[1] {
        assert!((transform.tx - -160.0).abs() < 1e-4);
        assert!(transform.ty.abs() < 1e-4);
    } else {
        panic!("Expected SceneNode::Group for clip 2");
    }
}

#[test]
fn scene_transition_series_slide_up() {
    let rect1 = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
    let rect2 = SceneRect::new(0.0, 0.0, 20.0, 20.0, Color::BLACK);
    let series = SceneTransitionSeries::new()
        .clip(60, rect1)
        .transition(TransitionKind::SlideUp, TransitionTiming::new(20))
        .clip(60, rect2);

    // Frame 50 (midpoint of overlap [40, 60)): p = 0.5
    // Composition height = 180
    // Outgoing (clip 1): ty = -p * height = -90.0
    // Incoming (clip 2): ty = (1 - p) * height = +90.0
    let mut scene = Scene::new();
    series
        .emit(
            SceneFrameContext::new(50, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 2);

    if let SceneNode::Group { transform, .. } = &scene.nodes[0] {
        assert!(transform.tx.abs() < 1e-4);
        assert!((transform.ty - -90.0).abs() < 1e-4);
    } else {
        panic!("Expected SceneNode::Group for clip 1");
    }

    if let SceneNode::Group { transform, .. } = &scene.nodes[1] {
        assert!(transform.tx.abs() < 1e-4);
        assert!((transform.ty - 90.0).abs() < 1e-4);
    } else {
        panic!("Expected SceneNode::Group for clip 2");
    }
}

#[test]
fn scene_transition_series_slide_down() {
    let rect1 = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
    let rect2 = SceneRect::new(0.0, 0.0, 20.0, 20.0, Color::BLACK);
    let series = SceneTransitionSeries::new()
        .clip(60, rect1)
        .transition(TransitionKind::SlideDown, TransitionTiming::new(20))
        .clip(60, rect2);

    // Frame 50 (midpoint of overlap [40, 60)): p = 0.5
    // Composition height = 180
    // Outgoing (clip 1): ty = p * height = +90.0
    // Incoming (clip 2): ty = -(1 - p) * height = -90.0
    let mut scene = Scene::new();
    series
        .emit(
            SceneFrameContext::new(50, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 2);

    if let SceneNode::Group { transform, .. } = &scene.nodes[0] {
        assert!(transform.tx.abs() < 1e-4);
        assert!((transform.ty - 90.0).abs() < 1e-4);
    } else {
        panic!("Expected SceneNode::Group for clip 1");
    }

    if let SceneNode::Group { transform, .. } = &scene.nodes[1] {
        assert!(transform.tx.abs() < 1e-4);
        assert!((transform.ty - -90.0).abs() < 1e-4);
    } else {
        panic!("Expected SceneNode::Group for clip 2");
    }
}

#[test]
fn scene_transition_series_chained_multi_transitions() {
    let r1 = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
    let r2 = SceneRect::new(0.0, 0.0, 20.0, 20.0, Color::BLACK);
    let r3 = SceneRect::new(0.0, 0.0, 30.0, 30.0, Color::rgb(255, 0, 0));

    let series = SceneTransitionSeries::new()
        .clip(40, r1)
        .transition(TransitionKind::SlideLeft, TransitionTiming::new(10))
        .clip(40, r2)
        .transition(TransitionKind::Fade, TransitionTiming::new(10))
        .clip(40, r3);

    // Clip 1: start = 0, duration = 40 (active [0, 40))
    // Clip 2: start = 30, duration = 40 (active [30, 70))
    // Clip 3: start = 60, duration = 40 (active [60, 100))
    // Total duration: 100
    assert_eq!(series.total_duration(), 100);

    // Transition 1 midpoint at frame 35 (overlap [30, 40), p = 0.5)
    let mut scene = Scene::new();
    series
        .emit(
            SceneFrameContext::new(35, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 2);
    if let SceneNode::Group { transform, .. } = &scene.nodes[0] {
        assert!((transform.tx - -160.0).abs() < 1e-4);
    } else {
        panic!("Clip 1 should be a group");
    }
    if let SceneNode::Group { transform, .. } = &scene.nodes[1] {
        assert!((transform.tx - 160.0).abs() < 1e-4);
    } else {
        panic!("Clip 2 should be a group");
    }

    // Transition 2 midpoint at frame 65 (overlap [60, 70), p = 0.5)
    let mut scene = Scene::new();
    series
        .emit(
            SceneFrameContext::new(65, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 2);
    if let SceneNode::Group { opacity, .. } = &scene.nodes[0] {
        assert!((*opacity - 0.5).abs() < 1e-4);
    } else {
        panic!("Clip 2 should fade out");
    }
    if let SceneNode::Group { opacity, .. } = &scene.nodes[1] {
        assert!((*opacity - 0.5).abs() < 1e-4);
    } else {
        panic!("Clip 3 should fade in");
    }
}

#[test]
fn scene_transition_series_combined_in_out_for_short_clip() {
    let r1 = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
    let r2 = SceneRect::new(0.0, 0.0, 20.0, 20.0, Color::BLACK);
    let r3 = SceneRect::new(0.0, 0.0, 30.0, 30.0, Color::rgb(255, 0, 0));

    let series = SceneTransitionSeries::new()
        .clip(30, r1)
        .transition(TransitionKind::Fade, TransitionTiming::new(10))
        .clip(10, r2) // 10 frames: entering on [0, 10) AND exiting on [0, 10)
        .transition(TransitionKind::Fade, TransitionTiming::new(10))
        .clip(30, r3);

    // Timeline starts:
    // Clip 1: start 0, duration 30
    // Clip 2: start 20 (30 - 10), duration 10
    // Clip 3: start 20 (20 + 10 - 10), duration 30
    // Total duration: 50
    assert_eq!(series.total_duration(), 50);

    // At frame 25 (local frame 5 in Clip 2):
    // Clip 2: p_in = 5/10 = 0.5 (alpha_in = 0.5), p_out = 5/10 = 0.5 (alpha_out = 0.5)
    // Combined alpha = 0.5 * 0.5 = 0.25
    let mut scene = Scene::new();
    series
        .emit(
            SceneFrameContext::new(25, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    // At frame 25, all 3 clips overlap:
    // Clip 1: local frame 25 -> out_start = 20 -> p_out = 5/10 = 0.5 -> alpha = 0.5
    // Clip 2: local frame 5 -> alpha = 0.25
    // Clip 3: local frame 5 -> in_overlap = 10 -> p_in = 5/10 = 0.5 -> alpha = 0.5
    assert_eq!(scene.nodes.len(), 3);

    if let SceneNode::Group { opacity, .. } = &scene.nodes[0] {
        assert!((*opacity - 0.5).abs() < 1e-4);
    } else {
        panic!("Clip 1 expected group");
    }

    if let SceneNode::Group { opacity, .. } = &scene.nodes[1] {
        assert!((*opacity - 0.25).abs() < 1e-4);
    } else {
        panic!("Clip 2 expected group with combined opacity");
    }

    if let SceneNode::Group { opacity, .. } = &scene.nodes[2] {
        assert!((*opacity - 0.5).abs() < 1e-4);
    } else {
        panic!("Clip 3 expected group");
    }
}

#[test]
fn scene_trail_emits_multiple_lagged_layers() {
    let captured_frames = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    let frames_clone = captured_frames.clone();
    let child = move |ctx: SceneFrameContext, _props: &Value, scene: &mut Scene| {
        frames_clone.lock().unwrap().push(ctx.frame);
        scene.push(SceneNode::Rect {
            x: ctx.frame as f32 * 10.0,
            y: 0.0,
            w: 20.0,
            h: 20.0,
            fill: Color::WHITE,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });
        Ok(())
    };

    // 3 layers with lag 2 frames:
    // at frame 10:
    // layer 2 (oldest): ghost_frame = 10 - 2*2 = 6
    // layer 1: ghost_frame = 10 - 1*2 = 8
    // layer 0 (newest): ghost_frame = 10 - 0*2 = 10
    let trail = SceneTrail::new(3, 2, child);
    let mut scene = Scene::new();
    trail
        .emit(
            SceneFrameContext::new(10, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();

    assert_eq!(*captured_frames.lock().unwrap(), vec![6, 8, 10]);
    assert_eq!(scene.nodes.len(), 3);

    // First 2 ghost layers should be wrapped in SceneNode::Group with opacity < 1.0
    // The newest layer (frame 10) is rendered directly (opacity = 1.0)
    assert!(matches!(scene.nodes[0], SceneNode::Group { opacity, .. } if opacity < 1.0));
    assert!(matches!(scene.nodes[1], SceneNode::Group { opacity, .. } if opacity < 1.0));
    assert!(matches!(scene.nodes[2], SceneNode::Rect { .. }));
}

#[test]
fn scene_trail_exponential_opacity() {
    let child = |ctx: SceneFrameContext, _props: &Value, scene: &mut Scene| {
        scene.push(SceneNode::Rect {
            x: ctx.frame as f32,
            y: 0.0,
            w: 10.0,
            h: 10.0,
            fill: Color::WHITE,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });
        Ok(())
    };

    let trail =
        SceneTrail::new(3, 1, child).with_opacity_strategy(SceneTrailOpacity::Exponential(0.5));
    let mut scene = Scene::new();
    trail
        .emit(
            SceneFrameContext::new(5, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();

    // 3 layers: layer 2 (opacity 0.25), layer 1 (opacity 0.5), layer 0 (opacity 1.0)
    assert_eq!(scene.nodes.len(), 3);
    if let SceneNode::Group { opacity, .. } = scene.nodes[0] {
        assert!((opacity - 0.25).abs() < 1e-4);
    } else {
        panic!("Expected group for layer 2");
    }
    if let SceneNode::Group { opacity, .. } = scene.nodes[1] {
        assert!((opacity - 0.5).abs() < 1e-4);
    } else {
        panic!("Expected group for layer 1");
    }
}

#[test]
fn scene_authoring_preserves_add_order_and_moves_readded_objects_to_front() {
    let authored = SceneAuthoringBuilder::new()
        .add(
            "z-background",
            SceneRect::new(1.0, 0.0, 1.0, 1.0, Color::BLACK),
        )
        .add(
            "a-foreground",
            SceneRect::new(2.0, 0.0, 1.0, 1.0, Color::WHITE),
        )
        .wait(1)
        .remove("z-background")
        .add(
            "z-background",
            SceneRect::new(3.0, 0.0, 1.0, 1.0, Color::BLACK),
        )
        .compile();
    for (frame, expected) in [(0, [1.0, 2.0]), (1, [2.0, 3.0])] {
        let mut scene = Scene::new();
        authored
            .emit(
                SceneFrameContext::new(frame, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        let positions: Vec<_> = scene
            .nodes
            .iter()
            .map(|node| match node {
                SceneNode::Rect { x, .. } => *x,
                _ => panic!("expected rectangle"),
            })
            .collect();
        assert_eq!(positions, expected);
    }
}

#[test]
fn scene_authoring_compiles_add_play_wait_remove() {
    let authored = SceneAuthoringBuilder::new()
        .add(
            "background",
            SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::BLACK),
        )
        .play(
            3,
            |ctx: SceneFrameContext, _props: &Value, scene: &mut Scene| {
                scene.push(SceneNode::Rect {
                    x: ctx.frame as f32,
                    y: 0.0,
                    w: 1.0,
                    h: 1.0,
                    fill: Color::WHITE,
                    stroke: None,
                    stroke_width: 0.0,
                    corner_radius: 0.0,
                });
                Ok(())
            },
        )
        .wait(2)
        .remove("background")
        .compile();

    assert_eq!(authored.duration_in_frames(), 5);
    for frame in [4, 0, 3, 1, 2] {
        let mut scene = Scene::new();
        authored
            .emit(
                SceneFrameContext::new(frame, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        let expected_nodes = usize::from(frame < 5) + usize::from((0..3).contains(&frame));
        assert_eq!(scene.nodes.len(), expected_nodes);
        if (0..3).contains(&frame) {
            assert!(matches!(scene.nodes[1], SceneNode::Rect { x, .. } if x == frame as f32));
        }
    }

    let mut after_remove = Scene::new();
    authored
        .emit(
            SceneFrameContext::new(5, context()),
            &Value::Null,
            &mut after_remove,
        )
        .unwrap();
    assert!(after_remove.nodes.is_empty());
}

#[test]
fn scene_authoring_parallel_actions_share_a_start_frame() {
    let authored = SceneAuthoringBuilder::new()
        .parallel([
            SceneAction::play(
                4,
                |ctx: SceneFrameContext, _props: &Value, scene: &mut Scene| {
                    scene.push(SceneNode::Rect {
                        x: ctx.frame as f32,
                        y: 0.0,
                        w: 1.0,
                        h: 1.0,
                        fill: Color::WHITE,
                        stroke: None,
                        stroke_width: 0.0,
                        corner_radius: 0.0,
                    });
                    Ok(())
                },
            ),
            SceneAction::wait(2),
        ])
        .play(1, SceneRect::new(1.0, 1.0, 2.0, 2.0, Color::WHITE))
        .compile();

    assert_eq!(authored.duration_in_frames(), 5);
    let mut scene = Scene::new();
    authored
        .emit(
            SceneFrameContext::new(3, context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 1);
    assert!(matches!(scene.nodes[0], SceneNode::Rect { x, .. } if x == 3.0));

    let mut final_scene = Scene::new();
    authored
        .emit(
            SceneFrameContext::new(4, context()),
            &Value::Null,
            &mut final_scene,
        )
        .unwrap();
    assert_eq!(final_scene.nodes.len(), 1);
    assert!(matches!(final_scene.nodes[0], SceneNode::Rect { x, .. } if x == 1.0));
}
