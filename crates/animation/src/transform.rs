//! CSS transform builder — Remotion `makeTransform` / `@remotion/animation-utils`.
//!
//! # Example
//! ```rust
//! use rust_motion_animation::transform::{make_transform, rotate, scale, translate_x};
//!
//! let css = make_transform(&[rotate(45.0), scale(1.5), translate_x(100.0)]);
//! assert_eq!(css, "rotate(45deg) scale(1.5) translateX(100px)");
//! ```

/// A single CSS transform function token.
#[derive(Debug, Clone, PartialEq)]
pub enum TransformOp {
    Rotate(f32),
    RotateX(f32),
    RotateY(f32),
    RotateZ(f32),
    Rotate3d(f32, f32, f32, f32),
    Scale(f32),
    ScaleX(f32),
    ScaleY(f32),
    ScaleZ(f32),
    Scale3d(f32, f32, f32),
    TranslateX(f32),
    TranslateY(f32),
    TranslateZ(f32),
    Translate(f32, f32),
    Translate3d(f32, f32, f32),
    SkewX(f32),
    SkewY(f32),
    Skew(f32, f32),
    Perspective(f32),
    Matrix(f32, f32, f32, f32, f32, f32),
    Matrix3d([f32; 16]),
}

impl TransformOp {
    pub fn to_css(&self) -> String {
        match self {
            Self::Rotate(deg) => format!("rotate({deg}deg)"),
            Self::RotateX(deg) => format!("rotateX({deg}deg)"),
            Self::RotateY(deg) => format!("rotateY({deg}deg)"),
            Self::RotateZ(deg) => format!("rotateZ({deg}deg)"),
            Self::Rotate3d(x, y, z, d) => format!("rotate3d({x}, {y}, {z}, {d}deg)"),
            Self::Scale(s) => format!("scale({s})"),
            Self::ScaleX(s) => format!("scaleX({s})"),
            Self::ScaleY(s) => format!("scaleY({s})"),
            Self::ScaleZ(s) => format!("scaleZ({s})"),
            Self::Scale3d(x, y, z) => format!("scale3d({x}, {y}, {z})"),
            Self::TranslateX(px) => format!("translateX({px}px)"),
            Self::TranslateY(px) => format!("translateY({px}px)"),
            Self::TranslateZ(px) => format!("translateZ({px}px)"),
            Self::Translate(x, y) => format!("translate({x}px, {y}px)"),
            Self::Translate3d(x, y, z) => format!("translate3d({x}px, {y}px, {z}px)"),
            Self::SkewX(deg) => format!("skewX({deg}deg)"),
            Self::SkewY(deg) => format!("skewY({deg}deg)"),
            Self::Skew(x, y) => format!("skew({x}deg, {y}deg)"),
            Self::Perspective(px) => format!("perspective({px}px)"),
            Self::Matrix(a, b, c, d, e, f) => format!("matrix({a}, {b}, {c}, {d}, {e}, {f})"),
            Self::Matrix3d(m) => {
                let vals: Vec<String> = m.iter().map(|v| v.to_string()).collect();
                format!("matrix3d({})", vals.join(", "))
            }
        }
    }
}

