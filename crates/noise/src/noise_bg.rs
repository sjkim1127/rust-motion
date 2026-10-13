//! Dioxus component and utilities for procedural organic noise background and SVG pattern rendering.

use dioxus::prelude::*;
use rust_motion_core::hooks::use_current_frame;

use crate::fbm::{fbm_3d, FbmOptions};
use crate::seed::NoiseSeed;

/// Pattern style rendered by `<NoiseBackground />`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum NoisePatternKind {
    /// Layered flowing organic liquid waves.
    #[default]
    Waves,
    /// Multi-contour topographic noise lines.
    Contours,
    /// Radial pulsating noise aura.
    RadialAura,
    /// Floating procedural organic blobs.
    Blobs,
    /// Multi-frequency simplex noise gradient mesh.
    Mesh,
}

/// Configuration options for procedural wave path generation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WavePathOptions {
    /// Normalized baseline Y position `[0.0, 1.0]`.
    pub base_y_ratio: f64,
    /// Wave displacement amplitude in pixels.
    pub amplitude: f64,
    /// Temporal offset / frame progression.
    pub time: f64,
    /// Spatial noise frequency.
    pub freq: f64,
    /// Harmonic octaves.
    pub octaves: usize,
}

impl Default for WavePathOptions {
    fn default() -> Self {
        Self {
            base_y_ratio: 0.5,
            amplitude: 50.0,
            time: 0.0,
            freq: 0.02,
            octaves: 3,
        }
    }
}

impl WavePathOptions {
    /// Creates a new `WavePathOptions` with custom parameters.
    pub fn new(base_y_ratio: f64, amplitude: f64, time: f64, freq: f64, octaves: usize) -> Self {
        Self {
            base_y_ratio,
            amplitude,
            time,
            freq,
            octaves,
        }
    }
}

/// Props for the `<NoiseBackground>` component.
#[derive(Props, Clone, PartialEq)]
pub struct NoiseBackgroundProps {
    /// Seed identifier for deterministic noise generation.
    #[props(default = "default-noise-seed".to_string())]
    pub seed: String,
    /// Primary background color (e.g. `"#0b0d19"`).
    #[props(default = "#0b0d19".to_string())]
    pub base_color: String,
    /// Primary accent color (e.g. `"#6c63ff"`).
    #[props(default = "#6c63ff".to_string())]
    pub accent_color: String,
    /// Additional color palette for multi-tone gradient layers.
    #[props(default = vec!["#0b0d19".to_string(), "#3b82f6".to_string(), "#8b5cf6".to_string(), "#ec4899".to_string()])]
    pub palette: Vec<String>,
    /// Evolution speed over animation frames.
    #[props(default = 0.03)]
    pub speed: f64,
    /// Spatial frequency of the procedural noise.
    #[props(default = 0.02)]
    pub frequency: f64,
    /// Number of noise octaves in multi-harmonic synthesis.
    #[props(default = 3)]
    pub octaves: usize,
    /// Additional inline CSS styles.
    #[props(default)]
    pub style: String,
}

/// Generates an SVG path string `d` representing an organic noise wave contour.
pub fn generate_noise_wave_path(
    seed: impl Into<NoiseSeed>,
    width: f64,
    height: f64,
    options: &WavePathOptions,
) -> String {
    let s = seed.into();
    let opts = FbmOptions::new(options.octaves, 2.0, 0.5);
    let steps = 24;
    let dx = width / steps as f64;
    let base_y = height * options.base_y_ratio;

    let mut path = format!("M 0,{:.2}", base_y);

    for i in 0..=steps {
        let x = i as f64 * dx;
        let nx = x * options.freq;
        let n = fbm_3d(
            s.clone(),
            nx,
            options.base_y_ratio * 10.0,
            options.time,
            &opts,
        );
        let y = (base_y + n * options.amplitude).clamp(0.0, height);
        path.push_str(&format!(" L {:.2},{:.2}", x, y));
    }

    path.push_str(&format!(
        " L {:.2},{:.2} L 0,{:.2} Z",
        width, height, height
    ));
    path
}

