//! End-to-end encodes through FFmpeg: containers, GIF, cancellation and failure handling.

use super::*;

#[test]
fn test_gif_pipe_writes_a_real_animation() {
    if !ffmpeg_available() {
        eprintln!("Skipping GIF pipe test: FFmpeg is unavailable");
        return;
    }
    let temp = unique_temp_dir("gif");
    std::fs::create_dir_all(&temp).unwrap();
    let output = temp.join("animation.gif");
    let config = PipeConfig::new(32, 24, 10.0, 2, &output).with_codec(VideoCodec::Gif);
    render_to_ffmpeg_pipe(&TinySkiaBackend::headless(), &config, |frame| {
        solid_scene(Color::rgb((frame * 100) as u8, 20, 30))(frame)
    })
    .unwrap();
    let bytes = std::fs::read(&output).unwrap();
    assert!(bytes.starts_with(b"GIF8"));
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn test_available_video_codecs_write_real_containers() {
    if !ffmpeg_available() {
        eprintln!("Skipping codec pipe test: FFmpeg is unavailable");
        return;
    }
    let temp = unique_temp_dir("codecs");
    std::fs::create_dir_all(&temp).unwrap();
    let av1_encoder = if ffmpeg_has_encoder("libsvtav1") {
        "libsvtav1"
    } else {
        "libaom-av1"
    };
    let cases = [
        (VideoCodec::H264, "h264.mp4", "libx264"),
        (VideoCodec::H265, "h265.mp4", "libx265"),
        (VideoCodec::Vp9, "vp9.webm", "libvpx-vp9"),
        (VideoCodec::Av1, "av1.webm", av1_encoder),
        (VideoCodec::ProRes, "prores.mov", "prores_ks"),
    ];

    for (codec, file_name, encoder) in cases {
        if !ffmpeg_has_encoder(encoder) {
            eprintln!("Skipping {codec:?} pipe test: {encoder} is unavailable");
            continue;
        }
        let output = temp.join(file_name);
        let config = PipeConfig::new(64, 64, 1.0, 1, &output)
            .with_codec(codec)
            .with_concurrency(1)
            .with_quality(35, "fast");
        render_to_ffmpeg_pipe(
            &TinySkiaBackend::headless(),
            &config,
            solid_scene(Color::rgb(40, 80, 120)),
        )
        .unwrap_or_else(|error| panic!("{codec:?} encode failed: {error}"));

        let bytes = std::fs::read(&output).unwrap();
        assert!(!bytes.is_empty(), "{codec:?} output is empty");
        match codec {
            VideoCodec::H264 | VideoCodec::H265 | VideoCodec::ProRes => {
                assert_eq!(&bytes[4..8], b"ftyp", "{codec:?} is not ISO BMFF")
            }
            VideoCodec::Vp9 | VideoCodec::Av1 => {
                assert!(bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]))
            }
            VideoCodec::Gif => unreachable!(),
        }
    }
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn test_cancelled_and_timed_out_render_stop_before_ffmpeg() {
    let temp = unique_temp_dir("control");
    std::fs::create_dir_all(&temp).unwrap();
    let cancelled_output = temp.join("cancelled.mp4");
    let cancelled_control = RenderControl::new();
    cancelled_control.cancellation_token().cancel();
    let cancelled =
        PipeConfig::new(32, 24, 30.0, 2, &cancelled_output).with_control(cancelled_control);
    let error = render_to_ffmpeg_pipe(
        &TinySkiaBackend::headless(),
        &cancelled,
        solid_scene(Color::rgb(1, 2, 3)),
    )
    .unwrap_err();
    assert!(matches!(error, RasterError::Cancelled));
    assert!(!cancelled_output.exists());

    let timeout_output = temp.join("timeout.mp4");
    let timed_out = PipeConfig::new(32, 24, 30.0, 2, &timeout_output)
        .with_control(RenderControl::new().with_timeout(Duration::ZERO));
    let error = render_to_ffmpeg_pipe(
        &TinySkiaBackend::headless(),
        &timed_out,
        solid_scene(Color::rgb(1, 2, 3)),
    )
    .unwrap_err();
    assert!(matches!(error, RasterError::Timeout));
    assert!(!timeout_output.exists());

    let mid_render_output = temp.join("mid-render-cancelled.mp4");
    let cancellation = RenderCancellationToken::default();
    let callback_cancellation = cancellation.clone();
    let mid_render_control = RenderControl::new()
        .with_cancellation(cancellation)
        .with_progress(move |progress| {
            if progress.completed_frames == 1 {
                callback_cancellation.cancel();
            }
        });
    let mid_render =
        PipeConfig::new(32, 24, 30.0, 30, &mid_render_output).with_control(mid_render_control);
    let error = render_to_ffmpeg_pipe(
        &TinySkiaBackend::headless(),
        &mid_render,
        solid_scene(Color::rgb(1, 2, 3)),
    )
    .unwrap_err();
    assert!(matches!(error, RasterError::Cancelled));
    assert!(!mid_render_output.exists());

    let preserved_output = temp.join("preserved.mp4");
    std::fs::write(&preserved_output, b"existing valid output").unwrap();
    let preserved_control = RenderControl::new();
    preserved_control.cancellation_token().cancel();
    let preserved =
        PipeConfig::new(32, 24, 30.0, 2, &preserved_output).with_control(preserved_control);
    let error = render_to_ffmpeg_pipe(
        &TinySkiaBackend::headless(),
        &preserved,
        solid_scene(Color::rgb(1, 2, 3)),
    )
    .unwrap_err();
    assert!(matches!(error, RasterError::Cancelled));
    assert_eq!(
        std::fs::read(&preserved_output).unwrap(),
        b"existing valid output"
    );
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn failed_render_preserves_existing_output() {
    let temp = unique_temp_dir("preserve_output");
    std::fs::create_dir_all(&temp).unwrap();
    let output = temp.join("existing.mp4");
    let original = b"existing output must survive";
    std::fs::write(&output, original).unwrap();
    let config = PipeConfig::new(32, 24, 30.0, 3, &output).with_concurrency(1);

    let error = render_to_ffmpeg_pipe_fallible(&TinySkiaBackend::headless(), &config, |frame| {
        if frame == 1 {
            Err(RasterError::Scene("intentional frame failure".into()))
        } else {
            Ok(solid_scene(Color::rgb(10, 20, 30))(frame))
        }
    })
    .unwrap_err();

    assert!(matches!(error, RasterError::Frame { frame: 1, .. }));
    assert_eq!(std::fs::read(&output).unwrap(), original);
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn native_h264_streaming_pipe_renders_valid_faststart_mp4_without_ffmpeg() {
    let temp = unique_temp_dir("native_h264_mp4");
    std::fs::create_dir_all(&temp).unwrap();
    let output = temp.join("native.mp4");
    let config = PipeConfig::new(160, 96, 24.0, 12, &output)
        .with_encoder_backend(EncoderBackend::Native)
        .with_quality(22, "fast")
        .with_concurrency(2);

    render_to_video(&TinySkiaBackend::headless(), &config, |frame| {
        let mut scene = Scene::new();
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 160.0,
            h: 96.0,
            fill: Color::rgb(16, 28, 48),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });
        scene.push(SceneNode::Rect {
            x: 12.0 + frame as f32 * 8.0,
            y: 24.0,
            w: 40.0,
            h: 40.0,
            fill: Color::rgb(230, 110, 40),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 6.0,
        });
        scene
    })
    .unwrap();

    let bytes = std::fs::read(&output).unwrap();
    assert_eq!(&bytes[4..8], b"ftyp");
    let moov_pos = bytes
        .windows(4)
        .position(|w| w == b"moov")
        .expect("missing moov");
    let mdat_pos = bytes
        .windows(4)
        .position(|w| w == b"mdat")
        .expect("missing mdat");
    assert!(moov_pos < mdat_pos, "expected faststart moov before mdat");
    assert!(bytes.windows(4).any(|w| w == b"colr"));
    assert!(bytes.windows(4).any(|w| w == b"nclx"));
    assert!(bytes.windows(4).any(|w| w == b"avcC"));

    if ffmpeg_available() {
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name,width,height,nb_frames,color_primaries",
                "-of",
                "default=noprint_wrappers=1",
            ])
            .arg(&output)
            .output()
            .unwrap();
        assert!(probe.status.success());
        let probe_text = String::from_utf8_lossy(&probe.stdout);
        assert!(probe_text.contains("codec_name=h264"), "{probe_text}");
        assert!(probe_text.contains("width=160"), "{probe_text}");
        assert!(probe_text.contains("height=96"), "{probe_text}");
        assert!(probe_text.contains("nb_frames=12"), "{probe_text}");
        assert!(probe_text.contains("color_primaries=bt709"), "{probe_text}");

        let decode = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&output)
            .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
            .output()
            .unwrap();
        assert!(
            decode.status.success(),
            "ffmpeg decode failed: {}",
            String::from_utf8_lossy(&decode.stderr)
        );
        assert_eq!(decode.stdout.len(), 12 * 160 * 96 * 3);
    }

    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn native_h264_respects_cancellation_and_rejects_unsupported_inputs() {
    let temp = unique_temp_dir("native_h264_errors");
    std::fs::create_dir_all(&temp).unwrap();

    let cancelled_out = temp.join("cancelled.mp4");
    let control = RenderControl::new();
    control.cancellation_token().cancel();
    let cfg = PipeConfig::new(64, 64, 30.0, 4, &cancelled_out)
        .with_encoder_backend(EncoderBackend::Native)
        .with_control(control);
    let err = render_to_video(
        &TinySkiaBackend::headless(),
        &cfg,
        solid_scene(Color::rgb(1, 2, 3)),
    )
    .unwrap_err();
    assert!(matches!(err, RasterError::Cancelled));
    assert!(!cancelled_out.exists());

    let vp9_out = temp.join("unsupported.webm");
    let cfg_vp9 = PipeConfig::new(64, 64, 30.0, 2, &vp9_out)
        .with_encoder_backend(EncoderBackend::Native)
        .with_codec(VideoCodec::Vp9);
    let err = render_to_video(
        &TinySkiaBackend::headless(),
        &cfg_vp9,
        solid_scene(Color::rgb(1, 2, 3)),
    )
    .unwrap_err();
    assert!(matches!(err, RasterError::ImageEncode(_)));
    assert!(!vp9_out.exists());

    let dummy_wav = temp.join("dummy.wav");
    std::fs::write(&dummy_wav, b"RIFF").unwrap();
    let audio_out = temp.join("audio.mp4");
    let cfg_audio = PipeConfig::new(64, 64, 30.0, 2, &audio_out)
        .with_encoder_backend(EncoderBackend::Native)
        .with_security_policy(crate::security::MediaSecurityPolicy::Permissive)
        .with_audio_tracks([AudioTrack::new(dummy_wav.to_string_lossy().into_owned())]);
    let err = render_to_video(
        &TinySkiaBackend::headless(),
        &cfg_audio,
        solid_scene(Color::rgb(1, 2, 3)),
    )
    .unwrap_err();
    assert!(
        matches!(err, RasterError::ImageEncode(_)),
        "expected ImageEncode for audio tracks with native encoder, got {err:?}"
    );
    assert!(!audio_out.exists());

    std::fs::remove_dir_all(temp).unwrap();
}
