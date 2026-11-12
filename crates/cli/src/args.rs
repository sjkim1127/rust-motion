//! CLI argument definitions, exit codes, codecs, and render request configuration.

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use thiserror::Error;

use crate::composition::CompositionRegistry;

/// Error types for CLI input parameter validation.
#[derive(Error, Debug, PartialEq, Eq)]
pub enum ValidationError {
    #[error("Composition name cannot be empty")]
    EmptyComposition,
    #[error("Provide exactly one composition source: --composition <ID> or --script <PATH>")]
    MissingCompositionSource,
    #[error("--composition and --script cannot be used together")]
    ConflictingCompositionSources,
    #[error("Rhai script file not found: {0}")]
    ScriptFileNotFound(PathBuf),
    #[error("Props file not found: {0}")]
    PropsFileNotFound(PathBuf),
    #[error("Audio file not found: {0}")]
    AudioFileNotFound(PathBuf),
    #[error("Invalid resolution: width ({0}) and height ({1}) must be greater than 0")]
    InvalidZeroResolution(u32, u32),
    #[error(
        "Invalid resolution: width ({0}) and height ({1}) must be even numbers for video encoding"
    )]
    InvalidOddResolution(u32, u32),
    #[error("Invalid FPS: {0} must be a finite number greater than 0")]
    InvalidFps(String),
    #[error("Invalid duration: {0} must be greater than 0 frames")]
    InvalidDuration(u32),
    #[error("Invalid frame range: start {start}, end {end}, composition duration {duration}")]
    InvalidFrameRange { start: u32, end: u32, duration: u32 },
    #[error("Invalid frame step: {0}; expected a value greater than zero")]
    InvalidFrameStep(u32),
    #[error("Invalid concurrency: expected a value greater than zero")]
    InvalidConcurrency,
    #[error("Output extension '.{actual}' is invalid for {codec}; expected {expected}")]
    InvalidOutputExtension {
        codec: String,
        actual: String,
        expected: String,
    },
    #[error("Audio tracks are not supported for {0} output")]
    AudioNotSupported(String),
    #[error("Timeout must be greater than zero seconds")]
    InvalidTimeout,
    #[error("Invalid scale: {0}; expected a finite value greater than 0")]
    InvalidScale(String),
    #[error("Invalid CRF {value} for {codec}; expected {range}")]
    InvalidCrf {
        codec: String,
        value: u32,
        range: String,
    },
    #[error("Invalid encoder preset '{0}'; expected ultrafast, superfast, veryfast, faster, fast, medium, slow, slower, veryslow, or placebo")]
    InvalidPreset(String),
}

/// Native render backend selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum RenderBackend {
    /// Pure-Rust CPU rasterizer via tiny-skia. Default.
    #[default]
    Native,
    /// Browser-backed Three.js/WebGL renderer.
    Browser,
    /// GPU rasterizer via wgpu. Requires `--features gpu`.
    Gpu,
    /// Automatic hybrid dispatch (Native for motion graphics/subtitles, Chromium Browser for Three.js/DOM).
    Auto,
}

/// Output codec or still-image format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum RenderCodec {
    #[default]
    H264,
    H265,
    Vp9,
    Av1,
    #[value(name = "prores", alias = "pro-res")]
    ProRes,
    Gif,
    Png,
    Jpeg,
    Webp,
}

impl RenderCodec {
    pub(crate) fn still_format(self) -> Option<rust_motion_rasterizer::StillImageFormat> {
        match self {
            Self::Png => Some(rust_motion_rasterizer::StillImageFormat::Png),
            Self::Jpeg => Some(rust_motion_rasterizer::StillImageFormat::Jpeg),
            Self::Webp => Some(rust_motion_rasterizer::StillImageFormat::WebP),
            _ => None,
        }
    }

    pub(crate) fn video_codec(self) -> Option<rust_motion_rasterizer::VideoCodec> {
        match self {
            Self::H264 => Some(rust_motion_rasterizer::VideoCodec::H264),
            Self::H265 => Some(rust_motion_rasterizer::VideoCodec::H265),
            Self::Vp9 => Some(rust_motion_rasterizer::VideoCodec::Vp9),
            Self::Av1 => Some(rust_motion_rasterizer::VideoCodec::Av1),
            Self::ProRes => Some(rust_motion_rasterizer::VideoCodec::ProRes),
            Self::Gif => Some(rust_motion_rasterizer::VideoCodec::Gif),
            Self::Png | Self::Jpeg | Self::Webp => None,
        }
    }

