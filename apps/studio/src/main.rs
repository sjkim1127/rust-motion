//! # Rust Motion Studio
//!
//! Desktop video editor application — Remotion Studio equivalent.
//!
//! Provides:
//! - Shared native composition preview with the `<Player>` component
//! - Composition selection from the built-in registry
//! - Render queue with real async rendering via `rust-motion-cli`
//! - Properties panel showing live composition metadata

use dioxus::prelude::*;
use dioxus_desktop::{Config, LogicalSize, WindowBuilder};
use rust_motion_cli::{
    built_in_registry, execute_render_command_with_registry_and_control, RenderBackend,
    RenderCodec, RenderRequest,
};
use rust_motion_composition::HelloWorldComposition;
use rust_motion_player::{CompositionHandle, NativeCompositionPreview, Player};
use rust_motion_rasterizer::RenderControl;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

// ─── Window config ────────────────────────────────────────────────────────────
const WINDOW_WIDTH: u32 = 1600;
const WINDOW_HEIGHT: u32 = 960;

// ─── Composition catalogue ────────────────────────────────────────────────────

/// Static metadata for a composition shown in the left-panel list.
#[derive(Clone, PartialEq, Debug)]
struct CompositionMeta {
    id: String,
    width: u32,
    height: u32,
    fps: f64,
    duration_in_frames: u32,
}

impl CompositionMeta {
    fn duration_secs(&self) -> f64 {
        self.duration_in_frames as f64 / self.fps
    }
}

/// Returns the catalogue of available compositions by querying the built-in registry.
fn composition_catalogue() -> Vec<CompositionMeta> {
    // Default configuration for all built-in compositions.
    // A future version would expose per-composition defaults from the registry.
    let registry = built_in_registry();
    registry
        .ids()
        .into_iter()
        .map(|id| CompositionMeta {
            id: id.to_string(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration_in_frames: 180,
        })
        .collect()
}

// ─── Render queue ─────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Debug)]
enum RenderStatus {
    Queued,
    Running { percent: u8 },
    Done { output: PathBuf, elapsed: Duration },
    Failed { reason: String },
    Cancelled,
}

#[derive(Clone)]
struct RenderJob {
    id: u64,
    composition_id: String,
    width: u32,
    height: u32,
    fps: f64,
    duration_in_frames: u32,
    output: PathBuf,
    status: RenderStatus,
    queued_at: Instant,
    cancellation_token: rust_motion_rasterizer::RenderCancellationToken,
}

impl PartialEq for RenderJob {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.composition_id == other.composition_id
            && self.width == other.width
            && self.height == other.height
            && (self.fps - other.fps).abs() < f64::EPSILON
            && self.duration_in_frames == other.duration_in_frames
            && self.output == other.output
            && self.status == other.status
            && self.queued_at == other.queued_at
    }
}

impl RenderJob {
    fn new(
        id: u64,
        meta: &CompositionMeta,
        output: PathBuf,
        cancellation_token: rust_motion_rasterizer::RenderCancellationToken,
    ) -> Self {
        Self {
            id,
            composition_id: meta.id.clone(),
            width: meta.width,
            height: meta.height,
            fps: meta.fps,
            duration_in_frames: meta.duration_in_frames,
            output,
            status: RenderStatus::Queued,
            queued_at: Instant::now(),
            cancellation_token,
        }
    }

    fn status_label(&self) -> String {
        match &self.status {
            RenderStatus::Queued => "Queued".into(),
            RenderStatus::Running { percent } => format!("{percent}%"),
            RenderStatus::Done { elapsed, .. } => {
                format!("Done ({:.1}s)", elapsed.as_secs_f64())
            }
            RenderStatus::Failed { reason } => format!("Failed: {reason}"),
            RenderStatus::Cancelled => "Cancelled".into(),
        }
    }

    fn status_color(&self) -> &'static str {
        match &self.status {
            RenderStatus::Queued => "#94a3b8",
            RenderStatus::Running { .. } => "#6c63ff",
            RenderStatus::Done { .. } => "#22c55e",
            RenderStatus::Failed { .. } => "#ef4444",
            RenderStatus::Cancelled => "#f59e0b",
        }
    }

    fn progress_percent(&self) -> u8 {
        match &self.status {
            RenderStatus::Queued => 0,
            RenderStatus::Running { percent } => *percent,
            RenderStatus::Done { .. } => 100,
            RenderStatus::Failed { .. } | RenderStatus::Cancelled => 0,
        }
    }

    fn is_running(&self) -> bool {
        matches!(self.status, RenderStatus::Running { .. })
    }
}

