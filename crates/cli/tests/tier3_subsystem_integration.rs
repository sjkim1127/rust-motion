//! Tier 3: Subsystem Integration E2E Tests
//!
//! Validates contracts between HTTP server manager, Native CPU/GPU rasterizer,
//! and FFmpeg MP4 encoding compiler.

use rust_motion_rasterizer::{
    render_all_frames, Color, NativeRenderConfig, Scene, SceneNode, TinySkiaBackend,
};
use rust_motion_renderer::{encode_frames, spawn_server, EncodeConfig};
use std::fs;

#[tokio::test]
async fn test_subsystem_http_server_lifecycle() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rust_motion_tier3_server_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    let index_file = temp_dir.join("index.html");
    fs::write(
        &index_file,
        "<html><body><h1>Rust Motion Subsystem Test</h1></body></html>",
    )
    .unwrap();

    let server_handle = spawn_server(0, &temp_dir)
        .await
        .expect("Failed to spawn server");
    assert!(server_handle.port() > 0);
    assert!(server_handle.url().starts_with("http://127.0.0.1:"));

    // Verify health endpoint
    let health_url = format!("{}/health", server_handle.url());
    let resp = reqwest::get(&health_url)
        .await
        .expect("Health check GET failed");
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert_eq!(body, "OK");

    // Clean server stop
    server_handle
        .stop()
        .await
        .expect("Server failed to stop cleanly");
    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_subsystem_native_rasterizer_frame_capture() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rust_motion_tier3_rasterizer_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    let frames_dir = temp_dir.join("frames");

    let backend = TinySkiaBackend::headless();
    let config = NativeRenderConfig::new(640, 360, 30.0, 3, &frames_dir);

    let frame_paths = render_all_frames(&backend, &config, |frame| {
        let mut scene = Scene::new();
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 640.0,
            h: 360.0,
            fill: Color::rgb(frame as u8 * 50, 50, 100),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });
        scene
    })
    .expect("render_all_frames failed");

    assert_eq!(frame_paths.len(), 3);

    for path in &frame_paths {
        assert!(
            path.exists(),
            "Frame PNG file missing at {}",
            path.display()
        );
        let metadata = fs::metadata(path).unwrap();
        assert!(
            metadata.len() > 0,
            "Frame PNG file is empty at {}",
            path.display()
        );

        // Validate PNG Magic Header Signature: \x89 PNG \r \n \x1a \n
        let bytes = fs::read(path).unwrap();
        assert!(bytes.len() >= 8);
        assert_eq!(
            &bytes[0..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
        );
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_subsystem_ffmpeg_mp4_encoding() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rust_motion_tier3_ffmpeg_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    let frames_dir = temp_dir.join("frames");

    let backend = TinySkiaBackend::headless();
    let config = NativeRenderConfig::new(640, 360, 30.0, 5, &frames_dir);

    let _frame_paths = render_all_frames(&backend, &config, |_frame| {
        let mut scene = Scene::new();
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 640.0,
            h: 360.0,
            fill: Color::rgb(255, 0, 0),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });
        scene
    })
    .expect("Frame rendering failed");

    let output_mp4 = temp_dir.join("output_test.mp4");
    let encode_cfg = EncodeConfig::h264(&frames_dir, &output_mp4, 30.0);

    encode_frames(&encode_cfg)
        .await
        .expect("FFmpeg encoding failed");

    assert!(output_mp4.exists(), "Output MP4 file does not exist");
    let metadata = fs::metadata(&output_mp4).unwrap();
    assert!(metadata.len() > 0, "Output MP4 file is empty");

    // Verify MP4 container header signature ("ftyp" atom)
    let bytes = fs::read(&output_mp4).unwrap();
    assert!(bytes.len() > 12);
    let ftyp_slice = &bytes[4..8];
    assert_eq!(
        ftyp_slice, b"ftyp",
        "MP4 file missing 'ftyp' container header"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}
