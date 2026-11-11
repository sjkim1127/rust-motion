//! Deterministic Scene Play/Wait Authoring API (#49) & Linked Audio/Video Media Strip Engine (#51).
//!
//! Provides:
//! - Manim-inspired declarative `SceneDirector` (`add`, `remove`, `play`, `play_parallel`,
//!   `play_sequential`, `wait`) that compiles into an immutable, random-access `CompiledSceneSchedule`
//!   guaranteeing identical frame evaluation in ascending, descending, or concurrent order (#49).
//! - First-class `LinkedMediaStrip` with synchronized video/audio source in/out, timeline placement,
//!   playback-rate retiming, independent visual opacity vs. audio gain/mute, and waveform peak generation (#51).

use crate::scene::{AudioTrack, Color, ImageFit, Scene, SceneNode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Property mutation animated by `SceneDirector::play`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ObjectAnimation {
    /// Fade opacity from `from` to `to` over the animation span.
    Fade {
        object_id: String,
        from: f32,
        to: f32,
    },
    /// Translate `(x, y)` position to `(target_x, target_y)`.
    MoveTo {
        object_id: String,
        target_x: f32,
        target_y: f32,
    },
    /// Resize `(w, h)` or radius/font_size by `scale_factor`.
    ScaleTo {
        object_id: String,
        scale_factor: f32,
    },
    /// Transition fill/text color to `target_color`.
    ColorTo {
        object_id: String,
        target_color: Color,
    },
}

impl ObjectAnimation {
    pub fn object_id(&self) -> &str {
        match self {
            Self::Fade { object_id, .. }
            | Self::MoveTo { object_id, .. }
            | Self::ScaleTo { object_id, .. }
            | Self::ColorTo { object_id, .. } => object_id,
        }
    }
}

#[derive(Clone, Debug)]
struct ObjectTimelineRecord {
    initial_node: SceneNode,
    added_at_frame: u32,
    removed_at_frame: Option<u32>,
}

#[derive(Clone, Debug)]
struct ScheduledSpan {
    start_frame: u32,
    end_frame: u32,
    anim: ObjectAnimation,
}

/// Authoring builder for Manim-style `add` / `play` / `wait` / `remove` scene construction (#49).
#[derive(Clone, Debug, Default)]
pub struct SceneDirector {
    cursor_frame: u32,
    objects: BTreeMap<String, ObjectTimelineRecord>,
    insertion_order: Vec<String>,
    spans: Vec<ScheduledSpan>,
}

impl SceneDirector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current_frame(&self) -> u32 {
        self.cursor_frame
    }

    /// Add an addressable scene object at the current timeline cursor.
    pub fn add(&mut self, id: impl Into<String>, node: SceneNode) -> &mut Self {
        let id = id.into();
        if !self.insertion_order.contains(&id) {
            self.insertion_order.push(id.clone());
        }
        self.objects.insert(
            id,
            ObjectTimelineRecord {
                initial_node: node,
                added_at_frame: self.cursor_frame,
                removed_at_frame: None,
            },
        );
        self
    }

    /// Remove an addressable scene object at the current timeline cursor.
    pub fn remove(&mut self, id: &str) -> &mut Self {
        if let Some(obj) = self.objects.get_mut(id) {
            obj.removed_at_frame = Some(self.cursor_frame);
        }
        self
    }

    /// Advance the timeline cursor by `duration_frames` without scheduling new animations.
    pub fn wait(&mut self, duration_frames: u32) -> &mut Self {
        self.cursor_frame = self.cursor_frame.saturating_add(duration_frames);
        self
    }

    /// Schedule a single animation over `duration_frames` and advance the cursor.
    pub fn play(&mut self, anim: ObjectAnimation, duration_frames: u32) -> &mut Self {
        let d = duration_frames.max(1);
        self.spans.push(ScheduledSpan {
            start_frame: self.cursor_frame,
            end_frame: self.cursor_frame + d,
            anim,
        });
        self.cursor_frame += d;
        self
    }

    /// Schedule multiple animations in parallel over `duration_frames` and advance the cursor.
    pub fn play_parallel(
        &mut self,
        anims: Vec<ObjectAnimation>,
        duration_frames: u32,
    ) -> &mut Self {
        let d = duration_frames.max(1);
        for anim in anims {
            self.spans.push(ScheduledSpan {
                start_frame: self.cursor_frame,
                end_frame: self.cursor_frame + d,
                anim,
            });
        }
        self.cursor_frame += d;
        self
    }

    /// Schedule a sequence of `(ObjectAnimation, duration_frames)` one after another.
    pub fn play_sequential(&mut self, steps: Vec<(ObjectAnimation, u32)>) -> &mut Self {
        for (anim, dur) in steps {
            self.play(anim, dur);
        }
        self
    }

    /// Compile the authored steps into an immutable, thread-safe, random-access `CompiledSceneSchedule`.
    pub fn compile(&self) -> CompiledSceneSchedule {
        CompiledSceneSchedule {
            total_frames: self.cursor_frame,
            objects: self.objects.clone(),
            insertion_order: self.insertion_order.clone(),
            spans: self.spans.clone(),
        }
    }
}

