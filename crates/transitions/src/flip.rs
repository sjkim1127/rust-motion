//! 3D perspective flip transition presentation and component.

use crate::presentation::{PresentationVisual, TransitionContext, TransitionPresentation};
use dioxus::prelude::*;
use rust_motion_animation::interpolate::{interpolate, ExtrapolateType, InterpolateOptions};
use rust_motion_composition::{CompositionError, SceneEmitter, SceneFrameContext};
use rust_motion_core::hooks::use_current_frame;
use rust_motion_rasterizer::{Scene, SceneNode, Transform2D};
use serde_json::Value;

/// Direction for 3D card flips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlipDirection {
    #[default]
    FromRight,
    FromLeft,
    FromTop,
    FromBottom,
}

/// 3D perspective projection flip transition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flip {
    pub direction: FlipDirection,
    pub perspective: f32,
}

impl Default for Flip {
    fn default() -> Self {
        Self {
            direction: FlipDirection::FromRight,
            perspective: 1000.0,
        }
    }
}

impl Flip {
    pub fn new(direction: FlipDirection) -> Self {
        Self {
            direction,
            perspective: 1000.0,
        }
    }

    pub fn with_perspective(mut self, perspective: f32) -> Self {
        self.perspective = perspective.max(1.0);
        self
    }
}

impl TransitionPresentation for Flip {
    fn name(&self) -> &'static str {
        "Flip"
    }

    fn render_entering(&self, ctx: &TransitionContext) -> PresentationVisual {
        let p = ctx.progress;
        if p < 0.5 {
            // Backface culled before midpoint
            return PresentationVisual::identity().with_opacity(0.0);
        }

        // Midpoint to end: 90 deg -> 0 deg
        let local_p = (p - 0.5) * 2.0; // 0.0 -> 1.0
        let angle_rad = ((1.0 - local_p) * 90.0).to_radians();
        let scale = angle_rad.cos().max(0.0);

        let (transform, opacity) = match self.direction {
            FlipDirection::FromRight | FlipDirection::FromLeft => {
                let tx = (ctx.width / 2.0) * (1.0 - scale);
                let t = Transform2D {
                    tx,
                    ty: 0.0,
                    scale_x: scale,
                    scale_y: 1.0,
                    rotate_deg: 0.0,
                };
                (t, 1.0)
            }
            FlipDirection::FromTop | FlipDirection::FromBottom => {
                let ty = (ctx.height / 2.0) * (1.0 - scale);
                let t = Transform2D {
                    tx: 0.0,
                    ty,
                    scale_x: 1.0,
                    scale_y: scale,
                    rotate_deg: 0.0,
                };
                (t, 1.0)
            }
        };

        PresentationVisual {
            transform,
            opacity,
            clip: None,
        }
    }

    fn render_exiting(&self, ctx: &TransitionContext) -> PresentationVisual {
        let p = ctx.progress;
        if p >= 0.5 {
            // Backface culled past midpoint
            return PresentationVisual::identity().with_opacity(0.0);
        }

        // Start to midpoint: 0 deg -> 90 deg
        let local_p = p * 2.0; // 0.0 -> 1.0
        let angle_rad = (local_p * 90.0).to_radians();
        let scale = angle_rad.cos().max(0.0);

        let (transform, opacity) = match self.direction {
            FlipDirection::FromRight | FlipDirection::FromLeft => {
                let tx = (ctx.width / 2.0) * (1.0 - scale);
                let t = Transform2D {
                    tx,
                    ty: 0.0,
                    scale_x: scale,
                    scale_y: 1.0,
                    rotate_deg: 0.0,
                };
                (t, 1.0)
            }
            FlipDirection::FromTop | FlipDirection::FromBottom => {
                let ty = (ctx.height / 2.0) * (1.0 - scale);
                let t = Transform2D {
                    tx: 0.0,
                    ty,
                    scale_x: 1.0,
                    scale_y: scale,
                    rotate_deg: 0.0,
                };
                (t, 1.0)
            }
        };

        PresentationVisual {
            transform,
            opacity,
            clip: None,
        }
    }
}

/// Native Scene emitter for Flip.
pub struct SceneFlip<E> {
    pub enter_duration: u32,
    pub presentation: Flip,
    pub child: E,
}

impl<E> SceneFlip<E> {
    pub fn new(child: E) -> Self {
        Self {
            enter_duration: 20,
            presentation: Flip::default(),
            child,
        }
    }

    pub fn with_duration(mut self, frames: u32) -> Self {
        self.enter_duration = frames;
        self
    }

    pub fn with_direction(mut self, direction: FlipDirection) -> Self {
        self.presentation.direction = direction;
        self
    }
}

impl<E: SceneEmitter> SceneEmitter for SceneFlip<E> {
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

/// Props for `<Flip>` component.
#[derive(Props, Clone, PartialEq)]
pub struct FlipProps {
    #[props(default = 20)]
    pub enter_duration: u32,
    #[props(default)]
    pub direction: FlipDirection,
    #[props(default = 1000.0)]
    pub perspective: f32,
    pub children: Element,
}

/// 3D perspective card flip component.
#[component]
pub fn Flip(props: FlipProps) -> Element {
    let frame = use_current_frame() as f64;
    let enter = props.enter_duration as f64;

    let p = if enter > 0.0 {
        interpolate(
            frame,
            &[0.0, enter],
            &[0.0, 1.0],
            InterpolateOptions {
                extrapolate_left: ExtrapolateType::Clamp,
                extrapolate_right: ExtrapolateType::Clamp,
                ..Default::default()
            },
        )
    } else {
        1.0
    };

    let rotate_axis = match props.direction {
        FlipDirection::FromRight | FlipDirection::FromLeft => "rotateY",
        FlipDirection::FromTop | FlipDirection::FromBottom => "rotateX",
    };
    let angle = (1.0 - p) * 90.0;

    rsx! {
        div {
            style: "
                position: absolute; top: 0; left: 0; right: 0; bottom: 0;
                perspective: {props.perspective}px;
                transform: {rotate_axis}({angle:.2}deg);
                backface-visibility: hidden;
            ",
            {props.children}
        }
    }
}
