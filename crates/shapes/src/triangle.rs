//! Triangle shape path generator.

/// Generates SVG path string and dimensions for an equilateral triangle.
pub fn make_triangle(length: f64) -> (String, f64, f64) {
    let l = length.max(0.0);
    let h = l * (3.0_f64.sqrt() / 2.0);
    let half_l = l / 2.0;

    let path = format!("M {half_l:.4} 0 L {l:.4} {h:.4} L 0 {h:.4} Z");
    (path, l, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_triangle_geometry() {
        let (path, w, h) = make_triangle(100.0);
        assert_eq!(w, 100.0);
        assert!((h - 86.6025).abs() < 0.01);
        assert!(path.contains("M 50.0000 0"));
        assert!(path.contains("Z"));
    }
}
