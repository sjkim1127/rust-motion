//! Media image fitting, scaling, and Pixmap conversion.

use crate::backend::RasterError;
use crate::scene::ImageFit;
use image::{imageops, RgbaImage};
use tiny_skia::{IntSize, Pixmap, PixmapPaint, Transform};

pub(crate) const MAX_IMAGE_NODE_PIXELS: u64 = 16 * 1024 * 1024;

pub(crate) fn rounded_dimension(value: f32) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else {
        value.round().clamp(1.0, u32::MAX as f32) as u32
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_media(
    pixmap: &mut Pixmap,
    source: &RgbaImage,
    src: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fit: ImageFit,
    opacity: f32,
    transform: Transform,
) -> Result<(), RasterError> {
    draw_media_with_filter(
        pixmap,
        source,
        src,
        x,
        y,
        w,
        h,
        fit,
        opacity,
        transform,
        imageops::FilterType::Lanczos3,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_video_media(
    pixmap: &mut Pixmap,
    source: &RgbaImage,
    src: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fit: ImageFit,
    opacity: f32,
    transform: Transform,
) -> Result<(), RasterError> {
    draw_media_with_filter(
        pixmap,
        source,
        src,
        x,
        y,
        w,
        h,
        fit,
        opacity,
        transform,
        imageops::FilterType::Triangle,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_media_with_filter(
    pixmap: &mut Pixmap,
    source: &RgbaImage,
    src: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fit: ImageFit,
    opacity: f32,
    transform: Transform,
    resize_filter: imageops::FilterType,
) -> Result<(), RasterError> {
    let width = rounded_dimension(w);
    let height = rounded_dimension(h);
    if width == 0 || height == 0 {
        return Ok(());
    }
    if u64::from(width) * u64::from(height) > MAX_IMAGE_NODE_PIXELS {
        return Err(RasterError::MediaAsset {
            path: src.into(),
            reason: format!(
                "destination {width}x{height} exceeds the {} pixel safety limit",
                MAX_IMAGE_NODE_PIXELS
            ),
        });
    }

    let fitted = fit_image_with_filter(source, width, height, fit, resize_filter);
    let media_pixmap = rgba_to_pixmap(fitted).ok_or_else(|| RasterError::MediaAsset {
        path: src.into(),
        reason: "media dimensions are too large for the rasterizer".into(),
    })?;
    let paint = PixmapPaint {
        opacity: opacity.clamp(0.0, 1.0),
        ..Default::default()
    };
    pixmap.draw_pixmap(
        x.round() as i32,
        y.round() as i32,
        media_pixmap.as_ref(),
        &paint,
        transform,
        None,
    );
    Ok(())
}

pub(crate) fn fit_image_with_filter(
    source: &RgbaImage,
    width: u32,
    height: u32,
    fit: ImageFit,
    resize_filter: imageops::FilterType,
) -> RgbaImage {
    let mut output = RgbaImage::new(width, height);
    if source.width() == 0 || source.height() == 0 || width == 0 || height == 0 {
        return output;
    }

    let effective_fit = match fit {
        ImageFit::ScaleDown if source.width() <= width && source.height() <= height => {
            ImageFit::None
        }
        ImageFit::ScaleDown => ImageFit::Contain,
        other => other,
    };

    match effective_fit {
        ImageFit::Fill => imageops::resize(source, width, height, resize_filter),
        ImageFit::Cover => {
            let scale =
                (width as f64 / source.width() as f64).max(height as f64 / source.height() as f64);
            let scaled_width = ((source.width() as f64 * scale).ceil() as u32).max(width);
            let scaled_height = ((source.height() as f64 * scale).ceil() as u32).max(height);
            let resized = imageops::resize(source, scaled_width, scaled_height, resize_filter);
            let crop_x = (scaled_width - width) / 2;
            let crop_y = (scaled_height - height) / 2;
            imageops::crop_imm(&resized, crop_x, crop_y, width, height).to_image()
        }
        ImageFit::Contain => {
            let scale =
                (width as f64 / source.width() as f64).min(height as f64 / source.height() as f64);
            let scaled_width = ((source.width() as f64 * scale).round() as u32).clamp(1, width);
            let scaled_height = ((source.height() as f64 * scale).round() as u32).clamp(1, height);
            let resized = imageops::resize(source, scaled_width, scaled_height, resize_filter);
            imageops::overlay(
                &mut output,
                &resized,
                ((width - scaled_width) / 2) as i64,
                ((height - scaled_height) / 2) as i64,
            );
            output
        }
        ImageFit::None => {
            imageops::overlay(
                &mut output,
                source,
                (width as i64 - source.width() as i64) / 2,
                (height as i64 - source.height() as i64) / 2,
            );
            output
        }
        ImageFit::ScaleDown => unreachable!("scale-down is normalized above"),
    }
}

pub(crate) fn rgba_to_pixmap(image: RgbaImage) -> Option<Pixmap> {
    let size = IntSize::from_wh(image.width(), image.height())?;
    let mut data = image.into_raw();
    for pixel in data.chunks_exact_mut(4) {
        let alpha = pixel[3] as u16;
        pixel[0] = ((pixel[0] as u16 * alpha + 127) / 255) as u8;
        pixel[1] = ((pixel[1] as u16 * alpha + 127) / 255) as u8;
        pixel[2] = ((pixel[2] as u16 * alpha + 127) / 255) as u8;
    }
    Pixmap::from_vec(data, size)
}
