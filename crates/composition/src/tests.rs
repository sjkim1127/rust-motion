//! Registry and built-in composition tests.

use super::*;
use rust_motion_rasterizer::SceneNode;

#[test]
fn registry_rejects_duplicate_ids_and_reports_known_ids() {
    let mut registry = CompositionRegistry::new();
    registry.register(HelloWorldComposition).unwrap();
    assert_eq!(
        registry.register(HelloWorldComposition),
        Err(CompositionRegistryError::Duplicate("HelloWorld".into()))
    );

    let error = match registry.get("Missing") {
        Ok(_) => panic!("missing composition unexpectedly resolved"),
        Err(error) => error,
    };
    assert_eq!(
        error,
        CompositionRegistryError::Unknown {
            requested: "Missing".into(),
            available: "HelloWorld".into(),
        }
    );
}

#[test]
fn hello_world_uses_props_and_reaches_full_progress() {
    let context = NativeCompositionContext {
        width: 100,
        height: 100,
        fps: 30.0,
        duration_in_frames: 3,
    };
    let props = serde_json::json!({"title": "Custom title"});
    let scene = HelloWorldComposition.render(2, &props, context).unwrap();

    assert!(scene.nodes.iter().any(|node| matches!(
        node,
        SceneNode::Text { content, .. } if content == "Custom title"
    )));
    assert!(scene.nodes.iter().any(|node| matches!(
        node,
        SceneNode::Rect { w, h, .. } if *w == 100.0 && *h == 6.0
    )));
}

#[test]
fn killer_templates_render_successfully_in_registry() {
    let registry = built_in_registry();
    let expected_ids = [
        "HelloWorld",
        "SpringRects",
        "ShapesAndFilters",
        "CyberpunkGrid",
        "ComplexGradients",
        "PodcastWaveform",
        "KaraokeCaptions",
        "BarChartRace",
        "CodeTerminal",
    ];
    for id in expected_ids {
        assert!(
            registry.get(id).is_ok(),
            "Composition '{}' must be registered in built_in_registry()",
            id
        );
    }

    let context = NativeCompositionContext {
        width: 960,
        height: 540,
        fps: 30.0,
        duration_in_frames: 60,
    };
    let props = serde_json::json!({});

    let spring_rects = registry.get("SpringRects").unwrap();
    let spring_scene = spring_rects
        .prepare(&props, context)
        .unwrap()
        .render(15)
        .unwrap();
    assert_eq!(spring_scene.nodes.len(), 33);

    // Test PodcastWaveform
    let podcast = registry.get("PodcastWaveform").unwrap();
    let scene = podcast
        .prepare(&props, context)
        .unwrap()
        .render(15)
        .unwrap();
    assert!(!scene.nodes.is_empty(), "PodcastWaveform must emit nodes");
    assert!(scene
        .nodes
        .iter()
        .any(|n| matches!(n, SceneNode::LinearGradient { .. })));

    // Test KaraokeCaptions
    let karaoke = registry.get("KaraokeCaptions").unwrap();
    let scene = karaoke
        .prepare(&props, context)
        .unwrap()
        .render(15)
        .unwrap();
    assert!(!scene.nodes.is_empty(), "KaraokeCaptions must emit nodes");
    assert!(scene
        .nodes
        .iter()
        .any(|n| matches!(n, SceneNode::Text { .. })));

    // Test BarChartRace
    let race = registry.get("BarChartRace").unwrap();
    let scene = race.prepare(&props, context).unwrap().render(30).unwrap();
    assert!(!scene.nodes.is_empty(), "BarChartRace must emit nodes");
    assert!(scene
        .nodes
        .iter()
        .any(|n| matches!(n, SceneNode::Rect { .. })));

    // Test CodeTerminal
    let terminal = registry.get("CodeTerminal").unwrap();
    let scene = terminal
        .prepare(&props, context)
        .unwrap()
        .render(45)
        .unwrap();
    assert!(!scene.nodes.is_empty(), "CodeTerminal must emit nodes");
    assert!(scene
        .nodes
        .iter()
        .any(|n| matches!(n, SceneNode::Circle { .. })));
}
