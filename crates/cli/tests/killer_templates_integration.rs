use rust_motion_cli::composition::{built_in_registry, NativeCompositionContext};
use rust_motion_cli::rhai_runtime::RhaiComposition;
use rust_motion_cli::Composition;
use rust_motion_rasterizer::SceneNode;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn test_context() -> NativeCompositionContext {
    NativeCompositionContext {
        width: 960,
        height: 540,
        fps: 30.0,
        duration_in_frames: 60,
    }
}

#[test]
fn test_built_in_killer_templates_render_across_timeline() {
    let registry = built_in_registry();
    let template_ids = [
        "PodcastWaveform",
        "KaraokeCaptions",
        "BarChartRace",
        "CodeTerminal",
    ];

    let context = test_context();
    let props = serde_json::json!({});

    for id in template_ids {
        let comp = registry
            .get(id)
            .unwrap_or_else(|_| panic!("Failed to find built-in composition '{id}'"));

        let prepared = comp
            .prepare(&props, context)
            .unwrap_or_else(|e| panic!("Failed to prepare '{id}': {e}"));

        for frame in [0, 15, 30, 59] {
            let scene = prepared
                .render(frame)
                .unwrap_or_else(|e| panic!("Failed to render frame {frame} of '{id}': {e}"));

            assert!(
                !scene.nodes.is_empty(),
                "Composition '{id}' emitted 0 nodes at frame {frame}"
            );
        }
    }
}

#[test]
fn test_rhai_killer_templates_compile_and_render_with_props() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let templates_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join("templates");

    let templates = [
        ("podcast_waveform.rhai", "podcast_waveform_props.json"),
        ("karaoke_captions.rhai", "karaoke_captions_props.json"),
        ("bar_chart_race.rhai", "bar_chart_race_props.json"),
        ("code_terminal.rhai", "code_terminal_props.json"),
    ];

    let context = test_context();

    for (script_name, props_name) in templates {
        let script_path = templates_dir.join(script_name);
        let props_path = templates_dir.join(props_name);

        assert!(
            script_path.exists(),
            "Template script not found: {}",
            script_path.display()
        );
        assert!(
            props_path.exists(),
            "Props file not found: {}",
            props_path.display()
        );

        let script_content = fs::read_to_string(&script_path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {e}", script_path.display()));
        let props_str = fs::read_to_string(&props_path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {e}", props_path.display()));
        let props: Value = serde_json::from_str(&props_str)
            .unwrap_or_else(|e| panic!("Failed to parse JSON {}: {e}", props_path.display()));

        let comp = RhaiComposition::from_source(script_name, &script_content)
            .unwrap_or_else(|e| panic!("Failed to compile Rhai script '{script_name}': {e}"));

        let prepared = comp
            .prepare(&props, context)
            .unwrap_or_else(|e| panic!("Failed to prepare '{script_name}': {e}"));

        // Render start, middle, and end frames
        for frame in [0, 30, 59] {
            let scene = prepared.render(frame).unwrap_or_else(|e| {
                panic!("Failed to render frame {frame} of '{script_name}': {e}")
            });

            assert!(
                !scene.nodes.is_empty(),
                "Template '{script_name}' emitted empty scene at frame {frame}"
            );

            // Verify basic visual node integrity
            let has_drawings = scene.nodes.iter().any(|node| {
                matches!(
                    node,
                    SceneNode::Rect { .. }
                        | SceneNode::Circle { .. }
                        | SceneNode::Text { .. }
                        | SceneNode::LinearGradient { .. }
                )
            });
            assert!(
                has_drawings,
                "Template '{script_name}' had no standard drawing nodes at frame {frame}"
            );
        }
    }
}