/// Immutable compiled schedule that evaluates any frame deterministically without state mutation (#49).
#[derive(Clone, Debug)]
pub struct CompiledSceneSchedule {
    pub total_frames: u32,
    objects: BTreeMap<String, ObjectTimelineRecord>,
    insertion_order: Vec<String>,
    spans: Vec<ScheduledSpan>,
}

impl CompiledSceneSchedule {
    /// Evaluate the exact `Scene` at `frame`. Guaranteed deterministic regardless of call order.
    pub fn evaluate_frame(&self, frame: u32) -> Scene {
        let mut scene = Scene::new();

        for id in &self.insertion_order {
            let Some(rec) = self.objects.get(id) else {
                continue;
            };
            if frame < rec.added_at_frame {
                continue;
            }
            if let Some(rem) = rec.removed_at_frame {
                if frame >= rem {
                    continue;
                }
            }

            let mut node = rec.initial_node.clone();
            let mut opacity = 1.0f32;

            // Apply all scheduled spans for this object in chronological order up to `frame`
            for span in &self.spans {
                if span.anim.object_id() != id || frame < span.start_frame {
                    continue;
                }
                let raw_t = if frame >= span.end_frame {
                    1.0
                } else {
                    (frame - span.start_frame) as f32
                        / (span.end_frame - span.start_frame).max(1) as f32
                };
                // Smoothstep easing for natural motion
                let t = raw_t * raw_t * (3.0 - 2.0 * raw_t);

                match &span.anim {
                    ObjectAnimation::Fade { from, to, .. } => {
                        opacity = from + (to - from) * t;
                    }
                    ObjectAnimation::MoveTo {
                        target_x, target_y, ..
                    } => match &mut node {
                        SceneNode::Rect { x, y, .. } | SceneNode::Text { x, y, .. } => {
                            *x = *x + (*target_x - *x) * t;
                            *y = *y + (*target_y - *y) * t;
                        }
                        SceneNode::Circle { cx, cy, .. } => {
                            *cx = *cx + (*target_x - *cx) * t;
                            *cy = *cy + (*target_y - *cy) * t;
                        }
                        _ => {}
                    },
                    ObjectAnimation::ScaleTo { scale_factor, .. } => {
                        let s = 1.0 + (*scale_factor - 1.0) * t;
                        match &mut node {
                            SceneNode::Rect { w, h, .. } => {
                                *w *= s;
                                *h *= s;
                            }
                            SceneNode::Circle { r, .. } => {
                                *r *= s;
                            }
                            SceneNode::Text { font_size, .. } => {
                                *font_size *= s;
                            }
                            _ => {}
                        }
                    }
                    ObjectAnimation::ColorTo { target_color, .. } => {
                        let lerp_c = |c: &mut Color| {
                            c.r = (c.r as f32 + (target_color.r as f32 - c.r as f32) * t).round()
                                as u8;
                            c.g = (c.g as f32 + (target_color.g as f32 - c.g as f32) * t).round()
                                as u8;
                            c.b = (c.b as f32 + (target_color.b as f32 - c.b as f32) * t).round()
                                as u8;
                            c.a = (c.a as f32 + (target_color.a as f32 - c.a as f32) * t).round()
                                as u8;
                        };
                        match &mut node {
                            SceneNode::Rect { fill, .. } | SceneNode::Circle { fill, .. } => {
                                lerp_c(fill);
                            }
                            SceneNode::Text { color, .. } => {
                                lerp_c(color);
                            }
                            _ => {}
                        }
                    }
                }
            }

            if opacity <= 0.001 {
                continue;
            }
            if (opacity - 1.0).abs() > 1e-4 {
                match &mut node {
                    SceneNode::Rect { fill, .. } | SceneNode::Circle { fill, .. } => {
                        fill.a = ((fill.a as f32) * opacity).round() as u8;
                    }
                    SceneNode::Text { color, .. } => {
                        color.a = ((color.a as f32) * opacity).round() as u8;
                    }
                    _ => {}
                }
            }

            scene.push(node);
        }

        scene
    }
}

