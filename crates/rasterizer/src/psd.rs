//! PSD/PSB file parsing and rendering into Native Motion Scenes.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use image::{ImageBuffer, Rgba};
use photocraft_psd::PsdFile;

use crate::backend::{FrameConfig, RasterError, RasterizerBackend};
use crate::scene::{
    BlendMode, Color, ImageFit, MaskMode, Scene, SceneFilter, SceneNode,
};

/// Encode raw RGBA8 pixels into a PNG data URI.
pub fn rgba_to_png_data_uri(width: u32, height: u32, rgba: &[u8]) -> Result<String, RasterError> {
    if width == 0 || height == 0 {
        return Err(RasterError::Scene("zero width or height".into()));
    }
    let expected = (width as usize) * (height as usize) * 4;
    if rgba.len() < expected {
        return Err(RasterError::Scene("insufficient RGBA pixel data".into()));
    }
    let img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_raw(width, height, rgba[..expected].to_vec())
        .ok_or_else(|| RasterError::Scene("invalid image dimensions".into()))?;
    let mut bytes = std::io::Cursor::new(Vec::new());
    img.write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|e| RasterError::Scene(format!("failed to encode PNG: {e}")))?;
    let b64 = BASE64.encode(bytes.into_inner());
    Ok(format!("data:image/png;base64,{b64}"))
}

/// Convert a PSD or PSB file into a compositable `(Scene, width, height)`.
pub fn load_psd_as_scene(data: &[u8]) -> Result<(Scene, u32, u32), RasterError> {
    let psd = PsdFile::from_bytes(data)
        .map_err(|e| RasterError::Scene(format!("failed to parse PSD: {e}")))?;

    let canvas_width = psd.header.width;
    let canvas_height = psd.header.height;
    let mut nodes = Vec::new();

    // Iterate through layers
    for layer in psd.iter_layers() {
        if !layer.record.is_visible() {
            continue;
        }

        let blend_mode = BlendMode::from(layer.record.blend_mode);
        let opacity = layer.record.opacity as f32 / 255.0;

        // Check if layer is an adjustment layer
        if let Some(adj) = layer.record.adjustment(psd.header.color_mode) {
            let filter = SceneFilter::Adjustment(Box::new(adj));
            let (x, y, w, h) = if layer.record.rect.width() > 0 && layer.record.rect.height() > 0 {
                (
                    layer.record.rect.left as f32,
                    layer.record.rect.top as f32,
                    layer.record.rect.width() as f32,
                    layer.record.rect.height() as f32,
                )
            } else {
                (0.0, 0.0, canvas_width as f32, canvas_height as f32)
            };

            let rect_node = SceneNode::Rect {
                x,
                y,
                w,
                h,
                fill: Color::TRANSPARENT,
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            };

            nodes.push(SceneNode::Layer {
                opacity,
                blend_mode,
                clip: None,
                mask: None,
                mask_mode: MaskMode::Alpha,
                filters: vec![filter],
                shadow: None,
                children: vec![rect_node],
            });
        } else {
            // Normal raster pixel layer
            let w = layer.record.rect.width() as u32;
            let h = layer.record.rect.height() as u32;
            if w > 0 && h > 0 {
                if let Ok(rgba8) = layer.rgba8() {
                    let uri = rgba_to_png_data_uri(w, h, &rgba8.data)?;
                    let img_node = SceneNode::Image {
                        src: uri,
                        x: layer.record.rect.left as f32,
                        y: layer.record.rect.top as f32,
                        w: w as f32,
                        h: h as f32,
                        fit: ImageFit::Fill,
                        opacity: 1.0,
                    };
                    nodes.push(SceneNode::Layer {
                        opacity,
                        blend_mode,
                        clip: None,
                        mask: None,
                        mask_mode: MaskMode::Alpha,
                        filters: Vec::new(),
                        shadow: None,
                        children: vec![img_node],
                    });
                }
            }
        }
    }

    Ok((Scene { nodes }, canvas_width, canvas_height))
}

/// Parse and render a PSD file directly into an `image::RgbaImage`.
pub fn render_psd_to_image(data: &[u8]) -> Result<image::RgbaImage, RasterError> {
    let (scene, width, height) = load_psd_as_scene(data)?;
    let backend = crate::tiny_skia_backend::TinySkiaBackend::new();
    let config = FrameConfig {
        width,
        height,
        frame: 0,
        fps: 30.0,
    };
    backend.render_frame(&scene, &config)
}

