//! Single-frame still image rendering, optionally scaled.

use super::*;

pub(super) fn scaled_dimensions(
    width: u32,
    height: u32,
    scale: f64,
) -> Result<(u32, u32), RasterError> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(RasterError::Init(
            "render scale must be finite and positive".into(),
        ));
    }
    let scaled_width = (width as f64 * scale).round();
    let scaled_height = (height as f64 * scale).round();
    if scaled_width < 1.0
        || scaled_height < 1.0
        || scaled_width > u32::MAX as f64
        || scaled_height > u32::MAX as f64
    {
        return Err(RasterError::Init(
            "render scale produces dimensions outside the supported range".into(),
        ));
    }
    Ok((scaled_width as u32, scaled_height as u32))
}

/// Render one composition frame directly to PNG, JPEG, or WebP.
#[allow(clippy::too_many_arguments)]
pub fn render_still_fallible<F, B, E>(
    backend: &B,
    width: u32,
    height: u32,
    fps: f64,
    frame: u32,
    output: &Path,
    format: StillImageFormat,
    control: &RenderControl,
    scene_fn: F,
) -> Result<(), RasterError>
where
    B: RasterizerBackend + Send + Sync,
    F: FnOnce(u32) -> Result<Scene, E>,
    E: std::fmt::Display,
{
    render_still_fallible_scaled(
        backend, width, height, fps, frame, output, format, control, 1.0, scene_fn,
    )
}

/// Render one composition frame with a post-rasterization output scale.
#[allow(clippy::too_many_arguments)]
pub fn render_still_fallible_scaled<F, B, E>(
    backend: &B,
    width: u32,
    height: u32,
    fps: f64,
    frame: u32,
    output: &Path,
    format: StillImageFormat,
    control: &RenderControl,
    scale: f64,
    scene_fn: F,
) -> Result<(), RasterError>
where
    B: RasterizerBackend + Send + Sync,
    F: FnOnce(u32) -> Result<Scene, E>,
    E: std::fmt::Display,
{
    let started = Instant::now();
    control.check(started)?;
    let scene = scene_fn(frame).map_err(|error| RasterError::Frame {
        frame,
        reason: error.to_string(),
    })?;
    let image = backend.render_frame(&scene, &FrameConfig::new(width, height, frame, fps))?;
    let (output_width, output_height) = scaled_dimensions(width, height, scale)?;
    let image = if (output_width, output_height) == (width, height) {
        image
    } else {
        image::imageops::resize(
            &image,
            output_width,
            output_height,
            image::imageops::FilterType::Lanczos3,
        )
    };
    control.check(started)?;
    match format {
        StillImageFormat::Png => image
            .save_with_format(output, image::ImageFormat::Png)
            .map_err(|error| RasterError::ImageEncode(error.to_string()))?,
        StillImageFormat::Jpeg => image::DynamicImage::ImageRgba8(image)
            .to_rgb8()
            .save_with_format(output, image::ImageFormat::Jpeg)
            .map_err(|error| RasterError::ImageEncode(error.to_string()))?,
        StillImageFormat::WebP => image
            .save_with_format(output, image::ImageFormat::WebP)
            .map_err(|error| RasterError::ImageEncode(error.to_string()))?,
    }
    if let Some(callback) = &control.progress {
        callback(RenderProgress {
            completed_frames: 1,
            total_frames: 1,
            frame,
        });
    }
    Ok(())
}
