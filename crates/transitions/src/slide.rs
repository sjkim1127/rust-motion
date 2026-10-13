use crate::presentation::{PresentationVisual, TransitionContext, TransitionPresentation};
use dioxus::prelude::*;
use rust_motion_animation::interpolate::{interpolate, ExtrapolateType, InterpolateOptions};
use rust_motion_core::hooks::use_current_frame;
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

/// Props for `<Slide>`.
#[derive(Props, Clone, PartialEq)]
pub struct SlideProps {
    /// Duration of the slide-in (frames).
    #[props(default = 20)]
    pub enter_duration: u32,
    /// Direction the content enters from.
    #[props(default)]
    pub direction: SlideDirection,
    /// Children to slide in.
    pub children: Element,
}

/// Slides children in from the specified direction.
#[component]
pub fn Slide(props: SlideProps) -> Element {
    let frame = use_current_frame() as f64;
    let enter = props.enter_duration as f64;

    let t = if enter > 0.0 {
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

    // Translate by 100% in the enter direction, lerp to 0%
    let translate_style = match props.direction {
        SlideDirection::FromRight => format!("translateX({:.2}%)", (1.0 - t) * 100.0),
        SlideDirection::FromLeft => format!("translateX({:.2}%)", (t - 1.0) * 100.0),
        SlideDirection::FromBottom => format!("translateY({:.2}%)", (1.0 - t) * 100.0),
        SlideDirection::FromTop => format!("translateY({:.2}%)", (t - 1.0) * 100.0),
    };

    rsx! {
        div {
            style: "
                position: absolute; top: 0; left: 0; right: 0; bottom: 0;
                transform: {translate_style};
            ",
            {props.children}
        }
    }
}
