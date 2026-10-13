//! Frame rendering coordinator — sequential, parallel, and streaming pipeline modes.
//!
//! # Render Modes
//!
//! | Mode                     | I/O               | Parallelism | Best for                         |
//! |--------------------------|-------------------|-------------|----------------------------------|
//! | [`render_all_frames`]    | PNG files on disk | Sequential  | Debugging, inspection            |
//! | [`render_parallel`]      | PNG files on disk | Rayon (N cores) | Large renders with disk I/O  |
//! | [`render_to_ffmpeg_pipe`]| FFmpeg stdin      | Rayon + pipe| **Fastest** — zero PNG overhead  |
//!
//! ## Pipeline comparison
//!
//! ```text
//! Sequential PNG:   [frame 0] → PNG → [frame 1] → PNG → … → FFmpeg
//! Parallel PNG:     [frame 0]
//!                   [frame 1]  (all at once, Rayon)
//!                   [frame 2] → disk → FFmpeg
//!
//! Pipe (fastest):   bounded render window → ordered RGBA frames → FFmpeg → MP4
//!                   Zero disk I/O, zero PNG compression overhead
//! ```

use crate::backend::{FrameConfig, FrameSink, RasterError, RasterizerBackend};
use crate::scene::{AudioTrack, Scene};
use crate::security::MediaSecurityPolicy;
use image::RgbaImage;
use rayon::prelude::*;
use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

struct FfmpegFrameSink<'a, W: Write> {
    control: &'a RenderControl,
    started: Instant,
    input_width: u32,
    input_height: u32,
    output_width: u32,
    output_height: u32,
    total: u32,
    start_frame: u32,
    frame_step: u32,
    stdin: &'a mut W,
}

impl<W: Write> FrameSink for FfmpegFrameSink<'_, W> {
    fn consume(&mut self, frame: u32, rgba: &[u8]) -> Result<(), RasterError> {
        self.control.check(self.started)?;
        let scaled = scale_rgba_frame(
            rgba,
            self.input_width,
            self.input_height,
            self.output_width,
            self.output_height,
        )?;
        self.stdin
            .write_all(&scaled)
            .map_err(|e| RasterError::ImageEncode(format!("FFmpeg pipe write error: {e}")))?;
        if let Some(callback) = &self.control.progress {
            callback(RenderProgress {
                completed_frames: frame + 1,
                total_frames: self.total,
                frame: self.start_frame + frame * self.frame_step,
            });
        }
        Ok(())
    }
}

// ── Types ────────────────────────────────────────────────────────────────────

/// Configuration for a native render job.
#[derive(Debug, Clone)]
pub struct NativeRenderConfig {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_in_frames: u32,
    pub output_dir: PathBuf,
    /// Number of Rayon threads to use. `None` = auto (# logical CPUs).
    pub concurrency: Option<usize>,
}

impl NativeRenderConfig {
    pub fn new(
        width: u32,
        height: u32,
        fps: f64,
        duration_in_frames: u32,
        output_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            width,
            height,
            fps,
            duration_in_frames,
            output_dir: output_dir.into(),
            concurrency: None,
        }
    }

    pub fn with_concurrency(mut self, n: usize) -> Self {
        self.concurrency = Some(n);
        self
    }
}

/// Video codec used by the FFmpeg pipe encoder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VideoCodec {
    #[default]
    H264,
    H265,
    Vp9,
    Av1,
    ProRes,
    Gif,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StillImageFormat {
    #[default]
    Png,
    Jpeg,
    WebP,
}

/// Hardware-accelerated video encoder selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HwAccel {
    /// Automatically choose the best hardware encoder for the platform (VideoToolbox on macOS, NVENC on Linux).
    #[default]
    Auto,
    /// Force software encoding (libx264, libx265).
    Disabled,
    /// Force Apple VideoToolbox hardware encoder (macOS).
    VideoToolbox,
    /// Force NVIDIA NVENC hardware encoder (Linux/Windows).
    Nvenc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderProgress {
    pub completed_frames: u32,
    pub total_frames: u32,
    pub frame: u32,
}

/// Progress reported after a rendered frame has been accepted by the encoder.
/// This is intentionally separate from [`RenderProgress`]: rendering can run
/// ahead of the ordered FFmpeg pipe when concurrency is greater than one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodingProgress {
    pub encoded_frames: u32,
    pub total_frames: u32,
}

/// Backend-level counters reported after a render path has finished.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderDiagnostics {
    pub backend: &'static str,
    pub gpu_frames: u64,
    pub cpu_fallback_frames: u64,
    pub fallback_reason: Option<String>,
    /// Wall-clock time for the complete render, including FFmpeg encoding.
    pub elapsed_ms: f64,
    pub encoded_frames: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub fps: f64,
    pub texture_cache_hits: u64,
    pub texture_cache_misses: u64,
    pub video_decode_ms: f64,
    pub texture_upload_ms: f64,
    pub gpu_submit_readback_ms: f64,
    pub browser_frame_ms: f64,
}

#[derive(Clone, Default)]
pub struct RenderCancellationToken(Arc<AtomicBool>);

/// Alias for [`RenderCancellationToken`] for Remotion `makeCancelSignal` API parity.
pub type CancelSignal = RenderCancellationToken;

/// Create a cancel signal for stopping renders asynchronously.
///
/// Remotion `@remotion/renderer` `makeCancelSignal()` parity.
pub fn make_cancel_signal() -> CancelSignal {
    RenderCancellationToken::default()
}

impl RenderCancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone, Default)]
pub struct RenderControl {
    cancellation: RenderCancellationToken,
    timeout: Option<Duration>,
    progress: Option<Arc<dyn Fn(RenderProgress) + Send + Sync>>,
    encoding_progress: Option<Arc<dyn Fn(EncodingProgress) + Send + Sync>>,
    diagnostics: Option<Arc<dyn Fn(RenderDiagnostics) + Send + Sync>>,
}

impl fmt::Debug for RenderControl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RenderControl")
            .field("cancelled", &self.cancellation.is_cancelled())
            .field("timeout", &self.timeout)
            .field("has_progress_callback", &self.progress.is_some())
            .field(
                "has_encoding_progress_callback",
                &self.encoding_progress.is_some(),
            )
            .field("has_diagnostics_callback", &self.diagnostics.is_some())
            .finish()
    }
}

