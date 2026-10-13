//! Rust Motion Shapes — procedural SVG motion graphics paths and optional Dioxus components.
//!
//! Pure path generators are always available. Enable the Dioxus feature for component wrappers.

pub mod arrow;
pub mod callout;
pub mod circle;
pub mod ellipse;
pub mod heart;
pub mod pie;
pub mod polygon;
pub mod rect;
#[cfg(feature = "dioxus")]
pub mod render_svg;
pub mod rounded_text_box;
pub mod scene;
pub mod shape_output;
pub mod spark;
pub mod star;
pub mod triangle;

pub use arrow::make_arrow;
#[cfg(feature = "dioxus")]
pub use arrow::{Arrow, ArrowProps};
pub use callout::{make_callout, CalloutDirection};
#[cfg(feature = "dioxus")]
pub use callout::{Callout, CalloutProps};
pub use circle::make_circle;
#[cfg(feature = "dioxus")]
pub use circle::{Circle, CircleProps};
pub use ellipse::make_ellipse;
#[cfg(feature = "dioxus")]
pub use ellipse::{Ellipse, EllipseProps};
pub use heart::make_heart;
#[cfg(feature = "dioxus")]
pub use heart::{Heart, HeartProps};
pub use pie::make_pie;
#[cfg(feature = "dioxus")]
pub use pie::{Pie, PieProps};
pub use polygon::make_polygon;
#[cfg(feature = "dioxus")]
pub use polygon::{Polygon, PolygonProps};
pub use rect::make_rect;
#[cfg(feature = "dioxus")]
pub use rect::{Rect, RectProps};
#[cfg(feature = "dioxus")]
pub use render_svg::{RenderSvg, RenderSvgProps};
pub use rounded_text_box::{
    create_rounded_text_box, create_rounded_text_box_from_measurements, make_rounded_text_box,
    RoundedTextBoxOptions, TextAlign, TextLineDimension,
};
#[cfg(feature = "dioxus")]
pub use rounded_text_box::{RoundedTextBox, RoundedTextBoxProps};
pub use scene::SceneShape;
pub use shape_output::ShapeOutput;
pub use spark::make_spark;
#[cfg(feature = "dioxus")]
pub use spark::{Spark, SparkProps};
pub use star::make_star;
#[cfg(feature = "dioxus")]
pub use star::{Star, StarProps};
pub use triangle::make_triangle;
#[cfg(feature = "dioxus")]
pub use triangle::{Triangle, TriangleProps};
