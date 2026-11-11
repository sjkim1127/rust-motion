//! Directional polygon linear wipe transition presentation and scene emitter.

use crate::presentation::{PresentationVisual, TransitionContext, TransitionPresentation};
use rust_motion_composition::{CompositionError, SceneEmitter, SceneFrameContext};
use rust_motion_rasterizer::{ClipRegion, Scene, SceneNode};
use serde_json::Value;

/// Direction for linear and polygon wipes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WipeDirection {
    #[default]
    FromLeft,
    FromRight,
    FromTop,
    FromBottom,
    FromTopLeft,
    FromTopRight,
    FromBottomLeft,
    FromBottomRight,
}

/// Linear polygon wipe transition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearWipe {
    pub direction: WipeDirection,
    pub angle_rad: f32,
}

impl Default for LinearWipe {
    fn default() -> Self {
        Self {
            direction: WipeDirection::FromLeft,
            angle_rad: 0.0,
        }
    }
}

impl LinearWipe {
    pub fn new(direction: WipeDirection) -> Self {
        Self {
            direction,
            angle_rad: 0.0,
        }
    }

    pub fn with_angle(mut self, rad: f32) -> Self {
        self.angle_rad = rad;
        self
    }

    /// Builds the polygon SVG path string for the linear wipe.
    pub fn build_clip_path(&self, width: f32, height: f32, progress: f32) -> String {
        let p = progress.clamp(0.0, 1.0);
        if p <= 0.0 {
            return "M 0 0 Z".to_string();
        }
        if p >= 1.0 {
            return format!("M 0 0 L {width} 0 L {width} {height} L 0 {height} Z");
        }

        match self.direction {
            WipeDirection::FromLeft => {
                let w = width * p;
                format!("M 0 0 L {w:.2} 0 L {w:.2} {height:.2} L 0 {height:.2} Z")
            }
            WipeDirection::FromRight => {
                let x0 = width * (1.0 - p);
                format!(
                    "M {x0:.2} 0 L {width:.2} 0 L {width:.2} {height:.2} L {x0:.2} {height:.2} Z"
                )
            }
            WipeDirection::FromTop => {
                let h = height * p;
                format!("M 0 0 L {width:.2} 0 L {width:.2} {h:.2} L 0 {h:.2} Z")
            }
            WipeDirection::FromBottom => {
                let y0 = height * (1.0 - p);
                format!(
                    "M 0 {y0:.2} L {width:.2} {y0:.2} L {width:.2} {height:.2} L 0 {height:.2} Z"
                )
            }
            WipeDirection::FromTopLeft => {
                // Diagonal polygon: corner expanding
                let diag = (width + height) * p;
                if diag <= width.min(height) {
                    format!("M 0 0 L {diag:.2} 0 L 0 {diag:.2} Z")
                } else {
                    let x_top = diag.min(width);
                    let y_left = diag.min(height);
                    let x_bot = (diag - height).max(0.0);
                    let y_right = (diag - width).max(0.0);
                    format!(
                        "M 0 0 L {x_top:.2} 0 L {width:.2} {y_right:.2} L {x_bot:.2} {height:.2} L 0 {y_left:.2} Z"
                    )
                }
            }
            WipeDirection::FromTopRight => {
                let diag = (width + height) * p;
                let x_right = width;
                if diag <= width.min(height) {
                    let x0 = (width - diag).max(0.0);
                    format!("M {x_right:.2} 0 L {x_right:.2} {diag:.2} L {x0:.2} 0 Z")
                } else {
                    let x0 = (width - diag).max(0.0);
                    let y_bot = (diag - width).max(0.0);
                    format!(
                        "M {x0:.2} 0 L {x_right:.2} 0 L {x_right:.2} {diag:.2} L {width:.2} {height:.2} L 0 {y_bot:.2} Z"
                    )
                }
            }
            WipeDirection::FromBottomLeft => {
                let diag = (width + height) * p;
                if diag <= width.min(height) {
                    let y0 = height - diag;
                    format!("M 0 {height:.2} L {diag:.2} {height:.2} L 0 {y0:.2} Z")
                } else {
                    format!("M 0 {height:.2} L {width:.2} {height:.2} L {width:.2} 0 L 0 0 Z")
                }
            }
            WipeDirection::FromBottomRight => {
                let diag = (width + height) * p;
                if diag <= width.min(height) {
                    let x0 = width - diag;
                    let y0 = height - diag;
                    format!("M {width:.2} {height:.2} L {x0:.2} {height:.2} L {width:.2} {y0:.2} Z")
                } else {
                    format!("M {width:.2} {height:.2} L 0 {height:.2} L 0 0 L {width:.2} 0 Z")
                }
            }
        }
    }
}

impl TransitionPresentation for LinearWipe {
    fn name(&self) -> &'static str {
        "LinearWipe"
    }

    fn render_entering(&self, ctx: &TransitionContext) -> PresentationVisual {
        let path = self.build_clip_path(ctx.width, ctx.height, ctx.progress);
        PresentationVisual::identity().with_clip(ClipRegion::Path { d: path })
    }

    fn render_exiting(&self, _ctx: &TransitionContext) -> PresentationVisual {
        PresentationVisual::identity()
    }
}

/// Native Scene emitter for LinearWipe.
pub struct SceneLinearWipe<E> {
    pub enter_duration: u32,
    pub presentation: LinearWipe,
    pub child: E,
}

impl<E> SceneLinearWipe<E> {
    pub fn new(child: E) -> Self {
        Self {
            enter_duration: 20,
            presentation: LinearWipe::default(),
            child,
        }
    }

    pub fn with_duration(mut self, frames: u32) -> Self {
        self.enter_duration = frames;
        self
    }

    pub fn with_direction(mut self, direction: WipeDirection) -> Self {
        self.presentation.direction = direction;
        self
    }
}

impl<E: SceneEmitter> SceneEmitter for SceneLinearWipe<E> {
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

        let path = self.presentation.build_clip_path(width, height, progress);

        let mut child_scene = Scene::new();
        self.child.emit(context, props, &mut child_scene)?;

        if !child_scene.nodes.is_empty() {
            scene.push(SceneNode::Layer {
                opacity: 1.0,
                blend_mode: rust_motion_rasterizer::BlendMode::Normal,
                clip: Some(ClipRegion::Path { d: path }),
                mask: None,
                mask_mode: rust_motion_rasterizer::MaskMode::Alpha,
                filters: Vec::new(),
                shadow: None,
                children: child_scene.nodes,
            });
        }
        Ok(())
    }
}
