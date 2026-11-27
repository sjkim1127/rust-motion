//! Scene-to-GPU compilation: lowers scene nodes into draw commands, masks and path meshes.

use super::types::*;
use crate::scene::{Color, GradientStop, Scene, SceneNode};
use crate::tiny_skia_backend::{svgpath_to_tiny_skia, TinySkiaBackend};
use lyon_tessellation::geometry_builder::{BuffersBuilder, VertexBuffers};
use lyon_tessellation::math::point;
use lyon_tessellation::path::Path as LyonPath;
use lyon_tessellation::{FillOptions, FillTessellator};
use tiny_skia::{Path as TinyPath, PathSegment, Stroke, Transform};

mod effects;
mod masks;
mod nodes;
mod path_mask;
mod support;

pub(crate) use effects::*;
use masks::*;
pub(crate) use nodes::*;
pub(crate) use path_mask::gpu_path_mask_from_svg;
use path_mask::*;
pub(crate) use support::*;