    pub(crate) fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::H264 | Self::H265 => &["mp4"],
            Self::Vp9 | Self::Av1 => &["webm"],
            Self::ProRes => &["mov"],
            Self::Gif => &["gif"],
            Self::Png => &["png"],
            Self::Jpeg => &["jpg", "jpeg"],
            Self::Webp => &["webp"],
        }
    }
}

/// Resolve the browser worker script path, checking environment variables,
/// project-relative directories, and installation roots.
pub fn resolve_browser_worker_path() -> anyhow::Result<std::path::PathBuf> {
    if let Some(worker) = std::env::var_os("RUST_MOTION_BROWSER_WORKER") {
        let path = std::path::PathBuf::from(worker);
        if path.exists() {
            return Ok(path);
        }
        anyhow::bail!(
            "RUST_MOTION_BROWSER_WORKER path does not exist: {}",
            path.display()
        );
    }

    let candidates = [
        "scripts/three-render-worker.mjs",
        "../scripts/three-render-worker.mjs",
        "../../scripts/three-render-worker.mjs",
    ];

    for candidate in &candidates {
        let p = std::path::Path::new(candidate);
        if p.exists() {
            return Ok(p.canonicalize().unwrap_or_else(|_| p.to_path_buf()));
        }
    }

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            let relative_candidates = [
                exe_dir.join("scripts/three-render-worker.mjs"),
                exe_dir.join("../scripts/three-render-worker.mjs"),
                exe_dir.join("../../scripts/three-render-worker.mjs"),
                exe_dir.join("../../../scripts/three-render-worker.mjs"),
            ];
            for candidate in &relative_candidates {
                if candidate.exists() {
                    return Ok(candidate.clone());
                }
            }
        }
    }

    anyhow::bail!(
        "Browser backend requires a worker script. Set RUST_MOTION_BROWSER_WORKER or ensure 'scripts/three-render-worker.mjs' exists."
    )
}

/// Automatically select the optimal rendering backend based on composition and asset requirements.
/// Web-exclusive scenes (Three.js, DOM) dispatch to Chromium, while general motion graphics,
/// captions, and video scenes run 10x faster on Rust native (tiny-skia / wgpu).
pub fn resolve_hybrid_backend(
    backend: RenderBackend,
    composition_id: Option<&str>,
    script_path: Option<&std::path::Path>,
    registry: &CompositionRegistry,
) -> RenderBackend {
    match backend {
        RenderBackend::Native => RenderBackend::Native,
        RenderBackend::Browser => RenderBackend::Browser,
        RenderBackend::Gpu => RenderBackend::Gpu,
        RenderBackend::Auto => {
            if script_path.is_some() {
                return RenderBackend::Native;
            }
            if let Some(id) = composition_id {
                let id_lower = id.to_ascii_lowercase();
                if id_lower.starts_with("three_")
                    || id_lower.starts_with("web_")
                    || id_lower.starts_with("browser_")
                    || id_lower.starts_with("dom_")
                    || id_lower.ends_with(".tsx")
                    || id_lower.ends_with(".jsx")
                    || id_lower.ends_with(".html")
                {
                    return RenderBackend::Browser;
                }
                if registry.get(id).is_ok() {
                    return RenderBackend::Native;
                }

                // Fallback for custom/unregistered web compositions
                return RenderBackend::Browser;
            }
            RenderBackend::Native
        }
    }
}

/// Standard exit codes for the Rust Motion CLI.
pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_GENERAL_ERROR: i32 = 1;
pub const EXIT_INVALID_ARGUMENTS: i32 = 2;
pub const EXIT_MISSING_DEPENDENCY: i32 = 3;
pub const EXIT_VALIDATION_ERROR: i32 = 4;
pub const EXIT_TIMEOUT_OR_CANCELLED: i32 = 5;

