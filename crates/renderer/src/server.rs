//! Automated Web Server Lifecycle for Dioxuscut.
//!
//! Provides spawning, health checking, dynamic port allocation,
//! and clean termination for Dioxus web app rendering.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{routing::get, Router};
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::{debug, error, info};

/// Errors that can occur during web server lifecycle management.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to bind server to address {0}: {1}")]
    BindError(String, std::io::Error),

    #[error("Server health check timed out for {0} after {1:?}")]
    HealthCheckTimeout(String, Duration),

    #[error("Reqwest error during health check: {0}")]
    ReqwestError(#[from] reqwest::Error),

    #[error("Child process exited prematurely with code: {0:?}")]
    ProcessExited(Option<i32>),

    #[error("Server already stopped")]
    AlreadyStopped,
}

/// Mode of operation for the web server.
#[derive(Debug, Clone)]
pub enum ServeMode {
    /// Embedded static HTTP server serving a directory.
    Static { root_dir: PathBuf },
    /// External child process command (e.g. `dx serve`).
    Command {
        cmd: String,
        args: Vec<String>,
        cwd: Option<PathBuf>,
    },
}

/// Configuration for launching the web server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Server mode (static folder or external child process).
    pub mode: ServeMode,
    /// Requested port (0 for dynamic port selection).
    pub port: u16,
    /// Host interface to bind (default: "127.0.0.1").
    pub host: String,
    /// Health check timeout.
    pub health_check_timeout: Duration,
    /// Health check retry interval.
    pub health_check_interval: Duration,
}

impl ServerConfig {
    /// Create static server config for a root directory on a dynamic port (0).
    pub fn static_dir(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            mode: ServeMode::Static {
                root_dir: root_dir.into(),
            },
            port: 0,
            host: "127.0.0.1".to_string(),
            health_check_timeout: Duration::from_secs(10),
            health_check_interval: Duration::from_millis(50),
        }
    }

    /// Create command server config for running `dx serve` or custom CLI.
    pub fn command(cmd: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            mode: ServeMode::Command {
                cmd: cmd.into(),
                args,
                cwd: None,
            },
            port: 0,
            host: "127.0.0.1".to_string(),
            health_check_timeout: Duration::from_secs(15),
            health_check_interval: Duration::from_millis(100),
        }
    }

    /// Set the port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the working directory for command mode.
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        if let ServeMode::Command {
            cwd: ref mut slot, ..
        } = self.mode
        {
            *slot = Some(cwd.into());
        }

        self
    }

    /// Set the health check timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.health_check_timeout = timeout;
        self
    }
}

/// Handle to a running server instance, ensuring clean termination on Drop or explicit `.stop()`.
pub struct ServerHandle {
    port: u16,
    url: String,
    shutdown_tx: Option<oneshot::Sender<()>>,
    child_process: Option<Arc<Mutex<tokio::process::Child>>>,
    stopped: bool,
}

impl ServerHandle {
    /// Returns the port the server is listening on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Returns the full URL of the server (e.g. "http://127.0.0.1:51234").
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Explicitly stop the server and wait for clean termination.
    pub async fn stop(mut self) -> Result<(), ServerError> {
        self.perform_stop().await
    }

    async fn perform_stop(&mut self) -> Result<(), ServerError> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;

        info!("Stopping web server at {}", self.url);

        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }

        if let Some(child_arc) = self.child_process.take() {
            let mut child = child_arc.lock().await;
            match child.kill().await {
                Ok(_) => {
                    let _ = child.wait().await;
                    debug!("Child process killed successfully");
                }
                Err(e) => {
                    debug!("Child process already exited or failed to kill: {}", e);
                }
            }
        }

        Ok(())
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        if !self.stopped {
            if let Some(tx) = self.shutdown_tx.take() {
                let _ = tx.send(());
            }
            if let Some(child_arc) = self.child_process.take() {
                if let Ok(mut child) = child_arc.try_lock() {
                    let _ = child.start_kill();
                }
            }
            self.stopped = true;
        }
    }
}

