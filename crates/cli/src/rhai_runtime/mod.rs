//! Sandboxed Rhai composition runtime.
//!
//! JSON remains the external props format. A Rhai script receives those props
//! plus an immutable frame context and returns a restricted [`SceneBuilder`].

use crate::composition::{
    layout::prelude::*, Composition, CompositionError, LayoutBox, NativeCompositionContext,
    PreparedComposition,
};
use base64::Engine as _;
use rhai::module_resolvers::DummyModuleResolver;
use rhai::{
    Dynamic, Engine, EvalAltResult, ImmutableString, Map, Position, Scope, AST, FLOAT, INT,
};
use rust_motion_rasterizer::{
    layout_text_box, parse_glb, parse_gltf, AudioTrack, Color, ConfettiEmitter, ConfettiShape,
    GradientStop, ImageFit, Mesh3D, Scene, SceneNode, TextBox, TextHorizontalAlign, TextOverflow,
    Transform2D, Vec3, VisualizerStyle,
};
use serde_json::Value;
use std::fs;
use std::path::Path;

const MAX_OPERATIONS_PER_FRAME: u64 = 100_000;
const MAX_STRING_SIZE: usize = 1_048_576;
const MAX_ARRAY_SIZE: usize = 4_096;
const MAX_MAP_SIZE: usize = 1_024;

type RhaiResult<T> = Result<T, Box<EvalAltResult>>;

mod builder;
mod composition;
mod diagrams;
mod effects;
mod engine;
mod layout;
mod media;
mod options;
mod shapes;
#[cfg(test)]
mod tests;
mod text;
mod three_d;
mod values;

pub use builder::*;
pub use composition::*;
use engine::*;
pub use layout::*;
use options::*;
use values::*;
