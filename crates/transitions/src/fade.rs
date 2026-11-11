//! Fade transition presentation — crossfade between transparent and opaque.
//!
//! Equivalent to Remotion `<TransitionSeries>` with `fade()` presentation.

use crate::presentation::{PresentationVisual, TransitionContext, TransitionPresentation};

/// Presentation parameters for a fade transition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FadePresentation {
    pub should_fade_out_exiting_scene: bool,
}

impl Default for FadePresentation {
    fn default() -> Self {
        Self {
            should_fade_out_exiting_scene: true,
        }
    }
}

impl FadePresentation {
    pub fn new(should_fade_out_exiting_scene: bool) -> Self {
        Self {
            should_fade_out_exiting_scene,
        }
    }
}

impl TransitionPresentation for FadePresentation {
    fn name(&self) -> &'static str {
        "Fade"
    }

    fn render_entering(&self, ctx: &TransitionContext) -> PresentationVisual {
        PresentationVisual::identity().with_opacity(ctx.progress)
    }

    fn render_exiting(&self, ctx: &TransitionContext) -> PresentationVisual {
        let opacity = if self.should_fade_out_exiting_scene {
            1.0 - ctx.progress
        } else {
            1.0
        };
        PresentationVisual::identity().with_opacity(opacity)
    }
}
