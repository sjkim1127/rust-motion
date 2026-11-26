//! Render configuration, progress, diagnostics and cancellation/control types.

use super::*;

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
    pub(super) progress: Option<Arc<dyn Fn(RenderProgress) + Send + Sync>>,
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

    pub(super) fn check(&self, started: Instant) -> Result<(), RasterError> {
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

    pub(super) fn output_dimensions(&self) -> Result<(u32, u32), RasterError> {
        scaled_dimensions(self.width, self.height, self.scale)
    }
}
