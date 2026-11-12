//! Inspect commands and capability reporting for backends, codecs, and compositions.

use serde::{Deserialize, Serialize};

use crate::composition::built_in_registry;
use crate::doctor::{check_command_output, check_ffmpeg_encoder_supported};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackendCapability {
    pub name: String,
    pub tier: String,
    pub status: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodecCapability {
    pub codec: String,
    pub extensions: Vec<String>,
    pub is_video: bool,
    pub available: bool,
}

/// Inspect supported rendering backends and capability matrix.
pub fn run_inspect_backends() -> Vec<BackendCapability> {
    vec![
        BackendCapability {
            name: "native".into(),
            tier: "reference".into(),
            status: "supported".into(),
            description: "High-performance browser-free CPU rasterizer with deterministic frame evaluation (reference backend)".into(),
        },
        BackendCapability {
            name: "gpu".into(),
            tier: "experimental".into(),
            status: "experimental".into(),
            description: "Hardware-accelerated WGPU pipeline with GPU texture cache and compositor (driver-dependent)".into(),
        },
        BackendCapability {
            name: "browser".into(),
            tier: "optional".into(),
            status: "supported with fallback".into(),
            description: "Headless WebCodecs and DOM rendering via Node/Chromium worker".into(),
        },
    ]
}

/// Inspect supported codecs and system encoder availability.
pub fn run_inspect_codecs() -> Vec<CodecCapability> {
    let (ffmpeg_avail, _) = check_command_output("ffmpeg", &["-version"]);
    vec![
        CodecCapability {
            codec: "h264".into(),
            extensions: vec!["mp4".into()],
            is_video: true,
            available: ffmpeg_avail && check_ffmpeg_encoder_supported("libx264"),
        },
        CodecCapability {
            codec: "hevc".into(),
            extensions: vec!["mp4".into(), "mov".into()],
            is_video: true,
            available: ffmpeg_avail
                && (check_ffmpeg_encoder_supported("libx265")
                    || check_ffmpeg_encoder_supported("hevc")),
        },
        CodecCapability {
            codec: "vp9".into(),
            extensions: vec!["webm".into()],
            is_video: true,
            available: ffmpeg_avail && check_ffmpeg_encoder_supported("libvpx-vp9"),
        },
        CodecCapability {
            codec: "av1".into(),
            extensions: vec!["webm".into(), "mp4".into()],
            is_video: true,
            available: ffmpeg_avail && check_ffmpeg_encoder_supported("libsvtav1"),
        },
        CodecCapability {
            codec: "prores".into(),
            extensions: vec!["mov".into()],
            is_video: true,
            available: ffmpeg_avail && check_ffmpeg_encoder_supported("prores"),
        },
        CodecCapability {
            codec: "gif".into(),
            extensions: vec!["gif".into()],
            is_video: true,
            available: ffmpeg_avail,
        },
        CodecCapability {
            codec: "png".into(),
            extensions: vec!["png".into()],
            is_video: false,
            available: true,
        },
        CodecCapability {
            codec: "jpeg".into(),
            extensions: vec!["jpg".into(), "jpeg".into()],
            is_video: false,
            available: true,
        },
        CodecCapability {
            codec: "webp".into(),
            extensions: vec!["webp".into()],
            is_video: false,
            available: true,
        },
    ]
}

/// List compositions available to the native registry.
pub fn run_inspect_compositions() -> Vec<String> {
    built_in_registry()
        .ids()
        .into_iter()
        .map(str::to_owned)
        .collect()
}
