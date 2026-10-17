use rust_motion_rasterizer::backend::{FrameConfig, RasterizerBackend};
use rust_motion_rasterizer::scene::{BlendMode, Color, Scene, SceneFilter, SceneNode};
use rust_motion_rasterizer::tiny_skia_backend::TinySkiaBackend;
use serde_json::json;

const INVERT_WASM: &[u8] = include_bytes!("../../plugins/tests/fixtures/invert.wasm");

#[test]
fn test_wasm_plugin_serde_roundtrip() {
    let filter = SceneFilter::WasmPlugin {
        plugin_name: "crt_scanlines.wasm".to_string(),
        wasm_bytes: Some(vec![0x00, 0x61, 0x73, 0x6d]),
        params: json!({
            "scanline_intensity": 0.8,
            "color_bleed": true
        }),
    };

    let serialized = serde_json::to_string(&filter).expect("serialize wasm plugin filter");
    let deserialized: SceneFilter = serde_json::from_str(&serialized).expect("deserialize wasm plugin filter");
    assert_eq!(filter, deserialized);
}

#[test]
fn test_sandboxed_wasm_plugin_execution() {
    let backend = TinySkiaBackend::new();
    let config = FrameConfig::new(40, 40, 0, 30.0);

    let inner_rect = SceneNode::Rect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
        fill: Color::rgb(200, 50, 100),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    };

    let layer = SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: rust_motion_rasterizer::scene::MaskMode::Alpha,
        filters: vec![SceneFilter::WasmPlugin {
            plugin_name: "invert.wasm".to_string(),
            wasm_bytes: Some(INVERT_WASM.to_vec()),
            params: json!({}),
        }],
        shadow: None,
        children: vec![inner_rect],
    };

    let mut scene = Scene::new();
    scene.push(layer);

    let img = backend.render_frame(&scene, &config).expect("render wasm plugin frame");
    assert_eq!(img.width(), 40);
    assert_eq!(img.height(), 40);

    let px = img.get_pixel(20, 20);
    // Original: RGB(200, 50, 100) -> Inverted should be roughly (255-200, 255-50, 255-100) = (55, 205, 155)
    assert!((px[0] as i32 - 55).abs() <= 2, "r: {}", px[0]);
    assert!((px[1] as i32 - 205).abs() <= 2, "g: {}", px[1]);
    assert!((px[2] as i32 - 155).abs() <= 2, "b: {}", px[2]);
}

#[test]
fn test_missing_or_corrupt_wasm_plugin_fails_cleanly() {
    let backend = TinySkiaBackend::new();
    let config = FrameConfig::new(20, 20, 0, 30.0);

    // Corrupt bytes
    let corrupt_layer = SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: rust_motion_rasterizer::scene::MaskMode::Alpha,
        filters: vec![SceneFilter::WasmPlugin {
            plugin_name: "bogus.wasm".to_string(),
            wasm_bytes: Some(vec![1, 2, 3, 4, 5]),
            params: json!({}),
        }],
        shadow: None,
        children: vec![],
    };

    let mut scene = Scene::new();
    scene.push(corrupt_layer);
    assert!(backend.render_frame(&scene, &config).is_err());

    // Missing file from filesystem
    let missing_layer = SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: rust_motion_rasterizer::scene::MaskMode::Alpha,
        filters: vec![SceneFilter::WasmPlugin {
            plugin_name: "nonexistent_file_xyz_123.wasm".to_string(),
            wasm_bytes: None,
            params: json!({}),
        }],
        shadow: None,
        children: vec![],
    };

    let mut scene2 = Scene::new();
    scene2.push(missing_layer);
    assert!(backend.render_frame(&scene2, &config).is_err());
}
