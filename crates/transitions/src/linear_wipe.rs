//! Directional polygon linear wipe transition presentation and component.

use crate::presentation::{PresentationVisual, TransitionContext, TransitionPresentation};
use dioxus::prelude::*;
use rust_motion_animation::interpolate::{interpolate, ExtrapolateType, InterpolateOptions};
use rust_motion_composition::{CompositionError, SceneEmitter, SceneFrameContext};
use rust_motion_core::hooks::use_current_frame;
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
                let d = 2.0 * p;
                let x1 = width * d;
                let y1 = height * d;
                format!("M 0 0 L {x1:.2} 0 L 0 {y1:.2} Z")
            }
            WipeDirection::FromTopRight => {
                let d = 2.0 * p;
                let x0 = width * (1.0 - d);
                let y1 = height * d;
                format!("M {width:.2} 0 L {x0:.2} 0 L {width:.2} {y1:.2} Z")
            }
            WipeDirection::FromBottomLeft => {
                let d = 2.0 * p;
                let y0 = height * (1.0 - d);
                let x1 = width * d;
                format!("M 0 {height:.2} L 0 {y0:.2} L {x1:.2} {height:.2} Z")
            }
            WipeDirection::FromBottomRight => {
                let d = 2.0 * p;
                let x0 = width * (1.0 - d);
                let y0 = height * (1.0 - d);
                format!("M {width:.2} {height:.2} L {x0:.2} {height:.2} L {width:.2} {y0:.2} Z")
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

/// Props for `<LinearWipe>` Dioxus component.
#[derive(Props, Clone, PartialEq)]
pub struct LinearWipeProps {
    #[props(default = 20)]
    pub enter_duration: u32,
    #[props(default)]
    pub direction: WipeDirection,
    pub children: Element,
}

/// Linear polygon wipe component.
#[component]
pub fn LinearWipe(props: LinearWipeProps) -> Element {
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
    } as f32;

    let wipe = LinearWipe::new(props.direction);
    let path = wipe.build_clip_path(100.0, 100.0, p);

    rsx! {
        div {
            style: "
                position: absolute; top: 0; left: 0; right: 0; bottom: 0;
                clip-path: path('{path}');
            ",
            {props.children}
        }
    }
}
