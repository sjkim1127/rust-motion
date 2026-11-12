//! 3D Virtual Camera for Scene3D graph.

use crate::camera3d::{Camera3D as AeCamera3D, CameraType, DepthOfField};
use crate::mesh3d::{Mat4, Vec3};
use serde::{Deserialize, Serialize};

/// 3D Virtual Camera for `Scene3D`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Scene3DCamera {
    /// Eye position in 3D world space.
    pub position: Vec3,
    /// Point of interest / target position the camera looks toward.
    pub target: Vec3,
    /// World up vector (usually `(0.0, 1.0, 0.0)`).
    pub up: Vec3,
    /// Vertical field of view in degrees (default `45.0`).
    pub fov_y_deg: f32,
    /// Near clipping plane distance (default `0.1`).
    pub near_clip: f32,
    /// Far clipping plane distance (default `10000.0`).
    pub far_clip: f32,
    /// Whether the camera is in orthographic projection mode.
    pub is_orthographic: bool,
    /// Orthographic view extent scale (half-height in world units).
    pub ortho_scale: f32,
    /// Optional depth-of-field physical lens simulation.
    pub dof: Option<DepthOfField>,
}

impl Default for Scene3DCamera {
    fn default() -> Self {
        Self {
            position: Vec3::new(0.0, 0.0, 500.0),
            target: Vec3::new(0.0, 0.0, 0.0),
            up: Vec3::new(0.0, 1.0, 0.0),
            fov_y_deg: 45.0,
            near_clip: 0.1,
            far_clip: 10000.0,
            is_orthographic: false,
            ortho_scale: 500.0,
            dof: None,
        }
    }
}

impl Scene3DCamera {
    /// Creates a perspective camera.
    pub fn perspective(position: Vec3, target: Vec3, fov_y_deg: f32) -> Self {
        Self {
            position,
            target,
            fov_y_deg,
            is_orthographic: false,
            ..Default::default()
        }
    }

    /// Creates an orthographic camera.
    pub fn orthographic(position: Vec3, target: Vec3, ortho_scale: f32) -> Self {
        Self {
            position,
            target,
            is_orthographic: true,
            ortho_scale,
            ..Default::default()
        }
    }

    /// Computes the look-at 4x4 view matrix.
    pub fn view_matrix(&self) -> Mat4 {
        let forward = Vec3::new(
            self.target.x - self.position.x,
            self.target.y - self.position.y,
            self.target.z - self.position.z,
        )
        .normalize();

        // If camera looks straight up or down parallel to up vector, adjust up
        let up = if forward.cross(self.up).length() < 1e-4 {
            Vec3::new(0.0, 0.0, 1.0)
        } else {
            self.up.normalize()
        };

        let right = forward.cross(up).normalize();
        let true_up = right.cross(forward).normalize();

        // Right-handed view matrix looking down -Z
        Mat4([
            right.x,
            true_up.x,
            -forward.x,
            0.0,
            right.y,
            true_up.y,
            -forward.y,
            0.0,
            right.z,
            true_up.z,
            -forward.z,
            0.0,
            -self.position.dot(right),
            -self.position.dot(true_up),
            self.position.dot(forward),
            1.0,
        ])
    }

    /// Computes the 4x4 projection matrix.
    pub fn projection_matrix(&self, aspect_ratio: f32) -> Mat4 {
        let aspect = if aspect_ratio > 1e-4 {
            aspect_ratio
        } else {
            1.0
        };
        let near = self.near_clip.max(0.001);
        let far = self.far_clip.max(near + 0.1);

        if self.is_orthographic {
            let top = self.ortho_scale;
            let bottom = -top;
            let right = top * aspect;
            let left = -right;
            let range_inv = 1.0 / (near - far);

            Mat4([
                2.0 / (right - left),
                0.0,
                0.0,
                0.0,
                0.0,
                2.0 / (top - bottom),
                0.0,
                0.0,
                0.0,
                0.0,
                range_inv,
                0.0,
                -(right + left) / (right - left),
                -(top + bottom) / (top - bottom),
                near * range_inv,
                1.0,
            ])
        } else {
            let fov_rad = self.fov_y_deg.to_radians();
            let f = 1.0 / (fov_rad * 0.5).tan();
            let range_inv = 1.0 / (near - far);

            Mat4([
                f / aspect,
                0.0,
                0.0,
                0.0,
                0.0,
                f,
                0.0,
                0.0,
                0.0,
                0.0,
                (far + near) * range_inv,
                -1.0,
                0.0,
                0.0,
                (2.0 * far * near) * range_inv,
                0.0,
            ])
        }
    }

    /// Computes the combined View-Projection matrix: `P * V`.
    pub fn view_projection_matrix(&self, aspect_ratio: f32) -> Mat4 {
        let p = self.projection_matrix(aspect_ratio);
        let v = self.view_matrix();
        p * v
    }

    /// Projects a 3D world coordinate to 2D pixel coordinates and camera depth.
    /// Returns `(pixel_x, pixel_y, depth)` where pixel coords are `[0, screen_w]` and `[0, screen_h]`.
    pub fn project_point(
        &self,
        point: Vec3,
        screen_w: f32,
        screen_h: f32,
    ) -> Option<(f32, f32, f32)> {
        if screen_w <= 0.0 || screen_h <= 0.0 {
            return None;
        }
        let aspect = screen_w / screen_h;
        let vp = self.view_projection_matrix(aspect);

        // Eye-space depth check
        let view = self.view_matrix();
        let eye_pt = view.transform_point(point);
        let cam_depth = (-eye_pt.z).max(0.001);
        if eye_pt.z > -self.near_clip || eye_pt.z < -self.far_clip {
            return None;
        }

        let ndc = vp.transform_point(point);
        let screen_x = (ndc.x * 0.5 + 0.5) * screen_w;
        let screen_y = (-ndc.y * 0.5 + 0.5) * screen_h;

        Some((screen_x, screen_y, cam_depth))
    }

    /// Converts to the After Effects compatible `Camera3D`.
    pub fn to_ae_camera(&self) -> AeCamera3D {
        AeCamera3D {
            camera_type: CameraType::TwoNode,
            position: self.position,
            point_of_interest: self.target,
            orientation_deg: Vec3::default(),
            focal_length_mm: 50.0,
            film_gate_mm: (36.0, 24.0),
            near_clip: self.near_clip,
            far_clip: self.far_clip,
            dof: self.dof.unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camera_projection() {
        let cam =
            Scene3DCamera::perspective(Vec3::new(0.0, 0.0, 100.0), Vec3::new(0.0, 0.0, 0.0), 60.0);
        let pt = Vec3::new(0.0, 0.0, 0.0);
        let proj = cam.project_point(pt, 1920.0, 1080.0);
        assert!(proj.is_some());
        let (x, y, depth) = proj.unwrap();
        assert!((x - 960.0).abs() < 1e-2);
        assert!((y - 540.0).abs() < 1e-2);
        assert!((depth - 100.0).abs() < 1e-2);
    }
}
