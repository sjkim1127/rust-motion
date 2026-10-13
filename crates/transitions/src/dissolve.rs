//! Cross-dissolve transition presentation and component.

use crate::presentation::{PresentationVisual, TransitionContext, TransitionPresentation};
use dioxus::prelude::*;
use rust_motion_animation::interpolate::{interpolate, ExtrapolateType, InterpolateOptions};
use rust_motion_composition::{CompositionError, SceneEmitter, SceneFrameContext};
use rust_motion_core::hooks::use_current_frame;
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

/// Props for `<Dissolve>` component.
#[derive(Props, Clone, PartialEq)]
pub struct DissolveProps {
    #[props(default = 20)]
    pub enter_duration: u32,
    pub children: Element,
}

/// Dissolve crossfade component.
#[component]
pub fn Dissolve(props: DissolveProps) -> Element {
    let frame = use_current_frame() as f64;
    let enter = props.enter_duration as f64;

    let opacity = if enter > 0.0 {
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

    rsx! {
        div {
            style: "
                position: absolute; top: 0; left: 0; right: 0; bottom: 0;
                opacity: {opacity:.4};
            ",
            {props.children}
        }
    }
}
