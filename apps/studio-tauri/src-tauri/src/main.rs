#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use dioxuscut_cli::{
    execute_project_render_command_with_control, RenderBackend, RenderCodec, RenderRequest,
};
use dioxuscut_project::{JobStatus, JobStore, Project, RenderJob};
use dioxuscut_rasterizer::{
    make_cancel_signal, render_still_fallible_scaled, render_web_to_ffmpeg_pipe_fallible,
    BackendCapabilities, BrowserFrameBackend, EncodingProgress, PipeConfig,
    RenderCancellationToken, RenderControl, RenderDiagnostics, StillImageFormat, VideoCodec,
    WebFrameRequest, WebTimelineClip, WebWorkerMessage, WEB_WORKER_PROTOCOL_VERSION,
};
use dioxuscut_renderer::spawn_server;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

mod render_job_controller;
use render_job_controller::RenderJobController;

fn project_render_codec(path: &std::path::Path) -> RenderCodec {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "webm" => RenderCodec::Vp9,
        "mov" => RenderCodec::ProRes,
        "gif" => RenderCodec::Gif,
        _ => RenderCodec::H264,
    }
}

fn project_video_codec(path: &std::path::Path) -> VideoCodec {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "webm" => VideoCodec::Vp9,
        "mov" => VideoCodec::ProRes,
        "gif" => VideoCodec::Gif,
        _ => VideoCodec::H264,
    }
}

fn browser_frame_cache_bytes() -> usize {
    std::env::var("DIOXUSCUT_FRAME_CACHE_BYTES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(dioxuscut_rasterizer::DEFAULT_MAX_CACHE_BYTES)
}

fn default_browser_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|parallelism| parallelism.get().min(4))
        .unwrap_or(1)
}

fn use_file_transport(transport: Option<&str>) -> bool {
    transport
        .map(|value| value.trim().eq_ignore_ascii_case("file"))
        .unwrap_or(true)
}

fn project_still_format(path: &std::path::Path) -> Option<StillImageFormat> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some(StillImageFormat::Png),
        "jpg" | "jpeg" => Some(StillImageFormat::Jpeg),
        "webp" => Some(StillImageFormat::WebP),
        _ => None,
    }
}

fn selected_output_frame_count(
    frame_start: u32,
    frame_end: u32,
    frame_step: u32,
    still_image: bool,
) -> u32 {
    if still_image {
        1
    } else {
        (frame_end - frame_start + 1).div_ceil(frame_step)
    }
}

fn create_render_job_temp_dir() -> Result<tempfile::TempDir, String> {
    tempfile::Builder::new()
        .prefix("dioxuscut-render-job-")
        .tempdir()
        .map_err(|error| format!("failed to create render job temp directory: {error}"))
}

fn browser_worker_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("DIOXUSCUT_BROWSER_WORKER") {
        return Ok(PathBuf::from(path));
    }

    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            candidates.push(parent.join("resources/_up_/scripts/three-render-worker.mjs"));
            candidates.push(parent.join("resources/scripts/three-render-worker.mjs"));
            candidates.push(parent.join("../Resources/scripts/three-render-worker.mjs"));
            candidates.push(parent.join("../Resources/_up_/scripts/three-render-worker.mjs"));
            candidates.push(parent.join("scripts/three-render-worker.mjs"));
        }
    }
    candidates
        .push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts/three-render-worker.mjs"));

    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            "Browser backend requires DIOXUSCUT_BROWSER_WORKER or a bundled three-render-worker.mjs"
                .to_string()
        })
}

fn browser_frontend_path() -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            candidates.push(parent.join("resources/_up_/dist"));
            candidates.push(parent.join("resources/dist"));
            candidates.push(parent.join("../Resources/_up_/dist"));
        }
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist"));
    candidates
        .into_iter()
        .find(|path| path.join("index.html").is_file())
        .ok_or_else(|| "bundled browser frontend dist/index.html was not found".to_string())
}

struct AppState {
    jobs: Arc<Mutex<JobStore>>,
    cancellations: Arc<Mutex<HashMap<String, RenderCancellationToken>>>,
}

const DEFAULT_MAX_TOTAL_REMOTE_ASSET_BYTES: usize = 1024 * 1024 * 1024;

