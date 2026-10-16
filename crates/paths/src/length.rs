//! SVG path length calculation.

use crate::parser::parse_path;
use crate::types::Instruction;

/// Calculates total length of an SVG path string in pixels.
pub fn get_length(path: &str) -> f64 {
    let instructions = match parse_path(path) {
        Ok(insts) => insts,
        Err(_) => return 0.0,
    };

    get_instructions_length(&instructions)
}

/// Calculates total length of a list of [`Instruction`]s.
pub fn get_instructions_length(instructions: &[Instruction]) -> f64 {
    let mut total_length = 0.0;
    let mut current_x = 0.0;
    let mut current_y = 0.0;
    let mut start_x = 0.0;
    let mut start_y = 0.0;

    for inst in instructions {
        match inst {
            Instruction::MoveTo { x, y } => {
                current_x = *x;
                current_y = *y;
                start_x = *x;
                start_y = *y;
            }
            Instruction::LineTo { x, y } => {
                let dx = x - current_x;
                let dy = y - current_y;
                total_length += (dx * dx + dy * dy).sqrt();
                current_x = *x;
                current_y = *y;
            }
            Instruction::ClosePath => {
                let dx = start_x - current_x;
                let dy = start_y - current_y;
                total_length += (dx * dx + dy * dy).sqrt();
                current_x = start_x;
                current_y = start_y;
            }
            Instruction::CubicCurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                total_length +=
                    cubic_bezier_length((current_x, current_y), (*x1, *y1), (*x2, *y2), (*x, *y));
                current_x = *x;
                current_y = *y;
            }
            Instruction::QuadCurveTo { x1, y1, x, y } => {
                total_length += quad_bezier_length(current_x, current_y, *x1, *y1, *x, *y);
                current_x = *x;
                current_y = *y;
            }
            Instruction::ArcTo {
                rx,
                ry,
                x_axis_rotation,
                large_arc_flag,
                sweep_flag,
                x,
                y,
            } => {
                total_length += arc_segment_length(
                    (current_x, current_y),
                    (*x, *y),
                    *rx,
                    *ry,
                    *x_axis_rotation,
                    *large_arc_flag,
                    *sweep_flag,
                );
                current_x = *x;
                current_y = *y;
            }
        }
    }

    total_length
}

pub(crate) fn cubic_bezier_length(
    (x0, y0): (f64, f64),
    (x1, y1): (f64, f64),
    (x2, y2): (f64, f64),
    (x3, y3): (f64, f64),
) -> f64 {
    let steps = 16;
    let mut length = 0.0;
    let mut prev_x = x0;
    let mut prev_y = y0;

    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let mt = 1.0 - t;

        let px =
            mt * mt * mt * x0 + 3.0 * mt * mt * t * x1 + 3.0 * mt * t * t * x2 + t * t * t * x3;

        let py =
            mt * mt * mt * y0 + 3.0 * mt * mt * t * y1 + 3.0 * mt * t * t * y2 + t * t * t * y3;

        let dx = px - prev_x;
        let dy = py - prev_y;
        length += (dx * dx + dy * dy).sqrt();

        prev_x = px;
        prev_y = py;
    }

    length
}

fn quad_bezier_length(x0: f64, y0: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let steps = 16;
    let mut length = 0.0;
    let mut prev_x = x0;
    let mut prev_y = y0;

    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let mt = 1.0 - t;

        let px = mt * mt * x0 + 2.0 * mt * t * x1 + t * t * x2;
        let py = mt * mt * y0 + 2.0 * mt * t * y1 + t * t * y2;

        let dx = px - prev_x;
        let dy = py - prev_y;
        length += (dx * dx + dy * dy).sqrt();

        prev_x = px;
        prev_y = py;
    }

    length
}

