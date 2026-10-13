pub use crate::clock_wipe::SceneClockWipe;
pub use crate::dissolve::SceneDissolve;
pub use crate::flip::SceneFlip;
pub use crate::iris::SceneIris;
pub use crate::linear_wipe::{SceneLinearWipe, SceneLinearWipe as SceneWipe};
pub use crate::slide::SlideDirection;
pub use crate::zoom::SceneZoom;
use rust_motion_composition::{CompositionError, SceneEmitter, SceneFrameContext};
use rust_motion_rasterizer::{Scene, SceneNode, Transform2D};
use serde_json::Value;

/// Native fade-in/fade-out wrapper.
pub struct SceneFade<E> {
    pub enter_duration: u32,
    pub exit_duration: u32,
    pub duration_in_frames: Option<u32>,
    pub child: E,
}

impl<E> SceneFade<E> {
    pub fn new(child: E) -> Self {
        Self {
            enter_duration: 20,
            exit_duration: 0,
            duration_in_frames: None,
            child,
        }
    }

    pub fn with_enter_duration(mut self, frames: u32) -> Self {
        self.enter_duration = frames;
        self
    }

    pub fn with_exit(mut self, duration_in_frames: u32, exit_duration: u32) -> Self {
        self.duration_in_frames = Some(duration_in_frames);
        self.exit_duration = exit_duration;
        self
    }
}

impl<E: SceneEmitter> SceneEmitter for SceneFade<E> {
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let enter = if self.enter_duration == 0 {
            1.0
        } else {
            context.frame as f32 / self.enter_duration as f32
        }
        .clamp(0.0, 1.0);
        let exit = match (self.duration_in_frames, self.exit_duration) {
            (_, 0) | (None, _) => 1.0,
            (Some(duration), exit_duration) => {
                let exit_start = duration.saturating_sub(exit_duration);
                if context.frame <= exit_start {
                    1.0
                } else {
                    1.0 - (context.frame - exit_start) as f32 / exit_duration as f32
                }
            }
        }
        .clamp(0.0, 1.0);
        emit_group(
            &self.child,
            context,
            props,
            scene,
            enter.min(exit),
            Transform2D::default(),
        )
    }
}

/// Native slide-in wrapper using composition pixel dimensions.
pub struct SceneSlide<E> {
    pub enter_duration: u32,
    pub direction: SlideDirection,
    pub child: E,
}

impl<E> SceneSlide<E> {
    pub fn new(child: E) -> Self {
        Self {
            enter_duration: 20,
            direction: SlideDirection::FromRight,
            child,
        }
    }

    pub fn with_duration(mut self, frames: u32) -> Self {
        self.enter_duration = frames;
        self
    }

    pub fn from(mut self, direction: SlideDirection) -> Self {
        self.direction = direction;
        self
    }
}

impl<E: SceneEmitter> SceneEmitter for SceneSlide<E> {
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
        let remaining = 1.0 - progress;
        let width = context.composition.width as f32;
        let height = context.composition.height as f32;
        let (tx, ty) = match self.direction {
            SlideDirection::FromRight => (remaining * width, 0.0),
            SlideDirection::FromLeft => (-remaining * width, 0.0),
            SlideDirection::FromBottom => (0.0, remaining * height),
            SlideDirection::FromTop => (0.0, -remaining * height),
        };
        emit_group(
            &self.child,
            context,
            props,
            scene,
            1.0,
            Transform2D {
                tx,
                ty,
                ..Default::default()
            },
        )
    }
}

fn emit_group<E: SceneEmitter>(
    child: &E,
    context: SceneFrameContext,
    props: &Value,
    scene: &mut Scene,
    opacity: f32,
    transform: Transform2D,
) -> Result<(), CompositionError> {
    let mut children = Scene::new();
    child.emit(context, props, &mut children)?;
    if !children.nodes.is_empty() {
        scene.push(SceneNode::Group {
            transform,
            opacity,
            children: children.nodes,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_motion_composition::{
        NativeComposition, NativeCompositionContext, SceneEmitterComposition,
    };
    use rust_motion_rasterizer::Color;

    fn context() -> NativeCompositionContext {
        NativeCompositionContext {
            width: 320,
            height: 180,
            fps: 30.0,
            duration_in_frames: 60,
        }
    }

    fn node() -> SceneNode {
        SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 20.0,
            h: 20.0,
            fill: Color::WHITE,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }
    }

    #[test]
    fn fade_matches_enter_and_exit_windows() {
        let fade = SceneFade::new(node())
            .with_enter_duration(10)
            .with_exit(30, 10);
        let composition = SceneEmitterComposition::new("fade", fade);
        for (frame, expected) in [(5, 0.5), (15, 1.0), (25, 0.5)] {
            let scene = composition.render(frame, &Value::Null, context()).unwrap();
            assert!(matches!(
                scene.nodes[0],
                SceneNode::Group { opacity, .. } if (opacity - expected).abs() < f32::EPSILON
            ));
        }
    }

    #[test]
    fn slide_uses_composition_dimensions() {
        let slide = SceneSlide::new(node())
            .with_duration(10)
            .from(SlideDirection::FromLeft);
        let composition = SceneEmitterComposition::new("slide", slide);
        let scene = composition.render(5, &Value::Null, context()).unwrap();
        assert!(matches!(
            scene.nodes[0],
            SceneNode::Group { transform, .. } if (transform.tx + 160.0).abs() < f32::EPSILON
        ));
    }
}
