//! After Effects Layer Parenting Hierarchy & Transform Stacking.
//!
//! Provides:
//! - 4x4 Transformation Matrix ([`Matrix4`]) with inversion and Euler rotations.
//! - Layer spatial transformation ([`LayerTransform`]) featuring Anchor Point, Position,
//!   Scale, Rotation (Euler), and Opacity following Adobe After Effects semantics:
//!   `M_local = Translate(Position) * Rotate(Rotation) * Scale(Scale) * Translate(-AnchorPoint)`
//!   `M_world = M_parent_world * M_local`
//! - Layer parenting graph ([`ParentingGraph`]) with cycle detection, topological resolution,
//!   and dynamic re-parenting with world transform preservation.

use crate::scene::Transform2D;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

/// Errors arising during layer parenting operations.
#[derive(Debug, Error, PartialEq)]
pub enum ParentingError {
    #[error("layer '{0}' not found in parenting graph")]
    LayerNotFound(String),

    #[error("cyclic parenting dependency detected involving layer '{0}'")]
    CycleDetected(String),

    #[error("failed to invert transformation matrix for layer '{0}'")]
    SingularMatrix(String),
}

/// A 4x4 floating-point transformation matrix represented in row-major order: `m[row][col]`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Matrix4 {
    pub m: [[f32; 4]; 4],
}

impl Default for Matrix4 {
    fn default() -> Self {
        Self::identity()
    }
}

