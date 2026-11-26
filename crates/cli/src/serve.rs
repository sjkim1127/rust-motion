//! `rust-motion serve` — embedded hot-reloading web studio with WebSocket frame preview.
//!
//! Architecture
//! ────────────
//! ```text
//! ┌──────────────────┐     GET /       ┌──────────────────────┐
//! │  Browser / curl  │ ◄─────────────► │  Axum HTTP server    │
//! │                  │   WS /ws        │  localhost:<port>    │
//! └──────────────────┘ ◄─────────────► └──────────┬───────────┘
//!                                                 │
//!                        Tokio broadcast channel  │
//!                        ┌───────────────────────►│
//!                        │                        │ bounded blocking tasks
//!                   ┌────┴──────┐         ┌───────▼──────────┐
//!                   │  file     │         │ RhaiComposition  │
//!                   │  poller   │         │ render frame N   │
//!                   └───────────┘         └──────────────────┘
//!                   script, props and referenced assets
//! ```
//!
//! Rendering never runs on an async executor thread. A cached frame is served only while the
//! script, props and every file its scene references are unchanged.
//!
//! WebSocket message format (server → client):
//! ```json
//! { "type": "frame", "data": "<base64-png>", "frame": 0 }
//! { "type": "error", "code": "render_failed", "frame": 0, "message": "..." }
//! ```
//!
//! Each connection renders one frame at a time. Seeks that arrive meanwhile replace each other, so
//! only the latest requested frame is rendered next.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::{Html, IntoResponse},
    routing::get,
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rust_motion_rasterizer::{MediaSecurityPolicy, TinySkiaBackend};
use serde::Deserialize;
use std::{
    collections::{BTreeSet, VecDeque},
    future::Future,
    net::SocketAddr,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{broadcast, Semaphore};
use tracing::{error, info, warn};

/// Maximum number of frame messages buffered in the broadcast channel.
const CHANNEL_CAPACITY: usize = 4;
/// Maximum number of rendered preview frames retained for scrubbing.
const PREVIEW_CACHE_CAPACITY: usize = 8;
/// How often watched files are checked for changes.
const POLL_INTERVAL: Duration = Duration::from_millis(250);
/// Upper bound on frames rendering at the same time, across all clients.
const MAX_CONCURRENT_RENDERS: usize = 4;
/// Strings longer than this are scene content, not file paths.
const MAX_PATH_LEN: usize = 4096;

// ──────────────────────────────────────────────────────────────
// Public configuration type
// ──────────────────────────────────────────────────────────────

/// Configuration for the `rust-motion serve` subcommand.
#[derive(Debug, Clone)]
pub struct ServeConfig {
    /// Path to the Rhai composition script.
    pub script: PathBuf,
    /// Optional path to a JSON props file.
    pub props: Option<PathBuf>,
    /// TCP port to bind on (default: 7890).
    pub port: u16,
    /// Which frame to preview (default: 0). May be overridden per WebSocket client via `?frame=N`.
    pub default_frame: u32,
    /// Render width.
    pub width: u32,
    /// Render height.
    pub height: u32,
    /// Frames per second (for context only, not used for playback here).
    pub fps: f64,
    /// Duration in frames (for context).
    pub duration: u32,
}

// ──────────────────────────────────────────────────────────────
// Shared application state
// ──────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    /// Source-change notifications; each client re-renders its current frame.
    tx: broadcast::Sender<()>,
    config: Arc<ServeConfig>,
    frame_cache: Arc<Mutex<VecDeque<CachedFrame>>>,
    render_slots: Arc<Semaphore>,
}

impl AppState {
    fn new(
        tx: broadcast::Sender<()>,
        config: Arc<ServeConfig>,
        frame_cache: Arc<Mutex<VecDeque<CachedFrame>>>,
    ) -> Self {
        let slots = std::thread::available_parallelism()
            .map_or(1, std::num::NonZeroUsize::get)
            .min(MAX_CONCURRENT_RENDERS);
        Self {
            tx,
            config,
            frame_cache,
            render_slots: Arc::new(Semaphore::new(slots)),
        }
    }

    /// Render (or fetch) a frame on a blocking thread once a render slot is free.
    ///
    /// Dropping the future while it waits for a slot cancels the request.
    async fn frame(&self, frame: u32) -> anyhow::Result<Arc<Vec<u8>>> {
        let slot = self.render_slots.clone().acquire_owned().await?;
        let config = self.config.clone();
        let cache = self.frame_cache.clone();
        tokio::task::spawn_blocking(move || {
            // The slot stays taken until the render finishes, even if the caller goes away.
            let _slot = slot;
            cached_frame(&config, frame, &cache)
        })
        .await
        .map_err(|error| anyhow::anyhow!("frame render task failed: {error}"))?
    }
}

/// State of the files a scene reads, taken before they are read.
type Dependencies = Vec<(PathBuf, FileStamp)>;

