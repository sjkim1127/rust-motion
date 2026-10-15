//! CLI options, validation, native composition registry, and render execution.

pub mod composition;
pub mod migrate;
#[cfg(feature = "rhai")]
pub mod rhai_runtime;
pub mod serve;
pub mod studio;

pub use composition::{
    built_in_registry, BarChartRaceComposition, CodeTerminalComposition,
    ComplexGradientsComposition, Composition, CompositionError, CompositionRegistry,
    CompositionRegistryError, CyberpunkGridComposition, HelloWorldComposition,
    KaraokeCaptionsComposition, NativeComposition, NativeCompositionContext,
    PodcastWaveformComposition, PreparedComposition, ShapesAndFiltersComposition,
    TimeEventSchedule,
};
pub use migrate::{transpile_remotion, MigrationStats, MigrationTarget};
pub use rust_motion_media::{
    get_audio_metadata, get_video_metadata, parse_media, static_file, AudioMetadata,
    ParsedMediaMetadata, VideoMetadata,
};
use rust_motion_project::{Clip, Project};

/// Resolve audio assets declared by a project into encoder input paths.
///
/// Keeping this mapping in the shared host layer makes CLI and Tauri project
/// renders agree on which assets become audio streams.
pub fn project_audio_assets(project: &Project) -> Vec<std::path::PathBuf> {
    project
        .assets
        .iter()
        .filter(|asset| asset.kind == rust_motion_project::AssetKind::Audio)
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
        .filter(|asset| asset.kind == rust_motion_project::AssetKind::Audio)
        .map(|asset| {
            if asset.path.contains("://") || asset.path.starts_with("data:") {
                std::path::PathBuf::from(&asset.path)
            } else {
                base_dir.join(&asset.path)
            }
        })
        .collect()
}

struct InferredAudioTrack {
    track_index: usize,
    track: rust_motion_rasterizer::AudioTrack,
    has_explicit_volume_keyframes: bool,
    last_sampled_volume: f64,
}

fn collect_scene_audio_tracks(
    prepared: &dyn PreparedComposition,
    first_scene: &rust_motion_rasterizer::Scene,
    duration_in_frames: u32,
    fps: f64,
) -> Result<Vec<rust_motion_rasterizer::AudioTrack>, CompositionError> {
    let mut tracks: Vec<InferredAudioTrack> = Vec::new();
    for frame in 0..duration_in_frames {
        let scene;
        let frame_tracks = if frame == 0 {
            first_scene.audio_tracks()
        } else {
            scene = prepared.render(frame)?;
            scene.audio_tracks()
        };
        for (index, mut candidate) in frame_tracks.into_iter().enumerate() {
            let sample_time = frame as f64 / fps;
            if candidate.timeline_start == 0.0 && frame > 0 {
                candidate.timeline_start = sample_time;
            }

            if let Some(existing) = tracks.iter_mut().find(|existing| {
                existing.track_index == index
                    && same_inferred_audio_track(&existing.track, &candidate)
            }) {
                match (existing.track.duration, candidate.duration) {
                    (None, Some(duration)) => existing.track.duration = Some(duration),
                    (Some(current), Some(candidate)) => {
                        existing.track.duration = Some(current.max(candidate));
                    }
                    _ => {}
                }

                if !candidate.volume_keyframes.is_empty() {
                    // An explicit curve is authoritative over inferred volume
                    // samples, including when it first appears on a later frame.
                    if !existing.has_explicit_volume_keyframes {
                        existing.track.volume = candidate.volume;
                        existing.track.volume_keyframes = candidate.volume_keyframes;
                        existing.has_explicit_volume_keyframes = true;
                    }
                } else if !existing.has_explicit_volume_keyframes
                    && candidate.volume != existing.last_sampled_volume
                {
                    if existing.track.volume_keyframes.is_empty() {
                        // Keyframe values are multipliers of the base volume in
                        // the FFmpeg path. Use a unity base and record absolute
                        // sampled gains so zero-to-positive changes are preserved.
                        existing
                            .track
                            .volume_keyframes
                            .push((existing.track.timeline_start, existing.track.volume));
                        existing.track.volume = 1.0;
                    }
                    existing
                        .track
                        .volume_keyframes
                        .push((sample_time, candidate.volume));
                }

                existing.last_sampled_volume = candidate.volume;
                continue;
            }

            tracks.push(InferredAudioTrack {
                track_index: index,
                has_explicit_volume_keyframes: !candidate.volume_keyframes.is_empty(),
                last_sampled_volume: candidate.volume,
                track: candidate,
            });
        }
    }
    Ok(tracks.into_iter().map(|inferred| inferred.track).collect())
}

