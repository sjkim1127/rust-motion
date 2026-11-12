//! 3D Virtual Light sources for Scene3D.

use crate::camera3d::LightSource;
use crate::mesh3d::Vec3;
use crate::scene::Color;
use serde::{Deserialize, Serialize};

/// Classification of virtual light emitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Light3DType {
    /// Non-directional uniform ambient fill light.
    Ambient,
    /// Parallel distant rays with no spatial falloff (e.g. sunlight).
    Directional,
    /// Omnidirectional point emitter with distance decay.
    Point,
    /// Conical spotlight with inner hot-spot and outer penumbra feathering.
    Spot,
}

/// A 3D Light source in `Scene3D`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Scene3DLight {
    /// Light emitter type.
    pub light_type: Light3DType,
    /// Emitted light color.
    pub color: Color,
    /// Energy/intensity multiplier (default `1.0`).
    pub intensity: f32,
    /// World position for Point and Spot lights.
    pub position: Vec3,
    /// Propagation direction vector for Directional and Spot lights.
    pub direction: Vec3,
    /// Maximum effective illumination radius in world units.
    pub range: f32,
    /// Distance decay falloff exponent (default `2.0` = inverse-square law).
    pub decay: f32,
    /// Spotlight inner cone angle in degrees (full brightness).
    pub inner_cone_angle_deg: f32,
    /// Spotlight outer cone angle in degrees (feathered penumbra edge).
    pub outer_cone_angle_deg: f32,
    /// Whether this light participates in shadow mapping.
    pub casts_shadows: bool,
}

impl Default for Scene3DLight {
    fn default() -> Self {
        Self {
            light_type: Light3DType::Directional,
            color: Color::WHITE,
            intensity: 1.0,
            position: Vec3::new(0.0, 500.0, 500.0),
            direction: Vec3::new(0.0, -1.0, -1.0).normalize(),
            range: 2000.0,
            decay: 2.0,
            inner_cone_angle_deg: 30.0,
            outer_cone_angle_deg: 45.0,
            casts_shadows: false,
        }
    }
}

impl Scene3DLight {
    /// Constructs an ambient fill light.
    pub fn ambient(color: Color, intensity: f32) -> Self {
        Self {
            light_type: Light3DType::Ambient,
            color,
            intensity,
            ..Default::default()
        }
    }

    /// Constructs a directional sun light.
    pub fn directional(direction: Vec3, color: Color, intensity: f32) -> Self {
        Self {
            light_type: Light3DType::Directional,
            direction: direction.normalize(),
            color,
            intensity,
            ..Default::default()
        }
    }

    /// Constructs an omnidirectional point light.
    pub fn point(position: Vec3, color: Color, intensity: f32, range: f32) -> Self {
        Self {
            light_type: Light3DType::Point,
            position,
            color,
            intensity,
            range,
            ..Default::default()
        }
    }

    /// Constructs a conical spot light.
    pub fn spot(
        position: Vec3,
        direction: Vec3,
        color: Color,
        intensity: f32,
        range: f32,
        inner_deg: f32,
        outer_deg: f32,
    ) -> Self {
        Self {
            light_type: Light3DType::Spot,
            position,
            direction: direction.normalize(),
            color,
            intensity,
            range,
            inner_cone_angle_deg: inner_deg,
            outer_cone_angle_deg: outer_deg,
            ..Default::default()
        }
    }

    /// Converts this light into a `LightSource` for Blinn-Phong evaluation.
    pub fn to_light_source(&self) -> LightSource {
        match self.light_type {
            Light3DType::Ambient => LightSource::Ambient {
                color: self.color,
                intensity: self.intensity,
            },
            Light3DType::Directional => LightSource::Directional {
                direction: self.direction,
                color: self.color,
                intensity: self.intensity,
            },
            Light3DType::Point => LightSource::Point {
                position: self.position,
                color: self.color,
                intensity: self.intensity,
                radius: self.range,
                decay: self.decay,
            },
            Light3DType::Spot => LightSource::Spot {
                position: self.position,
                target: Vec3::new(
                    self.position.x + self.direction.x * self.range,
                    self.position.y + self.direction.y * self.range,
                    self.position.z + self.direction.z * self.range,
                ),
                color: self.color,
                intensity: self.intensity,
                radius: self.range,
                cone_angle_deg: self.outer_cone_angle_deg,
                feather: (self.outer_cone_angle_deg - self.inner_cone_angle_deg).max(0.0),
            },
        }
    }
}
