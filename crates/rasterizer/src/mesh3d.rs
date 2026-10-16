//! Native 3D procedural geometry & perspective rasterizer (Blender/SOP style).
//!
//! Renders 3D meshes (Cube, Sphere, Torus, 3D Particle Cloud) directly into
//! the 2D scene graph with perspective projection, backface culling, and Lambertian lighting.

use crate::scene::{Color, Scene, SceneNode};
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

/// 3D Vector with basic geometric operations.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl std::ops::Neg for Vec3 {
    type Output = Self;
    fn neg(self) -> Self::Output {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
    }
}

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(self, other: Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    pub fn normalize(self) -> Self {
        let len = self.length();
        if len > 1e-6 {
            Self {
                x: self.x / len,
                y: self.y / len,
                z: self.z / len,
            }
        } else {
            Self::new(0.0, 1.0, 0.0)
        }
    }

    pub fn rotate_x(self, angle_rad: f32) -> Self {
        let cos = angle_rad.cos();
        let sin = angle_rad.sin();
        Self {
            x: self.x,
            y: self.y * cos - self.z * sin,
            z: self.y * sin + self.z * cos,
        }
    }

    pub fn rotate_y(self, angle_rad: f32) -> Self {
        let cos = angle_rad.cos();
        let sin = angle_rad.sin();
        Self {
            x: self.x * cos + self.z * sin,
            y: self.y,
            z: -self.x * sin + self.z * cos,
        }
    }

    pub fn rotate_z(self, angle_rad: f32) -> Self {
        let cos = angle_rad.cos();
        let sin = angle_rad.sin();
        Self {
            x: self.x * cos - self.y * sin,
            y: self.x * sin + self.y * cos,
            z: self.z,
        }
    }

    pub fn rotate(self, pitch: f32, yaw: f32, roll: f32) -> Self {
        self.rotate_x(pitch).rotate_y(yaw).rotate_z(roll)
    }
}

/// A 4-component Quaternion representing 3D rotation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Default for Quat {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Quat {
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w
    }

    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    pub fn normalize(self) -> Self {
        let len = self.length();
        if len > 1e-6 {
            Self {
                x: self.x / len,
                y: self.y / len,
                z: self.z / len,
                w: self.w / len,
            }
        } else {
            Self::IDENTITY
        }
    }

    /// Normalized linear interpolation (fast spherical interpolation approximation).
    pub fn nlerp(self, mut other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        if self.dot(other) < 0.0 {
            other.x = -other.x;
            other.y = -other.y;
            other.z = -other.z;
            other.w = -other.w;
        }
        let one_minus_t = 1.0 - t;
        Self {
            x: self.x * one_minus_t + other.x * t,
            y: self.y * one_minus_t + other.y * t,
            z: self.z * one_minus_t + other.z * t,
            w: self.w * one_minus_t + other.w * t,
        }
        .normalize()
    }

    /// Convert quaternion into a column-major 4x4 rotation matrix.
    pub fn to_mat4(self) -> Mat4 {
        let q = self.normalize();
        let x2 = q.x + q.x;
        let y2 = q.y + q.y;
        let z2 = q.z + q.z;
        let xx = q.x * x2;
        let xy = q.x * y2;
        let xz = q.x * z2;
        let yy = q.y * y2;
        let yz = q.y * z2;
        let zz = q.z * z2;
        let wx = q.w * x2;
        let wy = q.w * y2;
        let wz = q.w * z2;

        Mat4([
            1.0 - (yy + zz),
            xy + wz,
            xz - wy,
            0.0,
            xy - wz,
            1.0 - (xx + zz),
            yz + wx,
            0.0,
            xz + wy,
            yz - wx,
            1.0 - (xx + yy),
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ])
    }
}

/// A 4x4 column-major matrix for 3D affine transforms.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Mat4(pub [f32; 16]);

impl Default for Mat4 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mat4 {
    pub const IDENTITY: Self = Self([
        1.0, 0.0, 0.0, 0.0, // col 0
        0.0, 1.0, 0.0, 0.0, // col 1
        0.0, 0.0, 1.0, 0.0, // col 2
        0.0, 0.0, 0.0, 1.0, // col 3
    ]);