/// Parse and render a PSD file directly into a `tiny_skia::Pixmap`.
pub fn render_psd_to_pixmap(data: &[u8]) -> Result<tiny_skia::Pixmap, RasterError> {
    let img = render_psd_to_image(data)?;
    let mut pixmap = tiny_skia::Pixmap::new(img.width(), img.height())
        .ok_or_else(|| RasterError::Scene("failed to allocate Pixmap".into()))?;
    let p_data = pixmap.data_mut();
    let raw = img.as_raw();
    for (dst, src) in p_data.chunks_exact_mut(4).zip(raw.chunks_exact(4)) {
        let a = src[3] as f32 / 255.0;
        dst[0] = (src[0] as f32 * a + 0.5) as u8;
        dst[1] = (src[1] as f32 * a + 0.5) as u8;
        dst[2] = (src[2] as f32 * a + 0.5) as u8;
        dst[3] = src[3];
    }
    Ok(pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_psd::builder::{LayerSpec, PixelData, PsdBuilder};
    use photocraft_psd::blend::BlendMode as PsdBlendMode;

    #[test]
    fn test_psd_builder_render_with_blend_modes() {
        let mut builder = PsdBuilder::new(100, 100);

        // Backdrop: solid red
        let red_pixels = vec![255, 0, 0, 255].repeat(100 * 100);
        let mut l1 = LayerSpec::new("Background", 0, 0, 100, 100, PixelData::Rgba8(red_pixels));
        l1.blend_mode = PsdBlendMode::Normal;
        builder.push_layer(l1);

        // Top layer: solid green with Multiply
        let green_pixels = vec![0, 255, 0, 255].repeat(100 * 100);
        let mut l2 = LayerSpec::new("Multiply Layer", 0, 0, 100, 100, PixelData::Rgba8(green_pixels));
        l2.blend_mode = PsdBlendMode::Multiply;
        builder.push_layer(l2);

        let psd_bytes = builder.to_bytes().expect("build PSD");
        let pixmap = render_psd_to_pixmap(&psd_bytes).expect("render psd");

        assert_eq!(pixmap.width(), 100);
        assert_eq!(pixmap.height(), 100);

        // In Multiply: Red (255, 0, 0) * Green (0, 255, 0) = Black (0, 0, 0)
        let pixel = pixmap.pixel(50, 50).expect("pixel");
        assert_eq!(pixel.red(), 0);
        assert_eq!(pixel.green(), 0);
        assert_eq!(pixel.blue(), 0);
        assert_eq!(pixel.alpha(), 255);
    }

    #[test]
    fn test_psd_all_27_blend_modes_conversion() {
        for mode in PsdBlendMode::ALL {
            let rasterizer_mode = BlendMode::from(mode);
            let color_mode = rasterizer_mode.to_color_blend_mode();
            assert_eq!(color_mode.psd_key(), mode.key());
        }
    }

    #[test]
    fn test_curves_and_vibrance_filters() {
        use rust_motion_color::adjust::Adjustment;

        let curves = SceneFilter::Curves {
            master: vec![[0.0, 0.0], [0.5, 0.8], [1.0, 1.0]],
            red: Vec::new(),
            green: Vec::new(),
            blue: Vec::new(),
        };

        let vibrance = SceneFilter::Vibrance {
            vibrance: 50.0,
            saturation: -20.0,
        };

        let levels = SceneFilter::Levels {
            in_black: 0.1,
            in_white: 0.9,
            gamma: 1.2,
            out_black: 0.05,
            out_white: 0.95,
        };

        let color_balance = SceneFilter::ColorBalance {
            shadows: [10.0, -5.0, 0.0],
            midtones: [0.0, 15.0, -10.0],
            highlights: [-5.0, 0.0, 20.0],
            preserve_luminosity: true,
        };

        let direct = SceneFilter::Adjustment(Box::new(Adjustment::Invert));

        // Create scene and render
        let scene = Scene {
            nodes: vec![SceneNode::Layer {
                opacity: 1.0,
                blend_mode: BlendMode::Normal,
                clip: None,
                mask: None,
                mask_mode: MaskMode::Alpha,
                filters: vec![curves, vibrance, levels, color_balance, direct],
                shadow: None,
                children: vec![SceneNode::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 50.0,
                    h: 50.0,
                    fill: Color::rgb(128, 128, 128),
                    stroke: None,
                    stroke_width: 0.0,
                    corner_radius: 0.0,
                }],
            }],
        };

        let backend = crate::tiny_skia_backend::TinySkiaBackend::new();
        let config = FrameConfig {
            width: 50,
            height: 50,
            frame: 0,
            fps: 30.0,
        };
        let img = backend.render_frame(&scene, &config).expect("render scene with adjustment filters");
        assert_eq!(img.width(), 50);
        assert_eq!(img.height(), 50);
        let pixel = img.get_pixel(25, 25);
        assert_eq!(pixel[3], 255);
    }
}