impl Matrix4 {
    /// Identity 4x4 matrix.
    pub const fn identity() -> Self {
        Self {
            m: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Translation matrix.
    pub fn translation(tx: f32, ty: f32, tz: f32) -> Self {
        let mut res = Self::identity();
        res.m[0][3] = tx;
        res.m[1][3] = ty;
        res.m[2][3] = tz;
        res
    }

    /// Scale matrix.
    pub fn scale(sx: f32, sy: f32, sz: f32) -> Self {
        let mut res = Self::identity();
        res.m[0][0] = sx;
        res.m[1][1] = sy;
        res.m[2][2] = sz;
        res
    }

    /// Rotation around the X axis in degrees.
    pub fn rotate_x(deg: f32) -> Self {
        let rad = deg.to_radians();
        let (sin, cos) = (rad.sin(), rad.cos());
        let mut res = Self::identity();
        res.m[1][1] = cos;
        res.m[1][2] = -sin;
        res.m[2][1] = sin;
        res.m[2][2] = cos;
        res
    }

    /// Rotation around the Y axis in degrees.
    pub fn rotate_y(deg: f32) -> Self {
        let rad = deg.to_radians();
        let (sin, cos) = (rad.sin(), rad.cos());
        let mut res = Self::identity();
        res.m[0][0] = cos;
        res.m[0][2] = sin;
        res.m[2][0] = -sin;
        res.m[2][2] = cos;
        res
    }

    /// Rotation around the Z axis in degrees.
    pub fn rotate_z(deg: f32) -> Self {
        let rad = deg.to_radians();
        let (sin, cos) = (rad.sin(), rad.cos());
        let mut res = Self::identity();
        res.m[0][0] = cos;
        res.m[0][1] = -sin;
        res.m[1][0] = sin;
        res.m[1][1] = cos;
        res
    }

    /// After Effects Euler rotation composition: Rotate Z, then Rotate Y, then Rotate X.
    /// `R = R_z * R_y * R_x`
    pub fn rotate_euler(rx_deg: f32, ry_deg: f32, rz_deg: f32) -> Self {
        let rz = Self::rotate_z(rz_deg);
        let ry = Self::rotate_y(ry_deg);
        let rx = Self::rotate_x(rx_deg);
        rz * ry * rx
    }

    /// Multiplies this matrix with another: `self * rhs`.
    pub fn mul(&self, rhs: &Self) -> Self {
        let mut out = [[0.0f32; 4]; 4];
        for i in 0..4 {
            for j in 0..4 {
                let mut sum = 0.0f32;
                for k in 0..4 {
                    sum += self.m[i][k] * rhs.m[k][j];
                }
                out[i][j] = sum;
            }
        }
        Self { m: out }
    }

    /// Transforms a 3D coordinate: `M * [x, y, z, 1]^T`.
    pub fn transform_point(&self, p: [f32; 3]) -> [f32; 3] {
        let x = self.m[0][0] * p[0] + self.m[0][1] * p[1] + self.m[0][2] * p[2] + self.m[0][3];
        let y = self.m[1][0] * p[0] + self.m[1][1] * p[1] + self.m[1][2] * p[2] + self.m[1][3];
        let z = self.m[2][0] * p[0] + self.m[2][1] * p[1] + self.m[2][2] * p[2] + self.m[2][3];
        let w = self.m[3][0] * p[0] + self.m[3][1] * p[1] + self.m[3][2] * p[2] + self.m[3][3];

        if w.abs() > 1e-7 && (w - 1.0).abs() > 1e-7 {
            [x / w, y / w, z / w]
        } else {
            [x, y, z]
        }
    }

    /// Transforms a 2D coordinate on the XY plane: `(x, y) -> (x', y')`.
    pub fn transform_point_2d(&self, x: f32, y: f32) -> (f32, f32) {
        let pt = self.transform_point([x, y, 0.0]);
        (pt[0], pt[1])
    }

    /// Computes the exact matrix inverse using 4x4 Laplace expansion / Cramer's rule.
    pub fn invert(&self) -> Option<Self> {
        let m = &self.m;

        let a00 = m[0][0]; let a01 = m[0][1]; let a02 = m[0][2]; let a03 = m[0][3];
        let a10 = m[1][0]; let a11 = m[1][1]; let a12 = m[1][2]; let a13 = m[1][3];
        let a20 = m[2][0]; let a21 = m[2][1]; let a22 = m[2][2]; let a23 = m[2][3];
        let a30 = m[3][0]; let a31 = m[3][1]; let a32 = m[3][2]; let a33 = m[3][3];

        let b00 = a00 * a11 - a01 * a10;
        let b01 = a00 * a12 - a02 * a10;
        let b02 = a00 * a13 - a03 * a10;
        let b03 = a01 * a12 - a02 * a11;
        let b04 = a01 * a13 - a03 * a11;
        let b05 = a02 * a13 - a03 * a12;
        let b06 = a20 * a31 - a21 * a30;
        let b07 = a20 * a32 - a22 * a30;
        let b08 = a20 * a33 - a23 * a30;
        let b09 = a21 * a32 - a22 * a31;
        let b10 = a21 * a33 - a23 * a31;
        let b11 = a22 * a33 - a23 * a32;

        let det = b00 * b11 - b01 * b10 + b02 * b09 + b03 * b08 - b04 * b07 + b05 * b06;

        if det.abs() < 1e-9 {
            return None;
        }

        let inv_det = 1.0 / det;

        let mut inv = [[0.0f32; 4]; 4];
        inv[0][0] = (a11 * b11 - a12 * b10 + a13 * b09) * inv_det;
        inv[0][1] = (-a01 * b11 + a02 * b10 - a03 * b09) * inv_det;
        inv[0][2] = (a31 * b05 - a32 * b04 + a33 * b03) * inv_det;
        inv[0][3] = (-a21 * b05 + a22 * b04 - a23 * b03) * inv_det;

        inv[1][0] = (-a10 * b11 + a12 * b08 - a13 * b07) * inv_det;
        inv[1][1] = (a00 * b11 - a02 * b08 + a03 * b07) * inv_det;
        inv[1][2] = (-a30 * b05 + a32 * b02 - a33 * b01) * inv_det;
        inv[1][3] = (a20 * b05 - a22 * b02 + a23 * b01) * inv_det;

        inv[2][0] = (a10 * b10 - a11 * b08 + a13 * b06) * inv_det;
        inv[2][1] = (-a00 * b10 + a01 * b08 - a03 * b06) * inv_det;
        inv[2][2] = (a30 * b04 - a31 * b02 + a33 * b00) * inv_det;
        inv[2][3] = (-a20 * b04 + a21 * b02 - a23 * b00) * inv_det;

        inv[3][0] = (-a10 * b09 + a11 * b07 - a12 * b06) * inv_det;
        inv[3][1] = (a00 * b09 - a01 * b07 + a02 * b06) * inv_det;
        inv[3][2] = (-a30 * b03 + a31 * b01 - a32 * b00) * inv_det;
        inv[3][3] = (a20 * b03 - a21 * b01 + a22 * b00) * inv_det;

        Some(Self { m: inv })
    }

    /// Converts the 2D affine submatrix to a `tiny_skia::Transform`.
    pub fn to_tiny_skia(&self) -> tiny_skia::Transform {
        tiny_skia::Transform::from_row(
            self.m[0][0],
            self.m[1][0],
            self.m[0][1],
            self.m[1][1],
            self.m[0][3],
            self.m[1][3],
        )
    }

    /// Converts to a rasterizer `Transform2D`.
    pub fn to_transform2d(&self) -> Transform2D {
        let tx = self.m[0][3];
        let ty = self.m[1][3];

        let m00 = self.m[0][0];
        let m10 = self.m[1][0];
        let m01 = self.m[0][1];
        let m11 = self.m[1][1];

        let scale_x = (m00 * m00 + m10 * m10).sqrt();
        let scale_y = (m01 * m01 + m11 * m11).sqrt();
        let rotate_deg = m10.atan2(m00).to_degrees();

        Transform2D {
            tx,
            ty,
            scale_x,
            scale_y,
            rotate_deg,
        }
    }
}

impl std::ops::Mul for Matrix4 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self::Output {
        Matrix4::mul(&self, &rhs)
    }
}

/// Transform properties of an After Effects layer.
///
/// Follows standard After Effects spatial transform semantics:
/// `AnchorPoint`, `Position`, `Scale`, `Rotation`, and `Opacity`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LayerTransform {
    /// Anchor point in layer-local coordinates `[x, y, z]`. Default `[0.0, 0.0, 0.0]`.
    pub anchor_point: [f32; 3],
    /// Spatial position in parent coordinates `[x, y, z]`. Default `[0.0, 0.0, 0.0]`.
    pub position: [f32; 3],
    /// Scaling factors along `[sx, sy, sz]`. Default `[1.0, 1.0, 1.0]`.
    pub scale: [f32; 3],
    /// Rotation in Euler degrees `[rx, ry, rz]`. Default `[0.0, 0.0, 0.0]`.
    pub rotation: [f32; 3],
    /// Layer opacity in range `0.0..=1.0`. Default `1.0`.
    pub opacity: f32,
}

