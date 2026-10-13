//! Native 3D primitive emitters shared by Dioxus, Tauri, and CLI compositions.

use crate::{CompositionError, SceneEmitter, SceneFrameContext};
use rust_motion_rasterizer::{Color, Mesh3D, Scene, Vec3};
use serde_json::Value;
use std::sync::Arc;

/// Procedural mesh families that can be rendered without a browser runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mesh3DPrimitive {
    Cube,
    Sphere,
    Torus,
}

/// A frame-driven native equivalent of a simple Three.js mesh scene.
#[derive(Debug, Clone)]
pub struct SceneMesh3D {
    pub primitive: Mesh3DPrimitive,
    pub center_x: f32,
    pub center_y: f32,
    pub size: f32,
    pub color: Color,
    pub camera_distance: f32,
    pub rotation: Vec3,
    pub rotation_per_frame: Vec3,
    pub light_direction: Vec3,
    pub wireframe: bool,
    /// Cached procedural topology. Per-frame work only clones and rotates
    /// vertices; expensive sphere/torus construction happens once.
    mesh: Arc<Mesh3D>,
}

impl SceneMesh3D {
    pub fn cube(center_x: f32, center_y: f32, size: f32, color: Color) -> Self {
        Self::new(Mesh3DPrimitive::Cube, center_x, center_y, size, color)
    }

    pub fn sphere(center_x: f32, center_y: f32, radius: f32, color: Color) -> Self {
        Self::new(Mesh3DPrimitive::Sphere, center_x, center_y, radius, color)
    }

    pub fn torus(center_x: f32, center_y: f32, radius: f32, color: Color) -> Self {
        Self::new(Mesh3DPrimitive::Torus, center_x, center_y, radius, color)
    }

    fn new(
        primitive: Mesh3DPrimitive,
        center_x: f32,
        center_y: f32,
        size: f32,
        color: Color,
    ) -> Self {
        let mesh = match primitive {
            Mesh3DPrimitive::Cube => Mesh3D::cube(size.max(0.0)),
            Mesh3DPrimitive::Sphere => Mesh3D::sphere(size.max(0.0), 12, 16),
            Mesh3DPrimitive::Torus => Mesh3D::torus(size.max(0.0), (size * 0.3).max(0.0), 16, 12),
        };
        Self {
            primitive,
            center_x,
            center_y,
            size,
            color,
            camera_distance: 500.0,
            rotation: Vec3::default(),
            rotation_per_frame: Vec3::default(),
            light_direction: Vec3::new(0.6, 1.0, 0.8),
            wireframe: false,
            mesh: Arc::new(mesh),
        }
    }

    pub fn with_camera_distance(mut self, distance: f32) -> Self {
        self.camera_distance = distance;
        self
    }

    pub fn with_rotation(mut self, rotation: Vec3) -> Self {
        self.rotation = rotation;
        self
    }

    pub fn with_rotation_per_frame(mut self, rotation: Vec3) -> Self {
        self.rotation_per_frame = rotation;
        self
    }

    pub fn with_light_direction(mut self, direction: Vec3) -> Self {
        self.light_direction = direction;
        self
    }

    pub fn wireframe(mut self, enabled: bool) -> Self {
        self.wireframe = enabled;
        self
    }
}

impl SceneEmitter for SceneMesh3D {
    fn emit(
        &self,
        context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let frame = context.frame as f32;
        let rotation = Vec3::new(
            self.rotation.x + self.rotation_per_frame.x * frame,
            self.rotation.y + self.rotation_per_frame.y * frame,
            self.rotation.z + self.rotation_per_frame.z * frame,
        );
        self.mesh.render_rotated_to_scene(
            scene,
            self.center_x,
            self.center_y,
            self.camera_distance,
            self.color,
            self.light_direction,
            self.wireframe,
            rotation,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NativeCompositionContext;

    #[test]
    fn emits_a_rotating_cube_as_projected_paths() {
        let emitter = SceneMesh3D::cube(320.0, 180.0, 80.0, Color::rgb(255, 120, 0))
            .with_rotation_per_frame(Vec3::new(0.02, 0.03, 0.0));
        let context = NativeCompositionContext {
            width: 640,
            height: 360,
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
            .unwrap();
        assert!(!scene.nodes.is_empty());
    }
}
