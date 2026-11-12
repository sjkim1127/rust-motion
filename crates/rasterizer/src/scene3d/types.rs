//! 3D spatial transformations, bounding boxes, and linear algebra primitives.

use crate::mesh3d::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

pub use crate::mesh3d::{Mat4 as Matrix4, Quat as Quaternion, Vec3 as Vector3};

/// Axis-aligned 3D bounding box (AABB).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb3D {
    pub min: Vec3,
    pub max: Vec3,
}

impl Default for Aabb3D {
    fn default() -> Self {
        Self {
            min: Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY),
            max: Vec3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
        }
    }
}

impl Aabb3D {
    pub const fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    pub fn from_points(points: &[Vec3]) -> Self {
        if points.is_empty() {
            return Self::default();
        }
        let mut min = points[0];
        let mut max = points[0];
        for &p in &points[1..] {
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            min.z = min.z.min(p.z);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
            max.z = max.z.max(p.z);
        }
        Self { min, max }
    }

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    pub fn center(&self) -> Vec3 {
        Vec3::new(
            (self.min.x + self.max.x) * 0.5,
            (self.min.y + self.max.y) * 0.5,
            (self.min.z + self.max.z) * 0.5,
        )
    }

    pub fn size(&self) -> Vec3 {
        Vec3::new(
            (self.max.x - self.min.x).max(0.0),
            (self.max.y - self.min.y).max(0.0),
            (self.max.z - self.min.z).max(0.0),
        )
    }

    pub fn union(&self, other: &Self) -> Self {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        Self {
            min: Vec3::new(
                self.min.x.min(other.min.x),
                self.min.y.min(other.min.y),
                self.min.z.min(other.min.z),
            ),
            max: Vec3::new(
                self.max.x.max(other.max.x),
                self.max.y.max(other.max.y),
                self.max.z.max(other.max.z),
            ),
        }
    }
}

/// 3D Transform represented by Translation, Rotation (Quaternion), and Scale.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform3D {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform3D {
    fn default() -> Self {
        Self {
            translation: Vec3::default(),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(1.0, 1.0, 1.0),
        }
    }
}

impl Transform3D {
    pub const IDENTITY: Self = Self {
        translation: Vec3::new(0.0, 0.0, 0.0),
        rotation: Quat::IDENTITY,
        scale: Vec3::new(1.0, 1.0, 1.0),
    };

    pub fn from_translation(t: Vec3) -> Self {
        Self {
            translation: t,
            ..Default::default()
        }
    }

    pub fn from_euler(pitch_rad: f32, yaw_rad: f32, roll_rad: f32) -> Self {
        Self {
            rotation: Quat::from_euler(pitch_rad, yaw_rad, roll_rad),
            ..Default::default()
        }
    }

    pub fn from_scale(s: Vec3) -> Self {
        Self {
            scale: s,
            ..Default::default()
        }
    }

    pub fn from_translation_rotation_scale(t: Vec3, r: Quat, s: Vec3) -> Self {
        Self {
            translation: t,
            rotation: r,
            scale: s,
        }
    }

    /// Computes the 4x4 affine transform matrix `M = T * R * S`.
    pub fn to_mat4(&self) -> Mat4 {
        Mat4::from_translation_rotation_scale(self.translation, self.rotation, self.scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_matrix() {
        let t = Transform3D {
            translation: Vec3::new(10.0, 20.0, 30.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(2.0, 2.0, 2.0),
        };
        let m = t.to_mat4();
        let pt = Vec3::new(1.0, 1.0, 1.0);
        let transformed = m.transform_vec3(pt);
        assert!((transformed.x - 12.0).abs() < 1e-4);
        assert!((transformed.y - 22.0).abs() < 1e-4);
        assert!((transformed.z - 32.0).abs() < 1e-4);
    }

    #[test]
    fn test_aabb_union() {
        let b1 = Aabb3D::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0));
        let b2 = Aabb3D::new(Vec3::new(-1.0, 0.5, 0.0), Vec3::new(2.0, 3.0, 1.0));
        let u = b1.union(&b2);
        assert_eq!(u.min, Vec3::new(-1.0, 0.0, 0.0));
        assert_eq!(u.max, Vec3::new(2.0, 3.0, 1.0));
    }
}