/// Helper function to find an available local port.
pub fn find_available_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    Ok(port)
}

/// Spawn an embedded static web server serving `root_dir` on `port` (or dynamic port if 0).
pub async fn spawn_server(
    port: u16,
    root_dir: impl AsRef<Path>,
) -> Result<ServerHandle, ServerError> {
    let config = ServerConfig::static_dir(root_dir.as_ref().to_path_buf()).with_port(port);
    spawn_server_with_config(config).await
}

/// Spawn a web server with full configuration and perform health check readiness polling.
pub async fn spawn_server_with_config(config: ServerConfig) -> Result<ServerHandle, ServerError> {
    match config.mode {
        ServeMode::Static { ref root_dir } => spawn_static_server(&config, root_dir).await,
        ServeMode::Command {
            ref cmd,
            ref args,
            ref cwd,
        } => spawn_command_server(&config, cmd, args, cwd.as_deref()).await,
    }
}

async fn spawn_static_server(
    config: &ServerConfig,
    root_dir: &Path,
) -> Result<ServerHandle, ServerError> {
    // Ensure root directory exists or create it
    if !root_dir.exists() {
        std::fs::create_dir_all(root_dir)?;
    }

    let bind_addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&bind_addr)
        .await
        .map_err(|e| ServerError::BindError(bind_addr.clone(), e))?;

    let bound_addr: SocketAddr = listener.local_addr()?;
    let actual_port = bound_addr.port();
    let url = format!("http://{}:{}", config.host, actual_port);

    info!(
        "Starting static web server for {} at {}",
        root_dir.display(),
        url
    );

    let serve_dir = ServeDir::new(root_dir).append_index_html_on_directories(true);
    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .nest_service("/", serve_dir)
        .layer(CorsLayer::permissive());

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
        {
            error!("Static web server error: {}", err);
        }
    });

    // Poll health check until ready
    poll_health_check(
        &url,
        config.health_check_timeout,
        config.health_check_interval,
    )
    .await?;

    Ok(ServerHandle {
        port: actual_port,
        url,
        shutdown_tx: Some(shutdown_tx),
        child_process: None,
        stopped: false,
    })
}

async fn spawn_command_server(
    config: &ServerConfig,
    cmd: &str,
    args: &[String],
    cwd: Option<&Path>,
) -> Result<ServerHandle, ServerError> {
    let port = if config.port == 0 {
        find_available_port()?
    } else {
        config.port
    };

    let url = format!("http://{}:{}", config.host, port);

    info!(
        "Spawning child process server: {} {} at {}",
        cmd,
        args.join(" "),
        url
    );

    let mut command = tokio::process::Command::new(cmd);
    command.args(args);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    command.env("PORT", port.to_string());

    let child = command.spawn()?;
    let child_arc = Arc::new(Mutex::new(child));

    // Poll health check
    let health_res = poll_health_check(
        &url,
        config.health_check_timeout,
        config.health_check_interval,
    )
    .await;

    if let Err(e) = health_res {
        let mut c = child_arc.lock().await;
        let _ = c.kill().await;
        return Err(e);
    }

    Ok(ServerHandle {
        port,
        url,
        shutdown_tx: None,
        child_process: Some(child_arc),
        stopped: false,
    })
}

