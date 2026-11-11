//! Rust Motion Shapes — procedural SVG motion graphics paths and scene nodes.
//!
//! Pure path generators and scene graph representations for geometric shapes.

pub mod arrow;
pub mod callout;
pub mod circle;
pub mod ellipse;
pub mod heart;
pub mod pie;
pub mod polygon;
pub mod rect;
pub mod rounded_text_box;
pub mod scene;
pub mod shape_output;
pub mod spark;
pub mod star;
pub mod triangle;

pub use arrow::make_arrow;
pub use callout::{make_callout, CalloutDirection};
pub use circle::make_circle;
pub use ellipse::make_ellipse;
pub use heart::make_heart;
pub use pie::make_pie;
pub use polygon::make_polygon;
pub use rect::make_rect;
pub use rounded_text_box::{
    create_rounded_text_box, create_rounded_text_box_from_measurements, make_rounded_text_box,
    RoundedTextBoxOptions, TextAlign, TextLineDimension,
};
pub use scene::SceneShape;
pub use shape_output::ShapeOutput;
pub use spark::make_spark;
pub use star::make_star;
pub use triangle::make_triangle;
