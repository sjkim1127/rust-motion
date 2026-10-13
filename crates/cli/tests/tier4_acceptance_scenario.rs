//! Tier 4: Real-World Acceptance Scenario E2E Test
//!
//! Replicates real-world CLI invocation:
//! `cargo run -p rust-motion-cli -- render -c HelloWorld -p data.json -o output.mp4 --width 1280 --height 720 --fps 30 --duration 60`

use rust_motion_cli::{execute_render_command, RenderBackend, RenderRequest};
use std::fs;

#[tokio::test]
async fn test_tier4_real_world_acceptance_scenario() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rust_motion_tier4_acceptance_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    // Create data.json input props file
    let props_path = temp_dir.join("data.json");
    let props_content = r#"{
        "title": "HelloWorld",
        "author": "Rust Motion E2E Suite",
        "fps": 30,
        "width": 1280,
        "height": 720
    }"#;
    fs::write(&props_path, props_content).unwrap();

    let output_mp4 = temp_dir.join("output.mp4");

    // Execute full acceptance render workflow via Native CPU rasterizer
    let request = RenderRequest {
        composition: Some("HelloWorld".into()),
        script: None,
        props: Some(props_path),
        output: output_mp4.clone(),
        audio: Vec::new(),
        width: 1280,
        height: 720,
        scale: 1.0,
        fps: 30.0,
        duration: 60,
        backend: RenderBackend::Native,
        codec: rust_motion_cli::RenderCodec::H264,
        frame_start: 0,
        frame_end: None,
        frame_step: 1,
        concurrency: None,
        timeout_seconds: None,
        crf: 18,
        preset: "fast".into(),
        hw_accel: rust_motion_rasterizer::HwAccel::Disabled,
        sandbox_roots: Vec::new(),
        permissive: false,
    };
    let result = execute_render_command(&request).await;

    assert!(
        result.is_ok(),
        "Acceptance render failed: {:?}",
        result.err()
    );

    // 1. Verify output file exists
    assert!(
        output_mp4.exists(),
        "Target output.mp4 file was not created"
    );

    // 2. Verify non-zero file size
    let metadata = fs::metadata(&output_mp4).unwrap();
    assert!(metadata.len() > 0, "Output MP4 file is zero bytes");

    // 3. Verify MP4 file header container format ('ftyp' atom)
    let bytes = fs::read(&output_mp4).unwrap();
    assert!(bytes.len() > 12);
    let ftyp_slice = &bytes[4..8];
    assert_eq!(
        ftyp_slice, b"ftyp",
        "Produced MP4 file missing 'ftyp' container signature"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}