    pub fn from_cols(c0: [f32; 4], c1: [f32; 4], c2: [f32; 4], c3: [f32; 4]) -> Self {
        Self([
            c0[0], c0[1], c0[2], c0[3], c1[0], c1[1], c1[2], c1[3], c2[0], c2[1], c2[2], c2[3],
            c3[0], c3[1], c3[2], c3[3],
        ])
    }

    pub fn from_translation(t: Vec3) -> Self {
        Self([
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, t.x, t.y, t.z, 1.0,
        ])
    }

    pub fn from_scale(s: Vec3) -> Self {
        Self([
            s.x, 0.0, 0.0, 0.0, 0.0, s.y, 0.0, 0.0, 0.0, 0.0, s.z, 0.0, 0.0, 0.0, 0.0, 1.0,
        ])
    }

    pub fn from_translation_rotation_scale(t: Vec3, r: Quat, s: Vec3) -> Self {
        let rot = r.to_mat4();
        // M = T * R * S
        Self([
            rot.0[0] * s.x,
            rot.0[1] * s.x,
            rot.0[2] * s.x,
            0.0,
            rot.0[4] * s.y,
            rot.0[5] * s.y,
            rot.0[6] * s.y,
            0.0,
            rot.0[8] * s.z,
            rot.0[9] * s.z,
            rot.0[10] * s.z,
            0.0,
            t.x,
            t.y,
            t.z,
            1.0,
        ])
    }

    pub fn mul(&self, rhs: &Self) -> Self {
        let mut out = [0.0f32; 16];
        for col in 0..4 {
            for row in 0..4 {
                let mut sum = 0.0;
                for k in 0..4 {
                    sum += self.0[k * 4 + row] * rhs.0[col * 4 + k];
                }
                out[col * 4 + row] = sum;
            }
        }
        Self(out)
    }

    pub fn transform_point(&self, p: Vec3) -> Vec3 {
        let x = self.0[0] * p.x + self.0[4] * p.y + self.0[8] * p.z + self.0[12];
        let y = self.0[1] * p.x + self.0[5] * p.y + self.0[9] * p.z + self.0[13];
        let z = self.0[2] * p.x + self.0[6] * p.y + self.0[10] * p.z + self.0[14];
        let w = self.0[3] * p.x + self.0[7] * p.y + self.0[11] * p.z + self.0[15];
        if w.abs() > 1e-6 && (w - 1.0).abs() > 1e-6 {
            Vec3::new(x / w, y / w, z / w)
        } else {
            Vec3::new(x, y, z)
        }
    }

    pub fn transform_vector(&self, v: Vec3) -> Vec3 {
        let x = self.0[0] * v.x + self.0[4] * v.y + self.0[8] * v.z;
        let y = self.0[1] * v.x + self.0[5] * v.y + self.0[9] * v.z;
        let z = self.0[2] * v.x + self.0[6] * v.y + self.0[10] * v.z;
        Vec3::new(x, y, z)
    }

    pub fn transform_vec3(&self, p: Vec3) -> Vec3 {
        self.transform_point(p)
    }
}

/// Vertex with skeletal bone joint influences for GPU/CPU vertex skinning.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct SkinnedVertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}

/// A 3D procedural mesh (SOP - Surface Operator).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Mesh3D {
    pub vertices: Vec<Vec3>,
    pub faces: Vec<Vec<usize>>,
}

impl Mesh3D {
    pub fn new(vertices: Vec<Vec3>, faces: Vec<Vec<usize>>) -> Self {
        Self { vertices, faces }
    }

    /// Construct a 3D Cube with the given edge length.
    pub fn cube(size: f32) -> Self {
        let h = size * 0.5;
        let vertices = vec![
            Vec3::new(-h, -h, -h), // 0
            Vec3::new(h, -h, -h),  // 1
            Vec3::new(h, h, -h),   // 2
            Vec3::new(-h, h, -h),  // 3
            Vec3::new(-h, -h, h),  // 4
            Vec3::new(h, -h, h),   // 5
            Vec3::new(h, h, h),    // 6
            Vec3::new(-h, h, h),   // 7
        ];

        let faces = vec![
            vec![0, 1, 2, 3], // front (-Z)
            vec![5, 4, 7, 6], // back (+Z)
            vec![4, 0, 3, 7], // left (-X)
            vec![1, 5, 6, 2], // right (+X)
            vec![3, 2, 6, 7], // top (+Y)
            vec![4, 5, 1, 0], // bottom (-Y)
        ];

        Self { vertices, faces }
    }

