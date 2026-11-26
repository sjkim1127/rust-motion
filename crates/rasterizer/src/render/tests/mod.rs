//! Tests for the frame rendering coordinator.

use super::*;
use crate::scene::{Color, Scene, SceneNode};
use crate::tiny_skia_backend::TinySkiaBackend;
use crate::{BackendCapabilities, FrameConfig, RasterizerBackend};
use image::RgbaImage;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

mod audio;
mod callbacks;
mod encode;
mod ffmpeg_args;
mod frames;
mod still;
mod streaming;

pub(super) fn unique_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "rust_motion_{label}_{}_{}",
        std::process::id(),
        nonce
    ))
}

pub(super) fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub(super) fn solid_scene(color: Color) -> impl Fn(u32) -> Scene + Send + Sync {
    move |_frame| {
        let mut s = Scene::new();
        s.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 64.0,
            h: 64.0,
            fill: color,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });
        s
    }
}

pub(super) struct BrowserLikeBackend;

impl RasterizerBackend for BrowserLikeBackend {
    fn render_frame(&self, scene: &Scene, config: &FrameConfig) -> Result<RgbaImage, RasterError> {
        TinySkiaBackend::headless().render_frame(scene, config)
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            native_scene: false,
            browser_runtime: true,
            gpu_accelerated: true,
            supports_streaming: false,
        }
    }
}
