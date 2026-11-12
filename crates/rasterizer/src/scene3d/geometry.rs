//! 3D Procedural and imported Geometry.

use crate::mesh3d::{Mesh3D, Vec3};
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;
use std::sync::Arc;

/// 3D Geometry representations supported by Scene3D.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Geometry3D {
    /// Subdivided horizontal planar quad.
    Plane {
        width: f32,
        height: f32,
        subdivisions: u32,
    },
    /// Isometric cube / rectangular cuboid.
    Cube { size: f32 },
    /// UV Sphere.
    Sphere {
        radius: f32,
        rings: usize,
        sectors: usize,
    },
    /// Cylinder / Cone.
    Cylinder {
        radius: f32,
        height: f32,
        segments: usize,
    },
    /// Torus (donut).
    Torus {
        r_major: f32,
        r_minor: f32,
        segs_major: usize,
        segs_minor: usize,
    },
    /// Extruded 2D SVG vector path (After Effects 3D text / shape extrusion).
    ExtrudedPath {
        svg_path: String,
        depth: f32,
        bevel_radius: f32,
        bevel_segments: usize,
    },
    /// Custom indexed polygon mesh.
    Mesh(Arc<Mesh3D>),
    /// Loaded glTF 2.0 PBR 3D asset reference.
    GltfAsset {
        path: String,
        #[serde(default)]
        scene_index: usize,
        #[serde(default)]
        animation_time: Option<f32>,
    },
}

impl PartialEq for Geometry3D {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Plane {
                    width: w1,
                    height: h1,
                    subdivisions: s1,
                },
                Self::Plane {
                    width: w2,
                    height: h2,
                    subdivisions: s2,
                },
            ) => w1 == w2 && h1 == h2 && s1 == s2,
            (Self::Cube { size: s1 }, Self::Cube { size: s2 }) => s1 == s2,
            (
                Self::Sphere {
                    radius: r1,
                    rings: rg1,
                    sectors: sc1,
                },
                Self::Sphere {
                    radius: r2,
                    rings: rg2,
                    sectors: sc2,
                },
            ) => r1 == r2 && rg1 == rg2 && sc1 == sc2,
            (
                Self::Cylinder {
                    radius: r1,
                    height: h1,
                    segments: s1,
                },
                Self::Cylinder {
                    radius: r2,
                    height: h2,
                    segments: s2,
                },
            ) => r1 == r2 && h1 == h2 && s1 == s2,
            (
                Self::Torus {
                    r_major: r1,
                    r_minor: m1,
                    segs_major: sm1,
                    segs_minor: sn1,
                },
                Self::Torus {
                    r_major: r2,
                    r_minor: m2,
                    segs_major: sm2,
                    segs_minor: sn2,
                },
            ) => r1 == r2 && m1 == m2 && sm1 == sm2 && sn1 == sn2,
            (
                Self::ExtrudedPath {
                    svg_path: p1,
                    depth: d1,
                    bevel_radius: b1,
                    bevel_segments: s1,
                },
                Self::ExtrudedPath {
                    svg_path: p2,
                    depth: d2,
                    bevel_radius: b2,
                    bevel_segments: s2,
                },
            ) => p1 == p2 && d1 == d2 && b1 == b2 && s1 == s2,
            (Self::Mesh(m1), Self::Mesh(m2)) => Arc::ptr_eq(m1, m2) || m1.as_ref() == m2.as_ref(),
            (
                Self::GltfAsset {
                    path: p1,
                    scene_index: s1,
                    animation_time: a1,
                },
                Self::GltfAsset {
                    path: p2,
                    scene_index: s2,
                    animation_time: a2,
                },
            ) => p1 == p2 && s1 == s2 && a1 == a2,
            _ => false,
        }
    }
}

impl Geometry3D {
    /// Generates a triangulated/polygonal [`Mesh3D`] representation for rendering.
    pub fn to_mesh(&self) -> Mesh3D {
        match self {
            Self::Cube { size } => Mesh3D::cube(*size),
            Self::Sphere {
                radius,
                rings,
                sectors,
            } => Mesh3D::sphere(*radius, *rings, *sectors),
            Self::Torus {
                r_major,
                r_minor,
                segs_major,
                segs_minor,
            } => Mesh3D::torus(*r_major, *r_minor, *segs_major, *segs_minor),
            Self::Plane {
                width,
                height,
                subdivisions,
            } => generate_plane_mesh(*width, *height, *subdivisions),
            Self::Cylinder {
                radius,
                height,
                segments,
            } => generate_cylinder_mesh(*radius, *height, *segments),
            Self::Mesh(mesh) => (**mesh).clone(),
            Self::ExtrudedPath { .. } => {
                // In Phase 1/3: basic fallback cuboid matching depth or empty mesh
                Mesh3D::cube(100.0)
            }
            Self::GltfAsset { .. } => Mesh3D::cube(100.0),
        }
    }
}

fn generate_plane_mesh(width: f32, height: f32, subdivisions: u32) -> Mesh3D {
    let subs = subdivisions.max(1);
    let mut vertices = Vec::new();
    let mut faces = Vec::new();

    let hw = width * 0.5;
    let hh = height * 0.5;

    for y in 0..=subs {
        let v = y as f32 / subs as f32;
        let pz = -hh + v * height;
        for x in 0..=subs {
            let u = x as f32 / subs as f32;
            let px = -hw + u * width;
            vertices.push(Vec3::new(px, 0.0, pz));
        }
    }

    let stride = subs + 1;
    for y in 0..subs {
        for x in 0..subs {
            let i0 = (y * stride + x) as usize;
            let i1 = ((y + 1) * stride + x) as usize;
            let i2 = ((y + 1) * stride + (x + 1)) as usize;
            let i3 = (y * stride + (x + 1)) as usize;
            faces.push(vec![i0, i1, i2, i3]);
        }
    }

    Mesh3D { vertices, faces }
}

fn generate_cylinder_mesh(radius: f32, height: f32, segments: usize) -> Mesh3D {
    let segs = segments.max(3);
    let mut vertices = Vec::new();
    let mut faces = Vec::new();

    let half_h = height * 0.5;
    let step = (2.0 * PI) / segs as f32;

    // Center vertices for top and bottom end caps
    let top_center_idx = 0;
    vertices.push(Vec3::new(0.0, half_h, 0.0));
    let bottom_center_idx = 1;
    vertices.push(Vec3::new(0.0, -half_h, 0.0));

    // Ring vertices: top ring, then bottom ring
    let top_ring_start = 2;
    for i in 0..segs {
        let theta = i as f32 * step;
        let x = radius * theta.cos();
        let z = radius * theta.sin();
        vertices.push(Vec3::new(x, half_h, z));
    }

    let bottom_ring_start = top_ring_start + segs;
    for i in 0..segs {
        let theta = i as f32 * step;
        let x = radius * theta.cos();
        let z = radius * theta.sin();
        vertices.push(Vec3::new(x, -half_h, z));
    }

    // End caps & side walls
    for i in 0..segs {
        let next = (i + 1) % segs;

        // Top cap (counter-clockwise looking down from +Y)
        faces.push(vec![
            top_center_idx,
            top_ring_start + i,
            top_ring_start + next,
        ]);

        // Bottom cap (counter-clockwise looking up from -Y)
        faces.push(vec![
            bottom_center_idx,
            bottom_ring_start + next,
            bottom_ring_start + i,
        ]);

        // Side quad
        faces.push(vec![
            top_ring_start + i,
            bottom_ring_start + i,
            bottom_ring_start + next,
            top_ring_start + next,
        ]);
    }

    Mesh3D { vertices, faces }
}
