//! Render execution pipeline for native CPU, GPU, and browser-backed workflows.

use std::fs;

use crate::args::{
    resolve_browser_worker_path, resolve_hybrid_backend, RenderBackend, RenderCodec, RenderRequest,
    ValidationError,
};
use crate::audio::collect_scene_audio_tracks;
use crate::composition::{
    built_in_registry, Composition, CompositionRegistry, NativeCompositionContext,
    TimeEventSchedule,
};
use crate::composition_shim::{
    uses_project_timeline, BrowserComposition, ProjectTimelineComposition,
};
use crate::validate::{
    validate_composition_source, validate_render_options, validate_render_params_for_codec,
};
use rust_motion_project::Project;

#[cfg(feature = "rhai")]
use crate::rhai_runtime::RhaiComposition;

pub(crate) fn native_image_cache_bytes() -> Option<usize> {
    std::env::var("RUST_MOTION_IMAGE_CACHE_BYTES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
}

/// Execute a render using the compositions shipped with the standalone CLI.
pub async fn execute_render_command(request: &RenderRequest) -> anyhow::Result<()> {
    let registry = built_in_registry();
    execute_render_command_with_registry_and_control(
        request,
        &registry,
        default_render_control(request),
    )
    .await
}

/// Execute a render using an application-provided composition registry.
pub async fn execute_render_command_with_registry(
    request: &RenderRequest,
    registry: &CompositionRegistry,
) -> anyhow::Result<()> {
    execute_render_command_with_registry_and_control(
        request,
        registry,
        default_render_control(request),
    )
    .await
}

/// Render a project timeline through the Native or GPU scene pipeline while
/// preserving the shared render request contract. Browser timelines are
/// evaluated by the browser worker.
pub async fn execute_project_render_command_with_control(
    request: &RenderRequest,
    project: &Project,
    control: rust_motion_rasterizer::RenderControl,
) -> anyhow::Result<()> {
    let time_events = TimeEventSchedule::new(project.events.clone())
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    if !uses_project_timeline(request.backend, !project.tracks.is_empty()) {
        return execute_render_command_with_registry_and_time_events_and_control(
            request,
            &built_in_registry(),
            time_events,
            control,
        )
        .await;
    }
    let mut registry = CompositionRegistry::new();
    registry.register(ProjectTimelineComposition {
        id: project.composition.clone(),
        clips: project
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter().cloned())
            .collect(),
        registry: built_in_registry(),
    })?;
    execute_render_command_with_registry_and_time_events_and_control(
        request,
        &registry,
        time_events,
        control,
    )
    .await
}

/// Build the standard CLI progress and timeout controls for a render request.
pub fn default_render_control(request: &RenderRequest) -> rust_motion_rasterizer::RenderControl {
    // Progress is emitted by the shared render engine so every CLI render
    // path (native, browser, still, and video) has the same observable
    // behavior. Keep the CLI responsible only for request-specific timeout
    // configuration.
    let mut control = rust_motion_rasterizer::RenderControl::new()
        .with_stderr_progress()
        .with_stderr_render_stats();
    if let Some(seconds) = request.timeout_seconds {
        control = control.with_timeout(std::time::Duration::from_secs(seconds));
    }
    control
}

#[cfg(feature = "gpu")]
fn report_gpu_fallback_diagnostics(
    rasterizer: &rust_motion_rasterizer::WgpuBackend,
    control: &rust_motion_rasterizer::RenderControl,
) {
    let stats = rasterizer.render_stats();
    control.report_diagnostics(rust_motion_rasterizer::RenderDiagnostics {
        backend: "gpu",
        gpu_frames: stats.gpu_frames,
        cpu_fallback_frames: stats.cpu_fallback_frames,
        fallback_reason: rasterizer.last_cpu_fallback_reason(),
        elapsed_ms: 0.0,
        encoded_frames: 0,
        output_width: 0,
        output_height: 0,
        fps: 0.0,
        texture_cache_hits: stats.texture_cache_hits,
        texture_cache_misses: stats.texture_cache_misses,
        video_decode_ms: 0.0,
        texture_upload_ms: 0.0,
        gpu_submit_readback_ms: 0.0,
        browser_frame_ms: 0.0,
    });
    if stats.cpu_fallback_frames > 0 {
        tracing::warn!(
            gpu_frames = stats.gpu_frames,
            cpu_fallback_frames = stats.cpu_fallback_frames,
            reason = rasterizer
                .last_cpu_fallback_reason()
                .as_deref()
                .unwrap_or("unknown"),
            "GPU render used CPU fallback"
        );
    }
}

