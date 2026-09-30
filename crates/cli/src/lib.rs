//! CLI options, validation, native composition registry, and render execution.

pub mod composition;
pub mod migrate;
#[cfg(feature = "rhai")]
pub mod rhai_runtime;
pub mod serve;

pub use composition::{
    built_in_registry, BarChartRaceComposition, CodeTerminalComposition,
    ComplexGradientsComposition, Composition, CompositionError, CompositionRegistry,
    CompositionRegistryError, CyberpunkGridComposition, HelloWorldComposition,
    KaraokeCaptionsComposition, NativeComposition, NativeCompositionContext,
    PodcastWaveformComposition, PreparedComposition, ShapesAndFiltersComposition,
};
pub use dioxuscut_media::{
    get_audio_metadata, get_video_metadata, parse_media, static_file, AudioMetadata,
    ParsedMediaMetadata, VideoMetadata,
};
use dioxuscut_project::{Clip, Project};
pub use migrate::{transpile_remotion, MigrationStats, MigrationTarget};

/// Resolve audio assets declared by a project into encoder input paths.
///
/// Keeping this mapping in the shared host layer makes CLI and Tauri project
/// renders agree on which assets become audio streams.
pub fn project_audio_assets(project: &Project) -> Vec<std::path::PathBuf> {
    project
        .assets
        .iter()
        .filter(|asset| asset.kind == dioxuscut_project::AssetKind::Audio)
        .map(|asset| std::path::PathBuf::from(&asset.path))
        .collect()
}

/// Resolve project audio assets relative to the project file directory.
/// URL assets remain unchanged for browser-side resolution.
pub fn project_audio_assets_from_dir(
    project: &Project,
    base_dir: impl AsRef<std::path::Path>,
) -> Vec<std::path::PathBuf> {
    let base_dir = base_dir.as_ref();
    project
        .assets
        .iter()
        .filter(|asset| asset.kind == dioxuscut_project::AssetKind::Audio)
        .map(|asset| {
            if asset.path.contains("://") || asset.path.starts_with("data:") {
                std::path::PathBuf::from(&asset.path)
            } else {
                base_dir.join(&asset.path)
            }
        })
        .collect()
}

fn collect_scene_audio_tracks(
    prepared: &dyn PreparedComposition,
    first_scene: &dioxuscut_rasterizer::Scene,
    duration_in_frames: u32,
    fps: f64,
) -> Result<Vec<dioxuscut_rasterizer::AudioTrack>, CompositionError> {
    let mut tracks: Vec<(usize, dioxuscut_rasterizer::AudioTrack)> = Vec::new();
    for frame in 0..duration_in_frames {
        let scene;
        let frame_tracks = if frame == 0 {
            first_scene.audio_tracks()
        } else {
            scene = prepared.render(frame)?;
            scene.audio_tracks()
        };
        for (index, mut candidate) in frame_tracks.into_iter().enumerate() {
            if candidate.timeline_start == 0.0 && frame > 0 {
                candidate.timeline_start = frame as f64 / fps;
            }
            if let Some((_, existing)) = tracks.iter_mut().find(|(track_index, track)| {
                *track_index == index && same_inferred_audio_track(track, &candidate)
            }) {
                if existing.duration.is_none() {
                    existing.duration = candidate.duration;
                } else if let Some(candidate_duration) = candidate.duration {
                    existing.duration = Some(
                        existing
                            .duration
                            .expect("duration was checked above")
                            .max(candidate_duration),
                    );
                }
                if existing.volume_keyframes.is_empty() {
                    existing.volume_keyframes = candidate.volume_keyframes;
                }
                continue;
            }
            tracks.push((index, candidate));
        }
    }
    Ok(tracks.into_iter().map(|(_, track)| track).collect())
}

fn same_inferred_audio_track(
    left: &dioxuscut_rasterizer::AudioTrack,
    right: &dioxuscut_rasterizer::AudioTrack,
) -> bool {
    left.src == right.src
        && left.start_from == right.start_from
        && left.playback_rate == right.playback_rate
        && left.looped == right.looped
}

