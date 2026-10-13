//! Expanding circular aperture Iris transition presentation and component.

use crate::presentation::{PresentationVisual, TransitionContext, TransitionPresentation};
use dioxus::prelude::*;
use rust_motion_animation::interpolate::{interpolate, ExtrapolateType, InterpolateOptions};
use rust_motion_composition::{CompositionError, SceneEmitter, SceneFrameContext};
use rust_motion_core::hooks::use_current_frame;
use rust_motion_rasterizer::{ClipRegion, Scene, SceneNode};
use serde_json::Value;

/// Iris transition expanding in a circle from the center.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Iris {}

impl Iris {
    pub fn new() -> Self {
        Self {}
    }

    /// Builds the circular SVG path for the expanding aperture.
    pub fn build_clip_path(&self, width: f32, height: f32, progress: f32) -> String {
        let p = progress.clamp(0.0, 1.0);
        if p <= 0.0 {
            return "M 0 0 Z".to_string();
        }
        if p >= 1.0 {
            return format!("M 0 0 L {width} 0 L {width} {height} L 0 {height} Z");
        }

        let cx = width / 2.0;
        let cy = height / 2.0;
        let max_r = (width * width + height * height).sqrt() / 2.0 * 1.05;
        let r = p * max_r;
        let d = 2.0 * r;

        format!("M {cx:.2} {cy:.2} m -{r:.2}, 0 a {r:.2},{r:.2} 0 1,0 {d:.2},0 a {r:.2},{r:.2} 0 1,0 -{d:.2},0 Z")
    }
}

impl TransitionPresentation for Iris {
    fn name(&self) -> &'static str {
        "Iris"
    }

    fn render_entering(&self, ctx: &TransitionContext) -> PresentationVisual {
        let path = self.build_clip_path(ctx.width, ctx.height, ctx.progress);
        PresentationVisual::identity().with_clip(ClipRegion::Path { d: path })
    }

    fn render_exiting(&self, _ctx: &TransitionContext) -> PresentationVisual {
        PresentationVisual::identity()
    }
}

/// Native Scene emitter for Iris.
pub struct SceneIris<E> {
    pub enter_duration: u32,
    pub presentation: Iris,
    pub child: E,
}

impl<E> SceneIris<E> {
    pub fn new(child: E) -> Self {
        Self {
            enter_duration: 20,
            presentation: Iris::default(),
            child,
        }
    }

    pub fn with_duration(mut self, frames: u32) -> Self {
        self.enter_duration = frames;
        self
    }
}

impl<E: SceneEmitter> SceneEmitter for SceneIris<E> {
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

/// Props for `<Iris>` component.
#[derive(Props, Clone, PartialEq)]
pub struct IrisProps {
    #[props(default = 20)]
    pub enter_duration: u32,
    pub children: Element,
}

/// Iris circular aperture transition component.
#[component]
pub fn Iris(props: IrisProps) -> Element {
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

    let iris = Iris::new();
    let path = iris.build_clip_path(100.0, 100.0, p);

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
