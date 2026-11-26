//! FFmpeg pipe render mode: spawning the encoder and feeding it ordered RGBA frames.

use super::*;

pub(super) struct FfmpegFrameSink<'a, W: Write> {
    pub(super) control: &'a RenderControl,
    pub(super) started: Instant,
    pub(super) input_width: u32,
    pub(super) input_height: u32,
    pub(super) output_width: u32,
    pub(super) output_height: u32,
    pub(super) total: u32,
    pub(super) start_frame: u32,
    pub(super) frame_step: u32,
    pub(super) stdin: &'a mut W,
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
