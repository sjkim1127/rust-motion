//! SceneNode traversal and per-primitive rasterization.

use crate::backend::RasterError;
use crate::gif_cache::GifFrameCache;
use crate::scene::{BlendMode, ClipRegion, ImageFit, MaskMode, SceneNode, TrackMatteMode};
use tiny_skia::{
    FillRule, Mask, MaskType, Paint, PathBuilder, Pixmap, PixmapPaint, PixmapRef,
    PremultipliedColorU8, Rect, Stroke, Transform,
};

use super::color_blend::{apply_opacity, composite_custom_psd_blend, tiny_skia_blend_mode};
use super::filters::{apply_filter, make_shadow};
use super::media::{draw_media, draw_video_media};
use super::shapes::{build_circle, build_rounded_rect, svgpath_to_tiny_skia};
use super::visualizer::render_audio_visualizer;
use super::RenderResources;

pub(crate) fn render_nodes(
    pixmap: &mut Pixmap,
    nodes: &[SceneNode],
    parent_transform: Transform,
    parent_opacity: f32,
    resources: &RenderResources<'_>,
) -> Result<(), RasterError> {
    for node in nodes {
        render_node(pixmap, node, parent_transform, parent_opacity, resources)?;
    }
    Ok(())
}

pub(crate) fn render_node(
    pixmap: &mut Pixmap,
    node: &SceneNode,
    transform: Transform,
    opacity: f32,
    resources: &RenderResources<'_>,
) -> Result<(), RasterError> {
    match node {
        SceneNode::Rect {
            x,
            y,
            w,
            h,
            fill,
            stroke,
            stroke_width,
            corner_radius,
        } => {
            let rect = match Rect::from_xywh(*x, *y, *w, *h) {
                Some(r) => r,
                None => return Ok(()),
            };

            let path = if *corner_radius > 0.0 {
                build_rounded_rect(*x, *y, *w, *h, *corner_radius)
            } else {
                PathBuilder::from_rect(rect)
            };

            // Fill
            let mut paint = Paint::default();
            paint.set_color(apply_opacity(*fill, opacity));
            paint.anti_alias = true;
            pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);

            // Stroke
            if let (Some(stroke_color), sw) = (stroke, stroke_width) {
                if *sw > 0.0 {
                    let mut stroke_paint = Paint::default();
                    stroke_paint.set_color(apply_opacity(*stroke_color, opacity));
                    stroke_paint.anti_alias = true;
                    let stroke = Stroke {
                        width: *sw,
                        ..Default::default()
                    };
                    pixmap.stroke_path(&path, &stroke_paint, &stroke, transform, None);
                }
            }
        }

        SceneNode::Circle {
            cx,
            cy,
            r,
            fill,
            stroke,
            stroke_width,
        } => {
            let path = build_circle(*cx, *cy, *r);

            let mut paint = Paint::default();
            paint.set_color(apply_opacity(*fill, opacity));
            paint.anti_alias = true;
            pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);

            if let (Some(stroke_color), sw) = (stroke, stroke_width) {
                if *sw > 0.0 {
                    let mut stroke_paint = Paint::default();
                    stroke_paint.set_color(apply_opacity(*stroke_color, opacity));
                    stroke_paint.anti_alias = true;
                    let stroke = Stroke {
                        width: *sw,
                        ..Default::default()
                    };
                    pixmap.stroke_path(&path, &stroke_paint, &stroke, transform, None);
                }
            }
        }

        SceneNode::Path {
            d,
            fill,
            stroke,
            stroke_width,
            opacity: node_opacity,
        } => {
            let combined_opacity = opacity * node_opacity;

            if let Some(path) = svgpath_to_tiny_skia(d) {
                if let Some(fill_color) = fill {
                    let mut paint = Paint::default();
                    paint.set_color(apply_opacity(*fill_color, combined_opacity));
                    paint.anti_alias = true;
                    pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
                }

                if let (Some(stroke_color), sw) = (stroke, stroke_width) {
                    if *sw > 0.0 {
                        let mut stroke_paint = Paint::default();
                        stroke_paint.set_color(apply_opacity(*stroke_color, combined_opacity));
                        stroke_paint.anti_alias = true;
                        let stroke = Stroke {
                            width: *sw,
                            ..Default::default()
                        };
                        pixmap.stroke_path(&path, &stroke_paint, &stroke, transform, None);
                    }
                }
            }
        }

        SceneNode::Image {
            src,
            x,
            y,
            w,
            h,
            fit,
            opacity: node_opacity,
        } => {
            let source = resources.images.load_with_policy(src, resources.security)?;
            draw_media(
                pixmap,
                &source,
                src,
                *x,
                *y,
                *w,
                *h,
                *fit,
                opacity * node_opacity,
                transform,
            )?;
        }

        SceneNode::Video {
            src,
            time,
            looped,
            x,
            y,
            w,
            h,
            fit,
            opacity: node_opacity,
            interpolation,
            time_remap,
        } => {
            let eff_time = match time_remap {
                Some(ref curve) => curve.eval(*time),
                None => *time,
            };

            let source = match interpolation {
                crate::time_remap::FrameInterpolationMode::FrameRepeat => {
                    resources.videos.load_with_policy(
                        src,
                        eff_time,
                        resources.sampling_fps,
                        *looped,
                        resources.security,
                    )?
                }
                crate::time_remap::FrameInterpolationMode::FrameBlend
                | crate::time_remap::FrameInterpolationMode::PixelMotion => {
                    let fps = resources.sampling_fps;
                    let exact_frame = eff_time * fps;
                    let f0 = exact_frame.floor();
                    let alpha = (exact_frame - f0) as f32;
                    let t0 = f0 / fps;
                    let t1 = (f0 + 1.0) / fps;

                    let img0 = resources.videos.load_with_policy(
                        src,
                        t0,
                        fps,
                        *looped,
                        resources.security,
                    )?;

                    if alpha <= 0.001 {
                        img0
                    } else if alpha >= 0.999 {
                        resources.videos.load_with_policy(
                            src,
                            t1,
                            fps,
                            *looped,
                            resources.security,
                        )?
                    } else {
                        let img1 = resources.videos.load_with_policy(
                            src,
                            t1,
                            fps,
                            *looped,
                            resources.security,
                        )?;

                        let p0 = Pixmap::from_vec(
                            img0.as_raw().clone(),
                            tiny_skia::IntSize::from_wh(img0.width(), img0.height()).unwrap(),
                        );
                        let p1 = Pixmap::from_vec(
                            img1.as_raw().clone(),
                            tiny_skia::IntSize::from_wh(img1.width(), img1.height()).unwrap(),
                        );

                        if let (Some(px0), Some(px1)) = (p0, p1) {
                            let interpolated =
                                crate::time_remap::OpticalFlowEngine::interpolate_frames(
                                    &px0,
                                    &px1,
                                    alpha,
                                    *interpolation,
                                );
                            let rgba = image::RgbaImage::from_raw(
                                interpolated.width(),
                                interpolated.height(),
                                interpolated.take(),
                            )
                            .unwrap();
                            std::sync::Arc::new(rgba)
                        } else {
                            img0
                        }
                    }
                }
            };

            draw_video_media(
                pixmap,
                &source,
                src,
                *x,
                *y,
                *w,
                *h,
                *fit,
                opacity * node_opacity,
                transform,
            )?;
        }

        SceneNode::Audio { .. } => {}

        SceneNode::Gif {
            src,
            time,
            x,
            y,
            w,
            h,
            playback_rate,
            loop_behavior,
            fit,
            opacity: node_opacity,
        } => {
            let frames = resources
                .gifs
                .load_frames_with_policy(src, resources.security)?;
            // Convert composition time to GIF playback time in milliseconds.
            // `time` is already the composition time in seconds; scale by playback_rate.
            let time_ms = *time * *playback_rate as f64 * 1000.0;
            let maybe_frame = GifFrameCache::frame_at_time_ms(&frames, time_ms, *loop_behavior);
            if let Some(frame_image) = maybe_frame {
                draw_media(
                    pixmap,
                    frame_image,
                    src,
                    *x,
                    *y,
                    *w,
                    *h,
                    *fit,
                    opacity * node_opacity,
                    transform,
                )?;
            }
        }

        SceneNode::Lottie {
            src,
            time,
            x,
            y,
            w,
            h,
            playback_rate,
            loop_behavior,
            opacity: node_opacity,
        } => {
            let time_secs = *time * *playback_rate as f64;
            let target_w = (*w).round().max(1.0) as u32;
            let target_h = (*h).round().max(1.0) as u32;
            let frame_image = resources.lotties.render_with_policy(
                src,
                time_secs,
                target_w,
                target_h,
                *loop_behavior,
                resources.security,
            )?;
            draw_media(
                pixmap,
                &frame_image,
                src,
                *x,
                *y,
                *w,
                *h,
                ImageFit::Contain,
                opacity * node_opacity,
                transform,
            )?;
        }

        SceneNode::Emoji {
            emoji,
            x,
            y,
            size,
            opacity: node_opacity,
        } => {
            let target_size = (*size).round().max(8.0) as u32;
            if let Some(emoji_img) = crate::emoji::render_emoji(emoji, target_size) {
                draw_media(
                    pixmap,
                    &emoji_img,
                    emoji,
                    *x,
                    *y,
                    *size,
                    *size,
                    ImageFit::Contain,
                    opacity * node_opacity,
                    transform,
                )?;
            }
        }

        SceneNode::AudioVisualizer {
            src,
            x,
            y,
            width,
            height,
            color,
            style,
            time,
            opacity: node_opacity,
        } => {
            let eff_opacity = opacity * node_opacity;
            if eff_opacity > 0.0 && *width > 0.0 && *height > 0.0 {
                render_audio_visualizer(
                    pixmap,
                    resources,
                    src,
                    *x,
                    *y,
                    *width,
                    *height,
                    color.with_opacity(eff_opacity),
                    style,
                    *time,
                    transform,
                )?;
            }
        }

        SceneNode::LinearGradient {
            x,
            y,
            w,
            h,
            angle_deg,
            stops,
        } => {
            if stops.is_empty() {
                return Ok(());
            }

            let rect = match Rect::from_xywh(*x, *y, *w, *h) {
                Some(r) => r,
                None => return Ok(()),
            };
            let path = PathBuilder::from_rect(rect);

            // Compute gradient endpoints from angle
            let angle_rad = angle_deg.to_radians();
            let cx = x + w / 2.0;
            let cy = y + h / 2.0;
            let half_diag = (w * w + h * h).sqrt() / 2.0;

            let x1 = cx - angle_rad.sin() * half_diag;
            let y1 = cy - angle_rad.cos() * half_diag;
            let x2 = cx + angle_rad.sin() * half_diag;
            let y2 = cy + angle_rad.cos() * half_diag;

            let sk_stops: Vec<tiny_skia::GradientStop> = stops
                .iter()
                .map(|s| tiny_skia::GradientStop::new(s.position, apply_opacity(s.color, opacity)))
                .collect();

            if let Some(shader) = tiny_skia::LinearGradient::new(
                tiny_skia::Point::from_xy(x1, y1),
                tiny_skia::Point::from_xy(x2, y2),
                sk_stops,
                tiny_skia::SpreadMode::Pad,
                Transform::identity(),
            ) {
                let paint = Paint {
                    shader,
                    anti_alias: true,
                    ..Default::default()
                };
                pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
            }
        }

        SceneNode::RadialGradient { cx, cy, r, stops } => {
            if stops.is_empty() {
                return Ok(());
            }

            let path = build_circle(*cx, *cy, *r);

            let sk_stops: Vec<tiny_skia::GradientStop> = stops
                .iter()
                .map(|s| tiny_skia::GradientStop::new(s.position, apply_opacity(s.color, opacity)))
                .collect();

            if let Some(shader) = tiny_skia::RadialGradient::new(
                tiny_skia::Point::from_xy(*cx, *cy),
                tiny_skia::Point::from_xy(*cx, *cy),
                *r,
                sk_stops,
                tiny_skia::SpreadMode::Pad,
                Transform::identity(),
            ) {
                let paint = Paint {
                    shader,
                    anti_alias: true,
                    ..Default::default()
                };
                pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
            }
        }

        SceneNode::Group {
            transform: group_transform,
            opacity: group_opacity,
            children,
        }
        | SceneNode::NullObject {
            transform: group_transform,
            opacity: group_opacity,
            children,
            ..
        } => {
            let new_transform = transform.post_concat(group_transform.to_tiny_skia());
            let new_opacity = opacity * group_opacity;
            render_nodes(pixmap, children, new_transform, new_opacity, resources)?;
        }

        SceneNode::Layer {
            opacity: layer_opacity,
            blend_mode,
            clip,
            mask,
            mask_mode,
            filters,
            shadow,
            children,
        } => {
            if !opacity.is_finite() || !layer_opacity.is_finite() {
                return Err(RasterError::Scene(
                    "layer and inherited opacity must be finite".into(),
                ));
            }
            if opacity <= 0.0 || *layer_opacity <= 0.0 {
                // A fully transparent layer cannot affect the destination;
                // avoid evaluating children or allocating compositing surfaces.
                return Ok(());
            }
            if children.is_empty() && clip.is_none() && mask.is_none() && shadow.is_none() {
                // Preserve filter validation while avoiding a full-canvas
                // surface for an empty layer (for example an overlay filter).
                let mut validation_surface = Pixmap::new(1, 1).ok_or_else(|| {
                    RasterError::Scene("failed to allocate filter validation surface".into())
                })?;
                for filter in filters {
                    apply_filter(&mut validation_surface, filter)?;
                }
                return Ok(());
            }
            if filters.is_empty()
                && shadow.is_none()
                && clip.is_none()
                && mask.is_none()
                && *blend_mode == BlendMode::Normal
                && (opacity - 1.0).abs() <= f32::EPSILON
                && (*layer_opacity - 1.0).abs() <= f32::EPSILON
            {
                // A fully opaque normal layer without compositing features is
                // equivalent to its children; avoid a full-canvas allocation
                // and copy on the common grouping path.
                render_nodes(pixmap, children, transform, opacity, resources)?;
                return Ok(());
            }
            let mut layer = Pixmap::new(pixmap.width(), pixmap.height())
                .ok_or_else(|| RasterError::Scene("failed to allocate layer surface".into()))?;
            render_nodes(&mut layer, children, transform, 1.0, resources)?;

            for filter in filters {
                apply_filter(&mut layer, filter)?;
            }
            if let Some(clip) = clip {
                let clip_mask = render_clip_mask(pixmap.width(), pixmap.height(), clip, transform)?;
                layer.apply_mask(&clip_mask);
            }
            if let Some(mask_nodes) = mask {
                let mut mask_pixmap = Pixmap::new(pixmap.width(), pixmap.height())
                    .ok_or_else(|| RasterError::Scene("failed to allocate mask surface".into()))?;
                render_nodes(&mut mask_pixmap, mask_nodes, transform, 1.0, resources)?;
                let (mask_type, invert) = match mask_mode {
                    MaskMode::Alpha => (MaskType::Alpha, false),
                    MaskMode::AlphaInverted => (MaskType::Alpha, true),
                    MaskMode::Luminance => (MaskType::Luminance, false),
                    MaskMode::LuminanceInverted => (MaskType::Luminance, true),
                };

                if invert {
                    match mask_mode {
                        MaskMode::AlphaInverted => {
                            for pixel in mask_pixmap.pixels_mut() {
                                let inv_a = 255 - pixel.alpha();
                                *pixel = PremultipliedColorU8::from_rgba(0, 0, 0, inv_a)
                                    .unwrap_or(PremultipliedColorU8::TRANSPARENT);
                            }
                        }
                        MaskMode::LuminanceInverted => {
                            for pixel in mask_pixmap.pixels_mut() {
                                let a = pixel.alpha();
                                if a == 0 {
                                    *pixel = PremultipliedColorU8::from_rgba(255, 255, 255, 255)
                                        .unwrap();
                                } else {
                                    let r = 255 - pixel.red();
                                    let g = 255 - pixel.green();
                                    let b = 255 - pixel.blue();
                                    *pixel = PremultipliedColorU8::from_rgba(r, g, b, 255)
                                        .unwrap_or(PremultipliedColorU8::TRANSPARENT);
                                }
                            }
                        }
                        _ => {}
                    }
                }

                layer.apply_mask(&Mask::from_pixmap(mask_pixmap.as_ref(), mask_type));
            }

            let eff_opacity = (opacity * layer_opacity).clamp(0.0, 1.0);
            if let Some(sk_mode) = tiny_skia_blend_mode(*blend_mode) {
                let composite_paint = PixmapPaint {
                    opacity: eff_opacity,
                    blend_mode: sk_mode,
                    ..Default::default()
                };
                if let Some(shadow) = shadow {
                    let shadow_pixmap = make_shadow(&layer, shadow)?;
                    pixmap.draw_pixmap(
                        shadow.offset_x.round() as i32,
                        shadow.offset_y.round() as i32,
                        shadow_pixmap.as_ref(),
                        &composite_paint,
                        Transform::identity(),
                        None,
                    );
                }
                pixmap.draw_pixmap(
                    0,
                    0,
                    layer.as_ref(),
                    &composite_paint,
                    Transform::identity(),
                    None,
                );
            } else {
                let color_mode = blend_mode.to_color_blend_mode();
                if let Some(shadow) = shadow {
                    let shadow_pixmap = make_shadow(&layer, shadow)?;
                    composite_custom_psd_blend(
                        pixmap,
                        &shadow_pixmap,
                        color_mode,
                        eff_opacity,
                        shadow.offset_x.round() as i32,
                        shadow.offset_y.round() as i32,
                    );
                }
                composite_custom_psd_blend(pixmap, &layer, color_mode, eff_opacity, 0, 0);
            }
        }

        SceneNode::Text {
            x,
            y,
            content,
            font_size,
            color,
            font_sources,
            font_weight,
        } => {
            let has_emoji = content.chars().any(crate::emoji::is_emoji_char);
            if has_emoji {
                let runs = crate::emoji::split_text_and_emojis(content);
                let mut pen_x = *x;
                for run in runs {
                    match run {
                        crate::emoji::TextRun::Text(text_part) => {
                            if text_part.is_empty() {
                                continue;
                            }
                            let part_width = crate::font::measure_text_width(
                                &text_part,
                                *font_size,
                                font_sources,
                            )
                            .unwrap_or_else(|_| {
                                text_part.chars().count() as f32 * *font_size * 0.5
                            });
                            let sub_node = SceneNode::Text {
                                x: pen_x,
                                y: *y,
                                content: text_part,
                                font_size: *font_size,
                                color: *color,
                                font_weight: *font_weight,
                                font_sources: font_sources.clone(),
                            };
                            render_node(pixmap, &sub_node, transform, opacity, resources)?;
                            pen_x += part_width;
                        }
                        crate::emoji::TextRun::Emoji(emoji_str) => {
                            let emoji_node = SceneNode::Emoji {
                                emoji: emoji_str,
                                x: pen_x,
                                y: *y - *font_size * 0.85,
                                size: *font_size,
                                opacity: 1.0,
                            };
                            render_node(pixmap, &emoji_node, transform, opacity, resources)?;
                            pen_x += *font_size * 1.08;
                        }
                    }
                }
                return Ok(());
            }

            let text_color = apply_opacity(*color, opacity);

            if let Some(rendered) = resources
                .font
                .rasterize_with_weight(content, *font_size, *font_weight, font_sources)
                .map_err(|error| RasterError::FontAsset {
                    path: error.path,
                    reason: error.reason,
                })?
            {
                // Blit the glyph coverage map onto the pixmap at (x, y - baseline)
                let origin_x = x.floor() as i32;
                let origin_y = (*y - rendered.baseline as f32).floor() as i32;

                let pw = pixmap.width() as i32;
                let ph = pixmap.height() as i32;
                let pixmap_width = pixmap.width(); // cache before mutable borrow

                let pixels_rgba = pixmap.pixels_mut();

                for gy in 0..rendered.height {
                    for gx in 0..rendered.width {
                        let coverage = rendered.pixels[(gy * rendered.width + gx) as usize];
                        if coverage == 0 {
                            continue;
                        }

                        let px = origin_x + gx as i32;
                        let py = origin_y + gy as i32;
                        if px < 0 || py < 0 || px >= pw || py >= ph {
                            continue;
                        }

                        let idx = (py as u32 * pixmap_width + px as u32) as usize;
                        let glyph_coverage = coverage as f32 / 255.0;
                        let src_alpha = glyph_coverage * text_color.alpha();
                        if src_alpha <= 0.0 {
                            continue;
                        }
                        let inv_src_alpha = 1.0 - src_alpha;

                        // Premultiplied source channels in 0..=255
                        let src_r = text_color.red() * 255.0 * src_alpha;
                        let src_g = text_color.green() * 255.0 * src_alpha;
                        let src_b = text_color.blue() * 255.0 * src_alpha;
                        let src_a = 255.0 * src_alpha;

                        let dst = pixels_rgba[idx];
                        let out_r = (src_r + dst.red() as f32 * inv_src_alpha)
                            .round()
                            .min(255.0) as u8;
                        let out_g = (src_g + dst.green() as f32 * inv_src_alpha)
                            .round()
                            .min(255.0) as u8;
                        let out_b = (src_b + dst.blue() as f32 * inv_src_alpha)
                            .round()
                            .min(255.0) as u8;
                        let out_a = (src_a + dst.alpha() as f32 * inv_src_alpha)
                            .round()
                            .min(255.0) as u8;

                        pixels_rgba[idx] =
                            tiny_skia::PremultipliedColorU8::from_rgba(out_r, out_g, out_b, out_a)
                                .unwrap_or(pixels_rgba[idx]);
                    }
                }
            } else {
                // Fallback: draw a solid colour block per character (no font loaded)
                let char_w = *font_size * 0.6;
                let mut cx = *x;
                for _ch in content.chars() {
                    let rect = match Rect::from_xywh(
                        cx,
                        *y - font_size,
                        char_w.max(1.0),
                        font_size.max(1.0),
                    ) {
                        Some(r) => r,
                        None => {
                            cx += char_w;
                            continue;
                        }
                    };
                    let path = PathBuilder::from_rect(rect);
                    let mut paint = Paint::default();
                    paint.set_color(text_color);
                    paint.anti_alias = true;
                    pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
                    cx += char_w;
                }
            }
        }

        SceneNode::Shader {
            x,
            y,
            w,
            h,
            source: _,
            time,
            params,
            opacity: node_opacity,
        } => {
            let eff_opacity = opacity * node_opacity;
            if eff_opacity > 0.0 && *w > 0.0 && *h > 0.0 {
                let pw = (*w as u32).clamp(16, 256);
                let ph = (*h as u32).clamp(16, 256);
                if let Some(mut sm_pixmap) = Pixmap::new(pw, ph) {
                    let pixels = sm_pixmap.data_mut();
                    let t = *time;
                    let p0 = params[0].max(0.1);
                    let p1 = params[1].max(0.1);
                    let p2 = params[2].max(0.1);
                    for py in 0..ph {
                        let ny = py as f32 / ph as f32;
                        for px in 0..pw {
                            let nx = px as f32 / pw as f32;
                            let v1 = (nx * 10.0 + t).sin();
                            let v2 =
                                ((ny * 10.0 + t * 1.3).sin() + (nx * ny * 5.0 + t).cos()) * 0.5;
                            let r = (((v1 * 0.5 + 0.5) * p0) * 255.0).clamp(0.0, 255.0) as u8;
                            let g = (((v2 * 0.5 + 0.5) * p1) * 255.0).clamp(0.0, 255.0) as u8;
                            let b = (((((nx + ny) * 5.0 + t * 0.7).sin() * 0.5 + 0.5) * p2) * 255.0)
                                .clamp(0.0, 255.0) as u8;
                            let idx = ((py * pw + px) * 4) as usize;
                            pixels[idx] = r;
                            pixels[idx + 1] = g;
                            pixels[idx + 2] = b;
                            pixels[idx + 3] = (255.0 * eff_opacity).clamp(0.0, 255.0) as u8;
                        }
                    }
                    let sx = *w / pw as f32;
                    let sy = *h / ph as f32;
                    let shader_transform = transform.pre_translate(*x, *y).pre_scale(sx, sy);
                    let paint = PixmapPaint {
                        opacity: eff_opacity.clamp(0.0, 1.0),
                        ..Default::default()
                    };
                    pixmap.draw_pixmap(0, 0, sm_pixmap.as_ref(), &paint, shader_transform, None);
                }
            }
        }

        SceneNode::TrackMatte {
            target,
            matte,
            mode,
        } => {
            if opacity <= 0.0 {
                return Ok(());
            }

            let mut target_pixmap =
                Pixmap::new(pixmap.width(), pixmap.height()).ok_or_else(|| {
                    RasterError::Scene("failed to allocate track matte target surface".into())
                })?;
            render_node(&mut target_pixmap, target, transform, opacity, resources)?;

            let mut matte_pixmap =
                Pixmap::new(pixmap.width(), pixmap.height()).ok_or_else(|| {
                    RasterError::Scene("failed to allocate track matte surface".into())
                })?;
            render_node(&mut matte_pixmap, matte, transform, 1.0, resources)?;

            let (mask_type, invert) = match mode {
                TrackMatteMode::Alpha => (MaskType::Alpha, false),
                TrackMatteMode::AlphaInverted => (MaskType::Alpha, true),
                TrackMatteMode::Luma => (MaskType::Luminance, false),
                TrackMatteMode::LumaInverted => (MaskType::Luminance, true),
            };

            if invert {
                match mode {
                    TrackMatteMode::AlphaInverted => {
                        for pixel in matte_pixmap.pixels_mut() {
                            let inv_a = 255 - pixel.alpha();
                            *pixel = PremultipliedColorU8::from_rgba(0, 0, 0, inv_a)
                                .unwrap_or(PremultipliedColorU8::TRANSPARENT);
                        }
                    }
                    TrackMatteMode::LumaInverted => {
                        for pixel in matte_pixmap.pixels_mut() {
                            let a = pixel.alpha();
                            if a == 0 {
                                *pixel =
                                    PremultipliedColorU8::from_rgba(255, 255, 255, 255).unwrap();
                            } else {
                                let r = 255 - pixel.red();
                                let g = 255 - pixel.green();
                                let b = 255 - pixel.blue();
                                *pixel = PremultipliedColorU8::from_rgba(r, g, b, 255)
                                    .unwrap_or(PremultipliedColorU8::TRANSPARENT);
                            }
                        }
                    }
                    _ => {}
                }
            }

            let mask = Mask::from_pixmap(matte_pixmap.as_ref(), mask_type);
            target_pixmap.apply_mask(&mask);

            let composite_paint = PixmapPaint {
                opacity: 1.0,
                blend_mode: tiny_skia::BlendMode::SourceOver,
                ..Default::default()
            };
            pixmap.draw_pixmap(
                0,
                0,
                target_pixmap.as_ref(),
                &composite_paint,
                Transform::identity(),
                None,
            );
        }

        SceneNode::FractalNoise {
            x,
            y,
            w,
            h,
            noise_type,
            fractal_type,
            contrast,
            brightness,
            complexity,
            scale_x,
            scale_y,
            evolution,
            seed,
            opacity: node_opacity,
        } => {
            let eff_opacity = opacity * node_opacity;
            if eff_opacity <= 0.0 || *w <= 0.0 || *h <= 0.0 {
                return Ok(());
            }
            let config = crate::vfx::FractalNoise {
                noise_type: *noise_type,
                fractal_type: *fractal_type,
                contrast: *contrast,
                brightness: *brightness,
                complexity: *complexity,
                scale_x: *scale_x,
                scale_y: *scale_y,
                evolution: *evolution,
                seed: *seed,
            };
            let noise_pixmap = crate::vfx::generate_fractal_noise_pixmap(
                (w.round() as u32).max(1),
                (h.round() as u32).max(1),
                &config,
            );
            let paint = PixmapPaint {
                opacity: eff_opacity.clamp(0.0, 1.0),
                ..Default::default()
            };
            pixmap.draw_pixmap(
                x.round() as i32,
                y.round() as i32,
                noise_pixmap.as_ref(),
                &paint,
                transform,
                None,
            );
        }

        SceneNode::AnimatedStroke {
            stroke,
            progress,
            color,
            opacity: node_opacity,
        } => {
            let eff_opacity = opacity * node_opacity;
            if eff_opacity <= 0.0 || *progress <= 0.0 || stroke.points.is_empty() {
                return Ok(());
            }

            let pts = interpolate_stroke_progress(&stroke.points, *progress);
            if pts.is_empty() {
                return Ok(());
            }

            let mut brush = stroke.brush.clone();
            if let Some(c) = color {
                brush.color = [
                    c.r as f32 / 255.0,
                    c.g as f32 / 255.0,
                    c.b as f32 / 255.0,
                    c.a as f32 / 255.0,
                ];
            }

            let mut surface =
                rust_motion_raster::Surface::new(rust_motion_color::PixelFormat::RGBA8);
            let damage =
                rust_motion_paint::render_stroke(&mut surface, &brush, &pts, None, false, 1.0);
            if damage.is_empty() {
                return Ok(());
            }

            let canvas_rect =
                rust_motion_geom::Rect::from_xywh(0, 0, pixmap.width(), pixmap.height());
            let r = damage.intersect(&canvas_rect);
            if r.is_empty() {
                return Ok(());
            }

            let mut buf = vec![[0u8; 4]; r.width() as usize * r.height() as usize];
            surface.read_rgba8_into(r, &mut buf);

            let mut premul = Vec::with_capacity(buf.len() * 4);
            for px in buf {
                let a = (px[3] as f32 / 255.0) * eff_opacity;
                let red = ((px[0] as f32 / 255.0) * a * 255.0).round() as u8;
                let grn = ((px[1] as f32 / 255.0) * a * 255.0).round() as u8;
                let blu = ((px[2] as f32 / 255.0) * a * 255.0).round() as u8;
                let alf = (a * 255.0).round() as u8;
                premul.extend_from_slice(&[red, grn, blu, alf]);
            }

            if let Some(patch) = PixmapRef::from_bytes(&premul, r.width(), r.height()) {
                let paint = PixmapPaint {
                    opacity: 1.0,
                    ..Default::default()
                };
                let patch_trans = transform.pre_translate(r.x0 as f32, r.y0 as f32);
                pixmap.draw_pixmap(0, 0, patch, &paint, patch_trans, None);
            }
        }

        SceneNode::VectorShape {
            path,
            fill,
            stroke_color,
            stroke_width,
            opacity: node_opacity,
        } => {
            let eff_opacity = opacity * node_opacity;
            if eff_opacity <= 0.0 {
                return Ok(());
            }

            let shape = rust_motion_doc::ShapeLayer {
                path: path.clone(),
                fill: fill.map(|c| {
                    rust_motion_doc::Fill::Solid(rust_motion_color::Color::rgba(
                        c.r as f32 / 255.0,
                        c.g as f32 / 255.0,
                        c.b as f32 / 255.0,
                        c.a as f32 / 255.0,
                    ))
                }),
                stroke: if let Some(sc) = stroke_color {
                    if *stroke_width > 0.0 {
                        Some(rust_motion_doc::ShapeStroke {
                            width: *stroke_width,
                            paint: rust_motion_doc::Fill::Solid(rust_motion_color::Color::rgba(
                                sc.r as f32 / 255.0,
                                sc.g as f32 / 255.0,
                                sc.b as f32 / 255.0,
                                sc.a as f32 / 255.0,
                            )),
                            ..Default::default()
                        })
                    } else {
                        None
                    }
                } else {
                    None
                },
                live: None,
                cache: None,
                psd_raw: None,
            };

            let canvas_rect =
                rust_motion_geom::Rect::from_xywh(0, 0, pixmap.width(), pixmap.height());
            let compiled = rust_motion_vector::CompiledShape::new(
                &shape,
                rust_motion_vector::DEFAULT_TOLERANCE,
                canvas_rect,
            );
            if let Some(bounds) = compiled.bounds() {
                let r = bounds.intersect(&canvas_rect);
                if !r.is_empty() {
                    let surface =
                        compiled.render(rust_motion_color::PixelFormat::RGBA8, canvas_rect);
                    let mut buf = vec![[0u8; 4]; r.width() as usize * r.height() as usize];
                    surface.read_rgba8_into(r, &mut buf);

                    let mut premul = Vec::with_capacity(buf.len() * 4);
                    for px in buf {
                        let a = (px[3] as f32 / 255.0) * eff_opacity;
                        let red = ((px[0] as f32 / 255.0) * a * 255.0).round() as u8;
                        let grn = ((px[1] as f32 / 255.0) * a * 255.0).round() as u8;
                        let blu = ((px[2] as f32 / 255.0) * a * 255.0).round() as u8;
                        let alf = (a * 255.0).round() as u8;
                        premul.extend_from_slice(&[red, grn, blu, alf]);
                    }

                    if let Some(patch) = PixmapRef::from_bytes(&premul, r.width(), r.height()) {
                        let paint = PixmapPaint {
                            opacity: 1.0,
                            ..Default::default()
                        };
                        let patch_trans = transform.pre_translate(r.x0 as f32, r.y0 as f32);
                        pixmap.draw_pixmap(0, 0, patch, &paint, patch_trans, None);
                    }
                }
            }
        }

        SceneNode::Scene3D {
            x,
            y,
            w,
            h,
            scene,
            opacity: node_opacity,
        } => {
            let eff_opacity = opacity * node_opacity;
            if eff_opacity > 0.0 {
                crate::scene3d::render_scene3d_software(
                    scene,
                    *x,
                    *y,
                    *w,
                    *h,
                    eff_opacity,
                    pixmap,
                    transform,
                );
            }
        }
    }
    Ok(())
}