#[derive(Clone)]
struct CachedFrame {
    frame: u32,
    source_stamp: SourceStamp,
    dependencies: Dependencies,
    png: Arc<Vec<u8>>,
}

impl CachedFrame {
    fn is_current(&self, source_stamp: SourceStamp) -> bool {
        self.source_stamp == source_stamp && dependencies_unchanged(&self.dependencies)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SourceStamp {
    script: FileStamp,
    props: Option<FileStamp>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FileStamp {
    modified_nanos: Option<u128>,
    len: Option<u64>,
    /// Inode and change time, which also differ after an atomic replacement that preserves the
    /// modification time and length. `None` where the platform does not provide them.
    identity: Option<(u64, i64, i64)>,
}
// ──────────────────────────────────────────────────────────────
// Entry point
// ──────────────────────────────────────────────────────────────

/// Run the serve loop.  Never returns under normal operation.
pub async fn run(config: ServeConfig) -> anyhow::Result<()> {
    let config = Arc::new(config);
    let frame_cache = Arc::new(Mutex::new(VecDeque::new()));
    let (tx, _) = broadcast::channel::<()>(CHANNEL_CAPACITY);

    let state = AppState::new(tx, config.clone(), frame_cache);

    // Render the initial frame immediately so the page is never blank.
    if let Err(error) = state.frame(config.default_frame).await {
        error!(frame = config.default_frame, error = %error, "Initial frame render failed");
    }

    // Spawn the file-watcher thread.
    spawn_watcher(state.tx.clone(), config.clone(), state.frame_cache.clone());

    let app = serve_router(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    info!(
        port = config.port,
        script = %config.script.display(),
        "rust-motion serve — open http://localhost:{} in your browser",
        config.port
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn serve_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/health", get(health_handler))
        .route("/frame", get(frame_handler))
        .route("/ws", get(ws_handler))
        .with_state(state)
}

async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "ok": true,
        "service": "rust-motion-serve",
        "protocol": 1,
        "composition": state.config.script.display().to_string(),
        "frame": state.config.default_frame,
        "width": state.config.width,
        "height": state.config.height,
        "fps": state.config.fps,
        "preview_cache": {
            "entries": state.frame_cache.lock().map(|cache| cache.len()).unwrap_or(0),
            "capacity": PREVIEW_CACHE_CAPACITY,
        },
    }))
}

#[derive(Debug, Deserialize)]
struct FrameQuery {
    frame: Option<u32>,
}

async fn frame_handler(
    State(state): State<AppState>,
    Query(query): Query<FrameQuery>,
) -> impl IntoResponse {
    let frame = query.frame.unwrap_or(state.config.default_frame);
    match state.frame(frame).await {
        Ok(png) => Json(serde_json::json!({
            "type": "frame",
            "frame": frame,
            "width": state.config.width,
            "height": state.config.height,
            "png_base64": BASE64.encode(&*png),
        }))
        .into_response(),
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "type": "error",
                "frame": frame,
                "message": error.to_string(),
            })),
        )
            .into_response(),
    }
}

// ──────────────────────────────────────────────────────────────
// Render helper
// ──────────────────────────────────────────────────────────────

/// The WebSocket message for a finished render of `frame`.
fn frame_message(frame: u32, result: anyhow::Result<Arc<Vec<u8>>>) -> String {
    match result {
        Ok(png) => serde_json::json!({
            "type": "frame",
            "data": BASE64.encode(&*png),
            "frame": frame,
        }),
        Err(error) => {
            error!(frame, error = %error, "Render failed");
            serde_json::json!({
                "type": "error",
                "code": "render_failed",
                "frame": frame,
                "message": error.to_string(),
            })
        }
    }
    .to_string()
}

fn file_stamp(path: &Path) -> FileStamp {
    let Ok(metadata) = std::fs::metadata(path) else {
        return FileStamp::default();
    };
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos());
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        Some((metadata.ino(), metadata.ctime(), metadata.ctime_nsec()))
    };
    #[cfg(not(unix))]
    let identity = None;
    FileStamp {
        modified_nanos,
        len: Some(metadata.len()),
        identity,
    }
}

fn source_stamp(config: &ServeConfig) -> SourceStamp {
    SourceStamp {
        script: file_stamp(&config.script),
        props: config.props.as_deref().map(file_stamp),
    }
}

fn dependencies_unchanged(dependencies: &Dependencies) -> bool {
    dependencies
        .iter()
        .all(|(path, stamp)| file_stamp(path) == *stamp)
}