/// Rust Motion CLI — render registered Rust or Rhai compositions to video.
#[derive(Parser, Debug, Clone, PartialEq)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Emit structured JSON output for machine consumption and automation.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum Commands {
    /// Render a registered Rust composition or Rhai script to a media file.
    Render {
        /// ID of the composition to render.
        #[arg(
            long,
            short,
            required_unless_present = "script",
            conflicts_with = "script"
        )]
        composition: Option<String>,

        /// Path to a Rhai composition script. Requires the `rhai` feature.
        #[arg(
            long,
            required_unless_present = "composition",
            conflicts_with = "composition"
        )]
        script: Option<PathBuf>,

        /// Path to a JSON file containing input props.
        #[arg(long, short)]
        props: Option<PathBuf>,

        /// Output media file path.
        #[arg(long, short, default_value = "out.mp4")]
        output: PathBuf,

        /// Local audio file to mix into the output. May be repeated.
        #[arg(long = "audio", value_name = "PATH")]
        audio: Vec<PathBuf>,

        /// Resolution width.
        #[arg(long, default_value_t = 1920)]
        width: u32,

        /// Resolution height.
        #[arg(long, default_value_t = 1080)]
        height: u32,

        /// Output scale applied after logical composition rendering.
        #[arg(long, default_value_t = 1.0)]
        scale: f64,

        /// Frames per second.
        #[arg(long, default_value_t = 30.0)]
        fps: f64,

        /// Duration in frames.
        #[arg(long, default_value_t = 150)]
        duration: u32,

        /// Rendering backend.
        #[arg(long, value_enum, default_value_t = RenderBackend::Native)]
        backend: RenderBackend,

        /// Video codec or still-image output format.
        #[arg(long, value_enum, default_value_t = RenderCodec::H264)]
        codec: RenderCodec,

        /// First composition frame to render.
        #[arg(long, default_value_t = 0)]
        frame_start: u32,

        /// Last composition frame to render, inclusive.
        #[arg(long)]
        frame_end: Option<u32>,

        /// Render every nth source frame for video output.
        #[arg(long, default_value_t = 1)]
        frame_step: u32,

        /// Number of parallel frame workers. Omit to use host defaults.
        #[arg(long)]
        concurrency: Option<usize>,

        /// Abort the render after this many seconds.
        #[arg(long)]
        timeout_seconds: Option<u64>,

        /// Codec quality value. Lower is generally higher quality.
        #[arg(long, default_value_t = 18)]
        crf: u32,

        /// FFmpeg encoder preset for H.264 and H.265.
        #[arg(long, default_value = "fast")]
        preset: String,

        /// Hardware acceleration mode for video encoding.
        #[arg(long, value_enum, default_value_t = HwAccelArg::Auto)]
        hw_accel: HwAccelArg,

        /// Allowed root directory for media assets. May be repeated (alias: --media-root).
        #[arg(long = "sandbox-root", alias = "media-root", value_name = "DIR")]
        sandbox_roots: Vec<PathBuf>,

        /// Run in permissive mode without sandbox jail (unrestricted filesystem access).
        #[arg(long, default_value_t = false)]
        permissive: bool,

        /// Output end-to-end GPU profiling table and telemetry.
        #[arg(long, default_value_t = false)]
        profile: bool,
    },

    /// List compositions available to the native registry.
    ListCompositions,

    /// Render browser WebCodecs frames and write a timeline-drift report.
    WebcodecsDrift {
        /// Browser worker executable/script implementing the Rust Motion protocol.
        #[arg(long)]
        worker: PathBuf,

        /// Node or compatible worker host executable.
        #[arg(long, default_value = "node")]
        node: String,

        /// Browser composition URL.
        #[arg(long, default_value = "http://localhost:1420")]
        url: String,

        /// Browser-side composition identifier.
        #[arg(long, default_value = "BrowserComposition")]
        composition: String,

        #[arg(long, default_value_t = 1920)]
        width: u32,
        #[arg(long, default_value_t = 1080)]
        height: u32,
        #[arg(long, default_value_t = 30.0)]
        fps: f64,
        #[arg(long, default_value_t = 0)]
        frame_start: u32,
        /// Number of consecutive output frames to validate.
        #[arg(long, default_value_t = 60)]
        frames: u32,
        #[arg(long, default_value_t = 1)]
        concurrency: usize,
        /// Number of independent frame-sequence measurements.
        #[arg(long, default_value_t = 1)]
        repetitions: usize,
        /// JSON report output path.
        #[arg(long, short, default_value = "webcodecs-drift.json")]
        output: PathBuf,
        /// Optional CSV report output path.
        #[arg(long)]
        csv: Option<PathBuf>,
    },

    /// Validate a versioned Rust Motion project file without rendering.
    ValidateProject {
        /// Path to the `.rust_motion.json` project file.
        input: PathBuf,
    },

    /// Render a versioned `.rust_motion.json` project file.
    RenderProject {
        /// Path to the project file.
        input: PathBuf,
        /// Output media file path.
        #[arg(long, short, default_value = "out.mp4")]
        output: PathBuf,
        /// Optional directory for downloading HTTP(S) project assets before rendering.
        #[arg(long)]
        asset_cache_dir: Option<PathBuf>,
        /// Maximum combined bytes downloaded for this project render.
        #[arg(long, default_value_t = 1024 * 1024 * 1024)]
        max_total_asset_bytes: usize,
    },

    /// (Experimental) Scaffold a Rust Motion component from a Remotion (.tsx) file.
    Migrate {
        /// Path to the Remotion .tsx file.
        input: PathBuf,

        /// Target language ('rust', 'rhai', or 'python').
        #[arg(long, short, default_value = "rust")]
        target: String,

        /// Destination output file path (optional, prints to stdout if omitted).
        #[arg(long, short)]
        output: Option<PathBuf>,

        /// Explicit acknowledgment of experimental preview status.
        #[arg(long, default_value_t = false)]
        experimental: bool,
    },

    /// Inspect video or audio metadata (resolution, fps, duration, aspect ratio).
    Probe {
        /// Media file path to inspect.
        path: PathBuf,
        /// Return unified image/video/audio metadata instead of legacy track output.
        #[arg(long, default_value_t = false)]
        full: bool,
    },

    /// Start a hot-reloading web studio with a live WebSocket frame preview.
    Serve {
        /// Path to the Rhai composition script to preview.
        #[arg(long, short)]
        script: PathBuf,

        /// Path to a JSON props file (optional).
        #[arg(long, short)]
        props: Option<PathBuf>,

        /// TCP port to bind on.
        #[arg(long, default_value_t = 7890)]
        port: u16,

        /// Default frame index to render on change.
        #[arg(long, short, default_value_t = 0)]
        frame: u32,

        /// Render width.
        #[arg(long, default_value_t = 1920)]
        width: u32,

        /// Render height.
        #[arg(long, default_value_t = 1080)]
        height: u32,

        /// Frames per second (used for composition context).
        #[arg(long, default_value_t = 30.0)]
        fps: f64,

        /// Duration in frames (used for composition context).
        #[arg(long, default_value_t = 150)]
        duration: u32,
    },

    /// Validate inputs (project file, Rhai script, or composition) before rendering.
    Validate {
        /// Path to a versioned `.rust_motion.json` project file.
        #[arg(long, short = 'p', conflicts_with_all = ["composition", "script"])]
        project: Option<PathBuf>,

        /// ID of a registered composition to validate.
        #[arg(long, short = 'c', conflicts_with_all = ["project", "script"])]
        composition: Option<String>,

        /// Path to a Rhai composition script to validate.
        #[arg(long, short = 's', conflicts_with_all = ["project", "composition"])]
        script: Option<PathBuf>,

        /// Path to a JSON props file.
        #[arg(long)]
        props: Option<PathBuf>,
    },

    /// Inspect media metadata, project structure, or system capabilities.
    Inspect {
        /// Target path to inspect (media file or project file).
        #[arg(value_name = "TARGET")]
        target: Option<PathBuf>,

        /// Inspect registered built-in compositions.
        #[arg(long, default_value_t = false)]
        compositions: bool,

        /// Inspect supported rendering backends and capability matrix.
        #[arg(long, default_value_t = false)]
        backends: bool,

        /// Inspect supported codecs and system encoder availability.
        #[arg(long, default_value_t = false)]
        codecs: bool,
    },

    /// Verify local environment, external dependencies (FFmpeg, FFprobe, Node), and hardware encoding capabilities.
    Doctor,

    /// Launch the lightweight web studio in your default browser.
    Studio {
        /// TCP port to bind on.
        #[arg(long, default_value_t = 1420)]
        port: u16,

        /// Automatically open the browser.
        #[arg(long, default_value_t = true)]
        open: bool,

        /// Path to an optional project file or script to load on start.
        #[arg(long, short)]
        project: Option<PathBuf>,
    },
}

