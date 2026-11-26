//! FFmpeg argument construction and frame-step source mapping.

use super::*;

#[test]
fn test_pipe_config_ffmpeg_args() {
    let config = PipeConfig::new(1920, 1080, 30.0, 10, "/tmp/out.mp4");
    let args = build_pipe_ffmpeg_args(&config);
    assert!(
        args.contains(&"rawvideo".to_string()),
        "args should contain rawvideo"
    );
    assert!(
        args.contains(&"1920x1080".to_string()),
        "args should contain resolution"
    );
    assert!(
        args.contains(&"pipe:0".to_string()),
        "args should read from stdin"
    );
    assert!(
        args.contains(&"/tmp/out.mp4".to_string()),
        "args should contain output path"
    );
    assert!(args.contains(&"-an".to_string()));
}

#[test]
fn scaled_pipe_uses_scaled_rawvideo_dimensions() {
    let config = PipeConfig::new(640, 360, 30.0, 2, "/tmp/out.mp4").with_scale(1.5);
    let args = build_pipe_ffmpeg_args(&config);
    assert!(args.contains(&"960x540".to_string()));
    assert_eq!(config.output_dimensions().unwrap(), (960, 540));
}

#[test]
fn test_codec_specific_ffmpeg_args() {
    let cases = [
        (VideoCodec::H264, "out.mp4", "libx264"),
        (VideoCodec::H265, "out.mp4", "libx265"),
        (VideoCodec::Vp9, "out.webm", "libvpx-vp9"),
        (VideoCodec::ProRes, "out.mov", "prores_ks"),
    ];
    for (codec, output, encoder) in cases {
        let config = PipeConfig::new(64, 64, 30.0, 2, output)
            .with_codec(codec)
            .with_hw_accel(HwAccel::Disabled);
        let args = build_pipe_ffmpeg_args(&config);
        assert!(args.contains(&encoder.to_string()), "missing {encoder}");
        assert_eq!(args.last(), Some(&output.to_string()));
    }

    #[cfg(target_os = "macos")]
    {
        let config = PipeConfig::new(64, 64, 30.0, 2, "out.mp4")
            .with_codec(VideoCodec::H264)
            .with_hw_accel(HwAccel::VideoToolbox);
        let args = build_pipe_ffmpeg_args(&config);
        assert!(args.contains(&"h264_videotoolbox".to_string()));

        let config_hevc = PipeConfig::new(64, 64, 30.0, 2, "out.mp4")
            .with_codec(VideoCodec::H265)
            .with_hw_accel(HwAccel::VideoToolbox);
        let args_hevc = build_pipe_ffmpeg_args(&config_hevc);
        assert!(args_hevc.contains(&"hevc_videotoolbox".to_string()));
    }

    let av1 = build_pipe_ffmpeg_args(
        &PipeConfig::new(64, 64, 30.0, 2, "out.webm").with_codec(VideoCodec::Av1),
    );
    assert!(av1
        .iter()
        .any(|arg| arg == "libsvtav1" || arg == "libaom-av1"));

    let gif = build_pipe_ffmpeg_args(
        &PipeConfig::new(64, 64, 30.0, 2, "out.gif").with_codec(VideoCodec::Gif),
    );
    assert!(gif.iter().any(|arg| arg.contains("palettegen")));
    assert!(gif.contains(&"-an".to_string()));
}

#[test]
fn video_frame_step_maps_source_frames_and_output_fps() {
    let config = PipeConfig::new(64, 64, 30.0, 3, "out.gif")
        .with_codec(VideoCodec::Gif)
        .with_frame_start(10)
        .with_frame_step(2);
    let args = build_pipe_ffmpeg_args(&config);
    assert!(args.windows(2).any(|pair| pair == ["-r", "15"]));
    assert!(validate_pipe_config(&config).is_ok());

    let video = PipeConfig::new(64, 64, 30.0, 3, "out.mp4")
        .with_frame_step(2)
        .with_codec(VideoCodec::H264);
    let args = build_pipe_ffmpeg_args(&video);
    assert!(args.windows(2).any(|pair| pair == ["-r", "15"]));
    assert!(validate_pipe_config(&video).is_ok());
}

#[test]
fn frame_step_preserves_sampled_video_duration_for_audio() {
    let config = PipeConfig::new(64, 64, 30.0, 3, "out.mp4")
        .with_codec(VideoCodec::H264)
        .with_frame_step(2)
        .with_audio_tracks([AudioTrack::new("music.wav")]);
    let track = AudioTrack::new("music.wav");

    assert_eq!(active_audio_tracks(&config).len(), 1);
    let filter = build_audio_filter(&config, &[&track]);
    assert!(filter.contains("atrim=duration=0.200000000"));
}