/// Poll server URL until responding or timeout.
async fn poll_health_check(
    url: &str,
    timeout: Duration,
    interval: Duration,
) -> Result<(), ServerError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;

    let health_url = if url.ends_with('/') {
        format!("{}health", url)
    } else {
        format!("{}/health", url)
    };

    let start = Instant::now();

    while start.elapsed() < timeout {
        if let Ok(resp) = client.get(&health_url).send().await {
            if resp.status().is_success() {
                debug!("Health check succeeded at {}", health_url);
                return Ok(());
            }
        }

        if let Ok(resp) = client.get(url).send().await {
            if resp.status().is_success() {
                debug!("Health check succeeded at {}", url);
                return Ok(());
            }
        }

        tokio::time::sleep(interval).await;
    }

    Err(ServerError::HealthCheckTimeout(url.to_string(), timeout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_temp_test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dioxuscut_server_test_{}_{}",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    async fn spawn_status_server(
        status: reqwest::StatusCode,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().fallback(move || async move { status });
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}"), server)
    }

    #[tokio::test]
    async fn health_check_requires_a_success_status_from_health_or_root() {
        for status in [reqwest::StatusCode::OK, reqwest::StatusCode::NO_CONTENT] {
            let (url, server) = spawn_status_server(status).await;
            let result =
                poll_health_check(&url, Duration::from_millis(500), Duration::from_millis(5)).await;
            assert!(result.is_ok(), "expected HTTP {status} to pass readiness");
            server.abort();
        }

        for status in [
            reqwest::StatusCode::NOT_FOUND,
            reqwest::StatusCode::UNAUTHORIZED,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        ] {
            let (url, server) = spawn_status_server(status).await;
            let result =
                poll_health_check(&url, Duration::from_millis(50), Duration::from_millis(5)).await;
            assert!(
                matches!(result, Err(ServerError::HealthCheckTimeout(_, _))),
                "expected HTTP {status} to fail readiness"
            );
            server.abort();
        }
    }

    #[tokio::test]
    async fn test_spawn_static_server_dynamic_port() {
        let temp_dir = create_temp_test_dir("dyn_port");
        let index_path = temp_dir.join("index.html");
        fs::write(&index_path, "<h1>Test Dioxus App</h1>").unwrap();

        let handle = spawn_server(0, &temp_dir)
            .await
            .expect("Failed to spawn server");

        assert!(handle.port() > 0);
        assert!(handle.url().starts_with("http://127.0.0.1:"));

        // Health check endpoint
        let health_res = reqwest::get(format!("{}/health", handle.url()))
            .await
            .unwrap();
        assert_eq!(health_res.status(), 200);
        let health_text = health_res.text().await.unwrap();
        assert_eq!(health_text, "OK");

        // Static file serving
        let file_res = reqwest::get(handle.url()).await.unwrap();
        assert_eq!(file_res.status(), 200);
        let file_text = file_res.text().await.unwrap();
        assert_eq!(file_text, "<h1>Test Dioxus App</h1>");

        // Clean stop
        handle.stop().await.expect("Failed to stop server");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_server_drop_cleanup() {
        let temp_dir = create_temp_test_dir("drop_cleanup");
        let url;
        {
            let handle = spawn_server(0, &temp_dir).await.unwrap();
            url = handle.url().to_string();
            let res = reqwest::get(&url).await;
            assert!(res.is_ok());
        }
        // Handle dropped here
        tokio::time::sleep(Duration::from_millis(150)).await;
        let res = reqwest::get(&url).await;
        assert!(res.is_err());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_server_explicit_port() {
        let temp_dir = create_temp_test_dir("explicit_port");
        let available_port = find_available_port().expect("Failed to find port");

        let handle = spawn_server(available_port, &temp_dir)
            .await
            .expect("Failed to spawn server");
        assert_eq!(handle.port(), available_port);
        assert_eq!(handle.url(), format!("http://127.0.0.1:{}", available_port));

        let res = reqwest::get(format!("{}/health", handle.url()))
            .await
            .unwrap();
        assert_eq!(res.status(), 200);

        handle.stop().await.unwrap();
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_server_config_builder() {
        let temp_dir = create_temp_test_dir("config_builder");
        let config = ServerConfig::static_dir(&temp_dir)
            .with_port(0)
            .with_timeout(Duration::from_secs(5));

        let handle = spawn_server_with_config(config).await.unwrap();
        assert!(handle.port() > 0);
        handle.stop().await.unwrap();
        let _ = fs::remove_dir_all(&temp_dir);

        let cmd_config =
            ServerConfig::command("echo", vec!["hello".to_string()]).with_cwd(&temp_dir);
        if let ServeMode::Command {
            ref cmd, ref cwd, ..
        } = cmd_config.mode
        {
            assert_eq!(cmd, "echo");
            assert_eq!(cwd.as_ref().unwrap(), &temp_dir);
        }
    }
}
