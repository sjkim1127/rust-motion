//! Host-neutral project and render-job contracts.
//!
//! Dioxus, Tauri, CLI, and Python integrations should exchange these models
//! instead of depending on one another's UI state.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Read;
#[cfg(not(target_arch = "wasm32"))]
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub version: u32,
    pub composition: String,
    pub settings: ProjectSettings,
    #[serde(default = "default_props")]
    pub props: serde_json::Value,
    #[serde(default)]
    pub assets: Vec<AssetRef>,
    #[serde(default)]
    pub tracks: Vec<Track>,
    /// Named frame markers used by composition timing APIs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<TimeEvent>,
    /// Optional project audio asset to audition while retiming named events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_over_asset_id: Option<String>,
}

fn default_props() -> serde_json::Value {
    serde_json::json!({})
}

fn default_frame_step() -> u32 {
    1
}

fn default_scale() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectSettings {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: u32,
    /// Output scale applied after logical composition rendering.
    #[serde(default = "default_scale")]
    pub scale: f64,
    /// Optional FFmpeg quality value; hosts use their default when omitted.
    #[serde(default)]
    pub crf: Option<u32>,
    /// Optional FFmpeg encoder preset.
    #[serde(default)]
    pub preset: Option<String>,
    /// Optional render worker count. `None` lets the host choose automatically.
    #[serde(default)]
    pub concurrency: Option<u32>,
    #[serde(default = "default_frame_step")]
    pub frame_step: u32,
    /// Optional inclusive frame range for automation and partial renders.
    #[serde(default)]
    pub frame_start: Option<u32>,
    #[serde(default)]
    pub frame_end: Option<u32>,
    #[serde(default)]
    pub backend: BackendKind,
    /// Browser worker screenshot format (`png` or `jpeg`).
    #[serde(default)]
    pub browser_image_format: Option<String>,
    /// JPEG quality used by the browser worker.
    #[serde(default)]
    pub browser_jpeg_quality: Option<u8>,
    /// Browser worker response timeout in milliseconds.
    #[serde(default)]
    pub browser_frame_timeout_ms: Option<u64>,
    /// Browser frame transport (`base64` by default or `file`).
    #[serde(default)]
    pub browser_transport: Option<String>,
    /// Number of transport retries for browser worker failures.
    #[serde(default)]
    pub browser_transport_retries: Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    #[default]
    Native,
    Browser,
    Gpu,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AssetRef {
    pub id: String,
    pub path: String,
    #[serde(default)]
    pub kind: AssetKind,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    #[default]
    Other,
    Image,
    Video,
    Audio,
    Font,
    Lottie,
    Model,
    Shader,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub id: String,
    #[serde(default)]
    pub clips: Vec<Clip>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub id: String,
    pub composition: String,
    pub start: u32,
    pub duration: u32,
    #[serde(default)]
    pub props: serde_json::Value,
}

/// A stable named point on the project frame timeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TimeEvent {
    pub id: String,
    pub frame: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Preparing,
    Rendering,
    Encoding,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderJob {
    pub id: String,
    pub project: Project,
    pub status: JobStatus,
    pub completed_frames: u32,
    /// Frames accepted by the media encoder. During rendering this may lag
    /// behind `completed_frames` when frame rendering is concurrent.
    #[serde(default)]
    pub encoded_frames: u32,
    pub error: Option<String>,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub gpu_frames: Option<u64>,
    #[serde(default)]
    pub cpu_fallback_frames: Option<u64>,
    #[serde(default)]
    pub gpu_fallback_reason: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProjectError {
    #[error("project composition cannot be empty")]
    EmptyComposition,
    #[error("project dimensions and duration must be greater than zero")]
    InvalidDimensions,
    #[error("project fps must be finite and greater than zero")]
    InvalidFps,
    #[error("track '{track}' contains invalid clip '{clip}': {reason}")]
    InvalidClip {
        track: String,
        clip: String,
        reason: String,
    },
    #[error("invalid project frame range: {start}..={end} for duration {duration}")]
    InvalidFrameRange { start: u32, end: u32, duration: u32 },
    #[error("project frame step must be greater than zero")]
    InvalidFrameStep,
    #[error("project concurrency must be greater than zero")]
    InvalidConcurrency,
    #[error("browser image format must be png or jpeg")]
    InvalidBrowserImageFormat,
    #[error("browser JPEG quality must be between 1 and 100")]
    InvalidBrowserJpegQuality,
    #[error("browser frame timeout must be greater than zero")]
    InvalidBrowserFrameTimeout,
    #[error("browser transport must be base64 or file")]
    InvalidBrowserTransport,
    #[error("project asset id cannot be empty")]
    EmptyAssetId,
    #[error("project asset path cannot be empty for '{0}'")]
    EmptyAssetPath(String),
    #[error("project asset id '{0}' is duplicated")]
    DuplicateAssetId(String),
    #[error("project references unknown asset '{0}'")]
    UnknownAssetReference(String),
    #[error("time event name cannot be empty")]
    EmptyTimeEventId,
    #[error("time event '{0}' is duplicated")]
    DuplicateTimeEvent(String),
    #[error("time event '{event}' is at frame {frame}, outside project duration {duration}")]
    InvalidTimeEventFrame {
        event: String,
        frame: u32,
        duration: u32,
    },
    #[error("voice-over asset '{0}' must reference an audio asset in the project")]
    InvalidVoiceOverAsset(String),
    #[error("project asset '{asset}' could not be read: {reason}")]
    AssetRead { asset: String, reason: String },
    #[error("project asset '{0}' resolves outside the project directory")]
    AssetOutsideProject(String),
    #[error("project asset '{asset}' SHA-256 mismatch: expected {expected}, got {actual}")]
    AssetHashMismatch {
        asset: String,
        expected: String,
        actual: String,
    },
    #[error("project asset '{0}' has an invalid SHA-256 digest")]
    InvalidAssetHash(String),
    #[error("project scale must be finite and greater than zero")]
    InvalidScale,
    #[error("invalid render job transition from {from:?} to {to:?}")]
    InvalidJobTransition { from: JobStatus, to: JobStatus },
    #[error("render job '{0}' was not found")]
    JobNotFound(String),
    #[error("render progress cannot regress from {previous} to {next}")]
    ProgressRegressed { previous: u32, next: u32 },
    #[error("unsupported project schema version {0}")]
    UnsupportedVersion(u32),
    #[error("project file error: {0}")]
    File(String),
    #[error("project JSON error: {0}")]
    Json(String),
}

impl Project {
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.version != 1 {
            return Err(ProjectError::UnsupportedVersion(self.version));
        }
        if self.composition.trim().is_empty() {
            return Err(ProjectError::EmptyComposition);
        }
        if self.settings.width == 0 || self.settings.height == 0 || self.settings.duration == 0 {
            return Err(ProjectError::InvalidDimensions);
        }
        if !self.settings.fps.is_finite() || self.settings.fps <= 0.0 {
            return Err(ProjectError::InvalidFps);
        }
        if !self.settings.scale.is_finite() || self.settings.scale <= 0.0 {
            return Err(ProjectError::InvalidScale);
        }
        if self.settings.frame_step == 0 {
            return Err(ProjectError::InvalidFrameStep);
        }
        if self.settings.concurrency == Some(0) {
            return Err(ProjectError::InvalidConcurrency);
        }
        if let Some(format) = &self.settings.browser_image_format {
            let format = format.trim().to_ascii_lowercase();
            if format != "png" && format != "jpeg" {
                return Err(ProjectError::InvalidBrowserImageFormat);
            }
        }
        if self
            .settings
            .browser_jpeg_quality
            .is_some_and(|quality| !(1..=100).contains(&quality))
        {
            return Err(ProjectError::InvalidBrowserJpegQuality);
        }
        if self
            .settings
            .browser_frame_timeout_ms
            .is_some_and(|timeout| timeout == 0)
        {
            return Err(ProjectError::InvalidBrowserFrameTimeout);
        }
        if self
            .settings
            .browser_transport
            .as_deref()
            .is_some_and(|transport| {
                !matches!(
                    transport.trim().to_ascii_lowercase().as_str(),
                    "base64" | "file"
                )
            })
        {
            return Err(ProjectError::InvalidBrowserTransport);
        }
        let start = self.settings.frame_start.unwrap_or(0);
        let end = self
            .settings
            .frame_end
            .unwrap_or_else(|| self.settings.duration.saturating_sub(1));
        if start > end || end >= self.settings.duration {
            return Err(ProjectError::InvalidFrameRange {
                start,
                end,
                duration: self.settings.duration,
            });
        }
        let mut asset_ids = BTreeSet::new();
        for asset in &self.assets {
            if asset.id.trim().is_empty() {
                return Err(ProjectError::EmptyAssetId);
            }
            if asset.path.trim().is_empty() {
                return Err(ProjectError::EmptyAssetPath(asset.id.clone()));
            }
            if let Some(hash) = &asset.sha256 {
                if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(ProjectError::InvalidAssetHash(asset.id.clone()));
                }
            }
            if !asset_ids.insert(asset.id.as_str()) {
                return Err(ProjectError::DuplicateAssetId(asset.id.clone()));
            }
        }
        let mut event_ids = BTreeSet::new();
        for event in &self.events {
            if event.id.trim().is_empty() {
                return Err(ProjectError::EmptyTimeEventId);
            }
            if !event_ids.insert(event.id.as_str()) {
                return Err(ProjectError::DuplicateTimeEvent(event.id.clone()));
            }
            if event.frame >= self.settings.duration {
                return Err(ProjectError::InvalidTimeEventFrame {
                    event: event.id.clone(),
                    frame: event.frame,
                    duration: self.settings.duration,
                });
            }
        }
        if let Some(asset_id) = &self.voice_over_asset_id {
            if !self
                .assets
                .iter()
                .any(|asset| asset.id == *asset_id && asset.kind == AssetKind::Audio)
            {
                return Err(ProjectError::InvalidVoiceOverAsset(asset_id.clone()));
            }
        }
        for track in &self.tracks {
            for clip in &track.clips {
                let reason = if clip.composition.trim().is_empty() {
                    Some("composition cannot be empty")
                } else if clip.duration == 0 {
                    Some("duration must be greater than zero")
                } else if clip
                    .start
                    .checked_add(clip.duration)
                    .is_none_or(|end| end > self.settings.duration)
                {
                    Some("clip range must fit within the project duration")
                } else {
                    None
                };
                if let Some(reason) = reason {
                    return Err(ProjectError::InvalidClip {
                        track: track.id.clone(),
                        clip: clip.id.clone(),
                        reason: reason.into(),
                    });
                }
                validate_asset_references(&clip.props, &asset_ids)?;
            }
        }
        validate_asset_references(&self.props, &asset_ids)?;
        Ok(())
    }

    pub fn from_json_str(source: &str) -> Result<Self, ProjectError> {
        let project: Self =
            serde_json::from_str(source).map_err(|error| ProjectError::Json(error.to_string()))?;
        project.validate()?;
        Ok(project)
    }

    /// Validate local manifest assets relative to the project file directory.
    /// Remote URLs are intentionally left to the browser backend.
    pub fn validate_asset_files(
        &self,
        base_dir: impl AsRef<std::path::Path>,
    ) -> Result<(), ProjectError> {
        let base_dir =
            base_dir
                .as_ref()
                .canonicalize()
                .map_err(|error| ProjectError::AssetRead {
                    asset: "<project>".into(),
                    reason: error.to_string(),
                })?;
        for asset in &self.assets {
            if asset.path.contains("://") || asset.path.starts_with("data:") {
                continue;
            }
            let path = base_dir.join(&asset.path);
            let canonical = path
                .canonicalize()
                .map_err(|error| ProjectError::AssetRead {
                    asset: asset.id.clone(),
                    reason: format!("{} ({})", error, path.display()),
                })?;
            if !canonical.starts_with(&base_dir) {
                return Err(ProjectError::AssetOutsideProject(asset.id.clone()));
            }
            let bytes = std::fs::read(&canonical).map_err(|error| ProjectError::AssetRead {
                asset: asset.id.clone(),
                reason: format!("{} ({})", error, canonical.display()),
            })?;
            if let Some(expected) = &asset.sha256 {
                let actual = format!("{:x}", Sha256::digest(bytes));
                if !expected.eq_ignore_ascii_case(&actual) {
                    return Err(ProjectError::AssetHashMismatch {
                        asset: asset.id.clone(),
                        expected: expected.clone(),
                        actual,
                    });
                }
            }
        }
        Ok(())
    }

    /// Resolve local manifest paths and matching prop values from one project directory.
    pub fn resolve_local_asset_paths(&mut self, base_dir: impl AsRef<std::path::Path>) {
        let base_dir = base_dir.as_ref();
        let replacements: Vec<(String, String)> = self
            .assets
            .iter_mut()
            .flat_map(|asset| {
                let reference = format!("asset://{}", asset.id);
                let original = asset.path.clone();
                let resolved = if original.contains("://") || original.starts_with("data:") {
                    original.clone()
                } else {
                    base_dir.join(&original).to_string_lossy().into_owned()
                };
                asset.path = resolved.clone();
                vec![(original, resolved.clone()), (reference, resolved)]
            })
            .collect();

        fn rewrite(value: &mut serde_json::Value, replacements: &[(String, String)]) {
            match value {
                serde_json::Value::String(text) => {
                    if let Some((_, resolved)) =
                        replacements.iter().find(|(original, _)| original == text)
                    {
                        *text = resolved.clone();
                    }
                }
                serde_json::Value::Array(values) => values
                    .iter_mut()
                    .for_each(|value| rewrite(value, replacements)),
                serde_json::Value::Object(values) => values
                    .values_mut()
                    .for_each(|value| rewrite(value, replacements)),
                serde_json::Value::Null
                | serde_json::Value::Bool(_)
                | serde_json::Value::Number(_) => {}
            }
        }
        rewrite(&mut self.props, &replacements);
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                rewrite(&mut clip.props, &replacements);
            }
        }
    }

    /// Download remote HTTP(S) assets into `cache_dir` and rewrite the project
    /// to use the downloaded local files. Both per-asset and total downloaded
    /// byte limits are required. This is opt-in so Browser projects can
    /// continue to let Chromium fetch remote media directly.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn materialize_remote_assets(
        &mut self,
        cache_dir: impl AsRef<std::path::Path>,
        max_asset_bytes: usize,
        max_total_bytes: usize,
    ) -> Result<(), ProjectError> {
        self.materialize_remote_assets_with_resolver(
            cache_dir,
            max_asset_bytes,
            max_total_bytes,
            &PublicRemoteAssetResolver,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn materialize_remote_assets_with_resolver(
        &mut self,
        cache_dir: impl AsRef<std::path::Path>,
        max_asset_bytes: usize,
        max_total_bytes: usize,
        resolver: &impl RemoteAssetResolver,
    ) -> Result<(), ProjectError> {
        if max_asset_bytes == 0 {
            return Err(ProjectError::AssetRead {
                asset: "remote".into(),
                reason: "per-asset remote byte limit must be greater than zero".into(),
            });
        }
        if max_total_bytes == 0 {
            return Err(ProjectError::AssetRead {
                asset: "remote".into(),
                reason: "total remote asset byte limit must be greater than zero".into(),
            });
        }
        let cache_dir = cache_dir.as_ref();
        std::fs::create_dir_all(cache_dir)
            .map_err(|error| ProjectError::File(error.to_string()))?;
        let mut replacements = Vec::new();
        let mut downloaded_bytes = 0usize;
        for asset in &mut self.assets {
            let source = asset.path.trim();
            if !has_http_scheme(source) {
                continue;
            }
            let mut current_url =
                reqwest::Url::parse(source).map_err(|error| ProjectError::AssetRead {
                    asset: asset.id.clone(),
                    reason: format!("invalid remote asset URL: {error}"),
                })?;
            validate_remote_url(&current_url).map_err(|reason| ProjectError::AssetRead {
                asset: asset.id.clone(),
                reason,
            })?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            let mut response = None;
            for redirect_count in 0..=5 {
                let addresses =
                    resolver
                        .resolve(&current_url)
                        .map_err(|reason| ProjectError::AssetRead {
                            asset: asset.id.clone(),
                            reason,
                        })?;
                if addresses.is_empty() {
                    return Err(ProjectError::AssetRead {
                        asset: asset.id.clone(),
                        reason: "remote asset hostname resolved to no addresses".into(),
                    });
                }
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return Err(ProjectError::AssetRead {
                        asset: asset.id.clone(),
                        reason: "remote asset download timed out".into(),
                    });
                }
                let mut builder = reqwest::blocking::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .no_proxy()
                    .timeout(remaining);
                if let Some(host) = current_url.host_str() {
                    if parse_url_ip_literal(host).is_none() {
                        builder = builder.resolve_to_addrs(host, &addresses);
                    }
                }
                let client = builder.build().map_err(|error| ProjectError::AssetRead {
                    asset: asset.id.clone(),
                    reason: error.to_string(),
                })?;
                let result = client.get(current_url.clone()).send().map_err(|error| {
                    ProjectError::AssetRead {
                        asset: asset.id.clone(),
                        reason: error.to_string(),
                    }
                })?;
                if result.status().is_redirection() {
                    if let Some(location) = result.headers().get(reqwest::header::LOCATION) {
                        let location =
                            location.to_str().map_err(|error| ProjectError::AssetRead {
                                asset: asset.id.clone(),
                                reason: format!("invalid remote asset redirect: {error}"),
                            })?;
                        if redirect_count == 5 {
                            return Err(ProjectError::AssetRead {
                                asset: asset.id.clone(),
                                reason: "remote asset exceeded the 5 redirect limit".into(),
                            });
                        }
                        current_url = current_url.join(location).map_err(|error| {
                            ProjectError::AssetRead {
                                asset: asset.id.clone(),
                                reason: format!("invalid remote asset redirect URL: {error}"),
                            }
                        })?;
                        validate_remote_url(&current_url).map_err(|reason| {
                            ProjectError::AssetRead {
                                asset: asset.id.clone(),
                                reason,
                            }
                        })?;
                        continue;
                    }
                }
                response =
                    Some(
                        result
                            .error_for_status()
                            .map_err(|error| ProjectError::AssetRead {
                                asset: asset.id.clone(),
                                reason: error.to_string(),
                            })?,
                    );
                break;
            }
            let response = response.ok_or_else(|| ProjectError::AssetRead {
                asset: asset.id.clone(),
                reason: "remote asset redirect did not produce a response".into(),
            })?;
            let remaining_total_bytes = max_total_bytes.saturating_sub(downloaded_bytes);
            if let Some(length) = response.content_length() {
                if length > max_asset_bytes as u64 {
                    return Err(ProjectError::AssetRead {
                        asset: asset.id.clone(),
                        reason: format!(
                            "remote asset exceeds the {max_asset_bytes} byte per-asset limit"
                        ),
                    });
                }
                if length > remaining_total_bytes as u64 {
                    return Err(ProjectError::AssetRead {
                        asset: asset.id.clone(),
                        reason: format!(
                            "project remote assets exceed the {max_total_bytes} byte total limit"
                        ),
                    });
                }
            }
            let read_limit = max_asset_bytes.min(remaining_total_bytes);
            let mut bytes = Vec::with_capacity(read_limit.min(1024 * 1024));
            std::io::Read::read_to_end(
                &mut response.take((read_limit as u64).saturating_add(1)),
                &mut bytes,
            )
            .map_err(|error| ProjectError::AssetRead {
                asset: asset.id.clone(),
                reason: error.to_string(),
            })?;
            if bytes.len() > max_asset_bytes {
                return Err(ProjectError::AssetRead {
                    asset: asset.id.clone(),
                    reason: format!(
                        "remote asset exceeds the {max_asset_bytes} byte per-asset limit"
                    ),
                });
            }
            if bytes.len() > remaining_total_bytes {
                return Err(ProjectError::AssetRead {
                    asset: asset.id.clone(),
                    reason: format!(
                        "project remote assets exceed the {max_total_bytes} byte total limit"
                    ),
                });
            }
            let actual = format!("{:x}", Sha256::digest(&bytes));
            if let Some(expected) = &asset.sha256 {
                if !expected.eq_ignore_ascii_case(&actual) {
                    return Err(ProjectError::AssetHashMismatch {
                        asset: asset.id.clone(),
                        expected: expected.clone(),
                        actual,
                    });
                }
            }
            // Use only the digest in the cache filename. Asset IDs are user
            // controlled metadata and must never influence filesystem paths.
            let filename = format!("{actual}.asset");
            let local = cache_dir.join(filename);
            std::fs::write(&local, &bytes).map_err(|error| ProjectError::AssetRead {
                asset: asset.id.clone(),
                reason: error.to_string(),
            })?;
            let reference = format!("asset://{}", asset.id);
            replacements.push((source.to_string(), local.to_string_lossy().into_owned()));
            replacements.push((reference, local.to_string_lossy().into_owned()));
            asset.path = local.to_string_lossy().into_owned();
            downloaded_bytes += bytes.len();
        }
        fn rewrite(value: &mut serde_json::Value, replacements: &[(String, String)]) {
            match value {
                serde_json::Value::String(text) => {
                    if let Some((_, replacement)) =
                        replacements.iter().find(|(source, _)| source == text)
                    {
                        *text = replacement.clone();
                    }
                }
                serde_json::Value::Array(values) => values
                    .iter_mut()
                    .for_each(|value| rewrite(value, replacements)),
                serde_json::Value::Object(values) => values
                    .values_mut()
                    .for_each(|value| rewrite(value, replacements)),
                serde_json::Value::Null
                | serde_json::Value::Bool(_)
                | serde_json::Value::Number(_) => {}
            }
        }
        rewrite(&mut self.props, &replacements);
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                rewrite(&mut clip.props, &replacements);
            }
        }
        Ok(())
    }

    /// Convert local asset paths back to project-relative paths before saving.
    pub fn relativize_local_asset_paths(&mut self, base_dir: impl AsRef<std::path::Path>) {
        let base_dir = base_dir.as_ref();
        let replacements: Vec<(String, String)> = self
            .assets
            .iter_mut()
            .flat_map(|asset| {
                if asset.path.contains("://") || asset.path.starts_with("data:") {
                    return vec![(asset.path.clone(), format!("asset://{}", asset.id))];
                }
                let path = std::path::Path::new(&asset.path);
                let Some(relative) = path.strip_prefix(base_dir).ok() else {
                    return Vec::new();
                };
                let relative = relative.to_string_lossy().into_owned();
                let resolved = asset.path.clone();
                asset.path = relative.clone();
                vec![(resolved, relative)]
            })
            .collect();

        fn rewrite(value: &mut serde_json::Value, replacements: &[(String, String)]) {
            match value {
                serde_json::Value::String(text) => {
                    if let Some((_resolved, relative)) =
                        replacements.iter().find(|(resolved, _)| resolved == text)
                    {
                        *text = relative.clone();
                    }
                }
                serde_json::Value::Array(values) => values
                    .iter_mut()
                    .for_each(|value| rewrite(value, replacements)),
                serde_json::Value::Object(values) => values
                    .values_mut()
                    .for_each(|value| rewrite(value, replacements)),
                serde_json::Value::Null
                | serde_json::Value::Bool(_)
                | serde_json::Value::Number(_) => {}
            }
        }
        rewrite(&mut self.props, &replacements);
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                rewrite(&mut clip.props, &replacements);
            }
        }
    }

    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, ProjectError> {
        let source =
            std::fs::read_to_string(path).map_err(|error| ProjectError::File(error.to_string()))?;
        Self::from_json_str(&source)
    }

    pub fn save(&self, path: impl AsRef<std::path::Path>) -> Result<(), ProjectError> {
        self.validate()?;
        use std::io::Write;

        let path = path.as_ref();
        let source = serde_json::to_vec_pretty(self)
            .map_err(|error| ProjectError::Json(error.to_string()))?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .map_err(|error| ProjectError::File(error.to_string()))?;
        temporary
            .write_all(&source)
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|error| ProjectError::File(error.to_string()))?;
        if let Ok(metadata) = std::fs::metadata(path) {
            temporary
                .as_file()
                .set_permissions(metadata.permissions())
                .map_err(|error| ProjectError::File(error.to_string()))?;
        }
        temporary
            .persist(path)
            .map(|_| ())
            .map_err(|error| ProjectError::File(error.to_string()))
    }
}