/// Every existing file named anywhere in the scene (images, video, fonts, animations, ...).
///
/// Paths are resolved the way the rasterizer resolves them, relative to the working directory.
#[cfg(feature = "rhai")]
fn scene_dependencies(scene: &rust_motion_rasterizer::Scene) -> anyhow::Result<Dependencies> {
    fn collect(value: &serde_json::Value, files: &mut BTreeSet<PathBuf>) {
        match value {
            serde_json::Value::String(text) => {
                let text = text.trim();
                let text = text.strip_prefix("file://").unwrap_or(text);
                if !text.is_empty() && text.len() <= MAX_PATH_LEN && !text.contains('\n') {
                    let path = Path::new(text);
                    if path.is_file() {
                        files.insert(path.to_path_buf());
                    }
                }
            }
            serde_json::Value::Array(values) => {
                values.iter().for_each(|value| collect(value, files));
            }
            serde_json::Value::Object(values) => {
                values.values().for_each(|value| collect(value, files));
            }
            _ => {}
        }
    }

    let mut files = BTreeSet::new();
    collect(&serde_json::to_value(scene)?, &mut files);
    Ok(files
        .into_iter()
        .map(|path| {
            let stamp = file_stamp(&path);
            (path, stamp)
        })
        .collect())
}

fn cached_frame(
    config: &ServeConfig,
    frame: u32,
    cache: &Mutex<VecDeque<CachedFrame>>,
) -> anyhow::Result<Arc<Vec<u8>>> {
    // Taken before rendering: if a file changes during the render, the result is already stale.
    let stamp = source_stamp(config);
    if let Ok(mut guard) = cache.lock() {
        let hit = guard
            .iter()
            .position(|cached| cached.frame == frame && cached.is_current(stamp));
        if let Some(cached) = hit.and_then(|index| guard.remove(index)) {
            let png = cached.png.clone();
            guard.push_front(cached);
            return Ok(png);
        }
    }
    let (png, dependencies) = render_frame(config, frame)?;
    let rendered = CachedFrame {
        frame,
        source_stamp: stamp,
        dependencies,
        png: Arc::new(png),
    };
    let png = rendered.png.clone();
    if let Ok(mut guard) = cache.lock() {
        // A slow render of old inputs must not displace a newer result.
        if rendered.is_current(source_stamp(config)) {
            guard.retain(|cached| cached.frame != frame);
            guard.push_front(rendered);
            guard.truncate(PREVIEW_CACHE_CAPACITY);
        }
    }
    Ok(png)
}

/// Render a single composition frame → raw PNG bytes and the files the scene read.
fn render_frame(config: &ServeConfig, frame: u32) -> anyhow::Result<(Vec<u8>, Dependencies)> {
    #[cfg(feature = "rhai")]
    {
        use crate::rhai_runtime::RhaiComposition;
        use rust_motion_composition::{Composition, NativeCompositionContext};
        use rust_motion_rasterizer::{FrameConfig, RasterizerBackend};

        let script_dir = config
            .script
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .unwrap_or_else(|| config.script.clone());

        let policy = MediaSecurityPolicy::sandboxed(vec![script_dir]);
        let cache_bytes = std::env::var("RUST_MOTION_IMAGE_CACHE_BYTES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(rust_motion_rasterizer::DEFAULT_IMAGE_CACHE_BYTES);
        let backend = TinySkiaBackend::new()
            .with_image_cache_bytes(cache_bytes)
            .with_security_policy(policy);

        let composition = RhaiComposition::from_file(&config.script)?;
        let props = load_props(config)?;
        let context = NativeCompositionContext {
            width: config.width,
            height: config.height,
            fps: config.fps,
            duration_in_frames: config.duration,
        };
        let prepared = composition.prepare(&props, context)?;
        let scene = prepared.render(frame)?;
        // Stamped before rasterization reads the files.
        let dependencies = scene_dependencies(&scene)?;

        let frame_cfg = FrameConfig::new(config.width, config.height, frame, config.fps);
        let rgba = backend
            .render_frame(&scene, &frame_cfg)
            .map_err(|e| anyhow::anyhow!("Rasterize error: {e}"))?;

        // Encode to PNG in memory.
        let mut png_bytes: Vec<u8> = Vec::new();
        image::DynamicImage::ImageRgba8(rgba)
            .write_to(
                &mut std::io::Cursor::new(&mut png_bytes),
                image::ImageFormat::Png,
            )
            .map_err(|e| anyhow::anyhow!("PNG encode error: {e}"))?;
        Ok((png_bytes, dependencies))
    }
    #[cfg(not(feature = "rhai"))]
    {
        let _ = (config, frame);
        anyhow::bail!(
            "`rust-motion serve` requires the `rhai` feature. \
             Rebuild with `--features rhai`."
        )
    }
}

/// Load props from file (or return empty object).
fn load_props(config: &ServeConfig) -> anyhow::Result<serde_json::Value> {
    match &config.props {
        Some(path) => {
            let json = std::fs::read_to_string(path)?;
            Ok(serde_json::from_str(&json)?)
        }
        None => Ok(serde_json::Value::Object(Default::default())),
    }
}

// ──────────────────────────────────────────────────────────────
// File watcher
// ──────────────────────────────────────────────────────────────

fn spawn_watcher(
    tx: broadcast::Sender<()>,
    config: Arc<ServeConfig>,
    cache: Arc<Mutex<VecDeque<CachedFrame>>>,
) {
    std::thread::spawn(move || {
        info!(
            script = %config.script.display(),
            "Watching for file changes…"
        );
        let mut last = source_stamp(&config);
        loop {
            std::thread::sleep(POLL_INTERVAL);
            if poll_changes(&config, &cache, &mut last) {
                info!("Inputs changed — notifying clients to re-render their current frames");
                let _ = tx.send(());
            }
        }
    });
}

/// Whether the script, props or a referenced asset changed since the previous poll.
///
/// Polling also sees files that were replaced by a rename, which a watch on the old file misses.
/// Stale cache entries are dropped, so each change is reported once.
fn poll_changes(
    config: &ServeConfig,
    cache: &Mutex<VecDeque<CachedFrame>>,
    last: &mut SourceStamp,
) -> bool {
    let now = source_stamp(config);
    let mut changed = now != *last;
    *last = now;
    if let Ok(mut guard) = cache.lock() {
        let before = guard.len();
        guard.retain(|cached| dependencies_unchanged(&cached.dependencies));
        changed |= guard.len() != before;
    }
    changed
}

// ──────────────────────────────────────────────────────────────
// HTTP handlers
// ──────────────────────────────────────────────────────────────

/// Serve the embedded HTML player page.
async fn index_handler(State(state): State<AppState>) -> impl IntoResponse {
    Html(player_html(
        state.config.port,
        state.config.default_frame,
        state.config.width,
        state.config.height,
    ))
}

#[derive(Debug, Deserialize)]
struct WsQuery {
    frame: Option<u32>,
}

/// Upgrade an HTTP connection to a WebSocket.
async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let frame = query.frame.unwrap_or(state.config.default_frame);

    ws.on_upgrade(move |socket| handle_socket(socket, state, frame))
}