    /// Construct a UV Sphere with the given radius and resolution.
    pub fn sphere(radius: f32, rings: usize, sectors: usize) -> Self {
        let mut vertices = Vec::new();
        let mut faces = Vec::new();

        let r_step = PI / rings.max(2) as f32;
        let s_step = (2.0 * PI) / sectors.max(3) as f32;

        for i in 0..=rings {
            let phi = i as f32 * r_step;
            let y = radius * phi.cos();
            let ring_r = radius * phi.sin();

            for j in 0..=sectors {
                let theta = j as f32 * s_step;
                let x = ring_r * theta.sin();
                let z = ring_r * theta.cos();
                vertices.push(Vec3::new(x, y, z));
            }
        }

        let stride = sectors + 1;
        for i in 0..rings {
            for j in 0..sectors {
                let first = i * stride + j;
                let second = first + stride;
                faces.push(vec![first, second, second + 1, first + 1]);
            }
        }

        Self { vertices, faces }
    }

    /// Construct a 3D Torus (donut) with major and minor radii.
    pub fn torus(r_major: f32, r_minor: f32, segs_major: usize, segs_minor: usize) -> Self {
        let mut vertices = Vec::new();
        let mut faces = Vec::new();

        let u_step = (2.0 * PI) / segs_major.max(3) as f32;
        let v_step = (2.0 * PI) / segs_minor.max(3) as f32;

        for i in 0..=segs_major {
            let u = i as f32 * u_step;
            let cos_u = u.cos();
            let sin_u = u.sin();

            for j in 0..=segs_minor {
                let v = j as f32 * v_step;
                let cos_v = v.cos();
                let sin_v = v.sin();

                let x = (r_major + r_minor * cos_v) * cos_u;
                let y = r_minor * sin_v;
                let z = (r_major + r_minor * cos_v) * sin_u;
                vertices.push(Vec3::new(x, y, z));
            }
        }

        let stride = segs_minor + 1;
        for i in 0..segs_major {
            for j in 0..segs_minor {
                let first = i * stride + j;
                let second = first + stride;
                faces.push(vec![first, second, second + 1, first + 1]);
            }
        }

        Self { vertices, faces }
    }

    /// Apply 3D Euler rotation (pitch, yaw, roll) in radians.
    pub fn rotate(&mut self, pitch: f32, yaw: f32, roll: f32) {
        for v in &mut self.vertices {
            *v = v.rotate(pitch, yaw, roll);
        }
    }

    /// Scale all vertex coordinates.
    pub fn scale(&mut self, factor: f32) {
        for v in &mut self.vertices {
            v.x *= factor;
            v.y *= factor;
            v.z *= factor;
        }
    }

