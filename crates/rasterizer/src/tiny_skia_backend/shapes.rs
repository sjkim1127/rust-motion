//! SVG path parsing and geometry builders (circle, rounded rect).

use tiny_skia::{Path, PathBuilder, Rect};

pub(crate) fn build_circle(cx: f32, cy: f32, r: f32) -> Path {
    let mut pb = PathBuilder::new();
    // Approximate circle with 4 cubic bezier curves (standard approximation)
    let k = 0.552_284_8 * r;
    pb.move_to(cx, cy - r);
    pb.cubic_to(cx + k, cy - r, cx + r, cy - k, cx + r, cy);
    pb.cubic_to(cx + r, cy + k, cx + k, cy + r, cx, cy + r);
    pb.cubic_to(cx - k, cy + r, cx - r, cy + k, cx - r, cy);
    pb.cubic_to(cx - r, cy - k, cx - k, cy - r, cx, cy - r);
    pb.close();
    pb.finish()
        .unwrap_or_else(|| PathBuilder::new().finish().unwrap())
}

pub(crate) fn build_rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();

    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();

    pb.finish()
        .unwrap_or_else(|| PathBuilder::from_rect(Rect::from_xywh(x, y, w, h).unwrap()))
}

/// Minimal SVG `d` attribute parser → tiny-skia PathBuilder.
/// Supports M, L, H, V, C, Q, A, Z commands. Shape emitters currently use
/// absolute commands; relative elliptical arcs are accepted as well.
pub(crate) fn svgpath_to_tiny_skia(d: &str) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let tokens = tokenize_path(d);
    let mut pos = 0usize;

    let mut cx = 0.0f32;
    let mut cy = 0.0f32;
    let mut subpath_x = 0.0f32;
    let mut subpath_y = 0.0f32;

    while pos < tokens.len() {
        match tokens[pos].as_str() {
            "M" => {
                pos += 1;
                let x = parse_f32(&tokens, &mut pos)?;
                let y = parse_f32(&tokens, &mut pos)?;
                pb.move_to(x, y);
                cx = x;
                cy = y;
                subpath_x = x;
                subpath_y = y;
            }
            "L" => {
                pos += 1;
                let x = parse_f32(&tokens, &mut pos)?;
                let y = parse_f32(&tokens, &mut pos)?;
                pb.line_to(x, y);
                cx = x;
                cy = y;
            }
            "H" => {
                pos += 1;
                let x = parse_f32(&tokens, &mut pos)?;
                pb.line_to(x, cy);
                cx = x;
            }
            "V" => {
                pos += 1;
                let y = parse_f32(&tokens, &mut pos)?;
                pb.line_to(cx, y);
                cy = y;
            }
            "C" => {
                pos += 1;
                let x1 = parse_f32(&tokens, &mut pos)?;
                let y1 = parse_f32(&tokens, &mut pos)?;
                let x2 = parse_f32(&tokens, &mut pos)?;
                let y2 = parse_f32(&tokens, &mut pos)?;
                let x = parse_f32(&tokens, &mut pos)?;
                let y = parse_f32(&tokens, &mut pos)?;
                pb.cubic_to(x1, y1, x2, y2, x, y);
                cx = x;
                cy = y;
            }
            "Q" => {
                pos += 1;
                let x1 = parse_f32(&tokens, &mut pos)?;
                let y1 = parse_f32(&tokens, &mut pos)?;
                let x = parse_f32(&tokens, &mut pos)?;
                let y = parse_f32(&tokens, &mut pos)?;
                pb.quad_to(x1, y1, x, y);
                cx = x;
                cy = y;
            }
            "A" | "a" => {
                let relative = tokens[pos] == "a";
                pos += 1;
                let rx = parse_f32(&tokens, &mut pos)?;
                let ry = parse_f32(&tokens, &mut pos)?;
                let rotation = parse_f32(&tokens, &mut pos)?;
                let large_arc = parse_f32(&tokens, &mut pos)? != 0.0;
                let sweep = parse_f32(&tokens, &mut pos)? != 0.0;
                let mut x = parse_f32(&tokens, &mut pos)?;
                let mut y = parse_f32(&tokens, &mut pos)?;
                if relative {
                    x += cx;
                    y += cy;
                }
                append_svg_arc(&mut pb, cx, cy, rx, ry, rotation, large_arc, sweep, x, y);
                cx = x;
                cy = y;
            }
            "Z" | "z" => {
                pb.close();
                pos += 1;
                cx = subpath_x;
                cy = subpath_y;
            }
            _ => return None,
        }
    }

    pb.finish()
}

