//! 4-point star/spark shape path generator.

use crate::shape_output::ShapeOutput;

const KAPPA: f64 = 0.5522847498307936;

/// Generates parametric SVG path data and metadata for a 4-point spark shape.
pub fn make_spark(width: f64, height: f64, edge_roundness: f64, corner_radius: f64) -> ShapeOutput {
    let w = width.max(0.0);
    let h = height.max(0.0);

    if w == 0.0 || h == 0.0 {
        return ShapeOutput::new(String::new(), w, h, format!("{} {}", w / 2.0, h / 2.0));
    }

    let cx = w / 2.0;
    let cy = h / 2.0;
    let hx = w / 2.0;
    let hy = h / 2.0;
    let r = corner_radius.clamp(0.0, (hx / 2.0).min(hy / 2.0));
    let k = KAPPA * edge_roundness.clamp(0.0, 1.0);

    let path = if r <= 0.0 {
        format!(
            "M {cx} 0 \
             C {cx} {} {} {cy} {w} {cy} \
             C {} {cy} {cx} {} {cx} {h} \
             C {cx} {} {} {cy} 0 {cy} \
             C {} {cy} {cx} {} {cx} 0 Z",
            hy * k,
            w - hx * k,
            w - hx * k,
            h - hy * k,
            h - hy * k,
            hx * k,
            hx * k,
            hy * k,
        )
    } else {
        let cap_k = KAPPA * r;
        let dx = hx - 2.0 * r;
        let dy = hy - 2.0 * r;

        format!(
            "M {} {r} \
             C {} {} {} 0 {cx} 0 \
             C {} 0 {} {} {} {r} \
             C {} {} {} {} {} {} \
             C {} {} {w} {} {w} {cy} \
             C {w} {} {} {} {} {} \
             C {} {} {} {} {} {} \
             C {} {} {} {h} {cx} {h} \
             C {} {h} {} {} {} {} \
             C {} {} {} {} {r} {} \
             C {} {} 0 {} 0 {cy} \
             C 0 {} {} {} {r} {} \
             C {} {} {} {} {} {r} Z",
            cx - r,
            cx - r,
            r - cap_k,
            cx - cap_k,
            cx + cap_k,
            cx + r,
            r - cap_k,
            cx + r,
            cx + r,
            r + dy * k,
            w - r - dx * k,
            cy - r,
            w - r,
            cy - r,
            w - r + cap_k,
            cy - r,
            cy - cap_k,
            cy + cap_k,
            w - r + cap_k,
            cy + r,
            w - r,
            cy + r,
            w - r - dx * k,
            cy + r,
            cx + r,
            h - r - dy * k,
            cx + r,
            h - r,
            cx + r,
            h - r + cap_k,
            cx + cap_k,
            cx - cap_k,
            cx - r,
            h - r + cap_k,
            cx - r,
            h - r,
            cx - r,
            h - r - dy * k,
            r + dx * k,
            cy + r,
            cy + r,
            r - cap_k,
            cy + r,
            cy + cap_k,
            cy - cap_k,
            r - cap_k,
            cy - r,
            cy - r,
            r + dx * k,
            cy - r,
            cx - r,
            r + dy * k,
            cx - r,
        )
    };

    ShapeOutput::new(path, w, h, format!("{cx} {cy}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_spark_sharp() {
        let spark = make_spark(100.0, 100.0, 0.5, 0.0);
        assert_eq!(spark.width, 100.0);
        assert_eq!(spark.height, 100.0);
        assert_eq!(spark.transform_origin, "50 50");
        assert!(spark.path.starts_with("M 50 0"));
        assert!(spark.path.ends_with("Z"));
    }

    #[test]
    fn test_make_spark_rounded() {
        let spark = make_spark(100.0, 100.0, 0.5, 5.0);
        assert_eq!(spark.width, 100.0);
        assert_eq!(spark.height, 100.0);
        assert_eq!(spark.transform_origin, "50 50");
        assert!(spark.path.starts_with("M 45 5"));
        assert!(spark.path.ends_with("Z"));
    }

    #[test]
    fn test_make_spark_zero_dimension() {
        let spark = make_spark(0.0, 100.0, 0.5, 0.0);
        assert_eq!(spark.path, "");
        assert_eq!(spark.width, 0.0);
        assert_eq!(spark.height, 100.0);
    }

    #[test]
    fn test_make_spark_diamond_edge() {
        let spark = make_spark(100.0, 100.0, 0.0, 0.0);
        assert_eq!(spark.width, 100.0);
        assert_eq!(spark.height, 100.0);
        assert!(spark.path.contains("C 50 0 100 50 100 50"));
    }

    #[test]
    fn test_make_spark_non_square() {
        let spark = make_spark(200.0, 100.0, 0.7, 0.0);
        assert_eq!(spark.width, 200.0);
        assert_eq!(spark.height, 100.0);
        assert_eq!(spark.transform_origin, "100 50");
    }
}