#[cfg(not(target_arch = "wasm32"))]
trait RemoteAssetResolver {
    fn resolve(&self, url: &reqwest::Url) -> Result<Vec<SocketAddr>, String>;
}

#[cfg(not(target_arch = "wasm32"))]
struct PublicRemoteAssetResolver;

#[cfg(not(target_arch = "wasm32"))]
fn parse_url_ip_literal(host: &str) -> Option<IpAddr> {
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    host.parse().ok()
}

#[cfg(not(target_arch = "wasm32"))]
impl RemoteAssetResolver for PublicRemoteAssetResolver {
    fn resolve(&self, url: &reqwest::Url) -> Result<Vec<SocketAddr>, String> {
        let host = url
            .host_str()
            .ok_or_else(|| "remote asset URL has no hostname".to_string())?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| "remote asset URL has no valid port".to_string())?;
        let addresses = if let Some(ip) = parse_url_ip_literal(host) {
            vec![SocketAddr::new(ip, port)]
        } else {
            (host, port)
                .to_socket_addrs()
                .map_err(|error| format!("failed to resolve remote asset hostname: {error}"))?
                .collect()
        };
        validate_public_addresses(&addresses)?;
        Ok(addresses)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<F> RemoteAssetResolver for F
where
    F: Fn(&reqwest::Url) -> Result<Vec<SocketAddr>, String>,
{
    fn resolve(&self, url: &reqwest::Url) -> Result<Vec<SocketAddr>, String> {
        self(url)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn has_http_scheme(value: &str) -> bool {
    value
        .get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
        || value
            .get(..8)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
}

#[cfg(not(target_arch = "wasm32"))]
fn validate_remote_url(url: &reqwest::Url) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err("remote asset URL must use HTTP or HTTPS".into());
    }
    if url.host_str().is_none() {
        return Err("remote asset URL has no hostname".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("remote asset URL must not contain credentials".into());
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn validate_public_addresses(addresses: &[SocketAddr]) -> Result<(), String> {
    if addresses.is_empty() {
        return Err("remote asset hostname resolved to no addresses".into());
    }
    if addresses.iter().any(|address| !is_public_ip(address.ip())) {
        return Err("remote asset destination is not a public IP address".into());
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let value = u32::from(ip);
            let in_range = |network: u32, prefix: u32| {
                let mask = if prefix == 0 {
                    0
                } else {
                    u32::MAX << (32 - prefix)
                };
                value & mask == network & mask
            };
            let blocked = [
                (0x0000_0000, 8),  // This network
                (0x0a00_0000, 8),  // Private use
                (0x6440_0000, 10), // Shared address space
                (0x7f00_0000, 8),  // Loopback
                (0xa9fe_0000, 16), // Link local
                (0xac10_0000, 12), // Private use
                (0xc000_0000, 24), // IETF protocol assignments
                (0xc000_0200, 24), // Documentation
                (0xc058_6300, 24), // Deprecated 6to4 relay anycast
                (0xc0a8_0000, 16), // Private use
                (0xc612_0000, 15), // Benchmarking
                (0xc633_6400, 24), // Documentation
                (0xcb00_7100, 24), // Documentation
                (0xe000_0000, 4),  // Multicast
                (0xf000_0000, 4),  // Reserved and broadcast
            ];
            !blocked
                .iter()
                .any(|(network, prefix)| in_range(*network, *prefix))
                && value != u32::from(std::net::Ipv4Addr::new(168, 63, 129, 16))
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            let global_unicast = segments[0] >> 13 == 0b001;
            let value = u128::from(ip);
            // This allowlist follows the assigned prefixes in IANA's IPv6
            // Global Unicast Address Space registry; unlisted space is reserved.
            let assigned_global_unicast = [
                (0x2001_0200_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_0400_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_0600_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_0800_0000_0000_0000_0000_0000_0000, 22),
                (0x2001_0c00_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_0e00_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_1200_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_1400_0000_0000_0000_0000_0000_0000, 22),
                (0x2001_1800_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_1a00_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_1c00_0000_0000_0000_0000_0000_0000, 22),
                (0x2001_2000_0000_0000_0000_0000_0000_0000, 19),
                (0x2001_4000_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_4200_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_4400_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_4600_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_4800_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_4a00_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_4c00_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_5000_0000_0000_0000_0000_0000_0000, 20),
                (0x2001_8000_0000_0000_0000_0000_0000_0000, 19),
                (0x2001_a000_0000_0000_0000_0000_0000_0000, 20),
                (0x2001_b000_0000_0000_0000_0000_0000_0000, 20),
                (0x2003_0000_0000_0000_0000_0000_0000_0000, 18),
                (0x2400_0000_0000_0000_0000_0000_0000_0000, 12),
                (0x2410_0000_0000_0000_0000_0000_0000_0000, 12),
                (0x2600_0000_0000_0000_0000_0000_0000_0000, 12),
                (0x2610_0000_0000_0000_0000_0000_0000_0000, 23),
                (0x2620_0000_0000_0000_0000_0000_0000_0000, 23),
                (0x2630_0000_0000_0000_0000_0000_0000_0000, 12),
                (0x2800_0000_0000_0000_0000_0000_0000_0000, 12),
                (0x2a00_0000_0000_0000_0000_0000_0000_0000, 12),
                (0x2a10_0000_0000_0000_0000_0000_0000_0000, 12),
                (0x2c00_0000_0000_0000_0000_0000_0000_0000, 12),
            ];
            let in_prefix = |network: u128, prefix: u32| {
                let mask = u128::MAX << (128 - prefix);
                value & mask == network
            };
            let globally_reachable_special = [
                (0x2001_0001_0000_0000_0000_0000_0000_0001, 128),
                (0x2001_0001_0000_0000_0000_0000_0000_0002, 128),
                (0x2001_0001_0000_0000_0000_0000_0000_0003, 128),
                (0x2001_0003_0000_0000_0000_0000_0000_0000, 32),
                (0x2001_0004_0112_0000_0000_0000_0000_0000, 48),
                (0x2001_0020_0000_0000_0000_0000_0000_0000, 28),
                (0x2001_0030_0000_0000_0000_0000_0000_0000, 28),
            ];
            let allowed_special = globally_reachable_special
                .iter()
                .any(|(network, prefix)| in_prefix(*network, *prefix));
            let assigned = assigned_global_unicast
                .iter()
                .any(|(network, prefix)| in_prefix(*network, *prefix))
                || allowed_special;
            let special_use = (in_prefix(0x2001_0000_0000_0000_0000_0000_0000_0000, 23)
                && !allowed_special)
                || in_prefix(0x2001_0db8_0000_0000_0000_0000_0000_0000, 32)
                || in_prefix(0x2002_0000_0000_0000_0000_0000_0000_0000, 16);
            global_unicast && assigned && !special_use
        }
    }
}

/// In-memory job registry used by desktop hosts and suitable for replacement
/// by a persistent/server implementation without changing the API contract.
#[derive(Debug, Default)]
pub struct JobStore {
    jobs: BTreeMap<String, RenderJob>,
    next_id: u64,
}

impl JobStore {
    pub fn submit(&mut self, project: Project) -> Result<String, ProjectError> {
        project.validate()?;
        self.next_id += 1;
        let id = format!("job-{}", self.next_id);
        self.jobs.insert(
            id.clone(),
            RenderJob {
                id: id.clone(),
                project,
                status: JobStatus::Queued,
                completed_frames: 0,
                encoded_frames: 0,
                error: None,
                output: None,
                gpu_frames: None,
                cpu_fallback_frames: None,
                gpu_fallback_reason: None,
            },
        );
        Ok(id)
    }
    pub fn get(&self, id: &str) -> Option<&RenderJob> {
        self.jobs.get(id)
    }
    pub fn list(&self) -> Vec<RenderJob> {
        self.jobs.values().cloned().collect()
    }
    pub fn set_output(&mut self, id: &str, output: impl Into<String>) -> Result<(), ProjectError> {
        let job = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| ProjectError::JobNotFound(id.to_string()))?;
        job.output = Some(output.into());
        Ok(())
    }

    pub fn set_render_diagnostics(
        &mut self,
        id: &str,
        gpu_frames: u64,
        cpu_fallback_frames: u64,
        fallback_reason: Option<String>,
    ) -> Result<(), ProjectError> {
        let job = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| ProjectError::JobNotFound(id.to_string()))?;
        job.gpu_frames = Some(gpu_frames);
        job.cpu_fallback_frames = Some(cpu_fallback_frames);
        job.gpu_fallback_reason = fallback_reason;
        Ok(())
    }

    pub fn set_encoding_progress(
        &mut self,
        id: &str,
        encoded_frames: u32,
    ) -> Result<(), ProjectError> {
        let job = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| ProjectError::JobNotFound(id.to_string()))?;
        if !matches!(
            job.status,
            JobStatus::Queued | JobStatus::Preparing | JobStatus::Rendering | JobStatus::Encoding
        ) {
            return Err(ProjectError::InvalidJobTransition {
                from: job.status.clone(),
                to: JobStatus::Encoding,
            });
        }
        if encoded_frames < job.encoded_frames {
            return Err(ProjectError::ProgressRegressed {
                previous: job.encoded_frames,
                next: encoded_frames,
            });
        }
        job.encoded_frames = encoded_frames;
        Ok(())
    }

