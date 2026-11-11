//! Remotion-compatible animation math and procedural noise bindings for WebAssembly.

use rust_motion_animation::{
    interpolate as rust_interpolate, measure_spring as rust_measure_spring,
    spring_with_options as rust_spring, ExtrapolateType, InterpolateOptions, SpringConfig,
    SpringOptions,
};
use rust_motion_noise::{fbm_2d, noise2d, noise3d, FbmOptions};
use wasm_bindgen::prelude::*;

fn parse_extrapolate(s: Option<&str>) -> ExtrapolateType {
    match s {
        Some("clamp") => ExtrapolateType::Clamp,
        Some("identity") => ExtrapolateType::Identity,
        _ => ExtrapolateType::Extend,
    }
}

/// Remotion-compatible `interpolate()` function in WebAssembly.
///
/// Maps a numeric value from an input range to an output range with optional clamping.
#[wasm_bindgen]
pub fn wasm_interpolate(
    input: f64,
    input_range: &[f64],
    output_range: &[f64],
    extrapolate_left: Option<String>,
    extrapolate_right: Option<String>,
) -> Result<f64, JsValue> {
    if input_range.len() != output_range.len() || input_range.len() < 2 {
        return Err(JsValue::from_str(
            "input_range and output_range must have equal length and at least 2 elements",
        ));
    }

    let options = InterpolateOptions {
        extrapolate_left: parse_extrapolate(extrapolate_left.as_deref()),
        extrapolate_right: parse_extrapolate(extrapolate_right.as_deref()),
        ..Default::default()
    };

    Ok(rust_interpolate(input, input_range, output_range, options))
}

/// Remotion-compatible `spring()` physics evaluation in WebAssembly.
///
/// Returns normalized progress (typically `0.0` to `1.0`, possibly overshooting based on damping).
#[wasm_bindgen]
pub fn wasm_spring(
    frame: f64,
    fps: f64,
    damping: Option<f64>,
    mass: Option<f64>,
    stiffness: Option<f64>,
    overshoot_clamping: Option<bool>,
) -> f64 {
    let mut config = SpringConfig::default();
    if let Some(d) = damping {
        config.damping = d;
    }
    if let Some(m) = mass {
        config.mass = m;
    }
    if let Some(s) = stiffness {
        config.stiffness = s;
    }
    if let Some(c) = overshoot_clamping {
        config.overshoot_clamping = c;
    }

    let options = SpringOptions::default();
    rust_spring(frame, if fps > 0.0 { fps } else { 30.0 }, config, options).unwrap_or(1.0)
}

/// Computes the exact frame count until a spring reaches physical rest.
#[wasm_bindgen]
pub fn wasm_measure_spring(
    fps: f64,
    damping: Option<f64>,
    mass: Option<f64>,
    stiffness: Option<f64>,
) -> u32 {
    let mut config = SpringConfig::default();
    if let Some(d) = damping {
        config.damping = d;
    }
    if let Some(m) = mass {
        config.mass = m;
    }
    if let Some(s) = stiffness {
        config.stiffness = s;
    }

    rust_measure_spring(if fps > 0.0 { fps } else { 30.0 }, &config, 0.005)
        .map(|f| f.round() as u32)
        .unwrap_or(30)
}

/// Procedural 2D Simplex Noise in WebAssembly.
#[wasm_bindgen]
pub fn wasm_noise2d(x: f64, y: f64) -> f64 {
    noise2d("wasm", x, y)
}

/// Procedural 3D Simplex Noise in WebAssembly.
#[wasm_bindgen]
pub fn wasm_noise3d(x: f64, y: f64, z: f64) -> f64 {
    noise3d("wasm", x, y, z)
}

/// Fractal Brownian Motion (fBm) in WebAssembly.
#[wasm_bindgen]
pub fn wasm_fbm2d(
    x: f64,
    y: f64,
    octaves: usize,
    lacunarity: Option<f64>,
    persistence: Option<f64>,
) -> f64 {
    let opts = FbmOptions {
        octaves: octaves.max(1),
        lacunarity: lacunarity.unwrap_or(2.0),
        persistence: persistence.unwrap_or(0.5),
    };
    fbm_2d("wasm", x, y, &opts)
}