fn same_inferred_audio_track(
    left: &rust_motion_rasterizer::AudioTrack,
    right: &rust_motion_rasterizer::AudioTrack,
) -> bool {
    left.src == right.src
        && left.start_from == right.start_from
        && left.playback_rate == right.playback_rate
        && left.looped == right.looped
}

fn native_image_cache_bytes() -> Option<usize> {
    std::env::var("RUST_MOTION_IMAGE_CACHE_BYTES")
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
            settings: rust_motion_project::ProjectSettings {
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
                backend: rust_motion_project::BackendKind::Native,
                browser_image_format: None,
                browser_jpeg_quality: None,
                browser_frame_timeout_ms: None,
                browser_transport: None,
                browser_transport_retries: None,
            },
            props: serde_json::json!({}),
            assets: vec![],
            tracks: vec![],
            events: vec![],
            voice_over_asset_id: None,
        }
    }

    #[test]
    fn project_audio_assets_selects_only_audio_assets() {
        let project = Project {
            version: 1,
            composition: "test".into(),
            settings: rust_motion_project::ProjectSettings {
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
                backend: rust_motion_project::BackendKind::Native,
                browser_image_format: None,
                browser_jpeg_quality: None,
                browser_frame_timeout_ms: None,
                browser_transport: None,
                browser_transport_retries: None,
            },
            props: serde_json::json!({}),
            assets: vec![
                rust_motion_project::AssetRef {
                    id: "music".into(),
                    path: "music.wav".into(),
                    kind: rust_motion_project::AssetKind::Audio,
                    sha256: None,
                },
                rust_motion_project::AssetRef {
                    id: "logo".into(),
                    path: "logo.png".into(),
                    kind: rust_motion_project::AssetKind::Image,
                    sha256: None,
                },
            ],
            tracks: vec![],
            events: vec![],
            voice_over_asset_id: None,
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
                rust_motion_project::AssetRef {
                    id: "local".into(),
                    path: "audio/music.wav".into(),
                    kind: rust_motion_project::AssetKind::Audio,
                    sha256: None,
                },
                rust_motion_project::AssetRef {
                    id: "remote".into(),
                    path: "https://cdn.example/music.wav".into(),
                    kind: rust_motion_project::AssetKind::Audio,
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

    struct PreparedFrameVaryingAudio {
        explicit_keyframes: bool,
    }

    impl rust_motion_composition::Composition for AudioOnlyComposition {
        fn id(&self) -> &str {
            "AudioOnly"
        }

        fn prepare(
            &self,
            _props: &serde_json::Value,
            _context: NativeCompositionContext,
        ) -> Result<Box<dyn rust_motion_composition::PreparedComposition + '_>, CompositionError>
        {
            Ok(Box::new(PreparedAudioOnly))
        }
    }

    impl rust_motion_composition::PreparedComposition for PreparedAudioOnly {
        fn render(&self, _frame: u32) -> Result<rust_motion_rasterizer::Scene, CompositionError> {
            Ok(rust_motion_rasterizer::Scene::new())
        }

        fn audio_tracks(
            &self,
        ) -> Result<Option<Vec<rust_motion_rasterizer::AudioTrack>>, CompositionError> {
            Ok(Some(vec![rust_motion_rasterizer::AudioTrack::new(
                "later.wav",
            )]))
        }
    }

    impl rust_motion_composition::PreparedComposition for PreparedLateSceneAudio {
        fn render(&self, frame: u32) -> Result<rust_motion_rasterizer::Scene, CompositionError> {
            let mut scene = rust_motion_rasterizer::Scene::new();
            if frame >= 30 {
                scene.push(rust_motion_rasterizer::SceneNode::Audio {
                    track: rust_motion_rasterizer::AudioTrack::new("later.wav"),
                });
            }
            Ok(scene)
        }
    }

    impl rust_motion_composition::PreparedComposition for PreparedFrameVaryingAudio {
        fn render(&self, frame: u32) -> Result<rust_motion_rasterizer::Scene, CompositionError> {
            let mut scene = rust_motion_rasterizer::Scene::new();
            let mut track = rust_motion_rasterizer::AudioTrack::new("ducked.wav");
            track.volume = if frame < 30 { 1.0 } else { 0.25 };
            if self.explicit_keyframes {
                track.volume_keyframes = vec![(0.0, 1.0), (1.0, 0.5)];
            }
            scene.push(rust_motion_rasterizer::SceneNode::Audio { track });
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
    fn legacy_audio_volume_changes_become_deduplicated_keyframes() {
        let prepared = PreparedFrameVaryingAudio {
            explicit_keyframes: false,
        };
        let first_scene = prepared.render(0).expect("frame zero renders");

        let tracks = collect_scene_audio_tracks(&prepared, &first_scene, 60, 30.0)
            .expect("scene audio is collected");

        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].volume, 1.0);
        assert_eq!(tracks[0].volume_keyframes, vec![(0.0, 1.0), (1.0, 0.25)]);
    }

    #[test]
    fn explicit_audio_volume_keyframes_override_sampled_volume_changes() {
        let prepared = PreparedFrameVaryingAudio {
            explicit_keyframes: true,
        };
        let first_scene = prepared.render(0).expect("frame zero renders");

        let tracks = collect_scene_audio_tracks(&prepared, &first_scene, 60, 30.0)
            .expect("scene audio is collected");

        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].volume, 1.0);
        assert_eq!(tracks[0].volume_keyframes, vec![(0.0, 1.0), (1.0, 0.5)]);
    }

    #[test]
    fn nested_scene_audio_keeps_rebased_nonzero_start_during_cli_inference() {
        let mut audio = rust_motion_media::SceneAudio::new("voice.wav");
        audio.track.timeline_start = 1.0;
        let composition = rust_motion_composition::SceneEmitterComposition::new(
            "nested-audio",
            rust_motion_composition::SceneSequence::new(60, audio),
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
    time_events: TimeEventSchedule,
}

impl rust_motion_composition::PreparedComposition for ProjectTimelinePrepared<'_> {
    fn render(&self, frame: u32) -> Result<rust_motion_rasterizer::Scene, CompositionError> {
        let mut scene = rust_motion_rasterizer::Scene::new();
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
            let clip_events = self.time_events.within_clip(clip.start, clip.duration);
            let prepared = composition
                .prepare_with_time_events(&clip.props, clip_context, clip_events)
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
    ) -> Result<Option<Vec<rust_motion_rasterizer::AudioTrack>>, CompositionError> {
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
            let clip_events = self.time_events.within_clip(clip.start, clip.duration);
            let prepared = composition
                .prepare_with_time_events(&clip.props, clip_context, clip_events)
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
    ) -> Result<Box<dyn rust_motion_composition::PreparedComposition + '_>, CompositionError> {
        Ok(Box::new(ProjectTimelinePrepared {
            composition: self,
            context,
            time_events: TimeEventSchedule::default(),
        }))
    }

    fn prepare_with_time_events(
        &self,
        _props: &serde_json::Value,
        context: NativeCompositionContext,
        time_events: TimeEventSchedule,
    ) -> Result<Box<dyn rust_motion_composition::PreparedComposition + '_>, CompositionError> {
        Ok(Box::new(ProjectTimelinePrepared {
            composition: self,
            context,
            time_events,
        }))
    }
}