    /// Advance a successfully rendered job through encoding to completion.
    /// Keeping this transition sequence in the store prevents backend workers
    /// from accidentally omitting or reordering terminal states.
    pub fn complete_render(&mut self, id: &str, completed_frames: u32) -> Result<(), ProjectError> {
        let status = self
            .jobs
            .get(id)
            .ok_or_else(|| ProjectError::JobNotFound(id.to_string()))?
            .status
            .clone();
        if status == JobStatus::Preparing {
            self.try_update(id, JobStatus::Rendering, completed_frames)?;
        } else if status != JobStatus::Rendering {
            return Err(ProjectError::InvalidJobTransition {
                from: status,
                to: JobStatus::Completed,
            });
        }
        self.try_update(id, JobStatus::Encoding, completed_frames)?;
        self.set_encoding_progress(id, completed_frames)?;
        self.try_update(id, JobStatus::Completed, completed_frames)
    }
    pub fn update(&mut self, id: &str, status: JobStatus, completed_frames: u32) -> bool {
        self.try_update(id, status, completed_frames).is_ok()
    }
    pub fn try_update(
        &mut self,
        id: &str,
        status: JobStatus,
        completed_frames: u32,
    ) -> Result<(), ProjectError> {
        let job = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| ProjectError::JobNotFound(id.to_string()))?;
        let valid = matches!(
            (&job.status, &status),
            (
                JobStatus::Queued,
                JobStatus::Preparing | JobStatus::Cancelled | JobStatus::Failed
            ) | (
                JobStatus::Preparing,
                JobStatus::Rendering | JobStatus::Cancelled | JobStatus::Failed
            ) | (
                JobStatus::Rendering,
                JobStatus::Rendering
                    | JobStatus::Encoding
                    | JobStatus::Cancelled
                    | JobStatus::Failed
            ) | (
                JobStatus::Encoding,
                JobStatus::Encoding
                    | JobStatus::Completed
                    | JobStatus::Cancelled
                    | JobStatus::Failed
            ) | (JobStatus::Completed, JobStatus::Completed)
                | (JobStatus::Failed, JobStatus::Failed)
                | (JobStatus::Cancelled, JobStatus::Cancelled)
        );
        if !valid {
            return Err(ProjectError::InvalidJobTransition {
                from: job.status.clone(),
                to: status,
            });
        }
        if completed_frames < job.completed_frames {
            return Err(ProjectError::ProgressRegressed {
                previous: job.completed_frames,
                next: completed_frames,
            });
        }
        job.status = status;
        job.completed_frames = completed_frames;
        Ok(())
    }
    pub fn fail(&mut self, id: &str, message: impl Into<String>) -> Result<(), ProjectError> {
        let job = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| ProjectError::JobNotFound(id.to_string()))?;
        if !matches!(
            job.status,
            JobStatus::Queued
                | JobStatus::Preparing
                | JobStatus::Rendering
                | JobStatus::Encoding
                | JobStatus::Failed
        ) {
            return Err(ProjectError::InvalidJobTransition {
                from: job.status.clone(),
                to: JobStatus::Failed,
            });
        }
        job.status = JobStatus::Failed;
        job.error = Some(message.into());
        Ok(())
    }

    pub fn cancel(&mut self, id: &str) -> Result<(), ProjectError> {
        let job = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| ProjectError::JobNotFound(id.to_string()))?;
        if !matches!(
            job.status,
            JobStatus::Queued | JobStatus::Preparing | JobStatus::Rendering | JobStatus::Encoding
        ) {
            return Err(ProjectError::InvalidJobTransition {
                from: job.status.clone(),
                to: JobStatus::Cancelled,
            });
        }
        job.status = JobStatus::Cancelled;
        Ok(())
    }

    pub fn retry(&mut self, id: &str) -> Result<String, ProjectError> {
        let project = self
            .jobs
            .get(id)
            .ok_or_else(|| ProjectError::JobNotFound(id.to_string()))?;
        if !matches!(project.status, JobStatus::Failed | JobStatus::Cancelled) {
            return Err(ProjectError::InvalidJobTransition {
                from: project.status.clone(),
                to: JobStatus::Queued,
            });
        }
        self.submit(project.project.clone())
    }
}

