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

/// Parses an SVG `d` attribute into a tiny-skia [`Path`].
///
/// Supports the full SVG 1.1 / SVG 2 path grammar via [`svgtypes::PathParser`]:
/// every command in absolute and relative form (`M L H V C S Q T A Z`),
/// implicit command repetition, `M` followed by implicit `L`, compact numbers
/// (`82-78`, `.5.5`, `1e-3`) and arc flags written without separators.
///
/// Malformed data follows the SVG error-handling rules: everything up to the
/// first error is drawn, and a `tracing` warning names the problem so a broken
/// path never disappears silently. Returns `None` only when nothing drawable
/// was parsed (empty input, a lone `M`, or an error before the first segment).
pub(crate) fn svgpath_to_tiny_skia(d: &str) -> Option<Path> {
    let (builder, error) = build_svg_path(d);
    if let Some(error) = error {
        tracing::warn!(
            path = %truncate_for_log(d),
            %error,
            "malformed SVG path data; rendering only the segments before the error"
        );
    }
    builder.finish()
}

/// Strict variant of [`svgpath_to_tiny_skia`]: any syntax error is returned
/// instead of rendering the valid prefix. `Ok(None)` means the data was valid
/// but contains nothing drawable (e.g. an empty string or a lone `M`).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn try_svgpath_to_tiny_skia(d: &str) -> Result<Option<Path>, svgtypes::Error> {
    let (builder, error) = build_svg_path(d);
    match error {
        Some(error) => Err(error),
        None => Ok(builder.finish()),
    }
}

fn truncate_for_log(d: &str) -> &str {
    const MAX: usize = 120;
    match d.char_indices().nth(MAX) {
        Some((idx, _)) => &d[..idx],
        None => d,
    }
}

