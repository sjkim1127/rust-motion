//! `dioxuscut serve` — embedded hot-reloading web studio with WebSocket frame preview.
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
//!                        │                        │
//!                   ┌────┴──────┐         ┌───────▼──────────┐
//!                   │  notify   │         │ RhaiComposition  │
//!                   │  watcher  │         │ render frame N   │
//!                   └───────────┘         └──────────────────┘
//!                   *.rhai / *_props.json
//! ```
//!
//! WebSocket message format (server → client):
//! ```json
//! { "type": "frame", "data": "<base64-png>", "frame": 0 }
//! ```

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
use dioxuscut_rasterizer::{MediaSecurityPolicy, TinySkiaBackend};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use std::{
    collections::VecDeque,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

/// Maximum number of frame messages buffered in the broadcast channel.
const CHANNEL_CAPACITY: usize = 4;
/// Maximum number of rendered preview frames retained for scrubbing.
const PREVIEW_CACHE_CAPACITY: usize = 8;
/// Debounce period before re-rendering after a file change.
const DEBOUNCE_MS: u64 = 150;

// ──────────────────────────────────────────────────────────────
// Public configuration type
// ──────────────────────────────────────────────────────────────

/// Configuration for the `dioxuscut serve` subcommand.
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
}

#[derive(Clone)]
struct CachedFrame {
    frame: u32,
    source_stamp: u128,
    png: Arc<Vec<u8>>,
}

// ──────────────────────────────────────────────────────────────
// Entry point
// ──────────────────────────────────────────────────────────────

/// Run the serve loop.  Never returns under normal operation.
pub async fn run(config: ServeConfig) -> anyhow::Result<()> {
    let config = Arc::new(config);
    let frame_cache = Arc::new(Mutex::new(VecDeque::new()));
    let (tx, _) = broadcast::channel::<()>(CHANNEL_CAPACITY);

    // Render the initial frame immediately so the page is never blank.
    if let Err(error) = cached_frame(&config, config.default_frame, &frame_cache) {
        error!(frame = config.default_frame, error = %error, "Initial frame render failed");
    }

    // Spawn the file-watcher task.
    spawn_watcher(tx.clone(), config.clone());

    let state = AppState {
        tx,
        config: config.clone(),
        frame_cache,
    };

    let app = serve_router(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    info!(
        port = config.port,
        script = %config.script.display(),
        "dioxuscut serve — open http://localhost:{} in your browser",
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
        "service": "dioxuscut-serve",
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
    let config = state.config.clone();
    let frame_cache = state.frame_cache.clone();
    let result =
        tokio::task::spawn_blocking(move || cached_frame(&config, frame, &frame_cache)).await;
    match result {
        Ok(Ok(png)) => Json(serde_json::json!({
            "type": "frame",
            "frame": frame,
            "width": state.config.width,
            "height": state.config.height,
            "png_base64": BASE64.encode(&*png),
        }))
        .into_response(),
        Ok(Err(error)) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "type": "error",
                "frame": frame,
                "message": error.to_string(),
            })),
        )
            .into_response(),
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "type": "error",
                "frame": frame,
                "message": format!("frame render task failed: {error}"),
            })),
        )
            .into_response(),
    }
}

// ──────────────────────────────────────────────────────────────
// Render helper
// ──────────────────────────────────────────────────────────────

/// Render a frame and send it only to the WebSocket connection requesting it.
async fn render_and_send(
    socket: &mut WebSocket,
    config: &ServeConfig,
    frame: u32,
    cache: &Mutex<VecDeque<CachedFrame>>,
) -> anyhow::Result<()> {
    match cached_frame(config, frame, cache) {
        Ok(png_bytes) => {
            let png_b64 = BASE64.encode(&*png_bytes);
            let payload = serde_json::json!({
                "type": "frame",
                "data": png_b64,
                "frame": frame,
            })
            .to_string();
            socket.send(Message::Text(payload)).await?;
        }
        Err(e) => {
            error!(frame, error = %e, "Render failed");
        }
    }
    Ok(())
}

fn source_stamp(config: &ServeConfig) -> u128 {
    let stamp = |path: &PathBuf| {
        std::fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_nanos())
    };
    stamp(&config.script) ^ config.props.as_ref().map_or(0, stamp)
}

fn cached_frame(
    config: &ServeConfig,
    frame: u32,
    cache: &Mutex<VecDeque<CachedFrame>>,
) -> anyhow::Result<Arc<Vec<u8>>> {
    let stamp = source_stamp(config);
    if let Ok(mut guard) = cache.lock() {
        if let Some(index) = guard
            .iter()
            .position(|cached| cached.frame == frame && cached.source_stamp == stamp)
        {
            let cached = guard.remove(index).expect("cache index was found");
            let png = cached.png.clone();
            guard.push_front(cached);
            return Ok(png);
        }
    }
    let png = Arc::new(render_frame(config, frame)?);
    if let Ok(mut guard) = cache.lock() {
        guard.push_front(CachedFrame {
            frame,
            source_stamp: stamp,
            png: png.clone(),
        });
        guard.truncate(PREVIEW_CACHE_CAPACITY);
    }
    Ok(png)
}