impl RenderControl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancellation_token(&self) -> RenderCancellationToken {
        self.cancellation.clone()
    }

    pub fn with_cancellation(mut self, cancellation: RenderCancellationToken) -> Self {
        self.cancellation = cancellation;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn with_progress(
        mut self,
        callback: impl Fn(RenderProgress) + Send + Sync + 'static,
    ) -> Self {
        let callback = Arc::new(callback);
        let previous = self.progress.take();
        self.progress = Some(Arc::new(move |progress| {
            if let Some(previous) = &previous {
                previous(progress);
            }
            callback(progress);
        }));
        self
    }

    pub fn with_encoding_progress(
        mut self,
        callback: impl Fn(EncodingProgress) + Send + Sync + 'static,
    ) -> Self {
        let callback = Arc::new(callback);
        let previous = self.encoding_progress.take();
        self.encoding_progress = Some(Arc::new(move |progress| {
            if let Some(previous) = &previous {
                previous(progress);
            }
            callback(progress);
        }));
        self
    }

    pub fn report_encoding_progress(&self, progress: EncodingProgress) {
        if let Some(callback) = &self.encoding_progress {
            callback(progress);
        }
    }

    pub fn with_diagnostics(
        mut self,
        callback: impl Fn(RenderDiagnostics) + Send + Sync + 'static,
    ) -> Self {
        let callback = Arc::new(callback);
        let previous = self.diagnostics.take();
        self.diagnostics = Some(Arc::new(move |diagnostics| {
            if let Some(previous) = &previous {
                previous(diagnostics.clone());
            }
            callback(diagnostics);
        }));
        self
    }

    /// Print one machine-readable completion record from the render engine.
    ///
    /// The record covers the complete native pipeline, including frame
    /// scheduling, RGBA delivery to FFmpeg, and encoder completion. This is
    /// intentionally available through the engine control object so callers do
    /// not need a benchmark harness to observe production render performance.
    pub fn with_stderr_render_stats(self) -> Self {
        self.with_diagnostics(|diagnostics| {
            eprintln!(
                "{{\"rust_motion_render\":true,\"backend\":\"{}\",\"elapsed_ms\":{:.3},\"encoded_frames\":{},\"encoded_fps\":{:.3},\"resolution\":\"{}x{}\",\"gpu_frames\":{},\"cpu_fallback_frames\":{},\"texture_cache_hits\":{},\"texture_cache_misses\":{},\"video_decode_ms\":{:.3},\"texture_upload_ms\":{:.3},\"gpu_submit_readback_ms\":{:.3},\"browser_frame_ms\":{:.3},\"fallback_reason\":{}}}",
                diagnostics.backend,
                diagnostics.elapsed_ms,
                diagnostics.encoded_frames,
                if diagnostics.elapsed_ms > 0.0 {
                    diagnostics.encoded_frames as f64 * 1000.0 / diagnostics.elapsed_ms
                } else {
                    0.0
                },
                diagnostics.output_width,
                diagnostics.output_height,
                diagnostics.gpu_frames,
                diagnostics.cpu_fallback_frames,
                diagnostics.texture_cache_hits,
                diagnostics.texture_cache_misses,
                diagnostics.video_decode_ms,
                diagnostics.texture_upload_ms,
                diagnostics.gpu_submit_readback_ms,
                diagnostics.browser_frame_ms,
                serde_json::to_string(&diagnostics.fallback_reason).unwrap_or_else(|_| "null".into()),
            );
        })
    }

    pub fn report_diagnostics(&self, diagnostics: RenderDiagnostics) {
        if let Some(callback) = &self.diagnostics {
            callback(diagnostics);
        }
    }

    /// Report bounded render progress to stderr from the shared render engine.
    ///
    /// The callback is invoked for every frame internally, but output is
    /// throttled to every 30 frames and the final frame so CLI, Python, and
    /// Tauri callers get useful progress without flooding their terminals.
    pub fn with_stderr_progress(self) -> Self {
        let control = self.with_progress(|progress| {
            if progress.completed_frames == progress.total_frames
                || progress.completed_frames % 30 == 0
            {
                let percent = if progress.total_frames == 0 {
                    100.0
                } else {
                    progress.completed_frames as f64 * 100.0 / progress.total_frames as f64
                };
                eprintln!(
                    "[*] [Rust Motion Progress] {} / {} frames ({percent:.1}%)...",
                    progress.completed_frames, progress.total_frames,
                );
            }
        });
        control.with_encoding_progress(|progress| {
            if progress.encoded_frames == progress.total_frames || progress.encoded_frames % 30 == 0
            {
                let percent = if progress.total_frames == 0 {
                    100.0
                } else {
                    progress.encoded_frames as f64 * 100.0 / progress.total_frames as f64
                };
                eprintln!(
                    "[*] [Rust Motion Encoding] {} / {} frames ({percent:.1}%)...",
                    progress.encoded_frames, progress.total_frames,
                );
            }
        })
    }

    fn check(&self, started: Instant) -> Result<(), RasterError> {
        if self.cancellation.is_cancelled() {
            return Err(RasterError::Cancelled);
        }
        if self
            .timeout
            .is_some_and(|timeout| started.elapsed() >= timeout)
        {
            return Err(RasterError::Timeout);
        }
        Ok(())
    }
}

/// Piped encoding configuration (no intermediate PNG files).
#[derive(Debug, Clone)]
pub struct PipeConfig {
    pub width: u32,
    pub height: u32,
    /// Output scale applied after composition rendering, matching Remotion's
    /// `scale` option. The composition is evaluated at its logical size.
    pub scale: f64,
    pub fps: f64,
    pub duration_in_frames: u32,
    /// First composition frame included in the output.
    pub start_frame: u32,
    /// Source-frame stride for video output; still images always render one frame.
    pub frame_step: u32,
    /// Output media file path.
    pub output: PathBuf,
    /// Number of parallel render workers. `None` = auto.
    pub concurrency: Option<usize>,
    /// FFmpeg CRF quality (0–51, lower = better).
    pub crf: u32,
    /// FFmpeg preset: "ultrafast", "fast", "medium", etc.
    pub preset: String,
    pub codec: VideoCodec,
    /// Hardware acceleration mode for video encoding.
    pub hw_accel: HwAccel,
    /// Audio tracks mixed and trimmed to the rendered video duration.
    pub audio_tracks: Vec<AudioTrack>,
    pub control: RenderControl,
    /// Media security sandbox policy for audio and asset loading.
    pub security_policy: MediaSecurityPolicy,
}

impl PipeConfig {
    pub fn new(
        width: u32,
        height: u32,
        fps: f64,
        duration_in_frames: u32,
        output: impl Into<PathBuf>,
    ) -> Self {
        Self {
            width,
            height,
            scale: 1.0,
            fps,
            duration_in_frames,
            start_frame: 0,
            frame_step: 1,
            output: output.into(),
            concurrency: None,
            crf: 18,
            preset: "fast".to_string(),
            codec: VideoCodec::H264,
            hw_accel: HwAccel::default(),
            audio_tracks: Vec::new(),
            control: RenderControl::default(),
            security_policy: MediaSecurityPolicy::default(),
        }
    }

    pub fn with_security_policy(mut self, policy: MediaSecurityPolicy) -> Self {
        self.security_policy = policy;
        self
    }

    pub fn with_concurrency(mut self, n: usize) -> Self {
        self.concurrency = Some(n);
        self
    }

    pub fn with_hw_accel(mut self, hw_accel: HwAccel) -> Self {
        self.hw_accel = hw_accel;
        self
    }

    pub fn with_quality(mut self, crf: u32, preset: impl Into<String>) -> Self {
        self.crf = crf;
        self.preset = preset.into();
        self
    }

    pub fn with_audio_tracks(mut self, tracks: impl IntoIterator<Item = AudioTrack>) -> Self {
        self.audio_tracks = tracks.into_iter().collect();
        self
    }

    pub fn with_codec(mut self, codec: VideoCodec) -> Self {
        self.codec = codec;
        self
    }

    pub fn with_frame_start(mut self, start_frame: u32) -> Self {
        self.start_frame = start_frame;
        self
    }

    /// Render every `step`th source frame, lowering output FPS accordingly.
    pub fn with_frame_step(mut self, step: u32) -> Self {
        self.frame_step = step;
        self
    }

    pub fn with_control(mut self, control: RenderControl) -> Self {
        self.control = control;
        self
    }

    /// Scale the encoded output dimensions while preserving logical scene coordinates.
    pub fn with_scale(mut self, scale: f64) -> Self {
        self.scale = scale;
        self
    }

    fn output_dimensions(&self) -> Result<(u32, u32), RasterError> {
        scaled_dimensions(self.width, self.height, self.scale)
    }
}

fn scaled_dimensions(width: u32, height: u32, scale: f64) -> Result<(u32, u32), RasterError> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(RasterError::Init(
            "render scale must be finite and positive".into(),
        ));
    }
    let scaled_width = (width as f64 * scale).round();
    let scaled_height = (height as f64 * scale).round();
    if scaled_width < 1.0
        || scaled_height < 1.0
        || scaled_width > u32::MAX as f64
        || scaled_height > u32::MAX as f64
    {
        return Err(RasterError::Init(
            "render scale produces dimensions outside the supported range".into(),
        ));
    }
    Ok((scaled_width as u32, scaled_height as u32))
}

/// Render one composition frame directly to PNG, JPEG, or WebP.
#[allow(clippy::too_many_arguments)]
pub fn render_still_fallible<F, B, E>(
    backend: &B,
    width: u32,
    height: u32,
    fps: f64,
    frame: u32,
    output: &Path,
    format: StillImageFormat,
    control: &RenderControl,
    scene_fn: F,
) -> Result<(), RasterError>
where
    B: RasterizerBackend + Send + Sync,
    F: FnOnce(u32) -> Result<Scene, E>,
    E: std::fmt::Display,
{
    render_still_fallible_scaled(
        backend, width, height, fps, frame, output, format, control, 1.0, scene_fn,
    )
}

