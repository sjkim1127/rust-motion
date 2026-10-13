use dioxuscut_composition::{
    NativeCompositionContext, SceneEmitter, SceneFrameContext, SceneGroup, SceneLayer, SceneLoop,
    SceneRect, SceneSequence,
};
use dioxuscut_rasterizer::{Color, Scene, SceneNode, Transform2D};
use serde_json::Value;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

fn test_context() -> NativeCompositionContext {
    NativeCompositionContext {
        width: 1920,
        height: 1080,
        fps: 30.0,
        duration_in_frames: 300,
    }
}

// ── 1. Frame Boundary Exact Tests ─────────────────────────────────────────────

#[test]
fn test_scene_loop_boundary_frames_exact() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();

    let child = move |ctx: SceneFrameContext, _props: &Value, _scene: &mut Scene| {
        captured_clone
            .lock()
            .unwrap()
            .push((ctx.frame, ctx.global_frame));
        Ok(())
    };

    let duration = 10;
    let looper = SceneLoop::new(duration, child);

    // Test specific boundary frames:
    // 0: start of first iteration -> local 0
    // 8: middle of first iteration -> local 8
    // 9 (duration - 1): last frame of first iteration -> local 9
    // 10 (duration): start of second iteration -> local 0
    // 11 (duration + 1): second frame of second iteration -> local 1
    // 19 (2*duration - 1): last frame of second iteration -> local 9
    // 20 (2*duration): start of third iteration -> local 0
    let test_frames = vec![0, 1, 8, 9, 10, 11, 19, 20, 29, 30];
    for &f in &test_frames {
        let ctx = SceneFrameContext {
            frame: f,
            global_frame: f,
            composition: test_context(),
        };
        let mut scene = Scene::new();
        looper.emit(ctx, &Value::Null, &mut scene).unwrap();
    }

    let results = captured.lock().unwrap().clone();
    assert_eq!(
        results,
        vec![
            (0, 0),
            (1, 1),
            (8, 8),
            (9, 9),
            (0, 10),
            (1, 11),
            (9, 19),
            (0, 20),
            (9, 29),
            (0, 30),
        ],
        "SceneLoop boundary wrapping mismatch"
    );
}

// ── 2. Huge Frames (> 1,000,000 and u32::MAX) ────────────────────────────────

#[test]
fn test_scene_loop_large_and_max_frames_no_overflow() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();

    let child = move |ctx: SceneFrameContext, _props: &Value, _scene: &mut Scene| {
        captured_clone
            .lock()
            .unwrap()
            .push((ctx.frame, ctx.global_frame));
        Ok(())
    };

    let duration = 60;
    let looper = SceneLoop::new(duration, child);

    let large_frames = vec![
        1_000_000,
        1_000_007,
        10_000_000,
        100_000_000,
        u32::MAX - 60,
        u32::MAX - 1,
        u32::MAX,
    ];

    for &f in &large_frames {
        let ctx = SceneFrameContext {
            frame: f,
            global_frame: f,
            composition: test_context(),
        };
        let mut scene = Scene::new();
        looper.emit(ctx, &Value::Null, &mut scene).unwrap();
    }

    let results = captured.lock().unwrap().clone();
    let expected: Vec<(u32, u32)> = large_frames.iter().map(|&f| (f % duration, f)).collect();

    assert_eq!(results, expected, "Large frame modulo failed");
}

// ── 3. Zero-Duration Loop Guard ───────────────────────────────────────────────

#[test]
fn test_scene_loop_zero_duration_guard_never_divides_by_zero() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();

    let child = move |ctx: SceneFrameContext, _props: &Value, _scene: &mut Scene| {
        captured_clone
            .lock()
            .unwrap()
            .push((ctx.frame, ctx.global_frame));
        Ok(())
    };

    // A zero-duration loop emits nothing and must not divide by zero.
    let looper = SceneLoop::new(0, child);

    for &f in &[0, 1, 5, 100, 1_000_000, u32::MAX] {
        let ctx = SceneFrameContext {
            frame: f,
            global_frame: f,
            composition: test_context(),
        };
        let mut scene = Scene::new();
        let res = looper.emit(ctx, &Value::Null, &mut scene);
        assert!(res.is_ok(), "Zero-duration loop must not fail or panic");
        assert!(scene.nodes.is_empty());
    }

    let results = captured.lock().unwrap().clone();
    assert!(results.is_empty());
}

// ── 4. Bounded Repetitions (times: u32) ────────────────────────────────────────

