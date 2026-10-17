//! Affinity document (.af, .afdesign, .afphoto) reader and native scene loader.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use image::RgbaImage;
use tiny_skia::Pixmap;

use crate::backend::{FrameConfig, RasterError, RasterizerBackend};
use crate::psd::rgba_to_png_data_uri;
use crate::scene::{BlendMode, Color, ImageFit, Scene, SceneNode};
use crate::tiny_skia_backend::TinySkiaBackend;

use photocraft_affinity::model::{Kind, Node};
use photocraft_affinity::paint::{Color as AffColor, Paint};
use photocraft_affinity::{preview, read, Limits};

/// Convert an Affinity color to a rasterizer RGBA [`Color`].
pub fn affinity_color_to_rgba(c: &AffColor) -> Color {
    match c {
        AffColor::Rgb { r, g, b, a } => Color::rgba(
            (r.clamp(0.0, 1.0) * 255.0).round() as u8,
            (g.clamp(0.0, 1.0) * 255.0).round() as u8,
            (b.clamp(0.0, 1.0) * 255.0).round() as u8,
            (a.clamp(0.0, 1.0) * 255.0).round() as u8,
        ),
        AffColor::Gray { v, a } => {
            let val = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            Color::rgba(val, val, val, (a.clamp(0.0, 1.0) * 255.0).round() as u8)
        }
        AffColor::Cmyk { c, m, y, k, a } => {
            let r = (1.0 - c) * (1.0 - k);
            let g = (1.0 - m) * (1.0 - k);
            let b = (1.0 - y) * (1.0 - k);
            Color::rgba(
                (r.clamp(0.0, 1.0) * 255.0).round() as u8,
                (g.clamp(0.0, 1.0) * 255.0).round() as u8,
                (b.clamp(0.0, 1.0) * 255.0).round() as u8,
                (a.clamp(0.0, 1.0) * 255.0).round() as u8,
            )
        }
        AffColor::Lab { l, alpha, .. } => {
            let val = (l.clamp(0.0, 100.0) / 100.0 * 255.0).round() as u8;
            Color::rgba(val, val, val, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
        }
    }
}

/// Convert an Affinity blend mode to a native [`BlendMode`].
pub fn map_affinity_blend(b: photocraft_affinity::model::Blend) -> BlendMode {
    match b {
        photocraft_affinity::model::Blend::Normal => BlendMode::Normal,
        photocraft_affinity::model::Blend::PassThrough => BlendMode::PassThrough,
        photocraft_affinity::model::Blend::Darken => BlendMode::Darken,
        photocraft_affinity::model::Blend::DarkerColor => BlendMode::DarkerColor,
        photocraft_affinity::model::Blend::Multiply => BlendMode::Multiply,
        photocraft_affinity::model::Blend::ColorBurn => BlendMode::ColorBurn,
        photocraft_affinity::model::Blend::Lighten => BlendMode::Lighten,
        photocraft_affinity::model::Blend::LighterColor => BlendMode::LighterColor,
        photocraft_affinity::model::Blend::Screen => BlendMode::Screen,
        photocraft_affinity::model::Blend::ColorDodge => BlendMode::ColorDodge,
        photocraft_affinity::model::Blend::Add => BlendMode::LinearDodge,
        photocraft_affinity::model::Blend::Overlay => BlendMode::Overlay,
        photocraft_affinity::model::Blend::SoftLight => BlendMode::SoftLight,
        photocraft_affinity::model::Blend::HardLight => BlendMode::HardLight,
        photocraft_affinity::model::Blend::VividLight => BlendMode::VividLight,
        photocraft_affinity::model::Blend::PinLight => BlendMode::PinLight,
        photocraft_affinity::model::Blend::LinearLight => BlendMode::LinearLight,
        photocraft_affinity::model::Blend::HardMix => BlendMode::HardMix,
        photocraft_affinity::model::Blend::Difference => BlendMode::Difference,
        photocraft_affinity::model::Blend::Exclusion => BlendMode::Exclusion,
        photocraft_affinity::model::Blend::Subtract => BlendMode::Subtract,
        photocraft_affinity::model::Blend::Hue => BlendMode::Hue,
        photocraft_affinity::model::Blend::Saturation => BlendMode::Saturation,
        photocraft_affinity::model::Blend::Luminosity => BlendMode::Luminosity,
        photocraft_affinity::model::Blend::Color => BlendMode::Color,
        _ => BlendMode::Normal,
    }
}

/// Convert an Affinity Bézier path into SVG path command string.
pub fn affinity_path_to_svg_d(path: &photocraft_affinity::model::Path) -> String {
    let mut d = String::new();
    for subpath in &path.subpaths {
        d.push_str(&format!("M {} {} ", subpath.start.x, subpath.start.y));
        for seg in &subpath.segments {
            d.push_str(&format!(
                "C {} {} {} {} {} {} ",
                seg[0].x, seg[0].y, seg[1].x, seg[1].y, seg[2].x, seg[2].y
            ));
        }
        if subpath.closed {
            d.push_str("Z ");
        }
    }
    d
}

/// Convert an Affinity Node into a SceneNode.
fn node_to_scene_node(node: &Node) -> Result<Option<SceneNode>, RasterError> {
    if !node.visible {
        return Ok(None);
    }

    let blend_mode = map_affinity_blend(node.blend);
    let opacity = node.opacity as f32;

    let mut direct_node = match &node.kind {
        Kind::Shape {
            path,
            fills,
            strokes,
            ..
        } => {
            let d = affinity_path_to_svg_d(path);
            let fill_color = fills.first().and_then(|f| match f {
                Paint::Solid(c) => Some(affinity_color_to_rgba(c)),
                _ => None,
            });
            let (stroke_color, stroke_width) = strokes
                .first()
                .map(|s| {
                    let color = match &s.paint {
                        Paint::Solid(c) => Some(affinity_color_to_rgba(c)),
                        _ => None,
                    };
                    (color, s.width as f32)
                })
                .unwrap_or((None, 0.0));

            Some(SceneNode::Path {
                d,
                fill: fill_color,
                stroke: stroke_color,
                stroke_width,
                opacity: 1.0,
            })
        }
        Kind::Text(text) => {
            let mut text_nodes = Vec::new();
            for run in &text.runs {
                let color = match &run.fill {
                    Paint::Solid(c) => affinity_color_to_rgba(c),
                    _ => Color::BLACK,
                };
                text_nodes.push(SceneNode::Text {
                    x: text.anchor.x as f32,
                    y: text.anchor.y as f32,
                    content: run.text.clone(),
                    font_size: (run.size * text.transform.scale()) as f32,
                    color,
                    font_weight: run.weight as u16,
                    font_sources: if run.family.is_empty() {
                        Vec::new()
                    } else {
                        vec![run.family.clone()]
                    },
                });
            }
            if text_nodes.len() == 1 {
                text_nodes.pop()
            } else if !text_nodes.is_empty() {
                Some(SceneNode::Group {
                    transform: Default::default(),
                    opacity: 1.0,
                    children: text_nodes,
                })
            } else {
                None
            }
        }
        Kind::Image(img) => {
            let data_uri = match &img.pixels {
                photocraft_affinity::model::Pixels::Encoded(bytes) => {
                    let b64 = BASE64.encode(bytes);
                    format!("data:image/png;base64,{b64}")
                }
                photocraft_affinity::model::Pixels::Rgba8(rgba) => {
                    rgba_to_png_data_uri(img.width, img.height, rgba)?
                }
            };
            let x = img.transform.0[4] as f32;
            let y = img.transform.0[5] as f32;
            let w = (img.width as f64 * img.transform.scale()) as f32;
            let h = (img.height as f64 * img.transform.scale()) as f32;
            Some(SceneNode::Image {
                src: data_uri,
                x,
                y,
                w,
                h,
                fit: ImageFit::Contain,
                opacity: 1.0,
            })
        }
        Kind::Artboard { rect, background } => {
            let mut artboard_nodes = Vec::new();
            if let Some(Paint::Solid(c)) = background.first() {
                artboard_nodes.push(SceneNode::Rect {
                    x: rect.x0 as f32,
                    y: rect.y0 as f32,
                    w: (rect.x1 - rect.x0) as f32,
                    h: (rect.y1 - rect.y0) as f32,
                    fill: affinity_color_to_rgba(c),
                    stroke: None,
                    stroke_width: 0.0,
                    corner_radius: 0.0,
                });
            }
            Some(SceneNode::Group {
                transform: Default::default(),
                opacity: 1.0,
                children: artboard_nodes,
            })
        }
        Kind::Layer | Kind::Group | Kind::Unsupported => None,
    };

    // Recursively convert children
    let mut child_scene_nodes = Vec::new();
    for ch in &node.children {
        if let Some(scn) = node_to_scene_node(ch)? {
            child_scene_nodes.push(scn);
        }
    }

    if let Some(dnode) = direct_node.take() {
        if child_scene_nodes.is_empty() && (opacity - 1.0).abs() < 1e-4 && blend_mode == BlendMode::Normal {
            return Ok(Some(dnode));
        }
        let mut all_children = vec![dnode];
        all_children.extend(child_scene_nodes);
        Ok(Some(SceneNode::Layer {
            opacity,
            blend_mode,
            clip: None,
            mask: None,
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: Vec::new(),
            shadow: None,
            children: all_children,
        }))
    } else if !child_scene_nodes.is_empty() {
        Ok(Some(SceneNode::Layer {
            opacity,
            blend_mode,
            clip: None,
            mask: None,
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: Vec::new(),
            shadow: None,
            children: child_scene_nodes,
        }))
    } else {
        Ok(None)
    }
}

/// Convert an Affinity document (.af, .afdesign, .afphoto) into a native `(Scene, width, height)`.
pub fn load_affinity_as_scene(data: &[u8]) -> Result<(Scene, u32, u32), RasterError> {
    let doc = read(data, Limits::default())
        .map_err(|e| RasterError::Scene(format!("failed to parse Affinity document: {e}")))?;

    let (canvas_w, canvas_h, spread_nodes) = if let Some(spread) = doc.spreads.first() {
        let w = (spread.bounds.x1 - spread.bounds.x0).ceil().max(1.0) as u32;
        let h = (spread.bounds.y1 - spread.bounds.y0).ceil().max(1.0) as u32;
        (w, h, &spread.nodes)
    } else {
        (1920, 1080, &Vec::new())
    };

    let mut nodes = Vec::new();

    for node in spread_nodes {
        if let Some(sn) = node_to_scene_node(node)? {
            nodes.push(sn);
        }
    }

    Ok((Scene { nodes }, canvas_w, canvas_h))
}

/// Read the embedded PNG preview / thumbnail if present.
pub fn read_affinity_preview(data: &[u8]) -> Result<Option<Vec<u8>>, RasterError> {
    match preview(data) {
        Ok(p) => Ok(Some(p.png.to_vec())),
        Err(photocraft_affinity::Error::Unsupported(_)) => Ok(None),
        Err(e) => Err(RasterError::Scene(format!("failed to read Affinity preview: {e}"))),
    }
}

/// Render an Affinity document to an RGBA pixel buffer image.
pub fn render_affinity_to_image(data: &[u8]) -> Result<RgbaImage, RasterError> {
    let (scene, width, height) = load_affinity_as_scene(data)?;
    let backend = TinySkiaBackend::new();
    let config = FrameConfig::new(width, height, 0, 30.0);
    backend.render_frame(&scene, &config)
}

/// Render an Affinity document directly to a tiny-skia Pixmap.
pub fn render_affinity_to_pixmap(data: &[u8]) -> Result<Pixmap, RasterError> {
    let img = render_affinity_to_image(data)?;
    let mut pixmap = Pixmap::new(img.width(), img.height())
        .ok_or_else(|| RasterError::Scene("failed to allocate Pixmap".into()))?;

    // Copy RGBA straight to premultiplied
    for (src_px, dst_px) in img.pixels().zip(pixmap.pixels_mut()) {
        let a = src_px[3] as f32 / 255.0;
        let r = ((src_px[0] as f32 / 255.0) * a * 255.0).round() as u8;
        let g = ((src_px[1] as f32 / 255.0) * a * 255.0).round() as u8;
        let b = ((src_px[2] as f32 / 255.0) * a * 255.0).round() as u8;
        let a = src_px[3];
        *dst_px = tiny_skia::PremultipliedColorU8::from_rgba(r, g, b, a)
            .ok_or_else(|| RasterError::Scene("invalid pixel color".into()))?;
    }

    Ok(pixmap)
}
