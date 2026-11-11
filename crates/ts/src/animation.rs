//! Animation evaluation bridges matching Remotion 1:1.

use rust_motion_animation::{
    bezier, interpolate, interpolate_colors_range, measure_spring, random, spring_with_options,
    ExtrapolateType, InterpolateOptions, RandomSeed, SpringConfig, SpringOptions,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtrapolateMode {
    Extend,
    Clamp,
    Identity,
    Wrap,
}

impl From<ExtrapolateMode> for ExtrapolateType {
    fn from(m: ExtrapolateMode) -> Self {
        match m {
            ExtrapolateMode::Extend => ExtrapolateType::Extend,
            ExtrapolateMode::Clamp => ExtrapolateType::Clamp,
            ExtrapolateMode::Identity => ExtrapolateType::Identity,
            ExtrapolateMode::Wrap => ExtrapolateType::Wrap,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterpolateRequest {
    pub input: f64,
    pub input_range: Vec<f64>,
    pub output_range: Vec<f64>,
    #[serde(default)]
    pub extrapolate_left: Option<ExtrapolateMode>,
    #[serde(default)]
    pub extrapolate_right: Option<ExtrapolateMode>,
    #[serde(default)]
    pub easing_bezier: Option<[f64; 4]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpringConfigRequest {
    #[serde(default)]
    pub damping: Option<f64>,
    #[serde(default)]
    pub mass: Option<f64>,
    #[serde(default)]
    pub stiffness: Option<f64>,
    #[serde(default)]
    pub overshoot_clamping: Option<bool>,
}

impl From<SpringConfigRequest> for SpringConfig {
    fn from(req: SpringConfigRequest) -> Self {
        let def = SpringConfig::default();
        SpringConfig {
            damping: req.damping.unwrap_or(def.damping),
            mass: req.mass.unwrap_or(def.mass),
            stiffness: req.stiffness.unwrap_or(def.stiffness),
            overshoot_clamping: req.overshoot_clamping.unwrap_or(def.overshoot_clamping),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpringRequest {
    pub frame: f64,
    pub fps: f64,
    #[serde(default)]
    pub config: Option<SpringConfigRequest>,
    #[serde(default)]
    pub from: Option<f64>,
    #[serde(default)]
    pub to: Option<f64>,
    #[serde(default)]
    pub delay: Option<f64>,
    #[serde(default)]
    pub duration_in_frames: Option<f64>,
    #[serde(default)]
    pub duration_rest_threshold: Option<f64>,
    #[serde(default)]
    pub reverse: Option<bool>,
}

pub fn evaluate_interpolate(req: &InterpolateRequest) -> f64 {
    if req.input_range.len() < 2 || req.input_range.len() != req.output_range.len() {
        return req.input;
    }

    let left = req
        .extrapolate_left
        .map(Into::into)
        .unwrap_or(ExtrapolateType::Extend);
    let right = req
        .extrapolate_right
        .map(Into::into)
        .unwrap_or(ExtrapolateType::Extend);

    if let Some([x1, y1, x2, y2]) = req.easing_bezier {
        let b = bezier(x1, y1, x2, y2);
        let options = InterpolateOptions {
            easing: Some(&b),
            extrapolate_left: left,
            extrapolate_right: right,
        };
        interpolate(req.input, &req.input_range, &req.output_range, options)
    } else {
        let options = InterpolateOptions {
            easing: None,
            extrapolate_left: left,
            extrapolate_right: right,
        };
        interpolate(req.input, &req.input_range, &req.output_range, options)
    }
}

pub fn evaluate_spring(req: &SpringRequest) -> Result<f64, String> {
    let config = req.config.clone().map(Into::into).unwrap_or_default();

    let options = SpringOptions {
        from: req.from.unwrap_or(0.0),
        to: req.to.unwrap_or(1.0),
        delay: req.delay.unwrap_or(0.0),
        duration_in_frames: req.duration_in_frames,
        duration_rest_threshold: req.duration_rest_threshold.unwrap_or(0.005),
        reverse: req.reverse.unwrap_or(false),
    };

    spring_with_options(req.frame, req.fps, config, options)
        .map_err(|e| format!("Spring calculation error: {e:?}"))
}

pub fn evaluate_measure_spring(
    fps: f64,
    config: &SpringConfigRequest,
    threshold: f64,
) -> Result<f64, String> {
    let conf: SpringConfig = config.clone().into();
    measure_spring(fps, &conf, threshold).map_err(|e| format!("Measure spring error: {e:?}"))
}

pub fn evaluate_colors(
    input: f64,
    input_range: &[f64],
    output_range: &[String],
) -> Result<String, String> {
    if input_range.len() < 2 || input_range.len() != output_range.len() {
        return Err("Input range and output range must have matching length >= 2".to_string());
    }
    let str_refs: Vec<&str> = output_range.iter().map(AsRef::as_ref).collect();
    Ok(interpolate_colors_range(input, input_range, &str_refs))
}

pub fn evaluate_random(seed: &serde_json::Value) -> f64 {
    let r_seed = match seed {
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                RandomSeed::Num(f)
            } else {
                RandomSeed::Str(n.to_string())
            }
        }
        serde_json::Value::String(s) => RandomSeed::Str(s.clone()),
        other => RandomSeed::Str(other.to_string()),
    };
    random(r_seed)
}