/// Walks the path segments, resolving relative coordinates and smooth-curve
/// reflections, and stops at the first parse error.
fn build_svg_path(d: &str) -> (PathBuilder, Option<svgtypes::Error>) {
    use svgtypes::{PathParser, PathSegment};

    /// Control point of the previous segment, kept for `S`/`T` reflection.
    #[derive(Clone, Copy)]
    enum LastControl {
        None,
        Cubic(f64, f64),
        Quad(f64, f64),
    }

    let mut pb = PathBuilder::new();
    // Current point and start of the current subpath, in absolute coordinates.
    let (mut cx, mut cy) = (0.0f64, 0.0f64);
    let (mut sx, mut sy) = (0.0f64, 0.0f64);
    let mut last = LastControl::None;

    for segment in PathParser::from(d) {
        let segment = match segment {
            Ok(segment) => segment,
            Err(error) => return (pb, Some(error)),
        };
        let (ox, oy) = (cx, cy);
        let resolve = |abs: bool, x: f64, y: f64| if abs { (x, y) } else { (ox + x, oy + y) };

        last = match segment {
            PathSegment::MoveTo { abs, x, y } => {
                (cx, cy) = resolve(abs, x, y);
                (sx, sy) = (cx, cy);
                pb.move_to(cx as f32, cy as f32);
                LastControl::None
            }
            PathSegment::LineTo { abs, x, y } => {
                (cx, cy) = resolve(abs, x, y);
                pb.line_to(cx as f32, cy as f32);
                LastControl::None
            }
            PathSegment::HorizontalLineTo { abs, x } => {
                cx = if abs { x } else { ox + x };
                pb.line_to(cx as f32, cy as f32);
                LastControl::None
            }
            PathSegment::VerticalLineTo { abs, y } => {
                cy = if abs { y } else { oy + y };
                pb.line_to(cx as f32, cy as f32);
                LastControl::None
            }
            PathSegment::CurveTo {
                abs,
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                let (x1, y1) = resolve(abs, x1, y1);
                let (x2, y2) = resolve(abs, x2, y2);
                (cx, cy) = resolve(abs, x, y);
                pb.cubic_to(
                    x1 as f32, y1 as f32, x2 as f32, y2 as f32, cx as f32, cy as f32,
                );
                LastControl::Cubic(x2, y2)
            }
            PathSegment::SmoothCurveTo { abs, x2, y2, x, y } => {
                let (x1, y1) = match last {
                    LastControl::Cubic(px, py) => (2.0 * ox - px, 2.0 * oy - py),
                    _ => (ox, oy),
                };
                let (x2, y2) = resolve(abs, x2, y2);
                (cx, cy) = resolve(abs, x, y);
                pb.cubic_to(
                    x1 as f32, y1 as f32, x2 as f32, y2 as f32, cx as f32, cy as f32,
                );
                LastControl::Cubic(x2, y2)
            }
            PathSegment::Quadratic { abs, x1, y1, x, y } => {
                let (x1, y1) = resolve(abs, x1, y1);
                (cx, cy) = resolve(abs, x, y);
                pb.quad_to(x1 as f32, y1 as f32, cx as f32, cy as f32);
                LastControl::Quad(x1, y1)
            }
            PathSegment::SmoothQuadratic { abs, x, y } => {
                let (x1, y1) = match last {
                    LastControl::Quad(px, py) => (2.0 * ox - px, 2.0 * oy - py),
                    _ => (ox, oy),
                };
                (cx, cy) = resolve(abs, x, y);
                pb.quad_to(x1 as f32, y1 as f32, cx as f32, cy as f32);
                LastControl::Quad(x1, y1)
            }
            PathSegment::EllipticalArc {
                abs,
                rx,
                ry,
                x_axis_rotation,
                large_arc,
                sweep,
                x,
                y,
            } => {
                (cx, cy) = resolve(abs, x, y);
                append_svg_arc(
                    &mut pb,
                    ox as f32,
                    oy as f32,
                    rx as f32,
                    ry as f32,
                    x_axis_rotation as f32,
                    large_arc,
                    sweep,
                    cx as f32,
                    cy as f32,
                );
                LastControl::None
            }
            PathSegment::ClosePath { .. } => {
                pb.close();
                (cx, cy) = (sx, sy);
                LastControl::None
            }
        };
    }

    (pb, None)
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

#[cfg(test)]
mod tests {
    use super::{svgpath_to_tiny_skia, try_svgpath_to_tiny_skia};
    use tiny_skia::{Color, FillRule, Paint, Path, PathSegment as Seg, Pixmap, Point, Transform};

    /// The R mark from `assets/logo.svg`, exactly as exported (relative
    /// commands, implicit repetition, compact numbers).
    const LOGO_R: &str = "M48 36h107c77 0 121 42 121 105 0 52-28 82-78 96l96 89h-82l-82-81v81H48V36Zm82 62v81h22c28 0 44-13 44-40s-16-41-44-41h-22Z";

    fn parse(d: &str) -> Path {
        try_svgpath_to_tiny_skia(d)
            .unwrap_or_else(|e| panic!("{d:?} failed to parse: {e}"))
            .unwrap_or_else(|| panic!("{d:?} produced no path"))
    }

    fn segments(d: &str) -> Vec<Seg> {
        parse(d).segments().collect()
    }

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
    }

    fn same_segment(a: &Seg, b: &Seg) -> bool {
        match (a, b) {
            (Seg::MoveTo(p), Seg::MoveTo(q)) | (Seg::LineTo(p), Seg::LineTo(q)) => close(*p, *q),
            (Seg::QuadTo(p1, p), Seg::QuadTo(q1, q)) => close(*p1, *q1) && close(*p, *q),
            (Seg::CubicTo(p1, p2, p), Seg::CubicTo(q1, q2, q)) => {
                close(*p1, *q1) && close(*p2, *q2) && close(*p, *q)
            }
            (Seg::Close, Seg::Close) => true,
            _ => false,
        }
    }

    fn assert_same(a: &str, b: &str) {
        let (sa, sb) = (segments(a), segments(b));
        assert!(
            sa.len() == sb.len() && sa.iter().zip(&sb).all(|(x, y)| same_segment(x, y)),
            "{a:?} and {b:?} differ:\n  {sa:?}\n  {sb:?}"
        );
    }

    fn assert_segments(d: &str, expected: &[Seg]) {
        let actual = segments(d);
        assert!(
            actual.len() == expected.len()
                && actual.iter().zip(expected).all(|(x, y)| same_segment(x, y)),
            "{d:?}:\n  got      {actual:?}\n  expected {expected:?}"
        );
    }

    fn pt(x: f32, y: f32) -> Point {
        Point::from_xy(x, y)
    }

    #[test]
    fn absolute_commands() {
        assert_segments(
            "M10 20 L30 40 H50 V60 C1 2 3 4 5 6 Q7 8 9 10 Z",
            &[
                Seg::MoveTo(pt(10.0, 20.0)),
                Seg::LineTo(pt(30.0, 40.0)),
                Seg::LineTo(pt(50.0, 40.0)),
                Seg::LineTo(pt(50.0, 60.0)),
                Seg::CubicTo(pt(1.0, 2.0), pt(3.0, 4.0), pt(5.0, 6.0)),
                Seg::QuadTo(pt(7.0, 8.0), pt(9.0, 10.0)),
                Seg::Close,
            ],
        );
    }

    #[test]
    fn relative_commands_match_absolute_equivalents() {
        let pairs = [
            ("M5 5 m10 20 l5 5", "M5 5 M15 25 L20 30"),
            ("M10 20 l30 40", "M10 20 L40 60"),
            ("M10 20 h30", "M10 20 H40"),
            ("M10 20 v30", "M10 20 V50"),
            ("M10 20 c1 2 3 4 5 6", "M10 20 C11 22 13 24 15 26"),
            (
                "M10 20 C0 0 20 10 20 20 s10 20 20 20",
                "M10 20 C0 0 20 10 20 20 S30 40 40 40",
            ),
            ("M10 20 q5 -5 10 0", "M10 20 Q15 15 20 20"),
            ("M10 20 Q15 15 20 20 t10 0", "M10 20 Q15 15 20 20 T30 20"),
            ("M10 20 a5 5 0 0 1 10 0", "M10 20 A5 5 0 0 1 20 20"),
            ("M10 20 a5 8 30 1 0 10 10", "M10 20 A5 8 30 1 0 20 30"),
            ("M10 20 h10 v10 z l5 5", "M10 20 H20 V30 Z L15 25"),
        ];
        for (relative, absolute) in pairs {
            assert_same(relative, absolute);
        }
    }

    #[test]
    fn smooth_cubic_reflects_previous_control_point() {
        assert_segments(
            "M0 0 C10 0 20 10 20 20 S30 40 40 40",
            &[
                Seg::MoveTo(pt(0.0, 0.0)),
                Seg::CubicTo(pt(10.0, 0.0), pt(20.0, 10.0), pt(20.0, 20.0)),
                Seg::CubicTo(pt(20.0, 30.0), pt(30.0, 40.0), pt(40.0, 40.0)),
            ],
        );
        // Without a preceding cubic the first control point is the current point.
        assert_segments(
            "M0 0 L10 0 S20 10 30 0",
            &[
                Seg::MoveTo(pt(0.0, 0.0)),
                Seg::LineTo(pt(10.0, 0.0)),
                Seg::CubicTo(pt(10.0, 0.0), pt(20.0, 10.0), pt(30.0, 0.0)),
            ],
        );
        // A quadratic control point is not reflected into a smooth cubic.
        assert_segments(
            "M0 0 Q5 5 10 0 S20 10 30 0",
            &[
                Seg::MoveTo(pt(0.0, 0.0)),
                Seg::QuadTo(pt(5.0, 5.0), pt(10.0, 0.0)),
                Seg::CubicTo(pt(10.0, 0.0), pt(20.0, 10.0), pt(30.0, 0.0)),
            ],
        );
    }

    #[test]
    fn smooth_quadratic_reflects_previous_control_point() {
        assert_segments(
            "M0 0 Q10 10 20 0 T40 0 T60 0",
            &[
                Seg::MoveTo(pt(0.0, 0.0)),
                Seg::QuadTo(pt(10.0, 10.0), pt(20.0, 0.0)),
                Seg::QuadTo(pt(30.0, -10.0), pt(40.0, 0.0)),
                Seg::QuadTo(pt(50.0, 10.0), pt(60.0, 0.0)),
            ],
        );
        // Without a preceding quadratic the control point is the current point.
        assert_segments(
            "M0 0 L10 0 T20 0",
            &[
                Seg::MoveTo(pt(0.0, 0.0)),
                Seg::LineTo(pt(10.0, 0.0)),
                Seg::QuadTo(pt(10.0, 0.0), pt(20.0, 0.0)),
            ],
        );
    }

    #[test]
    fn arcs_produce_curves_ending_at_the_target() {
        let segs = segments("M0 0 A10 10 0 0 1 20 0");
        assert!(segs.len() > 1, "{segs:?}");
        assert!(segs[1..].iter().all(|s| matches!(s, Seg::CubicTo(..))));
        let Some(Seg::CubicTo(_, _, end)) = segs.last() else {
            unreachable!()
        };
        assert!(close(*end, pt(20.0, 0.0)), "{end:?}");
        // Zero radius degrades to a straight line, per the SVG spec.
        assert_segments(
            "M0 0 A0 5 0 0 1 20 0",
            &[Seg::MoveTo(pt(0.0, 0.0)), Seg::LineTo(pt(20.0, 0.0))],
        );
    }

    #[test]
    fn implicit_command_repetition() {
        assert_same("M0 0 L10 0 20 0 20 10", "M0 0 L10 0 L20 0 L20 10");
        assert_same("M0 0 l10 0 10 0", "M0 0 L10 0 L20 0");
        assert_same("M0 0 h5 5 v5 5", "M0 0 H5 H10 V5 V10");
        assert_same(
            "M0 0 c1 1 2 2 3 3 1 1 2 2 3 3",
            "M0 0 C1 1 2 2 3 3 C4 4 5 5 6 6",
        );
    }

    #[test]
    fn moveto_followed_by_implicit_lineto() {
        assert_same("M0 0 10 0 10 10", "M0 0 L10 0 L10 10");
        // Relative moveto implies relative lineto.
        assert_same("M5 5 m10 10 5 0 0 5", "M5 5 M15 15 L20 15 L20 20");
    }

    #[test]
    fn compact_number_syntax() {
        assert_segments(
            "M.5.5L1e1-2.5l-.5-.5",
            &[
                Seg::MoveTo(pt(0.5, 0.5)),
                Seg::LineTo(pt(10.0, -2.5)),
                Seg::LineTo(pt(9.5, -3.0)),
            ],
        );
        assert_same("M0,0L1E+1,2.5e-1", "M0 0 L10 0.25");
        assert_same(
            "M0 0c77 0 121 42 121 105 0 52-28 82-78 96",
            "M0 0 C77 0 121 42 121 105 C121 157 93 187 43 201",
        );
    }

    #[test]
    fn arc_flags_without_separators() {
        assert_same("M0 0a5 5 0 1010 0", "M0 0 a5 5 0 1 0 10 0");
        assert_same("M0 0A5,5,0,0,1,10,0", "M0 0 A5 5 0 0 1 10 0");
    }

    #[test]
    fn logo_r_mark_parses_with_expected_geometry() {
        let path = parse(LOGO_R);
        let bounds = path.bounds();
        assert_eq!(
            (bounds.left(), bounds.top(), bounds.right(), bounds.bottom()),
            (48.0, 36.0, 294.0, 326.0)
        );
        let closes = path.segments().filter(|s| matches!(s, Seg::Close)).count();
        assert_eq!(closes, 2);

        // Rasterise with the same fill rule the renderer uses: the stem is
        // filled and the bowl's counter is a hole.
        let mut pixmap = Pixmap::new(320, 340).unwrap();
        let mut paint = Paint::default();
        paint.set_color(Color::WHITE);
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
        let alpha = |x: u32, y: u32| pixmap.pixel(x, y).unwrap().alpha();
        assert_eq!(alpha(80, 200), 255, "stem should be filled");
        assert_eq!(alpha(160, 140), 0, "bowl counter should be empty");
        assert_eq!(alpha(250, 300), 255, "leg should be filled");
        assert_eq!(alpha(10, 10), 0, "outside should be empty");
    }

    #[test]
    fn malformed_path_reports_error_and_keeps_valid_prefix() {
        let bad = "M0 0 L10 10 L20 nope";
        assert!(try_svgpath_to_tiny_skia(bad).is_err());
        let prefix = svgpath_to_tiny_skia(bad).expect("valid prefix should still render");
        let segs: Vec<_> = prefix.segments().collect();
        assert_eq!(segs.len(), 2, "{segs:?}");

        for garbage in ["hello", "L10 10", "M0 0 Z 5 5", "M0 0 L10"] {
            assert!(
                try_svgpath_to_tiny_skia(garbage).is_err(),
                "{garbage:?} should be rejected"
            );
        }
        assert!(svgpath_to_tiny_skia("hello").is_none());
    }

    #[test]
    fn empty_and_degenerate_paths_are_not_errors() {
        assert!(matches!(try_svgpath_to_tiny_skia(""), Ok(None)));
        assert!(matches!(try_svgpath_to_tiny_skia("  "), Ok(None)));
        assert!(matches!(try_svgpath_to_tiny_skia("M10 10"), Ok(None)));
    }
}
