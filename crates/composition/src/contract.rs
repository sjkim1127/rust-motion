//! Native composition contract: render context, error type and the composition traits.

use crate::{format_timecode_frames, FrameRate, Tick, TimeEventSchedule};
use rust_motion_rasterizer::{AudioTrack, Scene};
use serde_json::Value;
use thiserror::Error;

/// Immutable render parameters supplied to every native composition frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeCompositionContext {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_in_frames: u32,
}

impl NativeCompositionContext {
    /// Normalized timeline progress in the inclusive range `0.0..=1.0`.
    pub fn progress(self, frame: u32) -> f32 {
        let last_frame = self.duration_in_frames.saturating_sub(1).max(1);
        (frame.min(last_frame) as f32 / last_frame as f32).clamp(0.0, 1.0)
    }

    /// Exact rational frame rate for this composition.
    pub fn frame_rate(self) -> FrameRate {
        FrameRate::from_f64(self.fps)
    }

    /// Total duration in exact zero-drift integer ticks.
    pub fn duration_ticks(self) -> Tick {
        self.frame_rate().tick_of(self.duration_in_frames as i64)
    }

    /// Convert a composition frame number to exact integer ticks.
    pub fn frame_to_tick(self, frame: u32) -> Tick {
        self.frame_rate().tick_of(frame as i64)
    }

    /// Convert exact ticks to composition frame number (floored).
    pub fn tick_to_frame(self, tick: Tick) -> u32 {
        self.frame_rate().frame_at(tick).max(0) as u32
    }

    /// Format a frame number as SMPTE timecode string (e.g. `HH:MM:SS:FF` or `HH;MM;SS;FF`).
    pub fn format_timecode(self, frame: u32, drop_frame: bool) -> String {
        format_timecode_frames(frame as i64, self.frame_rate(), drop_frame)
    }
}

/// Errors produced while preparing or rendering a composition.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CompositionError {
    #[error("Failed to prepare composition: {0}")]
    Prepare(String),
    #[error("Failed to render frame {frame}: {reason}")]
    Render { frame: u32, reason: String },
}

impl CompositionError {
    pub fn render(frame: u32, reason: impl Into<String>) -> Self {
        Self::Render {
            frame,
            reason: reason.into(),
        }
    }
}

/// A composition instance prepared once for a complete render job.
///
/// Implementations may cache parsed input, compiled scripts, and other
/// immutable state here. `render` can be called concurrently for different
/// frames.
pub trait PreparedComposition: Send + Sync {
    fn render(&self, frame: u32) -> Result<Scene, CompositionError>;

    /// Return audio tracks for the complete render, when the composition has
    /// an explicit timeline-level audio contract.
    ///
    /// `None` asks the host to derive tracks from rendered scenes. The CLI
    /// scans the complete frame range before encoding so it can find tracks
    /// first emitted after frame zero; this requires an extra render pass.
    /// Implementations that can describe the complete audio timeline should
    /// return `Some`, including tracks whose timing changes over time, to
    /// avoid that scan.
    fn audio_tracks(&self) -> Result<Option<Vec<AudioTrack>>, CompositionError> {
        Ok(None)
    }
}

/// General composition contract used by the registry.
pub trait Composition: Send + Sync {
    fn id(&self) -> &str;

    fn prepare(
        &self,
        props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Box<dyn PreparedComposition + '_>, CompositionError>;

    /// Prepare against the project's immutable named-event schedule.
    /// Existing compositions remain compatible and can opt in by overriding
    /// this method or by implementing [`NativeComposition::render_with_time_events`].
    fn prepare_with_time_events(
        &self,
        props: &Value,
        context: NativeCompositionContext,
        _events: TimeEventSchedule,
    ) -> Result<Box<dyn PreparedComposition + '_>, CompositionError> {
        self.prepare(props, context)
    }
}

/// A browser-free Rust composition that produces one rasterizer scene per frame.
///
/// Applications can implement this trait, register implementations in a
/// [`CompositionRegistry`], and call `execute_render_command_with_registry`.
pub trait NativeComposition: Send + Sync {
    fn id(&self) -> &str;

    fn render(
        &self,
        frame: u32,
        props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Scene, CompositionError>;

    /// Render with a prepared named-event schedule. The default preserves
    /// existing frame-based compositions unchanged.
    fn render_with_time_events(
        &self,
        frame: u32,
        props: &Value,
        context: NativeCompositionContext,
        _events: &TimeEventSchedule,
    ) -> Result<Scene, CompositionError> {
        self.render(frame, props, context)
    }
}

struct PreparedNativeComposition<'a, C> {
    composition: &'a C,
    props: Value,
    context: NativeCompositionContext,
    events: TimeEventSchedule,
}

impl<C> PreparedComposition for PreparedNativeComposition<'_, C>
where
    C: NativeComposition,
{
    fn render(&self, frame: u32) -> Result<Scene, CompositionError> {
        self.composition
            .render_with_time_events(frame, &self.props, self.context, &self.events)
    }
}

impl<C> Composition for C
where
    C: NativeComposition,
{
    fn id(&self) -> &str {
        NativeComposition::id(self)
    }

    fn prepare(
        &self,
        props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Box<dyn PreparedComposition + '_>, CompositionError> {
        Ok(Box::new(PreparedNativeComposition {
            composition: self,
            props: props.clone(),
            context,
            events: TimeEventSchedule::default(),
        }))
    }

    fn prepare_with_time_events(
        &self,
        props: &Value,
        context: NativeCompositionContext,
        events: TimeEventSchedule,
    ) -> Result<Box<dyn PreparedComposition + '_>, CompositionError> {
        Ok(Box::new(PreparedNativeComposition {
            composition: self,
            props: props.clone(),
            context,
            events,
        }))
    }
}
