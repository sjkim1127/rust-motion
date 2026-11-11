//! Integration tests for rust-motion-wasm.

use rust_motion_composition::{NativeComposition, NativeCompositionContext};
use rust_motion_wasm::{
    wasm_fbm2d, wasm_interpolate, wasm_measure_spring, wasm_noise2d, wasm_noise3d, wasm_spring,
    TouchDesignerGenerativeComposition,
};

#[test]
fn test_wasm_interpolate_basic() {
    let input_range = [0.0, 1.0];
    let output_range = [100.0, 200.0];

    let result = wasm_interpolate(0.5, &input_range, &output_range, None, None).unwrap();
    assert!((result - 150.0).abs() < 1e-4);

    // Clamping
    let clamped_left = wasm_interpolate(
        -1.0,
        &input_range,
        &output_range,
        Some("clamp".into()),
        None,
    )
    .unwrap();
    assert_eq!(clamped_left, 100.0);

    let clamped_right =
        wasm_interpolate(2.0, &input_range, &output_range, None, Some("clamp".into())).unwrap();
    assert_eq!(clamped_right, 200.0);
}

#[test]
fn test_wasm_spring_physics() {
    let start_val = wasm_spring(0.0, 30.0, None, None, None, None);
    assert!((start_val - 0.0).abs() < 1e-3);

    // After settling
    let settled_val = wasm_spring(60.0, 30.0, None, None, None, None);
    assert!((settled_val - 1.0).abs() < 0.05);

    // Settle duration measurement
    let duration = wasm_measure_spring(30.0, None, None, None);
    assert!(duration > 10 && duration < 100);
}

#[test]
fn test_wasm_procedural_noise() {
    let n2 = wasm_noise2d(1.5, 2.5);
    assert!((-1.0..=1.0).contains(&n2));

    let n3 = wasm_noise3d(0.5, 1.5, 2.5);
    assert!((-1.0..=1.0).contains(&n3));

    let fbm = wasm_fbm2d(0.1, 0.2, 4, Some(2.0), Some(0.5));
    assert!((-1.5..=1.5).contains(&fbm));
}

#[test]
fn test_touch_designer_generative_composition() {
    let comp = TouchDesignerGenerativeComposition;
    assert_eq!(comp.id(), "TouchDesignerGenerative");

    let context = NativeCompositionContext {
        width: 1280,
        height: 720,
        fps: 30.0,
        duration_in_frames: 150,
    };

    let props = serde_json::json!({
        "title": "Test Generative TOP"
    });

    let scene = comp.render(15, &props, context).unwrap();
    // Verify nodes were pushed
    assert!(scene.nodes.len() > 10);
}