/// Execute a render with caller-owned progress, cancellation, and timeout controls.
pub async fn execute_render_command_with_control(
    request: &RenderRequest,
    control: rust_motion_rasterizer::RenderControl,
) -> anyhow::Result<()> {
    let registry = built_in_registry();
    execute_render_command_with_registry_and_control(request, &registry, control).await
}

/// Execute an application-provided composition registry with caller-owned controls.
pub async fn execute_render_command_with_registry_and_control(
    request: &RenderRequest,
    registry: &CompositionRegistry,
    control: rust_motion_rasterizer::RenderControl,
) -> anyhow::Result<()> {
    execute_render_command_with_registry_and_time_events_and_control(
        request,
        registry,
        TimeEventSchedule::default(),
        control,
    )
    .await
}

async fn execute_render_command_with_registry_and_time_events_and_control(
    request: &RenderRequest,
    registry: &CompositionRegistry,
    time_events: TimeEventSchedule,
    control: rust_motion_rasterizer::RenderControl,
) -> anyhow::Result<()> {
    let render_started = std::time::Instant::now();
    validate_composition_source(request.composition.as_deref(), request.script.as_ref())?;
    validate_render_params_for_codec(
        request.composition.as_deref().unwrap_or("RhaiScript"),
        request.props.as_ref(),
        request.width,
        request.height,
        request.fps,
        request.duration,
        request.codec,
    )?;
    let (frame_start, frame_end) = validate_render_options(request)?;
    let frame_count = frame_end - frame_start + 1;
    let output_frame_count = if request.codec.still_format().is_none() {
        frame_count.div_ceil(request.frame_step)
    } else {
        frame_count
    };
    for path in &request.audio {
        if !path.is_file() {
            return Err(ValidationError::AudioFileNotFound(path.clone()).into());
        }
    }

    let props = match &request.props {
        Some(path) => {
            let json = fs::read_to_string(path)?;
            serde_json::from_str(&json).map_err(|error| {
                anyhow::anyhow!("Invalid props JSON in {}: {error}", path.display())
            })?
        }
        None => serde_json::Value::Object(Default::default()),
    };
    let context = NativeCompositionContext {
        width: request.width,
        height: request.height,
        fps: request.fps,
        duration_in_frames: request.duration,
    };

    #[cfg(feature = "rhai")]
    let script_composition = request
        .script
        .as_deref()
        .map(RhaiComposition::from_file)
        .transpose()?;

    let effective_backend = resolve_hybrid_backend(
        request.backend,
        request.composition.as_deref(),
        request.script.as_deref(),
        registry,
    );

    let browser_fallback = BrowserComposition {
        id: request
            .composition
            .clone()
            .unwrap_or_else(|| "BrowserComposition".into()),
    };

    #[cfg(feature = "rhai")]
    let composition: &dyn Composition = match script_composition.as_ref() {
        Some(composition) => composition,
        None => registry
            .get(
                request
                    .composition
                    .as_deref()
                    .expect("validated native composition ID"),
            )
            .or_else(|error| {
                if effective_backend == RenderBackend::Browser {
                    Ok(&browser_fallback as &dyn Composition)
                } else {
                    Err(error)
                }
            })?,
    };

    #[cfg(not(feature = "rhai"))]
    let composition: &dyn Composition = {
        if request.script.is_some() {
            anyhow::bail!(
                "Rhai support is not compiled in. Rebuild with `--features rhai`:\n  \
                 cargo build -p rust-motion-cli --features rhai"
            );
        }
        registry
            .get(
                request
                    .composition
                    .as_deref()
                    .expect("validated native composition ID"),
            )
            .or_else(|error| {
                if effective_backend == RenderBackend::Browser {
                    Ok(&browser_fallback as &dyn Composition)
                } else {
                    Err(error)
                }
            })?
    };

    let prepared = composition.prepare_with_time_events(&props, context, time_events.clone())?;

    // Validate the first frame before starting FFmpeg. Dynamic compositions
    // therefore report syntax, type, and API errors without creating an output.
    let first_scene = prepared.render(0)?;
    let mut audio_tracks = match prepared.audio_tracks()? {
        Some(tracks) => tracks,
        None if effective_backend != RenderBackend::Browser
            && request.codec.still_format().is_none()
            && request.codec != RenderCodec::Gif =>
        {
            // FFmpeg's input layout is fixed before frame rendering starts, so
            // legacy scene-derived audio must be collected across the timeline.
            // Implementing audio_tracks() avoids this extra render pass.
            collect_scene_audio_tracks(
                prepared.as_ref(),
                &first_scene,
                context.duration_in_frames,
                context.fps,
            )?
        }
        None => first_scene.audio_tracks(),
    };
    audio_tracks.extend(
        request.audio.iter().map(|path| {
            rust_motion_rasterizer::AudioTrack::new(path.to_string_lossy().into_owned())
        }),
    );
    let first_scene_cache = std::sync::Arc::new(std::sync::Mutex::new(Some(first_scene)));

    tracing::info!(
        composition = composition.id(),
        backend = ?effective_backend,
        codec = ?request.codec,
        frame_start,
        frame_end,
        "Starting render"
    );

    let security_policy = request.effective_security_policy();
    #[cfg(feature = "gpu")]
    let mut gpu_profile_summary: Option<rust_motion_rasterizer::ProfilingSummary> = None;

    match effective_backend {
        RenderBackend::Native => {
            use rust_motion_rasterizer::{
                render_still_fallible_scaled, render_to_ffmpeg_pipe_fallible, PipeConfig,
                TinySkiaBackend,
            };

            let rasterizer = TinySkiaBackend::new()
                .with_image_cache_bytes(
                    native_image_cache_bytes()
                        .unwrap_or(rust_motion_rasterizer::DEFAULT_IMAGE_CACHE_BYTES),
                )
                .with_security_policy(security_policy.clone());
            if let Some(format) = request.codec.still_format() {
                let first_scene = std::sync::Arc::clone(&first_scene_cache);
                render_still_fallible_scaled(
                    &rasterizer,
                    request.width,
                    request.height,
                    request.fps,
                    frame_start,
                    &request.output,
                    format,
                    &control,
                    request.scale,
                    |frame| {
                        if frame == 0 {
                            let mut cached = first_scene
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            if let Some(scene) = cached.take() {
                                return Ok(scene);
                            }
                        }
                        prepared.render(frame)
                    },
                )?;
            } else {
                let first_scene = std::sync::Arc::clone(&first_scene_cache);
                let pipe_config = PipeConfig::new(
                    request.width,
                    request.height,
                    request.fps,
                    output_frame_count,
                    &request.output,
                )
                .with_scale(request.scale)
                .with_concurrency(
                    request.effective_concurrency(
                        std::thread::available_parallelism()
                            .map(|n| n.get())
                            .unwrap_or(4),
                    ),
                )
                .with_frame_start(frame_start)
                .with_frame_step(request.frame_step)
                .with_codec(request.codec.video_codec().expect("video codec validated"))
                .with_hw_accel(request.hw_accel)
                .with_quality(request.crf, &request.preset)
                .with_audio_tracks(audio_tracks.clone())
                .with_control(control.clone())
                .with_security_policy(security_policy.clone());
                render_to_ffmpeg_pipe_fallible(&rasterizer, &pipe_config, move |frame| {
                    if frame == 0 {
                        let mut cached = first_scene
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        if let Some(scene) = cached.take() {
                            return Ok(scene);
                        }
                    }
                    prepared.render(frame)
                })?;
            }
        }
        RenderBackend::Browser => {
            use rust_motion_rasterizer::{
                render_still_fallible_scaled, render_web_to_ffmpeg_pipe_fallible,
                BrowserFrameBackend, PipeConfig,
            };
            let worker = resolve_browser_worker_path()?;

            let url = std::env::var("RUST_MOTION_BROWSER_URL")
                .unwrap_or_else(|_| "http://localhost:1420".to_string());
            let browser_capacity = std::env::var("RUST_MOTION_BROWSER_CONCURRENCY")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or_else(|| {
                    std::thread::available_parallelism()
                        .map(|parallelism| parallelism.get().min(4))
                        .unwrap_or(1)
                });
            let concurrency = request.effective_concurrency(browser_capacity);
            let node =
                std::env::var_os("RUST_MOTION_BROWSER_NODE").unwrap_or_else(|| "node".into());
            let mut rasterizer =
                BrowserFrameBackend::with_concurrency(node, worker, url, concurrency)
                    .map_err(|error| anyhow::anyhow!("Browser backend init failed: {error}"))?
                    .with_frame_cache_bytes(
                        std::env::var("RUST_MOTION_FRAME_CACHE_BYTES")
                            .ok()
                            .and_then(|value| value.parse::<usize>().ok())
                            .filter(|value| *value > 0)
                            .unwrap_or(rust_motion_rasterizer::DEFAULT_MAX_CACHE_BYTES),
                    );
            if !std::env::var("RUST_MOTION_BROWSER_TRANSPORT")
                .ok()
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("base64"))
            {
                rasterizer = rasterizer.with_file_transport(true);
            }
            rasterizer.set_composition(
                request
                    .composition
                    .as_deref()
                    .unwrap_or("BrowserComposition"),
            )?;
            rasterizer.set_props(props.clone())?;
            rasterizer.set_time_events(
                time_events
                    .to_events()
                    .into_iter()
                    .map(|event| rust_motion_rasterizer::WebTimeEvent {
                        id: event.id,
                        frame: event.frame,
                    })
                    .collect(),
            )?;
            if let Some(format) = request.codec.still_format() {
                render_still_fallible_scaled(
                    &rasterizer,
                    request.width,
                    request.height,
                    request.fps,
                    frame_start,
                    &request.output,
                    format,
                    &control,
                    request.scale,
                    // BrowserFrameBackend owns the browser request; the Scene
                    // value is intentionally empty for this backend.
                    |_| Ok::<_, std::convert::Infallible>(rust_motion_rasterizer::Scene::new()),
                )?;
            } else {
                let pipe_config = PipeConfig::new(
                    request.width,
                    request.height,
                    request.fps,
                    output_frame_count,
                    &request.output,
                )
                .with_scale(request.scale)
                .with_concurrency(request.effective_concurrency(rasterizer.worker_count()))
                .with_frame_start(frame_start)
                .with_frame_step(request.frame_step)
                .with_codec(request.codec.video_codec().expect("video codec validated"))
                .with_hw_accel(request.hw_accel)
                .with_quality(request.crf, &request.preset)
                .with_audio_tracks(audio_tracks.clone())
                .with_control(control.clone())
                .with_security_policy(security_policy.clone());
                render_web_to_ffmpeg_pipe_fallible(&rasterizer, &pipe_config, props.clone())?;
            }
        }
        RenderBackend::Gpu => {
            #[cfg(not(feature = "gpu"))]
            anyhow::bail!(
                "GPU backend is not compiled in. Rebuild with `--features gpu`:\n  \
                 cargo build -p rust-motion-cli --features gpu"
            );

            #[cfg(feature = "gpu")]
            {
                use rust_motion_rasterizer::{
                    render_still_fallible_scaled, render_to_ffmpeg_pipe_fallible, PipeConfig,
                    WgpuBackend,
                };

                let profile_enabled = std::env::var("RUST_MOTION_PROFILE")
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);

                let rasterizer = WgpuBackend::new()
                    .map_err(|error| anyhow::anyhow!("GPU backend init failed: {error}"))?
                    .with_image_cache_bytes(
                        native_image_cache_bytes()
                            .unwrap_or(rust_motion_rasterizer::DEFAULT_IMAGE_CACHE_BYTES),
                    )
                    .with_profiling(profile_enabled);
                if let Some(format) = request.codec.still_format() {
                    let first_scene = std::sync::Arc::clone(&first_scene_cache);
                    render_still_fallible_scaled(
                        &rasterizer,
                        request.width,
                        request.height,
                        request.fps,
                        frame_start,
                        &request.output,
                        format,
                        &control,
                        request.scale,
                        |frame| {
                            if frame == 0 {
                                let mut cached = first_scene
                                    .lock()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                                if let Some(scene) = cached.take() {
                                    return Ok(scene);
                                }
                            }
                            prepared.render(frame)
                        },
                    )?;
                    report_gpu_fallback_diagnostics(&rasterizer, &control);
                } else {
                    let first_scene = std::sync::Arc::clone(&first_scene_cache);
                    let pipe_config = PipeConfig::new(
                        request.width,
                        request.height,
                        request.fps,
                        output_frame_count,
                        &request.output,
                    )
                    .with_scale(request.scale)
                    .with_concurrency(
                        request.effective_concurrency(
                            std::thread::available_parallelism()
                                .map(|n| n.get())
                                .unwrap_or(4),
                        ),
                    )
                    .with_frame_start(frame_start)
                    .with_frame_step(request.frame_step)
                    .with_codec(request.codec.video_codec().expect("video codec validated"))
                    .with_hw_accel(request.hw_accel)
                    .with_quality(request.crf, &request.preset)
                    .with_audio_tracks(audio_tracks.clone())
                    .with_control(control.clone())
                    .with_security_policy(security_policy.clone());
                    render_to_ffmpeg_pipe_fallible(&rasterizer, &pipe_config, move |frame| {
                        if frame == 0 {
                            let mut cached = first_scene
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            if let Some(scene) = cached.take() {
                                return Ok(scene);
                            }
                        }
                        prepared.render(frame)
                    })?;
                }
                if profile_enabled {
                    let summary = rasterizer.profile_summary();
                    eprintln!("\n{}", summary.render_table());
                    gpu_profile_summary = Some(summary);
                }
            }
        }
        RenderBackend::Auto => {
            unreachable!("effective_backend is always resolved before rendering")
        }
    }

    tracing::info!(output = %request.output.display(), "Render completed");
    if std::env::var_os("RUST_MOTION_JSON").is_some() {
        #[allow(unused_mut)]
        let mut json_obj = serde_json::json!({
            "ok": true,
            "output": request.output,
            "backend": format!("{:?}", request.backend).to_ascii_lowercase(),
            "codec": format!("{:?}", request.codec).to_ascii_lowercase(),
            "frame_start": frame_start,
            "frame_end": frame_end,
            "frames": output_frame_count,
            "elapsed_ms": render_started.elapsed().as_secs_f64() * 1000.0,
        });
        #[cfg(feature = "gpu")]
        if let Some(ref prof) = gpu_profile_summary {
            if let Some(map) = json_obj.as_object_mut() {
                map.insert(
                    "profile".to_string(),
                    serde_json::to_value(prof).unwrap_or_default(),
                );
            }
        }
        println!("{}", json_obj);
    }
    Ok(())
}
