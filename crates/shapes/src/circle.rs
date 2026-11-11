//! Circle shape path generation and geometry.

/// Generates SVG path string and dimensions for a circle of given radius.
pub fn make_circle(radius: f64) -> (String, f64, f64) {
    let r = radius.max(0.0);
    let size = r * 2.0;
    // Circle drawn from top point (r, 0) down to (r, 2r) and back to (r, 0)
    let path = format!("M {r} 0 A {r} {r} 0 1 0 {r} {size} A {r} {r} 0 1 0 {r} 0 Z");
    (path, size, size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_circle_geometry() {
        let (path, w, h) = make_circle(50.0);
        assert_eq!(w, 100.0);
        assert_eq!(h, 100.0);
        assert!(path.contains("M 50 0"));
        assert!(path.contains("A 50 50"));
    }
}