/// Computes the length of an elliptical arc segment according to the SVG specification.
pub fn arc_segment_length(
    start: (f64, f64),
    end: (f64, f64),
    mut rx: f64,
    mut ry: f64,
    x_axis_rotation: f64,
    large_arc_flag: bool,
    sweep_flag: bool,
) -> f64 {
    let (x1, y1) = start;
    let (x2, y2) = end;

    if (x1 - x2).abs() < 1e-9 && (y1 - y2).abs() < 1e-9 {
        return 0.0;
    }

    rx = rx.abs();
    ry = ry.abs();
    if rx < 1e-9 || ry < 1e-9 {
        let dx = x2 - x1;
        let dy = y2 - y1;
        return (dx * dx + dy * dy).sqrt();
    }

    let phi = x_axis_rotation.to_radians();
    let cos_phi = phi.cos();
    let sin_phi = phi.sin();

    // Step 1: Compute (x1', y1')
    let dx = (x1 - x2) / 2.0;
    let dy = (y1 - y2) / 2.0;
    let x1_prime = cos_phi * dx + sin_phi * dy;
    let y1_prime = -sin_phi * dx + cos_phi * dy;

    // Step 2: Correct out-of-range radii (SVG spec F.6.2)
    let lambda = (x1_prime * x1_prime) / (rx * rx) + (y1_prime * y1_prime) / (ry * ry);
    if lambda > 1.0 {
        let sqrt_lambda = lambda.sqrt();
        rx *= sqrt_lambda;
        ry *= sqrt_lambda;
    }

    // Step 3: Compute (cx', cy') (SVG spec F.6.5)
    let rx_sq = rx * rx;
    let ry_sq = ry * ry;
    let x1_p_sq = x1_prime * x1_prime;
    let y1_p_sq = y1_prime * y1_prime;

    let num = (rx_sq * ry_sq - rx_sq * y1_p_sq - ry_sq * x1_p_sq).max(0.0);
    let den = rx_sq * y1_p_sq + ry_sq * x1_p_sq;
    let sq = if den < 1e-9 { 0.0 } else { num / den };
    let mut s = sq.sqrt();
    if large_arc_flag == sweep_flag {
        s = -s;
    }
    let cx_prime = s * (rx * y1_prime / ry);
    let cy_prime = s * (-ry * x1_prime / rx);

    // Step 4: Compute (cx, cy)
    let cx = cos_phi * cx_prime - sin_phi * cy_prime + (x1 + x2) / 2.0;
    let cy = sin_phi * cx_prime + cos_phi * cy_prime + (y1 + y2) / 2.0;

    // Step 5: Compute theta1 and delta_theta
    let v1 = ((x1_prime - cx_prime) / rx, (y1_prime - cy_prime) / ry);
    let v2 = ((-x1_prime - cx_prime) / rx, (-y1_prime - cy_prime) / ry);

    let theta1 = vector_angle((1.0, 0.0), v1);
    let mut delta_theta = vector_angle(v1, v2);

    let two_pi = std::f64::consts::TAU;
    if !sweep_flag && delta_theta > 0.0 {
        delta_theta -= two_pi;
    } else if sweep_flag && delta_theta < 0.0 {
        delta_theta += two_pi;
    }

    // Step 6: Sample points along arc to compute length
    let num_steps =
        ((delta_theta.abs() / (std::f64::consts::PI / 64.0)).ceil() as usize).clamp(32, 256);
    let mut length = 0.0;
    let mut prev_x = x1;
    let mut prev_y = y1;

    for i in 1..=num_steps {
        let t = i as f64 / num_steps as f64;
        let theta = theta1 + t * delta_theta;
        let cos_theta = theta.cos();
        let sin_theta = theta.sin();

        let px = cx + rx * cos_theta * cos_phi - ry * sin_theta * sin_phi;
        let py = cy + rx * cos_theta * sin_phi + ry * sin_theta * cos_phi;

        let seg_dx = px - prev_x;
        let seg_dy = py - prev_y;
        length += (seg_dx * seg_dx + seg_dy * seg_dy).sqrt();

        prev_x = px;
        prev_y = py;
    }

    length
}

pub(crate) fn vector_angle((ux, uy): (f64, f64), (vx, vy): (f64, f64)) -> f64 {
    let dot = ux * vx + uy * vy;
    let len_u = (ux * ux + uy * uy).sqrt();
    let len_v = (vx * vx + vy * vy).sqrt();
    if len_u < 1e-9 || len_v < 1e-9 {
        return 0.0;
    }
    let cos_val = (dot / (len_u * len_v)).clamp(-1.0, 1.0);
    let angle = cos_val.acos();
    if ux * vy - uy * vx < 0.0 {
        -angle
    } else {
        angle
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn test_rect_path_length() {
        let d = "M 0 0 L 100 0 L 100 100 L 0 100 Z";
        let len = get_length(d);
        assert_eq!(len, 400.0);
    }

    #[test]
    fn test_quarter_circle_arc_length() {
        // Quarter circle with radius 100 from (100, 0) to (0, 100)
        let len = arc_segment_length((100.0, 0.0), (0.0, 100.0), 100.0, 100.0, 0.0, false, true);
        let expected = 0.5 * PI * 100.0;
        assert!(
            (len - expected).abs() < 0.1,
            "expected ~{expected}, got {len}"
        );
    }

    #[test]
    fn test_semi_circle_arc_length() {
        // Semi-circle with radius 50
        let len = arc_segment_length((50.0, 0.0), (-50.0, 0.0), 50.0, 50.0, 0.0, false, true);
        let expected = PI * 50.0;
        assert!(
            (len - expected).abs() < 0.1,
            "expected ~{expected}, got {len}"
        );
    }

    #[test]
    fn test_cubic_bezier_length() {
        let d = "M 0 0 C 0 50 100 50 100 0";
        let len = get_length(d);
        assert!(
            len > 100.0,
            "Cubic bezier should be longer than chord (100.0), got {len}"
        );
    }

    #[test]
    fn test_quad_bezier_length() {
        let d = "M 0 0 Q 50 100 100 0";
        let len = get_length(d);
        assert!(
            len > 100.0,
            "Quad bezier should be longer than chord (100.0), got {len}"
        );
    }

    #[test]
    fn test_full_circle_arc_length() {
        // Full circle in two arcs
        let d = "M 100 0 A 100 100 0 1 0 -100 0 A 100 100 0 1 0 100 0 Z";
        let len = get_length(d);
        let expected = 2.0 * PI * 100.0;
        assert!(
            (len - expected).abs() < 1.0,
            "expected ~{expected}, got {len}"
        );
    }

    #[test]
    fn test_invalid_path_length_returns_zero() {
        let len = get_length("invalid path string !!!");
        assert_eq!(len, 0.0);
    }

    #[test]
    fn test_instructions_length_empty() {
        let len = get_instructions_length(&[]);
        assert_eq!(len, 0.0);
    }
}