fn parse_ws_seek(value: &serde_json::Value) -> Result<Option<u32>, u64> {
    let Some(frame) = value.get("seek").and_then(serde_json::Value::as_u64) else {
        return Ok(None);
    };

    u32::try_from(frame).map(Some).map_err(|_| frame)
}

/// A render in progress for one connection, resolving to the message to send.
type PendingFrame = Pin<Box<dyn Future<Output = String> + Send>>;

/// Handle an individual WebSocket connection.
async fn handle_socket(mut socket: WebSocket, state: AppState, requested_frame: u32) {
    let mut rx = state.tx.subscribe();
    let mut current_frame = requested_frame;
    // The current frame has to be (re)rendered; the first render happens on connect.
    let mut dirty = true;
    let mut pending: Option<PendingFrame> = None;

    loop {
        if dirty && pending.is_none() {
            dirty = false;
            let state = state.clone();
            let frame = current_frame;
            pending = Some(Box::pin(async move {
                frame_message(frame, state.frame(frame).await)
            }));
        }

        tokio::select! {
            payload = async {
                match pending.as_mut() {
                    Some(render) => render.await,
                    None => std::future::pending().await,
                }
            } => {
                pending = None;
                if socket.send(Message::Text(payload)).await.is_err() {
                    break;
                }
            }
            // Refresh this client's current frame after input changes.
            event = rx.recv() => {
                match event {
                    Ok(()) => dirty = true,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "WS client lagged, refreshing its current frame");
                        dirty = true;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            // Handle incoming client messages (e.g. ping/close).
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Text(text))) => {
                        // Client may request a specific frame: {"seek": 42}
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                            match parse_ws_seek(&v) {
                                Ok(Some(frame)) => {
                                    current_frame = frame;
                                    dirty = true;
                                }
                                Err(frame) => {
                                    let payload = serde_json::json!({
                                        "type": "error",
                                        "code": "seek_out_of_range",
                                        "frame": frame,
                                        "max_frame": u32::MAX,
                                        "message": format!(
                                            "seek frame exceeds the supported maximum of {}",
                                            u32::MAX
                                        ),
                                    })
                                    .to_string();
                                    if socket.send(Message::Text(payload)).await.is_err() {
                                        break;
                                    }
                                }
                                Ok(None) => {}
                            }
                        }
                    }
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

// ──────────────────────────────────────────────────────────────
// Embedded HTML player
// ──────────────────────────────────────────────────────────────

fn player_html(port: u16, default_frame: u32, width: u32, height: u32) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1"/>
  <title>Rust Motion Studio — Live Preview</title>
  <style>
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    body {{
      background: #0f0f10;
      color: #e4e4ef;
      font-family: "Inter", system-ui, sans-serif;
      display: flex;
      flex-direction: column;
      align-items: center;
      min-height: 100vh;
      padding: 24px 16px;
      gap: 20px;
    }}
    header {{
      display: flex;
      align-items: center;
      gap: 12px;
      width: 100%;
      max-width: 900px;
    }}
    header h1 {{
      font-size: 1.1rem;
      font-weight: 600;
      letter-spacing: -0.02em;
    }}
    .badge {{
      font-size: 0.68rem;
      padding: 2px 8px;
      border-radius: 999px;
      background: #2a2a3a;
      color: #a0a0c0;
    }}
    #status {{
      font-size: 0.75rem;
      padding: 2px 10px;
      border-radius: 999px;
    }}
    .connected    {{ background: #1a3a1a; color: #4caf78; }}
    .disconnected {{ background: #3a1a1a; color: #cf6b6b; }}
    .rendering    {{ background: #2a2a10; color: #cfb84c; }}

    #canvas-wrap {{
      position: relative;
      width: 100%;
      max-width: 900px;
      border-radius: 10px;
      overflow: hidden;
      box-shadow: 0 0 0 1px #2a2a40, 0 8px 40px #000a;
      background: #111;
    }}
    #preview {{
      display: block;
      width: 100%;
      height: auto;
      image-rendering: crisp-edges;
    }}
    #overlay {{
      position: absolute;
      inset: 0;
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 0.85rem;
      color: #666;
      pointer-events: none;
    }}
    .controls {{
      width: 100%;
      max-width: 900px;
      display: flex;
      flex-direction: column;
      gap: 10px;
    }}
    .row {{
      display: flex;
      align-items: center;
      gap: 12px;
    }}
    label {{ font-size: 0.78rem; color: #8888aa; min-width: 70px; }}
    input[type=range] {{
      flex: 1;
      accent-color: #6b8ef0;
      cursor: pointer;
    }}
    #frame-num {{
      font-size: 0.78rem;
      font-variant-numeric: tabular-nums;
      min-width: 40px;
      text-align: right;
    }}
    footer {{
      font-size: 0.7rem;
      color: #444;
    }}
  </style>