pub(crate) fn interpolate_stroke_progress(
    points: &[rust_motion_paint::StrokePoint],
    progress: f32,
) -> Vec<rust_motion_paint::StrokePoint> {
    let p = progress.clamp(0.0, 1.0);
    if p <= 0.0 || points.is_empty() {
        return Vec::new();
    }
    if p >= 1.0 || points.len() == 1 {
        return points.to_vec();
    }

    let mut lengths = Vec::with_capacity(points.len());
    lengths.push(0.0);
    let mut total_len = 0.0;
    for i in 1..points.len() {
        let dx = points[i].x - points[i - 1].x;
        let dy = points[i].y - points[i - 1].y;
        let dist = (dx * dx + dy * dy).sqrt();
        total_len += dist;
        lengths.push(total_len);
    }

    if total_len <= 1e-6 {
        let take_n = ((points.len() as f32 * p).ceil() as usize).clamp(1, points.len());
        return points[..take_n].to_vec();
    }

    let target_len = total_len * (p as f64);
    let mut out = Vec::new();

    for i in 1..points.len() {
        out.push(points[i - 1]);
        if lengths[i] >= target_len {
            let seg_len = lengths[i] - lengths[i - 1];
            let t = if seg_len > 1e-6 {
                ((target_len - lengths[i - 1]) / seg_len).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let p0 = &points[i - 1];
            let p1 = &points[i];
            let interp = rust_motion_paint::StrokePoint {
                x: p0.x + (p1.x - p0.x) * t,
                y: p0.y + (p1.y - p0.y) * t,
                pressure: (p0.pressure + (p1.pressure - p0.pressure) * (t as f32)).clamp(0.0, 1.0),
                tilt_x: p0.tilt_x + (p1.tilt_x - p0.tilt_x) * (t as f32),
                tilt_y: p0.tilt_y + (p1.tilt_y - p0.tilt_y) * (t as f32),
                rotation: p0.rotation + (p1.rotation - p0.rotation) * (t as f32),
                wheel: p0.wheel + (p1.wheel - p0.wheel) * (t as f32),
                time: p0.time + (p1.time - p0.time) * t,
            };
            out.push(interp);
            return out;
        }
    }

    points.to_vec()
}

pub(crate) fn render_clip_mask(
    width: u32,
    height: u32,
    clip: &ClipRegion,
    transform: Transform,
) -> Result<Mask, RasterError> {
    let mut clip_pixmap = Pixmap::new(width, height)
        .ok_or_else(|| RasterError::Scene("failed to allocate clip surface".into()))?;
    let path = match clip {
        ClipRegion::Rect {
            x,
            y,
            w,
            h,
            corner_radius,
        } => {
            let Some(rect) = Rect::from_xywh(*x, *y, *w, *h) else {
                return Mask::new(width, height).ok_or_else(|| {
                    RasterError::Scene("failed to allocate empty clip mask".into())
                });
            };
            if *corner_radius > 0.0 {
                build_rounded_rect(*x, *y, *w, *h, *corner_radius)
            } else {
                PathBuilder::from_rect(rect)
            }
        }
        ClipRegion::Path { d } => svgpath_to_tiny_skia(d)
            .ok_or_else(|| RasterError::Scene(format!("invalid SVG clip path: {d}")))?,
    };
    let mut paint = Paint::default();
    paint.set_color(tiny_skia::Color::WHITE);
    paint.anti_alias = true;
    clip_pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
    Ok(Mask::from_pixmap(clip_pixmap.as_ref(), MaskType::Alpha))
}
