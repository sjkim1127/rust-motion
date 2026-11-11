//! Heart shape path generator.

use crate::shape_output::ShapeOutput;

/// Generates parametric SVG path data and metadata for a heart shape.
pub fn make_heart(width: f64, height: f64) -> ShapeOutput {
    let w = width.max(0.0);
    let h = height.max(0.0);

    if w == 0.0 || h == 0.0 {
        return ShapeOutput::new(String::new(), w, h, format!("{} {}", w / 2.0, h / 2.0));
    }

    let bottom_cp_x = (23.0 / 110.0) * w;
    let bottom_cp_y = (69.0 / 100.0) * h;
    let bottom_left_cp_y = (60.0 / 100.0) * h;
    let top_left_cp = (13.0 / 100.0) * h;
    let top_bezier_w = (29.0 / 110.0) * w;
    let top_right_cp_x = (15.0 / 110.0) * w;
    let inner_cp_x = (5.0 / 110.0) * w;
    let inner_cp_y = (7.0 / 100.0) * h;
    let depth = (17.0 / 100.0) * h;

    let half_w = w / 2.0;
    let quarter_w = w / 4.0;
    let three_quarter_w = 3.0 * w / 4.0;
    let quarter_h = h / 4.0;

    let path = format!(
        "M {half_w} {h} \
         C {} {bottom_cp_y} 0 {bottom_left_cp_y} 0 {quarter_h} \
         C 0 {top_left_cp} {} 0 {quarter_w} 0 \
         C {} 0 {} {inner_cp_y} {half_w} {depth} \
         C {} {inner_cp_y} {} 0 {three_quarter_w} 0 \
         C {} 0 {w} {top_left_cp} {w} {quarter_h} \
         C {w} {bottom_left_cp_y} {} {bottom_cp_y} {half_w} {h} Z",
        half_w - bottom_cp_x,
        quarter_w - top_bezier_w / 2.0,
        quarter_w + top_bezier_w / 2.0,
        half_w - inner_cp_x,
        half_w + inner_cp_x,
        half_w + top_right_cp_x,
        three_quarter_w + top_bezier_w / 2.0,
        half_w + bottom_cp_x,
    );

    ShapeOutput::new(path, w, h, format!("{} {}", half_w, h / 2.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_heart_geometry() {
        let heart = make_heart(110.0, 100.0);
        assert_eq!(heart.width, 110.0);
        assert_eq!(heart.height, 100.0);
        assert_eq!(heart.transform_origin, "55 50");
        assert!(heart.path.starts_with("M 55 100"));
        assert!(heart.path.ends_with("Z"));
        assert!(heart.path.contains("C 32 69 0 60 0 25"));
    }

    #[test]
    fn test_make_heart_zero_dimensions() {
        let heart = make_heart(0.0, 0.0);
        assert_eq!(heart.path, "");
        assert_eq!(heart.width, 0.0);
        assert_eq!(heart.height, 0.0);
    }
}
