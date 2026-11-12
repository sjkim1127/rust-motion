//! Shims adapting project timelines and browser workers to the composition trait.

use crate::args::RenderBackend;
use crate::composition::{
    Composition, CompositionError, CompositionRegistry, NativeCompositionContext,
    PreparedComposition, TimeEventSchedule,
};
use rust_motion_project::Clip;

/// Composition wrapper that evaluates multi-clip timelines from project files.
pub(crate) struct ProjectTimelineComposition {
    pub(crate) id: String,
    pub(crate) clips: Vec<Clip>,
    pub(crate) registry: CompositionRegistry,
}

pub(crate) struct ProjectTimelinePrepared<'a> {
    pub(crate) composition: &'a ProjectTimelineComposition,
    pub(crate) context: NativeCompositionContext,
    pub(crate) time_events: TimeEventSchedule,
}

impl PreparedComposition for ProjectTimelinePrepared<'_> {
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
    ) -> Result<Box<dyn PreparedComposition + '_>, CompositionError> {
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
    ) -> Result<Box<dyn PreparedComposition + '_>, CompositionError> {
        Ok(Box::new(ProjectTimelinePrepared {
            composition: self,
            context,
            time_events,
        }))
    }
}

/// Browser workers own the actual composition runtime. This placeholder keeps
/// the shared render pipeline's native validation stage from rejecting a
/// browser-only composition ID that is not registered in the Rust registry.
pub(crate) struct BrowserComposition {
    pub(crate) id: String,
}

pub(crate) struct BrowserPreparedComposition;

impl PreparedComposition for BrowserPreparedComposition {
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
    ) -> Result<Box<dyn PreparedComposition + '_>, CompositionError> {
        Ok(Box::new(BrowserPreparedComposition))
    }
}

pub(crate) fn uses_project_timeline(backend: RenderBackend, has_tracks: bool) -> bool {
    has_tracks && matches!(backend, RenderBackend::Native | RenderBackend::Gpu)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::collect_scene_audio_tracks;
    use crate::composition::built_in_registry;

    struct AudioOnlyComposition;

    struct PreparedAudioOnly;

    struct PreparedLateSceneAudio;

    struct PreparedFrameVaryingAudio {
        explicit_keyframes: bool,
    }

    impl Composition for AudioOnlyComposition {
        fn id(&self) -> &str {
            "AudioOnly"
        }

        fn prepare(
            &self,
            _props: &serde_json::Value,
            _context: NativeCompositionContext,
        ) -> Result<Box<dyn PreparedComposition + '_>, CompositionError> {
            Ok(Box::new(PreparedAudioOnly))
        }
    }

    impl PreparedComposition for PreparedAudioOnly {
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

    impl PreparedComposition for PreparedLateSceneAudio {
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

    impl PreparedComposition for PreparedFrameVaryingAudio {
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
