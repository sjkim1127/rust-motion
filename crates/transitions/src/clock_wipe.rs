//! Circular ClockWipe transition presentation and component.

use crate::presentation::{PresentationVisual, TransitionContext, TransitionPresentation};
use dioxus::prelude::*;
use rust_motion_animation::interpolate::{interpolate, ExtrapolateType, InterpolateOptions};
use rust_motion_composition::{CompositionError, SceneEmitter, SceneFrameContext};
use rust_motion_core::hooks::use_current_frame;
use rust_motion_rasterizer::{ClipRegion, Scene, SceneNode};
use serde_json::Value;

/// Clock wipe transition revealing the entering scene in a circular clock sweep.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockWipe {
    pub counter_clockwise: bool,
    pub start_angle_deg: f32,
}

impl Default for ClockWipe {
    fn default() -> Self {
        Self {
            counter_clockwise: false,
            start_angle_deg: 0.0,
        }
    }
}

impl ClockWipe {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn counter_clockwise(mut self, ccw: bool) -> Self {
        self.counter_clockwise = ccw;
        self
    }

    pub fn start_angle(mut self, deg: f32) -> Self {
        self.start_angle_deg = deg;
        self
    }

    /// Computes the SVG path string for the clock wipe pie sector clip.
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
        let radius = (width * width + height * height).sqrt() / 2.0 * 1.05;

        // Top 12 o'clock corresponds to -90 degrees in standard Cartesian angles
        let start_deg = self.start_angle_deg - 90.0;
        let sweep_angle = p * 360.0;
        let end_deg = if self.counter_clockwise {
            start_deg - sweep_angle
        } else {
            start_deg + sweep_angle
        };

        let start_rad = start_deg.to_radians();
        let end_rad = end_deg.to_radians();

        let x0 = cx + radius * start_rad.cos();
        let y0 = cy + radius * start_rad.sin();
        let x1 = cx + radius * end_rad.cos();
        let y1 = cy + radius * end_rad.sin();

        let large_arc_flag = if p > 0.5 { 1 } else { 0 };
        let sweep_flag = if self.counter_clockwise { 0 } else { 1 };

        format!(
            "M {cx:.2} {cy:.2} L {x0:.2} {y0:.2} A {radius:.2} {radius:.2} 0 {large_arc_flag} {sweep_flag} {x1:.2} {y1:.2} Z"
        )
    }
}

impl TransitionPresentation for ClockWipe {
    fn name(&self) -> &'static str {
        "ClockWipe"
    }

    fn render_entering(&self, ctx: &TransitionContext) -> PresentationVisual {
        let path = self.build_clip_path(ctx.width, ctx.height, ctx.progress);
        PresentationVisual::identity().with_clip(ClipRegion::Path { d: path })
    }

    fn render_exiting(&self, _ctx: &TransitionContext) -> PresentationVisual {
        PresentationVisual::identity()
    }
}

/// Native Scene emitter for ClockWipe transition.
pub struct SceneClockWipe<E> {
    pub enter_duration: u32,
    pub presentation: ClockWipe,
    pub child: E,
}

impl<E> SceneClockWipe<E> {
    pub fn new(child: E) -> Self {
        Self {
            enter_duration: 20,
            presentation: ClockWipe::default(),
            child,
        }
    }

    pub fn with_duration(mut self, frames: u32) -> Self {
        self.enter_duration = frames;
        self
    }

    pub fn with_presentation(mut self, presentation: ClockWipe) -> Self {
        self.presentation = presentation;
        self
    }
}

impl<E: SceneEmitter> SceneEmitter for SceneClockWipe<E> {
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

/// Props for the `<ClockWipe>` Dioxus component.
#[derive(Props, Clone, PartialEq)]
pub struct ClockWipeProps {
    #[props(default = 20)]
    pub enter_duration: u32,
    #[props(default = false)]
    pub counter_clockwise: bool,
    #[props(default = 0.0)]
    pub start_angle_deg: f32,
    pub children: Element,
}

/// Circular clock sweep transition reveal component.
#[component]
pub fn ClockWipe(props: ClockWipeProps) -> Element {
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

    let wipe = ClockWipe {
        counter_clockwise: props.counter_clockwise,
        start_angle_deg: props.start_angle_deg,
    };
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
