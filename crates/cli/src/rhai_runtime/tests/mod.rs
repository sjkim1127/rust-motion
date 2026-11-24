//! Unit tests for the sandboxed Rhai composition runtime.

use super::*;

fn context() -> NativeCompositionContext {
    NativeCompositionContext {
        width: 320,
        height: 180,
        fps: 30.0,
        duration_in_frames: 10,
    }
}

#[test]
fn script_builds_a_deterministic_scene_from_context_and_props() {
    let script = r##"
            fn render(ctx, props) {
                let output = scene();
                output.rect(0.0, 0.0, ctx.width.to_float(), ctx.height.to_float(), props.background);
                let x = interpolate(ctx.frame.to_float(), 0.0, 9.0, 0.0, 90.0);
                output.text_bold(x, 80.0, props.title, 24.0, "#ffffff");
            output
        }
    "##;
    let composition = RhaiComposition::from_source("test", script).unwrap();
    let props = serde_json::json!({
        "background": "#102030",
        "title": "Hello Rhai"
    });
    let prepared = composition.prepare(&props, context()).unwrap();

    let first = prepared.render(3).unwrap();
    let second = prepared.render(3).unwrap();
    assert_eq!(first.nodes, second.nodes);
    assert!(matches!(
        &first.nodes[1],
        SceneNode::Text { x, content, .. }
            if (*x - 30.0).abs() < f32::EPSILON && content == "Hello Rhai"
    ));
}

#[test]
fn script_declares_an_explicit_font_source() {
    let script = r##"
            fn render(ctx, props) {
                let output = scene();
                output.text_font(10.0, 30.0, "Pinned", 20.0, "#ffffff", props.font);
            output
        }
    "##;
    let composition = RhaiComposition::from_source("font", script).unwrap();
    let prepared = composition
        .prepare(&serde_json::json!({"font": "assets/Inter.ttf"}), context())
        .unwrap();
    let scene = prepared.render(0).unwrap();

    assert!(matches!(
        &scene.nodes[0],
        SceneNode::Text { font_sources, .. } if font_sources == &["assets/Inter.ttf"]
    ));
}

#[test]
fn script_resolves_a_fitted_multiline_text_box() {
    let script = r##"
            fn render(ctx, props) {
                let output = scene();
                output.text_box(
                    10.0, 20.0, 120.0, 52.0,
                    "one two three four five six", 32.0, 14.0, 2,
                    "#ffffff", props.font, "center"
            );
            output
        }
    "##;
    let font_cache = rust_motion_rasterizer::FontCache::load();
    let Some(font) = font_cache.font_path() else {
        return;
    };
    let composition = RhaiComposition::from_source("text-box", script).unwrap();
    let prepared = composition
        .prepare(&serde_json::json!({"font": font}), context())
        .unwrap();
    let scene = prepared.render(0).unwrap();

    assert!(!scene.nodes.is_empty());
    assert!(scene.nodes.len() <= 2);
    assert!(scene.nodes.iter().all(|node| matches!(
        node,
        SceneNode::Text { x, font_size, .. } if *x >= 10.0 && *font_size <= 32.0
    )));
}

#[test]
fn operation_limit_stops_an_infinite_loop() {
    let composition = RhaiComposition::from_source(
        "infinite",
        "fn render(ctx, props) { while true {} scene() }",
    )
    .unwrap();
    let prepared = composition
        .prepare(&serde_json::json!({}), context())
        .unwrap();

    let error = prepared.render(0).unwrap_err();
    assert!(error.to_string().contains("Too many operations"));
}

#[test]
fn script_builds_a_local_image_node() {
    let script = r#"
            fn render(ctx, props) {
                let output = scene();
                output.image(10.0, 20.0, 100.0, 60.0, props.src, "contain", 0.75);
                output
            }
        "#;
    let composition = RhaiComposition::from_source("image", script).unwrap();
    let prepared = composition
        .prepare(&serde_json::json!({"src": "assets/card.png"}), context())
        .unwrap();
    let scene = prepared.render(0).unwrap();

    assert!(matches!(
        &scene.nodes[0],
        SceneNode::Image { src, fit: ImageFit::Contain, opacity, .. }
            if src == "assets/card.png" && (*opacity - 0.75).abs() < f32::EPSILON
    ));
}

