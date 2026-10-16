//! After Effects 3D Virtual Camera, Point/Spot Lights, & Depth of Field (DoF/Bokeh).
//!
//! Provides:
//! - One-Node and Two-Node Virtual Cameras ([`Camera3D`]) with focal length, FOV, and film gate.
//! - Depth of Field simulation ([`DepthOfField`]) with focus distance, f-stop, Circle of Confusion (CoC),
//!   and multi-blade Bokeh diaphragm shapes ([`BokehShape`]: Circular, Hexagonal, Octagonal).
//! - Virtual 3D Light sources ([`LightSource`]): Ambient, Point (inverse-square decay), Spot (cone angle & feather), Directional.
//! - Material properties and Blinn-Phong lighting evaluation ([`Material3D`]).
//! - Planar shadow projection matrix generation.

use crate::mesh3d::{Mat4, Vec3};
use crate::scene::Color;
use serde::{Deserialize, Serialize};

/// Camera navigation topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CameraType {
    /// Free-flying camera oriented by Euler angles (Pitch, Yaw, Roll).
    OneNode,
    /// Targeted camera constrained to always point toward a Point of Interest / Target.
    #[default]
    TwoNode,
}

/// Bokeh aperture diaphragm blade shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BokehShape {
    /// Perfectly circular aperture diaphragm.
    #[default]
    Circular,
    /// 6-blade hexagonal bokeh diaphragm.
    Hexagonal,
    /// 8-blade octagonal bokeh diaphragm.
    Octagonal,
}

impl BokehShape {
    /// Number of diaphragm blades for polygonal bokeh shapes.
    pub fn blade_count(&self) -> u32 {
        match self {
            Self::Circular => 32,
            Self::Hexagonal => 6,
            Self::Octagonal => 8,
        }
    }
}

/// Depth of field and physical lens optics settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DepthOfField {
    /// Whether Depth of Field simulation is active. Default `true`.
    pub enabled: bool,
    /// Distance from the camera lens to the in-focus critical plane in pixels/units.
    pub focus_distance: f32,
    /// Lens aperture f-stop number (e.g. 1.4, 2.8, 5.6, 8.0, 16.0). Lower = shallower DoF / larger bokeh.
    pub f_stop: f32,
    /// Diaphragm blade geometry.
    pub bokeh_shape: BokehShape,
    /// Blur scale multiplier. Default `1.0`.
    pub blur_level: f32,
}

impl Default for DepthOfField {
    fn default() -> Self {
        Self {
            enabled: true,
            focus_distance: 1000.0,
            f_stop: 2.8,
            bokeh_shape: BokehShape::Circular,
            blur_level: 1.0,
        }
    }
}

/// Virtual 3D Camera matching Adobe After Effects camera mechanics.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Camera3D {
    /// Navigation type: One-Node (free) or Two-Node (target tracking).
    pub camera_type: CameraType,
    /// Camera position in 3D world space.
    pub position: Vec3,
    /// Point of interest target for Two-Node camera.
    pub point_of_interest: Vec3,
    /// Euler orientation angles `(pitch, yaw, roll)` in degrees for One-Node camera, or roll for Two-Node.
    pub orientation_deg: Vec3,
    /// Lens focal length in millimeters (e.g. 15mm, 28mm, 35mm, 50mm, 85mm, 135mm).
    pub focal_length_mm: f32,
    /// Virtual film sensor / gate size in millimeters `(width, height)`. Standard 35mm full frame is `(36.0, 24.0)`.
    pub film_gate_mm: (f32, f32),
    /// Near clipping plane distance.
    pub near_clip: f32,
    /// Far clipping plane distance.
    pub far_clip: f32,
    /// Depth of field optical configuration.
    pub dof: DepthOfField,
}

