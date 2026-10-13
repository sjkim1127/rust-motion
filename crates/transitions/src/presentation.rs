//! Base presentation contract and context for video transitions.

use rust_motion_rasterizer::{ClipRegion, Transform2D};

/// Contextual data provided to a transition presentation during evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransitionContext {
    /// Normalized transition progress in `0.0..=1.0`.
    pub progress: f32,
    /// Composition pixel width.
    pub width: f32,
    /// Composition pixel height.
    pub height: f32,
    /// Current local frame within the transition.
    pub frame: u32,
    /// Total duration of the transition in frames.
    pub duration_in_frames: u32,
}

impl TransitionContext {
    pub fn new(
        progress: f32,
        width: f32,
        height: f32,
        frame: u32,
        duration_in_frames: u32,
    ) -> Self {
        Self {
            progress: progress.clamp(0.0, 1.0),
            width: width.max(1.0),
            height: height.max(1.0),
            frame,
            duration_in_frames,
        }
    }
}

/// Visual properties produced by a transition for a scene layer/group.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PresentationVisual {
    pub transform: Transform2D,
    pub opacity: f32,
    pub clip: Option<ClipRegion>,
}

impl PresentationVisual {
    pub fn identity() -> Self {
        Self {
            transform: Transform2D::default(),
            opacity: 1.0,
            clip: None,
        }
    }

    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    pub fn with_transform(mut self, transform: Transform2D) -> Self {
        self.transform = transform;
        self
    }

    pub fn with_clip(mut self, clip: ClipRegion) -> Self {
        self.clip = Some(clip);
        self
    }
}

/// A presentation effect governing how entering and exiting scenes visually transition.
pub trait TransitionPresentation: Send + Sync {
    /// Descriptive name of the presentation effect.
    fn name(&self) -> &'static str;

    /// Computes the visual transformation for the entering scene.
    fn render_entering(&self, ctx: &TransitionContext) -> PresentationVisual;

    /// Computes the visual transformation for the exiting scene.
    fn render_exiting(&self, ctx: &TransitionContext) -> PresentationVisual;
}