/// Stream composition of an imported media asset (#51).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaStreamKind {
    VideoWithAudio,
    VideoOnly,
    AudioOnly,
}

/// Linked audio/video timeline strip where trim, offset, duration, and playback-rate retiming
/// stay strictly synchronized between video frames and embedded audio samples (#51).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinkedMediaStrip {
    pub id: String,
    pub src: String,
    pub stream_kind: MediaStreamKind,
    pub source_in_frame: u32,
    pub source_out_frame: u32,
    pub timeline_start_frame: u32,
    pub duration_frames: u32,
    pub playback_rate: f64,
    pub visual_opacity: f32,
    pub audio_gain_db: f32,
    pub audio_muted: bool,
    pub width: f32,
    pub height: f32,
}

impl LinkedMediaStrip {
    pub fn new(
        id: impl Into<String>,
        src: impl Into<String>,
        stream_kind: MediaStreamKind,
        timeline_start_frame: u32,
        duration_frames: u32,
        width: f32,
        height: f32,
    ) -> Result<Self, String> {
        if duration_frames == 0 {
            return Err("Media strip duration must be at least 1 frame".into());
        }
        Ok(Self {
            id: id.into(),
            src: src.into(),
            stream_kind,
            source_in_frame: 0,
            source_out_frame: duration_frames,
            timeline_start_frame,
            duration_frames,
            playback_rate: 1.0,
            visual_opacity: 1.0,
            audio_gain_db: 0.0,
            audio_muted: false,
            width,
            height,
        })
    }

    /// Trim the start (`in_point`) of the linked strip by `delta_frames`, updating both
    /// `source_in_frame`, `timeline_start_frame`, and `duration_frames` in lockstep.
    pub fn trim_in(&mut self, delta_frames: i32) -> Result<(), String> {
        let new_in = (self.source_in_frame as i64 + delta_frames as i64).max(0) as u32;
        let new_start = (self.timeline_start_frame as i64 + delta_frames as i64).max(0) as u32;
        let new_dur = self.duration_frames as i64 - delta_frames as i64;
        if new_dur < 1 {
            return Err("Trim in would reduce strip duration below 1 frame".into());
        }
        self.source_in_frame = new_in;
        self.timeline_start_frame = new_start;
        self.duration_frames = new_dur as u32;
        Ok(())
    }

    /// Move the linked strip to a new `timeline_start_frame`, keeping source in/out intact.
    pub fn move_to(&mut self, new_timeline_start: u32) {
        self.timeline_start_frame = new_timeline_start;
    }