fn native_image_cache_bytes() -> Option<usize> {
    std::env::var("DIOXUSCUT_IMAGE_CACHE_BYTES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
}

#[cfg(test)]
mod project_asset_tests {
    use super::*;

    fn project_fixture() -> Project {
        Project {
            version: 1,
            composition: "test".into(),
            settings: dioxuscut_project::ProjectSettings {
                width: 320,
                height: 240,
                fps: 30.0,
                duration: 30,
                scale: 1.0,
                crf: None,
                preset: None,
                concurrency: None,
                frame_step: 1,
                frame_start: None,
                frame_end: None,
                backend: dioxuscut_project::BackendKind::Native,
                browser_image_format: None,
                browser_jpeg_quality: None,
                browser_frame_timeout_ms: None,
                browser_transport: None,
                browser_transport_retries: None,
            },
            props: serde_json::json!({}),
            assets: vec![],
            tracks: vec![],
        }
    }

    #[test]
    fn project_audio_assets_selects_only_audio_assets() {
        let project = Project {
            version: 1,
            composition: "test".into(),
            settings: dioxuscut_project::ProjectSettings {
                width: 320,
                height: 240,
                fps: 30.0,
                duration: 30,
                scale: 1.0,
                crf: None,
                preset: None,
                concurrency: None,
                frame_step: 1,
                frame_start: None,
                frame_end: None,
                backend: dioxuscut_project::BackendKind::Native,
                browser_image_format: None,
                browser_jpeg_quality: None,
                browser_frame_timeout_ms: None,
                browser_transport: None,
                browser_transport_retries: None,
            },
            props: serde_json::json!({}),
            assets: vec![
                dioxuscut_project::AssetRef {
                    id: "music".into(),
                    path: "music.wav".into(),
                    kind: dioxuscut_project::AssetKind::Audio,
                    sha256: None,
                },
                dioxuscut_project::AssetRef {
                    id: "logo".into(),
                    path: "logo.png".into(),
                    kind: dioxuscut_project::AssetKind::Image,
                    sha256: None,
                },
            ],
            tracks: vec![],
        };

        assert_eq!(
            project_audio_assets(&project),
            vec![std::path::PathBuf::from("music.wav")]
        );
    }

    #[test]
    fn project_audio_assets_from_dir_resolves_local_paths_without_touching_urls() {
        let project = Project {
            assets: vec![
                dioxuscut_project::AssetRef {
                    id: "local".into(),
                    path: "audio/music.wav".into(),
                    kind: dioxuscut_project::AssetKind::Audio,
                    sha256: None,
                },
                dioxuscut_project::AssetRef {
                    id: "remote".into(),
                    path: "https://cdn.example/music.wav".into(),
                    kind: dioxuscut_project::AssetKind::Audio,
                    sha256: None,
                },
            ],
            ..project_fixture()
        };
        assert_eq!(
            project_audio_assets_from_dir(&project, "/tmp/project"),
            vec![
                std::path::PathBuf::from("/tmp/project/audio/music.wav"),
                std::path::PathBuf::from("https://cdn.example/music.wav")
            ]
        );
    }
}
#[cfg(feature = "rhai")]
pub use rhai_runtime::{RhaiComposition, SceneBuilder};

/// Browser workers own the actual composition runtime. This placeholder keeps
/// the shared render pipeline's native validation stage from rejecting a
/// browser-only composition ID that is not registered in the Rust registry.
struct BrowserComposition {
    id: String,
}

#[cfg(test)]
mod project_timeline_tests {
    use super::*;

    struct AudioOnlyComposition;

    struct PreparedAudioOnly;

    struct PreparedLateSceneAudio;

    impl dioxuscut_composition::Composition for AudioOnlyComposition {
        fn id(&self) -> &str {
            "AudioOnly"
        }

        fn prepare(
            &self,
            _props: &serde_json::Value,
            _context: NativeCompositionContext,
        ) -> Result<Box<dyn dioxuscut_composition::PreparedComposition + '_>, CompositionError>
        {
            Ok(Box::new(PreparedAudioOnly))
        }
    }

    impl dioxuscut_composition::PreparedComposition for PreparedAudioOnly {
        fn render(&self, _frame: u32) -> Result<dioxuscut_rasterizer::Scene, CompositionError> {
            Ok(dioxuscut_rasterizer::Scene::new())
        }

        fn audio_tracks(
            &self,
        ) -> Result<Option<Vec<dioxuscut_rasterizer::AudioTrack>>, CompositionError> {
            Ok(Some(vec![dioxuscut_rasterizer::AudioTrack::new(
                "later.wav",
            )]))
        }
    }

    impl dioxuscut_composition::PreparedComposition for PreparedLateSceneAudio {
        fn render(&self, frame: u32) -> Result<dioxuscut_rasterizer::Scene, CompositionError> {
            let mut scene = dioxuscut_rasterizer::Scene::new();
            if frame >= 30 {
                scene.push(dioxuscut_rasterizer::SceneNode::Audio {
                    track: dioxuscut_rasterizer::AudioTrack::new("later.wav"),
                });
            }
            Ok(scene)
        }
    }

    #[test]
    fn clips_use_local_frames_only_when_active() {
        let timeline = ProjectTimelineComposition {
            id: "timeline".into(),
            clips: vec![Clip {
                id: "clip".into(),
                composition: "HelloWorld".into(),
                start: 2,
                duration: 2,
                props: serde_json::json!({}),
            }],
            registry: built_in_registry(),
        };
        let prepared = timeline
            .prepare(
                &serde_json::json!({}),
                NativeCompositionContext {
                    width: 320,
                    height: 240,
                    fps: 30.0,
                    duration_in_frames: 5,
                },
            )
            .expect("timeline prepares");
        assert!(prepared.render(1).expect("inactive frame").nodes.is_empty());
        assert!(!prepared.render(2).expect("active frame").nodes.is_empty());
        assert!(!prepared.render(3).expect("active frame").nodes.is_empty());
        assert!(prepared.render(4).expect("after clip").nodes.is_empty());
    }

    #[test]
    fn timeline_collects_audio_from_later_clips() {
        let mut registry = CompositionRegistry::new();
        registry
            .register(AudioOnlyComposition)
            .expect("audio composition registers");
        let timeline = ProjectTimelineComposition {
            id: "timeline-audio".into(),
            clips: vec![Clip {
                id: "later".into(),
                composition: "AudioOnly".into(),
                start: 10,
                duration: 5,
                props: serde_json::json!({}),
            }],
            registry,
        };
        let prepared = timeline
            .prepare(
                &serde_json::json!({}),
                NativeCompositionContext {
                    width: 320,
                    height: 240,
                    fps: 30.0,
                    duration_in_frames: 20,
                },
            )
            .expect("timeline prepares");
        let tracks = prepared
            .audio_tracks()
            .expect("audio tracks resolve")
            .expect("timeline provides audio tracks");
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].src, "later.wav");
        assert!((tracks[0].timeline_start - 10.0 / 30.0).abs() < f64::EPSILON);
        assert_eq!(tracks[0].duration, Some(5.0 / 30.0));
    }

    #[test]
    fn legacy_scene_audio_is_collected_when_first_emitted_after_frame_zero() {
        let prepared = PreparedLateSceneAudio;
        let first_scene = prepared.render(0).expect("frame zero renders");

        let tracks = collect_scene_audio_tracks(&prepared, &first_scene, 60, 30.0)
            .expect("scene audio is collected");

        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].src, "later.wav");
        assert!((tracks[0].timeline_start - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn nested_scene_audio_keeps_rebased_nonzero_start_during_cli_inference() {
        let mut audio = dioxuscut_media::SceneAudio::new("voice.wav");
        audio.track.timeline_start = 1.0;
        let composition = dioxuscut_composition::SceneEmitterComposition::new(
            "nested-audio",
            dioxuscut_composition::SceneSequence::new(60, audio),
        );
        let context = NativeCompositionContext {
            width: 320,
            height: 240,
            fps: 30.0,
            duration_in_frames: 120,
        };
        let prepared = composition
            .prepare(&serde_json::Value::Null, context)
            .expect("native composition prepares");
        let first_scene = prepared.render(0).expect("frame zero renders");

        let tracks = collect_scene_audio_tracks(prepared.as_ref(), &first_scene, 120, 30.0)
            .expect("nested scene audio is collected");

        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].src, "voice.wav");
        assert!((tracks[0].timeline_start - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn project_timelines_are_used_by_native_and_gpu_backends() {
        assert!(uses_project_timeline(RenderBackend::Native, true));
        assert!(uses_project_timeline(RenderBackend::Gpu, true));
        assert!(!uses_project_timeline(RenderBackend::Browser, true));
        assert!(!uses_project_timeline(RenderBackend::Gpu, false));
    }
}

struct BrowserPreparedComposition;

struct ProjectTimelineComposition {
    id: String,
    clips: Vec<Clip>,
    registry: CompositionRegistry,
}

struct ProjectTimelinePrepared<'a> {
    composition: &'a ProjectTimelineComposition,
    context: NativeCompositionContext,
}

impl dioxuscut_composition::PreparedComposition for ProjectTimelinePrepared<'_> {
    fn render(&self, frame: u32) -> Result<dioxuscut_rasterizer::Scene, CompositionError> {
        let mut scene = dioxuscut_rasterizer::Scene::new();
        for clip in &self.composition.clips {
            if frame < clip.start || frame >= clip.start.saturating_add(clip.duration) {
                continue;
            }
            let composition = self
                .composition
                .registry
                .get(&clip.composition)
                .map_err(|error| CompositionError::render(frame, error.to_string()))?;
            let clip_context = NativeCompositionContext {
                duration_in_frames: clip.duration,
                ..self.context
            };
            let prepared = composition
                .prepare(&clip.props, clip_context)
                .map_err(|error| CompositionError::render(frame, error.to_string()))?;
            let clip_scene = prepared
                .render(frame - clip.start)
                .map_err(|error| CompositionError::render(frame, error.to_string()))?;
            scene.nodes.extend(clip_scene.nodes);
        }
        Ok(scene)
    }

    fn audio_tracks(
        &self,
    ) -> Result<Option<Vec<dioxuscut_rasterizer::AudioTrack>>, CompositionError> {
        let mut tracks = Vec::new();
        for clip in &self.composition.clips {
            let composition = self
                .composition
                .registry
                .get(&clip.composition)
                .map_err(|error| CompositionError::render(clip.start, error.to_string()))?;
            let clip_context = NativeCompositionContext {
                duration_in_frames: clip.duration,
                ..self.context
            };
            let prepared = composition
                .prepare(&clip.props, clip_context)
                .map_err(|error| CompositionError::render(clip.start, error.to_string()))?;
            let clip_tracks = match prepared.audio_tracks()? {
                Some(tracks) => tracks,
                None => prepared.render(0)?.audio_tracks(),
            };
            let clip_offset = clip.start as f64 / self.context.fps;
            let clip_duration = clip.duration as f64 / self.context.fps;
            for mut track in clip_tracks {
                track.timeline_start += clip_offset;
                track.duration = Some(
                    track
                        .duration
                        .map_or(clip_duration, |duration| duration.min(clip_duration)),
                );
                for (time, _) in &mut track.volume_keyframes {
                    *time += clip_offset;
                }
                tracks.push(track);
            }
        }
        Ok(Some(tracks))
    }
}

