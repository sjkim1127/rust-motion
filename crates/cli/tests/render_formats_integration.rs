use rust_motion_cli::{
    execute_render_command, RenderBackend, RenderCodec, RenderRequest, ValidationError,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_dir() -> PathBuf {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "rust_motion_cli_formats_{}_{}_{}",
        std::process::id(),
        nonce,
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ))
}

fn request(output: &Path, codec: RenderCodec) -> RenderRequest {
    RenderRequest {
        composition: Some("HelloWorld".into()),
        script: None,
        props: None,
        output: output.to_path_buf(),
        audio: Vec::new(),
        width: 65,
        height: 49,
        scale: 1.0,
        fps: 30.0,
        duration: 20,
        backend: RenderBackend::Native,
        codec,
        frame_start: 7,
        frame_end: None,
        frame_step: 1,
        concurrency: None,
        timeout_seconds: None,
        crf: 18,
        preset: "fast".into(),
        hw_accel: rust_motion_rasterizer::HwAccel::Disabled,
        sandbox_roots: Vec::new(),
        permissive: false,
    }
}

#[tokio::test]
async fn still_formats_render_the_selected_frame_end_to_end() {
    let temp = unique_temp_dir();
    std::fs::create_dir_all(&temp).unwrap();
    for (codec, file_name) in [
        (RenderCodec::Png, "still.png"),
        (RenderCodec::Jpeg, "still.jpg"),
        (RenderCodec::Webp, "still.webp"),
    ] {
        let output = temp.join(file_name);
        execute_render_command(&request(&output, codec))
            .await
            .unwrap_or_else(|error| panic!("{codec:?} still render failed: {error}"));
        let bytes = std::fs::read(&output).expect("output image must exist");
        match codec {
            RenderCodec::Png => assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n")),
            RenderCodec::Jpeg => assert!(bytes.starts_with(&[0xff, 0xd8, 0xff])),
            RenderCodec::Webp => {
                assert!(bytes.starts_with(b"RIFF"));
                assert_eq!(bytes.get(8..12), Some(b"WEBP".as_slice()));
            }
            _ => unreachable!(),
        }
    }
    std::fs::remove_dir_all(temp).unwrap();
}

#[tokio::test]
async fn invalid_range_extension_timeout_and_audio_fail_before_rendering() {
    let temp = unique_temp_dir();
    std::fs::create_dir_all(&temp).unwrap();

    let mut invalid_range = request(&temp.join("range.png"), RenderCodec::Png);
    invalid_range.frame_start = 10;
    invalid_range.frame_end = Some(9);
    let error = execute_render_command(&invalid_range).await.unwrap_err();
    assert!(matches!(
        error.downcast_ref::<ValidationError>(),
        Some(ValidationError::InvalidFrameRange { .. })
    ));

    let wrong_extension = request(&temp.join("wrong.mp4"), RenderCodec::Png);
    let error = execute_render_command(&wrong_extension).await.unwrap_err();
    assert!(matches!(
        error.downcast_ref::<ValidationError>(),
        Some(ValidationError::InvalidOutputExtension { .. })
    ));

    let mut timeout = request(&temp.join("timeout.png"), RenderCodec::Png);
    timeout.timeout_seconds = Some(0);
    let error = execute_render_command(&timeout).await.unwrap_err();
    assert_eq!(
        error.downcast_ref::<ValidationError>(),
        Some(&ValidationError::InvalidTimeout)
    );

    let mut gif_audio = request(&temp.join("audio.gif"), RenderCodec::Gif);
    gif_audio.audio.push(temp.join("missing.wav"));
    let error = execute_render_command(&gif_audio).await.unwrap_err();
    assert!(matches!(
        error.downcast_ref::<ValidationError>(),
        Some(ValidationError::AudioNotSupported(_))
    ));

    let mut invalid_crf = request(&temp.join("quality.mp4"), RenderCodec::H264);
    invalid_crf.width = 64;
    invalid_crf.height = 48;
    invalid_crf.crf = 52;
    let error = execute_render_command(&invalid_crf).await.unwrap_err();
    assert!(matches!(
        error.downcast_ref::<ValidationError>(),
        Some(ValidationError::InvalidCrf { .. })
    ));

    let mut invalid_preset = request(&temp.join("preset.mp4"), RenderCodec::H264);
    invalid_preset.width = 64;
    invalid_preset.height = 48;
    invalid_preset.preset = "instant".into();
    let error = execute_render_command(&invalid_preset).await.unwrap_err();
    assert_eq!(
        error.downcast_ref::<ValidationError>(),
        Some(&ValidationError::InvalidPreset("instant".into()))
    );

    assert_eq!(std::fs::read_dir(&temp).unwrap().count(), 0);
    std::fs::remove_dir_all(temp).unwrap();
}