pub fn rotate(deg: f32) -> TransformOp {
    TransformOp::Rotate(deg)
}
pub fn rotate_x(deg: f32) -> TransformOp {
    TransformOp::RotateX(deg)
}
pub fn rotate_y(deg: f32) -> TransformOp {
    TransformOp::RotateY(deg)
}
pub fn rotate_z(deg: f32) -> TransformOp {
    TransformOp::RotateZ(deg)
}
pub fn rotate3d(x: f32, y: f32, z: f32, deg: f32) -> TransformOp {
    TransformOp::Rotate3d(x, y, z, deg)
}
pub fn scale(s: f32) -> TransformOp {
    TransformOp::Scale(s)
}
pub fn scale_x(s: f32) -> TransformOp {
    TransformOp::ScaleX(s)
}
pub fn scale_y(s: f32) -> TransformOp {
    TransformOp::ScaleY(s)
}
pub fn scale_z(s: f32) -> TransformOp {
    TransformOp::ScaleZ(s)
}
pub fn scale3d(x: f32, y: f32, z: f32) -> TransformOp {
    TransformOp::Scale3d(x, y, z)
}
pub fn translate_x(px: f32) -> TransformOp {
    TransformOp::TranslateX(px)
}
pub fn translate_y(px: f32) -> TransformOp {
    TransformOp::TranslateY(px)
}
pub fn translate_z(px: f32) -> TransformOp {
    TransformOp::TranslateZ(px)
}
pub fn translate(x: f32, y: f32) -> TransformOp {
    TransformOp::Translate(x, y)
}
pub fn translate3d(x: f32, y: f32, z: f32) -> TransformOp {
    TransformOp::Translate3d(x, y, z)
}
pub fn skew_x(deg: f32) -> TransformOp {
    TransformOp::SkewX(deg)
}
pub fn skew_y(deg: f32) -> TransformOp {
    TransformOp::SkewY(deg)
}
pub fn skew(x: f32, y: f32) -> TransformOp {
    TransformOp::Skew(x, y)
}
pub fn perspective(px: f32) -> TransformOp {
    TransformOp::Perspective(px)
}
pub fn matrix(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> TransformOp {
    TransformOp::Matrix(a, b, c, d, e, f)
}
pub fn matrix3d(m: [f32; 16]) -> TransformOp {
    TransformOp::Matrix3d(m)
}

/// Combines CSS transform operations into a single `transform` property string.
/// Equivalent to Remotion's `makeTransform(transforms)`.
pub fn make_transform(ops: &[TransformOp]) -> String {
    ops.iter()
        .map(|op| op.to_css())
        .collect::<Vec<_>>()
        .join(" ")
}

// ── interpolate_styles ────────────────────────────────────────────────────────

/// A CSS style value — string, pixel, percent, or unitless number.
#[derive(Debug, Clone, PartialEq)]
pub enum StyleValue {
    String(String),
    Px(f64),
    Percent(f64),
    Number(f64),
}

impl StyleValue {
    pub fn to_css_string(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Px(n) => format!("{n}px"),
            Self::Percent(n) => format!("{n}%"),
            Self::Number(n) => n.to_string(),
        }
    }
}

/// A keyframe style map: property name → `StyleValue`.
pub type StyleMap = std::collections::HashMap<String, StyleValue>;

