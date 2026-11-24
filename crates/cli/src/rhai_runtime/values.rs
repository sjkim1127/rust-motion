//! Argument validation and parsing helpers for script-provided values.

use super::*;

pub(super) fn finite_f32(name: &str, value: FLOAT) -> RhaiResult<f32> {
    if !value.is_finite() || value < f32::MIN as FLOAT || value > f32::MAX as FLOAT {
        return Err(runtime_error(format!(
            "{name} must be a finite 32-bit number"
        )));
    }
    Ok(value as f32)
}

pub(super) fn non_negative_f32(name: &str, value: FLOAT) -> RhaiResult<f32> {
    let value = finite_f32(name, value)?;
    if value < 0.0 {
        return Err(runtime_error(format!("{name} must not be negative")));
    }
    Ok(value)
}

pub(super) fn finite_f64(name: &str, value: FLOAT) -> RhaiResult<f64> {
    if !value.is_finite() {
        return Err(runtime_error(format!("{name} must be finite")));
    }
    Ok(value)
}

pub(super) fn non_negative_f64(name: &str, value: FLOAT) -> RhaiResult<f64> {
    let value = finite_f64(name, value)?;
    if value < 0.0 {
        return Err(runtime_error(format!("{name} must not be negative")));
    }
    Ok(value)
}

pub(super) fn unit_f32(name: &str, value: FLOAT) -> RhaiResult<f32> {
    let value = finite_f32(name, value)?;
    if !(0.0..=1.0).contains(&value) {
        return Err(runtime_error(format!("{name} must be between 0.0 and 1.0")));
    }
    Ok(value)
}

pub(super) fn validate_media_source(value: &str) -> RhaiResult<()> {
    if value.trim().is_empty() {
        Err(runtime_error("media source path must not be empty".into()))
    } else {
        Ok(())
    }
}

pub(super) fn parse_color(value: &str) -> RhaiResult<Color> {
    Color::from_css(value).ok_or_else(|| {
        runtime_error(format!(
            "invalid color '{value}'; expected CSS color (#rrggbb, #rrggbbaa, rgba(...), or name)"
        ))
    })
}

pub(super) fn parse_image_fit(value: &str) -> RhaiResult<ImageFit> {
    match value {
        "cover" => Ok(ImageFit::Cover),
        "contain" => Ok(ImageFit::Contain),
        "fill" => Ok(ImageFit::Fill),
        "none" => Ok(ImageFit::None),
        "scale-down" => Ok(ImageFit::ScaleDown),
        _ => Err(runtime_error(format!(
            "invalid image fit '{value}'; expected cover, contain, fill, none, or scale-down"
        ))),
    }
}

pub(super) fn runtime_error(message: String) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into()
}
