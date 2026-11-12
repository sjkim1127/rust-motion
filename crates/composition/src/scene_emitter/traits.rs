//! Core traits and context for composable scene emitters.

use crate::{CompositionError, NativeCompositionContext, Tick};
use rust_motion_rasterizer::{Scene, SceneNode};
use serde_json::Value;

/// Timeline state passed through a native scene-emitter tree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneFrameContext {
    /// Frame local to the current sequence or freeze boundary.
    pub frame: u32,
    /// Unmodified composition frame.
    pub global_frame: u32,
    pub composition: NativeCompositionContext,
}

impl SceneFrameContext {
    pub fn new(frame: u32, composition: NativeCompositionContext) -> Self {
        Self {
            frame,
            global_frame: frame,
            composition,
        }
    }

    pub fn time_secs(self) -> f64 {
        self.frame as f64 / self.composition.fps
    }

    pub fn global_time_secs(self) -> f64 {
        self.global_frame as f64 / self.composition.fps
    }

    /// Zero-drift exact media time in integer ticks.
    pub fn time_ticks(self) -> Tick {
        self.composition.frame_to_tick(self.frame)
    }

    /// Zero-drift global media time in integer ticks.
    pub fn global_time_ticks(self) -> Tick {
        self.composition.frame_to_tick(self.global_frame)
    }

    /// Format current local frame as SMPTE timecode.
    pub fn format_timecode(self, drop_frame: bool) -> String {
        self.composition.format_timecode(self.frame, drop_frame)
    }

    pub(crate) fn with_local_frame(self, frame: u32) -> Self {
        Self { frame, ..self }
    }
}

/// Shared primitive contract for media, shapes, captions, transitions, and apps.
///
/// Emitters append nodes to the supplied scene and can be nested through
/// [`SceneSequence`](super::combinators::SceneSequence),
/// [`SceneFreeze`](super::combinators::SceneFreeze),
/// [`SceneGroup`](super::combinators::SceneGroup), and
/// [`SceneStack`](super::combinators::SceneStack).
pub trait SceneEmitter: Send + Sync {
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError>;
}

impl<F> SceneEmitter for F
where
    F: Fn(SceneFrameContext, &Value, &mut Scene) -> Result<(), CompositionError> + Send + Sync,
{
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        self(context, props, scene)
    }
}

impl SceneEmitter for SceneNode {
    fn emit(
        &self,
        _context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        scene.push(self.clone());
        Ok(())
    }
}