/// Interpolates CSS style properties across keyframes.
/// Equivalent to Remotion's `interpolateStyles(input, inputRange, outputStylesRange)`.
pub fn interpolate_styles(input: f64, input_range: &[f64], output_styles: &[StyleMap]) -> StyleMap {
    assert!(
        input_range.len() >= 2,
        "input_range must have at least 2 entries"
    );
    assert_eq!(input_range.len(), output_styles.len());

    let clamped = input.clamp(input_range[0], *input_range.last().unwrap());
    let mut seg = input_range.len() - 2;
    for i in 0..input_range.len() - 1 {
        if clamped <= input_range[i + 1] {
            seg = i;
            break;
        }
    }

    let t0 = input_range[seg];
    let t1 = input_range[seg + 1];
    let t = if (t1 - t0).abs() < 1e-12 {
        1.0
    } else {
        (clamped - t0) / (t1 - t0)
    };

    let left = &output_styles[seg];
    let right = &output_styles[seg + 1];
    let mut result = StyleMap::new();
    let keys: std::collections::HashSet<&String> = left.keys().chain(right.keys()).collect();

    for key in keys {
        let lv = left.get(key);
        let rv = right.get(key);
        let v = match (lv, rv) {
            (Some(StyleValue::Number(a)), Some(StyleValue::Number(b))) => {
                StyleValue::Number(a + (b - a) * t)
            }
            (Some(StyleValue::Px(a)), Some(StyleValue::Px(b))) => StyleValue::Px(a + (b - a) * t),
            (Some(StyleValue::Percent(a)), Some(StyleValue::Percent(b))) => {
                StyleValue::Percent(a + (b - a) * t)
            }
            (Some(v), _) => v.clone(),
            (None, Some(v)) => v.clone(),
            (None, None) => continue,
        };
        result.insert(key.clone(), v);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_css() {
        assert_eq!(rotate(45.0).to_css(), "rotate(45deg)");
    }
    #[test]
    fn scale_css() {
        assert_eq!(scale(1.5).to_css(), "scale(1.5)");
    }
    #[test]
    fn translate_x_css() {
        assert_eq!(translate_x(100.0).to_css(), "translateX(100px)");
    }
    #[test]
    fn translate_css() {
        assert_eq!(translate(10.0, 20.0).to_css(), "translate(10px, 20px)");
    }
    #[test]
    fn skew_x_css() {
        assert_eq!(skew_x(15.0).to_css(), "skewX(15deg)");
    }
    #[test]
    fn perspective_css() {
        assert_eq!(perspective(800.0).to_css(), "perspective(800px)");
    }
    #[test]
    fn rotate3d_css() {
        assert_eq!(
            rotate3d(0.0, 1.0, 0.0, 45.0).to_css(),
            "rotate3d(0, 1, 0, 45deg)"
        );
    }
    #[test]
    fn translate3d_css() {
        assert_eq!(
            translate3d(10.0, 20.0, 30.0).to_css(),
            "translate3d(10px, 20px, 30px)"
        );
    }
    #[test]
    fn scale3d_css() {
        assert_eq!(scale3d(2.0, 1.0, 0.5).to_css(), "scale3d(2, 1, 0.5)");
    }
    #[test]
    fn matrix_css() {
        assert_eq!(
            matrix(1.0, 0.0, 0.0, 1.0, 50.0, 100.0).to_css(),
            "matrix(1, 0, 0, 1, 50, 100)"
        );
    }

    #[test]
    fn make_transform_empty() {
        assert_eq!(make_transform(&[]), "");
    }

    #[test]
    fn make_transform_chain() {
        assert_eq!(
            make_transform(&[rotate(45.0), scale(1.5), translate_x(100.0)]),
            "rotate(45deg) scale(1.5) translateX(100px)"
        );
    }

    #[test]
    fn make_transform_3d() {
        assert_eq!(
            make_transform(&[
                perspective(500.0),
                rotate_y(30.0),
                translate3d(0.0, 0.0, -100.0)
            ]),
            "perspective(500px) rotateY(30deg) translate3d(0px, 0px, -100px)"
        );
    }

    #[test]
    fn interpolate_styles_midpoint() {
        let kf0: StyleMap = [("opacity".to_string(), StyleValue::Number(0.0))].into();
        let kf1: StyleMap = [("opacity".to_string(), StyleValue::Number(1.0))].into();
        let r = interpolate_styles(15.0, &[0.0, 30.0], &[kf0, kf1]);
        if let Some(StyleValue::Number(v)) = r.get("opacity") {
            assert!((v - 0.5).abs() < 1e-9);
        } else {
            panic!("opacity missing");
        }
    }

    #[test]
    fn interpolate_styles_clamps() {
        let kf0: StyleMap = [("o".to_string(), StyleValue::Number(0.0))].into();
        let kf1: StyleMap = [("o".to_string(), StyleValue::Number(1.0))].into();
        let lo = interpolate_styles(-999.0, &[0.0, 30.0], &[kf0.clone(), kf1.clone()]);
        let hi = interpolate_styles(999.0, &[0.0, 30.0], &[kf0, kf1]);
        if let Some(StyleValue::Number(v)) = lo.get("o") {
            assert!((v - 0.0).abs() < 1e-9);
        }
        if let Some(StyleValue::Number(v)) = hi.get("o") {
            assert!((v - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn interpolate_styles_px() {
        let kf0: StyleMap = [("width".to_string(), StyleValue::Px(100.0))].into();
        let kf1: StyleMap = [("width".to_string(), StyleValue::Px(200.0))].into();
        let r = interpolate_styles(0.5, &[0.0, 1.0], &[kf0, kf1]);
        if let Some(StyleValue::Px(v)) = r.get("width") {
            assert!((v - 150.0).abs() < 1e-9);
        }
    }

    #[test]
    fn interpolate_styles_multi_segment() {
        let kf0: StyleMap = [("o".to_string(), StyleValue::Number(0.0))].into();
        let kf1: StyleMap = [("o".to_string(), StyleValue::Number(0.5))].into();
        let kf2: StyleMap = [("o".to_string(), StyleValue::Number(1.0))].into();
        let r = interpolate_styles(45.0, &[0.0, 30.0, 60.0], &[kf0, kf1, kf2]);
        if let Some(StyleValue::Number(v)) = r.get("o") {
            assert!((v - 0.75).abs() < 1e-9);
        }
    }

    #[test]
    fn style_value_css_strings() {
        assert_eq!(StyleValue::Number(1.0).to_css_string(), "1");
        assert_eq!(StyleValue::Px(32.5).to_css_string(), "32.5px");
        assert_eq!(StyleValue::Percent(50.0).to_css_string(), "50%");
        assert_eq!(StyleValue::String("auto".into()).to_css_string(), "auto");
    }
}
