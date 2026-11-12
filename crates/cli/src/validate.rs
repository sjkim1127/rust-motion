//! Input validation for project files, scripts, compositions, and render options.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::args::{RenderCodec, RenderRequest, ValidationError};
use crate::composition::built_in_registry;
use rust_motion_project::Project;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationReport {
    pub ok: bool,
    pub target_kind: String,
    pub target: String,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub details: serde_json::Value,
}

/// Validate inputs (project file, Rhai script, or composition) before rendering.
pub fn run_validate(
    project_path: Option<&std::path::Path>,
    composition: Option<&str>,
    script_path: Option<&std::path::Path>,
    props_path: Option<&std::path::Path>,
) -> ValidationReport {
    let mut errors = Vec::new();
    let warnings = Vec::new();
    let mut details = serde_json::json!({});

    if let Some(path) = project_path {
        let target_kind = "project".to_string();
        let target = path.display().to_string();
        match Project::load(path) {
            Ok(project) => {
                if let Some(parent) = path.parent() {
                    if let Err(e) = project.validate_asset_files(parent) {
                        errors.push(format!("Asset validation error: {e}"));
                    }
                }
                if project.settings.duration == 0 {
                    errors.push("Project settings duration must be greater than 0".into());
                }
                if project.settings.fps <= 0.0 {
                    errors.push("Project settings fps must be greater than 0".into());
                }
                if project.settings.width == 0 || project.settings.height == 0 {
                    errors.push("Project settings dimensions must be greater than 0".into());
                }
                details = serde_json::json!({
                    "version": project.version,
                    "composition": project.composition,
                    "width": project.settings.width,
                    "height": project.settings.height,
                    "fps": project.settings.fps,
                    "duration": project.settings.duration,
                    "tracks_count": project.tracks.len(),
                    "assets_count": project.assets.len(),
                    "events_count": project.events.len(),
                });
            }
            Err(e) => {
                errors.push(format!("Project parse error: {e}"));
            }
        }
        let ok = errors.is_empty();
        return ValidationReport {
            ok,
            target_kind,
            target,
            errors,
            warnings,
            details,
        };
    }

    if let Some(path) = script_path {
        let target_kind = "script".to_string();
        let target = path.display().to_string();
        if !path.exists() {
            errors.push(format!("Script file does not exist: {target}"));
        } else {
            #[cfg(feature = "rhai")]
            {
                let engine = rhai::Engine::new();
                if let Err(e) = engine.compile_file(path.to_path_buf()) {
                    errors.push(format!("Script syntax error: {e}"));
                } else {
                    details = serde_json::json!({ "syntax_valid": true });
                }
            }
        }
        if let Some(p) = props_path {
            match std::fs::read_to_string(p) {
                Ok(content) => {
                    if let Err(e) = serde_json::from_str::<serde_json::Value>(&content) {
                        errors.push(format!("Props JSON error: {e}"));
                    }
                }
                Err(e) => errors.push(format!("Failed to read props file: {e}")),
            }
        }
        let ok = errors.is_empty();
        return ValidationReport {
            ok,
            target_kind,
            target,
            errors,
            warnings,
            details,
        };
    }

    if let Some(comp_id) = composition {
        let target_kind = "composition".to_string();
        let target = comp_id.to_string();
        let registry = built_in_registry();
        if registry.get(comp_id).is_err() {
            errors.push(format!(
                "Composition '{}' not found in built-in registry",
                comp_id
            ));
        } else {
            details = serde_json::json!({ "registered": true });
        }
        if let Some(p) = props_path {
            match std::fs::read_to_string(p) {
                Ok(content) => {
                    if let Err(e) = serde_json::from_str::<serde_json::Value>(&content) {
                        errors.push(format!("Props JSON error: {e}"));
                    }
                }
                Err(e) => errors.push(format!("Failed to read props file: {e}")),
            }
        }
        let ok = errors.is_empty();
        return ValidationReport {
            ok,
            target_kind,
            target,
            errors,
            warnings,
            details,
        };
    }

    ValidationReport {
        ok: false,
        target_kind: "none".into(),
        target: "none".into(),
        errors: vec![
            "No target specified for validation (specify --project, --script, or --composition)"
                .into(),
        ],
        warnings: vec![],
        details: serde_json::json!({}),
    }
}

/// Validates that a render request selects exactly one available composition source.
pub fn validate_composition_source(
    composition: Option<&str>,
    script: Option<&PathBuf>,
) -> Result<(), ValidationError> {
    match (composition, script) {
        (None, None) => Err(ValidationError::MissingCompositionSource),
        (Some(_), Some(_)) => Err(ValidationError::ConflictingCompositionSources),
        (Some(composition), None) if composition.trim().is_empty() => {
            Err(ValidationError::EmptyComposition)
        }
        (Some(_), None) => Ok(()),
        (None, Some(path)) if !path.is_file() => {
            Err(ValidationError::ScriptFileNotFound(path.clone()))
        }
        (None, Some(_)) => Ok(()),
    }
}