    /// Retime the strip with `new_rate`, scaling `duration_frames` inversely so the same
    /// source range `[source_in_frame, source_out_frame)` plays at the new rate.
    pub fn set_playback_rate(&mut self, new_rate: f64) -> Result<(), String> {
        if new_rate <= 0.05 || new_rate > 16.0 {
            return Err(format!("Unsupported playback rate: {new_rate}"));
        }
        let source_span =
            (self.source_out_frame.saturating_sub(self.source_in_frame)).max(1) as f64;
        self.playback_rate = new_rate;
        self.duration_frames = (source_span / new_rate).round().max(1.0) as u32;
        Ok(())
    }

    /// Map a timeline `frame` (at `fps`) to the exact source media timestamp in seconds
    /// shared by both video frame extraction and audio stream playback.
    pub fn source_time_sec_at(&self, timeline_frame: u32, fps: f64) -> Option<f64> {
        if timeline_frame < self.timeline_start_frame
            || timeline_frame >= self.timeline_start_frame + self.duration_frames
        {
            return None;
        }
        let elapsed_frames = (timeline_frame - self.timeline_start_frame) as f64;
        let source_frame = self.source_in_frame as f64 + elapsed_frames * self.playback_rate;
        Some(source_frame / fps.max(1.0))
    }

    /// Linear audio volume multiplier derived from `audio_gain_db` and `audio_muted`
    /// (completely independent of `visual_opacity`).
    pub fn effective_audio_volume(&self) -> f32 {
        if self.audio_muted || self.stream_kind == MediaStreamKind::VideoOnly {
            0.0
        } else {
            10f32.powf(self.audio_gain_db / 20.0).clamp(0.0, 4.0)
        }
    }

    /// Emit the linked video node and/or audio track into `scene` for `timeline_frame`.
    pub fn emit_to_scene(&self, scene: &mut Scene, timeline_frame: u32, fps: f64) {
        let Some(src_time) = self.source_time_sec_at(timeline_frame, fps) else {
            return;
        };

        if matches!(
            self.stream_kind,
            MediaStreamKind::VideoWithAudio | MediaStreamKind::VideoOnly
        ) && self.visual_opacity > 0.001
        {
            scene.push(SceneNode::Video {
                src: self.src.clone(),
                time: src_time,
                looped: false,
                x: 0.0,
                y: 0.0,
                w: self.width,
                h: self.height,
                fit: ImageFit::Cover,
                opacity: self.visual_opacity.clamp(0.0, 1.0),
                interpolation: crate::time_remap::FrameInterpolationMode::default(),
                time_remap: None,
            });
        }

        if matches!(
            self.stream_kind,
            MediaStreamKind::VideoWithAudio | MediaStreamKind::AudioOnly
        ) {
            scene.push(SceneNode::Audio {
                track: AudioTrack {
                    src: self.src.clone(),
                    start_from: self.source_in_frame as f64 / fps.max(1.0),
                    timeline_start: self.timeline_start_frame as f64 / fps.max(1.0),
                    duration: Some(self.duration_frames as f64 / fps.max(1.0)),
                    volume: self.effective_audio_volume() as f64,
                    playback_rate: self.playback_rate,
                    looped: false,
                    volume_keyframes: Vec::new(),
                },
            });
        }
    }

    /// Compute normalized waveform peak bins (`0.0..=1.0`) for Studio timeline visualization.
    pub fn compute_waveform_peaks(&self, pcm_samples: &[f32], num_bins: usize) -> Vec<f32> {
        if num_bins == 0 || pcm_samples.is_empty() {
            return vec![0.0; num_bins];
        }
        let mut peaks = Vec::with_capacity(num_bins);
        let gain = self.effective_audio_volume();
        for b in 0..num_bins {
            let s0 = (b * pcm_samples.len()) / num_bins;
            let s1 = (((b + 1) * pcm_samples.len()) / num_bins)
                .max(s0 + 1)
                .min(pcm_samples.len());
            let max_abs = pcm_samples[s0..s1]
                .iter()
                .fold(0.0f32, |acc, &v| acc.max(v.abs()));
            peaks.push((max_abs * gain).clamp(0.0, 1.0));
        }
        peaks
    }
}
