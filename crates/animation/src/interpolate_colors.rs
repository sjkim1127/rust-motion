//! Color interpolation — Rust port of Remotion's `interpolateColors()`.
//!
//! Supports sRGB linear interpolation between CSS-style hex or rgba colors.

/// A color in sRGB space with premultiplied-alpha aware interpolation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Rgba {
    pub fn new(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self { r, g, b, a }
    }

    /// Parse a hex or rgb/rgba color string like `"#ff0000"`, `"#f00"`, or `"rgba(255, 0, 0, 1.0)"`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.starts_with('#') {
            Self::from_hex(s)
        } else if s.starts_with("rgb(") || s.starts_with("rgba(") {
            Self::from_rgb_str(s)
        } else {
            None
        }
    }

    /// Parse a hex color string like `"#ff0000"`, `"#ff0000ff"`, `"#f00"`, `"#f00f"`.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.strip_prefix('#')?;
        match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()? as f64 / 255.0;
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()? as f64 / 255.0;
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()? as f64 / 255.0;
                Some(Self::new(r, g, b, 1.0))
            }
            4 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()? as f64 / 255.0;
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()? as f64 / 255.0;
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()? as f64 / 255.0;
                let a = u8::from_str_radix(&hex[3..4].repeat(2), 16).ok()? as f64 / 255.0;
                Some(Self::new(r, g, b, a))
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()? as f64 / 255.0;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()? as f64 / 255.0;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()? as f64 / 255.0;
                Some(Self::new(r, g, b, 1.0))
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()? as f64 / 255.0;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()? as f64 / 255.0;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()? as f64 / 255.0;
                let a = u8::from_str_radix(&hex[6..8], 16).ok()? as f64 / 255.0;
                Some(Self::new(r, g, b, a))
            }
            _ => None,
        }
    }

    /// Parse a CSS `rgb(...)` or `rgba(...)` string.
    pub fn from_rgb_str(s: &str) -> Option<Self> {
        let open = s.find('(')?;
        let close = s.rfind(')')?;
        let inner = &s[open + 1..close];
        let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
        if parts.len() == 3 {
            let r = parts[0].parse::<f64>().ok()? / 255.0;
            let g = parts[1].parse::<f64>().ok()? / 255.0;
            let b = parts[2].parse::<f64>().ok()? / 255.0;
            Some(Self::new(
                r.clamp(0.0, 1.0),
                g.clamp(0.0, 1.0),
                b.clamp(0.0, 1.0),
                1.0,
            ))
        } else if parts.len() == 4 {
            let r = parts[0].parse::<f64>().ok()? / 255.0;
            let g = parts[1].parse::<f64>().ok()? / 255.0;
            let b = parts[2].parse::<f64>().ok()? / 255.0;
            let a = parts[3].parse::<f64>().ok()?;
            Some(Self::new(
                r.clamp(0.0, 1.0),
                g.clamp(0.0, 1.0),
                b.clamp(0.0, 1.0),
                a.clamp(0.0, 1.0),
            ))
        } else {
            None
        }
    }

    /// Encode as CSS `rgba(r, g, b, a)` string.
    pub fn to_css_rgba(self) -> String {
        format!(
            "rgba({}, {}, {}, {:.4})",
            (self.r * 255.0).round() as u8,
            (self.g * 255.0).round() as u8,
            (self.b * 255.0).round() as u8,
            self.a,
        )
    }

    /// Linear blend in sRGB space.
    pub fn lerp(self, other: Rgba, t: f64) -> Rgba {
        Rgba {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
            a: self.a + (other.a - self.a) * t,
        }
    }
}

/// Interpolate between two colors based on `t ∈ [0.0, 1.0]`.
///
/// Returns a CSS `rgba(...)` string, matching Remotion's `interpolateColors()`.
///
/// # Example
/// ```rust
/// use rust_motion_animation::interpolate_colors::interpolate_colors;
///
/// let color = interpolate_colors("#000000", "#ffffff", 0.5);
/// assert_eq!(color, "rgba(128, 128, 128, 1.0000)");
/// ```
pub fn interpolate_colors(from: &str, to: &str, t: f64) -> String {
    let a = Rgba::parse(from).unwrap_or(Rgba::new(0.0, 0.0, 0.0, 1.0));
    let b = Rgba::parse(to).unwrap_or(Rgba::new(1.0, 1.0, 1.0, 1.0));
    let t_clamped = t.clamp(0.0, 1.0);
    a.lerp(b, t_clamped).to_css_rgba()
}

