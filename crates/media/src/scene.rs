//! Native Scene emitters matching the Dioxus media components.

use crate::img::ImageFit;
use dioxuscut_composition::{CompositionError, SceneEmitter, SceneFrameContext};
use dioxuscut_rasterizer::{
    gif_cache::LoopBehavior, AudioTrack, ImageFit as SceneImageFit, Scene, SceneNode,
};
use serde_json::Value;

impl From<ImageFit> for SceneImageFit {
    fn from(fit: ImageFit) -> Self {
        match fit {
            ImageFit::Cover => Self::Cover,
            ImageFit::Contain => Self::Contain,
            ImageFit::Fill => Self::Fill,
            ImageFit::None => Self::None,
            ImageFit::ScaleDown => Self::ScaleDown,
        }
    }
}

/// Native counterpart of [`crate::Img`].
#[derive(Debug, Clone, PartialEq)]
pub struct SceneImage {
    pub src: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub fit: ImageFit,
    pub opacity: f32,
}

impl SceneImage {
    pub fn new(src: impl Into<String>, width: f32, height: f32) -> Self {
        Self {
            src: src.into(),
            x: 0.0,
            y: 0.0,
            width,
            height,
            fit: ImageFit::Cover,
            opacity: 1.0,
        }
    }
}

impl SceneEmitter for SceneImage {
    fn emit(
        &self,
        _context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        scene.push(SceneNode::Image {
            src: self.src.clone(),
            x: self.x,
            y: self.y,
            w: self.width.max(0.0),
            h: self.height.max(0.0),
            fit: self.fit.into(),
            opacity: self.opacity.clamp(0.0, 1.0),
        });
        Ok(())
    }
}

/// Native counterpart of [`crate::Gif`] / Remotion's `AnimatedImage`.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneGif {
    pub src: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub fit: ImageFit,
    pub opacity: f32,
    pub timeline_start: f64,
    pub duration: Option<f64>,
    pub playback_rate: f32,
    pub loop_behavior: LoopBehavior,
}

impl SceneGif {
    pub fn new(src: impl Into<String>, width: f32, height: f32) -> Self {
        Self {
            src: src.into(),
            x: 0.0,
            y: 0.0,
            width,
            height,
            fit: ImageFit::Cover,
            opacity: 1.0,
            timeline_start: 0.0,
            duration: None,
            playback_rate: 1.0,
            loop_behavior: LoopBehavior::Loop,
        }
    }
}

impl SceneEmitter for SceneGif {
    fn emit(
        &self,
        context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let elapsed = context.time_secs() - self.timeline_start;
        if elapsed < 0.0 || self.duration.is_some_and(|duration| elapsed >= duration) {
            return Ok(());
        }
        if !self.playback_rate.is_finite() || self.playback_rate <= 0.0 {
            return Err(CompositionError::render(
                context.global_frame,
                "GIF playback rate must be finite and greater than zero",
            ));
        }
        scene.push(SceneNode::Gif {
            src: self.src.clone(),
            time: elapsed,
            x: self.x,
            y: self.y,
            w: self.width.max(0.0),
            h: self.height.max(0.0),
            playback_rate: self.playback_rate,
            loop_behavior: self.loop_behavior,
            fit: self.fit.into(),
            opacity: self.opacity.clamp(0.0, 1.0),
        });
        Ok(())
    }
}

/// Timeline-aware native counterpart of [`crate::Video`].
#[derive(Debug, Clone, PartialEq)]
pub struct SceneVideo {
    pub src: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub fit: ImageFit,
    pub opacity: f32,
    pub start_from: f64,
    pub timeline_start: f64,
    pub duration: Option<f64>,
    pub playback_rate: f64,
    pub looped: bool,
}

impl SceneVideo {
    pub fn new(src: impl Into<String>, width: f32, height: f32) -> Self {
        Self {
            src: src.into(),
            x: 0.0,
            y: 0.0,
            width,
            height,
            fit: ImageFit::Cover,
            opacity: 1.0,
            start_from: 0.0,
            timeline_start: 0.0,
            duration: None,
            playback_rate: 1.0,
            looped: false,
        }
    }
}

impl SceneEmitter for SceneVideo {
    fn emit(
        &self,
        context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let elapsed = context.time_secs() - self.timeline_start;
        if elapsed < 0.0 || self.duration.is_some_and(|duration| elapsed >= duration) {
            return Ok(());
        }
        if !self.playback_rate.is_finite() || self.playback_rate <= 0.0 {
            return Err(CompositionError::render(
                context.global_frame,
                "video playback rate must be finite and greater than zero",
            ));
        }
        scene.push(SceneNode::Video {
            src: self.src.clone(),
            time: self.start_from.max(0.0) + elapsed * self.playback_rate,
            looped: self.looped,
            x: self.x,
            y: self.y,
            w: self.width.max(0.0),
            h: self.height.max(0.0),
            fit: self.fit.into(),
            opacity: self.opacity.clamp(0.0, 1.0),
        });
        Ok(())
    }
}