// ─── App entry ────────────────────────────────────────────────────────────────

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("info,studio=debug")
        .init();

    let window = WindowBuilder::new()
        .with_title("Rust Motion Preview Studio")
        .with_inner_size(LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT))
        .with_resizable(true);

    dioxus_desktop::launch::launch(
        StudioApp,
        vec![],
        vec![Box::new(Config::new().with_window(window))],
    );
}

// ─── App shell ────────────────────────────────────────────────────────────────

#[component]
fn StudioApp() -> Element {
    let catalogue = use_memo(composition_catalogue);
    let mut selected_id: Signal<String> = use_signal(|| {
        catalogue
            .read()
            .first()
            .map(|c| c.id.clone())
            .unwrap_or_default()
    });

    // Render queue — updated from the render callback via Arc<Mutex>
    let jobs: Signal<Vec<RenderJob>> = use_signal(Vec::new);
    let next_job_id: Signal<u64> = use_signal(|| 0);

    let selected_meta = use_memo(move || {
        let id = selected_id.read().clone();
        catalogue
            .read()
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .or_else(|| catalogue.read().first().cloned())
    });

    rsx! {
        div {
            style: "
                display: grid;
                grid-template-rows: 48px 1fr 200px;
                grid-template-columns: 240px 1fr 300px;
                height: 100vh;
                background: #0d0d14;
                color: #e8e8f0;
                font-family: 'Inter', system-ui, sans-serif;
                overflow: hidden;
            ",

            // ── Top bar ────────────────────────────────────────────────────
            TopBar {
                selected_meta: selected_meta.read().clone(),
                jobs,
                next_job_id,
                selected_id: selected_id.read().clone(),
            }

            // ── Left panel: Compositions list ──────────────────────────────
            div {
                style: "
                    background: #12121b;
                    border-right: 1px solid rgba(255,255,255,0.07);
                    padding: 16px;
                    overflow-y: auto;
                ",
                h3 {
                    style: "font-size: 11px; text-transform: uppercase; letter-spacing: 0.08em; color: rgba(255,255,255,0.35); margin: 0 0 12px;",
                    "Compositions"
                }
                for meta in catalogue.read().iter() {
                    CompositionListItem {
                        key: "{meta.id}",
                        name: meta.id.clone(),
                        selected: *selected_id.read() == meta.id,
                        on_click: {
                            let id = meta.id.clone();
                            move |_| selected_id.set(id.clone())
                        },
                    }
                }
            }

            // ── Centre: Preview ────────────────────────────────────────────
            div {
                style: "
                    display: flex; flex-direction: column;
                    align-items: center; justify-content: center;
                    background: #0a0a12; padding: 24px; gap: 16px;
                    overflow: auto;
                ",
                if let Some(meta) = selected_meta.read().as_ref() {
                    Player {
                        width: 960,
                        height: 540,
                        fps: meta.fps,
                        duration_in_frames: meta.duration_in_frames,
                        controls: true,
                        PreviewComposition {}
                    }
                }
            }

            // ── Right panel: Properties + Render Queue ─────────────────────
            div {
                style: "
                    background: #12121b;
                    border-left: 1px solid rgba(255,255,255,0.07);
                    padding: 16px; overflow-y: auto;
                    display: flex; flex-direction: column; gap: 20px;
                ",
                // Properties
                div {
                    h3 {
                        style: "font-size: 11px; text-transform: uppercase; letter-spacing: 0.08em; color: rgba(255,255,255,0.35); margin: 0 0 12px;",
                        "Properties"
                    }
                    if let Some(meta) = selected_meta.read().as_ref() {
                        PropertyRow { label: "Composition", value: meta.id.clone() }
                        PropertyRow { label: "Width",       value: format!("{}px", meta.width) }
                        PropertyRow { label: "Height",      value: format!("{}px", meta.height) }
                        PropertyRow { label: "FPS",         value: format!("{}", meta.fps) }
                        PropertyRow { label: "Duration",    value: format!("{:.1}s ({}f)", meta.duration_secs(), meta.duration_in_frames) }
                        PropertyRow { label: "Codec",       value: "H.264".to_string() }
                    }
                }
                // Render Queue
                RenderQueuePanel { jobs }
            }

            // ── Bottom: Timeline ───────────────────────────────────────────
            div {
                style: "
                    grid-column: 1/-1;
                    background: #10101a;
                    border-top: 1px solid rgba(255,255,255,0.07);
                    padding: 16px;
                    overflow-x: auto;
                ",
                TimelinePanel {
                    meta: selected_meta.read().clone(),
                }
            }
        }
    }
}

