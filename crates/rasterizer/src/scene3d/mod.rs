//! Native 3D Scene Graph, PBR Materials, Procedural Geometries, and GPU Instancing.
//!
//! Provides a first-class 3D graphics pipeline inside Rust Motion that renders
//! directly into the 2D timeline scene graph without an external browser engine.

pub mod camera;
pub mod geometry;
pub mod instancing;
pub mod light;
pub mod material;
pub mod node;
pub mod scene;
pub mod software_render;
pub mod types;

#[cfg(test)]
mod tests;

pub use camera::Scene3DCamera;
pub use geometry::Geometry3D;
pub use instancing::{AudioModulationConfig, InstanceTransform, InstancingBuffer};
pub use light::{Light3DType, Scene3DLight};
pub use material::{MatCapMaterial, Material3D, PbrMaterial, UnlitMaterial, WireframeMaterial};
pub use node::Node3D;
pub use scene::{RenderItem3D, Scene3D, Scene3DBackground};
pub use software_render::render_scene3d_software;
pub use types::{Aabb3D, Matrix4, Quaternion, Transform3D, Vector3};