/// Hardware acceleration selection for the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum HwAccelArg {
    #[default]
    Auto,
    Disabled,
    #[value(name = "videotoolbox", alias = "vt")]
    VideoToolbox,
    Nvenc,
}

impl From<HwAccelArg> for rust_motion_rasterizer::HwAccel {
    fn from(arg: HwAccelArg) -> Self {
        match arg {
            HwAccelArg::Auto => rust_motion_rasterizer::HwAccel::Auto,
            HwAccelArg::Disabled => rust_motion_rasterizer::HwAccel::Disabled,
            HwAccelArg::VideoToolbox => rust_motion_rasterizer::HwAccel::VideoToolbox,
            HwAccelArg::Nvenc => rust_motion_rasterizer::HwAccel::Nvenc,
        }
    }
}

/// A validated render request independent from argument parsing.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderRequest {
    pub composition: Option<String>,
    pub script: Option<PathBuf>,
    pub props: Option<PathBuf>,
    pub output: PathBuf,
    pub audio: Vec<PathBuf>,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub fps: f64,
    pub duration: u32,
    pub backend: RenderBackend,
    pub codec: RenderCodec,
    pub frame_start: u32,
    pub frame_end: Option<u32>,
    pub frame_step: u32,
    pub concurrency: Option<usize>,
    pub timeout_seconds: Option<u64>,
    pub crf: u32,
    pub preset: String,
    pub hw_accel: rust_motion_rasterizer::HwAccel,
    pub sandbox_roots: Vec<PathBuf>,
    pub permissive: bool,
}

