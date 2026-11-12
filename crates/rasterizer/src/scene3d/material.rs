//! 3D Materials: PBR Metallic-Roughness, MatCap, Unlit, and Wireframe.

use crate::camera3d::Material3D as AeMaterial3D;
use crate::scene::Color;
use serde::{Deserialize, Serialize};

/// Physically-Based Rendering (PBR) Metallic-Roughness material.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PbrMaterial {
    /// Base albedo color (RGBA). Default white.
    pub base_color: Color,
    /// Path or URI to base color diffuse texture map.
    pub base_color_texture: Option<String>,
    /// Metallic factor `[0.0, 1.0]` (0.0 = dielectric/plastic, 1.0 = raw metal).
    pub metallic: f32,
    /// Roughness factor `[0.0, 1.0]` (0.0 = mirror smooth, 1.0 = completely matte).
    pub roughness: f32,
    /// Path or URI to packed metallic-roughness texture map (Green=Roughness, Blue=Metallic).
    pub metallic_roughness_texture: Option<String>,
    /// Path or URI to tangent-space normal map (RGB normal vector).
    pub normal_texture: Option<String>,
    /// Emissive self-illumination color.
    pub emissive_color: Color,
    /// Emissive intensity multiplier.
    pub emissive_intensity: f32,
    /// Whether to render backfaces without culling.
    pub double_sided: bool,
    /// Alpha cutoff threshold for masked transparency (None = opaque or alpha blended).
    pub alpha_cutoff: Option<f32>,
}

impl Default for PbrMaterial {
    fn default() -> Self {
        Self {
            base_color: Color::WHITE,
            base_color_texture: None,
            metallic: 0.0,
            roughness: 0.5,
            metallic_roughness_texture: None,
            normal_texture: None,
            emissive_color: Color::BLACK,
            emissive_intensity: 0.0,
            double_sided: false,
            alpha_cutoff: None,
        }
    }
}

impl PbrMaterial {
    pub fn new(base_color: Color, metallic: f32, roughness: f32) -> Self {
        Self {
            base_color,
            metallic: metallic.clamp(0.0, 1.0),
            roughness: roughness.clamp(0.0, 1.0),
            ..Default::default()
        }
    }

    pub fn with_texture(mut self, path: impl Into<String>) -> Self {
        self.base_color_texture = Some(path.into());
        self
    }

    pub fn with_normal_map(mut self, path: impl Into<String>) -> Self {
        self.normal_texture = Some(path.into());
        self
    }

    pub fn with_emissive(mut self, color: Color, intensity: f32) -> Self {
        self.emissive_color = color;
        self.emissive_intensity = intensity.max(0.0);
        self
    }

    pub fn double_sided(mut self, enabled: bool) -> Self {
        self.double_sided = enabled;
        self
    }
}

/// Material Capture (MatCap / Spherical Environment Mapping) material.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatCapMaterial {
    /// Path or URI to circular MatCap reflection texture.
    pub texture: String,
    /// Color multiplier tint applied over the MatCap texture.
    pub tint: Color,
}

impl Default for MatCapMaterial {
    fn default() -> Self {
        Self {
            texture: String::new(),
            tint: Color::WHITE,
        }
    }
}

/// Simple flat unlit material (no shading/lighting).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnlitMaterial {
    pub color: Color,
    pub texture: Option<String>,
}

impl Default for UnlitMaterial {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            texture: None,
        }
    }
}

/// Wireframe rendering material.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WireframeMaterial {
    pub color: Color,
    pub line_width: f32,
}

impl Default for WireframeMaterial {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            line_width: 1.0,
        }
    }
}

/// Unified 3D surface material representation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Material3D {
    /// Physically-Based Rendering (PBR) Metallic-Roughness workflow.
    Pbr(PbrMaterial),
    /// MatCap reflection sphere.
    MatCap(MatCapMaterial),
    /// Unlit constant color / texture.
    Unlit(UnlitMaterial),
    /// Wireframe edge lines.
    Wireframe(WireframeMaterial),
    /// Classic Blinn-Phong specular/diffuse/ambient material.
    BlinnPhong(AeMaterial3D),
}

impl Default for Material3D {
    fn default() -> Self {
        Self::Pbr(PbrMaterial::default())
    }
}

impl Material3D {
    pub fn pbr(base_color: Color, metallic: f32, roughness: f32) -> Self {
        Self::Pbr(PbrMaterial::new(base_color, metallic, roughness))
    }

    pub fn unlit(color: Color) -> Self {
        Self::Unlit(UnlitMaterial {
            color,
            texture: None,
        })
    }

    pub fn wireframe(color: Color, line_width: f32) -> Self {
        Self::Wireframe(WireframeMaterial { color, line_width })
    }

    pub fn matcap(texture: impl Into<String>, tint: Color) -> Self {
        Self::MatCap(MatCapMaterial {
            texture: texture.into(),
            tint,
        })
    }

    pub fn base_color(&self) -> Color {
        match self {
            Self::Pbr(m) => m.base_color,
            Self::MatCap(m) => m.tint,
            Self::Unlit(m) => m.color,
            Self::Wireframe(m) => m.color,
            Self::BlinnPhong(_) => Color::WHITE,
        }
    }
}