/// Render a single composition frame → raw PNG bytes.
fn render_frame(config: &ServeConfig, frame: u32) -> anyhow::Result<Vec<u8>> {
    #[cfg(feature = "rhai")]
    {
        use crate::rhai_runtime::RhaiComposition;
        use dioxuscut_composition::{Composition, NativeCompositionContext};
        use dioxuscut_rasterizer::{FrameConfig, RasterizerBackend};

        let script_dir = config
            .script
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .unwrap_or_else(|| config.script.clone());

        let policy = MediaSecurityPolicy::sandboxed(vec![script_dir]);
        let cache_bytes = std::env::var("DIOXUSCUT_IMAGE_CACHE_BYTES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(dioxuscut_rasterizer::DEFAULT_IMAGE_CACHE_BYTES);
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
        Ok(png_bytes)
    }
    #[cfg(not(feature = "rhai"))]
    {
        let _ = (config, frame);
        anyhow::bail!(
            "`dioxuscut serve` requires the `rhai` feature. \
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

fn spawn_watcher(tx: broadcast::Sender<()>, config: Arc<ServeConfig>) {
    std::thread::spawn(move || {
        if let Err(e) = watch_loop(tx, config) {
            error!(error = %e, "File watcher terminated with error");
        }
    });
}

fn watch_loop(tx: broadcast::Sender<()>, config: Arc<ServeConfig>) -> anyhow::Result<()> {
    let (notify_tx, notify_rx) = std::sync::mpsc::channel::<notify::Result<Event>>();

    let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |res| {
        let _ = notify_tx.send(res);
    })?;

    // Watch the script file.
    watcher.watch(&config.script, RecursiveMode::NonRecursive)?;

    // Watch the props file if present.
    if let Some(props) = &config.props {
        if props.exists() {
            watcher.watch(props, RecursiveMode::NonRecursive)?;
        }
    }

    info!(
        script = %config.script.display(),
        "Watching for file changes…"
    );

    let debounce = Duration::from_millis(DEBOUNCE_MS);

    loop {
        // Block until the first event.
        match notify_rx.recv() {
            Err(_) => break, // sender dropped, exit watcher
            Ok(Err(e)) => {
                warn!(error = %e, "Watcher error");
                continue;
            }
            Ok(Ok(event)) => {
                if !is_relevant(&event) {
                    continue;
                }
            }
        }

        // Drain rapid successive events (debounce).
        while notify_rx.recv_timeout(debounce).is_ok() {}

        info!("Source changed — notifying clients to re-render their current frames");
        let _ = tx.send(());
    }

    Ok(())
}

/// Returns true for write / create / remove events (ignore metadata-only).
fn is_relevant(event: &Event) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
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
    let config = state.config.clone();
    let tx = state.tx.clone();
    let frame_cache = state.frame_cache.clone();

    ws.on_upgrade(move |socket| handle_socket(socket, tx, config, frame, frame_cache))
}

fn parse_ws_seek(value: &serde_json::Value) -> Result<Option<u32>, u64> {
    let Some(frame) = value.get("seek").and_then(serde_json::Value::as_u64) else {
        return Ok(None);
    };

    u32::try_from(frame).map(Some).map_err(|_| frame)
}

/// Handle an individual WebSocket connection.
async fn handle_socket(
    mut socket: WebSocket,
    tx: broadcast::Sender<()>,
    config: Arc<ServeConfig>,
    requested_frame: u32,
    frame_cache: Arc<Mutex<VecDeque<CachedFrame>>>,
) {
    let mut rx = tx.subscribe();
    let mut current_frame = requested_frame;

    // Send the current frame immediately on connect (render synchronously).
    if render_and_send(&mut socket, &config, current_frame, &frame_cache)
        .await
        .is_err()
    {
        return;
    }

    // Refresh this client's current frame after source changes.
    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Ok(()) => {
                        if render_and_send(&mut socket, &config, current_frame, &frame_cache).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "WS client lagged, refreshing its current frame");
                        if render_and_send(&mut socket, &config, current_frame, &frame_cache).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            // Handle incoming client messages (e.g. ping/close).
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Text(text))) => {
                        // Client may request a specific frame: {"seek": 42}
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                            match parse_ws_seek(&v) {
                                Ok(Some(frame)) => {
                                    current_frame = frame;
                                    if render_and_send(&mut socket, &config, current_frame, &frame_cache).await.is_err() {
                                        break;
                                    }
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
                    Some(Ok(_)) | Some(Err(_)) => {}
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
  <title>Dioxuscut Studio — Live Preview</title>
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
    <h1>🎬 Dioxuscut Studio</h1>
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
    use super::{parse_ws_seek, serve_router, AppState, ServeConfig, CHANNEL_CAPACITY};
    use serde_json::json;

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
        use std::{
            collections::VecDeque,
            fs,
            sync::{Arc, Mutex},
            time::Duration,
        };
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
        let app = serve_router(AppState {
            tx: tx.clone(),
            config,
            frame_cache: Arc::new(Mutex::new(VecDeque::new())),
        });
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
}
