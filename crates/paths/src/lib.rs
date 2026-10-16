//! Rust Motion Paths — SVG path parsing, metrics, and stroke evolution utilities.
//!
//! Ported from `@remotion/paths`:
//! - [`parse_path`] / [`serialize_instructions`]
//! - [`get_length`] / [`get_instructions_length`]
//! - [`evolve_path`] / [`evolve_path_with_length`]
//! - [`translate_path`] / [`scale_path`]
//! - [`get_point_at_length`]
//! - [`approximate_path_length`]
//! - [`interpolate_path`]

pub mod bounding_box;
pub mod evolve_path;
pub mod interpolate;
pub mod length;
pub mod parser;
pub mod point_at_length;
pub mod shape_modifiers;
pub mod subpaths;
pub mod tangent;
pub mod transform;
pub mod types;

pub use bounding_box::{get_bounding_box, get_instructions_bounding_box};
pub use evolve_path::{evolve_path, evolve_path_with_length};
pub use interpolate::{approximate_path_length, interpolate_path};
pub use length::{arc_segment_length, get_instructions_length, get_length};
pub use parser::{parse_path, serialize_instructions, PathParseError};
pub use point_at_length::get_point_at_length;
pub use shape_modifiers::{
    repeat_paths, round_corners, trim_path_str, trim_paths, wiggle_paths, RepeaterConfig,
    RepeaterInstance, TrimMode, WigglePointType,
};
pub use subpaths::get_subpaths;
pub use tangent::{get_tangent_angle_at_length, get_tangent_at_length};
pub use transform::{reset_path, reverse_path, rotate_path, scale_path, translate_path, warp_path};
pub use types::{BoundingBox, EvolvedPath, Instruction, Point};
