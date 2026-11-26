//! The Photocraft document model: pure data, no rendering, no UI.
//!
//! Layer order: inside every group (and the root), `children[0]` is the **bottom** layer, matching
//! compositing order and the PSD file order. UIs display the list reversed.
//!
//! Non-destructive kinds (adjustment, fill, text, shape, smart object) and colour modes beyond RGB are
//! part of the model from day one, even where rendering support lands later (architecture §1.1).
#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unimplemented,
        clippy::todo,
        clippy::unreachable
    )
)]

pub mod adjust;
pub mod advanced;
pub mod analysis;
pub mod blend_if;
pub mod comps;
pub mod effects;
pub mod mode;
pub mod pattern;
pub mod slices;
pub mod text;
pub mod text_styles;
pub mod variables;
pub mod vector;
pub mod video;

mod channels;
mod document;
mod fill;
mod guides;
mod ids;
mod label_color;
mod layer;
mod locks;
mod smart;
#[cfg(test)]
mod tests;
mod text_layer;

pub use adjust::Adjustment;
pub use advanced::{AdvancedBlending, Knockout};
pub use analysis::{CountGroup, Measurement, MeasurementScale, Note, Ruler};
pub use blend_if::{BlendIf, BlendRange};
pub use channels::*;
pub use comps::{Artboard, ArtboardBackground, CompAppearance, CompLayerState, LayerComp};
pub use document::*;
pub use effects::{
    Bevel, BevelContour, BevelStyle, BevelTechnique, BevelTexture, Contour, Effect, FxCommon,
    FxPaint, GlobalLight, Glow, GlowSource, GlowTechnique, Gradient, GradientStyle, Satin, Shadow,
    StrokeFx, StrokePosition,
};
pub use fill::*;
pub use guides::*;
pub use ids::*;
pub use label_color::*;
pub use layer::*;
pub use locks::*;
pub use mode::{ColorTable, Duotone, DuotoneInk, StackMode};
pub use pattern::Pattern;
pub use rust_motion_color::{BlendMode, Color, ColorMode, PixelFormat, SampleType};
pub use rust_motion_geom::{Affine, Rect, Size};
pub use rust_motion_raster::Surface;
pub use slices::{Slice, SliceKind, SliceOrigin, Slices};
pub use smart::*;
pub use text_layer::*;
pub use text_styles::TextStyles;
pub use variables::{DataSet, DataValue, PixelAlign, PixelMethod, VarKind, VariableDef, Variables};
pub use vector::{
    ClippingPath, FillRule, Knot, LineCap, LineJoin, LiveShape, NamedPath, Path, PathOp,
    ShapeLayer, ShapeStroke, StrokeAlign, Subpath, VectorMask,
};
pub use video::{Timeline, VideoData, VideoSource};