impl Composition for ProjectTimelineComposition {
    fn id(&self) -> &str {
        &self.id
    }

    fn prepare(
        &self,
        _props: &serde_json::Value,
        context: NativeCompositionContext,
    ) -> Result<Box<dyn dioxuscut_composition::PreparedComposition + '_>, CompositionError> {
        Ok(Box::new(ProjectTimelinePrepared {
            composition: self,
            context,
        }))
    }
}

impl dioxuscut_composition::PreparedComposition for BrowserPreparedComposition {
    fn render(&self, _frame: u32) -> Result<dioxuscut_rasterizer::Scene, CompositionError> {
        Ok(dioxuscut_rasterizer::Scene::new())
    }
}

impl Composition for BrowserComposition {
    fn id(&self) -> &str {
        &self.id
    }

    fn prepare(
        &self,
        _props: &serde_json::Value,
        _context: NativeCompositionContext,
    ) -> Result<Box<dyn dioxuscut_composition::PreparedComposition + '_>, CompositionError> {
        Ok(Box::new(BrowserPreparedComposition))
    }
}

use clap::{Parser, Subcommand, ValueEnum};
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

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
    /// Browser-backed Three.js/WebGL renderer. Requires a worker path in `DIOXUSCUT_BROWSER_WORKER`.
    Browser,
    /// GPU rasterizer via wgpu. Requires `--features gpu`.
    Gpu,
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
    fn still_format(self) -> Option<dioxuscut_rasterizer::StillImageFormat> {
        match self {
            Self::Png => Some(dioxuscut_rasterizer::StillImageFormat::Png),
            Self::Jpeg => Some(dioxuscut_rasterizer::StillImageFormat::Jpeg),
            Self::Webp => Some(dioxuscut_rasterizer::StillImageFormat::WebP),
            _ => None,
        }
    }

    fn video_codec(self) -> Option<dioxuscut_rasterizer::VideoCodec> {
        match self {
            Self::H264 => Some(dioxuscut_rasterizer::VideoCodec::H264),
            Self::H265 => Some(dioxuscut_rasterizer::VideoCodec::H265),
            Self::Vp9 => Some(dioxuscut_rasterizer::VideoCodec::Vp9),
            Self::Av1 => Some(dioxuscut_rasterizer::VideoCodec::Av1),
            Self::ProRes => Some(dioxuscut_rasterizer::VideoCodec::ProRes),
            Self::Gif => Some(dioxuscut_rasterizer::VideoCodec::Gif),
            Self::Png | Self::Jpeg | Self::Webp => None,
        }
    }

    fn extensions(self) -> &'static [&'static str] {
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

/// Dioxuscut CLI — render registered Rust or Rhai compositions to video.
#[derive(Parser, Debug, Clone, PartialEq)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
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
        /// Browser worker executable/script implementing the Dioxuscut protocol.
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

    /// Validate a versioned Dioxuscut project file without rendering.
    ValidateProject {
        /// Path to the `.dioxuscut.json` project file.
        input: PathBuf,
    },

    /// Render a versioned `.dioxuscut.json` project file.
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

    /// (Experimental) Scaffold a Dioxuscut component from a Remotion (.tsx) file.
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

impl From<HwAccelArg> for dioxuscut_rasterizer::HwAccel {
    fn from(arg: HwAccelArg) -> Self {
        match arg {
            HwAccelArg::Auto => dioxuscut_rasterizer::HwAccel::Auto,
            HwAccelArg::Disabled => dioxuscut_rasterizer::HwAccel::Disabled,
            HwAccelArg::VideoToolbox => dioxuscut_rasterizer::HwAccel::VideoToolbox,
            HwAccelArg::Nvenc => dioxuscut_rasterizer::HwAccel::Nvenc,
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
    pub hw_accel: dioxuscut_rasterizer::HwAccel,
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
    pub fn effective_security_policy(&self) -> dioxuscut_rasterizer::MediaSecurityPolicy {
        if self.permissive {
            return dioxuscut_rasterizer::MediaSecurityPolicy::Permissive;
        }
        if !self.sandbox_roots.is_empty() {
            return dioxuscut_rasterizer::MediaSecurityPolicy::sandboxed(
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
            return dioxuscut_rasterizer::MediaSecurityPolicy::sandboxed(roots);
        }
        dioxuscut_rasterizer::MediaSecurityPolicy::Permissive
    }
}

#[cfg(test)]
mod render_request_tests {
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
            hw_accel: dioxuscut_rasterizer::HwAccel::Auto,
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

/// Validates that a render request selects exactly one available composition source.
pub fn validate_composition_source(
    composition: Option<&str>,
    script: Option<&PathBuf>,
) -> Result<(), ValidationError> {
    match (composition, script) {
        (None, None) => Err(ValidationError::MissingCompositionSource),
        (Some(_), Some(_)) => Err(ValidationError::ConflictingCompositionSources),
        (Some(composition), None) if composition.trim().is_empty() => {
            Err(ValidationError::EmptyComposition)
        }
        (Some(_), None) => Ok(()),
        (None, Some(path)) if !path.is_file() => {
            Err(ValidationError::ScriptFileNotFound(path.clone()))
        }
        (None, Some(_)) => Ok(()),
    }
}

/// Validates command-line parameters prior to launching the renderer.
pub fn validate_render_params(
    composition: &str,
    props: Option<&PathBuf>,
    width: u32,
    height: u32,
    fps: f64,
    duration: u32,
) -> Result<(), ValidationError> {
    validate_render_params_for_codec(
        composition,
        props,
        width,
        height,
        fps,
        duration,
        RenderCodec::H264,
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_render_params_for_codec(
    composition: &str,
    props: Option<&PathBuf>,
    width: u32,
    height: u32,
    fps: f64,
    duration: u32,
    codec: RenderCodec,
) -> Result<(), ValidationError> {
    if composition.trim().is_empty() {
        return Err(ValidationError::EmptyComposition);
    }
    if let Some(path) = props {
        if !path.is_file() {
            return Err(ValidationError::PropsFileNotFound(path.clone()));
        }
    }
    if width == 0 || height == 0 {
        return Err(ValidationError::InvalidZeroResolution(width, height));
    }
    let requires_even_dimensions = matches!(
        codec,
        RenderCodec::H264
            | RenderCodec::H265
            | RenderCodec::Vp9
            | RenderCodec::Av1
            | RenderCodec::ProRes
    );
    if requires_even_dimensions && (!width.is_multiple_of(2) || !height.is_multiple_of(2)) {
        return Err(ValidationError::InvalidOddResolution(width, height));
    }
    if !fps.is_finite() || fps <= 0.0 {
        return Err(ValidationError::InvalidFps(fps.to_string()));
    }
    if duration == 0 {
        return Err(ValidationError::InvalidDuration(duration));
    }
    Ok(())
}

fn validate_render_options(request: &RenderRequest) -> Result<(u32, u32), ValidationError> {
    if !request.scale.is_finite() || request.scale <= 0.0 {
        return Err(ValidationError::InvalidScale(request.scale.to_string()));
    }
    if request.frame_step == 0 {
        return Err(ValidationError::InvalidFrameStep(request.frame_step));
    }
    if request.concurrency == Some(0) {
        return Err(ValidationError::InvalidConcurrency);
    }
    if request.frame_step > 1 && request.codec.still_format().is_some() {
        return Err(ValidationError::InvalidFrameStep(request.frame_step));
    }
    let end = request.frame_end.unwrap_or_else(|| {
        if request.codec.still_format().is_some() {
            request.frame_start
        } else {
            request.duration.saturating_sub(1)
        }
    });
    if request.frame_start > end || end >= request.duration {
        return Err(ValidationError::InvalidFrameRange {
            start: request.frame_start,
            end,
            duration: request.duration,
        });
    }
    if request.timeout_seconds == Some(0) {
        return Err(ValidationError::InvalidTimeout);
    }
    if request.codec.still_format().is_some() && end != request.frame_start {
        return Err(ValidationError::InvalidFrameRange {
            start: request.frame_start,
            end,
            duration: request.duration,
        });
    }
    let max_crf = match request.codec {
        RenderCodec::H264 | RenderCodec::H265 => Some(51),
        RenderCodec::Vp9 | RenderCodec::Av1 => Some(63),
        RenderCodec::ProRes
        | RenderCodec::Gif
        | RenderCodec::Png
        | RenderCodec::Jpeg
        | RenderCodec::Webp => None,
    };
    if max_crf.is_some_and(|max| request.crf > max) {
        return Err(ValidationError::InvalidCrf {
            codec: format!("{:?}", request.codec),
            value: request.crf,
            range: format!("0..={}", max_crf.expect("checked above")),
        });
    }
    if matches!(request.codec, RenderCodec::H264 | RenderCodec::H265)
        && !matches!(
            request.preset.as_str(),
            "ultrafast"
                | "superfast"
                | "veryfast"
                | "faster"
                | "fast"
                | "medium"
                | "slow"
                | "slower"
                | "veryslow"
                | "placebo"
        )
    {
        return Err(ValidationError::InvalidPreset(request.preset.clone()));
    }
    let actual = request
        .output
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let expected = request.codec.extensions();
    if !expected.contains(&actual.as_str()) {
        return Err(ValidationError::InvalidOutputExtension {
            codec: format!("{:?}", request.codec),
            actual,
            expected: expected.join(" or ."),
        });
    }
    if !request.audio.is_empty()
        && (request.codec.still_format().is_some() || request.codec == RenderCodec::Gif)
    {
        return Err(ValidationError::AudioNotSupported(format!(
            "{:?}",
            request.codec
        )));
    }
    Ok((request.frame_start, end))
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
    control: dioxuscut_rasterizer::RenderControl,
) -> anyhow::Result<()> {
    if !uses_project_timeline(request.backend, !project.tracks.is_empty()) {
        return execute_render_command_with_registry_and_control(
            request,
            &built_in_registry(),
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
    execute_render_command_with_registry_and_control(request, &registry, control).await
}

fn uses_project_timeline(backend: RenderBackend, has_tracks: bool) -> bool {
    has_tracks && matches!(backend, RenderBackend::Native | RenderBackend::Gpu)
}

/// Build the standard CLI progress and timeout controls for a render request.
pub fn default_render_control(request: &RenderRequest) -> dioxuscut_rasterizer::RenderControl {
    // Progress is emitted by the shared render engine so every CLI render
    // path (native, browser, still, and video) has the same observable
    // behavior. Keep the CLI responsible only for request-specific timeout
    // configuration.
    let mut control = dioxuscut_rasterizer::RenderControl::new()
        .with_stderr_progress()
        .with_stderr_render_stats();
    if let Some(seconds) = request.timeout_seconds {
        control = control.with_timeout(std::time::Duration::from_secs(seconds));
    }
    control
}

#[cfg(feature = "gpu")]
fn report_gpu_fallback_diagnostics(
    rasterizer: &dioxuscut_rasterizer::WgpuBackend,
    control: &dioxuscut_rasterizer::RenderControl,
) {
    let stats = rasterizer.render_stats();
    control.report_diagnostics(dioxuscut_rasterizer::RenderDiagnostics {
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
    control: dioxuscut_rasterizer::RenderControl,
) -> anyhow::Result<()> {
    let registry = built_in_registry();
    execute_render_command_with_registry_and_control(request, &registry, control).await
}

/// Execute an application-provided composition registry with caller-owned controls.
pub async fn execute_render_command_with_registry_and_control(
    request: &RenderRequest,
    registry: &CompositionRegistry,
    control: dioxuscut_rasterizer::RenderControl,
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
                if request.backend == RenderBackend::Browser {
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
                 cargo build -p dioxuscut-cli --features rhai"
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
                if request.backend == RenderBackend::Browser {
                    Ok(&browser_fallback as &dyn Composition)
                } else {
                    Err(error)
                }
            })?
    };

    let prepared = composition.prepare(&props, context)?;

    // Validate the first frame before starting FFmpeg. Dynamic compositions
    // therefore report syntax, type, and API errors without creating an output.
    let first_scene = prepared.render(0)?;
    let mut audio_tracks = match prepared.audio_tracks()? {
        Some(tracks) => tracks,
        None if request.backend != RenderBackend::Browser
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
        request
            .audio
            .iter()
            .map(|path| dioxuscut_rasterizer::AudioTrack::new(path.to_string_lossy().into_owned())),
    );
    let first_scene_cache = std::sync::Arc::new(std::sync::Mutex::new(Some(first_scene)));

    tracing::info!(
        composition = composition.id(),
        backend = ?request.backend,
        codec = ?request.codec,
        frame_start,
        frame_end,
        "Starting render"
    );

    let security_policy = request.effective_security_policy();
    #[cfg(feature = "gpu")]
    let mut gpu_profile_summary: Option<dioxuscut_rasterizer::ProfilingSummary> = None;

    match request.backend {
        RenderBackend::Native => {
            use dioxuscut_rasterizer::{
                render_still_fallible_scaled, render_to_ffmpeg_pipe_fallible, PipeConfig,
                TinySkiaBackend,
            };

            let rasterizer = TinySkiaBackend::new()
                .with_image_cache_bytes(
                    native_image_cache_bytes()
                        .unwrap_or(dioxuscut_rasterizer::DEFAULT_IMAGE_CACHE_BYTES),
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
            use dioxuscut_rasterizer::{
                render_still_fallible_scaled, render_web_to_ffmpeg_pipe_fallible,
                BrowserFrameBackend, PipeConfig,
            };
            let worker = std::env::var_os("DIOXUSCUT_BROWSER_WORKER").ok_or_else(|| {
                anyhow::anyhow!("Browser backend requires DIOXUSCUT_BROWSER_WORKER")
            })?;
            let url = std::env::var("DIOXUSCUT_BROWSER_URL")
                .unwrap_or_else(|_| "http://localhost:1420".to_string());
            let browser_capacity = std::env::var("DIOXUSCUT_BROWSER_CONCURRENCY")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or_else(|| {
                    std::thread::available_parallelism()
                        .map(|parallelism| parallelism.get().min(4))
                        .unwrap_or(1)
                });
            let concurrency = request.effective_concurrency(browser_capacity);
            let node = std::env::var_os("DIOXUSCUT_BROWSER_NODE").unwrap_or_else(|| "node".into());
            let mut rasterizer =
                BrowserFrameBackend::with_concurrency(node, worker, url, concurrency)
                    .map_err(|error| anyhow::anyhow!("Browser backend init failed: {error}"))?
                    .with_frame_cache_bytes(
                        std::env::var("DIOXUSCUT_FRAME_CACHE_BYTES")
                            .ok()
                            .and_then(|value| value.parse::<usize>().ok())
                            .filter(|value| *value > 0)
                            .unwrap_or(dioxuscut_rasterizer::DEFAULT_MAX_CACHE_BYTES),
                    );
            if !std::env::var("DIOXUSCUT_BROWSER_TRANSPORT")
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
                    |_| Ok::<_, std::convert::Infallible>(dioxuscut_rasterizer::Scene::new()),
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
                 cargo build -p dioxuscut-cli --features gpu"
            );

            #[cfg(feature = "gpu")]
            {
                use dioxuscut_rasterizer::{
                    render_still_fallible_scaled, render_to_ffmpeg_pipe_fallible, PipeConfig,
                    WgpuBackend,
                };

                let profile_enabled = std::env::var("DIOXUSCUT_PROFILE")
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);

                let rasterizer = WgpuBackend::new()
                    .map_err(|error| anyhow::anyhow!("GPU backend init failed: {error}"))?
                    .with_image_cache_bytes(
                        native_image_cache_bytes()
                            .unwrap_or(dioxuscut_rasterizer::DEFAULT_IMAGE_CACHE_BYTES),
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
    }

    tracing::info!(output = %request.output.display(), "Render completed");
    if std::env::var_os("DIOXUSCUT_JSON").is_some() {
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
