//! `<RoundedTextBox>` shape component and multi-corner parametric rounded text box helpers.

#[cfg(feature = "dioxus")]
use crate::render_svg::RenderSvg;
use crate::shape_output::ShapeOutput;
#[cfg(feature = "dioxus")]
use dioxus::prelude::*;
pub use rust_motion_rasterizer::font::{
    create_rounded_text_box, create_rounded_text_box_from_measurements, RoundedTextBoxOptions,
    TextAlign, TextLineDimension,
};

/// Props for the `<RoundedTextBox>` shape component.
#[cfg(feature = "dioxus")]
#[derive(Props, Clone, PartialEq)]
pub struct RoundedTextBoxProps {
    /// Line width and height measurements.
    #[props(default = Vec::new())]
    pub measurements: Vec<TextLineDimension>,
    /// Horizontal padding in pixels.
    #[props(default = 16.0)]
    pub padding_x: f32,
    /// Vertical padding in pixels.
    #[props(default = 12.0)]
    pub padding_y: f32,
    /// Maximum corner radius.
    #[props(default = 8.0)]
    pub border_radius: f32,
    /// Alignment of lines.
    #[props(default = TextAlign::Left)]
    pub align: TextAlign,
    /// Fill color.
    #[props(default = "#ffffff".to_string())]
    pub fill: String,
    /// Stroke color.
    #[props(default = "none".to_string())]
    pub stroke: String,
    /// Stroke width.
    #[props(default = 0.0)]
    pub stroke_width: f64,
    /// Opacity.
    #[props(default = 1.0)]
    pub opacity: f64,
    /// Custom CSS styles.
    #[props(default)]
    pub style: String,
}

/// Generates a `ShapeOutput` for a multi-corner rounded text box.
pub fn make_rounded_text_box(
    measurements: &[TextLineDimension],
    options: &RoundedTextBoxOptions,
) -> ShapeOutput {
    let path = create_rounded_text_box_from_measurements(measurements, options);
    let width = measurements
        .iter()
        .map(|m| (m.width + options.padding_x * 2.0) as f64)
        .fold(0.0_f64, f64::max);
    let height = measurements.iter().map(|m| m.height as f64).sum::<f64>();
    let transform_origin = format!("{} {}", width / 2.0, height / 2.0);

    ShapeOutput::new(path, width, height, transform_origin)
}

/// Renders a procedural SVG multi-corner rounded text box.
#[cfg(feature = "dioxus")]
#[component]
pub fn RoundedTextBox(props: RoundedTextBoxProps) -> Element {
    let options = RoundedTextBoxOptions {
        padding_x: props.padding_x,
        padding_y: props.padding_y,
        border_radius: props.border_radius,
        align: props.align,
    };
    let shape = make_rounded_text_box(&props.measurements, &options);

    rsx! {
        RenderSvg {
            path: shape.path,
            width: shape.width,
            height: shape.height,
            fill: props.fill,
            stroke: props.stroke,
            stroke_width: props.stroke_width,
            opacity: props.opacity,
            style: props.style,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_rounded_text_box_single_line() {
        let measurements = vec![TextLineDimension::new(200.0, 40.0)];
        let options = RoundedTextBoxOptions {
            padding_x: 10.0,
            padding_y: 5.0,
            border_radius: 8.0,
            align: TextAlign::Left,
        };
        let shape = make_rounded_text_box(&measurements, &options);
        assert_eq!(shape.width, 220.0);
        assert_eq!(shape.height, 40.0);
        assert!(shape.path.starts_with("M"));
        assert!(shape.path.contains("A 8 8"));
        assert!(shape.path.ends_with("Z"));
    }

    #[test]
    fn test_make_rounded_text_box_multi_line_stepped() {
        let measurements = vec![
            TextLineDimension::new(250.0, 30.0),
            TextLineDimension::new(150.0, 30.0),
        ];
        let options = RoundedTextBoxOptions {
            padding_x: 12.0,
            padding_y: 6.0,
            border_radius: 8.0,
            align: TextAlign::Left,
        };
        let shape = make_rounded_text_box(&measurements, &options);
        assert_eq!(shape.width, 274.0);
        assert_eq!(shape.height, 60.0);
        assert!(shape.path.starts_with("M"));
        assert!(shape.path.ends_with("Z"));
    }
}
