//! 3D Scene Graph Node hierarchy.

use super::geometry::Geometry3D;
use super::instancing::InstancingBuffer;
use super::material::Material3D;
use super::types::Transform3D;
use crate::mesh3d::Mat4;
use serde::{Deserialize, Serialize};

/// A node in the 3D scene hierarchy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node3D {
    /// Optional identifier for targeting/animation.
    pub id: Option<String>,
    /// Local spatial transform relative to parent.
    pub transform: Transform3D,
    /// 3D geometry shape or mesh.
    pub geometry: Geometry3D,
    /// Surface material.
    pub material: Material3D,
    /// Optional GPU instancing buffer.
    pub instancing: Option<InstancingBuffer>,
    /// Whether this node and its children are visible.
    pub visible: bool,
    /// Child nodes in the spatial hierarchy.
    pub children: Vec<Node3D>,
}

impl Node3D {
    pub fn new(geometry: Geometry3D, material: Material3D) -> Self {
        Self {
            id: None,
            transform: Transform3D::default(),
            geometry,
            material,
            instancing: None,
            visible: true,
            children: Vec::new(),
        }
    }

    pub fn with_transform(mut self, transform: Transform3D) -> Self {
        self.transform = transform;
        self
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn with_instancing(mut self, instancing: InstancingBuffer) -> Self {
        self.instancing = Some(instancing);
        self
    }

    pub fn add_child(&mut self, child: Node3D) {
        self.children.push(child);
    }

    /// Evaluates the node's world transform matrix given its parent's world matrix.
    pub fn world_matrix(&self, parent_matrix: &Mat4) -> Mat4 {
        let local = self.transform.to_mat4();
        *parent_matrix * local
    }
}