impl Default for LayerTransform {
    fn default() -> Self {
        Self {
            anchor_point: [0.0, 0.0, 0.0],
            position: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
            rotation: [0.0, 0.0, 0.0],
            opacity: 1.0,
        }
    }
}

impl LayerTransform {
    /// Convenience 2D constructor.
    pub fn new_2d(position: (f32, f32), anchor: (f32, f32), scale: (f32, f32), rotation_deg: f32) -> Self {
        Self {
            anchor_point: [anchor.0, anchor.1, 0.0],
            position: [position.0, position.1, 0.0],
            scale: [scale.0, scale.1, 1.0],
            rotation: [0.0, 0.0, rotation_deg],
            opacity: 1.0,
        }
    }

    /// Computes the local transformation matrix according to After Effects rules:
    /// `M_local = Translate(Position) * Rotate(Rotation) * Scale(Scale) * Translate(-AnchorPoint)`
    pub fn local_matrix(&self) -> Matrix4 {
        let t_neg_a = Matrix4::translation(
            -self.anchor_point[0],
            -self.anchor_point[1],
            -self.anchor_point[2],
        );
        let s = Matrix4::scale(self.scale[0], self.scale[1], self.scale[2]);
        let r = Matrix4::rotate_euler(self.rotation[0], self.rotation[1], self.rotation[2]);
        let t_p = Matrix4::translation(self.position[0], self.position[1], self.position[2]);

        t_p * r * s * t_neg_a
    }