#[test]
fn script_rejects_invalid_image_fit() {
    let script = r#"
            fn render(ctx, props) {
                let output = scene();
                output.image(0.0, 0.0, 10.0, 10.0, "asset.png", "stretchy", 1.0);
                output
            }
        "#;
    let composition = RhaiComposition::from_source("bad-image", script).unwrap();
    let prepared = composition
        .prepare(&serde_json::json!({}), context())
        .unwrap();
    let error = prepared.render(0).unwrap_err();

    assert!(error.to_string().contains("invalid image fit"));
}

#[test]
fn script_builds_video_and_audio_nodes() {
    let script = r#"
            fn render(ctx, props) {
                let output = scene();
                output.video(0.0, 0.0, 320.0, 180.0, props.video, ctx.frame.to_float() / ctx.fps, "cover", 1.0);
                output.video(0.0, 0.0, 320.0, 180.0, props.video, ctx.frame.to_float() / ctx.fps, "contain", 0.5, true);
                output.audio(props.video, 0.25, 0.5, 2.0, 0.75, 1.25, true);
                output
            }
        "#;
    let composition = RhaiComposition::from_source("media", script).unwrap();
    let prepared = composition
        .prepare(&serde_json::json!({"video": "assets/clip.mp4"}), context())
        .unwrap();
    let scene = prepared.render(3).unwrap();

    assert!(matches!(
        &scene.nodes[0],
        SceneNode::Video { src, time, fit: ImageFit::Cover, looped: false, .. }
            if src == "assets/clip.mp4" && (*time - 0.1).abs() < f64::EPSILON
    ));
    assert!(matches!(
        &scene.nodes[1],
        SceneNode::Video { fit: ImageFit::Contain, opacity, looped: true, .. }
            if (*opacity - 0.5).abs() < f32::EPSILON
    ));
    let tracks = scene.audio_tracks();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].src, "assets/clip.mp4");
    assert_eq!(tracks[0].duration, Some(2.0));
    assert!(tracks[0].looped);
}

#[test]
fn script_builds_a_valid_ducked_audio_track() {
    let script = r#"
            fn render(ctx, props) {
                let output = scene();
                output.audio_ducked("voice.wav", 0.8, [[0.0, 1.0], [0.5, 0.25]]);
                output
            }
        "#;
    let composition = RhaiComposition::from_source("valid-ducking", script).unwrap();
    let prepared = composition
        .prepare(&serde_json::json!({}), context())
        .unwrap();
    let scene = prepared.render(0).unwrap();
    let tracks = scene.audio_tracks();

    assert_eq!(tracks.len(), 1);
    assert!((tracks[0].volume - 0.8).abs() < 1e-6);
    assert_eq!(tracks[0].volume_keyframes, vec![(0.0, 1.0), (0.5, 0.25)]);
}