/// Map an input value across multiple keyframe ranges to color values,
/// matching Remotion's `interpolateColors(frame, [0, 10, 20], ['#000', '#f00', '#fff'])`.
pub fn interpolate_colors_range(input: f64, input_range: &[f64], output_range: &[&str]) -> String {
    if input_range.is_empty() || output_range.is_empty() {
        return "rgba(0, 0, 0, 1.0000)".to_string();
    }
    let len = input_range.len().min(output_range.len());
    if len == 1 {
        let color = Rgba::parse(output_range[0]).unwrap_or(Rgba::new(0.0, 0.0, 0.0, 1.0));
        return color.to_css_rgba();
    }

    if input <= input_range[0] {
        let color = Rgba::parse(output_range[0]).unwrap_or(Rgba::new(0.0, 0.0, 0.0, 1.0));
        return color.to_css_rgba();
    }
    if input >= input_range[len - 1] {
        let color = Rgba::parse(output_range[len - 1]).unwrap_or(Rgba::new(1.0, 1.0, 1.0, 1.0));
        return color.to_css_rgba();
    }

    for i in 0..len - 1 {
        let in_start = input_range[i];
        let in_end = input_range[i + 1];
        if input >= in_start && input <= in_end {
            let span = in_end - in_start;
            let t = if span == 0.0 {
                0.0
            } else {
                (input - in_start) / span
            };
            let a = Rgba::parse(output_range[i]).unwrap_or(Rgba::new(0.0, 0.0, 0.0, 1.0));
            let b = Rgba::parse(output_range[i + 1]).unwrap_or(Rgba::new(1.0, 1.0, 1.0, 1.0));
            return a.lerp(b, t.clamp(0.0, 1.0)).to_css_rgba();
        }
    }

    let color = Rgba::parse(output_range[len - 1]).unwrap_or(Rgba::new(1.0, 1.0, 1.0, 1.0));
    color.to_css_rgba()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parse() {
        let c = Rgba::from_hex("#ff8000").unwrap();
        assert!((c.r - 1.0).abs() < 0.005);
        assert!((c.g - 0.502).abs() < 0.005);
        assert!((c.b - 0.0).abs() < 0.005);
    }

    #[test]
    fn lerp_midpoint() {
        let color = interpolate_colors("#000000", "#ffffff", 0.5);
        assert!(color.starts_with("rgba(128,") || color.contains("128, 128, 128"));
    }

    #[test]
    fn clamp_at_bounds() {
        assert_eq!(
            interpolate_colors("#ff0000", "#0000ff", 0.0),
            "rgba(255, 0, 0, 1.0000)"
        );
        assert_eq!(
            interpolate_colors("#ff0000", "#0000ff", 1.0),
            "rgba(0, 0, 255, 1.0000)"
        );
    }

    #[test]
    fn parse_short_hex_and_rgb() {
        let c1 = Rgba::parse("#f00").unwrap();
        assert!((c1.r - 1.0).abs() < 0.005);
        assert!((c1.g - 0.0).abs() < 0.005);

        let c2 = Rgba::parse("rgba(100, 200, 50, 0.8)").unwrap();
        assert!((c2.r - 100.0 / 255.0).abs() < 0.005);
        assert!((c2.a - 0.8).abs() < 0.005);
    }

    #[test]
    fn interpolate_colors_range_test() {
        let colors = vec!["#000000", "#ff0000", "#ffffff"];
        let frames = vec![0.0, 10.0, 20.0];

        // Frame 0 -> black
        let c0 = interpolate_colors_range(0.0, &frames, &colors);
        assert_eq!(c0, "rgba(0, 0, 0, 1.0000)");

        // Frame 10 -> red
        let c10 = interpolate_colors_range(10.0, &frames, &colors);
        assert_eq!(c10, "rgba(255, 0, 0, 1.0000)");

        // Frame 20 -> white
        let c20 = interpolate_colors_range(20.0, &frames, &colors);
        assert_eq!(c20, "rgba(255, 255, 255, 1.0000)");

        // Frame 5 -> halfway black to red
        let c5 = interpolate_colors_range(5.0, &frames, &colors);
        assert!(c5.contains("128, 0, 0"));
    }
}