    /// Reconstructs local `position`, `scale`, and `rotation` from a target matrix,
    /// preserving the existing `anchor_point`.
    pub fn from_matrix_with_anchor(matrix: &Matrix4, anchor: [f32; 3], opacity: f32) -> Self {
        // Position: In AE, M * AnchorPoint = Position!
        // M = T(P) * R * S * T(-A) => M * A = T(P) * R * S * 0 = P.
        let new_pos = matrix.transform_point(anchor);

        // Decompose linear 3x3 block:
        let m = &matrix.m;
        let sx = (m[0][0] * m[0][0] + m[1][0] * m[1][0] + m[2][0] * m[2][0]).sqrt();
        let sy = (m[0][1] * m[0][1] + m[1][1] * m[1][1] + m[2][1] * m[2][1]).sqrt();
        let sz = (m[0][2] * m[0][2] + m[1][2] * m[1][2] + m[2][2] * m[2][2]).sqrt();

        // 2D rotation (Z-axis rotation in degrees)
        let rz = m[1][0].atan2(m[0][0]).to_degrees();

        Self {
            anchor_point: anchor,
            position: new_pos,
            scale: [sx, sy, sz],
            rotation: [0.0, 0.0, rz],
            opacity,
        }
    }
}

/// Single node in the layer parenting graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub transform: LayerTransform,
}

/// After Effects Layer Parenting Graph.
///
/// Resolves hierarchical transforms, detects circular dependencies, and enables
/// dynamic re-parenting with transform preservation (so layers don't jump spatially).
#[derive(Debug, Clone, Default)]
pub struct ParentingGraph {
    nodes: HashMap<String, LayerNode>,
}

