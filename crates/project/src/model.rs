//! Serializable project and render-job data models plus the shared error type.

use super::*;

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
    Auto,
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
