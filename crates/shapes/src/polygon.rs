//! Polygon shape path generator (regular N-sided polygons).

use std::f64::consts::PI;

/// Generates SVG path string and dimensions for a regular N-sided polygon.
pub fn make_polygon(points: usize, radius: f64) -> (String, f64, f64) {
    let pts = points.max(3);
    let r = radius.max(0.0);
    let size = r * 2.0;
    let center_x = r;
    let center_y = r;

    let degree_increment = (PI * 2.0) / (pts as f64);
    let mut path_cmd = String::new();

    for i in 0..pts {
        let angle = degree_increment * (i as f64) - PI / 2.0;
        let x = center_x + r * angle.cos();
        let y = center_y + r * angle.sin();

        if i == 0 {
            path_cmd.push_str(&format!("M {x:.4} {y:.4}"));
        } else {
            path_cmd.push_str(&format!(" L {x:.4} {y:.4}"));
        }
    }
    path_cmd.push_str(" Z");

    (path_cmd, size, size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_hexagon_geometry() {
        let (path, w, h) = make_polygon(6, 100.0);
        assert_eq!(w, 200.0);
        assert_eq!(h, 200.0);
        assert!(path.starts_with("M 100.0000 0.0000"));
        assert!(path.ends_with("Z"));
    }
}