#[test]
fn script_rejects_invalid_ducked_audio_values_at_evaluation_time() {
    let invalid_calls = [
        (r#"audio_ducked("voice.wav", 1.5, [])"#, "audio volume"),
        (
            r#"audio_ducked("voice.wav", 0.5, [[-1.0, 0.5]])"#,
            "audio keyframe time",
        ),
        (
            r#"audio_ducked("voice.wav", 0.5, [[parse_float("NaN"), 0.5]])"#,
            "audio keyframe time",
        ),
        (
            r#"audio_ducked("voice.wav", 0.5, [[0.0, -0.1]])"#,
            "audio keyframe volume",
        ),
        (
            r#"audio_ducked("voice.wav", 0.5, [[0.0, 1.1]])"#,
            "audio keyframe volume",
        ),
        (
            r#"audio_ducked("voice.wav", 0.5, [[0.0, parse_float("NaN")]])"#,
            "audio keyframe volume",
        ),
    ];

    for (call, expected_error) in invalid_calls {
        let script =
            format!("fn render(ctx, props) {{ let output = scene(); output.{call}; output }}");
        let composition = RhaiComposition::from_source("invalid-ducking", &script).unwrap();
        let prepared = composition
            .prepare(&serde_json::json!({}), context())
            .unwrap();
        let error = prepared.render(0).unwrap_err();
        assert!(
            error.to_string().contains(expected_error),
            "expected '{expected_error}' for `{call}`, got: {error}"
        );
    }
}

#[test]
fn unregistered_scene_api_is_rejected() {
    let composition = RhaiComposition::from_source(
        "unknown-api",
        "fn render(ctx, props) { let output = scene(); output.read_file(\"secret\"); output }",
    )
    .unwrap();
    let prepared = composition
        .prepare(&serde_json::json!({}), context())
        .unwrap();

    let error = prepared.render(0).unwrap_err();
    assert!(error.to_string().contains("Function not found"));
}

#[test]
fn module_imports_are_disabled() {
    let composition = RhaiComposition::from_source(
        "import",
        "import \"untrusted\" as imported; fn render(ctx, props) { scene() }",
    )
    .unwrap();
    let prepared = composition
        .prepare(&serde_json::json!({}), context())
        .unwrap();
    let error = prepared.render(0).unwrap_err();
    assert!(
        error.to_string().contains("Module not found"),
        "unexpected import error: {error}"
    );
}

#[test]
fn script_uses_animation_primitives() {
    let script = r##"
            fn render(ctx, props) {
                let rnd = random("seed-42");
                let col = interpolate_colors("#000000", "#ffffff", 0.5);
            let sp = spring(ctx.frame.to_float(), ctx.fps);
            let output = scene();
            output.rect(rnd * 100.0, sp * 50.0, 200.0, 100.0, col);
            output
        }
    "##;
    let composition = RhaiComposition::from_source("anim", script).unwrap();
    let prepared = composition
        .prepare(&serde_json::json!({}), context())
        .unwrap();
    let scene = prepared.render(15).unwrap();
    assert_eq!(scene.nodes.len(), 1);
    if let SceneNode::Rect { fill, .. } = &scene.nodes[0] {
        assert_eq!(fill.r, 128);
        assert_eq!(fill.g, 128);
        assert_eq!(fill.b, 128);
    } else {
        panic!("Expected Rect node");
    }
}

#[test]
fn script_renders_3d_mesh_and_chop() {
    let script = r##"
            fn render(ctx, props) {
                let frame = ctx.frame.to_float();
                let osc = lfo(frame, ctx.fps, 1.0, 50.0);
                let smoothed = lag(0.0, osc, 0.033, 0.1);

                let output = scene();
                output.cube_3d(320.0, 180.0, 80.0, frame * 0.05, frame * 0.05, 0.0, "#ff8800");
            output.sphere_3d(500.0, 180.0, 40.0, frame * 0.02, frame * 0.03, "#00f0ff");
            output.torus_3d(150.0, 180.0, 50.0, 15.0, frame * 0.03, frame * 0.04, "#ff0088");
            output
        }
    "##;
    let composition = RhaiComposition::from_source("mesh3d", script).unwrap();
    let prepared = composition
        .prepare(&serde_json::json!({}), context())
        .unwrap();
    let scene = prepared.render(10).unwrap();
    assert!(
        !scene.nodes.is_empty(),
        "3D meshes should emit projected Path nodes into the scene"
    );
}

#[test]
fn script_renders_extended_drawing_primitives() {
    let script = r##"
            fn render(ctx, props) {
                let output = scene();
                output.linear_gradient(0.0, 0.0, 320.0, 180.0, 45.0, "#ff0000", "#0000ff");
            output.rect_stroke(10.0, 10.0, 100.0, 50.0, "#112233", "#ffffff", 2.0, 8.0);
            output.circle_stroke(160.0, 90.0, 30.0, "#445566", "#00ffcc", 3.0);
            output.path("M 0 0 L 100 100 Z", "#aabbcc", "#ffffff", 1.5, 0.9);
            output
        }
    "##;
    let composition = RhaiComposition::from_source("extended_draw", script).unwrap();
    let prepared = composition
        .prepare(&serde_json::json!({}), context())
        .unwrap();
    let scene = prepared.render(0).unwrap();
    assert_eq!(scene.nodes.len(), 4);
    assert!(matches!(&scene.nodes[0], SceneNode::LinearGradient { .. }));
    assert!(matches!(
        &scene.nodes[1],
        SceneNode::Rect {
            stroke: Some(_),
            corner_radius,
            ..
        } if *corner_radius == 8.0
    ));
    assert!(matches!(
        &scene.nodes[2],
        SceneNode::Circle {
            stroke: Some(_),
            ..
        }
    ));
    assert!(matches!(&scene.nodes[3], SceneNode::Path { .. }));
}
