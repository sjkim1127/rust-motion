use rust_motion_composition::{
    NativeCompositionContext, SceneEmitter, SceneFrameContext, SceneRect, SceneTransitionSeries,
    TransitionKind, TransitionTiming,
};
use rust_motion_rasterizer::{Color, Scene, SceneNode};
use serde_json::Value;

fn test_ctx(w: u32, h: u32) -> NativeCompositionContext {
    NativeCompositionContext {
        width: w,
        height: h,
        fps: 30.0,
        duration_in_frames: 1000,
    }
}

// ── 1. Zero Clips Edge Case ───────────────────────────────────────────────────

#[test]
fn test_transition_series_zero_clips() {
    let series = SceneTransitionSeries::new();
    assert!(series.is_empty());
    assert_eq!(series.len(), 0);
    assert_eq!(series.total_duration(), 0);
    assert_eq!(series.duration_in_frames(), 0);

    let (starts, overlaps) = series.calculate_timeline();
    assert!(starts.is_empty());
    assert!(overlaps.is_empty());

    for f in [0, 1, 100, 1000] {
        let mut scene = Scene::new();
        series
            .emit(
                SceneFrameContext::new(f, test_ctx(1920, 1080)),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        assert!(scene.nodes.is_empty(), "Empty series must emit 0 nodes");
    }
}

// ── 2. Single Clip Edge Case ──────────────────────────────────────────────────

#[test]
fn test_transition_series_single_clip() {
    let rect = SceneRect::new(0.0, 0.0, 100.0, 100.0, Color::WHITE);
    let series = SceneTransitionSeries::new().clip(50, rect);

    assert!(!series.is_empty());
    assert_eq!(series.len(), 1);
    assert_eq!(series.total_duration(), 50);
    assert_eq!(series.duration_in_frames(), 50);

    let (starts, overlaps) = series.calculate_timeline();
    assert_eq!(starts, vec![0]);
    assert!(overlaps.is_empty());

    // During active frames [0, 50), rect should be emitted directly with NO Group wrapper
    for f in [0, 25, 49] {
        let mut scene = Scene::new();
        series
            .emit(
                SceneFrameContext::new(f, test_ctx(1920, 1080)),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        assert_eq!(scene.nodes.len(), 1);
        assert!(
            matches!(&scene.nodes[0], SceneNode::Rect { w, .. } if (*w - 100.0).abs() < f32::EPSILON),
            "Single clip should emit directly without transformation wrapper"
        );
    }

    // At frame >= 50, emits nothing
    for f in [50, 51, 100] {
        let mut scene = Scene::new();
        series
            .emit(
                SceneFrameContext::new(f, test_ctx(1920, 1080)),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        assert!(scene.nodes.is_empty());
    }
}

// ── 3. Clamped Overlap (Transition Timing > Clip Duration) ─────────────────────

#[test]
fn test_transition_series_clamped_overlap() {
    let r1 = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
    let r2 = SceneRect::new(0.0, 0.0, 20.0, 20.0, Color::BLACK);

    // Clip 1 is 10 frames, Clip 2 is 25 frames, Transition requested is 40 frames!
    // Overlap should be clamped to min(10, 25) = 10 frames.
    let series = SceneTransitionSeries::new()
        .clip(10, r1)
        .transition(TransitionKind::Fade, TransitionTiming::new(40))
        .clip(25, r2);

    let (starts, overlaps) = series.calculate_timeline();
    assert_eq!(
        overlaps,
        vec![10],
        "Overlap must be clamped to min(10, 25) = 10"
    );
    assert_eq!(starts, vec![0, 0], "Clip 2 start = 0 + 10 - 10 = 0");
    assert_eq!(series.total_duration(), 25, "Total duration = 0 + 25 = 25");

    // Overlap window is [0, 10):
    // Frame 0: local_frame in clip 1 is 0 -> out_start = 0 -> p_out = 0/10 = 0.0 -> alpha_out = 1.0.
    //          local_frame in clip 2 is 0 -> in_overlap = 10 -> p_in = 0/10 = 0.0 -> alpha_in = 0.0.
    let mut scene = Scene::new();
    series
        .emit(
            SceneFrameContext::new(0, test_ctx(100, 100)),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    // Clip 1 alpha=1.0 (no group needed or group with alpha=1.0), Clip 2 alpha=0.0 (group with opacity 0.0)
    assert_eq!(scene.nodes.len(), 2);

    // Frame 5 (midpoint of overlap): p = 5/10 = 0.5
    let mut scene = Scene::new();
    series
        .emit(
            SceneFrameContext::new(5, test_ctx(100, 100)),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 2);

    if let SceneNode::Group { opacity, .. } = &scene.nodes[0] {
        assert!((*opacity - 0.5).abs() < 1e-4);
    } else {
        panic!("Expected Group for clip 1 at midpoint");
    }
    if let SceneNode::Group { opacity, .. } = &scene.nodes[1] {
        assert!((*opacity - 0.5).abs() < 1e-4);
    } else {
        panic!("Expected Group for clip 2 at midpoint");
    }

    // Frame 10: Clip 1 ended (0..10). Clip 2 local_frame = 10 >= overlap(10) -> full opacity
    let mut scene = Scene::new();
    series
        .emit(
            SceneFrameContext::new(10, test_ctx(100, 100)),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert_eq!(scene.nodes.len(), 1);
    assert!(
        matches!(&scene.nodes[0], SceneNode::Rect { w, .. } if (*w - 20.0).abs() < f32::EPSILON)
    );

    // Frame 25: Clip 2 ended -> 0 nodes
    let mut scene = Scene::new();
    series
        .emit(
            SceneFrameContext::new(25, test_ctx(100, 100)),
            &Value::Null,
            &mut scene,
        )
        .unwrap();
    assert!(scene.nodes.is_empty());
}

// ── 4. 5-Clip Chain with Alternating Slide and Fade Transitions ──────────────

#[test]
fn test_transition_series_5_clip_chain_exact_matrices_and_opacities() {
    let width = 800.0f32;
    let height = 600.0f32;
    let ctx = test_ctx(width as u32, height as u32);

    // Clips:
    // C0: 40 frames, color 1
    // T0: SlideLeft(10)
    // C1: 50 frames, color 2
    // T1: Fade(20)
    // C2: 30 frames, color 3
    // T2: SlideUp(15)
    // C3: 60 frames, color 4
    // T3: SlideRight(10)
    // C4: 40 frames, color 5
    let c0 = SceneRect::new(0.0, 0.0, 100.0, 10.0, Color::rgb(10, 0, 0));
    let c1 = SceneRect::new(0.0, 0.0, 100.0, 20.0, Color::rgb(20, 0, 0));
    let c2 = SceneRect::new(0.0, 0.0, 100.0, 30.0, Color::rgb(30, 0, 0));
    let c3 = SceneRect::new(0.0, 0.0, 100.0, 40.0, Color::rgb(40, 0, 0));
    let c4 = SceneRect::new(0.0, 0.0, 100.0, 50.0, Color::rgb(50, 0, 0));

    let series = SceneTransitionSeries::new()
        .clip(40, c0)
        .transition(TransitionKind::SlideLeft, TransitionTiming::new(10))
        .clip(50, c1)
        .transition(TransitionKind::Fade, TransitionTiming::new(20))
        .clip(30, c2)
        .transition(TransitionKind::SlideUp, TransitionTiming::new(15))
        .clip(60, c3)
        .transition(TransitionKind::SlideRight, TransitionTiming::new(10))
        .clip(40, c4);

    let (starts, overlaps) = series.calculate_timeline();
    // Overlaps: [10, 20, 15, 10]
    assert_eq!(overlaps, vec![10, 20, 15, 10]);
    // Starts:
    // C0: 0 (len 40 -> [0, 40))
    // C1: 0 + 40 - 10 = 30 (len 50 -> [30, 80))
    // C2: 30 + 50 - 20 = 60 (len 30 -> [60, 90))
    // C3: 60 + 30 - 15 = 75 (len 60 -> [75, 135))
    // C4: 75 + 60 - 10 = 125 (len 40 -> [125, 165))
    assert_eq!(starts, vec![0, 30, 60, 75, 125]);
    assert_eq!(series.total_duration(), 165);

    // ── Exhaustive Per-Frame Step Verification ──
    for frame in 0..=170 {
        let mut scene = Scene::new();
        series
            .emit(SceneFrameContext::new(frame, ctx), &Value::Null, &mut scene)
            .unwrap();

        if frame >= 165 {
            assert!(scene.nodes.is_empty(), "Past total duration: frame {frame}");
            continue;
        }

        // Active intervals:
        let active_c0 = frame < 40;
        let active_c1 = (30..80).contains(&frame);
        let active_c2 = (60..90).contains(&frame);
        let active_c3 = (75..135).contains(&frame);
        let active_c4 = (125..165).contains(&frame);

        let active_count = usize::from(active_c0)
            + usize::from(active_c1)
            + usize::from(active_c2)
            + usize::from(active_c3)
            + usize::from(active_c4);

        assert_eq!(
            scene.nodes.len(),
            active_count,
            "Frame {frame}: expected {active_count} active nodes, got {}",
            scene.nodes.len()
        );

        // Spot check specific transition phases:

        // 1. T0: SlideLeft between C0 and C1 on [30, 40)
        if (30..40).contains(&frame) {
            let p = (frame - 30) as f32 / 10.0;
            // C0 outgoing: tx = -p * 800
            let expected_tx0 = -p * width;
            // C1 incoming: tx = (1 - p) * 800
            let expected_tx1 = (1.0 - p) * width;

            // Check C0
            if let SceneNode::Group {
                transform, opacity, ..
            } = &scene.nodes[0]
            {
                assert!((transform.tx - expected_tx0).abs() < 1e-3, "F{frame} C0 tx");
                assert!((transform.ty - 0.0).abs() < 1e-3);
                assert!((*opacity - 1.0).abs() < 1e-3);
            } else if p == 0.0 {
                // At p = 0.0, tx = 0.0, might not need group if exactly 0
            } else {
                panic!("F{frame}: C0 must be a Group during SlideLeft");
            }

            // Check C1
            if let SceneNode::Group {
                transform, opacity, ..
            } = &scene.nodes[1]
            {
                assert!((transform.tx - expected_tx1).abs() < 1e-3, "F{frame} C1 tx");
                assert!((transform.ty - 0.0).abs() < 1e-3);
                assert!((*opacity - 1.0).abs() < 1e-3);
            } else if (1.0 - p) == 0.0 {
                // At p = 1.0
            } else {
                panic!("F{frame}: C1 must be a Group during SlideLeft");
            }
        }

        // 2. T1: Fade between C1 and C2 on [60, 75) - pure fade before T2 begins
        if (60..75).contains(&frame) {
            let p = (frame - 60) as f32 / 20.0;
            // C1 outgoing: alpha = 1 - p
            let expected_a1 = 1.0 - p;
            // C2 incoming: alpha = p
            let expected_a2 = p;

            // Check C1
            if let SceneNode::Group {
                opacity, transform, ..
            } = &scene.nodes[0]
            {
                assert!(
                    (*opacity - expected_a1).abs() < 1e-3,
                    "F{frame} C1 fade alpha"
                );
                assert!(transform.tx.abs() < 1e-3);
                assert!(transform.ty.abs() < 1e-3);
            }
            // Check C2
            if let SceneNode::Group {
                opacity, transform, ..
            } = &scene.nodes[1]
            {
                assert!(
                    (*opacity - expected_a2).abs() < 1e-3,
                    "F{frame} C2 fade alpha"
                );
                assert!(transform.tx.abs() < 1e-3);
                assert!(transform.ty.abs() < 1e-3);
            }
        }

        // 2b. Double overlap window on [75, 80): C1 (fade out), C2 (fade in + slide up out), C3 (slide up in)
        if (75..80).contains(&frame) {
            let p_fade = (frame - 60) as f32 / 20.0; // T1 overlap 20
            let p_slide = (frame - 75) as f32 / 15.0; // T2 overlap 15

            // C1 (nodes[0]): outgoing fade
            if let SceneNode::Group {
                opacity, transform, ..
            } = &scene.nodes[0]
            {
                assert!((*opacity - (1.0 - p_fade)).abs() < 1e-3);
                assert!(transform.tx.abs() < 1e-3 && transform.ty.abs() < 1e-3);
            }
            // C2 (nodes[1]): incoming fade (alpha = p_fade) + outgoing slide up (ty = -p_slide * height)
            if let SceneNode::Group {
                opacity, transform, ..
            } = &scene.nodes[1]
            {
                assert!(
                    (*opacity - p_fade).abs() < 1e-3,
                    "F{frame} C2 combined alpha"
                );
                assert!(
                    (transform.ty - (-p_slide * height)).abs() < 1e-3,
                    "F{frame} C2 combined ty"
                );
                assert!(transform.tx.abs() < 1e-3);
            }
            // C3 (nodes[2]): incoming slide up (ty = (1 - p_slide) * height)
            if let SceneNode::Group {
                opacity, transform, ..
            } = &scene.nodes[2]
            {
                assert!((*opacity - 1.0).abs() < 1e-3);
                assert!(
                    (transform.ty - ((1.0 - p_slide) * height)).abs() < 1e-3,
                    "F{frame} C3 incoming ty"
                );
                assert!(transform.tx.abs() < 1e-3);
            }
        }

        // 3. T2: SlideUp between C2 and C3 on [75, 90)
        // Notice C2 has overlap with C1 ([60, 80)) AND C3 ([75, 90))!
        // At frame 77: C1 is active (60..80), C2 is active (60..90), C3 is active (75..135) -> 3 clips active!
        if frame == 77 {
            assert_eq!(scene.nodes.len(), 3, "At frame 77, 3 clips overlap");
        }

        // Check C3 during T2: [75, 90), p = (frame - 75) / 15.0
        if (75..90).contains(&frame) {
            let p = (frame - 75) as f32 / 15.0;
            // Incoming C3: ty = (1 - p) * height = (1 - p) * 600
            let expected_ty3 = (1.0 - p) * height;

            let c3_node = scene.nodes.last().unwrap();
            if let SceneNode::Group {
                transform, opacity, ..
            } = c3_node
            {
                assert!((transform.ty - expected_ty3).abs() < 1e-3, "F{frame} C3 ty");
                assert!(transform.tx.abs() < 1e-3);
                assert!((*opacity - 1.0).abs() < 1e-3);
            }
        }

        // 4. T3: SlideRight between C3 and C4 on [125, 135)
        if (125..135).contains(&frame) {
            let p = (frame - 125) as f32 / 10.0;
            // C3 outgoing: tx = p * width = p * 800
            let expected_tx3 = p * width;
            // C4 incoming: tx = -(1 - p) * width = -(1 - p) * 800
            let expected_tx4 = -(1.0 - p) * width;

            // Check C3
            if let SceneNode::Group { transform, .. } = &scene.nodes[0] {
                assert!((transform.tx - expected_tx3).abs() < 1e-3, "F{frame} C3 tx");
            }
            // Check C4
            if let SceneNode::Group { transform, .. } = &scene.nodes[1] {
                assert!((transform.tx - expected_tx4).abs() < 1e-3, "F{frame} C4 tx");
            }
        }
    }
}

// ── 5. SlideDown Transition Precision ─────────────────────────────────────────

#[test]
fn test_transition_series_slide_down_exact() {
    let width = 1000.0f32;
    let height = 500.0f32;
    let ctx = test_ctx(width as u32, height as u32);

    let r1 = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
    let r2 = SceneRect::new(0.0, 0.0, 20.0, 20.0, Color::BLACK);

    let series = SceneTransitionSeries::new()
        .clip(30, r1)
        .transition(TransitionKind::SlideDown, TransitionTiming::new(10))
        .clip(30, r2);

    // Overlap: [20, 30). p = (f - 20) / 10
    // Outgoing C1: ty = p * height
    // Incoming C2: ty = -(1 - p) * height

    for f in 20..30 {
        let p = (f - 20) as f32 / 10.0;
        let mut scene = Scene::new();
        series
            .emit(SceneFrameContext::new(f, ctx), &Value::Null, &mut scene)
            .unwrap();

        assert_eq!(scene.nodes.len(), 2);

        // Clip 1
        if p > 0.0 {
            if let SceneNode::Group { transform, .. } = &scene.nodes[0] {
                assert!((transform.ty - (p * height)).abs() < 1e-3);
                assert!(transform.tx.abs() < 1e-3);
            } else {
                panic!("Clip 1 should have SlideDown transform at f={f}");
            }
        }

        // Clip 2
        if p < 1.0 {
            if let SceneNode::Group { transform, .. } = &scene.nodes[1] {
                assert!((transform.ty - (-(1.0 - p) * height)).abs() < 1e-3);
                assert!(transform.tx.abs() < 1e-3);
            } else {
                panic!("Clip 2 should have SlideDown transform at f={f}");
            }
        }
    }
}
