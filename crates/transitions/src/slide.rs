use crate::presentation::{PresentationVisual, TransitionContext, TransitionPresentation};
use rust_motion_rasterizer::Transform2D;

/// Direction the content slides in from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SlideDirection {
    #[default]
    FromRight,
    FromLeft,
    FromTop,
    FromBottom,
}

/// Slide transition presentation parameters.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SlidePresentation {
    pub direction: SlideDirection,
}

impl SlidePresentation {
    pub fn new(direction: SlideDirection) -> Self {
        Self { direction }
    }
}

impl TransitionPresentation for SlidePresentation {
    fn name(&self) -> &'static str {
        "Slide"
    }

    fn render_entering(&self, ctx: &TransitionContext) -> PresentationVisual {
        let p = ctx.progress;
        let rem = 1.0 - p;
        let (tx, ty) = match self.direction {
            SlideDirection::FromRight => (rem * ctx.width, 0.0),
            SlideDirection::FromLeft => (-rem * ctx.width, 0.0),
            SlideDirection::FromBottom => (0.0, rem * ctx.height),
            SlideDirection::FromTop => (0.0, -rem * ctx.height),
        };
        PresentationVisual::identity().with_transform(Transform2D {
            tx,
            ty,
            ..Default::default()
        })
    }

    fn render_exiting(&self, ctx: &TransitionContext) -> PresentationVisual {
        let p = ctx.progress;
        let (tx, ty) = match self.direction {
            SlideDirection::FromRight => (-p * ctx.width, 0.0),
            SlideDirection::FromLeft => (p * ctx.width, 0.0),
            SlideDirection::FromBottom => (0.0, -p * ctx.height),
            SlideDirection::FromTop => (0.0, p * ctx.height),
        };
        PresentationVisual::identity().with_transform(Transform2D {
            tx,
            ty,
            ..Default::default()
        })
    }
}
