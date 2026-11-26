//! FFmpeg command-line construction and encoder availability probing.

use super::*;

/// Build FFmpeg arguments for rawvideo stdin pipe and the selected codec.
pub fn build_pipe_ffmpeg_args(config: &PipeConfig) -> Vec<String> {
    let (width, height) = config
        .output_dimensions()
        .unwrap_or((config.width, config.height));
    let mut args = vec![
        "-y".into(), // overwrite output
        "-loglevel".into(),
        "error".into(),
        "-f".into(),
        "rawvideo".into(), // input format
        "-pix_fmt".into(),
        "rgba".into(), // pixel format
        "-s".into(),
        format!("{}x{}", width, height),
        "-r".into(),
        format!("{}", config.fps / config.frame_step as f64),
        "-i".into(),
        "pipe:0".into(), // read from stdin
    ];

    let audio_tracks = active_audio_tracks(config);
    for track in &audio_tracks {
        if track.looped {
            args.extend(["-stream_loop".into(), "-1".into()]);
        }
        args.extend(["-i".into(), track.src.clone()]);
    }

    if config.codec == VideoCodec::Gif {
        args.extend([
            "-filter_complex".into(),
            "[0:v]split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse[gif]".into(),
            "-map".into(),
            "[gif]".into(),
            "-an".into(),
        ]);
    } else {
        args.extend(["-map".into(), "0:v:0".into()]);
        if audio_tracks.is_empty() {
            args.push("-an".into());
        } else {
            let audio_codec = match config.codec {
                VideoCodec::Vp9 | VideoCodec::Av1 => "libopus",
                VideoCodec::ProRes => "pcm_s16le",
                VideoCodec::H264 | VideoCodec::H265 => "aac",
                VideoCodec::Gif => unreachable!(),
            };
            args.extend([
                "-filter_complex".into(),
                build_audio_filter(config, &audio_tracks),
                "-map".into(),
                "[aout]".into(),
                "-c:a".into(),
                audio_codec.into(),
            ]);
            if audio_codec != "pcm_s16le" {
                args.extend(["-b:a".into(), "192k".into()]);
            }
        }
    }

    let effective_hw = match config.hw_accel {
        HwAccel::Auto => {
            if cfg!(target_os = "macos") {
                HwAccel::VideoToolbox
            } else {
                HwAccel::Disabled
            }
        }
        other => other,
    };

    let auto_bitrate = {
        let pixels = (width as u64) * (height as u64);
        if pixels >= 3840 * 2160 {
            "30M"
        } else if pixels >= 1920 * 1080 {
            "10M"
        } else {
            "4M"
        }
    };

    match config.codec {
        VideoCodec::H264 => match effective_hw {
            HwAccel::VideoToolbox => args.extend([
                "-c:v".into(),
                "h264_videotoolbox".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
                "-b:v".into(),
                auto_bitrate.into(),
                "-movflags".into(),
                "+faststart".into(),
            ]),
            HwAccel::Nvenc => args.extend([
                "-c:v".into(),
                "h264_nvenc".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
                "-cq".into(),
                config.crf.to_string(),
                "-preset".into(),
                "p4".into(),
                "-movflags".into(),
                "+faststart".into(),
            ]),
            _ => args.extend([
                "-c:v".into(),
                "libx264".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
                "-crf".into(),
                config.crf.to_string(),
                "-preset".into(),
                config.preset.clone(),
                "-movflags".into(),
                "+faststart".into(),
            ]),
        },
        VideoCodec::H265 => match effective_hw {
            HwAccel::VideoToolbox => args.extend([
                "-c:v".into(),
                "hevc_videotoolbox".into(),
                "-tag:v".into(),
                "hvc1".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
                "-b:v".into(),
                auto_bitrate.into(),
                "-movflags".into(),
                "+faststart".into(),
            ]),
            HwAccel::Nvenc => args.extend([
                "-c:v".into(),
                "hevc_nvenc".into(),
                "-tag:v".into(),
                "hvc1".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
                "-cq".into(),
                config.crf.to_string(),
                "-preset".into(),
                "p4".into(),
                "-movflags".into(),
                "+faststart".into(),
            ]),
            _ => args.extend([
                "-c:v".into(),
                "libx265".into(),
                "-tag:v".into(),
                "hvc1".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
                "-crf".into(),
                config.crf.to_string(),
                "-preset".into(),
                config.preset.clone(),
                "-movflags".into(),
                "+faststart".into(),
            ]),
        },
        VideoCodec::Vp9 => args.extend([
            "-c:v".into(),
            "libvpx-vp9".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-crf".into(),
            config.crf.min(63).to_string(),
            "-b:v".into(),
            "0".into(),
        ]),
        VideoCodec::Av1 => {
            if ffmpeg_has_encoder("libsvtav1") {
                args.extend([
                    "-c:v".into(),
                    "libsvtav1".into(),
                    "-pix_fmt".into(),
                    "yuv420p".into(),
                    "-crf".into(),
                    config.crf.min(63).to_string(),
                    "-preset".into(),
                    "8".into(),
                ]);
            } else {
                args.extend([
                    "-c:v".into(),
                    "libaom-av1".into(),
                    "-pix_fmt".into(),
                    "yuv420p".into(),
                    "-crf".into(),
                    config.crf.min(63).to_string(),
                    "-b:v".into(),
                    "0".into(),
                    "-cpu-used".into(),
                    "6".into(),
                ]);
            }
        }
        VideoCodec::ProRes => args.extend([
            "-c:v".into(),
            "prores_ks".into(),
            "-profile:v".into(),
            "3".into(),
            "-pix_fmt".into(),
            "yuv422p10le".into(),
        ]),
        VideoCodec::Gif => {}
    }
    args.extend([
        "-t".into(),
        format!(
            "{:.9}",
            config.duration_in_frames as f64 / (config.fps / config.frame_step as f64)
        ),
        config.output.to_string_lossy().to_string(),
    ]);
    args
}

pub(super) fn ffmpeg_has_encoder(name: &str) -> bool {
    static ENCODERS: OnceLock<String> = OnceLock::new();
    ENCODERS
        .get_or_init(|| {
            Command::new("ffmpeg")
                .args(["-hide_banner", "-encoders"])
                .output()
                .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
                .unwrap_or_default()
        })
        .split_whitespace()
        .any(|encoder| encoder == name)
}