/// Render one composition frame with a post-rasterization output scale.
#[allow(clippy::too_many_arguments)]
pub fn render_still_fallible_scaled<F, B, E>(
    backend: &B,
    width: u32,
    height: u32,
    fps: f64,
    frame: u32,
    output: &Path,
    format: StillImageFormat,
    control: &RenderControl,
    scale: f64,
    scene_fn: F,
) -> Result<(), RasterError>
where
    B: RasterizerBackend + Send + Sync,
    F: FnOnce(u32) -> Result<Scene, E>,
    E: std::fmt::Display,
{
    let started = Instant::now();
    control.check(started)?;
    let scene = scene_fn(frame).map_err(|error| RasterError::Frame {
        frame,
        reason: error.to_string(),
    })?;
    let image = backend.render_frame(&scene, &FrameConfig::new(width, height, frame, fps))?;
    let (output_width, output_height) = scaled_dimensions(width, height, scale)?;
    let image = if (output_width, output_height) == (width, height) {
        image
    } else {
        image::imageops::resize(
            &image,
            output_width,
            output_height,
            image::imageops::FilterType::Lanczos3,
        )
    };
    control.check(started)?;
    match format {
        StillImageFormat::Png => image
            .save_with_format(output, image::ImageFormat::Png)
            .map_err(|error| RasterError::ImageEncode(error.to_string()))?,
        StillImageFormat::Jpeg => image::DynamicImage::ImageRgba8(image)
            .to_rgb8()
            .save_with_format(output, image::ImageFormat::Jpeg)
            .map_err(|error| RasterError::ImageEncode(error.to_string()))?,
        StillImageFormat::WebP => image
            .save_with_format(output, image::ImageFormat::WebP)
            .map_err(|error| RasterError::ImageEncode(error.to_string()))?,
    }
    if let Some(callback) = &control.progress {
        callback(RenderProgress {
            completed_frames: 1,
            total_frames: 1,
            frame,
        });
    }
    Ok(())
}

// ── Mode 1: Sequential PNG ───────────────────────────────────────────────────

/// Render all frames sequentially to PNG files.
///
/// Each frame is rendered in order and saved as `frame_000001.png`, etc.
/// Simple and debuggable, but slow for large frame counts.
pub fn render_all_frames<F>(
    backend: &dyn RasterizerBackend,
    config: &NativeRenderConfig,
    mut scene_fn: F,
) -> Result<Vec<PathBuf>, RasterError>
where
    F: FnMut(u32) -> Scene,
{
    std::fs::create_dir_all(&config.output_dir)?;

    let mut paths = Vec::with_capacity(config.duration_in_frames as usize);

    for frame in 0..config.duration_in_frames {
        let scene = scene_fn(frame);
        let frame_config = FrameConfig::new(config.width, config.height, frame, config.fps);
        let img = backend.render_frame(&scene, &frame_config)?;

        let path = config
            .output_dir
            .join(format!("frame_{:06}.png", frame + 1));
        img.save(&path)
            .map_err(|e| RasterError::ImageEncode(e.to_string()))?;
        paths.push(path);
    }

    Ok(paths)
}

// ── Mode 2: Parallel PNG ─────────────────────────────────────────────────────

/// Render all frames in parallel using Rayon, then write PNGs.
///
/// Frames are rendered concurrently across all available CPU cores.
/// Requires `backend` to implement `Send + Sync`.
///
/// # Performance
/// On a machine with N cores, this is roughly N× faster than sequential
/// for the rasterization step. Each worker writes its indexed PNG immediately,
/// so peak image memory is bounded by the worker count rather than frame count.
pub fn render_parallel<F, B>(
    backend: &B,
    config: &NativeRenderConfig,
    scene_fn: F,
) -> Result<Vec<PathBuf>, RasterError>
where
    B: RasterizerBackend + Send + Sync,
    F: Fn(u32) -> Scene + Send + Sync,
{
    std::fs::create_dir_all(&config.output_dir)?;

    let width = config.width;
    let height = config.height;
    let fps = config.fps;
    let total = config.duration_in_frames;
    let dir = &config.output_dir;

    // Configure Rayon thread pool if explicit concurrency was requested
    let pool = match config.concurrency {
        Some(n) => rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build()
            .map_err(|e| RasterError::Init(format!("Failed to build thread pool: {e}")))?,
        None => rayon::ThreadPoolBuilder::new()
            .build()
            .map_err(|e| RasterError::Init(format!("Failed to build thread pool: {e}")))?,
    };

    pool.install(|| {
        (0..total).into_par_iter().try_for_each(|frame| {
            let scene = scene_fn(frame);
            let frame_cfg = FrameConfig::new(width, height, frame, fps);
            let img = backend.render_frame(&scene, &frame_cfg)?;
            let path = dir.join(format!("frame_{:06}.png", frame + 1));
            img.save(&path)
                .map_err(|e| RasterError::ImageEncode(e.to_string()))
        })
    })?;

    let paths = (0..total)
        .map(|frame| dir.join(format!("frame_{:06}.png", frame + 1)))
        .collect();

    Ok(paths)
}

// ── Mode 3: FFmpeg stdin pipe (fastest) ──────────────────────────────────────

/// Render a bounded window of parallel frames while streaming ordered RGBA to FFmpeg.
///
/// **This is the fastest rendering mode.** It eliminates:
/// - PNG compression overhead
/// - Disk write latency for intermediate frames
/// - A second disk read by FFmpeg
///
/// # Pipeline
/// ```text
/// bounded render window → ordered RGBA frames → FFmpeg stdin → MP4
/// ```
///
/// # FFmpeg invocation
/// ```text
/// ffmpeg -f rawvideo -pix_fmt rgba -s WxH -r FPS -i pipe:0
///        [-i audio ... -filter_complex mix] -c:v libx264 -c:a aac
///        -pix_fmt yuv420p -crf N -preset P
///        -movflags +faststart output.mp4
/// ```
pub fn render_to_ffmpeg_pipe<F, B>(
    backend: &B,
    config: &PipeConfig,
    scene_fn: F,
) -> Result<(), RasterError>
where
    B: RasterizerBackend + Send + Sync,
    F: Fn(u32) -> Scene + Send + Sync,
{
    render_to_ffmpeg_pipe_fallible(backend, config, |frame| {
        Ok::<Scene, std::convert::Infallible>(scene_fn(frame))
    })
}

/// Fallible variant of [`render_to_ffmpeg_pipe`].
///
/// Scene generation errors are annotated with the frame number and propagated
/// before rasterization. This is intended for script-backed compositions and
/// other dynamic scene sources that can fail while evaluating a frame.
pub fn render_to_ffmpeg_pipe_fallible<F, B, E>(
    backend: &B,
    config: &PipeConfig,
    scene_fn: F,
) -> Result<(), RasterError>
where
    B: RasterizerBackend + Send + Sync,
    F: Fn(u32) -> Result<Scene, E> + Send + Sync,
    E: std::fmt::Display + Send,
{
    let parent = config
        .output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let suffix = config
        .output
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{extension}"))
        .unwrap_or_default();
    let temporary_output = tempfile::Builder::new()
        .prefix(".rust-motion-render-")
        .suffix(&suffix)
        .tempfile_in(parent)
        .map_err(|error| {
            RasterError::ImageEncode(format!(
                "Failed to create temporary output beside {}: {error}",
                config.output.display()
            ))
        })?;
    let mut temporary_config = config.clone();
    temporary_config.output = temporary_output.path().to_path_buf();

    render_pipe_to_path(backend, &temporary_config, scene_fn)?;

    if let Ok(metadata) = std::fs::metadata(&config.output) {
        temporary_output
            .as_file()
            .set_permissions(metadata.permissions())
            .map_err(|error| {
                RasterError::ImageEncode(format!(
                    "Failed to preserve output permissions for {}: {error}",
                    config.output.display()
                ))
            })?;
    }
    temporary_output.as_file().sync_all().map_err(|error| {
        RasterError::ImageEncode(format!("Failed to flush rendered output: {error}"))
    })?;
    temporary_output.persist(&config.output).map_err(|error| {
        RasterError::ImageEncode(format!(
            "Failed to atomically replace {}: {error}",
            config.output.display()
        ))
    })?;
    Ok(())
}

