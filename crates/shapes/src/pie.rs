//! Pie shape path generator (pie chart / arc slices).

use crate::shape_output::ShapeOutput;
use std::f64::consts::PI;

#[inline]
fn clean_coord(val: f64) -> f64 {
    if val.abs() < 1e-10 {
        0.0
    } else {
        val
    }
}

/// Generates parametric SVG path string and metadata for a pie slice or arc.
pub fn make_pie(
    radius: f64,
    progress: f64,
    close_path: bool,
    counter_clockwise: bool,
    rotation: f64,
) -> ShapeOutput {
    let r = radius.max(0.0);
    let size = r * 2.0;
    let clamped_p = progress.clamp(0.0, 1.0);

    if r == 0.0 || clamped_p <= 0.0 {
        return ShapeOutput::new(String::new(), size, size, format!("{r} {r}"));
    }

    let start_angle = -PI / 2.0 + rotation;
    let factor = if counter_clockwise { -1.0 } else { 1.0 };
    let sweep_flag = if counter_clockwise { 0 } else { 1 };

    let end_angle = start_angle + factor * clamped_p * (PI * 2.0);

    let x0 = clean_coord(r + r * start_angle.cos());
    let y0 = clean_coord(r + r * start_angle.sin());

    let x_end = clean_coord(r + r * end_angle.cos());
    let y_end = clean_coord(r + r * end_angle.sin());

    let arc_body = if clamped_p <= 0.5 {
        format!("A {r} {r} 0 0 {sweep_flag} {x_end:.4} {y_end:.4}")
    } else {
        let mid_angle = start_angle + factor * 0.5 * (PI * 2.0);
        let x_mid = clean_coord(r + r * mid_angle.cos());
        let y_mid = clean_coord(r + r * mid_angle.sin());
        format!("A {r} {r} 0 0 {sweep_flag} {x_mid:.4} {y_mid:.4} A {r} {r} 0 0 {sweep_flag} {x_end:.4} {y_end:.4}")
    };

    let path = if close_path {
        if clamped_p < 1.0 {
            format!("M {x0:.4} {y0:.4} {arc_body} L {r} {r} Z")
        } else {
            format!("M {x0:.4} {y0:.4} {arc_body} Z")
        }
    } else {
        format!("M {x0:.4} {y0:.4} {arc_body}")
    };

    ShapeOutput::new(path, size, size, format!("{r} {r}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_pie_quarter() {
        let pie = make_pie(100.0, 0.25, true, false, 0.0);
        assert_eq!(pie.width, 200.0);
        assert_eq!(pie.height, 200.0);
        assert_eq!(pie.transform_origin, "100 100");
        assert!(pie.path.starts_with("M 100.0000 0.0000"));
        assert!(pie.path.contains("A 100 100 0 0 1 200.0000 100.0000"));
        assert!(pie.path.ends_with("L 100 100 Z"));
    }

    #[test]
    fn test_make_pie_split_arc() {
        let pie = make_pie(100.0, 0.75, true, false, 0.0);
        assert_eq!(pie.width, 200.0);
        assert_eq!(pie.height, 200.0);
        // Split arc: first arc to bottom (100, 200), second arc to left (0, 100)
        assert!(pie.path.contains("A 100 100 0 0 1 100.0000 200.0000"));
        assert!(pie.path.contains("A 100 100 0 0 1 0.0000 100.0000"));
        assert!(pie.path.ends_with("L 100 100 Z"));
    }

    #[test]
    fn test_make_pie_full_circle() {
        let pie = make_pie(100.0, 1.0, true, false, 0.0);
        assert_eq!(pie.width, 200.0);
        assert_eq!(pie.height, 200.0);
        assert!(pie.path.ends_with("Z"));
        assert!(!pie.path.contains("L 100 100"));
    }

    #[test]
    fn test_make_pie_open_arc() {
        let pie = make_pie(100.0, 0.5, false, false, 0.0);
        assert!(!pie.path.contains("L 100 100"));
        assert!(!pie.path.ends_with("Z"));
    }

    #[test]
    fn test_make_pie_counter_clockwise() {
        let pie = make_pie(100.0, 0.25, true, true, 0.0);
        assert!(pie.path.contains("A 100 100 0 0 0 0.0000 100.0000"));
    }

    #[test]
    fn test_make_pie_zero_radius() {
        let pie = make_pie(0.0, 0.5, true, false, 0.0);
        assert_eq!(pie.path, "");
        assert_eq!(pie.width, 0.0);
        assert_eq!(pie.height, 0.0);
    }

    #[test]
    fn test_make_pie_zero_progress() {
        let pie = make_pie(50.0, 0.0, true, false, 0.0);
        assert_eq!(pie.path, "");
        assert_eq!(pie.width, 100.0);
        assert_eq!(pie.height, 100.0);
    }

    #[test]
    fn test_make_pie_with_rotation() {
        let pie = make_pie(100.0, 0.25, true, false, PI / 2.0);
        // Start angle rotated by 90deg -> starts at (200, 100) (3 o'clock)
        assert!(pie.path.starts_with("M 200.0000 100.0000"));
    }
}