</head>
<body>
  <header>
    <h1>Rust Motion Studio</h1>
    <span class="badge">Live Preview</span>
    <span id="status" class="disconnected">⬤ Disconnected</span>
  </header>

  <div id="canvas-wrap">
    <img id="preview" width="{width}" height="{height}" alt="composition preview"/>
    <div id="overlay">Connecting…</div>
  </div>

  <div class="controls">
    <div class="row">
      <label>Frame</label>
      <input type="range" id="seek" min="0" max="999" value="{default_frame}" step="1"/>
      <span id="frame-num">{default_frame}</span>
    </div>
  </div>

  <footer>Edit your .rhai script — the preview updates automatically • port {port}</footer>

  <script>
    const preview  = document.getElementById('preview');
    const overlay  = document.getElementById('overlay');
    const status   = document.getElementById('status');
    const seekEl   = document.getElementById('seek');
    const frameNum = document.getElementById('frame-num');

    let ws = null;
    let reconnectDelay = 1000;

    function setStatus(label, cls) {{
      status.textContent = '⬤ ' + label;
      status.className = cls;
    }}

    function connect() {{
      ws = new WebSocket(`ws://localhost:{port}/ws?frame=${{seekEl.value}}`);

      ws.addEventListener('open', () => {{
        setStatus('Connected', 'connected');
        overlay.textContent = '';
        reconnectDelay = 1000;
      }});

      ws.addEventListener('message', (ev) => {{
        const msg = JSON.parse(ev.data);
        if (msg.type === 'frame') {{
          preview.src = 'data:image/png;base64,' + msg.data;
          frameNum.textContent = msg.frame;
          overlay.textContent = '';
        }} else if (msg.type === 'error') {{
          overlay.textContent = msg.message;
        }}
      }});

      ws.addEventListener('close', () => {{
        setStatus('Disconnected', 'disconnected');
        overlay.textContent = 'Reconnecting…';
        setTimeout(connect, reconnectDelay);
        reconnectDelay = Math.min(reconnectDelay * 2, 8000);
      }});

      ws.addEventListener('error', () => ws.close());
    }}

    seekEl.addEventListener('input', () => {{
      const f = parseInt(seekEl.value, 10);
      frameNum.textContent = f;
      if (ws && ws.readyState === WebSocket.OPEN) {{
        ws.send(JSON.stringify({{ seek: f }}));
        setStatus('Rendering…', 'rendering');
      }}
    }});

    connect();
  </script>