fn render_pipe_to_path<F, B, E>(
    backend: &B,
    config: &PipeConfig,
    scene_fn: F,
) -> Result<(), RasterError>
where
    B: RasterizerBackend + Send + Sync,
    F: Fn(u32) -> Result<Scene, E> + Send + Sync,
    E: std::fmt::Display + Send,
{
    let width = config.width;
    let height = config.height;
    let (output_width, output_height) = config.output_dimensions()?;
    let fps = config.fps;
    let total = config.duration_in_frames;
    let started = Instant::now();
    let backend_stats_before = backend.render_stats();

    validate_pipe_config(config)?;
    config.control.check(started)?;
    let output_preexisted = config.output.exists();

    // ── 1. Spawn FFmpeg ──────────────────────────────────────────────────────
    let ffmpeg_args = build_pipe_ffmpeg_args(config);
    let mut ffmpeg = Command::new("ffmpeg")
        .args(&ffmpeg_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            RasterError::Init(format!("Failed to spawn FFmpeg: {e}\nIs ffmpeg installed?"))
        })?;

    // ── 2. Render frames in parallel ─────────────────────────────────────────
    let concurrency = config.concurrency.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    });
    if concurrency == 0 {
        let _ = ffmpeg.kill();
        let _ = ffmpeg.wait();
        return Err(RasterError::Init(
            "Render concurrency must be greater than zero".into(),
        ));
    }

    // ── 3. Keep a bounded window of renders ahead of the pipe writer ─────────
    let mut stdin = ffmpeg
        .stdin
        .take()
        .ok_or_else(|| RasterError::Init("Failed to open FFmpeg stdin".into()))?;

    let render_result = if backend.supports_streaming() {
        let mut sink = FfmpegFrameSink {
            control: &config.control,
            started,
            input_width: width,
            input_height: height,
            output_width,
            output_height,
            total,
            start_frame: config.start_frame,
            frame_step: config.frame_step,
            stdin: &mut stdin,
        };
        backend.render_stream(
            total,
            &|frame| {
                config.control.check(started)?;
                let composition_frame = config.start_frame + frame * config.frame_step;
                scene_fn(composition_frame).map_err(|error| RasterError::Frame {
                    frame: composition_frame,
                    reason: error.to_string(),
                })
            },
            &|frame| {
                let composition_frame = config.start_frame + frame * config.frame_step;
                FrameConfig::new(width, height, composition_frame, fps)
            },
            &mut sink,
        )
    } else {
        if backend.capabilities().browser_runtime {
            for frame in 0..total {
                config.control.check(started)?;
                let composition_frame = config.start_frame + frame * config.frame_step;
                let scene = scene_fn(composition_frame).map_err(|error| RasterError::Frame {
                    frame: composition_frame,
                    reason: error.to_string(),
                })?;
                let frame_cfg = FrameConfig::new(width, height, composition_frame, fps);
                let image = backend.render_frame(&scene, &frame_cfg)?;
                config.control.check(started)?;
                let rgba = image.into_raw();
                let rgba = scale_rgba_frame(&rgba, width, height, output_width, output_height)?;
                stdin.write_all(&rgba).map_err(|error| {
                    RasterError::ImageEncode(format!("FFmpeg pipe write error: {error}"))
                })?;
                tracing::debug!(
                    frame = composition_frame,
                    bytes = rgba.len(),
                    "browser frame written to FFmpeg pipe"
                );
                if let Some(callback) = &config.control.progress {
                    callback(RenderProgress {
                        completed_frames: frame + 1,
                        total_frames: total,
                        frame: composition_frame,
                    });
                }
                config.control.report_encoding_progress(EncodingProgress {
                    encoded_frames: frame + 1,
                    total_frames: total,
                });
            }
            Ok(())
        } else {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(concurrency)
                .build()
                .map_err(|e| {
                    let _ = ffmpeg.kill();
                    let _ = ffmpeg.wait();
                    RasterError::Init(format!("Rayon pool error: {e}"))
                })?;

            let result = stream_ordered_frames(
                &pool,
                total,
                concurrency,
                |frame| {
                    config.control.check(started)?;
                    let composition_frame = config.start_frame + frame * config.frame_step; // validated above
                    let scene =
                        scene_fn(composition_frame).map_err(|error| RasterError::Frame {
                            frame: composition_frame,
                            reason: error.to_string(),
                        })?;
                    let frame_cfg = FrameConfig::new(width, height, composition_frame, fps);
                    let img = backend.render_frame(&scene, &frame_cfg)?;
                    config.control.check(started)?;
                    let rgba = img.into_raw();
                    scale_rgba_frame(&rgba, width, height, output_width, output_height)
                },
                |frame, rgba| {
                    config.control.check(started)?;
                    stdin.write_all(&rgba).map_err(|e| {
                        RasterError::ImageEncode(format!("FFmpeg pipe write error: {e}"))
                    })?;
                    if let Some(callback) = &config.control.progress {
                        callback(RenderProgress {
                            completed_frames: frame + 1,
                            total_frames: total,
                            frame: config.start_frame + frame * config.frame_step,
                        });
                    }
                    config.control.report_encoding_progress(EncodingProgress {
                        encoded_frames: frame + 1,
                        total_frames: total,
                    });
                    Ok(())
                },
            );
            result
        }
    };

    if render_result.is_ok() {
        tracing::debug!("flushing FFmpeg stdin after render");
        let _ = stdin.flush();
        tracing::debug!("FFmpeg stdin flush completed");
    }
    tracing::debug!("closing FFmpeg stdin to signal EOF");
    drop(stdin); // EOF for FFmpeg

    if let Err(error) = render_result {
        let _ = ffmpeg.kill();
        let _ = ffmpeg.wait();
        remove_failed_output(&config.output, output_preexisted);
        return Err(error);
    }

    // ── 4. Wait for FFmpeg to finish ─────────────────────────────────────────
    tracing::debug!("waiting for FFmpeg process to exit");
    let output = match wait_for_ffmpeg(ffmpeg, &config.control, started) {
        Ok(output) => output,
        Err(error) => {
            remove_failed_output(&config.output, output_preexisted);
            return Err(error);
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        remove_failed_output(&config.output, output_preexisted);
        return Err(RasterError::ImageEncode(format!(
            "FFmpeg exited with non-zero status {:?}: {}",
            output.status.code(),
            stderr.trim()
        )));
    }

    let capabilities = backend.capabilities();
    let backend_stats = backend.render_stats().delta_since(backend_stats_before);
    config.control.report_diagnostics(RenderDiagnostics {
        backend: if capabilities.browser_runtime {
            "browser"
        } else if capabilities.gpu_accelerated {
            "gpu"
        } else {
            "native"
        },
        gpu_frames: backend_stats.gpu_frames,
        cpu_fallback_frames: backend_stats.cpu_fallback_frames,
        fallback_reason: None,
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        encoded_frames: total,
        output_width,
        output_height,
        fps,
        texture_cache_hits: backend_stats.texture_cache_hits,
        texture_cache_misses: backend_stats.texture_cache_misses,
        video_decode_ms: backend_stats.video_decode_ns as f64 / 1_000_000.0,
        texture_upload_ms: backend_stats.texture_upload_ns as f64 / 1_000_000.0,
        gpu_submit_readback_ms: backend_stats.gpu_submit_readback_ns as f64 / 1_000_000.0,
        browser_frame_ms: backend_stats.browser_frame_ns as f64 / 1_000_000.0,
    });

    Ok(())
}

fn remove_failed_output(path: &Path, preexisted: bool) {
    if !preexisted && path.exists() {
        let _ = std::fs::remove_file(path);
    }
}

/// Render at most `window` frames ahead, consuming them in timeline order.
/// A new task is issued only after the consumer releases an earlier frame.
/// Thus rendering can overlap pipe writes without retaining extra frame batches.
fn stream_ordered_frames<F, C>(
    pool: &rayon::ThreadPool,
    total: u32,
    window: usize,
    render: F,
    mut consume: C,
) -> Result<(), RasterError>
where
    F: Fn(u32) -> Result<Vec<u8>, RasterError> + Sync,
    C: FnMut(u32, Vec<u8>) -> Result<(), RasterError>,
{
    if window == 0 {
        return Err(RasterError::Init(
            "Render concurrency must be greater than zero".into(),
        ));
    }
    let stopped = AtomicBool::new(false);
    // Signal queued tasks on errors and unwinding, before the scope joins them.
    struct StopOnDrop<'a>(&'a AtomicBool);
    impl Drop for StopOnDrop<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    pool.in_place_scope(|scope| {
        let _stop = StopOnDrop(&stopped);
        let (sender, receiver) = std::sync::mpsc::channel();
        let render = &render;
        let stopped = &stopped;
        let submit = |frame| {
            let sender = sender.clone();
            scope.spawn(move |_| {
                if stopped.load(Ordering::Relaxed) {
                    return;
                }
                // Forward panics too, so a missing frame cannot deadlock the
                // ordered consumer. The caller retains normal panic semantics.
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render(frame)));
                let _ = sender.send((frame, result));
            });
        };
        let mut submitted = total.min(u32::try_from(window).unwrap_or(u32::MAX));
        for frame in 0..submitted {
            submit(frame);
        }
        let mut next = 0;
        let mut pending = std::collections::BTreeMap::new();
        while next < total {
            let (frame, result) = receiver.recv().map_err(|_| {
                RasterError::Init(
                    "Render workers stopped before completing the frame window".into(),
                )
            })?;
            let rgba = match result {
                Ok(result) => result?,
                Err(panic) => std::panic::resume_unwind(panic),
            };
            pending.insert(frame, rgba);
            while let Some(rgba) = pending.remove(&next) {
                consume(next, rgba)?;
                next += 1;
                if submitted < total {
                    submit(submitted);
                    submitted += 1;
                }
            }
        }
        Ok(())
    })
}