#[allow(clippy::too_many_arguments)]
fn append_svg_arc(
    path: &mut PathBuilder,
    start_x: f32,
    start_y: f32,
    radius_x: f32,
    radius_y: f32,
    rotation_degrees: f32,
    large_arc: bool,
    sweep: bool,
    end_x: f32,
    end_y: f32,
) {
    if (start_x - end_x).abs() < f32::EPSILON && (start_y - end_y).abs() < f32::EPSILON {
        return;
    }
    let mut rx = radius_x.abs();
    let mut ry = radius_y.abs();
    if rx <= f32::EPSILON || ry <= f32::EPSILON {
        path.line_to(end_x, end_y);
        return;
    }

    let phi = rotation_degrees
        .to_radians()
        .rem_euclid(std::f32::consts::TAU);
    let (sin_phi, cos_phi) = phi.sin_cos();
    let half_dx = (start_x - end_x) * 0.5;
    let half_dy = (start_y - end_y) * 0.5;
    let start_prime_x = cos_phi * half_dx + sin_phi * half_dy;
    let start_prime_y = -sin_phi * half_dx + cos_phi * half_dy;

    let radii_scale = start_prime_x.powi(2) / rx.powi(2) + start_prime_y.powi(2) / ry.powi(2);
    if radii_scale > 1.0 {
        let scale = radii_scale.sqrt();
        rx *= scale;
        ry *= scale;
    }

    let rx_squared = rx.powi(2);
    let ry_squared = ry.powi(2);
    let x_squared = start_prime_x.powi(2);
    let y_squared = start_prime_y.powi(2);
    let numerator =
        (rx_squared * ry_squared - rx_squared * y_squared - ry_squared * x_squared).max(0.0);
    let denominator = rx_squared * y_squared + ry_squared * x_squared;
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let factor = if denominator <= f32::EPSILON {
        0.0
    } else {
        sign * (numerator / denominator).sqrt()
    };
    let center_prime_x = factor * rx * start_prime_y / ry;
    let center_prime_y = factor * -ry * start_prime_x / rx;
    let center_x = cos_phi * center_prime_x - sin_phi * center_prime_y + (start_x + end_x) * 0.5;
    let center_y = sin_phi * center_prime_x + cos_phi * center_prime_y + (start_y + end_y) * 0.5;

    let start_vector = (
        (start_prime_x - center_prime_x) / rx,
        (start_prime_y - center_prime_y) / ry,
    );
    let end_vector = (
        (-start_prime_x - center_prime_x) / rx,
        (-start_prime_y - center_prime_y) / ry,
    );
    let start_angle = start_vector.1.atan2(start_vector.0);
    let mut sweep_angle = vector_angle(start_vector, end_vector);
    if !sweep && sweep_angle > 0.0 {
        sweep_angle -= std::f32::consts::TAU;
    } else if sweep && sweep_angle < 0.0 {
        sweep_angle += std::f32::consts::TAU;
    }

    let segment_count = (sweep_angle.abs() / std::f32::consts::FRAC_PI_2)
        .ceil()
        .max(1.0) as usize;
    let segment_angle = sweep_angle / segment_count as f32;
    for segment in 0..segment_count {
        let angle_start = start_angle + segment_angle * segment as f32;
        let angle_end = angle_start + segment_angle;
        let alpha = 4.0 / 3.0 * (segment_angle * 0.25).tan();
        let start = ellipse_point(center_x, center_y, rx, ry, sin_phi, cos_phi, angle_start);
        let end = ellipse_point(center_x, center_y, rx, ry, sin_phi, cos_phi, angle_end);
        let start_derivative = ellipse_derivative(rx, ry, sin_phi, cos_phi, angle_start);
        let end_derivative = ellipse_derivative(rx, ry, sin_phi, cos_phi, angle_end);
        path.cubic_to(
            start.0 + alpha * start_derivative.0,
            start.1 + alpha * start_derivative.1,
            end.0 - alpha * end_derivative.0,
            end.1 - alpha * end_derivative.1,
            end.0,
            end.1,
        );
    }
}

fn vector_angle(from: (f32, f32), to: (f32, f32)) -> f32 {
    (from.0 * to.1 - from.1 * to.0).atan2(from.0 * to.0 + from.1 * to.1)
}

fn ellipse_point(
    center_x: f32,
    center_y: f32,
    rx: f32,
    ry: f32,
    sin_phi: f32,
    cos_phi: f32,
    angle: f32,
) -> (f32, f32) {
    let (sin_angle, cos_angle) = angle.sin_cos();
    (
        center_x + rx * cos_phi * cos_angle - ry * sin_phi * sin_angle,
        center_y + rx * sin_phi * cos_angle + ry * cos_phi * sin_angle,
    )
}

fn ellipse_derivative(rx: f32, ry: f32, sin_phi: f32, cos_phi: f32, angle: f32) -> (f32, f32) {
    let (sin_angle, cos_angle) = angle.sin_cos();
    (
        -rx * cos_phi * sin_angle - ry * sin_phi * cos_angle,
        -rx * sin_phi * sin_angle + ry * cos_phi * cos_angle,
    )
}

fn tokenize_path(d: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in d.chars() {
        if c.is_alphabetic() {
            if !current.trim().is_empty() {
                tokens.push(current.trim().to_string());
                current = String::new();
            }
            tokens.push(c.to_string());
        } else if c == ',' || c.is_whitespace() {
            if !current.trim().is_empty() {
                tokens.push(current.trim().to_string());
                current = String::new();
            }
        } else {
            current.push(c);
        }
    }
    if !current.trim().is_empty() {
        tokens.push(current.trim().to_string());
    }
    tokens
}

fn parse_f32(tokens: &[String], pos: &mut usize) -> Option<f32> {
    if *pos >= tokens.len() {
        return None;
    }
    let v = tokens[*pos].parse::<f32>().ok()?;
    *pos += 1;
    Some(v)
}