/// Native counterpart of [`crate::Audio`].
///
/// `track.timeline_start` is relative to this emitter's local timeline and is
/// rebased through enclosing timeline emitters such as [`SceneSequence`].
#[derive(Debug, Clone, PartialEq)]
pub struct SceneAudio {
    pub track: AudioTrack,
}

impl SceneAudio {
    pub fn new(src: impl Into<String>) -> Self {
        Self {
            track: AudioTrack::new(src),
        }
    }
}

impl SceneEmitter for SceneAudio {
    fn emit(
        &self,
        context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let mut track = self.track.clone();
        track.timeline_start += context.global_time_secs() - context.time_secs();
        scene.push(SceneNode::Audio { track });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dioxuscut_composition::{
        NativeComposition, NativeCompositionContext, SceneEmitterComposition, SceneSequence,
        SceneStack,
    };

    fn context() -> NativeCompositionContext {
        NativeCompositionContext {
            width: 320,
            height: 180,
            fps: 10.0,
            duration_in_frames: 100,
        }
    }

    #[test]
    fn video_emitter_applies_trim_rate_and_active_window() {
        let mut video = SceneVideo::new("clip.mp4", 320.0, 180.0);
        video.start_from = 2.0;
        video.timeline_start = 1.0;
        video.duration = Some(2.0);
        video.playback_rate = 1.5;
        let composition = SceneEmitterComposition::new("video", video);

        assert!(composition
            .render(5, &Value::Null, context())
            .unwrap()
            .nodes
            .is_empty());
        let active = composition.render(20, &Value::Null, context()).unwrap();
        assert!(matches!(
            &active.nodes[0],
            SceneNode::Video { time, .. } if (*time - 3.5).abs() < f64::EPSILON
        ));
        assert!(composition
            .render(30, &Value::Null, context())
            .unwrap()
            .nodes
            .is_empty());
    }

    #[test]
    fn image_and_audio_emit_shared_scene_nodes() {
        let image = SceneImage::new("card.png", 100.0, 50.0);
        let mut image_scene = Scene::new();
        image
            .emit(
                SceneFrameContext::new(0, context()),
                &Value::Null,
                &mut image_scene,
            )
            .unwrap();
        assert!(matches!(image_scene.nodes[0], SceneNode::Image { .. }));

        let audio = SceneAudio::new("voice.wav");
        let mut audio_scene = Scene::new();
        audio
            .emit(
                SceneFrameContext::new(0, context()),
                &Value::Null,
                &mut audio_scene,
            )
            .unwrap();
        assert_eq!(audio_scene.audio_tracks()[0].src, "voice.wav");
    }

    #[test]
    fn audio_and_video_share_nested_sequence_timeline_origin() {
        let mut video = SceneVideo::new("clip.mp4", 320.0, 180.0);
        video.timeline_start = 1.0;
        let mut audio = SceneAudio::new("clip.mp4");
        audio.track.timeline_start = 1.0;

        let composition = SceneEmitterComposition::new(
            "synchronized-media",
            SceneSequence::new(60, SceneStack::new().with(video).with(audio)),
        );
        let mut timeline = context();
        timeline.fps = 30.0;
        timeline.duration_in_frames = 120;

        let before_start = composition.render(89, &Value::Null, timeline).unwrap();
        assert!(!before_start
            .nodes
            .iter()
            .any(|node| matches!(node, SceneNode::Video { .. })));

        let at_start = composition.render(90, &Value::Null, timeline).unwrap();
        let video_time = at_start.nodes.iter().find_map(|node| match node {
            SceneNode::Video { time, .. } => Some(*time),
            _ => None,
        });
        assert_eq!(video_time, Some(0.0));
        let audio_start = at_start.audio_tracks()[0].timeline_start;
        assert!((audio_start - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn gif_emitter_applies_timeline_and_playback_contract() {
        let mut gif = SceneGif::new("clip.gif", 100.0, 50.0);
        gif.timeline_start = 1.0;
        gif.duration = Some(2.0);
        gif.playback_rate = 1.5;
        let composition = SceneEmitterComposition::new("gif", gif);
        let active = composition.render(20, &Value::Null, context()).unwrap();
        assert!(matches!(
            &active.nodes[0],
            SceneNode::Gif { time, playback_rate, .. }
                if (*time - 1.0).abs() < f64::EPSILON && (*playback_rate - 1.5).abs() < f32::EPSILON
        ));
        assert!(composition
            .render(5, &Value::Null, context())
            .unwrap()
            .nodes
            .is_empty());
        assert!(composition
            .render(30, &Value::Null, context())
            .unwrap()
            .nodes
            .is_empty());
    }
}