#[test]
fn test_scene_loop_bounded_repetitions_exhaustive() {
    // Test 1: times = 1, duration = 12
    let counter_1 = Arc::new(AtomicU32::new(0));
    let c1 = counter_1.clone();
    let child1 = move |ctx: SceneFrameContext, _: &Value, _: &mut Scene| {
        c1.fetch_add(1, Ordering::SeqCst);
        assert_eq!(ctx.frame, ctx.global_frame);
        Ok(())
    };
    let looper1 = SceneLoop::new(12, child1).times(1);
    assert_eq!(looper1.total_duration(), Some(12));

    for f in 0..12 {
        let mut scene = Scene::new();
        looper1
            .emit(
                SceneFrameContext::new(f, test_context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
    }
    assert_eq!(counter_1.load(Ordering::SeqCst), 12);

    // Frames >= 12 must NOT emit anything
    for f in 12..30 {
        let mut scene = Scene::new();
        looper1
            .emit(
                SceneFrameContext::new(f, test_context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        assert!(scene.nodes.is_empty());
    }
    assert_eq!(counter_1.load(Ordering::SeqCst), 12);

    // Test 2: times = 3, duration = 5 -> total 15 frames
    let captured_2 = Arc::new(Mutex::new(Vec::new()));
    let c2 = captured_2.clone();
    let child2 = move |ctx: SceneFrameContext, _: &Value, _: &mut Scene| {
        c2.lock().unwrap().push(ctx.frame);
        Ok(())
    };
    let looper2 = SceneLoop::with_times(5, 3, child2);
    assert_eq!(looper2.total_duration(), Some(15));

    for f in 0..25 {
        let mut scene = Scene::new();
        looper2
            .emit(
                SceneFrameContext::new(f, test_context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
    }

    let frames_emitted = captured_2.lock().unwrap().clone();
    assert_eq!(
        frames_emitted,
        vec![
            0, 1, 2, 3, 4, // iteration 1
            0, 1, 2, 3, 4, // iteration 2
            0, 1, 2, 3, 4, // iteration 3
        ]
    );

    // Test 3: times = 0 (infinite) -> total_duration is None
    let looper_inf = SceneLoop::new(10, SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE));
    assert_eq!(looper_inf.total_duration(), None);
}

// ── 5. Saturating Multiplication Overflow Safety ──────────────────────────────

#[test]
fn test_scene_loop_overflow_arithmetic_safety() {
    let child = SceneRect::new(0.0, 0.0, 50.0, 50.0, Color::WHITE);

    // u32::MAX * u32::MAX with bounded times
    let looper = SceneLoop::new(u32::MAX, child).times(u32::MAX);
    // saturating_mul should yield u32::MAX
    assert_eq!(looper.total_duration(), Some(u32::MAX));

    // Calling emit with frame near u32::MAX should not panic
    let mut scene = Scene::new();
    let res = looper.emit(
        SceneFrameContext::new(u32::MAX - 1, test_context()),
        &Value::Null,
        &mut scene,
    );
    assert!(res.is_ok());
    assert_eq!(scene.nodes.len(), 1);
}

// ── 6. Nested Composition Hierarchy ──────────────────────────────────────────

#[test]
fn test_scene_loop_nested_in_hierarchy() {
    let rect = SceneRect::new(10.0, 10.0, 100.0, 100.0, Color::rgb(255, 0, 0));
    let looper = SceneLoop::new(20, rect).times(2); // total 40 frames

    // Sequence starting at frame 10, lasting 50 frames
    let seq = SceneSequence::new(10, looper).with_duration(50);

    // Group with transform
    let group = SceneGroup::new(seq).with_transform(Transform2D {
        tx: 50.0,
        ty: 50.0,
        scale_x: 1.5,
        scale_y: 1.5,
        rotate_deg: 45.0,
    });

    // Layer with opacity
    let layer = SceneLayer::new(group).with_opacity(0.8);

    // Frame 5: Before sequence start (10) -> empty
    let mut scene = Scene::new();
    layer
        .emit(
            SceneFrameContext::new(5, test_context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert!(scene.nodes.is_empty());

    // Frame 25: Sequence local frame = 15. Looper local frame = 15 % 20 = 15 (< 40) -> active
    let mut scene = Scene::new();
    layer
        .emit(
            SceneFrameContext::new(25, test_context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 1);
    assert!(matches!(&scene.nodes[0], SceneNode::Layer { .. }));

    // Frame 55: Sequence local frame = 45. Looper total = 40, so local frame 45 >= 40 -> looper inactive -> empty
    let mut scene = Scene::new();
    layer
        .emit(
            SceneFrameContext::new(55, test_context()),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert!(scene.nodes.is_empty());
}