/// Validates command-line parameters prior to launching the renderer.
pub fn validate_render_params(
    composition: &str,
    props: Option<&PathBuf>,
    width: u32,
    height: u32,
    fps: f64,
    duration: u32,
) -> Result<(), ValidationError> {
    validate_render_params_for_codec(
        composition,
        props,
        width,
        height,
        fps,
        duration,
        RenderCodec::H264,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn validate_render_params_for_codec(
    composition: &str,
    props: Option<&PathBuf>,
    width: u32,
    height: u32,
    fps: f64,
    duration: u32,
    codec: RenderCodec,
) -> Result<(), ValidationError> {
    if composition.trim().is_empty() {
        return Err(ValidationError::EmptyComposition);
    }
    if let Some(path) = props {
        if !path.is_file() {
            return Err(ValidationError::PropsFileNotFound(path.clone()));
        }
    }
    if width == 0 || height == 0 {
        return Err(ValidationError::InvalidZeroResolution(width, height));
    }
    let requires_even_dimensions = matches!(
        codec,
        RenderCodec::H264
            | RenderCodec::H265
            | RenderCodec::Vp9
            | RenderCodec::Av1
            | RenderCodec::ProRes
    );
    if requires_even_dimensions && (!width.is_multiple_of(2) || !height.is_multiple_of(2)) {
        return Err(ValidationError::InvalidOddResolution(width, height));
    }
    if !fps.is_finite() || fps <= 0.0 {
        return Err(ValidationError::InvalidFps(fps.to_string()));
    }
    if duration == 0 {
        return Err(ValidationError::InvalidDuration(duration));
    }
    Ok(())
}

pub(crate) fn validate_render_options(
    request: &RenderRequest,
) -> Result<(u32, u32), ValidationError> {
    if !request.scale.is_finite() || request.scale <= 0.0 {
        return Err(ValidationError::InvalidScale(request.scale.to_string()));
    }
    if request.frame_step == 0 {
        return Err(ValidationError::InvalidFrameStep(request.frame_step));
    }
    if request.concurrency == Some(0) {
        return Err(ValidationError::InvalidConcurrency);
    }
    if request.frame_step > 1 && request.codec.still_format().is_some() {
        return Err(ValidationError::InvalidFrameStep(request.frame_step));
    }
    let end = request.frame_end.unwrap_or_else(|| {
        if request.codec.still_format().is_some() {
            request.frame_start
        } else {
            request.duration.saturating_sub(1)
        }
    });
    if request.frame_start > end || end >= request.duration {
        return Err(ValidationError::InvalidFrameRange {
            start: request.frame_start,
            end,
            duration: request.duration,
        });
    }
    if request.timeout_seconds == Some(0) {
        return Err(ValidationError::InvalidTimeout);
    }
    if request.codec.still_format().is_some() && end != request.frame_start {
        return Err(ValidationError::InvalidFrameRange {
            start: request.frame_start,
            end,
            duration: request.duration,
        });
    }
    let max_crf = match request.codec {
        RenderCodec::H264 | RenderCodec::H265 => Some(51),
        RenderCodec::Vp9 | RenderCodec::Av1 => Some(63),
        RenderCodec::ProRes
        | RenderCodec::Gif
        | RenderCodec::Png
        | RenderCodec::Jpeg
        | RenderCodec::Webp => None,
    };
    if max_crf.is_some_and(|max| request.crf > max) {
        return Err(ValidationError::InvalidCrf {
            codec: format!("{:?}", request.codec),
            value: request.crf,
            range: format!("0..={}", max_crf.expect("checked above")),
        });
    }
    if matches!(request.codec, RenderCodec::H264 | RenderCodec::H265)
        && !matches!(
            request.preset.as_str(),
            "ultrafast"
                | "superfast"
                | "veryfast"
                | "faster"
                | "fast"
                | "medium"
                | "slow"
                | "slower"
                | "veryslow"
                | "placebo"
        )
    {
        return Err(ValidationError::InvalidPreset(request.preset.clone()));
    }
    let actual = request
        .output
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let expected = request.codec.extensions();
    if !expected.contains(&actual.as_str()) {
        return Err(ValidationError::InvalidOutputExtension {
            codec: format!("{:?}", request.codec),
            actual,
            expected: expected.join(" or ."),
        });
    }
    if !request.audio.is_empty()
        && (request.codec.still_format().is_some() || request.codec == RenderCodec::Gif)
    {
        return Err(ValidationError::AudioNotSupported(format!(
            "{:?}",
            request.codec
        )));
    }
    Ok((request.frame_start, end))
}