impl rust_motion_composition::PreparedComposition for BrowserPreparedComposition {
    fn render(&self, _frame: u32) -> Result<rust_motion_rasterizer::Scene, CompositionError> {
        Ok(rust_motion_rasterizer::Scene::new())
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
    ) -> Result<Box<dyn rust_motion_composition::PreparedComposition + '_>, CompositionError> {
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
    fn still_format(self) -> Option<rust_motion_rasterizer::StillImageFormat> {
        match self {
            Self::Png => Some(rust_motion_rasterizer::StillImageFormat::Png),
            Self::Jpeg => Some(rust_motion_rasterizer::StillImageFormat::Jpeg),
            Self::Webp => Some(rust_motion_rasterizer::StillImageFormat::WebP),
            _ => None,
        }
    }

    fn video_codec(self) -> Option<rust_motion_rasterizer::VideoCodec> {
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
        "apps/studio-tauri/scripts/three-render-worker.mjs",
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ValidationReport {
    pub ok: bool,
    pub target_kind: String,
    pub target: String,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ToolCheck {
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoders: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct TempDirCheck {
    pub path: String,
    pub writable: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct DoctorReport {
    pub ok: bool,
    pub ffmpeg: ToolCheck,
    pub ffprobe: ToolCheck,
    pub node: ToolCheck,
    pub temp_dir: TempDirCheck,
    pub hw_accel: String,
    pub supported_codecs: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct BackendCapability {
    pub name: String,
    pub tier: String,
    pub status: String,
    pub description: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct CodecCapability {
    pub codec: String,
    pub extensions: Vec<String>,
    pub is_video: bool,
    pub available: bool,
}

fn check_command_output(cmd: &str, args: &[&str]) -> (bool, Option<String>) {
    match std::process::Command::new(cmd).args(args).output() {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let first_line = stdout.lines().next().unwrap_or("").trim().to_string();
            (true, Some(first_line))
        }
        _ => (false, None),
    }
}

fn check_ffmpeg_encoder_supported(name: &str) -> bool {
    std::process::Command::new("ffmpeg")
        .args(["-v", "quiet", "-encoders"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).contains(name))
        .unwrap_or(false)
}

pub fn run_doctor() -> DoctorReport {
    let (ffmpeg_avail, ffmpeg_ver) = check_command_output("ffmpeg", &["-version"]);
    let (ffprobe_avail, ffprobe_ver) = check_command_output("ffprobe", &["-version"]);
    let (node_avail, node_ver) = check_command_output("node", &["--version"]);

    let mut encoders = Vec::new();
    if ffmpeg_avail {
        for enc in ["libx264", "libx265", "libvpx-vp9", "prores", "libsvtav1"] {
            if check_ffmpeg_encoder_supported(enc) {
                encoders.push(enc.to_string());
            }
        }
    }

    let temp_dir_path = std::env::temp_dir();
    let temp_probe_file = temp_dir_path.join(format!(
        "rust_motion_doctor_probe_{}.tmp",
        std::process::id()
    ));
    let temp_writable = std::fs::write(&temp_probe_file, b"ok").is_ok()
        && std::fs::remove_file(&temp_probe_file).is_ok();

    let hw_accel = if cfg!(target_os = "macos") {
        "videotoolbox (hardware acceleration available)".into()
    } else if cfg!(target_os = "windows") {
        "d3d11va / nvenc (hardware acceleration available)".into()
    } else if cfg!(target_os = "linux") {
        "vaapi / nvenc (hardware acceleration available)".into()
    } else {
        "none (cpu rasterizer)".into()
    };

    let supported_codecs = vec![
        "h264".into(),
        "vp9".into(),
        "prores".into(),
        "gif".into(),
        "png".into(),
        "jpeg".into(),
        "webp".into(),
    ];

    let ok = ffmpeg_avail && temp_writable;

    DoctorReport {
        ok,
        ffmpeg: ToolCheck {
            available: ffmpeg_avail,
            path: if ffmpeg_avail {
                Some("ffmpeg".into())
            } else {
                None
            },
            version: ffmpeg_ver,
            encoders: if ffmpeg_avail { Some(encoders) } else { None },
        },
        ffprobe: ToolCheck {
            available: ffprobe_avail,
            path: if ffprobe_avail {
                Some("ffprobe".into())
            } else {
                None
            },
            version: ffprobe_ver,
            encoders: None,
        },
        node: ToolCheck {
            available: node_avail,
            path: if node_avail {
                Some("node".into())
            } else {
                None
            },
            version: node_ver,
            encoders: None,
        },
        temp_dir: TempDirCheck {
            path: temp_dir_path.display().to_string(),
            writable: temp_writable,
        },
        hw_accel,
        supported_codecs,
    }
}

pub fn run_validate(
    project_path: Option<&std::path::Path>,
    composition: Option<&str>,
    script_path: Option<&std::path::Path>,
    props_path: Option<&std::path::Path>,
) -> ValidationReport {
    let mut errors = Vec::new();
    let warnings = Vec::new();
    let mut details = serde_json::json!({});

    if let Some(path) = project_path {
        let target_kind = "project".to_string();
        let target = path.display().to_string();
        match Project::load(path) {
            Ok(project) => {
                if let Some(parent) = path.parent() {
                    if let Err(e) = project.validate_asset_files(parent) {
                        errors.push(format!("Asset validation error: {e}"));
                    }
                }
                if project.settings.duration == 0 {
                    errors.push("Project settings duration must be greater than 0".into());
                }
                if project.settings.fps <= 0.0 {
                    errors.push("Project settings fps must be greater than 0".into());
                }
                if project.settings.width == 0 || project.settings.height == 0 {
                    errors.push("Project settings dimensions must be greater than 0".into());
                }
                details = serde_json::json!({
                    "version": project.version,
                    "composition": project.composition,
                    "width": project.settings.width,
                    "height": project.settings.height,
                    "fps": project.settings.fps,
                    "duration": project.settings.duration,
                    "tracks_count": project.tracks.len(),
                    "assets_count": project.assets.len(),
                    "events_count": project.events.len(),
                });
            }
            Err(e) => {
                errors.push(format!("Project parse error: {e}"));
            }
        }
        let ok = errors.is_empty();
        return ValidationReport {
            ok,
            target_kind,
            target,
            errors,
            warnings,
            details,
        };
    }

    if let Some(path) = script_path {
        let target_kind = "script".to_string();
        let target = path.display().to_string();
        if !path.exists() {
            errors.push(format!("Script file does not exist: {target}"));
        } else {
            #[cfg(feature = "rhai")]
            {
                let engine = rhai::Engine::new();
                if let Err(e) = engine.compile_file(path.to_path_buf()) {
                    errors.push(format!("Script syntax error: {e}"));
                } else {
                    details = serde_json::json!({ "syntax_valid": true });
                }
            }
        }
        if let Some(p) = props_path {
            match std::fs::read_to_string(p) {
                Ok(content) => {
                    if let Err(e) = serde_json::from_str::<serde_json::Value>(&content) {
                        errors.push(format!("Props JSON error: {e}"));
                    }
                }
                Err(e) => errors.push(format!("Failed to read props file: {e}")),
            }
        }
        let ok = errors.is_empty();
        return ValidationReport {
            ok,
            target_kind,
            target,
            errors,
            warnings,
            details,
        };
    }

    if let Some(comp_id) = composition {
        let target_kind = "composition".to_string();
        let target = comp_id.to_string();
        let registry = built_in_registry();
        if registry.get(comp_id).is_err() {
            errors.push(format!(
                "Composition '{}' not found in built-in registry",
                comp_id
            ));
        } else {
            details = serde_json::json!({ "registered": true });
        }
        if let Some(p) = props_path {
            match std::fs::read_to_string(p) {
                Ok(content) => {
                    if let Err(e) = serde_json::from_str::<serde_json::Value>(&content) {
                        errors.push(format!("Props JSON error: {e}"));
                    }
                }
                Err(e) => errors.push(format!("Failed to read props file: {e}")),
            }
        }
        let ok = errors.is_empty();
        return ValidationReport {
            ok,
            target_kind,
            target,
            errors,
            warnings,
            details,
        };
    }

    ValidationReport {
        ok: false,
        target_kind: "none".into(),
        target: "none".into(),
        errors: vec![
            "No target specified for validation (specify --project, --script, or --composition)"
                .into(),
        ],
        warnings: vec![],
        details: serde_json::json!({}),
    }
}

pub fn run_inspect_backends() -> Vec<BackendCapability> {
    vec![
        BackendCapability {
            name: "native".into(),
            tier: "reference".into(),
            status: "supported".into(),
            description: "High-performance browser-free CPU rasterizer with deterministic frame evaluation (reference backend)".into(),
        },
        BackendCapability {
            name: "gpu".into(),
            tier: "experimental".into(),
            status: "experimental".into(),
            description: "Hardware-accelerated WGPU pipeline with GPU texture cache and compositor (driver-dependent)".into(),
        },
        BackendCapability {
            name: "browser".into(),
            tier: "optional".into(),
            status: "supported with fallback".into(),
            description: "Headless WebCodecs and DOM rendering via Node/Chromium worker".into(),
        },
    ]
}

pub fn run_inspect_codecs() -> Vec<CodecCapability> {
    let (ffmpeg_avail, _) = check_command_output("ffmpeg", &["-version"]);
    vec![
        CodecCapability {
            codec: "h264".into(),
            extensions: vec!["mp4".into()],
            is_video: true,
            available: ffmpeg_avail && check_ffmpeg_encoder_supported("libx264"),
        },
        CodecCapability {
            codec: "hevc".into(),
            extensions: vec!["mp4".into(), "mov".into()],
            is_video: true,
            available: ffmpeg_avail
                && (check_ffmpeg_encoder_supported("libx265")
                    || check_ffmpeg_encoder_supported("hevc")),
        },
        CodecCapability {
            codec: "vp9".into(),
            extensions: vec!["webm".into()],
            is_video: true,
            available: ffmpeg_avail && check_ffmpeg_encoder_supported("libvpx-vp9"),
        },
        CodecCapability {
            codec: "av1".into(),
            extensions: vec!["webm".into(), "mp4".into()],
            is_video: true,
            available: ffmpeg_avail && check_ffmpeg_encoder_supported("libsvtav1"),
        },
        CodecCapability {
            codec: "prores".into(),
            extensions: vec!["mov".into()],
            is_video: true,
            available: ffmpeg_avail && check_ffmpeg_encoder_supported("prores"),
        },
        CodecCapability {
            codec: "gif".into(),
            extensions: vec!["gif".into()],
            is_video: true,
            available: ffmpeg_avail,
        },
        CodecCapability {
            codec: "png".into(),
            extensions: vec!["png".into()],
            is_video: false,
            available: true,
        },
        CodecCapability {
            codec: "jpeg".into(),
            extensions: vec!["jpg".into(), "jpeg".into()],
            is_video: false,
            available: true,
        },
        CodecCapability {
            codec: "webp".into(),
            extensions: vec!["webp".into()],
            is_video: false,
            available: true,
        },
    ]
}

pub fn run_inspect_compositions() -> Vec<String> {
    built_in_registry()
        .ids()
        .into_iter()
        .map(str::to_owned)
        .collect()
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

fn uses_project_timeline(backend: RenderBackend, has_tracks: bool) -> bool {
    has_tracks && matches!(backend, RenderBackend::Native | RenderBackend::Gpu)
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
