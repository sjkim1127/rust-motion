//! Rasterizer backend trait and frame configuration.

use crate::scene::Scene;
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error type for rasterization failures.
#[derive(Error, Debug)]
pub enum RasterError {
    #[error("Backend initialization failed: {0}")]
    Init(String),
    #[error("Failed to render frame {frame}: {reason}")]
    Frame { frame: u32, reason: String },
    #[error("Image encode error: {0}")]
    ImageEncode(String),
    #[error("Image asset error for {path}: {reason}")]
    ImageAsset { path: String, reason: String },
    #[error("Media asset error for {path}: {reason}")]
    MediaAsset { path: String, reason: String },
    #[error("Security violation for {path}: {reason}")]
    SecurityViolation { path: String, reason: String },
    #[error("Font asset error for {path}: {reason}")]
    FontAsset { path: String, reason: String },
    #[error("Scene compositing error: {0}")]
    Scene(String),
    #[error("Render cancelled")]
    Cancelled,
    #[error("Render timed out")]
    Timeout,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Per-frame rendering configuration.
#[derive(Debug, Clone)]
pub struct FrameConfig {
    pub width: u32,
    pub height: u32,
    pub frame: u32,
    pub fps: f64,
}

/// Capabilities exposed by a rendering backend.
///
/// A frontend (for example a Three.js/Chromium worker) can use this metadata
/// to select an appropriate scene path without guessing from the backend name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendCapabilities {
    pub native_scene: bool,
    pub browser_runtime: bool,
    pub gpu_accelerated: bool,
    pub supports_streaming: bool,
}

/// Monotonic counters exposed by a backend after a render.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BackendRenderStats {
    pub gpu_frames: u64,
    pub cpu_fallback_frames: u64,
    pub texture_cache_hits: u64,
    pub texture_cache_misses: u64,
    pub video_decode_ns: u64,
    pub texture_upload_ns: u64,
    pub gpu_submit_readback_ns: u64,
    pub browser_frame_ns: u64,
}

impl BackendRenderStats {
    /// Compute the counters attributable to one render invocation.
    pub fn delta_since(self, before: Self) -> Self {
        Self {
            gpu_frames: self.gpu_frames.saturating_sub(before.gpu_frames),
            cpu_fallback_frames: self
                .cpu_fallback_frames
                .saturating_sub(before.cpu_fallback_frames),
            texture_cache_hits: self
                .texture_cache_hits
                .saturating_sub(before.texture_cache_hits),
            texture_cache_misses: self
                .texture_cache_misses
                .saturating_sub(before.texture_cache_misses),
            video_decode_ns: self.video_decode_ns.saturating_sub(before.video_decode_ns),
            texture_upload_ns: self
                .texture_upload_ns
                .saturating_sub(before.texture_upload_ns),
            gpu_submit_readback_ns: self
                .gpu_submit_readback_ns
                .saturating_sub(before.gpu_submit_readback_ns),
            browser_frame_ns: self
                .browser_frame_ns
                .saturating_sub(before.browser_frame_ns),
        }
    }
}

impl FrameConfig {
    pub fn new(width: u32, height: u32, frame: u32, fps: f64) -> Self {
        Self {
            width,
            height,
            frame,
            fps,
        }
    }

    /// Current playback time in seconds.
    pub fn time_secs(&self) -> f64 {
        self.frame as f64 / self.fps
    }
}

/// Destination for frames produced by a streaming rasterizer.
///
/// Backends currently provide tightly packed RGBA8 bytes. Keeping this
/// boundary as a trait lets a future GPU-native encoder consume frames without
/// changing scene scheduling or rasterizer implementations.
pub trait FrameSink {
    fn consume(&mut self, frame: u32, rgba: &[u8]) -> Result<(), RasterError>;

    /// Attempt to acquire an OS-managed hardware pixel buffer (`IOSurface` / `CVPixelBuffer`)
    /// sized to the encoder output so a GPU backend can write directly into encoder memory
    /// without a CPU readback. Returns `Ok(None)` when the sink does not support zero-copy
    /// submission or requires CPU scaling.
    #[cfg(all(target_os = "macos", feature = "metal"))]
    fn try_acquire_hardware_pixel_buffer(
        &mut self,
    ) -> Result<Option<rust_motion_media_hwaccel::HardwarePixelBuffer>, RasterError> {
        Ok(None)
    }

    /// Submit a previously acquired [`rust_motion_media_hwaccel::HardwarePixelBuffer`] directly
    /// to the video encoder without CPU pixel readback.
    #[cfg(all(target_os = "macos", feature = "metal"))]
    fn consume_hardware_pixel_buffer(
        &mut self,
        _frame: u32,
        _pb: rust_motion_media_hwaccel::HardwarePixelBuffer,
    ) -> Result<(), RasterError> {
        Err(RasterError::Init(
            "hardware pixel buffer submission is not supported by this sink".into(),
        ))
    }
}

impl<F> FrameSink for F
where
    F: for<'a> FnMut(u32, &'a [u8]) -> Result<(), RasterError>,
{
    fn consume(&mut self, frame: u32, rgba: &[u8]) -> Result<(), RasterError> {
        self(frame, rgba)
    }
}

/// Trait implemented by every rasterizer backend.
pub trait RasterizerBackend: Send + Sync {
    /// Render a single `Scene` into an `RgbaImage`.
    fn render_frame(&self, scene: &Scene, config: &FrameConfig) -> Result<RgbaImage, RasterError>;

    /// Describe the execution environment used by this backend.
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            native_scene: true,
            browser_runtime: false,
            gpu_accelerated: false,
            supports_streaming: self.supports_streaming(),
        }
    }

    /// Return cumulative backend counters for engine-level diagnostics.
    fn render_stats(&self) -> BackendRenderStats {
        BackendRenderStats::default()
    }

    /// Whether this backend provides an internal pipelined streaming implementation
    /// (e.g. GPU double-buffered ring buffers).
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Render a sequence of frames in streaming fashion with internal pipelining.
    #[allow(clippy::type_complexity)]
    fn render_stream(
        &self,
        _total: u32,
        _scene_fn: &(dyn Fn(u32) -> Result<Scene, RasterError> + Sync),
        _config_fn: &(dyn Fn(u32) -> FrameConfig + Sync),
        _sink: &mut dyn FrameSink,
    ) -> Result<(), RasterError> {
        Err(RasterError::Init(
            "Streaming is not supported on this backend".into(),
        ))
    }
}