/// Generates an SVG data URL embedding procedural multi-layer noise contours.
pub fn generate_noise_svg_data_url(
    seed: &str,
    width: u32,
    height: u32,
    time: f64,
    freq: f64,
    base_color: &str,
    accent_color: &str,
) -> String {
    let w = width as f64;
    let h = height as f64;
    let opt1 = WavePathOptions::new(0.4, h * 0.2, time, freq, 3);
    let opt2 = WavePathOptions::new(0.65, h * 0.25, time * 0.8, freq * 1.5, 3);
    let path1 = generate_noise_wave_path(seed, w, h, &opt1);
    let path2 = generate_noise_wave_path(format!("{}-2", seed), w, h, &opt2);

    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}"><rect width="100%" height="100%" fill="{base_color}"/><path d="{path1}" fill="{accent_color}" opacity="0.35"/><path d="{path2}" fill="{accent_color}" opacity="0.55"/></svg>"#
    );

    format!("data:image/svg+xml;utf8,{}", svg)
}

/// Dioxus component rendering animated procedural SVG patterns and organic shader backgrounds.
#[component]
pub fn NoiseBackground(props: NoiseBackgroundProps) -> Element {
    let frame = use_current_frame();
    let time = frame as f64 * props.speed;

    // Evaluate dynamic procedural coordinates
    let fbm_opts = FbmOptions::new(props.octaves, 2.0, 0.5);
    let n1 = fbm_3d(&props.seed, 0.5, 0.5, time, &fbm_opts);
    let n2 = fbm_3d(&props.seed, 1.5, 1.5, time * 0.85, &fbm_opts);

    let opt1 = WavePathOptions::new(0.5 + n1 * 0.1, 180.0, time, props.frequency, props.octaves);
    let wave_d1 = generate_noise_wave_path(&props.seed, 1000.0, 1000.0, &opt1);

    let opt2 = WavePathOptions::new(
        0.7 + n2 * 0.08,
        140.0,
        time * 1.2,
        props.frequency * 1.4,
        props.octaves,
    );
    let wave_d2 = generate_noise_wave_path(format!("{}-layer2", props.seed), 1000.0, 1000.0, &opt2);

    let opacity = ((n1 + 1.0) / 2.0 * 0.4 + 0.3).clamp(0.1, 0.9);
    let blur_radius = (25.0 + n2 * 12.0).max(5.0);

    let container_style = format!(
        "position: absolute; top: 0; left: 0; width: 100%; height: 100%; \
         background-color: {}; overflow: hidden; {};",
        props.base_color, props.style
    );

    let glow_style = format!(
        "position: absolute; width: 120%; height: 120%; top: -10%; left: -10%; \
         background: radial-gradient(circle at {:.1}% {:.1}%, {} 0%, transparent 65%); \
         opacity: {:.3}; filter: blur({:.1}px); pointer-events: none;",
        50.0 + n1 * 25.0,
        50.0 + n2 * 25.0,
        props.accent_color,
        opacity,
        blur_radius
    );

    let secondary_color = props
        .palette
        .get(1)
        .cloned()
        .unwrap_or_else(|| props.accent_color.clone());

    rsx! {
        div {
            style: "{container_style}",
            div {
                style: "{glow_style}",
            }
            svg {
                style: "position: absolute; top: 0; left: 0; width: 100%; height: 100%; pointer-events: none;",
                view_box: "0 0 1000 1000",
                preserve_aspect_ratio: "none",
                path {
                    d: "{wave_d1}",
                    fill: "{props.accent_color}",
                    opacity: "0.4"
                }
                path {
                    d: "{wave_d2}",
                    fill: "{secondary_color}",
                    opacity: "0.55"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_noise_wave_path() {
        let opt = WavePathOptions::new(0.5, 50.0, 1.0, 0.05, 3);
        let path = generate_noise_wave_path("wave-test", 800.0, 600.0, &opt);
        assert!(path.starts_with("M 0,"));
        assert!(path.ends_with("Z"));
    }

    #[test]
    fn test_generate_noise_svg_data_url() {
        let url =
            generate_noise_svg_data_url("svg-test", 400, 300, 0.5, 0.02, "#000000", "#ffffff");
        assert!(url.starts_with("data:image/svg+xml;utf8,<svg"));
        assert!(url.contains("viewBox=\"0 0 400 300\""));
    }
}
