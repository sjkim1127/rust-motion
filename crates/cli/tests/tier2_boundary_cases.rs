//! Tier 2: Boundary & Corner Cases E2E Tests
//!
//! Tests edge cases: empty composition name, missing props file, zero/odd resolutions,
//! zero FPS/duration, and malformed props JSON.

use rust_motion_cli::{
    execute_render_command, validate_composition_source, validate_render_params, RenderBackend,
    RenderRequest, ValidationError,
};
use std::fs;
use std::path::PathBuf;

#[test]
fn test_boundary_empty_composition() {
    let res = validate_render_params("", None, 1920, 1080, 30.0, 150);
    assert_eq!(res, Err(ValidationError::EmptyComposition));

    let res_whitespace = validate_render_params("   ", None, 1920, 1080, 30.0, 150);
    assert_eq!(res_whitespace, Err(ValidationError::EmptyComposition));
}

#[test]
fn test_boundary_composition_source_selection() {
    assert_eq!(
        validate_composition_source(None, None),
        Err(ValidationError::MissingCompositionSource)
    );
    let script = PathBuf::from("composition.rhai");
    assert_eq!(
        validate_composition_source(Some("HelloWorld"), Some(&script)),
        Err(ValidationError::ConflictingCompositionSources)
    );
    assert_eq!(
        validate_composition_source(None, Some(&script)),
        Err(ValidationError::ScriptFileNotFound(script))
    );
}

#[test]
fn test_boundary_missing_props_file() {
    let missing_path = PathBuf::from("/non_existent_directory/missing_props_file.json");
    let res = validate_render_params("HelloWorld", Some(&missing_path), 1920, 1080, 30.0, 150);
    assert_eq!(res, Err(ValidationError::PropsFileNotFound(missing_path)));
}

#[test]
fn test_boundary_zero_resolution() {
    let res_zero_w = validate_render_params("HelloWorld", None, 0, 1080, 30.0, 150);
    assert_eq!(
        res_zero_w,
        Err(ValidationError::InvalidZeroResolution(0, 1080))
    );

    let res_zero_h = validate_render_params("HelloWorld", None, 1920, 0, 30.0, 150);
    assert_eq!(
        res_zero_h,
        Err(ValidationError::InvalidZeroResolution(1920, 0))
    );
}

#[test]
fn test_boundary_odd_resolution() {
    let res_odd_w = validate_render_params("HelloWorld", None, 1921, 1080, 30.0, 150);
    assert_eq!(
        res_odd_w,
        Err(ValidationError::InvalidOddResolution(1921, 1080))
    );

    let res_odd_h = validate_render_params("HelloWorld", None, 1920, 1081, 30.0, 150);
    assert_eq!(
        res_odd_h,
        Err(ValidationError::InvalidOddResolution(1920, 1081))
    );
}

#[test]
fn test_boundary_zero_fps() {
    let res_zero_fps = validate_render_params("HelloWorld", None, 1920, 1080, 0.0, 150);
    assert_eq!(
        res_zero_fps,
        Err(ValidationError::InvalidFps("0".to_string()))
    );

    let res_neg_fps = validate_render_params("HelloWorld", None, 1920, 1080, -10.0, 150);
    assert_eq!(
        res_neg_fps,
        Err(ValidationError::InvalidFps("-10".to_string()))
    );

    let res_nan_fps = validate_render_params("HelloWorld", None, 1920, 1080, f64::NAN, 150);
    assert!(matches!(res_nan_fps, Err(ValidationError::InvalidFps(_))));
}

#[test]
fn test_boundary_zero_duration() {
    let res_zero_dur = validate_render_params("HelloWorld", None, 1920, 1080, 30.0, 0);
    assert_eq!(res_zero_dur, Err(ValidationError::InvalidDuration(0)));
}

#[tokio::test]
async fn test_boundary_valid_props_file_check() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rust_motion_test_boundary_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    let props_path = temp_dir.join("valid_props.json");
    fs::write(&props_path, r#"{"title": "Test Title"}"#).unwrap();

    let res = validate_render_params("HelloWorld", Some(&props_path), 1920, 1080, 30.0, 150);
    assert!(res.is_ok());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_boundary_malformed_props_fail_before_rendering() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rust_motion_test_malformed_props_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    let props_path = temp_dir.join("invalid.json");
    fs::write(&props_path, "{not valid json").unwrap();
    let output = temp_dir.join("must-not-exist.mp4");
    let request = RenderRequest {
        composition: Some("HelloWorld".into()),
        script: None,
        props: Some(props_path),
        output: output.clone(),
        audio: Vec::new(),
        width: 64,
        height: 64,
        scale: 1.0,
        fps: 30.0,
        duration: 1,
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

    let error = execute_render_command(&request).await.unwrap_err();
    assert!(error.to_string().contains("Invalid props JSON"));
    assert!(!output.exists());
    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_boundary_missing_audio_fails_before_rendering() {
    let missing = PathBuf::from("/non_existent_directory/missing_audio.wav");
    let request = RenderRequest {
        composition: Some("HelloWorld".into()),
        script: None,
        props: None,
        output: PathBuf::from("must-not-exist.mp4"),
        audio: vec![missing.clone()],
        width: 64,
        height: 64,
        scale: 1.0,
        fps: 30.0,
        duration: 1,
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

    let error = execute_render_command(&request).await.unwrap_err();
    assert_eq!(
        error.downcast_ref::<ValidationError>(),
        Some(&ValidationError::AudioFileNotFound(missing))
    );
}
