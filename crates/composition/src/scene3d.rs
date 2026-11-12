//! Native 3D Scene Emitter for timeline compositions.

use crate::{CompositionError, SceneEmitter, SceneFrameContext};
use rust_motion_rasterizer::scene3d::{
    Node3D, Scene3D, Scene3DBackground, Scene3DCamera, Scene3DLight, Transform3D,
};
use rust_motion_rasterizer::{Scene, SceneNode, Vec3};
use serde_json::Value;

/// An animated 3D scene node that emits `SceneNode::Scene3D` on each frame.
#[derive(Debug, Clone)]
pub struct Scene3DEmitter {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub opacity: f32,
    pub scene: Scene3D,
    /// Optional turntable rotation applied to the root nodes on each frame (radians per frame).
    pub turntable_rotation_per_frame: Vec3,
}

impl Scene3DEmitter {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
            opacity: 1.0,
            scene: Scene3D::new(),
            turntable_rotation_per_frame: Vec3::default(),
        }
    }

    pub fn with_camera(mut self, camera: Scene3DCamera) -> Self {
        self.scene.camera = camera;
        self
    }

    pub fn with_background(mut self, bg: Scene3DBackground) -> Self {
        self.scene.background = bg;
        self
    }

    pub fn with_turntable(mut self, rot_per_frame: Vec3) -> Self {
        self.turntable_rotation_per_frame = rot_per_frame;
        self
    }

    pub fn add_light(&mut self, light: Scene3DLight) -> &mut Self {
        self.scene.add_light(light);
        self
    }

    pub fn add_node(&mut self, node: Node3D) -> &mut Self {
        self.scene.add_node(node);
        self
    }

    pub fn with_node(mut self, node: Node3D) -> Self {
        self.scene.add_node(node);
        self
    }
}

impl SceneEmitter for Scene3DEmitter {
    fn emit(
        &self,
        context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let mut frame_scene = self.scene.clone();
        let frame = context.frame as f32;

        // Apply turntable rotation if configured
        if self.turntable_rotation_per_frame != Vec3::default() {
            let rot = self.turntable_rotation_per_frame;
            let pitch = rot.x * frame;
            let yaw = rot.y * frame;
            let roll = rot.z * frame;
            let turn_transform = Transform3D::from_euler(pitch, yaw, roll);

            for node in &mut frame_scene.nodes {
                node.transform.rotation = turn_transform.rotation * node.transform.rotation;
            }
        }

        scene.push(SceneNode::Scene3D {
            x: self.x,
            y: self.y,
            w: self.width,
            h: self.height,
            scene: Box::new(frame_scene),
            opacity: self.opacity,
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NativeCompositionContext;
    use rust_motion_rasterizer::scene3d::{Geometry3D, Material3D};
    use rust_motion_rasterizer::Color;

    #[test]
    fn emits_scene3d_node_with_turntable() {
        let mut emitter =
            Scene3DEmitter::new(0.0, 0.0, 1920.0, 1080.0).with_turntable(Vec3::new(0.0, 0.05, 0.0));
        emitter.add_node(Node3D::new(
            Geometry3D::Cube { size: 100.0 },
            Material3D::pbr(Color::rgb(255, 120, 0), 0.5, 0.5),
        ));

        let context = NativeCompositionContext {
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration_in_frames: 60,
        };

        let mut scene = Scene::new();
        emitter
            .emit(
                SceneFrameContext::new(10, context),
                &Value::Null,
                &mut scene,
            )
            .expect("emit failed");

        assert_eq!(scene.nodes.len(), 1);
        let SceneNode::Scene3D {
            x,
            y,
            w,
            h,
            scene: s3d,
            ..
        } = &scene.nodes[0]
        else {
            panic!("Expected SceneNode::Scene3D");
        };

        assert_eq!(*x, 0.0);
        assert_eq!(*y, 0.0);
        assert_eq!(*w, 1920.0);
        assert_eq!(*h, 1080.0);
        assert_eq!(s3d.nodes.len(), 1);
    }
}