impl Default for Camera3D {
    fn default() -> Self {
        Self {
            camera_type: CameraType::TwoNode,
            position: Vec3::new(960.0, 540.0, -1500.0),
            point_of_interest: Vec3::new(960.0, 540.0, 0.0),
            orientation_deg: Vec3::new(0.0, 0.0, 0.0),
            focal_length_mm: 50.0,
            film_gate_mm: (36.0, 24.0),
            near_clip: 10.0,
            far_clip: 50000.0,
            dof: DepthOfField::default(),
        }
    }
}

impl Camera3D {
    /// Preset 35mm wide-angle camera (approx 54° horizontal FOV).
    pub fn preset_35mm() -> Self {
        Self {
            focal_length_mm: 35.0,
            ..Default::default()
        }
    }

    /// Preset 50mm normal prime camera (approx 39.6° horizontal FOV).
    pub fn preset_50mm() -> Self {
        Self {
            focal_length_mm: 50.0,
            ..Default::default()
        }
    }

    /// Preset 85mm portrait telephoto camera (approx 24° horizontal FOV, shallow DoF).
    pub fn preset_85mm() -> Self {
        Self {
            focal_length_mm: 85.0,
            dof: DepthOfField {
                f_stop: 1.8,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Horizontal Field of View (FOV) in radians.
    pub fn fov_x_rad(&self) -> f32 {
        2.0 * (self.film_gate_mm.0 / (2.0 * self.focal_length_mm)).atan()
    }

    /// Vertical Field of View (FOV) in radians.
    pub fn fov_y_rad(&self) -> f32 {
        2.0 * (self.film_gate_mm.1 / (2.0 * self.focal_length_mm)).atan()
    }

    /// Calculates the 4x4 View Matrix: world space -> camera eye space.
    pub fn view_matrix(&self) -> Mat4 {
        match self.camera_type {
            CameraType::TwoNode => {
                let forward = Vec3::new(
                    self.point_of_interest.x - self.position.x,
                    self.point_of_interest.y - self.position.y,
                    self.point_of_interest.z - self.position.z,
                )
                .normalize();

                let up_hint = Vec3::new(0.0, -1.0, 0.0); // Y down for 2D screen coordinates
                let right = forward.cross(up_hint).normalize();
                let up = right.cross(forward).normalize();

                // Apply roll rotation around forward axis
                let roll_rad = self.orientation_deg.z.to_radians();
                let (cos_r, sin_r) = (roll_rad.cos(), roll_rad.sin());
                let final_right = Vec3::new(
                    right.x * cos_r - up.x * sin_r,
                    right.y * cos_r - up.y * sin_r,
                    right.z * cos_r - up.z * sin_r,
                );
                let final_up = Vec3::new(
                    right.x * sin_r + up.x * cos_r,
                    right.y * sin_r + up.y * cos_r,
                    right.z * sin_r + up.z * cos_r,
                );

                Mat4([
                    final_right.x, final_up.x, -forward.x, 0.0,
                    final_right.y, final_up.y, -forward.y, 0.0,
                    final_right.z, final_up.z, -forward.z, 0.0,
                    -self.position.dot(final_right),
                    -self.position.dot(final_up),
                    self.position.dot(forward),
                    1.0,
                ])
            }
            CameraType::OneNode => {
                let pitch_rad = self.orientation_deg.x.to_radians();
                let yaw_rad = self.orientation_deg.y.to_radians();
                let roll_rad = self.orientation_deg.z.to_radians();

                let (cp, sp) = (pitch_rad.cos(), pitch_rad.sin());
                let (cy, sy) = (yaw_rad.cos(), yaw_rad.sin());
                let (cr, sr) = (roll_rad.cos(), roll_rad.sin());

                let rx = Vec3::new(cy * cr + sy * sp * sr, cp * sr, -sy * cr + cy * sp * sr);
                let ry = Vec3::new(-cy * sr + sy * sp * cr, cp * cr, sy * sr + cy * sp * cr);
                let rz = Vec3::new(sy * cp, -sp, cy * cp);

                Mat4([
                    rx.x, ry.x, rz.x, 0.0,
                    rx.y, ry.y, rz.y, 0.0,
                    rx.z, ry.z, rz.z, 0.0,
                    -self.position.dot(rx),
                    -self.position.dot(ry),
                    -self.position.dot(rz),
                    1.0,
                ])
            }
        }
    }

    /// Calculates 4x4 Perspective Projection Matrix: camera eye space -> NDC `[-1, 1]`.
    pub fn projection_matrix(&self, aspect_ratio: f32) -> Mat4 {
        let fov_y = self.fov_y_rad();
        let f = 1.0 / (fov_y * 0.5).tan();
        let near = self.near_clip;
        let far = self.far_clip;
        let range_inv = 1.0 / (near - far);

        Mat4([
            f / aspect_ratio, 0.0, 0.0, 0.0,
            0.0, f, 0.0, 0.0,
            0.0, 0.0, (far + near) * range_inv, -1.0,
            0.0, 0.0, (2.0 * far * near) * range_inv, 0.0,
        ])
    }

    /// Projects a 3D world coordinate to 2D pixel coordinates and camera depth.
    ///
    /// Returns `(pixel_x, pixel_y, depth_from_camera)`.
    pub fn project_point(&self, point: Vec3, screen_w: f32, screen_h: f32) -> Option<(f32, f32, f32)> {
        let view = self.view_matrix();
        let eye_pt = view.transform_vec3(point);

        // In eye space, looking down negative Z (or forward Z):
        let cam_depth = (-eye_pt.z).max(0.001);
        if eye_pt.z > -self.near_clip || eye_pt.z < -self.far_clip {
            return None; // Clipped
        }

        let aspect = screen_w / screen_h;
        let proj = self.projection_matrix(aspect);
        let ndc = proj.transform_vec3(eye_pt);

        // Screen mapping: NDC [-1, 1] -> [0, W] and [0, H]
        let screen_x = (ndc.x * 0.5 + 0.5) * screen_w;
        let screen_y = (-ndc.y * 0.5 + 0.5) * screen_h;

        Some((screen_x, screen_y, cam_depth))
    }

    /// Calculates the Circle of Confusion (CoC) blur radius in pixels at depth `distance_from_camera`.
    ///
    /// Based on the thin-lens optics formula:
    /// $C = \left| A \cdot \frac{F \cdot (d - S)}{d \cdot (S - F)} \right| \cdot \text{scale}$
    pub fn circle_of_confusion_px(&self, distance_from_camera: f32, screen_h: f32) -> f32 {
        if !self.dof.enabled || self.dof.f_stop <= 0.0 {
            return 0.0;
        }

        let f = self.focal_length_mm;
        let s = self.dof.focus_distance; // in mm or scaled units
        let d = distance_from_camera.max(f + 1.0);
        let aperture_diameter = f / self.dof.f_stop;

        let num = aperture_diameter * f * (d - s).abs();
        let denom = d * (s - f).abs().max(1e-4);

        let coc_sensor_mm = num / denom;

        // Convert sensor mm to screen pixels:
        let sensor_h = self.film_gate_mm.1.max(1.0);
        let coc_px = (coc_sensor_mm / sensor_h) * screen_h * self.dof.blur_level;

        coc_px.clamp(0.0, 128.0)
    }
}

// ─── Virtual 3D Light Sources & Shading ───────────────────────────────────────

/// Type and parameters of a virtual 3D light source.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum LightSource {
    /// Ambient light: uniform flat illumination without directionality or shadows.
    Ambient {
        color: Color,
        intensity: f32,
    },
    /// Point light: omnidirectional spherical light with distance decay.
    Point {
        position: Vec3,
        color: Color,
        intensity: f32,
        /// Distance at which light intensity is 50% (inverse square falloff).
        radius: f32,
        /// Decay exponent (default 2.0 = physically accurate inverse square).
        decay: f32,
    },
    /// Spot light: conical directional spotlight with angular penumbra feathering.
    Spot {
        position: Vec3,
        target: Vec3,
        color: Color,
        intensity: f32,
        /// Total cone beam angle in degrees (e.g. 45°).
        cone_angle_deg: f32,
        /// Penumbra feather percentage (0.0 = hard edge, 1.0 = maximum soft edge).
        feather: f32,
        radius: f32,
    },
    /// Directional light: parallel rays without distance falloff (sunlight).
    Directional {
        direction: Vec3,
        color: Color,
        intensity: f32,
    },
}

impl LightSource {
    /// Ambient light constructor.
    pub fn ambient(color: Color, intensity: f32) -> Self {
        Self::Ambient { color, intensity }
    }

    /// Point light with inverse-square falloff.
    pub fn point(position: Vec3, color: Color, intensity: f32, radius: f32) -> Self {
        Self::Point {
            position,
            color,
            intensity,
            radius,
            decay: 2.0,
        }
    }

    /// Spot light constructor.
    pub fn spot(
        position: Vec3,
        target: Vec3,
        color: Color,
        intensity: f32,
        cone_angle_deg: f32,
        feather: f32,
    ) -> Self {
        Self::Spot {
            position,
            target,
            color,
            intensity,
            cone_angle_deg,
            feather: feather.clamp(0.0, 1.0),
            radius: 2000.0,
        }
    }

    /// Directional sunlight constructor.
    pub fn directional(direction: Vec3, color: Color, intensity: f32) -> Self {
        Self::Directional {
            direction: direction.normalize(),
            color,
            intensity,
        }
    }
}

/// Material properties for 3D surface shading.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Material3D {
    /// Ambient reflectance coefficient (0.0 to 1.0).
    pub ambient: f32,
    /// Diffuse reflectance coefficient (0.0 to 1.0).
    pub diffuse: f32,
    /// Specular highlight reflectance (0.0 to 1.0).
    pub specular: f32,
    /// Specular shininess exponent (higher = tighter, sharper specular glint).
    pub shininess: f32,
    /// Whether this surface receives illumination from virtual lights.
    pub accepts_lights: bool,
    /// Whether this surface casts planar shadows.
    pub casts_shadows: bool,
}

impl Default for Material3D {
    fn default() -> Self {
        Self {
            ambient: 0.2,
            diffuse: 0.8,
            specular: 0.3,
            shininess: 32.0,
            accepts_lights: true,
            casts_shadows: true,
        }
    }
}

/// Evaluates total Blinn-Phong illumination on a 3D surface point.
///
/// Returns illuminated RGBA color clamped to `0..=255`.
pub fn shade_surface(
    point: Vec3,
    normal: Vec3,
    base_color: Color,
    material: &Material3D,
    lights: &[LightSource],
    view_pos: Vec3,
) -> Color {
    if !material.accepts_lights || lights.is_empty() {
        return base_color;
    }

    let n = normal.normalize();
    let v = Vec3::new(view_pos.x - point.x, view_pos.y - point.y, view_pos.z - point.z).normalize();

    let mut r_acc = 0.0f32;
    let mut g_acc = 0.0f32;
    let mut b_acc = 0.0f32;

    for light in lights {
        match light {
            LightSource::Ambient { color, intensity } => {
                let factor = material.ambient * intensity;
                r_acc += (color.r as f32 / 255.0) * factor;
                g_acc += (color.g as f32 / 255.0) * factor;
                b_acc += (color.b as f32 / 255.0) * factor;
            }
            LightSource::Point {
                position,
                color,
                intensity,
                radius,
                decay,
            } => {
                let to_light = Vec3::new(position.x - point.x, position.y - point.y, position.z - point.z);
                let dist = to_light.length();
                let l = to_light.normalize();

                let n_dot_l = n.dot(l).max(0.0);
                if n_dot_l <= 0.0 {
                    continue;
                }

                let atten = 1.0 / (1.0 + (dist / radius.max(1.0)).powf(*decay));
                let h = Vec3::new(l.x + v.x, l.y + v.y, l.z + v.z).normalize();
                let n_dot_h = n.dot(h).max(0.0);
                let spec = n_dot_h.powf(material.shininess) * material.specular;

                let diff = n_dot_l * material.diffuse;
                let total_factor = (diff + spec) * intensity * atten;

                r_acc += (color.r as f32 / 255.0) * total_factor;
                g_acc += (color.g as f32 / 255.0) * total_factor;
                b_acc += (color.b as f32 / 255.0) * total_factor;
            }
            LightSource::Spot {
                position,
                target,
                color,
                intensity,
                cone_angle_deg,
                feather,
                radius,
            } => {
                let to_light = Vec3::new(position.x - point.x, position.y - point.y, position.z - point.z);
                let dist = to_light.length();
                let l = to_light.normalize();

                let n_dot_l = n.dot(l).max(0.0);
                if n_dot_l <= 0.0 {
                    continue;
                }

                // Check angle relative to spot forward direction
                let spot_dir = Vec3::new(target.x - position.x, target.y - position.y, target.z - position.z).normalize();
                let cos_theta = (-l).dot(spot_dir); // angle from spotlight axis

                let half_cone_rad = (cone_angle_deg * 0.5).to_radians();
                let outer_cos = half_cone_rad.cos();
                let inner_cos = (half_cone_rad * (1.0 - feather)).cos();

                if cos_theta < outer_cos {
                    continue; // Outside cone
                }

                let cone_atten = if outer_cos < inner_cos {
                    ((cos_theta - outer_cos) / (inner_cos - outer_cos)).clamp(0.0, 1.0)
                } else {
                    1.0
                };

                let dist_atten = 1.0 / (1.0 + (dist / radius.max(1.0)).powi(2));
                let atten = cone_atten * dist_atten;

                let h = Vec3::new(l.x + v.x, l.y + v.y, l.z + v.z).normalize();
                let spec = n.dot(h).max(0.0).powf(material.shininess) * material.specular;
                let diff = n_dot_l * material.diffuse;
                let total_factor = (diff + spec) * intensity * atten;

                r_acc += (color.r as f32 / 255.0) * total_factor;
                g_acc += (color.g as f32 / 255.0) * total_factor;
                b_acc += (color.b as f32 / 255.0) * total_factor;
            }
            LightSource::Directional {
                direction,
                color,
                intensity,
            } => {
                let l = Vec3::new(-direction.x, -direction.y, -direction.z).normalize();
                let n_dot_l = n.dot(l).max(0.0);
                if n_dot_l <= 0.0 {
                    continue;
                }
                let h = Vec3::new(l.x + v.x, l.y + v.y, l.z + v.z).normalize();
                let spec = n.dot(h).max(0.0).powf(material.shininess) * material.specular;
                let diff = n_dot_l * material.diffuse;
                let total_factor = (diff + spec) * intensity;

                r_acc += (color.r as f32 / 255.0) * total_factor;
                g_acc += (color.g as f32 / 255.0) * total_factor;
                b_acc += (color.b as f32 / 255.0) * total_factor;
            }
        }
    }

    let out_r = ((base_color.r as f32 / 255.0) * r_acc * 255.0).round().clamp(0.0, 255.0) as u8;
    let out_g = ((base_color.g as f32 / 255.0) * g_acc * 255.0).round().clamp(0.0, 255.0) as u8;
    let out_b = ((base_color.b as f32 / 255.0) * b_acc * 255.0).round().clamp(0.0, 255.0) as u8;

    Color::rgba(out_r, out_g, out_b, base_color.a)
}

/// Computes a 4x4 planar shadow projection matrix from a point light onto plane $ax + by + cz + d = 0$.
pub fn planar_shadow_matrix(light_pos: Vec3, plane_normal: Vec3, plane_d: f32) -> Mat4 {
    let n = plane_normal.normalize();
    let l = light_pos;
    let dot = n.x * l.x + n.y * l.y + n.z * l.z + plane_d;

    // Mat4 is column-major: [col0, col1, col2, col3]
    Mat4([
        dot - l.x * n.x,
        -l.y * n.x,
        -l.z * n.x,
        -n.x,
        -l.x * n.y,
        dot - l.y * n.y,
        -l.z * n.y,
        -n.y,
        -l.x * n.z,
        -l.y * n.z,
        dot - l.z * n.z,
        -n.z,
        -l.x * plane_d,
        -l.y * plane_d,
        -l.z * plane_d,
        dot - plane_d,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camera_fov_and_view_matrix() {
        let cam = Camera3D::preset_50mm();
        // 50mm on 36mm film gate has horizontal FOV approx 39.6 degrees (0.691 rad)
        let fov_x = cam.fov_x_rad().to_degrees();
        assert!((fov_x - 39.6).abs() < 1.0);

        let view = cam.view_matrix();
        // View matrix diagonal should be finite
        assert!(view.0.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn test_depth_of_field_circle_of_confusion() {
        let cam = Camera3D::preset_85mm();
        // At exact focus distance, CoC should be 0.0
        let coc_in_focus = cam.circle_of_confusion_px(cam.dof.focus_distance, 1080.0);
        assert!((coc_in_focus - 0.0).abs() < 1e-4);

        // Near foreground (closer than focus) has noticeable circle of confusion blur
        let coc_near = cam.circle_of_confusion_px(400.0, 1080.0);
        assert!(coc_near > 5.0, "foreground should have bokeh blur");

        // Far background (further than focus) has bokeh blur
        let coc_far = cam.circle_of_confusion_px(3000.0, 1080.0);
        assert!(coc_far > 5.0, "background should have bokeh blur");
    }

    #[test]
    fn test_point_and_spot_lighting() {
        let surface_pt = Vec3::new(0.0, 0.0, 0.0);
        let normal = Vec3::new(0.0, 1.0, 0.0); // Facing straight up
        let base_color = Color::rgb(200, 200, 200);
        let mat = Material3D::default();
        let view_pos = Vec3::new(0.0, 500.0, -500.0);

        // Light overhead
        let light = LightSource::point(Vec3::new(0.0, 100.0, 0.0), Color::WHITE, 1.0, 500.0);
        let shaded = shade_surface(surface_pt, normal, base_color, &mat, &[light], view_pos);

        assert!(shaded.r > 100);
        assert!(shaded.g > 100);
        assert!(shaded.b > 100);

        // Light facing backward (from underneath) produces no diffuse illumination
        let light_below = LightSource::point(Vec3::new(0.0, -100.0, 0.0), Color::WHITE, 1.0, 500.0);
        let shaded_below = shade_surface(surface_pt, normal, base_color, &mat, &[light_below], view_pos);
        assert_eq!(shaded_below.r, 0);
        assert_eq!(shaded_below.g, 0);
        assert_eq!(shaded_below.b, 0);
    }

    #[test]
    fn test_planar_shadow_matrix() {
        let light = Vec3::new(0.0, 500.0, 0.0);
        let plane_n = Vec3::new(0.0, 1.0, 0.0); // Ground plane Y = 0
        let shadow_mat = planar_shadow_matrix(light, plane_n, 0.0);

        // Transform a point above ground (0, 100, 0)
        let pt = Vec3::new(50.0, 100.0, 50.0);
        let proj = shadow_mat.transform_vec3(pt);
        // Shadow Y on plane should be 0.0
        assert!((proj.y).abs() < 1e-4);
    }
}
