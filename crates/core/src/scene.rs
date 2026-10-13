//! Native Scene emitter adapters re-exported from `rust-motion-composition`.
//!
//! Provides a single import path for `NativeComposition` authors who need
//! timeline primitives (`SceneSequence`, `SceneFreeze`) alongside the core
//! Dioxus components (`Sequence`, `Freeze`).
//!
//! # Example
//!
//! ```rust,ignore
//! use rust_motion_core::scene::{
//!     SceneAbsoluteFill, SceneEmitter, SceneEmitterComposition,
//!     SceneFrameContext, SceneRect, SceneSequence,
//! };
//! use rust_motion_rasterizer::Color;
//!
//! fn make_scene() -> impl SceneEmitter {
//!     SceneAbsoluteFill::new(SceneRect::new(
//!         0.0, 0.0, 1920.0, 1080.0, Color::rgb(15, 23, 42),
//!     ))
//! }
//! ```

use rust_motion_composition::CompositionError;
use rust_motion_rasterizer::{BlendMode, ClipRegion, MaskMode, Scene, SceneNode};
use serde_json::Value;

// ── Re-exports from rust-motion-composition ────────────────────────────────────
pub use rust_motion_composition::{
    SceneEmitter, SceneEmitterComposition, SceneFrameContext, SceneFreeze, SceneGroup, SceneLayer,
    SceneLinearGradient, SceneRect, SceneSequence, SceneStack, SceneText, SceneTextBlock,
};

// ── SceneAbsoluteFill ─────────────────────────────────────────────────────────

/// Native counterpart of the [`AbsoluteFill`](crate::AbsoluteFill) Dioxus component.
///
/// Wraps the `child` emitter in a compositing [`SceneNode::Layer`] clipped to
/// the full composition canvas (`0, 0, width, height`), exactly mirroring the
/// CSS `position: absolute; top: 0; left: 0; right: 0; bottom: 0` behaviour.
pub struct SceneAbsoluteFill<E> {
    pub child: E,
}

impl<E> SceneAbsoluteFill<E> {
    pub fn new(child: E) -> Self {
        Self { child }
    }
}

impl<E: SceneEmitter> SceneEmitter for SceneAbsoluteFill<E> {
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let w = context.composition.width as f32;
        let h = context.composition.height as f32;
        let mut child_scene = Scene::new();
        self.child.emit(context, props, &mut child_scene)?;
        if !child_scene.nodes.is_empty() {
            scene.push(SceneNode::Layer {
                opacity: 1.0,
                blend_mode: BlendMode::Normal,
                clip: Some(ClipRegion::Rect {
                    x: 0.0,
                    y: 0.0,
                    w,
                    h,
                    corner_radius: 0.0,
                }),
                mask: None,
                mask_mode: MaskMode::Alpha,
                filters: Vec::new(),
                shadow: None,
                children: child_scene.nodes,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_motion_composition::{
        NativeComposition, NativeCompositionContext, SceneEmitterComposition,
    };
    use rust_motion_rasterizer::{Color, SceneNode};

    fn context() -> NativeCompositionContext {
        NativeCompositionContext {
            width: 320,
            height: 180,
            fps: 30.0,
            duration_in_frames: 60,
        }
    }

    fn white_rect() -> SceneNode {
        SceneNode::Rect {
            x: 10.0,
            y: 10.0,
            w: 50.0,
            h: 50.0,
            fill: Color::WHITE,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }
    }

    #[test]
    fn absolute_fill_wraps_child_in_full_canvas_layer_clip() {
        let fill = SceneAbsoluteFill::new(white_rect());
        let comp = SceneEmitterComposition::new("abs-fill", fill);
        let scene = comp.render(0, &serde_json::Value::Null, context()).unwrap();

        assert_eq!(scene.nodes.len(), 1);
        assert!(matches!(
            &scene.nodes[0],
            SceneNode::Layer {
                clip: Some(ClipRegion::Rect { x, y, w, h, .. }),
                children,
                ..
            } if (*x - 0.0).abs() < f32::EPSILON
                && (*y - 0.0).abs() < f32::EPSILON
                && (*w - 320.0).abs() < f32::EPSILON
                && (*h - 180.0).abs() < f32::EPSILON
                && children.len() == 1
        ));
    }

    #[test]
    fn scene_sequence_re_export_applies_local_frame_offset() {
        let seq = SceneSequence::new(
            10,
            |ctx: SceneFrameContext, _: &serde_json::Value, scene: &mut Scene| {
                scene.push(SceneNode::Text {
                    x: 0.0,
                    y: 0.0,
                    content: ctx.frame.to_string(),
                    font_size: 16.0,
                    color: Color::WHITE,
                    font_weight: 400,
                    font_sources: Vec::new(),
                });
                Ok(())
            },
        )
        .with_duration(5);

        let comp = SceneEmitterComposition::new("seq", seq);
        let active = comp
            .render(12, &serde_json::Value::Null, context())
            .unwrap();
        let inactive = comp
            .render(15, &serde_json::Value::Null, context())
            .unwrap();

        assert!(matches!(
            &active.nodes[0],
            SceneNode::Text { content, .. } if content == "2"
        ));
        assert!(inactive.nodes.is_empty());
    }

    #[test]
    fn scene_freeze_re_export_pins_local_frame() {
        let freeze = SceneFreeze::new(
            7,
            |ctx: SceneFrameContext, _: &serde_json::Value, scene: &mut Scene| {
                scene.push(SceneNode::Text {
                    x: 0.0,
                    y: 0.0,
                    content: ctx.frame.to_string(),
                    font_size: 16.0,
                    color: Color::WHITE,
                    font_weight: 400,
                    font_sources: Vec::new(),
                });
                Ok(())
            },
        );
        let comp = SceneEmitterComposition::new("freeze", freeze);
        let scene = comp
            .render(42, &serde_json::Value::Null, context())
            .unwrap();

        assert!(matches!(
            &scene.nodes[0],
            SceneNode::Text { content, .. } if content == "7"
        ));
    }
}