fn validate_pipe_config(config: &PipeConfig) -> Result<(), RasterError> {
    let (output_width, output_height) = config.output_dimensions()?;
    if config.width == 0 || config.height == 0 {
        return Err(RasterError::Init(
            "render width and height must be positive".into(),
        ));
    }
    if !config.fps.is_finite() || config.fps <= 0.0 {
        return Err(RasterError::Init(
            "render FPS must be finite and positive".into(),
        ));
    }
    if config.duration_in_frames == 0 {
        return Err(RasterError::Init(
            "render duration must contain at least one frame".into(),
        ));
    }
    if config.frame_step == 0 {
        return Err(RasterError::Init(
            "render frame step must be greater than zero".into(),
        ));
    }
    config
        .start_frame
        .checked_add((config.duration_in_frames - 1).saturating_mul(config.frame_step))
        .ok_or_else(|| RasterError::Init("render frame range overflows u32".into()))?;
    if config.codec != VideoCodec::Gif
        && (!output_width.is_multiple_of(2) || !output_height.is_multiple_of(2))
    {
        return Err(RasterError::Init(
            "video render width and height must be even".into(),
        ));
    }
    let max_crf = match config.codec {
        VideoCodec::H264 | VideoCodec::H265 => Some(51),
        VideoCodec::Vp9 | VideoCodec::Av1 => Some(63),
        VideoCodec::ProRes | VideoCodec::Gif => None,
    };
    if max_crf.is_some_and(|maximum| config.crf > maximum) {
        return Err(RasterError::Init(format!(
            "CRF {} exceeds the {:?} maximum of {}",
            config.crf,
            config.codec,
            max_crf.expect("checked above")
        )));
    }
    if config.codec == VideoCodec::Gif && !config.audio_tracks.is_empty() {
        return Err(RasterError::Scene(
            "GIF output does not support audio tracks".into(),
        ));
    }
    validate_audio_tracks(&config.audio_tracks, &config.security_policy)
}

fn scale_rgba_frame(
    rgba: &[u8],
    width: u32,
    height: u32,
    output_width: u32,
    output_height: u32,
) -> Result<Vec<u8>, RasterError> {
    if width == output_width && height == output_height {
        return Ok(rgba.to_vec());
    }
    let image = image::RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or_else(|| {
        RasterError::ImageEncode("rendered RGBA buffer has an invalid length".into())
    })?;
    Ok(image::imageops::resize(
        &image,
        output_width,
        output_height,
        image::imageops::FilterType::Lanczos3,
    )
    .into_raw())
}

fn wait_for_ffmpeg(
    mut child: std::process::Child,
    control: &RenderControl,
    started: Instant,
) -> Result<std::process::Output, RasterError> {
    tracing::debug!("FFmpeg wait loop started");
    loop {
        if let Err(error) = control.check(started) {
            tracing::debug!(error = %error, "FFmpeg wait interrupted by render control");
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                tracing::debug!(?status, "FFmpeg process exited");
                let mut stderr = Vec::new();
                if let Some(mut pipe) = child.stderr.take() {
                    pipe.read_to_end(&mut stderr).map_err(|error| {
                        RasterError::ImageEncode(format!("FFmpeg stderr read error: {error}"))
                    })?;
                }
                return Ok(std::process::Output {
                    status,
                    stdout: Vec::new(),
                    stderr,
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RasterError::ImageEncode(format!(
                    "FFmpeg wait error: {error}"
                )));
            }
        }
    }
}

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

