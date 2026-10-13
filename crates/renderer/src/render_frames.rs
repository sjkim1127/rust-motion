//! Frame rendering configuration and error types.
//!
//! Frame rasterization is handled natively by `rust-motion-rasterizer` via CPU (tiny-skia) or GPU (wgpu).

use thiserror::Error;

/// Error type for the renderer.
#[derive(Debug, Error)]
pub enum RenderError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Server error: {0}")]
    Server(#[from] crate::server::ServerError),
    #[error("Encode error: {0}")]
    Encode(String),
    #[error("Frame {0} failed: {1}")]
    FrameFailed(u32, String),
}

/// Configuration for a render job.
#[derive(Debug, Clone)]
pub struct RenderConfig {
    /// URL of the Dioxus app or server.
    pub url: String,
    /// Output directory for frame PNGs.
    pub output_dir: std::path::PathBuf,
    /// Width of the video.
    pub width: u32,
    /// Height of the video.
    pub height: u32,
    /// Frames per second.
    pub fps: f64,
    /// Total duration in frames.
    pub duration_in_frames: u32,
    /// Frame range to render (`None` = all frames).
    pub frame_range: Option<std::ops::RangeInclusive<u32>>,
    /// Concurrency — how many frames to render in parallel.
    pub concurrency: usize,
}

impl RenderConfig {
    /// Create a new render config with sensible defaults.
    pub fn new(
        url: String,
        output_dir: impl Into<std::path::PathBuf>,
        width: u32,
        height: u32,
        fps: f64,
        duration_in_frames: u32,
    ) -> Self {
        Self {
            url,
            output_dir: output_dir.into(),
            width,
            height,
            fps,
            duration_in_frames,
            frame_range: None,
            concurrency: num_cpus(),
        }
    }

    /// Returns the frame range, defaulting to all frames.
    pub fn effective_range(&self) -> std::ops::RangeInclusive<u32> {
        self.frame_range
            .clone()
            .unwrap_or(0..=self.duration_in_frames.saturating_sub(1))
    }
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}
