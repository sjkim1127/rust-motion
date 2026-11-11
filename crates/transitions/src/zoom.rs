//! Zoom and scale transitions presentation and scene emitter.

use crate::presentation::{PresentationVisual, TransitionContext, TransitionPresentation};
use rust_motion_composition::{CompositionError, SceneEmitter, SceneFrameContext};
use rust_motion_rasterizer::{Scene, SceneNode, Transform2D};
use serde_json::Value;

/// Scaling mode for zoom transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoomMode {
    #[default]
    In,
    Out,
    InOut,
}

/// Zoom / scale reveal transition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zoom {
    pub mode: ZoomMode,
    pub max_scale: f32,
}

impl Default for Zoom {
    fn default() -> Self {
        Self {
            mode: ZoomMode::In,
            max_scale: 1.5,
        }
    }
}

impl Zoom {
    pub fn new(mode: ZoomMode) -> Self {
        Self {
            mode,
            max_scale: 1.5,
        }
    }

    pub fn with_max_scale(mut self, scale: f32) -> Self {
        self.max_scale = scale.max(1.0);
        self
    }
}

impl TransitionPresentation for Zoom {
    fn name(&self) -> &'static str {
        "Zoom"
    }

    fn render_entering(&self, ctx: &TransitionContext) -> PresentationVisual {
        let p = ctx.progress;
        let scale = match self.mode {
            ZoomMode::In => p,
            ZoomMode::Out => self.max_scale - (self.max_scale - 1.0) * p,
            ZoomMode::InOut => {
                if p < 0.5 {
                    0.0
                } else {
                    (p - 0.5) * 2.0
                }
            }
        };

        let opacity = match self.mode {
            ZoomMode::In | ZoomMode::Out => p,
            ZoomMode::InOut => {
                if p < 0.5 {
                    0.0
                } else {
                    (p - 0.5) * 2.0
                }
            }
        };

        let tx = (ctx.width / 2.0) * (1.0 - scale);
        let ty = (ctx.height / 2.0) * (1.0 - scale);

        PresentationVisual {
            transform: Transform2D {
                tx,
                ty,
                scale_x: scale,
                scale_y: scale,
                rotate_deg: 0.0,
            },
            opacity,
            clip: None,
        }
    }

    fn render_exiting(&self, ctx: &TransitionContext) -> PresentationVisual {
        let p = ctx.progress;
        let scale = match self.mode {
            ZoomMode::In => 1.0 + (self.max_scale - 1.0) * p,
            ZoomMode::Out => (1.0 - p).max(0.0),
            ZoomMode::InOut => {
                if p < 0.5 {
                    1.0 + (self.max_scale - 1.0) * (p * 2.0)
                } else {
                    0.0
                }
            }
        };

        let opacity = match self.mode {
            ZoomMode::In | ZoomMode::Out => 1.0 - p,
            ZoomMode::InOut => {
                if p < 0.5 {
                    1.0 - p * 2.0
                } else {
                    0.0
                }
            }
        };

        let tx = (ctx.width / 2.0) * (1.0 - scale);
        let ty = (ctx.height / 2.0) * (1.0 - scale);

        PresentationVisual {
            transform: Transform2D {
                tx,
                ty,
                scale_x: scale,
                scale_y: scale,
                rotate_deg: 0.0,
            },
            opacity,
            clip: None,
        }
    }
}

/// Native Scene emitter for Zoom.
pub struct SceneZoom<E> {
    pub enter_duration: u32,
    pub presentation: Zoom,
    pub child: E,
}

impl<E> SceneZoom<E> {
    pub fn new(child: E) -> Self {
        Self {
            enter_duration: 20,
            presentation: Zoom::default(),
            child,
        }
    }

    pub fn with_duration(mut self, frames: u32) -> Self {
        self.enter_duration = frames;
        self
    }

    pub fn with_mode(mut self, mode: ZoomMode) -> Self {
        self.presentation.mode = mode;
        self
    }
}

impl<E: SceneEmitter> SceneEmitter for SceneZoom<E> {
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

        let width = context.composition.width as f32;
        let height = context.composition.height as f32;
        let ctx =
            TransitionContext::new(progress, width, height, context.frame, self.enter_duration);

        let visual = self.presentation.render_entering(&ctx);
        if visual.opacity <= 0.0 {
            return Ok(());
        }

        let mut child_scene = Scene::new();
        self.child.emit(context, props, &mut child_scene)?;

        if !child_scene.nodes.is_empty() {
            scene.push(SceneNode::Group {
                transform: visual.transform,
                opacity: visual.opacity,
                children: child_scene.nodes,
            });
        }
        Ok(())
    }
}