impl RenderRequest {
    /// Returns the concurrency selected for a render job.
    ///
    /// Keeping this policy on the request makes CLI, Tauri, and embedded
    /// callers agree on the same lower bound while allowing a backend to
    /// provide its own capacity (for example, the number of browser workers).
    pub fn effective_concurrency(&self, backend_capacity: usize) -> usize {
        self.concurrency.unwrap_or(backend_capacity.max(1)).max(1)
    }

    /// Determines the active MediaSecurityPolicy.
    ///
    /// - If `permissive` is true, returns `MediaSecurityPolicy::Permissive`.
    /// - If `sandbox_roots` is non-empty, returns `MediaSecurityPolicy::sandboxed(sandbox_roots)`.
    /// - If rendering an external Rhai script (`self.script.is_some()`), automatically defaults
    ///   to `MediaSecurityPolicy::sandboxed([script_dir])` to safely sandbox untrusted scripts under least privilege.
    /// - Otherwise (built-in Rust composition with no roots specified), defaults to `MediaSecurityPolicy::Permissive`.
    pub fn effective_security_policy(&self) -> rust_motion_rasterizer::MediaSecurityPolicy {
        if self.permissive {
            return rust_motion_rasterizer::MediaSecurityPolicy::Permissive;
        }
        if !self.sandbox_roots.is_empty() {
            return rust_motion_rasterizer::MediaSecurityPolicy::sandboxed(
                self.sandbox_roots.clone(),
            );
        }
        if let Some(ref script_path) = self.script {
            let mut roots = Vec::new();
            if let Some(parent) = script_path.parent() {
                if let Ok(canon) = parent.canonicalize() {
                    roots.push(canon);
                } else if !parent.as_os_str().is_empty() {
                    roots.push(parent.to_path_buf());
                }
            }
            if roots.is_empty() {
                // If script_path had no parent directory component (e.g. "script.rhai"),
                // the script resides in current_dir.
                if let Ok(cwd) = std::env::current_dir() {
                    roots.push(cwd);
                }
            }
            return rust_motion_rasterizer::MediaSecurityPolicy::sandboxed(roots);
        }
        rust_motion_rasterizer::MediaSecurityPolicy::Permissive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(concurrency: Option<usize>) -> RenderRequest {
        RenderRequest {
            composition: Some("test".into()),
            script: None,
            props: None,
            output: "out.mp4".into(),
            audio: vec![],
            width: 320,
            height: 180,
            scale: 1.0,
            fps: 30.0,
            duration: 1,
            backend: RenderBackend::Native,
            codec: RenderCodec::H264,
            frame_start: 0,
            frame_end: None,
            frame_step: 1,
            concurrency,
            timeout_seconds: None,
            crf: 23,
            preset: "medium".into(),
            hw_accel: rust_motion_rasterizer::HwAccel::Auto,
            sandbox_roots: vec![],
            permissive: false,
        }
    }

    #[test]
    fn effective_concurrency_uses_request_then_backend_capacity() {
        assert_eq!(request(None).effective_concurrency(0), 1);
        assert_eq!(request(None).effective_concurrency(4), 4);
        assert_eq!(request(Some(3)).effective_concurrency(8), 3);
        assert_eq!(request(Some(0)).effective_concurrency(8), 1);
    }
}
