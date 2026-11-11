//! Ellipse shape path generator.

use crate::shape_output::ShapeOutput;

/// Generates SVG path and bounding dimensions for an ellipse of radii `rx` and `ry`.
pub fn make_ellipse(rx: f64, ry: f64) -> ShapeOutput {
    let rx = rx.max(0.0);
    let ry = ry.max(0.0);
    let width = rx * 2.0;
    let height = ry * 2.0;
    let path = format!("M {rx} 0 A {rx} {ry} 0 1 0 {rx} {height} A {rx} {ry} 0 1 0 {rx} 0 Z");
    ShapeOutput::new(path, width, height, format!("{rx} {ry}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_ellipse_dimensions() {
        let shape = make_ellipse(80.0, 40.0);
        assert_eq!(shape.width, 160.0);
        assert_eq!(shape.height, 80.0);
        assert!(shape.path.contains("M 80 0"));
        assert!(shape.path.contains("A 80 40"));
        assert_eq!(shape.transform_origin, "80 40");
    }
}