fn validate_asset_references(
    value: &serde_json::Value,
    asset_ids: &BTreeSet<&str>,
) -> Result<(), ProjectError> {
    match value {
        serde_json::Value::String(text) if text.starts_with("asset://") => {
            let id = text.trim_start_matches("asset://");
            if id.is_empty() || !asset_ids.contains(id) {
                return Err(ProjectError::UnknownAssetReference(id.to_string()));
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                validate_asset_references(value, asset_ids)?;
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                validate_asset_references(value, asset_ids)?;
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project() -> Project {
        Project {
            version: 1,
            composition: "shorts".into(),
            settings: ProjectSettings {
                width: 1080,
                height: 1920,
                fps: 30.0,
                duration: 60,
                scale: 1.0,
                crf: None,
                preset: None,
                concurrency: None,
                frame_step: 1,
                frame_start: None,
                frame_end: None,
                backend: BackendKind::Native,
                browser_image_format: None,
                browser_jpeg_quality: None,
                browser_frame_timeout_ms: None,
                browser_transport: None,
                browser_transport_retries: None,
            },
            props: serde_json::json!({"title":"hello"}),
            assets: vec![],
            tracks: vec![],
            events: vec![],
            voice_over_asset_id: None,
        }
    }
    #[test]
    fn project_json_round_trip() {
        let p = project();
        assert_eq!(
            serde_json::from_str::<Project>(&serde_json::to_string(&p).unwrap()).unwrap(),
            p
        );
    }

    #[test]
    fn save_rewrites_local_asset_paths_in_timeline_clip_props() {
        let directory = tempfile::tempdir().unwrap();
        let asset_path = directory.path().join("logo.png");
        std::fs::write(&asset_path, b"logo").unwrap();
        let resolved_asset = asset_path.to_string_lossy().into_owned();
        let mut p = project();
        p.assets.push(AssetRef {
            id: "logo".into(),
            path: resolved_asset.clone(),
            kind: AssetKind::Image,
            sha256: None,
        });
        p.tracks.push(Track {
            id: "main".into(),
            clips: vec![Clip {
                id: "logo-clip".into(),
                composition: "shorts".into(),
                start: 0,
                duration: 10,
                props: serde_json::json!({"overlay": {"src": resolved_asset}}),
            }],
        });

        p.relativize_local_asset_paths(directory.path());
        let project_path = directory.path().join("project.json");
        p.save(&project_path).unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(project_path).unwrap()).unwrap();

        assert_eq!(saved["assets"][0]["path"], "logo.png");
        assert_eq!(
            saved["tracks"][0]["clips"][0]["props"]["overlay"]["src"],
            "logo.png"
        );
    }

    #[test]
    fn project_rejects_ambiguous_asset_manifest() {
        let mut p = project();
        p.assets = vec![
            AssetRef {
                id: "logo".into(),
                path: "logo.png".into(),
                kind: AssetKind::Image,
                sha256: None,
            },
            AssetRef {
                id: "logo".into(),
                path: "other.png".into(),
                kind: AssetKind::Image,
                sha256: None,
            },
        ];
        assert_eq!(
            p.validate(),
            Err(ProjectError::DuplicateAssetId("logo".into()))
        );
        p.assets[1].id.clear();
        assert_eq!(p.validate(), Err(ProjectError::EmptyAssetId));
        p.assets[1].id = "other".into();
        p.assets[1].path.clear();
        assert_eq!(
            p.validate(),
            Err(ProjectError::EmptyAssetPath("other".into()))
        );
        p.assets[1].path = "other.png".into();
        p.assets[1].sha256 = Some("not-a-digest".into());
        assert_eq!(
            p.validate(),
            Err(ProjectError::InvalidAssetHash("other".into()))
        );
        p.assets[1].sha256 = None;
        p.props = serde_json::json!({"src": "asset://missing"});
        assert_eq!(
            p.validate(),
            Err(ProjectError::UnknownAssetReference("missing".into()))
        );
    }

    #[test]
    fn project_asset_files_verify_sha256_relative_to_base_dir() {
        let base =
            std::env::temp_dir().join(format!("dioxuscut-project-assets-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let asset_path = base.join("logo.bin");
        std::fs::write(&asset_path, b"dioxuscut asset").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"dioxuscut asset"));
        let mut p = project();
        p.assets = vec![AssetRef {
            id: "logo".into(),
            path: "logo.bin".into(),
            kind: AssetKind::Image,
            sha256: Some(digest.clone()),
        }];
        assert!(p.validate_asset_files(&base).is_ok());
        p.assets[0].sha256 = Some("00".repeat(32));
        assert!(matches!(
            p.validate_asset_files(&base),
            Err(ProjectError::AssetHashMismatch { .. })
        ));
        p.assets[0].path = "../outside.bin".into();
        std::fs::write(base.join("../outside.bin"), b"outside").unwrap();
        assert_eq!(
            p.validate_asset_files(&base),
            Err(ProjectError::AssetOutsideProject("logo".into()))
        );
        std::fs::remove_file(base.join("../outside.bin")).unwrap();
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn project_save_atomically_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.dcp");
        std::fs::write(&path, b"old incomplete project").unwrap();

        project().save(&path).unwrap();

        let saved = Project::load(&path).unwrap();
        assert_eq!(saved, project());
        assert!(std::fs::read_dir(dir.path())
            .unwrap()
            .all(|entry| { entry.unwrap().file_name() == "project.dcp" }));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn remote_asset_destination_rejects_non_public_and_encoded_ip_literals() {
        let resolver = PublicRemoteAssetResolver;
        for source in [
            "http://127.0.0.1/asset",
            "http://2130706433/asset",
            "http://0x7f000001/asset",
            "http://0177.1/asset",
            "http://[::1]/asset",
            "http://[2001:2::1]/asset",
            "http://[2200::1]/asset",
            "http://[2d00::1]/asset",
            "http://[2e00::1]/asset",
            "http://[2f00::1]/asset",
            "http://[3000::1]/asset",
            "http://[3800::1]/asset",
            "http://[3c00::1]/asset",
            "http://[3e00::1]/asset",
            "http://[3f00::1]/asset",
            "http://[3f80::1]/asset",
            "http://[3fc0::1]/asset",
            "http://[3fe0::1]/asset",
            "http://[3ff0::1]/asset",
            "http://[3ff8::1]/asset",
            "http://[3ffc::1]/asset",
            "http://[3ffe::1]/asset",
            "http://[3fff::1]/asset",
            "http://localhost/asset",
            "http://169.254.169.254/latest/meta-data/",
        ] {
            let url = reqwest::Url::parse(source).unwrap();
            assert!(
                resolver.resolve(&url).is_err(),
                "expected {source} to be rejected (parsed as {url})"
            );
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn remote_asset_destination_rejects_mixed_dns_answers() {
        let public = SocketAddr::from(([93, 184, 216, 34], 443));
        let private = SocketAddr::from(([10, 0, 0, 7], 443));
        let public_ipv6 = "2606:4700:4700::1111".parse().unwrap();
        let reachable_special_ipv6 = "2001:1::1".parse().unwrap();

        assert!(validate_public_addresses(&[public]).is_ok());
        assert!(is_public_ip(public_ipv6));
        assert!(is_public_ip(reachable_special_ipv6));
        assert!(validate_public_addresses(&[public, private])
            .unwrap_err()
            .contains("not a public IP"));
        assert!(validate_public_addresses(&[])
            .unwrap_err()
            .contains("no addresses"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn remote_asset_resolver_accepts_public_and_rejects_private_ipv6_literals() {
        let resolver = PublicRemoteAssetResolver;
        let public = reqwest::Url::parse("https://[2606:4700:4700::1111]/asset").unwrap();
        let globally_reachable_exception =
            reqwest::Url::parse("https://[2001:1::1]/asset").unwrap();
        let private = reqwest::Url::parse("https://[fd00::1]/asset").unwrap();

        assert_eq!(
            resolver.resolve(&public).unwrap(),
            vec![SocketAddr::new(
                "2606:4700:4700::1111".parse().unwrap(),
                443
            )]
        );
        assert!(resolver.resolve(&globally_reachable_exception).is_ok());
        assert!(resolver.resolve(&private).is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn remote_asset_redirect_targets_are_resolved_and_rechecked() {
        let public_ip = reqwest::Url::parse("https://93.184.216.34/assets/clip.png").unwrap();
        let internal = public_ip.join("http://127.0.0.1/admin").unwrap();
        let credentials = public_ip
            .join("https://user:pass@cdn.example/asset")
            .unwrap();
        let resolver = PublicRemoteAssetResolver;

        assert!(resolver.resolve(&public_ip).is_ok());
        assert!(resolver.resolve(&internal).is_err());
        assert!(validate_remote_url(&credentials)
            .unwrap_err()
            .contains("credentials"));
        assert!(validate_remote_url(&reqwest::Url::parse("file:///etc/passwd").unwrap()).is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn materializer_rechecks_redirect_destination_before_connecting() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 512];
            let _ = stream.read(&mut request);
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{}/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                address.port()
            )
            .unwrap();
        });
        let mut p = project();
        p.assets = vec![AssetRef {
            id: "redirected".into(),
            path: format!("http://asset.test:{}/start", address.port()),
            kind: AssetKind::Other,
            sha256: None,
        }];
        let resolver = |url: &reqwest::Url| {
            if url.host_str() == Some("asset.test") {
                Ok(vec![address])
            } else {
                PublicRemoteAssetResolver.resolve(url)
            }
        };
        let cache = tempfile::tempdir().unwrap();

        assert!(matches!(
            p.materialize_remote_assets_with_resolver(cache.path(), 1024, 1024, &resolver),
            Err(ProjectError::AssetRead { reason, .. }) if reason.contains("not a public IP")
        ));
        server.join().unwrap();
    }

    #[test]
    fn materializes_remote_assets_and_rewrites_clip_props() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let payload = b"remote project asset".to_vec();
        let server_payload = payload.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let _ = stream.read(&mut request);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                server_payload.len()
            )
            .unwrap();
            stream.write_all(&server_payload).unwrap();
        });

        let cache =
            std::env::temp_dir().join(format!("dioxuscut-remote-assets-{}", std::process::id()));
        let url = format!("http://asset.test:{}/poster.png", address.port());
        let digest = format!("{:x}", Sha256::digest(&payload));
        let mut p = project();
        p.assets = vec![AssetRef {
            id: "poster".into(),
            path: url.clone(),
            kind: AssetKind::Image,
            sha256: Some(digest),
        }];
        p.props = serde_json::json!({"poster": "asset://poster"});
        p.tracks = vec![Track {
            id: "track".into(),
            clips: vec![Clip {
                id: "clip".into(),
                composition: "shorts".into(),
                start: 0,
                duration: 1,
                props: serde_json::json!({"src": url}),
            }],
        }];

        let resolver = |_: &reqwest::Url| Ok(vec![address]);
        p.materialize_remote_assets_with_resolver(&cache, 1024, 1024, &resolver)
            .unwrap();
        let local = std::path::PathBuf::from(&p.assets[0].path);
        assert_eq!(std::fs::read(&local).unwrap(), payload);
        assert_eq!(p.props["poster"], local.to_string_lossy().as_ref());
        assert_eq!(
            p.tracks[0].clips[0].props["src"],
            local.to_string_lossy().as_ref()
        );
        server.join().unwrap();
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn materializer_rejects_unknown_length_response_over_limit() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 256];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
                .unwrap();
            stream.write_all(&[7; 2048]).unwrap();
        });
        let mut p = project();
        p.assets = vec![AssetRef {
            id: "large".into(),
            path: format!("http://asset.test:{}/large.bin", address.port()),
            kind: AssetKind::Other,
            sha256: None,
        }];
        let cache =
            std::env::temp_dir().join(format!("dioxuscut-remote-limit-{}", std::process::id()));
        let resolver = |_: &reqwest::Url| Ok(vec![address]);
        assert!(matches!(
            p.materialize_remote_assets_with_resolver(&cache, 1024, 2048, &resolver),
            Err(ProjectError::AssetRead { reason, .. }) if reason.contains("exceeds")
        ));
        server.join().unwrap();
        let _ = std::fs::remove_dir_all(cache);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn materializer_enforces_total_limit_for_unknown_length_responses() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for (index, payload) in [b"abcd".to_vec(), b"xyz".to_vec()].into_iter().enumerate() {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 512];
                let _ = stream.read(&mut request);
                if index == 0 {
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        payload.len()
                    )
                    .unwrap();
                } else {
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
                        .unwrap();
                }
                let _ = stream.write_all(&payload);
            }
        });

        let first = format!("http://asset.test:{}/first.bin", address.port());
        let second = format!("http://asset.test:{}/second.bin", address.port());
        let mut p = project();
        p.assets = vec![
            AssetRef {
                id: "first".into(),
                path: first,
                kind: AssetKind::Other,
                sha256: None,
            },
            AssetRef {
                id: "second".into(),
                path: second.clone(),
                kind: AssetKind::Other,
                sha256: None,
            },
        ];
        let cache = tempfile::tempdir().unwrap();
        let resolver = |_: &reqwest::Url| Ok(vec![address]);

        assert!(matches!(
            p.materialize_remote_assets_with_resolver(cache.path(), 8, 6, &resolver),
            Err(ProjectError::AssetRead { reason, .. }) if reason.contains("6 byte total limit")
        ));
        server.join().unwrap();
        assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 1);
        assert_eq!(p.assets[1].path, second);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn materializer_rejects_declared_size_over_total_limit_before_caching() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 512];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nabcd")
                .unwrap();
        });
        let mut p = project();
        p.assets = vec![AssetRef {
            id: "oversized-total".into(),
            path: format!("http://asset.test:{}/asset.bin", address.port()),
            kind: AssetKind::Other,
            sha256: None,
        }];
        let cache = tempfile::tempdir().unwrap();
        let resolver = |_: &reqwest::Url| Ok(vec![address]);

        assert!(matches!(
            p.materialize_remote_assets_with_resolver(cache.path(), 8, 3, &resolver),
            Err(ProjectError::AssetRead { reason, .. }) if reason.contains("3 byte total limit")
        ));
        server.join().unwrap();
        assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn project_asset_files_reject_symlink_escape() {
        use std::os::unix::fs::symlink;
        let base =
            std::env::temp_dir().join(format!("dioxuscut-project-symlink-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let outside = base.with_file_name(format!("dioxuscut-outside-{}", std::process::id()));
        std::fs::write(&outside, b"outside").unwrap();
        symlink(&outside, base.join("linked.bin")).unwrap();
        let mut p = project();
        p.assets = vec![AssetRef {
            id: "linked".into(),
            path: "linked.bin".into(),
            kind: AssetKind::Other,
            sha256: None,
        }];
        assert_eq!(
            p.validate_asset_files(&base),
            Err(ProjectError::AssetOutsideProject("linked".into()))
        );
        std::fs::remove_file(base.join("linked.bin")).unwrap();
        std::fs::remove_dir_all(base).unwrap();
        std::fs::remove_file(outside).unwrap();
    }

    #[test]
    fn project_resolves_manifest_paths_inside_nested_props() {
        let mut p = project();
        p.assets = vec![
            AssetRef {
                id: "poster".into(),
                path: "assets/poster.png".into(),
                kind: AssetKind::Image,
                sha256: None,
            },
            AssetRef {
                id: "remote".into(),
                path: "https://cdn.example/poster.png".into(),
                kind: AssetKind::Image,
                sha256: None,
            },
        ];
        p.props =
            serde_json::json!({"layers": [{"src": "asset://poster"}, {"src": "asset://remote"}]});
        p.resolve_local_asset_paths("/tmp/project");
        let expected_poster = std::path::Path::new("/tmp/project").join("assets/poster.png");
        assert_eq!(std::path::Path::new(&p.assets[0].path), expected_poster);
        assert_eq!(p.assets[1].path, "https://cdn.example/poster.png");
        assert_eq!(
            std::path::Path::new(p.props["layers"][0]["src"].as_str().unwrap()),
            expected_poster
        );
        assert_eq!(
            p.props["layers"][1]["src"],
            "https://cdn.example/poster.png"
        );
    }
    #[test]
    fn project_json_loader_validates_schema() {
        let mut value = serde_json::to_value(project()).unwrap();
        value["version"] = serde_json::json!(2);
        assert_eq!(
            Project::from_json_str(&value.to_string()),
            Err(ProjectError::UnsupportedVersion(2))
        );
    }

    #[test]
    fn project_json_rejects_unknown_fields() {
        let mut value = serde_json::to_value(project()).unwrap();
        value["unexpected"] = serde_json::json!(true);
        assert!(matches!(
            Project::from_json_str(&value.to_string()),
            Err(ProjectError::Json(_))
        ));
    }

    #[test]
    fn project_rejects_zero_concurrency() {
        let mut value = serde_json::to_value(project()).unwrap();
        value["settings"]["concurrency"] = serde_json::json!(0);

        assert_eq!(
            Project::from_json_str(&serde_json::to_string(&value).unwrap()),
            Err(ProjectError::InvalidConcurrency)
        );
    }

    #[test]
    fn project_rejects_invalid_browser_capture_settings() {
        let mut p = project();
        p.settings.browser_image_format = Some("webp".into());
        assert_eq!(p.validate(), Err(ProjectError::InvalidBrowserImageFormat));

        let mut p = project();
        p.settings.browser_jpeg_quality = Some(0);
        assert_eq!(p.validate(), Err(ProjectError::InvalidBrowserJpegQuality));

        let mut p = project();
        p.settings.browser_frame_timeout_ms = Some(0);
        assert_eq!(p.validate(), Err(ProjectError::InvalidBrowserFrameTimeout));

        let mut p = project();
        p.settings.browser_transport = Some("shared-memory".into());
        assert_eq!(p.validate(), Err(ProjectError::InvalidBrowserTransport));
    }

    #[test]
    fn project_accepts_valid_browser_capture_settings() {
        let mut p = project();
        p.settings.browser_image_format = Some(" JPEG ".into());
        p.settings.browser_jpeg_quality = Some(90);
        p.settings.browser_frame_timeout_ms = Some(5_000);
        p.settings.browser_transport = Some(" FILE ".into());
        assert!(p.validate().is_ok());
        p.settings.browser_transport_retries = Some(2);
        assert!(p.validate().is_ok());
    }

    #[test]
    fn job_store_validates_and_tracks_progress() {
        let mut store = JobStore::default();
        let id = store.submit(project()).unwrap();
        assert_eq!(store.get(&id).unwrap().status, JobStatus::Queued);
        assert!(store.update(&id, JobStatus::Preparing, 0));
        assert!(store.update(&id, JobStatus::Rendering, 12));
        assert_eq!(store.get(&id).unwrap().completed_frames, 12);
    }
    #[test]
    fn invalid_project_is_rejected() {
        let mut p = project();
        p.settings.fps = 0.0;
        assert_eq!(p.validate(), Err(ProjectError::InvalidFps));
    }
    #[test]
    fn invalid_project_frame_ranges_are_rejected() {
        let mut p = project();
        p.settings.frame_start = Some(20);
        p.settings.frame_end = Some(10);
        assert!(matches!(
            p.validate(),
            Err(ProjectError::InvalidFrameRange { .. })
        ));

        p.settings.frame_end = None;
        p.settings.frame_start = Some(p.settings.duration);
        assert!(matches!(
            p.validate(),
            Err(ProjectError::InvalidFrameRange { .. })
        ));

        p.settings.frame_start = Some(0);
        p.settings.frame_end = Some(p.settings.duration);
        assert!(matches!(
            p.validate(),
            Err(ProjectError::InvalidFrameRange { .. })
        ));
    }

    #[test]
    fn project_rejects_clips_outside_duration() {
        let mut p = project();
        p.tracks = vec![Track {
            id: "main".into(),
            clips: vec![Clip {
                id: "late".into(),
                composition: "caption".into(),
                start: 59,
                duration: 2,
                props: serde_json::json!({}),
            }],
        }];
        assert!(matches!(
            p.validate(),
            Err(ProjectError::InvalidClip { reason, .. })
                if reason == "clip range must fit within the project duration"
        ));

        p.tracks[0].clips[0].start = u32::MAX;
        assert!(matches!(
            p.validate(),
            Err(ProjectError::InvalidClip { reason, .. })
                if reason == "clip range must fit within the project duration"
        ));
    }

    #[test]
    fn project_frame_start_without_end_defaults_to_duration_end() {
        let mut p = project();
        p.settings.frame_start = Some(12);
        assert!(p.validate().is_ok());
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["settings"]["frame_start"], 12);
    }
    #[test]
    fn invalid_job_transition_is_rejected() {
        let mut store = JobStore::default();
        let id = store.submit(project()).unwrap();
        assert!(store.try_update(&id, JobStatus::Encoding, 0).is_err());
        assert!(store.try_update(&id, JobStatus::Preparing, 0).is_ok());
    }
    #[test]
    fn progress_regression_and_failure_are_recorded() {
        let mut store = JobStore::default();
        let id = store.submit(project()).unwrap();
        store.try_update(&id, JobStatus::Preparing, 4).unwrap();
        assert!(matches!(
            store.try_update(&id, JobStatus::Rendering, 3),
            Err(ProjectError::ProgressRegressed { .. })
        ));
        store.fail(&id, "worker exited").unwrap();
        let job = store.get(&id).unwrap();
        assert_eq!(job.status, JobStatus::Failed);
        assert_eq!(job.error.as_deref(), Some("worker exited"));
    }

    #[test]
    fn cancellation_preserves_progress() {
        let mut store = JobStore::default();
        let id = store.submit(project()).unwrap();
        store.update(&id, JobStatus::Preparing, 0);
        store.update(&id, JobStatus::Rendering, 12);
        store.cancel(&id).unwrap();
        let job = store.get(&id).unwrap();
        assert_eq!(job.status, JobStatus::Cancelled);
        assert_eq!(job.completed_frames, 12);
    }

    #[test]
    fn cancellation_is_terminal_against_late_worker_updates() {
        let mut store = JobStore::default();
        let completed_id = store.submit(project()).unwrap();
        store.update(&completed_id, JobStatus::Preparing, 0);
        store.update(&completed_id, JobStatus::Rendering, 4);
        store.cancel(&completed_id).unwrap();
        assert!(matches!(
            store.try_update(&completed_id, JobStatus::Completed, 4),
            Err(ProjectError::InvalidJobTransition {
                from: JobStatus::Cancelled,
                to: JobStatus::Completed,
            })
        ));

        let failed_id = store.submit(project()).unwrap();
        store.update(&failed_id, JobStatus::Preparing, 0);
        store.update(&failed_id, JobStatus::Rendering, 4);
        store.cancel(&failed_id).unwrap();
        assert!(matches!(
            store.fail(&failed_id, "late worker failure"),
            Err(ProjectError::InvalidJobTransition {
                from: JobStatus::Cancelled,
                to: JobStatus::Failed,
            })
        ));
        assert_eq!(store.get(&failed_id).unwrap().status, JobStatus::Cancelled);
    }

    #[test]
    fn render_diagnostics_are_stored_and_serde_compatible() {
        let mut store = JobStore::default();
        let id = store.submit(project()).unwrap();
        store
            .set_render_diagnostics(
                &id,
                120,
                3,
                Some("scene contains GPU-unsupported nodes or effects".into()),
            )
            .unwrap();
        let job = store.get(&id).unwrap();
        assert_eq!(job.gpu_frames, Some(120));
        assert_eq!(job.cpu_fallback_frames, Some(3));
        assert_eq!(
            job.gpu_fallback_reason.as_deref(),
            Some("scene contains GPU-unsupported nodes or effects")
        );

        let encoded = serde_json::to_value(job).unwrap();
        let decoded: RenderJob = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, *job);
    }

    #[test]
    fn render_job_lifecycle_keeps_diagnostics_until_completion() {
        let mut store = JobStore::default();
        let id = store.submit(project()).unwrap();
        store.try_update(&id, JobStatus::Preparing, 0).unwrap();
        store.try_update(&id, JobStatus::Rendering, 8).unwrap();
        store.set_render_diagnostics(&id, 8, 0, None).unwrap();
        store.set_encoding_progress(&id, 6).unwrap();
        assert_eq!(store.get(&id).unwrap().encoded_frames, 6);
        store.complete_render(&id, 8).unwrap();

        let job = store.get(&id).unwrap();
        assert_eq!(job.status, JobStatus::Completed);
        assert_eq!(job.completed_frames, 8);
        assert_eq!(job.encoded_frames, 8);
        assert_eq!(job.gpu_frames, Some(8));
        assert_eq!(job.cpu_fallback_frames, Some(0));
        assert_eq!(job.gpu_fallback_reason, None);
    }

    #[test]
    fn complete_render_rejects_cancelled_job_without_reviving_it() {
        let mut store = JobStore::default();
        let id = store.submit(project()).unwrap();
        store.try_update(&id, JobStatus::Preparing, 0).unwrap();
        store.try_update(&id, JobStatus::Rendering, 4).unwrap();
        store.cancel(&id).unwrap();
        assert!(matches!(
            store.complete_render(&id, 4),
            Err(ProjectError::InvalidJobTransition {
                from: JobStatus::Cancelled,
                to: JobStatus::Completed,
            })
        ));
        assert_eq!(store.get(&id).unwrap().status, JobStatus::Cancelled);
    }

    #[test]
    fn late_encoding_progress_cannot_mutate_terminal_job() {
        let mut store = JobStore::default();
        let id = store.submit(project()).unwrap();
        store.try_update(&id, JobStatus::Preparing, 0).unwrap();
        store.try_update(&id, JobStatus::Rendering, 4).unwrap();
        store.cancel(&id).unwrap();

        assert!(matches!(
            store.set_encoding_progress(&id, 4),
            Err(ProjectError::InvalidJobTransition {
                from: JobStatus::Cancelled,
                to: JobStatus::Encoding,
            })
        ));
        assert_eq!(store.get(&id).unwrap().encoded_frames, 0);
    }

    #[test]
    fn list_returns_jobs_in_submission_order() {
        let mut store = JobStore::default();
        let first = store.submit(project()).unwrap();
        let second = store.submit(project()).unwrap();
        let jobs = store.list();
        assert_eq!(
            jobs.iter().map(|job| job.id.as_str()).collect::<Vec<_>>(),
            vec![first.as_str(), second.as_str()]
        );
    }

    #[test]
    fn retry_creates_a_fresh_queued_job() {
        let mut store = JobStore::default();
        let original = store.submit(project()).unwrap();
        store.cancel(&original).unwrap();
        let retry = store.retry(&original).unwrap();
        assert_eq!(retry, "job-2");
        let retry_job = store.get(&retry).unwrap();
        assert_eq!(retry_job.status, JobStatus::Queued);
        assert_eq!(retry_job.project, project());
        assert_eq!(retry_job.gpu_frames, None);
        assert_eq!(retry_job.cpu_fallback_frames, None);
        assert_eq!(retry_job.gpu_fallback_reason, None);
    }
}