fn max_total_remote_asset_bytes() -> Result<usize, String> {
    let Some(value) = std::env::var_os("DIOXUSCUT_MAX_TOTAL_REMOTE_ASSET_BYTES") else {
        return Ok(DEFAULT_MAX_TOTAL_REMOTE_ASSET_BYTES);
    };
    let value = value
        .into_string()
        .map_err(|_| "DIOXUSCUT_MAX_TOTAL_REMOTE_ASSET_BYTES must be valid UTF-8".to_string())?;
    value
        .parse::<usize>()
        .map_err(|error| format!("invalid DIOXUSCUT_MAX_TOTAL_REMOTE_ASSET_BYTES value: {error}"))
}

#[tauri::command]
fn submit_project(state: tauri::State<'_, AppState>, project: Project) -> Result<String, String> {
    RenderJobController::new(Arc::clone(&state.jobs)).submit(project)
}

/// Load and submit a project using its file directory as the asset base.
/// This is the AI-friendly counterpart to `submit_project(Project)`, which is
/// intentionally path-independent for callers that already resolved assets.
#[tauri::command]
fn submit_project_from_path(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<String, String> {
    let project = load_project_file(std::path::Path::new(&path))?;
    submit_project(state, project)
}

#[tauri::command]
fn start_render_job(
    state: tauri::State<'_, AppState>,
    id: String,
    output: String,
) -> Result<(), String> {
    let mut project = {
        let store = state
            .jobs
            .lock()
            .map_err(|_| "job store lock poisoned".to_string())?;
        store
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("render job '{id}' was not found"))?
            .project
    };
    let backend_kind = project.settings.backend;
    let max_total_asset_bytes = (backend_kind != dioxuscut_project::BackendKind::Browser)
        .then(max_total_remote_asset_bytes)
        .transpose()?;
    let render_frame_start = project.settings.frame_start.unwrap_or(0);
    let render_frame_end = project
        .settings
        .frame_end
        .unwrap_or(project.settings.duration.saturating_sub(1));
    let browser_preflight = if backend_kind == dioxuscut_project::BackendKind::Browser {
        let worker = browser_worker_path()?;
        let configured_url = std::env::var("DIOXUSCUT_BROWSER_URL").ok();
        let frontend_path = if configured_url.is_none() {
            Some(browser_frontend_path()?)
        } else {
            None
        };
        Some((worker, configured_url, frontend_path))
    } else {
        None
    };
    let output_frame_count = selected_output_frame_count(
        render_frame_start,
        render_frame_end,
        project.settings.frame_step,
        project_still_format(std::path::Path::new(&output)).is_some(),
    );
    {
        let mut store = state
            .jobs
            .lock()
            .map_err(|_| "job store lock poisoned".to_string())?;
        store
            .set_output(&id, &output)
            .map_err(|error| error.to_string())?;
        store
            .try_update(&id, JobStatus::Preparing, 0)
            .map_err(|error| error.to_string())?;
    }
    if backend_kind != dioxuscut_project::BackendKind::Browser {
        let max_total_asset_bytes = max_total_asset_bytes.expect("native asset limit was read");
        let state_jobs = Arc::clone(&state.jobs);
        let state_cancellations = Arc::clone(&state.cancellations);
        let cancellation = make_cancel_signal();
        state_cancellations
            .lock()
            .map_err(|_| "cancellation store lock poisoned".to_string())?
            .insert(id.clone(), cancellation.clone());
        thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                let job_temp_dir = create_render_job_temp_dir()?;
                let props_path = job_temp_dir.path().join("props.json");
                let asset_cache_dir = job_temp_dir.path().join("assets");
                project
                    .materialize_remote_assets(
                        &asset_cache_dir,
                        256 * 1024 * 1024,
                        max_total_asset_bytes,
                    )
                    .map_err(|error| error.to_string())?;
                std::fs::write(
                    &props_path,
                    serde_json::to_vec(&project.props).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                let output_path = PathBuf::from(&output);
                let request = RenderRequest {
                    composition: Some(project.composition.clone()),
                    script: None,
                    props: Some(props_path.clone()),
                    output: output_path.clone(),
                    audio: dioxuscut_cli::project_audio_assets(&project),
                    width: project.settings.width,
                    height: project.settings.height,
                    scale: project.settings.scale,
                    fps: project.settings.fps,
                    duration: project.settings.duration,
                    backend: match project.settings.backend {
                        dioxuscut_project::BackendKind::Native => RenderBackend::Native,
                        dioxuscut_project::BackendKind::Gpu => RenderBackend::Gpu,
                        dioxuscut_project::BackendKind::Browser => unreachable!(),
                    },
                    codec: project_render_codec(&output_path),
                    frame_start: project.settings.frame_start.unwrap_or(0),
                    frame_end: project.settings.frame_end,
                    frame_step: project.settings.frame_step,
                    concurrency: project.settings.concurrency.map(|value| value as usize),
                    timeout_seconds: None,
                    crf: project.settings.crf.unwrap_or(18),
                    preset: project
                        .settings
                        .preset
                        .clone()
                        .unwrap_or_else(|| "fast".into()),
                    hw_accel: dioxuscut_rasterizer::HwAccel::Auto,
                    sandbox_roots: vec![],
                    permissive: true,
                };
                let progress_state = Arc::clone(&state_jobs);
                let progress_id = id.clone();
                let diagnostics_state = Arc::clone(&state_jobs);
                let diagnostics_id = id.clone();
                let encoding_state = Arc::clone(&state_jobs);
                let encoding_id = id.clone();
                let control = RenderControl::new()
                    .with_cancellation(cancellation)
                    .with_progress(move |progress| {
                        if let Ok(mut store) = progress_state.lock() {
                            let _ = store.try_update(
                                &progress_id,
                                JobStatus::Rendering,
                                progress.completed_frames,
                            );
                        }
                    })
                    .with_diagnostics(move |diagnostics: RenderDiagnostics| {
                        if let Ok(mut store) = diagnostics_state.lock() {
                            let _ = store.set_render_diagnostics(
                                &diagnostics_id,
                                diagnostics.gpu_frames,
                                diagnostics.cpu_fallback_frames,
                                diagnostics.fallback_reason,
                            );
                        }
                    })
                    .with_encoding_progress(move |progress: EncodingProgress| {
                        let controller = RenderJobController::new(Arc::clone(&encoding_state));
                        let _ =
                            controller.set_encoding_progress(&encoding_id, progress.encoded_frames);
                    });
                tokio::runtime::Runtime::new()
                    .map_err(|e| e.to_string())?
                    .block_on(execute_project_render_command_with_control(
                        &request, &project, control,
                    ))
                    .map_err(|e| e.to_string())
            })();
            if let Ok(mut cancellations) = state_cancellations.lock() {
                cancellations.remove(&id);
            }
            if let Err(error) = result {
                if let Ok(mut store) = state_jobs.lock() {
                    if store
                        .get(&id)
                        .is_some_and(|job| job.status != JobStatus::Cancelled)
                    {
                        let _ = store.fail(&id, error);
                    }
                }
            } else {
                let frames = output_frame_count;
                if let Err(error) =
                    RenderJobController::new(Arc::clone(&state_jobs)).complete(&id, frames)
                {
                    if let Ok(mut store) = state_jobs.lock() {
                        if store
                            .get(&id)
                            .is_some_and(|job| job.status != JobStatus::Cancelled)
                        {
                            let _ = store
                                .fail(&id, format!("render completion transition failed: {error}"));
                        }
                    }
                }
            }
        });
        return Ok(());
    }
    let (worker, configured_url, frontend_path) =
        browser_preflight.ok_or_else(|| "Browser render preflight was not prepared".to_string())?;
    let concurrency = project
        .settings
        .concurrency
        .map(|value| value as usize)
        .or_else(|| {
            std::env::var("DIOXUSCUT_BROWSER_CONCURRENCY")
                .ok()
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or_else(default_browser_concurrency);
    let state_jobs = Arc::clone(&state.jobs);
    let state_cancellations = Arc::clone(&state.cancellations);
    let cancellation = make_cancel_signal();
    state_cancellations
        .lock()
        .map_err(|_| "cancellation store lock poisoned".to_string())?
        .insert(id.clone(), cancellation.clone());
    thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let runtime = tokio::runtime::Runtime::new().map_err(|error| error.to_string())?;
            let local_server = frontend_path
                .as_ref()
                .map(|root| runtime.block_on(spawn_server(0, root)))
                .transpose()
                .map_err(|error| error.to_string())?;
            let url = configured_url
                .clone()
                .or_else(|| local_server.as_ref().map(|server| server.url().to_string()))
                .ok_or_else(|| "browser rendering URL is unavailable".to_string())?;
            let node = std::env::var_os("DIOXUSCUT_BROWSER_NODE").unwrap_or_else(|| "node".into());
            let mut backend = BrowserFrameBackend::with_concurrency(node, worker, url, concurrency)
                .map_err(|error| error.to_string())?
                .with_frame_cache_bytes(browser_frame_cache_bytes());
            if let Some(format) = project.settings.browser_image_format.as_deref() {
                backend = backend.with_image_format(format);
            }
            if let Some(quality) = project.settings.browser_jpeg_quality {
                backend = backend.with_jpeg_quality(quality);
            }
            if let Some(timeout_ms) = project.settings.browser_frame_timeout_ms {
                backend = backend.with_frame_timeout(std::time::Duration::from_millis(timeout_ms));
            }
            if let Some(retries) = project.settings.browser_transport_retries {
                backend = backend.with_transport_retries(retries);
            }
            // Tauri owns a local process-scoped filesystem, so prefer the
            // faster lossless path unless a project explicitly requests
            // the portable JSON/base64 compatibility transport.
            if use_file_transport(project.settings.browser_transport.as_deref()) {
                backend = backend.with_file_transport(true);
            }
            backend
                .set_composition(&project.composition)
                .map_err(|error| error.to_string())?;
            backend
                .set_assets(
                    project
                        .assets
                        .iter()
                        .map(|asset| asset.path.clone())
                        .collect(),
                )
                .map_err(|error| error.to_string())?;
            backend
                .set_timeline(
                    project
                        .tracks
                        .iter()
                        .flat_map(|track| track.clips.iter())
                        .map(|clip| WebTimelineClip {
                            id: clip.id.clone(),
                            composition: clip.composition.clone(),
                            start: clip.start,
                            duration: clip.duration,
                            props: clip.props.clone(),
                        })
                        .collect(),
                )
                .map_err(|error| error.to_string())?;
            backend
                .set_time_events(
                    project
                        .events
                        .iter()
                        .map(|event| dioxuscut_rasterizer::WebTimeEvent {
                            id: event.id.clone(),
                            frame: event.frame,
                        })
                        .collect(),
                )
                .map_err(|error| error.to_string())?;
            let state_for_progress = Arc::clone(&state_jobs);
            let progress_id = id.clone();
            let diagnostics_state = Arc::clone(&state_jobs);
            let diagnostics_id = id.clone();
            let encoding_state = Arc::clone(&state_jobs);
            let encoding_id = id.clone();
            let control = RenderControl::new()
                .with_cancellation(cancellation)
                .with_progress(move |progress| {
                    if let Ok(mut store) = state_for_progress.lock() {
                        let _ = store.try_update(
                            &progress_id,
                            JobStatus::Rendering,
                            progress.completed_frames,
                        );
                    }
                })
                .with_diagnostics(move |diagnostics: RenderDiagnostics| {
                    if let Ok(mut store) = diagnostics_state.lock() {
                        let _ = store.set_render_diagnostics(
                            &diagnostics_id,
                            diagnostics.gpu_frames,
                            diagnostics.cpu_fallback_frames,
                            diagnostics.fallback_reason,
                        );
                    }
                })
                .with_encoding_progress(move |progress: EncodingProgress| {
                    let controller = RenderJobController::new(Arc::clone(&encoding_state));
                    let _ = controller.set_encoding_progress(&encoding_id, progress.encoded_frames);
                });
            let output_path = PathBuf::from(&output);
            if let Some(format) = project_still_format(&output_path) {
                render_still_fallible_scaled(
                    &backend,
                    project.settings.width,
                    project.settings.height,
                    project.settings.fps,
                    render_frame_start,
                    &output_path,
                    format,
                    &control,
                    project.settings.scale,
                    |_| Ok::<_, std::convert::Infallible>(dioxuscut_rasterizer::Scene::new()),
                )
                .map_err(|error| error.to_string())?;
            } else {
                let config = PipeConfig::new(
                    project.settings.width,
                    project.settings.height,
                    project.settings.fps,
                    output_frame_count,
                    output_path.clone(),
                )
                .with_codec(project_video_codec(&output_path))
                .with_scale(project.settings.scale)
                .with_frame_step(project.settings.frame_step)
                .with_frame_start(render_frame_start)
                .with_quality(
                    project.settings.crf.unwrap_or(18),
                    project
                        .settings
                        .preset
                        .clone()
                        .unwrap_or_else(|| "fast".into()),
                )
                .with_audio_tracks(
                    dioxuscut_cli::project_audio_assets(&project)
                        .into_iter()
                        .map(|path| dioxuscut_rasterizer::AudioTrack::new(path.to_string_lossy()))
                        .collect::<Vec<_>>(),
                )
                .with_control(control);
                render_web_to_ffmpeg_pipe_fallible(&backend, &config, project.props.clone())
                    .map_err(|error| error.to_string())?;
            }
            RenderJobController::new(Arc::clone(&state_jobs)).complete(&id, output_frame_count)?;
            Ok(())
        })();
        if let Ok(mut cancellations) = state_cancellations.lock() {
            cancellations.remove(&id);
        }
        if let Err(error) = result {
            if let Ok(mut store) = state_jobs.lock() {
                if store
                    .get(&id)
                    .is_some_and(|job| job.status != JobStatus::Cancelled)
                {
                    let _ = store.fail(&id, error);
                }
            }
        }
    });
    Ok(())
}

