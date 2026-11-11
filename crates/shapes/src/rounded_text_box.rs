//! Parametric rounded text box helpers.

use crate::shape_output::ShapeOutput;
pub use rust_motion_rasterizer::font::{
    create_rounded_text_box, create_rounded_text_box_from_measurements, RoundedTextBoxOptions,
    TextAlign, TextLineDimension,
};

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