impl ParentingGraph {
    /// Creates an empty parenting graph.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
        }
    }

    /// Adds or updates a layer in the graph.
    pub fn add_layer(
        &mut self,
        id: impl Into<String>,
        parent_id: Option<String>,
        transform: LayerTransform,
    ) {
        let id_str = id.into();
        self.nodes.insert(
            id_str.clone(),
            LayerNode {
                id: id_str,
                parent_id,
                transform,
            },
        );
    }

    /// Gets a reference to a layer.
    pub fn get_layer(&self, id: &str) -> Option<&LayerNode> {
        self.nodes.get(id)
    }

    /// Gets a mutable reference to a layer.
    pub fn get_layer_mut(&mut self, id: &str) -> Option<&mut LayerNode> {
        self.nodes.get_mut(id)
    }

    /// Checks if adding a parent link `child_id -> new_parent_id` would introduce a cycle.
    pub fn would_create_cycle(&self, child_id: &str, new_parent_id: Option<&str>) -> bool {
        let Some(parent) = new_parent_id else {
            return false;
        };

        if child_id == parent {
            return true;
        }

        let mut curr = Some(parent);
        let mut visited = HashSet::new();
        visited.insert(child_id);

        while let Some(node_id) = curr {
            if !visited.insert(node_id) {
                return true;
            }
            curr = self.nodes.get(node_id).and_then(|n| n.parent_id.as_deref());
        }

        false
    }

    /// Resolves the absolute world transformation matrix for a specific layer.
    ///
    /// Evaluates:
    /// `M_world = M_parent_world * M_local`
    pub fn resolve_world_transform(&self, id: &str) -> Result<Matrix4, ParentingError> {
        if !self.nodes.contains_key(id) {
            return Err(ParentingError::LayerNotFound(id.to_string()));
        }

        // Detect cycle along the ancestor path
        let mut path = Vec::new();
        let mut curr_id = Some(id);
        let mut visited = HashSet::new();

        while let Some(cid) = curr_id {
            if !visited.insert(cid) {
                return Err(ParentingError::CycleDetected(cid.to_string()));
            }
            let n = self
                .nodes
                .get(cid)
                .ok_or_else(|| ParentingError::LayerNotFound(cid.to_string()))?;
            path.push(n);
            curr_id = n.parent_id.as_deref();
        }

        // Multiply from root down to this layer
        let mut world_matrix = Matrix4::identity();
        for ancestor in path.into_iter().rev() {
            world_matrix = world_matrix * ancestor.transform.local_matrix();
        }

        Ok(world_matrix)
    }

    /// Resolves the effective combined opacity for a layer along its parenting hierarchy.
    pub fn resolve_world_opacity(&self, id: &str) -> Result<f32, ParentingError> {
        let mut opacity = 1.0f32;
        let mut curr_id = Some(id);
        let mut visited = HashSet::new();

        while let Some(cid) = curr_id {
            if !visited.insert(cid) {
                return Err(ParentingError::CycleDetected(cid.to_string()));
            }
            let n = self
                .nodes
                .get(cid)
                .ok_or_else(|| ParentingError::LayerNotFound(cid.to_string()))?;
            opacity *= n.transform.opacity;
            curr_id = n.parent_id.as_deref();
        }

        Ok(opacity.clamp(0.0, 1.0))
    }

    /// Resolves world transformation matrices for all layers in the graph.
    pub fn resolve_all_world_transforms(&self) -> Result<HashMap<String, Matrix4>, ParentingError> {
        let mut result = HashMap::with_capacity(self.nodes.len());
        for id in self.nodes.keys() {
            let m = self.resolve_world_transform(id)?;
            result.insert(id.clone(), m);
        }
        Ok(result)
    }

    /// Dynamically re-parents a child layer to a new parent.
    ///
    /// If `preserve_world_transform` is `true`, this method calculates the new local
    /// transform such that the layer's world position, scale, and rotation remain
    /// completely unchanged (preserving spatial continuity without jumping).
    pub fn reparent(
        &mut self,
        child_id: &str,
        new_parent_id: Option<&str>,
        preserve_world_transform: bool,
    ) -> Result<(), ParentingError> {
        if self.would_create_cycle(child_id, new_parent_id) {
            return Err(ParentingError::CycleDetected(child_id.to_string()));
        }

        if !self.nodes.contains_key(child_id) {
            return Err(ParentingError::LayerNotFound(child_id.to_string()));
        }

        if let Some(pid) = new_parent_id {
            if !self.nodes.contains_key(pid) {
                return Err(ParentingError::LayerNotFound(pid.to_string()));
            }
        }

        if preserve_world_transform {
            // 1. Current world matrix of child: M_child_world
            let child_world = self.resolve_world_transform(child_id)?;

            // 2. Target parent's world matrix: M_parent_world
            let parent_world = match new_parent_id {
                Some(pid) => self.resolve_world_transform(pid)?,
                None => Matrix4::identity(),
            };

            // 3. Invert parent matrix: M_parent_world^-1
            let parent_inv = parent_world
                .invert()
                .ok_or_else(|| ParentingError::SingularMatrix(new_parent_id.unwrap_or("root").to_string()))?;

            // 4. New local matrix: M'_child_local = M_parent_world^-1 * M_child_world
            let new_local_matrix = parent_inv * child_world;

            let child_node = self.nodes.get_mut(child_id).unwrap();
            let anchor = child_node.transform.anchor_point;
            let opacity = child_node.transform.opacity;

            child_node.transform =
                LayerTransform::from_matrix_with_anchor(&new_local_matrix, anchor, opacity);
            child_node.parent_id = new_parent_id.map(ToString::to_string);
        } else {
            let child_node = self.nodes.get_mut(child_id).unwrap();
            child_node.parent_id = new_parent_id.map(ToString::to_string);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix4_identity_and_translation() {
        let m = Matrix4::translation(100.0, 50.0, 25.0);
        let pt = m.transform_point([10.0, 20.0, 5.0]);
        assert_eq!(pt, [110.0, 70.0, 30.0]);

        let inv = m.invert().expect("invert should succeed");
        let restored = inv.transform_point(pt);
        assert!((restored[0] - 10.0).abs() < 1e-4);
        assert!((restored[1] - 20.0).abs() < 1e-4);
        assert!((restored[2] - 5.0).abs() < 1e-4);
    }

    #[test]
    fn matrix4_scale_and_rotation() {
        let s = Matrix4::scale(2.0, 3.0, 1.0);
        let r = Matrix4::rotate_z(90.0);
        let combined = s * r;

        let pt = combined.transform_point([1.0, 0.0, 0.0]);
        // [1, 0, 0] rotated 90 deg around Z is [0, 1, 0]. Scaled by [2, 3, 1] is [0, 3, 0].
        assert!((pt[0] - 0.0).abs() < 1e-4);
        assert!((pt[1] - 3.0).abs() < 1e-4);
        assert!((pt[2] - 0.0).abs() < 1e-4);
    }

    #[test]
    fn layer_transform_anchor_point_rotation() {
        // Layer centered at (50, 50) with size 100x100
        // Anchor point at center: (50, 50)
        // Position on canvas: (500, 300)
        // When rotated 90 deg around anchor point, the anchor point remains at (500, 300)
        let lt = LayerTransform {
            anchor_point: [50.0, 50.0, 0.0],
            position: [500.0, 300.0, 0.0],
            scale: [1.0, 1.0, 1.0],
            rotation: [0.0, 0.0, 90.0],
            opacity: 1.0,
        };

        let mat = lt.local_matrix();
        let anchor_world = mat.transform_point([50.0, 50.0, 0.0]);
        assert!((anchor_world[0] - 500.0).abs() < 1e-4);
        assert!((anchor_world[1] - 300.0).abs() < 1e-4);

        // Top-left corner (0, 0) relative to anchor (50, 50) is (-50, -50).
        // Rotated 90 deg: (-50, -50) -> (50, -50).
        // Plus position (500, 300) -> (550, 250).
        let tl_world = mat.transform_point([0.0, 0.0, 0.0]);
        assert!((tl_world[0] - 550.0).abs() < 1e-4);
        assert!((tl_world[1] - 250.0).abs() < 1e-4);
    }

    #[test]
    fn layer_parenting_hierarchy_inheritance() {
        let mut graph = ParentingGraph::new();

        // Parent layer (Arm) at (100, 100) rotated 45 deg
        let parent_transform = LayerTransform::new_2d((100.0, 100.0), (0.0, 0.0), (1.0, 1.0), 45.0);
        graph.add_layer("parent", None, parent_transform);

        // Child layer (Hand) parented to Arm, local offset (50, 0)
        let child_transform = LayerTransform::new_2d((50.0, 0.0), (0.0, 0.0), (1.0, 1.0), 0.0);
        graph.add_layer("child", Some("parent".into()), child_transform);

        let child_world = graph.resolve_world_transform("child").unwrap();
        let pt = child_world.transform_point([0.0, 0.0, 0.0]);

        // (50, 0) rotated 45 deg is (50 * cos45, 50 * sin45) = (35.355, 35.355).
        // Offset by (100, 100) -> (135.355, 135.355).
        let expected = 100.0 + 50.0 * std::f32::consts::FRAC_1_SQRT_2;
        assert!((pt[0] - expected).abs() < 1e-3);
        assert!((pt[1] - expected).abs() < 1e-3);
    }

    #[test]
    fn multi_level_chain_and_opacity_inheritance() {
        let mut graph = ParentingGraph::new();

        let mut t1 = LayerTransform::new_2d((10.0, 0.0), (0.0, 0.0), (2.0, 2.0), 0.0);
        t1.opacity = 0.5;
        graph.add_layer("grandparent", None, t1);

        let mut t2 = LayerTransform::new_2d((20.0, 0.0), (0.0, 0.0), (1.5, 1.5), 0.0);
        t2.opacity = 0.8;
        graph.add_layer("parent", Some("grandparent".into()), t2);

        let mut t3 = LayerTransform::new_2d((5.0, 0.0), (0.0, 0.0), (1.0, 1.0), 0.0);
        t3.opacity = 0.9;
        graph.add_layer("child", Some("parent".into()), t3);

        let world_mat = graph.resolve_world_transform("child").unwrap();
        let world_pt = world_mat.transform_point([0.0, 0.0, 0.0]);

        // Grandparent translates 10, scales 2x.
        // Parent translates 20 in grandparent space -> 10 + 20*2 = 50. Scales 1.5x (total 3x).
        // Child translates 5 in parent space -> 50 + 5*3 = 65.
        assert!((world_pt[0] - 65.0).abs() < 1e-4);

        // Combined opacity: 0.5 * 0.8 * 0.9 = 0.36
        let opacity = graph.resolve_world_opacity("child").unwrap();
        assert!((opacity - 0.36).abs() < 1e-4);
    }

    #[test]
    fn dynamic_reparenting_with_transform_preservation() {
        let mut graph = ParentingGraph::new();

        // Controller / Parent layer
        let parent_transform = LayerTransform::new_2d((400.0, 300.0), (50.0, 50.0), (1.5, 1.5), 30.0);
        graph.add_layer("controller", None, parent_transform);

        // Free layer in world space
        let child_transform = LayerTransform::new_2d((600.0, 450.0), (20.0, 20.0), (2.0, 2.0), 45.0);
        graph.add_layer("item", None, child_transform);

        // World position before reparenting
        let before_world = graph.resolve_world_transform("item").unwrap();
        let before_pt = before_world.transform_point([0.0, 0.0, 0.0]);
        let before_corner = before_world.transform_point([100.0, 100.0, 0.0]);

        // Reparent with transform preservation
        graph
            .reparent("item", Some("controller"), true)
            .expect("reparenting must succeed");

        // World position after reparenting must match EXACTLY
        let after_world = graph.resolve_world_transform("item").unwrap();
        let after_pt = after_world.transform_point([0.0, 0.0, 0.0]);
        let after_corner = after_world.transform_point([100.0, 100.0, 0.0]);

        assert!(
            (before_pt[0] - after_pt[0]).abs() < 1e-3,
            "origin X jumped: before={}, after={}",
            before_pt[0],
            after_pt[0]
        );
        assert!(
            (before_pt[1] - after_pt[1]).abs() < 1e-3,
            "origin Y jumped: before={}, after={}",
            before_pt[1],
            after_pt[1]
        );
        assert!(
            (before_corner[0] - after_corner[0]).abs() < 1e-3,
            "corner X jumped: before={}, after={}",
            before_corner[0],
            after_corner[0]
        );
        assert!(
            (before_corner[1] - after_corner[1]).abs() < 1e-3,
            "corner Y jumped: before={}, after={}",
            before_corner[1],
            after_corner[1]
        );
    }

    #[test]
    fn cyclic_dependency_detection() {
        let mut graph = ParentingGraph::new();

        graph.add_layer("A", None, LayerTransform::default());
        graph.add_layer("B", Some("A".into()), LayerTransform::default());
        graph.add_layer("C", Some("B".into()), LayerTransform::default());

        // Self-parenting A -> A
        assert!(graph.would_create_cycle("A", Some("A")));

        // Cycle A -> C (since C depends on B which depends on A)
        assert!(graph.would_create_cycle("A", Some("C")));

        let res = graph.reparent("A", Some("C"), false);
        assert_eq!(res, Err(ParentingError::CycleDetected("A".into())));
    }
}
