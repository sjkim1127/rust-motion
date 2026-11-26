//! Backend-independent internal Render IR types.
//!
//! This module defines logical resources, formats, color metadata, pass
//! operations, and capability flags without depending on any graphics backend
//! (`wgpu`, Metal, Vulkan) or leaking into the public 1.x `Scene` API.

use crate::scene::{BlendMode, Color, Scene, Transform2D};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Typed handle to a logical surface/buffer resource within a [`RenderGraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct ResourceId(pub(crate) u32);

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "R{}", self.0)
    }
}

/// Typed handle to a pass node within a [`RenderGraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct PassId(pub(crate) u32);

impl fmt::Display for PassId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P{}", self.0)
    }
}

/// Pixel storage format for a logical render resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum PixelFormat {
    Rgba8Unorm,
    Rgba8UnormSrgb,
    Rgba16Float,
    R8Unorm,
}

impl PixelFormat {
    pub(crate) const fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Rgba8Unorm | Self::Rgba8UnormSrgb => 4,
            Self::Rgba16Float => 8,
            Self::R8Unorm => 1,
        }
    }

    pub(crate) const fn is_color(self) -> bool {
        matches!(
            self,
            Self::Rgba8Unorm | Self::Rgba8UnormSrgb | Self::Rgba16Float
        )
    }

    pub(crate) const fn is_mask(self) -> bool {
        matches!(self, Self::R8Unorm)
    }
}

/// Color space semantics carried by a render resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum ColorSpace {
    Srgb,
    LinearSrgb,
    DisplayP3,
    Rec2020Pq,
}

/// Alpha representation carried by a render resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum AlphaMode {
    Straight,
    Premultiplied,
    Opaque,
}

/// Combined color and alpha metadata for a logical resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct ColorMetadata {
    pub(crate) space: ColorSpace,
    pub(crate) alpha: AlphaMode,
}

impl ColorMetadata {
    pub(crate) const fn srgb_premultiplied() -> Self {
        Self {
            space: ColorSpace::Srgb,
            alpha: AlphaMode::Premultiplied,
        }
    }

    pub(crate) const fn mask_linear() -> Self {
        Self {
            space: ColorSpace::LinearSrgb,
            alpha: AlphaMode::Straight,
        }
    }
}

/// Bitflag set describing how a resource is used across passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct TextureUsage(u8);

impl TextureUsage {
    pub(crate) const EMPTY: Self = Self(0);
    pub(crate) const RENDER_TARGET: Self = Self(1 << 0);
    pub(crate) const SAMPLED: Self = Self(1 << 1);
    pub(crate) const STORAGE: Self = Self(1 << 2);
    pub(crate) const COPY_SRC: Self = Self(1 << 3);
    pub(crate) const COPY_DST: Self = Self(1 << 4);
    pub(crate) const CPU_READBACK: Self = Self(1 << 5);

    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub(crate) fn names(self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.contains(Self::RENDER_TARGET) {
            out.push("render_target");
        }
        if self.contains(Self::SAMPLED) {
            out.push("sampled");
        }
        if self.contains(Self::STORAGE) {
            out.push("storage");
        }
        if self.contains(Self::COPY_SRC) {
            out.push("copy_src");
        }
        if self.contains(Self::COPY_DST) {
            out.push("copy_dst");
        }
        if self.contains(Self::CPU_READBACK) {
            out.push("cpu_readback");
        }
        if out.is_empty() {
            out.push("none");
        }
        out
    }
}

/// Ownership and lifetime class of a logical resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum ResourceLifetime {
    /// Pre-populated input supplied outside the graph (e.g. imported texture).
    External,
    /// Intermediate graph resource eligible for pool reuse after its last read.
    Transient,
    /// Final frame output surface produced by the graph.
    Output,
}

/// Logical resource description in the Render IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ResourceDesc {
    pub(crate) id: ResourceId,
    pub(crate) label: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) format: PixelFormat,
    pub(crate) color: ColorMetadata,
    pub(crate) sample_count: u32,
    pub(crate) usage: TextureUsage,
    pub(crate) lifetime: ResourceLifetime,
}

impl ResourceDesc {
    /// Checked memory footprint in bytes (`width * height * bpp * sample_count`).
    pub(crate) fn checked_byte_size(&self) -> Option<u64> {
        u64::from(self.width)
            .checked_mul(u64::from(self.height))?
            .checked_mul(u64::from(self.format.bytes_per_pixel()))?
            .checked_mul(u64::from(self.sample_count))
    }
}

/// Target initialization behavior at the start of a raster/shader pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ClearOp {
    Load,
    Clear(Color),
    DontCare,
}

/// Rectangular region in canvas pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct RectBounds {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

impl RectBounds {
    pub(crate) fn is_valid(&self) -> bool {
        [self.x, self.y, self.w, self.h]
            .iter()
            .all(|v| v.is_finite())
            && self.w > 0.0
            && self.h > 0.0
    }
}

/// Backend-independent capability requirements for pass lowering and validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub(crate) struct CapabilityFlags(u32);

