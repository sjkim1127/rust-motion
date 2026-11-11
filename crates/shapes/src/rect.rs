//! Rect shape path generation and geometry.

/// Generates SVG path string and dimensions for a rectangle.
pub fn make_rect(width: f64, height: f64, corner_radius: f64) -> (String, f64, f64) {
    let w = width.max(0.0);
    let h = height.max(0.0);
    let max_r = (w / 2.0).min(h / 2.0);
    let r = corner_radius.clamp(0.0, max_r);

    let path = if r <= 0.0 {
        format!("M 0 0 L {w} 0 L {w} {h} L 0 {h} Z")
    } else {
        format!(
            "M {r} 0 L {} 0 A {r} {r} 0 0 1 {w} {r} L {w} {} A {r} {r} 0 0 1 {} {h} L {r} {h} A {r} {r} 0 0 1 0 {} L 0 {r} A {r} {r} 0 0 1 {r} 0 Z",
            w - r,
            h - r,
            w - r,
            h - r
        )
    };

    (path, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_rect_flat() {
        let (path, w, h) = make_rect(200.0, 100.0, 0.0);
        assert_eq!(w, 200.0);
        assert_eq!(h, 100.0);
        assert_eq!(path, "M 0 0 L 200 0 L 200 100 L 0 100 Z");
    }

    #[test]
    fn test_make_rect_rounded() {
        let (path, w, h) = make_rect(200.0, 100.0, 10.0);
        assert_eq!(w, 200.0);
        assert_eq!(h, 100.0);
        assert!(path.contains("M 10 0 L 190 0 A 10 10"));
    }
}