#[tauri::command]
fn load_project(path: String) -> Result<Project, String> {
    load_project_file(std::path::Path::new(&path))
}

fn load_project_file(path: &std::path::Path) -> Result<Project, String> {
    let mut project = Project::load(path).map_err(|error| error.to_string())?;
    let base_dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    project
        .validate_asset_files(base_dir)
        .map_err(|error| error.to_string())?;
    project.resolve_local_asset_paths(base_dir);
    Ok(project)
}

#[tauri::command]
fn save_project(path: String, project: Project) -> Result<(), String> {
    let path = std::path::PathBuf::from(path);
    let mut project = project;
    let base_dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    project.relativize_local_asset_paths(base_dir);
    project.save(path).map_err(|error| error.to_string())
}

#[tauri::command]
fn validate_project(source: String) -> Result<Project, String> {
    Project::from_json_str(&source).map_err(|error| error.to_string())
}

#[tauri::command]
fn project_schema() -> Result<serde_json::Value, String> {
    serde_json::from_str(include_str!(
        "../../../../schemas/dioxuscut-project-v1.schema.json"
    ))
    .map_err(|error| format!("embedded project schema is invalid: {error}"))
}

#[tauri::command]
fn get_render_job(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Option<RenderJob>, String> {
    RenderJobController::new(Arc::clone(&state.jobs)).get(&id)
}

#[tauri::command]
fn list_render_jobs(state: tauri::State<'_, AppState>) -> Result<Vec<RenderJob>, String> {
    RenderJobController::new(Arc::clone(&state.jobs)).list()
}

#[tauri::command]
fn update_render_job(
    state: tauri::State<'_, AppState>,
    id: String,
    status: JobStatus,
    completed_frames: u32,
) -> Result<(), String> {
    RenderJobController::new(Arc::clone(&state.jobs)).update(&id, status, completed_frames)
}

#[tauri::command]
fn fail_render_job(
    state: tauri::State<'_, AppState>,
    id: String,
    message: String,
) -> Result<(), String> {
    state
        .jobs
        .lock()
        .map_err(|_| "job store lock poisoned".to_string())?
        .fail(&id, message)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn cancel_render_job(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    if let Ok(cancellations) = state.cancellations.lock() {
        if let Some(token) = cancellations.get(&id) {
            token.cancel();
        }
    }
    state
        .jobs
        .lock()
        .map_err(|_| "job store lock poisoned".to_string())?
        .cancel(&id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn retry_render_job(state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    state
        .jobs
        .lock()
        .map_err(|_| "job store lock poisoned".to_string())?
        .retry(&id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn validate_frame_request(request: WebFrameRequest) -> Result<WebFrameRequest, String> {
    if request.width == 0 || request.height == 0 {
        return Err("preview dimensions must be greater than zero".into());
    }
    if !request.fps.is_finite() || request.fps <= 0.0 {
        return Err("preview fps must be finite and greater than zero".into());
    }
    Ok(request)
}

#[tauri::command]
fn backend_capabilities() -> BackendCapabilities {
    BackendCapabilities {
        native_scene: true,
        browser_runtime: true,
        // Preview remains Chromium-backed; native WGPU export is optional.
        // Only advertise it when this Tauri target was built with `gpu`.
        gpu_accelerated: cfg!(feature = "gpu"),
        supports_streaming: false,
    }
}

#[tauri::command]
fn web_worker_protocol() -> serde_json::Value {
    serde_json::json!({
        "version": WEB_WORKER_PROTOCOL_VERSION,
        "messages": ["ready", "render", "frame", "error", "shutdown"],
        "example": serde_json::to_value(WebWorkerMessage::Ready {
            protocol: WEB_WORKER_PROTOCOL_VERSION,
            compositions: vec![],
        }).expect("protocol message is serializable"),
    })
}

#[tauri::command]
fn list_browser_compositions() -> Result<Vec<String>, String> {
    let worker = browser_worker_path()?;
    let url = std::env::var("DIOXUSCUT_BROWSER_URL")
        .unwrap_or_else(|_| "http://localhost:1420".to_string());
    let node = std::env::var_os("DIOXUSCUT_BROWSER_NODE").unwrap_or_else(|| "node".into());
    let backend = BrowserFrameBackend::with_concurrency(node, worker, url, 1)
        .map_err(|error| error.to_string())?
        .with_frame_cache_bytes(browser_frame_cache_bytes());
    Ok(backend.compositions())
}

// Keep the privileged Studio webview on its packaged origin. The dev server is
// accepted only by debug builds; the render worker uses a separate Chromium process.
fn allow_studio_navigation(url: &tauri::Url, allow_dev_server: bool) -> bool {
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }

    match (url.scheme(), url.host_str(), url.port()) {
        ("tauri", Some("localhost"), None) => true,
        ("http" | "https", Some("tauri.localhost"), None) => true,
        ("http", Some("localhost"), Some(1420)) => allow_dev_server,
        _ => false,
    }
}

fn local_navigation_guard() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::<tauri::Wry>::new("local-navigation-guard")
        .on_navigation(|_, url| allow_studio_navigation(url, cfg!(debug_assertions)))
        .build()
}

fn main() {
    tauri::Builder::default()
        .plugin(local_navigation_guard())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            jobs: Arc::new(Mutex::new(JobStore::default())),
            cancellations: Arc::new(Mutex::new(HashMap::new())),
        })
        .invoke_handler(tauri::generate_handler![
            backend_capabilities,
            web_worker_protocol,
            list_browser_compositions,
            validate_frame_request,
            submit_project,
            submit_project_from_path,
            start_render_job,
            load_project,
            save_project,
            validate_project,
            project_schema,
            get_render_job,
            list_render_jobs,
            update_render_job,
            fail_render_job,
            cancel_render_job,
            retry_render_job
        ])
        .run(tauri::generate_context!())
        .expect("error while running Dioxuscut Studio");
}

#[cfg(test)]
mod tests {
    use super::{
        allow_studio_navigation, create_render_job_temp_dir, default_browser_concurrency,
        selected_output_frame_count, use_file_transport,
    };

    #[test]
    fn tauri_defaults_to_lossless_file_transport() {
        assert!(use_file_transport(None));
        assert!(use_file_transport(Some(" FILE ")));
        assert!(!use_file_transport(Some("base64")));
    }

    #[test]
    fn browser_concurrency_has_a_bounded_positive_default() {
        assert!((1..=4).contains(&default_browser_concurrency()));
    }

    #[test]
    fn output_frame_count_uses_stride_for_video_and_one_frame_for_stills() {
        assert_eq!(selected_output_frame_count(100, 199, 2, false), 50);
        assert_eq!(selected_output_frame_count(100, 199, 3, false), 34);
        assert_eq!(selected_output_frame_count(100, 199, 3, true), 1);
    }

    #[test]
    fn render_jobs_get_distinct_temporary_directories() {
        let first = create_render_job_temp_dir().unwrap();
        let second = create_render_job_temp_dir().unwrap();
        assert_ne!(first.path(), second.path());
    }

    #[test]
    fn navigation_guard_allows_only_studio_origins_and_dev_server() {
        let is_allowed = |raw_url: &str, allow_dev_server| {
            let url = raw_url.parse().expect("URL should parse");
            allow_studio_navigation(&url, allow_dev_server)
        };

        assert!(is_allowed("tauri://localhost/index.html", false));
        assert!(is_allowed("http://tauri.localhost/", false));
        assert!(is_allowed("https://tauri.localhost/", false));
        assert!(is_allowed("http://localhost:1420/", true));
        assert!(!is_allowed("http://localhost:1420/", false));

        assert!(!is_allowed("http://tauri.attacker.com/", false));
        assert!(!is_allowed("https://tauri.localhost.attacker.com/", false));
        assert!(!is_allowed("http://tauri.localhost:8080/", false));
        assert!(!is_allowed("http://tauri.localhost@attacker.com/", false));
        assert!(!is_allowed("http://localhost:1421/", true));
        assert!(!is_allowed("https://localhost:1420/", true));
        assert!(!is_allowed("file:///etc/passwd", true));
    }
}