fn ffmpeg_has_encoder(name: &str) -> bool {
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

fn validate_audio_tracks(
    tracks: &[AudioTrack],
    policy: &MediaSecurityPolicy,
) -> Result<(), RasterError> {
    for track in tracks {
        policy.validate_path(&track.src)?;
        if !track.start_from.is_finite() || track.start_from < 0.0 {
            return Err(invalid_audio(
                track,
                "start_from must be finite and non-negative",
            ));
        }
        if !track.timeline_start.is_finite() || track.timeline_start < 0.0 {
            return Err(invalid_audio(
                track,
                "timeline_start must be finite and non-negative",
            ));
        }
        if track
            .duration
            .is_some_and(|duration| !duration.is_finite() || duration <= 0.0)
        {
            return Err(invalid_audio(track, "duration must be finite and positive"));
        }
        if !track.volume.is_finite() || !(0.0..=1.0).contains(&track.volume) {
            return Err(invalid_audio(track, "volume must be between 0.0 and 1.0"));
        }
        if !track.playback_rate.is_finite() || !(0.5..=2.0).contains(&track.playback_rate) {
            return Err(invalid_audio(
                track,
                "playback_rate must be between 0.5 and 2.0",
            ));
        }
        for (time, volume) in &track.volume_keyframes {
            if !time.is_finite() || *time < 0.0 {
                return Err(invalid_audio(
                    track,
                    "volume keyframe time must be finite and non-negative",
                ));
            }
            if !volume.is_finite() || !(0.0..=1.0).contains(volume) {
                return Err(invalid_audio(
                    track,
                    "volume keyframe gain must be between 0.0 and 1.0",
                ));
            }
        }
    }
    Ok(())
}

fn invalid_audio(track: &AudioTrack, reason: &str) -> RasterError {
    RasterError::MediaAsset {
        path: track.src.clone(),
        reason: reason.into(),
    }
}

fn active_audio_tracks(config: &PipeConfig) -> Vec<&AudioTrack> {
    let range_start = config.start_frame as f64 / config.fps;
    let output_fps = config.fps / config.frame_step as f64;
    let range_end = range_start + config.duration_in_frames as f64 / output_fps;
    config
        .audio_tracks
        .iter()
        .filter(|track| {
            let track_end = track
                .duration
                .map(|duration| track.timeline_start + duration)
                .unwrap_or(f64::INFINITY);
            track.timeline_start < range_end && track_end > range_start
        })
        .collect()
}

fn build_volume_expression(base_volume: f64, keyframes: &[(f64, f64)]) -> String {
    if keyframes.is_empty() {
        return format!("{:.6}", base_volume);
    }
    if keyframes.len() == 1 {
        return format!("{:.6}", keyframes[0].1 * base_volume);
    }

    let mut sorted = keyframes.to_vec();
    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let last_val = sorted.last().unwrap().1 * base_volume;
    let mut expr = format!("{:.6}", last_val);

    for i in (0..sorted.len() - 1).rev() {
        let (t0, v0_raw) = sorted[i];
        let (t1, v1_raw) = sorted[i + 1];
        let v0 = v0_raw * base_volume;
        let v1 = v1_raw * base_volume;
        let dt = (t1 - t0).max(0.0001);

        let interp = format!("({:.6}+({:.6})*(t-({:.6}))/{:.6})", v0, v1 - v0, t0, dt);
        if i == 0 {
            expr = format!(
                "if(lt(t,{:.6}),{:.6},if(lt(t,{:.6}),{},{}))",
                t0, v0, t1, interp, expr
            );
        } else {
            expr = format!("if(lt(t,{:.6}),{},{})", t1, interp, expr);
        }
    }

    expr
}

fn build_audio_filter(config: &PipeConfig, tracks: &[&AudioTrack]) -> String {
    let mut filters = Vec::with_capacity(tracks.len() + 1);
    let range_start = config.start_frame as f64 / config.fps;
    for (index, track) in tracks.iter().enumerate() {
        let skipped_timeline = (range_start - track.timeline_start).max(0.0);
        let source_start = track.start_from + skipped_timeline * track.playback_rate;
        let mut chain = format!(
            "[{}:a:0]atrim=start={:.9},asetpts=PTS-STARTPTS,atempo={:.9}",
            index + 1,
            source_start,
            track.playback_rate,
        );
        if track.volume_keyframes.is_empty() {
            chain.push_str(&format!(",volume={:.9}", track.volume));
        } else {
            // `asetpts=PTS-STARTPTS` resets the trimmed audio stream to t=0.
            // Keyframes use absolute composition seconds, so shift them to the
            // first composition time represented by this trimmed stream.
            let keyframe_origin = range_start.max(track.timeline_start);
            let local_keyframes = track
                .volume_keyframes
                .iter()
                .map(|(time, volume)| (time - keyframe_origin, *volume))
                .collect::<Vec<_>>();
            let expr = build_volume_expression(track.volume, &local_keyframes);
            chain.push_str(&format!(",volume='{}':eval=frame", expr));
        }
        if let Some(duration) = track.duration {
            let remaining = (duration - skipped_timeline).max(0.0);
            chain.push_str(&format!(",atrim=duration={remaining:.9}"));
        }
        let relative_start = (track.timeline_start - range_start).max(0.0);
        if relative_start > 0.0 {
            let delay_ms = (relative_start * 1000.0).round() as u64;
            chain.push_str(&format!(",adelay={delay_ms}:all=1"));
        }
        chain.push_str(&format!("[a{index}]"));
        filters.push(chain);
    }

    let labels = (0..tracks.len())
        .map(|index| format!("[a{index}]"))
        .collect::<String>();
    let output_fps = config.fps / config.frame_step as f64;
    let output_duration = config.duration_in_frames as f64 / output_fps;
    if tracks.len() == 1 {
        filters.push(format!(
            "{labels}apad,atrim=duration={output_duration:.9},asetpts=N/SR/TB[aout]"
        ));
    } else {
        filters.push(format!(
            "{labels}amix=inputs={}:normalize=0:duration=longest,apad,atrim=duration={output_duration:.9},asetpts=N/SR/TB[aout]",
            tracks.len()
        ));
    }
    filters.join(";")
}

/// Save a single `RgbaImage` frame to disk.
pub fn save_frame(img: &RgbaImage, path: &Path) -> Result<(), RasterError> {
    img.save(path)
        .map_err(|e| RasterError::ImageEncode(e.to_string()))
}

// ── Benchmark helper ─────────────────────────────────────────────────────────

/// Render a single frame and return elapsed time (for benchmarking).
pub fn render_frame_timed(
    backend: &dyn RasterizerBackend,
    scene: &Scene,
    config: &FrameConfig,
) -> Result<(RgbaImage, std::time::Duration), RasterError> {
    let start = std::time::Instant::now();
    let img = backend.render_frame(scene, config)?;
    Ok((img, start.elapsed()))
}

/// Export browser-rendered frames through the shared FFmpeg pipeline.
pub fn render_web_to_ffmpeg_pipe_fallible(
    backend: &crate::web_backend::BrowserFrameBackend,
    config: &PipeConfig,
    props: serde_json::Value,
) -> Result<(), RasterError> {
    backend.set_props(props)?;
    // Match the shared streaming window to the persistent browser worker pool.
    // Each worker serializes its own protocol requests, while distinct workers
    // can render different frames concurrently like Remotion's pages.
    let serial_config;
    let config = if config.concurrency.is_none() {
        serial_config = config.clone().with_concurrency(backend.worker_count());
        &serial_config
    } else {
        config
    };
    render_to_ffmpeg_pipe_fallible(backend, config, |_| {
        Ok::<Scene, std::convert::Infallible>(Scene::new())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_callback_survives_control_clone() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&received);
        let control = RenderControl::new().with_diagnostics(move |diagnostics| {
            captured.lock().unwrap().push(diagnostics);
        });
        let cloned = control.clone();
        cloned.report_diagnostics(RenderDiagnostics {
            backend: "gpu",
            gpu_frames: 9,
            cpu_fallback_frames: 1,
            fallback_reason: Some("unsupported node".into()),
            elapsed_ms: 12.5,
            encoded_frames: 10,
            output_width: 1280,
            output_height: 720,
            fps: 30.0,
            texture_cache_hits: 2,
            texture_cache_misses: 1,
            video_decode_ms: 0.5,
            texture_upload_ms: 0.25,
            gpu_submit_readback_ms: 1.0,
            browser_frame_ms: 0.0,
        });
        assert_eq!(
            received.lock().unwrap().as_slice(),
            &[RenderDiagnostics {
                backend: "gpu",
                gpu_frames: 9,
                cpu_fallback_frames: 1,
                fallback_reason: Some("unsupported node".into()),
                elapsed_ms: 12.5,
                encoded_frames: 10,
                output_width: 1280,
                output_height: 720,
                fps: 30.0,
                texture_cache_hits: 2,
                texture_cache_misses: 1,
                video_decode_ms: 0.5,
                texture_upload_ms: 0.25,
                gpu_submit_readback_ms: 1.0,
                browser_frame_ms: 0.0,
            }]
        );
    }
    use crate::scene::{Color, Scene, SceneNode};
    use crate::tiny_skia_backend::TinySkiaBackend;
    use crate::{BackendCapabilities, FrameConfig, RasterizerBackend};
    use image::RgbaImage;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(label: &str) -> PathBuf {
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

    fn ffmpeg_available() -> bool {
        Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn solid_scene(color: Color) -> impl Fn(u32) -> Scene + Send + Sync {
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

    struct BrowserLikeBackend;

    impl RasterizerBackend for BrowserLikeBackend {
        fn render_frame(
            &self,
            scene: &Scene,
            config: &FrameConfig,
        ) -> Result<RgbaImage, RasterError> {
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

    #[test]
    fn browser_runtime_pipe_uses_sequential_frames_and_reports_both_progresses() {
        if !ffmpeg_available() {
            return;
        }
        let temp = unique_temp_dir("browser_runtime_pipe");
        std::fs::create_dir_all(&temp).unwrap();
        let output = temp.join("browser.mp4");
        let render_progress = Arc::new(Mutex::new(Vec::new()));
        let encode_progress = Arc::new(Mutex::new(Vec::new()));
        let render_capture = Arc::clone(&render_progress);
        let encode_capture = Arc::clone(&encode_progress);
        let control = RenderControl::new()
            .with_progress(move |progress| {
                render_capture
                    .lock()
                    .unwrap()
                    .push(progress.completed_frames);
            })
            .with_encoding_progress(move |progress| {
                encode_capture.lock().unwrap().push(progress.encoded_frames);
            });
        let config = PipeConfig::new(16, 16, 30.0, 3, &output)
            .with_codec(VideoCodec::H264)
            .with_hw_accel(HwAccel::Disabled)
            .with_control(control);
        render_to_ffmpeg_pipe(&BrowserLikeBackend, &config, |frame| {
            solid_scene(Color::rgb((frame * 40) as u8, 20, 200))(frame)
        })
        .unwrap();
        assert_eq!(*render_progress.lock().unwrap(), vec![1, 2, 3]);
        assert_eq!(*encode_progress.lock().unwrap(), vec![1, 2, 3]);
        assert!(output.is_file());
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn streaming_overlaps_the_next_render_without_exceeding_the_window() {
        use std::sync::atomic::AtomicUsize;
        use std::sync::mpsc;
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(3)
            .build()
            .unwrap();
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let (first_ready, wait_first) = mpsc::channel();
        let wait_first = Mutex::new(wait_first);
        let (next_ready, wait_next) = mpsc::channel();
        let mut output = Vec::new();
        stream_ordered_frames(
            &pool,
            12,
            3,
            |frame| {
                let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(count, Ordering::SeqCst);
                if frame == 0 {
                    // Force out-of-order rendering; frame one must already be ready.
                    wait_first
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap();
                } else if frame == 1 {
                    first_ready.send(()).unwrap();
                } else if frame == 3 {
                    next_ready.send(()).unwrap();
                }
                Ok(vec![frame as u8; 4])
            },
            |frame, bytes| {
                if frame == 1 {
                    // A batch barrier would prevent frame 3 starting here.
                    wait_next.recv_timeout(Duration::from_secs(5)).unwrap();
                }
                output.extend_from_slice(&bytes);
                drop(bytes);
                active.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            output,
            (0..12u8).flat_map(|frame| [frame; 4]).collect::<Vec<_>>()
        );
        assert!(peak.load(Ordering::SeqCst) <= 3);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn streaming_stops_scheduling_after_render_or_writer_error() {
        use std::sync::atomic::AtomicUsize;
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        for fail_render in [false, true] {
            let rendered = AtomicUsize::new(0);
            let mut consumed = 0;
            let result = stream_ordered_frames(
                &pool,
                100,
                2,
                |frame| {
                    rendered.fetch_add(1, Ordering::SeqCst);
                    if fail_render && frame == 0 {
                        Err(RasterError::Scene("failed frame".into()))
                    } else {
                        Ok(vec![frame as u8])
                    }
                },
                |_, _| {
                    consumed += 1;
                    Err(RasterError::ImageEncode("failed pipe".into()))
                },
            );
            assert!(result.is_err());
            assert!(rendered.load(Ordering::SeqCst) <= 2);
            assert_eq!(consumed, if fail_render { 0 } else { 1 });
        }
    }

    #[test]
    fn streaming_honors_cancellation_between_ordered_writes() {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(3)
            .build()
            .unwrap();
        let control = RenderControl::new();
        let started = Instant::now();
        let mut consumed = Vec::new();
        let result = stream_ordered_frames(
            &pool,
            100,
            3,
            |frame| {
                control.check(started)?;
                Ok(vec![frame as u8])
            },
            |frame, _| {
                control.check(started)?;
                consumed.push(frame);
                if frame == 2 {
                    control.cancellation_token().cancel();
                }
                Ok(())
            },
        );
        assert!(matches!(result, Err(RasterError::Cancelled)));
        assert_eq!(consumed, vec![0, 1, 2]);
    }

    #[test]
    fn streaming_single_worker_and_short_final_window() {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap();
        for window in [1, 2, 8] {
            let mut frames = Vec::new();
            stream_ordered_frames(
                &pool,
                3,
                window,
                |frame| Ok(vec![frame as u8]),
                |_, bytes| {
                    frames.extend(bytes);
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(frames, vec![0, 1, 2]);
        }
    }

    #[test]
    fn streaming_worker_panic_reaches_caller_instead_of_waiting_forever() {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                stream_ordered_frames(&pool, 10, 2, |_| panic!("test render panic"), |_, _| Ok(()))
            }));
            sender.send(result.is_err()).unwrap();
        });
        assert!(receiver.recv_timeout(Duration::from_secs(5)).unwrap());
    }

    #[test]
    fn test_sequential_renders_correct_count() {
        let backend = TinySkiaBackend::headless();
        let tmp = unique_temp_dir("sequential");
        let _ = std::fs::remove_dir_all(&tmp);

        let config = NativeRenderConfig::new(64, 64, 30.0, 5, &tmp);
        let paths = render_all_frames(&backend, &config, |frame| {
            let mut s = Scene::new();
            s.push(SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 64.0,
                h: 64.0,
                fill: Color::rgb(frame as u8 * 40, 0, 0),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            });
            s
        })
        .expect("sequential render failed");

        assert_eq!(paths.len(), 5, "Should have 5 frame files");
        for p in &paths {
            assert!(p.exists(), "PNG file should exist: {p:?}");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_parallel_renders_same_count_as_sequential() {
        let backend = TinySkiaBackend::headless();
        let tmp = unique_temp_dir("parallel");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).expect("create test tmp dir");

        let config = NativeRenderConfig::new(64, 64, 30.0, 10, &tmp).with_concurrency(4);

        let paths = render_parallel(&backend, &config, solid_scene(Color::rgb(0, 0, 255)))
            .expect("parallel render failed");

        assert_eq!(paths.len(), 10, "Should have 10 frame files");
        // Verify they are in order
        for (i, p) in paths.iter().enumerate() {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            assert_eq!(name, format!("frame_{:06}.png", i + 1));
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_parallel_pixel_values_correct() {
        let backend = TinySkiaBackend::headless();
        let tmp = unique_temp_dir("parallel_px");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).expect("create test tmp dir");

        let config = NativeRenderConfig::new(64, 64, 30.0, 4, &tmp);
        let paths = render_parallel(&backend, &config, |frame| {
            let mut s = Scene::new();
            // Different colour per frame so we can verify correctness
            s.push(SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 64.0,
                h: 64.0,
                fill: Color::rgb(frame as u8 * 60, 0, 0),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            });
            s
        })
        .expect("parallel pixel render failed");

        for (i, path) in paths.iter().enumerate() {
            let img = image::open(path).expect("open frame").into_rgba8();
            let px = img.get_pixel(32, 32);
            let expected_r = (i as u8) * 60;
            assert_eq!(px[0], expected_r, "Frame {i}: red channel mismatch");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

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
    fn streaming_progress_reports_the_sampled_source_frame() {
        let frames = Arc::new(Mutex::new(Vec::new()));
        let captured_frames = Arc::clone(&frames);
        let control = RenderControl::new().with_progress(move |progress| {
            captured_frames.lock().unwrap().push(progress.frame);
        });
        let mut output = Vec::new();
        let mut sink = FfmpegFrameSink {
            control: &control,
            started: Instant::now(),
            input_width: 1,
            input_height: 1,
            output_width: 1,
            output_height: 1,
            total: 2,
            start_frame: 10,
            frame_step: 3,
            stdin: &mut output,
        };

        sink.consume(1, &[0, 0, 0, 255]).unwrap();

        assert_eq!(*frames.lock().unwrap(), vec![13]);
        assert_eq!(output, vec![0, 0, 0, 255]);
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

    #[test]
    fn test_still_formats_write_decodable_images_and_report_progress() {
        let backend = TinySkiaBackend::headless();
        let temp = unique_temp_dir("stills");
        std::fs::create_dir_all(&temp).unwrap();

        for (format, extension) in [
            (StillImageFormat::Png, "png"),
            (StillImageFormat::Jpeg, "jpg"),
            (StillImageFormat::WebP, "webp"),
        ] {
            let output = temp.join(format!("frame.{extension}"));
            let progress = Arc::new(Mutex::new(Vec::new()));
            let captured = Arc::clone(&progress);
            let control = RenderControl::new().with_progress(move |event| {
                captured.lock().unwrap().push(event);
            });
            render_still_fallible(
                &backend,
                32,
                24,
                30.0,
                17,
                &output,
                format,
                &control,
                |frame| {
                    assert_eq!(frame, 17);
                    Ok::<_, std::convert::Infallible>(solid_scene(Color::rgb(12, 34, 56))(frame))
                },
            )
            .unwrap();

            let decoded = image::open(&output).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (32, 24));
            assert_eq!(
                *progress.lock().unwrap(),
                vec![RenderProgress {
                    completed_frames: 1,
                    total_frames: 1,
                    frame: 17,
                }]
            );
        }
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn progress_callbacks_compose_instead_of_replacing_engine_output() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let first = Arc::clone(&calls);
        let second = Arc::clone(&calls);
        let control = RenderControl::new()
            .with_progress(move |_| first.lock().unwrap().push("first"))
            .with_progress(move |_| second.lock().unwrap().push("second"));

        if let Some(callback) = &control.progress {
            callback(RenderProgress {
                completed_frames: 1,
                total_frames: 1,
                frame: 0,
            });
        }

        assert_eq!(*calls.lock().unwrap(), vec!["first", "second"]);
    }

    #[test]
    fn encoding_progress_callbacks_compose() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let first = Arc::clone(&calls);
        let second = Arc::clone(&calls);
        let control = RenderControl::new()
            .with_encoding_progress(move |progress| {
                first
                    .lock()
                    .unwrap()
                    .push(("first", progress.encoded_frames));
            })
            .with_encoding_progress(move |progress| {
                second
                    .lock()
                    .unwrap()
                    .push(("second", progress.encoded_frames));
            });

        control.report_encoding_progress(EncodingProgress {
            encoded_frames: 2,
            total_frames: 3,
        });

        assert_eq!(*calls.lock().unwrap(), vec![("first", 2), ("second", 2)]);
    }

    #[test]
    fn diagnostics_callbacks_compose() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let first = Arc::clone(&calls);
        let second = Arc::clone(&calls);
        let control = RenderControl::new()
            .with_diagnostics(move |diagnostics| {
                first
                    .lock()
                    .unwrap()
                    .push(("first", diagnostics.encoded_frames));
            })
            .with_diagnostics(move |diagnostics| {
                second
                    .lock()
                    .unwrap()
                    .push(("second", diagnostics.encoded_frames));
            });

        control.report_diagnostics(RenderDiagnostics {
            backend: "test",
            gpu_frames: 1,
            cpu_fallback_frames: 0,
            fallback_reason: None,
            elapsed_ms: 2.0,
            encoded_frames: 2,
            output_width: 16,
            output_height: 16,
            fps: 30.0,
            texture_cache_hits: 0,
            texture_cache_misses: 0,
            video_decode_ms: 0.0,
            texture_upload_ms: 0.0,
            gpu_submit_readback_ms: 0.0,
            browser_frame_ms: 0.0,
        });

        assert_eq!(*calls.lock().unwrap(), vec![("first", 2), ("second", 2)]);
    }

    #[test]
    fn scaled_still_writes_scaled_dimensions() {
        let backend = TinySkiaBackend::headless();
        let temp = unique_temp_dir("scaled_still");
        std::fs::create_dir_all(&temp).unwrap();
        let output = temp.join("frame.png");
        render_still_fallible_scaled(
            &backend,
            32,
            24,
            30.0,
            0,
            &output,
            StillImageFormat::Png,
            &RenderControl::new(),
            1.5,
            |frame| Ok::<_, std::convert::Infallible>(solid_scene(Color::rgb(255, 0, 0))(frame)),
        )
        .unwrap();
        let decoded = image::open(&output).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (48, 36));
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn test_frame_range_streams_absolute_frames_in_order() {
        if !ffmpeg_available() {
            eprintln!("Skipping frame-range pipe test: FFmpeg is unavailable");
            return;
        }
        let temp = unique_temp_dir("frame_range");
        std::fs::create_dir_all(&temp).unwrap();
        let output = temp.join("range.mp4");
        let rendered_frames = Arc::new(Mutex::new(Vec::new()));
        let rendered_capture = Arc::clone(&rendered_frames);
        let progress = Arc::new(Mutex::new(Vec::new()));
        let progress_capture = Arc::clone(&progress);
        let control = RenderControl::new().with_progress(move |event| {
            progress_capture.lock().unwrap().push(event);
        });
        let config = PipeConfig::new(32, 24, 30.0, 3, &output)
            .with_frame_start(10)
            .with_frame_step(2)
            .with_concurrency(2)
            .with_control(control);

        render_to_ffmpeg_pipe(&TinySkiaBackend::headless(), &config, move |frame| {
            rendered_capture.lock().unwrap().push(frame);
            solid_scene(Color::rgb(frame as u8, 20, 30))(frame)
        })
        .unwrap();

        let mut actual_frames = rendered_frames.lock().unwrap().clone();
        actual_frames.sort_unstable();
        assert_eq!(actual_frames, vec![10, 12, 14]);
        assert_eq!(
            progress.lock().unwrap().as_slice(),
            &[
                RenderProgress {
                    completed_frames: 1,
                    total_frames: 3,
                    frame: 10,
                },
                RenderProgress {
                    completed_frames: 2,
                    total_frames: 3,
                    frame: 12,
                },
                RenderProgress {
                    completed_frames: 3,
                    total_frames: 3,
                    frame: 14,
                },
            ]
        );
        assert!(std::fs::metadata(&output).unwrap().len() > 0);
        std::fs::remove_dir_all(temp).unwrap();
    }

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

        let error =
            render_to_ffmpeg_pipe_fallible(&TinySkiaBackend::headless(), &config, |frame| {
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
    fn test_audio_filter_args_mix_tracks() {
        let mut first = AudioTrack::new("first.wav");
        first.start_from = 0.25;
        first.timeline_start = 0.5;
        first.volume = 0.75;
        let mut second = AudioTrack::new("second.wav");
        second.looped = true;
        second.playback_rate = 1.25;
        let config = PipeConfig::new(1920, 1080, 30.0, 90, "/tmp/out.mp4")
            .with_audio_tracks([first, second]);

        let args = build_pipe_ffmpeg_args(&config);
        let filter_index = args
            .iter()
            .position(|arg| arg == "-filter_complex")
            .unwrap();
        let filter = &args[filter_index + 1];
        assert!(filter.contains("adelay=500:all=1"));
        assert!(filter.contains("atempo=1.250000000"));
        assert!(filter.contains("amix=inputs=2"));
        assert!(args.contains(&"-stream_loop".to_string()));
        assert!(args.contains(&"[aout]".to_string()));
    }

    #[test]
    fn test_audio_filter_trims_against_selected_frame_range() {
        let mut track = AudioTrack::new("track.wav");
        track.start_from = 0.25;
        track.duration = Some(5.0);
        let config = PipeConfig::new(64, 64, 30.0, 30, "out.mp4")
            .with_frame_start(60)
            .with_audio_tracks([track]);
        let args = build_pipe_ffmpeg_args(&config);
        let filter_index = args
            .iter()
            .position(|arg| arg == "-filter_complex")
            .unwrap();
        let filter = &args[filter_index + 1];
        assert!(filter.contains("atrim=start=2.250000000"));
        assert!(filter.contains("atrim=duration=3.000000000"));
        assert!(filter.contains("atrim=duration=1.000000000"));
        assert!(!filter.contains("adelay="));
    }

    #[test]
    fn test_audio_volume_keyframes_use_trimmed_composition_timeline() {
        let mut track = AudioTrack::new("track.wav");
        track.timeline_start = 5.0;
        track.volume = 0.5;
        track.volume_keyframes = vec![(5.0, 0.0), (7.0, 1.0), (9.0, 0.4)];
        let config = PipeConfig::new(64, 64, 30.0, 300, "out.mp4").with_audio_tracks([track]);
        let args = build_pipe_ffmpeg_args(&config);
        let filter_index = args
            .iter()
            .position(|arg| arg == "-filter_complex")
            .unwrap();
        let filter = &args[filter_index + 1];

        assert!(filter.contains("volume='if(lt(t,0.000000),0.000000,if(lt(t,2.000000)"));
        assert!(filter.contains("(t-(0.000000))/2.000000"));
        assert!(filter.contains("lt(t,4.000000)"));
        assert!(filter.contains("adelay=5000:all=1"));
    }

    #[test]
    fn test_partial_audio_range_rebases_keyframes_and_keeps_prior_envelope() {
        let mut track = AudioTrack::new("track.wav");
        track.timeline_start = 5.0;
        track.volume_keyframes = vec![(5.0, 0.0), (7.0, 1.0), (9.0, 0.4)];
        let config = PipeConfig::new(64, 64, 30.0, 90, "out.mp4")
            .with_frame_start(180)
            .with_audio_tracks([track]);
        let args = build_pipe_ffmpeg_args(&config);
        let filter_index = args
            .iter()
            .position(|arg| arg == "-filter_complex")
            .unwrap();
        let filter = &args[filter_index + 1];

        assert!(filter.contains("atrim=start=1.000000000"));
        assert!(filter.contains("volume='if(lt(t,-1.000000),0.000000,if(lt(t,1.000000)"));
        assert!(filter.contains("(t-(-1.000000))/2.000000"));
        assert!(filter.contains("lt(t,3.000000)"));
        assert!(!filter.contains("adelay="));
    }

    #[test]
    fn test_audio_track_validation_rejects_invalid_volume() {
        let mut track = AudioTrack::new(std::env::current_exe().unwrap().display().to_string());
        track.volume = 1.5;
        let error = validate_audio_tracks(&[track], &MediaSecurityPolicy::default()).unwrap_err();
        assert!(error.to_string().contains("volume must be between"));
    }

    #[test]
    fn test_audio_track_validation_rejects_invalid_volume_keyframes() {
        let src = std::env::current_exe().unwrap().display().to_string();
        let mut negative_time = AudioTrack::new(&src);
        negative_time.volume_keyframes = vec![(-0.1, 0.5)];
        let error =
            validate_audio_tracks(&[negative_time], &MediaSecurityPolicy::default()).unwrap_err();
        assert!(error.to_string().contains("keyframe time"));

        let mut invalid_gain = AudioTrack::new(src);
        invalid_gain.volume_keyframes = vec![(0.0, 1.1)];
        let error =
            validate_audio_tracks(&[invalid_gain], &MediaSecurityPolicy::default()).unwrap_err();
        assert!(error.to_string().contains("keyframe gain"));
    }

    #[test]
    fn test_frame_timed() {
        let backend = TinySkiaBackend::headless();
        let mut scene = Scene::new();
        scene.push(SceneNode::Circle {
            cx: 32.0,
            cy: 32.0,
            r: 20.0,
            fill: Color::rgb(255, 128, 0),
            stroke: None,
            stroke_width: 0.0,
        });
        let config = FrameConfig::new(64, 64, 0, 30.0);

        let (img, elapsed) =
            render_frame_timed(&backend, &scene, &config).expect("timed render failed");
        assert_eq!(img.width(), 64);
        println!("Single 64x64 frame: {:?}", elapsed);
    }
}