// ─── Top bar ──────────────────────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
struct TopBarProps {
    selected_meta: Option<CompositionMeta>,
    jobs: Signal<Vec<RenderJob>>,
    next_job_id: Signal<u64>,
    selected_id: String,
}

#[component]
fn TopBar(mut props: TopBarProps) -> Element {
    let rendering = props
        .jobs
        .read()
        .iter()
        .any(|j| matches!(j.status, RenderStatus::Running { .. }));

    let meta_label = props
        .selected_meta
        .as_ref()
        .map(|m| format!("{} — {}×{} @ {}fps", m.id, m.width, m.height, m.fps))
        .unwrap_or_else(|| "No composition selected".into());
    let selected_meta_clone = props.selected_meta.clone();

    let on_render = move |_| {
        let Some(ref meta) = selected_meta_clone else {
            return;
        };
        let job_id = *props.next_job_id.read();
        props.next_job_id += 1;

        // Derive output path: ~/Desktop/Rust Motion_<id>_<timestamp>.mp4
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let output = PathBuf::from(format!(
            "{}/Desktop/Rust Motion_{}_{}_{}.mp4",
            std::env::var("HOME").unwrap_or_default(),
            meta.id,
            meta.width,
            ts,
        ));

        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel::<u8>();

        // Create RenderControl with cancellation token and reactive progress callback
        let control = RenderControl::new().with_progress(move |p| {
            let pct = if p.total_frames > 0 {
                ((p.completed_frames as f64 / p.total_frames as f64) * 100.0).min(99.0) as u8
            } else {
                0
            };
            let _ = progress_tx.send(pct);
        });
        let cancel_token = control.cancellation_token();

        let job = RenderJob::new(job_id, meta, output.clone(), cancel_token);
        props.jobs.push(job);

        // Build the render request
        let request = Arc::new(RenderRequest {
            composition: Some(props.selected_id.clone()),
            script: None,
            props: None,
            output: output.clone(),
            audio: vec![],
            width: meta.width,
            height: meta.height,
            fps: meta.fps,
            duration: meta.duration_in_frames,
            backend: RenderBackend::Native,
            codec: RenderCodec::H264,
            frame_start: 0,
            frame_end: None,
            frame_step: 1,
            scale: 1.0,
            concurrency: None,
            timeout_seconds: Some(300),
            crf: 18,
            preset: "fast".into(),
            hw_accel: rust_motion_rasterizer::HwAccel::Auto,
            sandbox_roots: vec![],
            permissive: false,
        });

        let mut jobs_signal = props.jobs;
        let start = Instant::now();

        // Background progress listener task
        let mut progress_signal = jobs_signal;
        spawn(async move {
            while let Some(percent) = progress_rx.recv().await {
                if let Some(job) = progress_signal.write().iter_mut().find(|j| j.id == job_id) {
                    if matches!(job.status, RenderStatus::Running { .. }) {
                        job.status = RenderStatus::Running { percent };
                    }
                }
            }
        });

        // Spawn an async task — tokio runtime provided by dioxus-desktop
        spawn(async move {
            // Mark as running at 0%
            if let Some(job) = jobs_signal.write().iter_mut().find(|j| j.id == job_id) {
                job.status = RenderStatus::Running { percent: 0 };
            }

            let registry = built_in_registry();
            let result =
                execute_render_command_with_registry_and_control(&request, &registry, control)
                    .await;

            let elapsed = start.elapsed();
            if let Some(job) = jobs_signal.write().iter_mut().find(|j| j.id == job_id) {
                job.status = match result {
                    Ok(()) => RenderStatus::Done {
                        output: output.clone(),
                        elapsed,
                    },
                    Err(e) => {
                        let err_str = e.to_string();
                        if err_str.to_lowercase().contains("cancel") {
                            RenderStatus::Cancelled
                        } else {
                            RenderStatus::Failed { reason: err_str }
                        }
                    }
                };
            }
        });
    };

    let btn_bg = if rendering { "#3a3a5c" } else { "#6c63ff" };
    let btn_color = if rendering {
        "rgba(255,255,255,0.4)"
    } else {
        "white"
    };
    let btn_cursor = if rendering { "not-allowed" } else { "pointer" };
    let btn_label = if rendering {
        "⏳ Rendering…"
    } else {
        "▶ Render"
    };

    rsx! {
        div {
            style: "background: #16161f; border-bottom: 1px solid rgba(255,255,255,0.07); display: flex; align-items: center; gap: 16px; padding: 0 20px; grid-column-start: 1; grid-column-end: -1;",
            span {
                style: "font-size: 15px; font-weight: 700; color: #6c63ff; letter-spacing: -0.02em;",
                "🦀 Rust Motion Studio"
            }
            span { style: "color: rgba(255,255,255,0.2);", "│" }
            span {
                style: "font-size: 13px; color: rgba(255,255,255,0.5);",
                "{meta_label}"
            }
            div { style: "flex: 1;" }
            button {
                disabled: rendering,
                onclick: on_render,
                style: "background: {btn_bg}; color: {btn_color}; border: none; padding: 6px 16px; border-radius: 6px; font-size: 13px; cursor: {btn_cursor}; font-weight: 600; transition: background 0.2s;",
                "{btn_label}"
            }
        }
    }
}

