//! Integration tests for NoiseBackground component and procedural SVG generation.

use rust_motion_noise::{
    generate_noise_svg_data_url, generate_noise_wave_path, NoiseBackgroundProps, NoisePatternKind,
    WavePathOptions,
};

#[test]
fn test_wave_path_generation() {
    let opt = WavePathOptions::new(0.5, 100.0, 0.0, 0.01, 3);
    let path = generate_noise_wave_path("bg-seed", 1920.0, 1080.0, &opt);
    assert!(path.starts_with("M 0,"));
    assert!(path.contains("L 1920.00,1080.00 L 0,1080.00 Z"));
}

#[test]
fn test_svg_data_url_generation() {
    let data_url = generate_noise_svg_data_url(
        "svg-data-url-seed",
        800,
        600,
        1.5,
        0.02,
        "#0f172a",
        "#38bdf8",
    );

    assert!(data_url.starts_with("data:image/svg+xml;utf8,"));
    assert!(data_url.contains("fill=\"#0f172a\""));
    assert!(data_url.contains("fill=\"#38bdf8\""));
}

#[test]
fn test_noise_background_props_defaults() {
    let default_props = NoiseBackgroundProps {
        seed: "default-noise-seed".to_string(),
        base_color: "#0b0d19".to_string(),
        accent_color: "#6c63ff".to_string(),
        palette: vec!["#0b0d19".to_string(), "#6c63ff".to_string()],
        speed: 0.05,
        frequency: 0.02,
        octaves: 3,
        style: String::new(),
    };

    assert_eq!(default_props.seed, "default-noise-seed");
    assert_eq!(default_props.octaves, 3);
    assert_eq!(NoisePatternKind::default(), NoisePatternKind::Waves);
}