impl CapabilityFlags {
    pub(crate) const NONE: Self = Self(0);
    pub(crate) const RASTER_2D_ANALYTIC: Self = Self(1 << 0);
    pub(crate) const RASTER_2D_MESH: Self = Self(1 << 1);
    pub(crate) const RASTER_2D_IMAGE: Self = Self(1 << 2);
    pub(crate) const RASTER_2D_TEXT: Self = Self(1 << 3);
    pub(crate) const BLEND_BASIC: Self = Self(1 << 4);
    pub(crate) const BLEND_EXTENDED: Self = Self(1 << 5);
    pub(crate) const OFFSCREEN_COMPOSITE: Self = Self(1 << 6);
    pub(crate) const PATH_MASK: Self = Self(1 << 7);
    pub(crate) const SHADER_PASS: Self = Self(1 << 8);
    pub(crate) const COMPUTE_PASS: Self = Self(1 << 9);
    pub(crate) const COPY_OP: Self = Self(1 << 10);
    pub(crate) const CPU_ISLAND: Self = Self(1 << 11);
    pub(crate) const TIMESTAMP_QUERY: Self = Self(1 << 12);
    pub(crate) const RASTER_EXTENDED: Self = Self(1 << 13);

    pub(crate) const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub(crate) const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) const fn missing(self, available: Self) -> Self {
        Self(self.0 & !available.0)
    }

    pub(crate) fn names(self) -> Vec<&'static str> {
        const ALL: &[(CapabilityFlags, &str)] = &[
            (CapabilityFlags::RASTER_2D_ANALYTIC, "raster_2d_analytic"),
            (CapabilityFlags::RASTER_2D_MESH, "raster_2d_mesh"),
            (CapabilityFlags::RASTER_2D_IMAGE, "raster_2d_image"),
            (CapabilityFlags::RASTER_2D_TEXT, "raster_2d_text"),
            (CapabilityFlags::BLEND_BASIC, "blend_basic"),
            (CapabilityFlags::BLEND_EXTENDED, "blend_extended"),
            (CapabilityFlags::OFFSCREEN_COMPOSITE, "offscreen_composite"),
            (CapabilityFlags::PATH_MASK, "path_mask"),
            (CapabilityFlags::SHADER_PASS, "shader_pass"),
            (CapabilityFlags::COMPUTE_PASS, "compute_pass"),
            (CapabilityFlags::COPY_OP, "copy_op"),
            (CapabilityFlags::CPU_ISLAND, "cpu_island"),
            (CapabilityFlags::TIMESTAMP_QUERY, "timestamp_query"),
            (CapabilityFlags::RASTER_EXTENDED, "raster_extended"),
        ];
        ALL.iter()
            .filter_map(|(flag, name)| self.contains(*flag).then_some(*name))
            .collect()
    }
}

/// Operation performed by a single render pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum PassOp {
    /// Rasterize a scene fragment into a color target, optionally clipped by an R8 path mask.
    Raster2D {
        scene: Scene,
        clear: ClearOp,
        path_mask: Option<ResourceId>,
    },
    /// Rasterize an SVG path into an `R8Unorm` mask surface.
    PathMask {
        path_d: String,
        transform: Transform2D,
    },
    /// Composite a resolved source surface onto a target surface.
    Composite {
        source: ResourceId,
        opacity: f32,
        blend_mode: BlendMode,
        region: Option<RectBounds>,
    },
    /// Dispatch a compute kernel over logical resources.
    Compute {
        kernel: String,
        workgroups: [u32; 3],
        params: Vec<f32>,
    },
    /// Copy pixel contents from a source surface into a destination surface.
    Copy {
        source: ResourceId,
        dst_origin: [u32; 2],
    },
    /// Execute a custom WGSL/procedural shader region onto a color target.
    Shader {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        source: String,
        time: f32,
        params: [f32; 4],
        opacity: f32,
        clear: ClearOp,
    },
    /// Bounded CPU reference island for operations unsupported on the active GPU executor.
    CpuIsland {
        scene: Scene,
        bounds: Option<RectBounds>,
        reason: String,
    },
}

impl PassOp {
    pub(crate) const fn kind_name(&self) -> &'static str {
        match self {
            Self::Raster2D { .. } => "raster_2d",
            Self::PathMask { .. } => "path_mask",
            Self::Composite { .. } => "composite",
            Self::Compute { .. } => "compute",
            Self::Copy { .. } => "copy",
            Self::Shader { .. } => "shader",
            Self::CpuIsland { .. } => "cpu_island",
        }
    }
}

/// A pass node in a [`RenderGraph`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PassNode {
    pub(crate) id: PassId,
    pub(crate) label: String,
    pub(crate) op: PassOp,
    pub(crate) reads: Vec<ResourceId>,
    pub(crate) writes: Vec<ResourceId>,
    pub(crate) order_deps: Vec<PassId>,
    pub(crate) required_capabilities: CapabilityFlags,
}

/// Declarative, backend-independent render graph for a single frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RenderGraph {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) resources: Vec<ResourceDesc>,
    pub(crate) passes: Vec<PassNode>,
    pub(crate) output: Option<ResourceId>,
}

impl RenderGraph {
    pub(crate) fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            resources: Vec::new(),
            passes: Vec::new(),
            output: None,
        }
    }

    pub(crate) fn add_resource(&mut self, desc: ResourceDesc) -> ResourceId {
        let id = desc.id;
        self.resources.push(desc);
        id
    }

    pub(crate) fn add_pass(&mut self, pass: PassNode) -> PassId {
        let id = pass.id;
        self.passes.push(pass);
        id
    }

    pub(crate) fn set_output(&mut self, id: ResourceId) {
        self.output = Some(id);
    }

    pub(crate) fn resource(&self, id: ResourceId) -> Option<&ResourceDesc> {
        self.resources.iter().find(|r| r.id == id)
    }

    pub(crate) fn pass(&self, id: PassId) -> Option<&PassNode> {
        self.passes.iter().find(|p| p.id == id)
    }
}
