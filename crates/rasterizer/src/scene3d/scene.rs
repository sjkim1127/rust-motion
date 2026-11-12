//! 3D Scene container and render item flattening.

use super::camera::Scene3DCamera;
use super::geometry::Geometry3D;
use super::instancing::InstancingBuffer;
use super::light::Scene3DLight;
use super::material::Material3D;
use super::node::Node3D;
use crate::mesh3d::Mat4;
use crate::scene::Color;
use serde::{Deserialize, Serialize};

/// Background clear configuration for `Scene3D`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Scene3DBackground {
    /// Transparent clear (composites over underlying 2D elements).
    #[default]
    Transparent,
    /// Solid background color.
    Solid { color: Color },
    /// HDRI / panoramic skybox image.
    Skybox { texture: String },
}

/// A complete 3D scene graph ready for rendering.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Scene3D {
    /// Virtual camera.
    pub camera: Scene3DCamera,
    /// Dynamic light sources.
    pub lights: Vec<Scene3DLight>,
    /// Scene background mode.
    pub background: Scene3DBackground,
    /// Root node hierarchy.
    pub nodes: Vec<Node3D>,
    /// Whether to clear the depth buffer before rendering this 3D pass.
    #[serde(default = "default_true")]
    pub clear_depth: bool,
}

const fn default_true() -> bool {
    true
}

/// A flattened renderable element with its computed world transform.
#[derive(Debug, Clone)]
pub struct RenderItem3D<'a> {
    pub world_matrix: Mat4,
    pub geometry: &'a Geometry3D,
    pub material: &'a Material3D,
    pub instancing: Option<&'a InstancingBuffer>,
}

impl Scene3D {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_camera(mut self, camera: Scene3DCamera) -> Self {
        self.camera = camera;
        self
    }

    pub fn with_background(mut self, background: Scene3DBackground) -> Self {
        self.background = background;
        self
    }

    pub fn add_light(&mut self, light: Scene3DLight) {
        self.lights.push(light);
    }

    pub fn add_node(&mut self, node: Node3D) {
        self.nodes.push(node);
    }

    pub fn find_node(&self, id: &str) -> Option<&Node3D> {
        for node in &self.nodes {
            if let Some(found) = find_node_recursive(node, id) {
                return Some(found);
            }
        }
        None
    }

    /// Flattens the scene hierarchy into an ordered list of [`RenderItem3D`] with world matrices.
    pub fn collect_render_items(&self) -> Vec<RenderItem3D<'_>> {
        let mut items = Vec::new();
        let identity = Mat4::IDENTITY;
        for node in &self.nodes {
            collect_node_recursive(node, &identity, &mut items);
        }
        items
    }
}

fn find_node_recursive<'a>(node: &'a Node3D, id: &str) -> Option<&'a Node3D> {
    if node.id.as_deref() == Some(id) {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_node_recursive(child, id) {
            return Some(found);
        }
    }
    None
}

fn collect_node_recursive<'a>(
    node: &'a Node3D,
    parent_matrix: &Mat4,
    items: &mut Vec<RenderItem3D<'a>>,
) {
    if !node.visible {
        return;
    }
    let world_matrix = node.world_matrix(parent_matrix);
    items.push(RenderItem3D {
        world_matrix,
        geometry: &node.geometry,
        material: &node.material,
        instancing: node.instancing.as_ref(),
    });
    for child in &node.children {
        collect_node_recursive(child, &world_matrix, items);
    }
}