// ─── Render queue panel ────────────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
struct RenderQueuePanelProps {
    jobs: Signal<Vec<RenderJob>>,
}

#[component]
fn RenderQueuePanel(props: RenderQueuePanelProps) -> Element {
    let jobs = props.jobs.read();
    rsx! {
        div {
            h3 {
                style: "font-size: 11px; text-transform: uppercase; letter-spacing: 0.08em; color: rgba(255,255,255,0.35); margin: 0 0 12px;",
                "Render Queue ({jobs.len()})"
            }
            if jobs.is_empty() {
                div {
                    style: "font-size: 12px; color: rgba(255,255,255,0.25); text-align: center; padding: 16px 0;",
                    "No renders yet. Click ▶ Render to start."
                }
            } else {
                div {
                    style: "display: flex; flex-direction: column; gap: 8px;",
                    for job in jobs.iter().rev() {
                        RenderJobRow {
                            job: job.clone(),
                            jobs: props.jobs,
                        }
                    }
                }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct RenderJobRowProps {
    job: RenderJob,
    jobs: Signal<Vec<RenderJob>>,
}

#[component]
fn RenderJobRow(mut props: RenderJobRowProps) -> Element {
    let job = &props.job;
    let job_id = job.id;
    let pct = job.progress_percent();
    let color = job.status_color();
    let label = job.status_label();
    let is_running = job.is_running();
    let filename = job
        .output
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("output.mp4");

    let on_cancel = move |_| {
        if let Some(j) = props.jobs.write().iter_mut().find(|j| j.id == job_id) {
            j.cancellation_token.cancel();
            j.status = RenderStatus::Cancelled;
        }
    };

    rsx! {
        div {
            style: "
                background: rgba(255,255,255,0.04);
                border: 1px solid rgba(255,255,255,0.06);
                border-radius: 6px; padding: 8px 10px;
                font-size: 12px;
            ",
            div {
                style: "display: flex; justify-content: space-between; align-items: center; margin-bottom: 5px;",
                span {
                    style: "color: rgba(255,255,255,0.85); font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 140px;",
                    "{job.composition_id}"
                }
                div {
                    style: "display: flex; align-items: center; gap: 6px;",
                    span {
                        style: "color: {color}; font-size: 11px; font-weight: 600; white-space: nowrap;",
                        "{label}"
                    }
                    if is_running {
                        button {
                            onclick: on_cancel,
                            title: "Cancel render",
                            style: "background: rgba(239,68,68,0.2); color: #ef4444; border: 1px solid rgba(239,68,68,0.4); border-radius: 4px; padding: 1px 6px; font-size: 10px; cursor: pointer; font-weight: 600; transition: background 0.2s;",
                            "✕ Cancel"
                        }
                    }
                }
            }
            // Progress bar
            div {
                style: "height: 3px; background: rgba(255,255,255,0.08); border-radius: 2px; overflow: hidden;",
                div {
                    style: "height: 100%; width: {pct}%; background: {color}; transition: width 0.3s ease; border-radius: 2px;",
                }
            }
            div {
                style: "margin-top: 4px; color: rgba(255,255,255,0.3); font-size: 10px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;",
                "{filename}"
            }
        }
    }
}

// ─── Composition list item ─────────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
struct CompositionListItemProps {
    name: String,
    selected: bool,
    on_click: EventHandler<MouseEvent>,
}

#[component]
fn CompositionListItem(props: CompositionListItemProps) -> Element {
    let bg = if props.selected {
        "rgba(108, 99, 255, 0.15)"
    } else {
        "transparent"
    };
    let border = if props.selected {
        "1px solid rgba(108, 99, 255, 0.4)"
    } else {
        "1px solid transparent"
    };
    let color = if props.selected {
        "#c4bfff"
    } else {
        "rgba(255,255,255,0.6)"
    };

    rsx! {
        div {
            onclick: move |e| props.on_click.call(e),
            style: "
                padding: 8px 10px;
                border-radius: 6px;
                font-size: 13px;
                cursor: pointer;
                background: {bg};
                border: {border};
                color: {color};
                margin-bottom: 4px;
                transition: all 0.15s;
            ",
            "▶ {props.name}"
        }
    }
}

// ─── Property row ──────────────────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
struct PropertyRowProps {
    label: String,
    value: String,
}

#[component]
fn PropertyRow(props: PropertyRowProps) -> Element {
    rsx! {
        div {
            style: "display: flex; justify-content: space-between; padding: 8px 0; border-bottom: 1px solid rgba(255,255,255,0.05); font-size: 13px;",
            span { style: "color: rgba(255,255,255,0.45);", "{props.label}" }
            span {
                style: "color: rgba(255,255,255,0.85); font-family: monospace; font-size: 12px;",
                "{props.value}"
            }
        }
    }
}

// ─── Timeline panel ────────────────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
struct TimelinePanelProps {
    meta: Option<CompositionMeta>,
}

#[component]
fn TimelinePanel(props: TimelinePanelProps) -> Element {
    let (duration_frames, fps, label) = props
        .meta
        .as_ref()
        .map(|m| {
            (
                m.duration_in_frames,
                m.fps,
                format!(
                    "Timeline — {:.1}s ({} frames @ {}fps)",
                    m.duration_secs(),
                    m.duration_in_frames,
                    m.fps
                ),
            )
        })
        .unwrap_or_else(|| (180, 30.0, "Timeline".into()));

    // Divide the composition into equal thirds as illustrative tracks.
    let third = duration_frames / 3;
    let tracks = [
        ("Scene 1: Title", 0u32, third, "#6c63ff"),
        ("Scene 2: Body", third, third, "#22c55e"),
        (
            "Scene 3: Outro",
            third * 2,
            duration_frames - third * 2,
            "#f59e0b",
        ),
    ];

    rsx! {
        div {
            h3 {
                style: "font-size: 11px; text-transform: uppercase; letter-spacing: 0.08em; color: rgba(255,255,255,0.35); margin: 0 0 12px;",
                "{label}"
            }
            // Ruler — one tick per second
            div {
                style: "display: flex; margin-bottom: 6px; padding-left: 152px;",
                for sec in 0..=(duration_frames as f64 / fps).ceil() as u32 {
                    div {
                        key: "ruler-{sec}",
                        style: "
                            flex: 1; font-size: 10px;
                            color: rgba(255,255,255,0.2);
                            border-left: 1px solid rgba(255,255,255,0.1);
                            padding-left: 3px;
                        ",
                        "{sec}s"
                    }
                }
            }
            div {
                style: "display: flex; flex-direction: column; gap: 6px;",
                for (name, from, dur, color) in tracks {
                    div {
                        key: "{name}",
                        style: "display: flex; align-items: center; gap: 12px;",
                        div {
                            style: "font-size: 12px; color: rgba(255,255,255,0.5); width: 140px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;",
                            "{name}"
                        }
                        div {
                            style: "flex: 1; height: 24px; background: rgba(255,255,255,0.05); border-radius: 4px; position: relative;",
                            div {
                                style: "
                                    position: absolute;
                                    left: {from as f64 / duration_frames as f64 * 100.0:.1}%;
                                    width: {dur as f64 / duration_frames as f64 * 100.0:.1}%;
                                    height: 100%;
                                    background: {color};
                                    opacity: 0.7;
                                    border-radius: 4px;
                                    display: flex; align-items: center; padding: 0 6px;
                                    font-size: 11px; color: white; white-space: nowrap; overflow: hidden;
                                ",
                                "{from}f–{from + dur}f"
                            }
                        }
                    }
                }
            }
        }
    }
}

// ─── Shared native composition preview ────────────────────────────────────────

#[component]
fn PreviewComposition() -> Element {
    let composition = use_hook(|| CompositionHandle::new(HelloWorldComposition));
    let input_props = serde_json::json!({
        "title": "Rust Motion",
        "subtitle": "One scene contract for preview and export",
        "background_start": "#1a0533",
        "background_end": "#001a33",
        "accent_color": "#6c63ff"
    });

    rsx! {
        NativeCompositionPreview {
            composition,
            input_props,
            width: 960,
            height: 540,
            fps: 30.0,
            duration_in_frames: 180,
        }
    }
}