    /// Render this 3D mesh into a [`Scene`] with perspective projection and Lambertian diffuse shading.
    #[allow(clippy::too_many_arguments)]
    /// Render with a per-frame rotation without cloning the mesh topology.
    /// The source vertices remain shared and only the projected coordinates
    /// are materialized for this frame.
    #[allow(clippy::too_many_arguments)]
    pub fn render_rotated_to_scene(
        &self,
        scene: &mut Scene,
        center_x: f32,
        center_y: f32,
        camera_dist: f32,
        base_color: Color,
        light_dir: Vec3,
        wireframe: bool,
        rotation: Vec3,
    ) {
        let rotated: Vec<Vec3> = self
            .vertices
            .iter()
            .map(|vertex| vertex.rotate(rotation.x, rotation.y, rotation.z))
            .collect();
        self.render_vertices_to_scene(
            &rotated,
            scene,
            center_x,
            center_y,
            camera_dist,
            base_color,
            light_dir,
            wireframe,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_to_scene(
        &self,
        scene: &mut Scene,
        center_x: f32,
        center_y: f32,
        camera_dist: f32,
        base_color: Color,
        light_dir: Vec3,
        wireframe: bool,
    ) {
        self.render_vertices_to_scene(
            &self.vertices,
            scene,
            center_x,
            center_y,
            camera_dist,
            base_color,
            light_dir,
            wireframe,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn render_vertices_to_scene(
        &self,
        vertices: &[Vec3],
        scene: &mut Scene,
        center_x: f32,
        center_y: f32,
        camera_dist: f32,
        base_color: Color,
        light_dir: Vec3,
        wireframe: bool,
    ) {
        let light = light_dir.normalize();
        let cam_dist = camera_dist.max(100.0);

        // Project vertices
        let mut projected: Vec<(f32, f32, f32)> = Vec::with_capacity(vertices.len());
        for v in vertices {
            let z_eye = v.z + cam_dist;
            let scale = if z_eye > 1.0 { cam_dist / z_eye } else { 1.0 };
            let sx = center_x + v.x * scale;
            let sy = center_y - v.y * scale; // Flip Y for screen space
            projected.push((sx, sy, z_eye));
        }

        // Sort faces by depth (Painter's algorithm: farthest first)
        struct ProjectedFace<'a> {
            depth: f32,
            indices: &'a [usize],
            color: Color,
        }

        let mut sorted_faces: Vec<ProjectedFace<'_>> = Vec::with_capacity(self.faces.len());

        for face in &self.faces {
            if face.len() < 3 {
                continue;
            }

            let v0 = vertices[face[0]];
            let v1 = vertices[face[1]];
            let v2 = vertices[face[2]];

            let edge1 = Vec3::new(v1.x - v0.x, v1.y - v0.y, v1.z - v0.z);
            let edge2 = Vec3::new(v2.x - v0.x, v2.y - v0.y, v2.z - v0.z);
            let normal = edge1.cross(edge2).normalize();

            // Backface culling: if normal faces away from camera (+Z towards viewer), skip
            if !wireframe && normal.z <= 0.0 {
                continue;
            }

            // Lambertian diffuse shading
            let diffuse = normal.dot(light).max(0.0);
            let ambient = 0.25f32;
            let intensity = (ambient + diffuse * 0.75).clamp(0.0, 1.0);

            let shaded_color = Color::rgba(
                (base_color.r as f32 * intensity).round() as u8,
                (base_color.g as f32 * intensity).round() as u8,
                (base_color.b as f32 * intensity).round() as u8,
                base_color.a,
            );

            // Average depth
            let depth: f32 =
                face.iter().map(|&idx| projected[idx].2).sum::<f32>() / face.len() as f32;

            sorted_faces.push(ProjectedFace {
                depth,
                indices: face,
                color: shaded_color,
            });
        }

        // Sort descending by depth
        sorted_faces.sort_by(|a, b| {
            b.depth
                .partial_cmp(&a.depth)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Emit SVG path polygons for each face
        for face in sorted_faces {
            let mut d = String::new();
            for (i, &idx) in face.indices.iter().enumerate() {
                let (px, py, _) = projected[idx];
                if i == 0 {
                    d.push_str(&format!("M {px:.1} {py:.1} "));
                } else {
                    d.push_str(&format!("L {px:.1} {py:.1} "));
                }
            }
            d.push('Z');

            if wireframe {
                scene.push(SceneNode::Path {
                    d,
                    fill: None,
                    stroke: Some(face.color),
                    stroke_width: 1.5,
                    opacity: 1.0,
                });
            } else {
                scene.push(SceneNode::Path {
                    d,
                    fill: Some(face.color),
                    stroke: Some(Color::rgba(0, 0, 0, 40)),
                    stroke_width: 0.5,
                    opacity: 1.0,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cube_construction_and_rotation() {
        let mut cube = Mesh3D::cube(100.0);
        assert_eq!(cube.vertices.len(), 8);
        assert_eq!(cube.faces.len(), 6);

        cube.rotate(PI * 0.25, PI * 0.25, 0.0);
        let mut scene = Scene::default();
        cube.render_to_scene(
            &mut scene,
            640.0,
            360.0,
            500.0,
            Color::rgb(255, 120, 0),
            Vec3::new(0.5, 1.0, 1.0),
            false,
        );

        assert!(
            !scene.nodes.is_empty(),
            "Scene should contain projected faces"
        );
    }

    #[test]
    fn test_sphere_and_torus_construction() {
        let sphere = Mesh3D::sphere(50.0, 8, 8);
        assert!(!sphere.vertices.is_empty());
        assert!(!sphere.faces.is_empty());

        let torus = Mesh3D::torus(60.0, 20.0, 8, 8);
        assert!(!torus.vertices.is_empty());
        assert!(!torus.faces.is_empty());
    }
}