</body>
</html>"#,
        port = port,
        width = width,
        height = height,
        default_frame = default_frame,
    )
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "rhai")]
    use super::{cached_frame, source_stamp};
    use super::{parse_ws_seek, serve_router, AppState, ServeConfig, CHANNEL_CAPACITY};
    use serde_json::json;
    #[cfg(feature = "rhai")]
    use std::{collections::VecDeque, fs, sync::Mutex};

    #[test]
    fn websocket_seek_accepts_normal_and_maximum_u32_values() {
        assert_eq!(parse_ws_seek(&json!({ "seek": 42 })), Ok(Some(42)));
        assert_eq!(
            parse_ws_seek(&json!({ "seek": u32::MAX })),
            Ok(Some(u32::MAX))
        );
    }

    #[test]
    fn websocket_seek_rejects_values_above_u32_max() {
        assert_eq!(
            parse_ws_seek(&json!({ "seek": u64::from(u32::MAX) + 1 })),
            Err(u64::from(u32::MAX) + 1)
        );
    }

    #[cfg(feature = "rhai")]
    #[tokio::test]
    async fn websocket_seeks_and_source_refreshes_are_connection_local() {
        use futures_util::{SinkExt, StreamExt};
        use std::{sync::Arc, time::Duration};
        use tokio_tungstenite::{connect_async, tungstenite::Message as ClientMessage};

        async fn next_json<S>(
            socket: &mut tokio_tungstenite::WebSocketStream<S>,
        ) -> serde_json::Value
        where
            S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
        {
            match tokio::time::timeout(Duration::from_secs(5), socket.next())
                .await
                .expect("timed out waiting for WebSocket frame")
                .expect("WebSocket closed")
                .expect("WebSocket receive failed")
            {
                ClientMessage::Text(text) => serde_json::from_str(&text).unwrap(),
                message => panic!("expected a text frame, got {message:?}"),
            }
        }

        let directory = std::env::temp_dir().join(format!(
            "rust_motion_serve_ws_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let script = directory.join("composition.rhai");
        let props = directory.join("props.json");
        fs::write(
            &script,
            "fn render(ctx, props) { let output = scene(); output.rect(0.0, 0.0, ctx.width.to_float(), ctx.height.to_float(), props.color); output }",
        )
        .unwrap();
        fs::write(&props, r##"{"color":"#ff0000"}"##).unwrap();

        let config = Arc::new(ServeConfig {
            script,
            props: Some(props),
            port: 0,
            default_frame: 0,
            width: 16,
            height: 16,
            fps: 30.0,
            duration: 100,
        });
        let (tx, _) = tokio::sync::broadcast::channel::<()>(CHANNEL_CAPACITY);
        let app = serve_router(AppState::new(
            tx.clone(),
            config,
            Arc::new(Mutex::new(VecDeque::new())),
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let (mut client_a, _) = connect_async(format!("ws://{address}/ws?frame=10"))
            .await
            .unwrap();
        let (mut client_b, _) = connect_async(format!("ws://{address}/ws?frame=50"))
            .await
            .unwrap();
        assert_eq!(next_json(&mut client_a).await["frame"], 10);
        assert_eq!(next_json(&mut client_b).await["frame"], 50);

        client_a
            .send(ClientMessage::Text(r#"{"seek":20}"#.into()))
            .await
            .unwrap();
        assert_eq!(next_json(&mut client_a).await["frame"], 20);
        assert!(
            tokio::time::timeout(Duration::from_millis(200), client_b.next())
                .await
                .is_err()
        );

        client_b
            .send(ClientMessage::Text(r#"{"seek":70}"#.into()))
            .await
            .unwrap();
        assert_eq!(next_json(&mut client_b).await["frame"], 70);
        assert!(
            tokio::time::timeout(Duration::from_millis(200), client_a.next())
                .await
                .is_err()
        );

        tx.send(()).unwrap();
        let (frame_a, frame_b) = tokio::join!(next_json(&mut client_a), next_json(&mut client_b));
        assert_eq!(frame_a["frame"], 20);
        assert_eq!(frame_b["frame"], 70);

        server.abort();
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(feature = "rhai")]
    #[test]
    fn changed_script_and_props_with_equal_mtimes_invalidate_cached_frame() {
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("composition.rhai");
        let props = directory.path().join("props.json");
        let config = ServeConfig {
            script: script.clone(),
            props: Some(props.clone()),
            port: 7890,
            default_frame: 0,
            width: 16,
            height: 16,
            fps: 30.0,
            duration: 1,
        };
        let cache = Mutex::new(VecDeque::new());
        let source = |version: &str| {
            format!(
                "// version {version}\nfn render(ctx, props) {{ let output = scene(); output.rect(0.0, 0.0, ctx.width.to_float(), ctx.height.to_float(), props.color); output }}"
            )
        };

        fs::write(&script, source("one")).unwrap();
        fs::write(&props, r##"{"color":"#ff0000"}"##).unwrap();
        let first_mtime = filetime::FileTime::from_unix_time(1_000, 0);
        filetime::set_file_mtime(&script, first_mtime).unwrap();
        filetime::set_file_mtime(&props, first_mtime).unwrap();
        assert_eq!(
            filetime::FileTime::from_last_modification_time(&fs::metadata(&script).unwrap()),
            first_mtime
        );
        assert_eq!(
            filetime::FileTime::from_last_modification_time(&fs::metadata(&props).unwrap()),
            first_mtime
        );
        let initial_stamp = source_stamp(&config);
        let initial_png = cached_frame(&config, 0, &cache).unwrap();

        fs::write(&script, source("two")).unwrap();
        fs::write(&props, r##"{"color":"#00ff00"}"##).unwrap();
        let second_mtime = filetime::FileTime::from_unix_time(2_000, 0);
        filetime::set_file_mtime(&script, second_mtime).unwrap();
        filetime::set_file_mtime(&props, second_mtime).unwrap();
        assert_eq!(
            filetime::FileTime::from_last_modification_time(&fs::metadata(&script).unwrap()),
            second_mtime
        );
        assert_eq!(
            filetime::FileTime::from_last_modification_time(&fs::metadata(&props).unwrap()),
            second_mtime
        );

        let updated_stamp = source_stamp(&config);
        let updated_png = cached_frame(&config, 0, &cache).unwrap();

        assert_ne!(initial_stamp, updated_stamp);
        assert_ne!(initial_png.as_ref(), updated_png.as_ref());
    }

    /// Dependency invalidation (#158) and render scheduling (#157).
    #[cfg(feature = "rhai")]
    mod invalidation_and_scheduling {
        use super::super::{
            cached_frame, poll_changes, serve_router, source_stamp, AppState, ServeConfig,
            CHANNEL_CAPACITY,
        };
        use futures_util::{SinkExt, StreamExt};
        use std::{
            collections::VecDeque,
            fs,
            net::SocketAddr,
            path::{Path, PathBuf},
            sync::{Arc, Mutex},
            time::Duration,
        };
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio_tungstenite::{
            connect_async, tungstenite::Message as ClientMessage, MaybeTlsStream, WebSocketStream,
        };

        type Client = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

        /// Takes long enough to rasterize that other work can be observed overlapping it.
        const SLOW_SCRIPT: &str = "fn render(ctx, props) { let o = scene(); \
            for i in 0..80 { o.rect(0.0, 0.0, ctx.width.to_float(), ctx.height.to_float(), \"#10203040\"); } o }";

        fn config(script: PathBuf, props: Option<PathBuf>, width: u32, height: u32) -> ServeConfig {
            ServeConfig {
                script,
                props,
                port: 0,
                default_frame: 0,
                width,
                height,
                fps: 30.0,
                duration: 100,
            }
        }

        fn write_png(path: &Path, color: [u8; 4]) {
            image::RgbaImage::from_pixel(16, 16, image::Rgba(color))
                .save_with_format(path, image::ImageFormat::Png)
                .unwrap();
        }

        /// A script drawing `asset.png`, which the props file names.
        fn image_project(directory: &Path) -> (ServeConfig, PathBuf) {
            let script = directory.join("composition.rhai");
            let props = directory.join("props.json");
            let asset = directory.join("asset.png");
            write_png(&asset, [255, 0, 0, 255]);
            fs::write(
                &script,
                "fn render(ctx, props) { let o = scene(); o.image(0.0, 0.0, 16.0, 16.0, props.path, \"cover\", 1.0); o }",
            )
            .unwrap();
            fs::write(&props, serde_json::json!({ "path": asset }).to_string()).unwrap();
            (config(script, Some(props), 16, 16), asset)
        }

        fn pixel(png: &[u8]) -> [u8; 4] {
            image::load_from_memory(png)
                .unwrap()
                .to_rgba8()
                .get_pixel(8, 8)
                .0
        }

        #[test]
        fn replaced_asset_invalidates_the_cached_frame() {
            let directory = tempfile::tempdir().unwrap();
            let (config, asset) = image_project(directory.path());
            let cache = Mutex::new(VecDeque::new());
            let mtime = filetime::FileTime::from_unix_time(1_000, 0);
            filetime::set_file_mtime(&asset, mtime).unwrap();

            let first = cached_frame(&config, 0, &cache).unwrap();
            assert_eq!(pixel(&first), [255, 0, 0, 255]);
            // Unchanged inputs are served from the cache.
            let again = cached_frame(&config, 0, &cache).unwrap();
            assert!(Arc::ptr_eq(&first, &again));

            // Rewritten in place.
            write_png(&asset, [0, 255, 0, 255]);
            let rewritten = cached_frame(&config, 0, &cache).unwrap();
            assert_eq!(pixel(&rewritten), [0, 255, 0, 255]);

            // Atomically replaced by a file with the same modification time and length.
            filetime::set_file_mtime(&asset, mtime).unwrap();
            let before = cached_frame(&config, 0, &cache).unwrap();
            let replacement = directory.path().join("replacement.png");
            write_png(&replacement, [0, 0, 255, 255]);
            assert_eq!(
                fs::metadata(&replacement).unwrap().len(),
                fs::metadata(&asset).unwrap().len()
            );
            filetime::set_file_mtime(&replacement, mtime).unwrap();
            fs::rename(&replacement, &asset).unwrap();
            let after = cached_frame(&config, 0, &cache).unwrap();
            assert_eq!(pixel(&before), [0, 255, 0, 255]);
            #[cfg(unix)]
            assert_eq!(pixel(&after), [0, 0, 255, 255]);
            #[cfg(not(unix))]
            let _ = after;
        }

        #[test]
        fn polling_reports_each_asset_change_once() {
            let directory = tempfile::tempdir().unwrap();
            let (config, asset) = image_project(directory.path());
            let cache = Mutex::new(VecDeque::new());
            let mut last = source_stamp(&config);
            cached_frame(&config, 0, &cache).unwrap();
            assert!(!poll_changes(&config, &cache, &mut last));

            // A different size, so the change is visible whatever the timestamp resolution.
            image::RgbaImage::from_fn(64, 64, |x, y| image::Rgba([x as u8, y as u8, 0, 255]))
                .save_with_format(&asset, image::ImageFormat::Png)
                .unwrap();
            assert!(poll_changes(&config, &cache, &mut last));
            assert!(!poll_changes(&config, &cache, &mut last));

            fs::remove_file(&asset).unwrap();
            assert!(
                !poll_changes(&config, &cache, &mut last),
                "nothing cached depends on it"
            );
        }

        async fn serve(config: ServeConfig) -> (SocketAddr, tokio::task::JoinHandle<()>) {
            let (tx, _) = tokio::sync::broadcast::channel::<()>(CHANNEL_CAPACITY);
            let app = serve_router(AppState::new(
                tx,
                Arc::new(config),
                Arc::new(Mutex::new(VecDeque::new())),
            ));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            (address, server)
        }

        async fn next_json(client: &mut Client) -> serde_json::Value {
            match tokio::time::timeout(Duration::from_secs(60), client.next())
                .await
                .expect("timed out waiting for WebSocket frame")
                .expect("WebSocket closed")
                .expect("WebSocket receive failed")
            {
                ClientMessage::Text(text) => serde_json::from_str(&text).unwrap(),
                message => panic!("expected a text frame, got {message:?}"),
            }
        }

        async fn health(address: SocketAddr) -> String {
            let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
            stream
                .write_all(b"GET /health HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).await.unwrap();
            response
        }

        // The default test runtime has a single thread, so rendering on it would stall /health.
        #[tokio::test]
        async fn a_slow_websocket_render_does_not_stall_other_requests() {
            let directory = tempfile::tempdir().unwrap();
            let script = directory.path().join("slow.rhai");
            fs::write(&script, SLOW_SCRIPT).unwrap();
            let (address, server) = serve(config(script, None, 640, 360)).await;

            let (mut client, _) = connect_async(format!("ws://{address}/ws?frame=0"))
                .await
                .unwrap();
            tokio::select! {
                biased;
                message = next_json(&mut client) => panic!("the slow frame finished first: {}", message["type"]),
                response = health(address) => assert!(response.starts_with("HTTP/1.1 200"), "{response}"),
            }
            assert_eq!(next_json(&mut client).await["frame"], 0);
            server.abort();
        }

        #[tokio::test]
        async fn a_burst_of_seeks_renders_only_the_latest() {
            let directory = tempfile::tempdir().unwrap();
            let script = directory.path().join("slow.rhai");
            fs::write(&script, SLOW_SCRIPT).unwrap();
            let (address, server) = serve(config(script, None, 640, 360)).await;

            let (mut client, _) = connect_async(format!("ws://{address}/ws?frame=0"))
                .await
                .unwrap();
            for frame in 1..=20 {
                client
                    .send(ClientMessage::Text(format!("{{\"seek\":{frame}}}")))
                    .await
                    .unwrap();
            }
            let mut received = Vec::new();
            while received.last() != Some(&20) {
                received.push(next_json(&mut client).await["frame"].as_u64().unwrap());
            }
            // The frame in flight when the burst arrived, then the latest one.
            assert!(
                received.len() <= 3,
                "superseded seeks were rendered: {received:?}"
            );
            server.abort();
        }

        #[tokio::test]
        async fn render_failures_are_reported_and_the_connection_survives() {
            let directory = tempfile::tempdir().unwrap();
            let script = directory.path().join("failing.rhai");
            fs::write(
                &script,
                "fn render(ctx, props) { if ctx.frame > 0 { throw \"boom\"; } scene() }",
            )
            .unwrap();
            let (address, server) = serve(config(script, None, 16, 16)).await;

            let (mut client, _) = connect_async(format!("ws://{address}/ws?frame=0"))
                .await
                .unwrap();
            assert_eq!(next_json(&mut client).await["type"], "frame");
            client
                .send(ClientMessage::Text(r#"{"seek":3}"#.into()))
                .await
                .unwrap();
            let error = next_json(&mut client).await;
            assert_eq!(error["type"], "error");
            assert_eq!(error["code"], "render_failed");
            assert_eq!(error["frame"], 3);
            assert!(
                error["message"].as_str().unwrap().contains("boom"),
                "{error}"
            );
            client
                .send(ClientMessage::Text(r#"{"seek":0}"#.into()))
                .await
                .unwrap();
            assert_eq!(next_json(&mut client).await["type"], "frame");
            server.abort();
        }
    }
}
