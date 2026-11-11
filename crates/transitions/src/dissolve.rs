//! Cross-dissolve transition presentation and scene emitter.

use crate::presentation::{PresentationVisual, TransitionContext, TransitionPresentation};
use rust_motion_composition::{CompositionError, SceneEmitter, SceneFrameContext};
use rust_motion_rasterizer::{Scene, SceneNode, Transform2D};
use serde_json::Value;

/// Dissolve transition cross-fading scenes smoothly.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Dissolve {}

impl Dissolve {
    pub fn new() -> Self {
        Self {}
    }
}

impl TransitionPresentation for Dissolve {
    fn name(&self) -> &'static str {
        "Dissolve"
    }

    fn render_entering(&self, ctx: &TransitionContext) -> PresentationVisual {
        PresentationVisual::identity().with_opacity(ctx.progress)
    }

    fn render_exiting(&self, ctx: &TransitionContext) -> PresentationVisual {
        PresentationVisual::identity().with_opacity(1.0 - ctx.progress)
    }
}

/// Native Scene emitter for Dissolve.
pub struct SceneDissolve<E> {
    pub enter_duration: u32,
    pub child: E,
}

impl<E> SceneDissolve<E> {
    pub fn new(child: E) -> Self {
        Self {
            enter_duration: 20,
            child,
        }
    }

    pub fn with_duration(mut self, frames: u32) -> Self {
        self.enter_duration = frames;
        self
    }
}

impl<E: SceneEmitter> SceneEmitter for SceneDissolve<E> {
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let progress = if self.enter_duration == 0 {
            1.0
        } else {
            context.frame as f32 / self.enter_duration as f32
        }
        .clamp(0.0, 1.0);

        let mut child_scene = Scene::new();
        self.child.emit(context, props, &mut child_scene)?;

        if !child_scene.nodes.is_empty() {
            scene.push(SceneNode::Group {
                transform: Transform2D::default(),
                opacity: progress,
                children: child_scene.nodes,
            });
        }
        Ok(())
    }
}
