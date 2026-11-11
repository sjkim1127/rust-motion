use super::types::*;
use crate::scene::{Color, GradientStop, Scene, SceneNode};
use crate::tiny_skia_backend::{svgpath_to_tiny_skia, TinySkiaBackend};
use lyon_tessellation::geometry_builder::{BuffersBuilder, VertexBuffers};
use lyon_tessellation::math::point;
use lyon_tessellation::path::Path as LyonPath;
use lyon_tessellation::{FillOptions, FillTessellator};
use tiny_skia::{Path as TinyPath, PathSegment, Stroke, Transform};

#[cfg(test)]
pub(crate) fn compile_scene(scene: &Scene, font: &TinySkiaBackend) -> Option<Vec<DrawCommand>> {
    compile_scene_with_path_mask(scene, font).map(|(commands, _, _)| commands)
}

pub(crate) fn compile_scene_with_path_mask(
    scene: &Scene,
    font: &TinySkiaBackend,
) -> Option<(Vec<DrawCommand>, Option<GpuPathMask>, u64)> {
    let mut commands = Vec::new();
    let mut path_mask = None;
    let mut asset_decode_ns = 0u64;
    compile_nodes(
        &scene.nodes,
        Transform::identity(),
        1.0,
        &mut commands,
        font,
        &mut path_mask,
        &mut asset_decode_ns,
    )?;
    if scene.nodes.iter().any(|node| {
        matches!(
            node,
            SceneNode::Layer {
                blend_mode: crate::scene::BlendMode::Multiply
                    | crate::scene::BlendMode::Screen
                    | crate::scene::BlendMode::Darken
                    | crate::scene::BlendMode::Lighten,
                ..
            }
        )
    }) {
        for command in &mut commands {
            command.instance_mut().kind_data[3] |= 2;
        }
    }
    Some((commands, path_mask, asset_decode_ns))
}

pub(crate) fn compile_nodes(
    nodes: &[SceneNode],
    transform: Transform,
    opacity: f32,
    output: &mut Vec<DrawCommand>,
    font: &TinySkiaBackend,
    path_mask: &mut Option<GpuPathMask>,
    asset_decode_ns: &mut u64,
) -> Option<()> {
    for node in nodes {
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
                let active_stroke = stroke.filter(|_| *stroke_width > 0.0);
                let expansion = active_stroke
                    .map(|_| stroke_width * 0.5 + 1.0)
                    .unwrap_or(0.0);
                let mut instance = GpuInstance::solid(*fill, opacity, transform);
                instance.kind_data[0] = 0;
                instance.bounds = [
                    x - expansion,
                    y - expansion,
                    w + expansion * 2.0,
                    h + expansion * 2.0,
                ];
                instance.shape_bounds = [*x, *y, *w, *h];
                instance.color2 = color_to_f32(active_stroke.unwrap_or(*fill));
                instance.params[0] = *corner_radius;
                instance.params[1] = active_stroke.map(|_| *stroke_width).unwrap_or(0.0);
                output.push(DrawCommand::Analytic { instance });
            }

            SceneNode::Circle {
                cx,
                cy,
                r,
                fill,
                stroke,
                stroke_width,
            } => {
                let active_stroke = stroke.filter(|_| *stroke_width > 0.0);
                let expansion = active_stroke
                    .map(|_| stroke_width * 0.5 + 1.0)
                    .unwrap_or(0.0);
                let mut instance = GpuInstance::solid(*fill, opacity, transform);
                instance.kind_data[0] = 1;
                instance.bounds = [
                    cx - r - expansion,
                    cy - r - expansion,
                    r * 2.0 + expansion * 2.0,
                    r * 2.0 + expansion * 2.0,
                ];
                instance.shape_bounds = [cx - r, cy - r, r * 2.0, r * 2.0];
                instance.color2 = color_to_f32(active_stroke.unwrap_or(*fill));
                instance.params[1] = active_stroke.map(|_| *stroke_width).unwrap_or(0.0);
                output.push(DrawCommand::Analytic { instance });
            }

            SceneNode::Path {
                d,
                fill,
                stroke,
                stroke_width,
                opacity: node_opacity,
            } => {
                let path = svgpath_to_tiny_skia(d)?;
                let combined_opacity = opacity * node_opacity;
                if let Some(fill) = fill {
                    output.push(mesh_command(&path, *fill, combined_opacity, transform)?);
                }
                if let Some(stroke) = stroke.filter(|_| *stroke_width > 0.0) {
                    let stroked = path.stroke(
                        &Stroke {
                            width: *stroke_width,
                            ..Default::default()
                        },
                        transform
                            .get_scale()
                            .0
                            .max(transform.get_scale().1)
                            .max(1.0),
                    )?;
                    output.push(mesh_command(&stroked, stroke, combined_opacity, transform)?);
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
                    continue;
                }
                let mut instance = gradient_instance(stops, opacity, transform)?;
                instance.kind_data[0] = 2;
                instance.bounds = [*x, *y, *w, *h];
                instance.shape_bounds = instance.bounds;
                instance.params[2] = *angle_deg;
                output.push(DrawCommand::Analytic { instance });
            }

            SceneNode::RadialGradient { cx, cy, r, stops } => {
                if stops.is_empty() {
                    continue;
                }
                let mut instance = gradient_instance(stops, opacity, transform)?;
                instance.kind_data[0] = 3;
                instance.bounds = [cx - r, cy - r, r * 2.0, r * 2.0];
                instance.shape_bounds = instance.bounds;
                output.push(DrawCommand::Analytic { instance });
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
                if ![*x, *y, *w, *h, *node_opacity]
                    .iter()
                    .all(|value| value.is_finite())
                    || *w <= 0.0
                    || *h <= 0.0
                    || *node_opacity < 0.0
                {
                    return None;
                }
                let mut instance =
                    GpuInstance::solid(Color::WHITE, opacity * *node_opacity, transform);
                instance.kind_data[0] = 5;
                instance.bounds = [*x, *y, *w, *h];
                instance.shape_bounds = instance.bounds;
                instance.params = [0.0, 0.0, 1.0, 1.0];
                output.push(DrawCommand::Image {
                    instance,
                    src: src.clone(),
                    fit: *fit,
                });
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
                ..
            } => {
                if ![*x, *y, *w, *h, *node_opacity]
                    .iter()
                    .all(|value| value.is_finite())
                    || *w <= 0.0
                    || *h <= 0.0
                    || !time.is_finite()
                    || *time < 0.0
                    || *node_opacity < 0.0
                {
                    return None;
                }
                let mut instance =
                    GpuInstance::solid(Color::WHITE, opacity * *node_opacity, transform);
                instance.kind_data[0] = 5;
                instance.bounds = [*x, *y, *w, *h];
                instance.shape_bounds = instance.bounds;
                instance.params = [0.0, 0.0, 1.0, 1.0];
                output.push(DrawCommand::Video {
                    instance,
                    src: src.clone(),
                    time: *time,
                    looped: *looped,
                    fit: *fit,
                });
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
                if ![*x, *y, *w, *h, *playback_rate, *node_opacity]
                    .iter()
                    .all(|value| value.is_finite())
                    || !time.is_finite()
                    || *w <= 0.0
                    || *h <= 0.0
                    || *playback_rate <= 0.0
                    || *node_opacity < 0.0
                    || *node_opacity > 1.0
                {
                    return None;
                }
                let playback_time = *time * f64::from(*playback_rate);
                let target_w = w.round().max(1.0) as u32;
                let target_h = h.round().max(1.0) as u32;
                let t_start = std::time::Instant::now();
                let image = font
                    .lottie_frame(src, playback_time, target_w, target_h, *loop_behavior)
                    .ok()?;
                *asset_decode_ns += t_start.elapsed().as_nanos() as u64;
                let mut instance =
                    GpuInstance::solid(Color::WHITE, opacity * *node_opacity, transform);
                instance.kind_data[0] = 5;
                instance.bounds = [*x, *y, *w, *h];
                instance.shape_bounds = instance.bounds;
                instance.params = [0.0, 0.0, 1.0, 1.0];
                let key = format!(
                    "lottie:{src}:{playback_time:.9}:{loop_behavior:?}:{target_w}x{target_h}"
                );
                output.push(DrawCommand::Lottie {
                    instance,
                    key,
                    image,
                });
            }

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
                if ![*x, *y, *w, *h, *node_opacity, *playback_rate]
                    .iter()
                    .all(|value| value.is_finite())
                    || *w <= 0.0
                    || *h <= 0.0
                    || !time.is_finite()
                    || *time < 0.0
                    || *node_opacity < 0.0
                    || *node_opacity > 1.0
                    || *playback_rate <= 0.0
                {
                    return None;
                }
                let playback_time = *time * f64::from(*playback_rate);
                let t_start = std::time::Instant::now();
                let frame_res = font.gif_frame_with_index(src, playback_time, *loop_behavior);
                *asset_decode_ns += t_start.elapsed().as_nanos() as u64;
                let (frame_index, image) = frame_res.ok().flatten()?;
                let mut instance =
                    GpuInstance::solid(Color::WHITE, opacity * *node_opacity, transform);
                instance.kind_data[0] = 5;
                instance.bounds = [*x, *y, *w, *h];
                instance.shape_bounds = instance.bounds;
                instance.params = [0.0, 0.0, 1.0, 1.0];
                let key = format!("gif:{src}:{frame_index}");
                output.push(DrawCommand::Gif {
                    instance,
                    key,
                    image,
                    fit: *fit,
                });
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
                if ![*x, *y, *font_size].iter().all(|v| v.is_finite()) || *font_size <= 0.0 {
                    return None;
                }
                let t_start = std::time::Instant::now();
                let rendered =
                    font.rasterize_text(content, *font_size, *font_weight, font_sources)?;
                *asset_decode_ns += t_start.elapsed().as_nanos() as u64;
                let mut instance = GpuInstance::solid(*color, opacity, transform);
                instance.kind_data[0] = 6;
                instance.bounds = [
                    *x,
                    *y - rendered.baseline as f32,
                    rendered.width as f32,
                    rendered.height as f32,
                ];
                instance.shape_bounds = instance.bounds;
                let key = format!(
                    "{}:{}:{}:{:?}",
                    content,
                    font_size.to_bits(),
                    font_weight,
                    font_sources
                );
                let entry = font.text_atlas_entry(&key)?;
                output.push(DrawCommand::Text { instance, entry });
            }

            SceneNode::Group {
                transform: group_transform,
                opacity: group_opacity,
                children,
            } => {
                let next_transform = transform.post_concat(group_transform.to_tiny_skia());
                if !next_transform.is_finite() || !group_opacity.is_finite() {
                    return None;
                }
                compile_nodes(
                    children,
                    next_transform,
                    opacity * group_opacity,
                    output,
                    font,
                    path_mask,
                    asset_decode_ns,
                )?;
            }

            SceneNode::Layer {
                opacity: layer_opacity,
                blend_mode: crate::scene::BlendMode::Normal,
                clip: Some(crate::scene::ClipRegion::Path { d }),
                mask: None,
                mask_mode: crate::scene::MaskMode::Alpha,
                filters,
                shadow: None,
                children,
                ..
            } if path_mask.is_none()
                && gpu_layer_effects(filters, *layer_opacity).is_some()
                && gpu_path_mask_from_svg(d, transform, Color::WHITE).is_some() =>
            {
                let (layer_opacity, brightness, grayscale, contrast, saturation, invert, hue) =
                    gpu_layer_effects(filters, *layer_opacity).unwrap();
                let start = output.len();
                compile_nodes(
                    children,
                    transform,
                    opacity * layer_opacity,
                    output,
                    font,
                    path_mask,
                    asset_decode_ns,
                )?;
                *path_mask = gpu_path_mask_from_svg(d, transform, Color::WHITE);
                for command in &mut output[start..] {
                    let instance = command.instance_mut();
                    instance.kind_data[3] = 1;
                    instance.brightness[0] *= brightness;
                    instance.grayscale[0] = 1.0 - (1.0 - instance.grayscale[0]) * (1.0 - grayscale);
                    instance.contrast[0] *= contrast;
                    instance.saturation[0] *= saturation;
                    instance.invert[0] = invert;
                    instance.hue[0] = hue;
                    if let Some(tint) = gpu_tint(filters) {
                        instance.tint = tint;
                    }
                    if let Some((primary, secondary)) = gpu_duotone(filters) {
                        instance.duotone_primary = primary;
                        instance.duotone_secondary = secondary;
                    }
                    if let Some((grading, grading_tint)) = gpu_color_grading(filters) {
                        instance.grading = grading;
                        instance.grading_tint = grading_tint;
                    }
                    if let Some(vignette) = gpu_vignette(filters) {
                        instance.vignette = vignette;
                    }
                }
            }

            SceneNode::Layer {
                opacity: layer_opacity,
                clip: None,
                mask: Some(mask_nodes),
                blend_mode,
                mask_mode,
                filters,
                shadow: None,
                children,
                ..
            } if path_mask.is_none()
                && mask_nodes.len() == 1
                && matches!(mask_nodes.first(), Some(SceneNode::Path { .. }))
                && (*mask_mode == crate::scene::MaskMode::Alpha
                    || *mask_mode == crate::scene::MaskMode::Luminance)
                && matches!(
                    blend_mode,
                    crate::scene::BlendMode::Normal | crate::scene::BlendMode::Multiply
                )
                && (matches!(blend_mode, crate::scene::BlendMode::Normal)
                    || gpu_blend_layer_filters_supported(filters))
                && gpu_blend_children_supported(children)
                && gpu_layer_effects(filters, *layer_opacity).is_some()
                && gpu_path_mask_from_node(mask_nodes.first().unwrap(), transform, *mask_mode)
                    .is_some() =>
            {
                let (layer_opacity, brightness, grayscale, contrast, saturation, invert, hue) =
                    gpu_layer_effects(filters, *layer_opacity).unwrap();
                let start = output.len();
                compile_nodes(
                    children,
                    transform,
                    opacity * layer_opacity,
                    output,
                    font,
                    path_mask,
                    asset_decode_ns,
                )?;
                *path_mask =
                    gpu_path_mask_from_node(mask_nodes.first().unwrap(), transform, *mask_mode);
                for command in &mut output[start..] {
                    let instance = command.instance_mut();
                    instance.kind_data[3] = 1;
                    instance.kind_data[2] = match blend_mode {
                        crate::scene::BlendMode::Multiply => 1,
                        crate::scene::BlendMode::Normal => 0,
                        _ => return None,
                    };
                    instance.brightness[0] *= brightness;
                    instance.grayscale[0] = 1.0 - (1.0 - instance.grayscale[0]) * (1.0 - grayscale);
                    instance.contrast[0] *= contrast;
                    instance.saturation[0] *= saturation;
                    instance.invert[0] = invert;
                    instance.hue[0] = hue;
                    if let Some(tint) = gpu_tint(filters) {
                        instance.tint = tint;
                    }
                    if let Some((primary, secondary)) = gpu_duotone(filters) {
                        instance.duotone_primary = primary;
                        instance.duotone_secondary = secondary;
                    }
                    if let Some((grading, grading_tint)) = gpu_color_grading(filters) {
                        instance.grading = grading;
                        instance.grading_tint = grading_tint;
                    }
                    if let Some(vignette) = gpu_vignette(filters) {
                        instance.vignette = vignette;
                    }
                }
            }

            // A layer with no offscreen-only effect is semantically just a
            // group of GPU instances. Keep opacity and brightness filters on
            // the GPU while preserving child order; complex layers still take
            // the fallback path below so their compositing semantics remain exact.
            SceneNode::Layer {
                opacity: layer_opacity,
                blend_mode,
                clip,
                mask,
                mask_mode,
                filters,
                shadow: None,
                children,
                ..
            } if gpu_layer_effects(filters, *layer_opacity).is_some()
                && matches!(
                    blend_mode,
                    crate::scene::BlendMode::Normal
                        | crate::scene::BlendMode::Multiply
                        | crate::scene::BlendMode::Screen
                        | crate::scene::BlendMode::Darken
                        | crate::scene::BlendMode::Lighten
                )
                && ((matches!(blend_mode, crate::scene::BlendMode::Normal)
                    && (*layer_opacity >= 1.0
                        || gpu_normal_texture_layer_supported(children, transform)
                        || gpu_blend_children_supported(children)))
                    || (!matches!(blend_mode, crate::scene::BlendMode::Normal)
                        && gpu_blend_layer_filters_supported(filters)
                        && gpu_blend_children_supported(children)))
                && (*mask_mode == crate::scene::MaskMode::Alpha
                    || *mask_mode == crate::scene::MaskMode::Luminance)
                && (clip.is_none()
                    || gpu_clip_mask_info(clip.as_ref().unwrap(), transform).is_some())
                && (mask.is_none()
                    || gpu_mask_shapes(mask.as_deref().unwrap_or(&[]), transform, *mask_mode)
                        .is_some()) =>
            {
                let (layer_opacity, brightness, grayscale, contrast, saturation, invert, hue) =
                    gpu_layer_effects(filters, *layer_opacity).unwrap();
                let mask_info = mask
                    .as_deref()
                    .and_then(|nodes| gpu_mask_shapes(nodes, transform, *mask_mode));
                let mask_info = match (
                    mask_info,
                    clip.as_ref()
                        .and_then(|value| gpu_clip_mask_info(value, transform)),
                ) {
                    (Some(mask_info), Some(clip_info)) => merge_gpu_mask_info(mask_info, clip_info),
                    (Some(mask_info), None) => Some(mask_info),
                    (None, Some(clip_info)) => Some(clip_info),
                    (None, None) => None,
                };
                let start = output.len();
                compile_nodes(
                    children,
                    transform,
                    opacity * layer_opacity,
                    output,
                    font,
                    path_mask,
                    asset_decode_ns,
                )?;
                for command in &mut output[start..] {
                    let instance = command.instance_mut();
                    instance.kind_data[2] = match blend_mode {
                        crate::scene::BlendMode::Multiply => 1,
                        crate::scene::BlendMode::Screen => 2,
                        crate::scene::BlendMode::Darken => 3,
                        crate::scene::BlendMode::Lighten => 4,
                        _ => 0,
                    };
                    instance.brightness[0] *= brightness;
                    instance.grayscale[0] = 1.0 - (1.0 - instance.grayscale[0]) * (1.0 - grayscale);
                    instance.contrast[0] *= contrast;
                    instance.saturation[0] *= saturation;
                    instance.invert[0] = invert;
                    instance.hue[0] = hue;
                    if let Some(tint) = gpu_tint(filters) {
                        instance.tint = tint;
                    }
                    if let Some((primary, secondary)) = gpu_duotone(filters) {
                        instance.duotone_primary = primary;
                        instance.duotone_secondary = secondary;
                    }
                    if let Some((grading, grading_tint)) = gpu_color_grading(filters) {
                        instance.grading = grading;
                        instance.grading_tint = grading_tint;
                    }
                    if let Some(vignette) = gpu_vignette(filters) {
                        instance.vignette = vignette;
                    }
                    if let Some(mask_info) = mask_info {
                        apply_gpu_mask_info(instance, mask_info);
                    }
                }
            }

            SceneNode::Layer {
                opacity: layer_opacity,
                blend_mode: crate::scene::BlendMode::Normal,
                clip: Some(clip),
                mask: None,
                mask_mode: crate::scene::MaskMode::Alpha,
                filters,
                shadow: None,
                children,
                ..
            } if gpu_layer_effects(filters, *layer_opacity).is_some()
                && gpu_clip_mask_info(clip, transform).is_some() =>
            {
                let (layer_opacity, brightness, grayscale, contrast, saturation, invert, hue) =
                    gpu_layer_effects(filters, *layer_opacity).unwrap();
                let mask_info = gpu_clip_mask_info(clip, transform);
                let start = output.len();
                compile_nodes(
                    children,
                    transform,
                    opacity * layer_opacity,
                    output,
                    font,
                    path_mask,
                    asset_decode_ns,
                )?;
                for command in &mut output[start..] {
                    let instance = command.instance_mut();
                    instance.brightness[0] *= brightness;
                    instance.grayscale[0] = 1.0 - (1.0 - instance.grayscale[0]) * (1.0 - grayscale);
                    instance.contrast[0] *= contrast;
                    instance.saturation[0] *= saturation;
                    instance.invert[0] = invert;
                    instance.hue[0] = hue;
                    if let Some(tint) = gpu_tint(filters) {
                        instance.tint = tint;
                    }
                    if let Some((primary, secondary)) = gpu_duotone(filters) {
                        instance.duotone_primary = primary;
                        instance.duotone_secondary = secondary;
                    }
                    if let Some((grading, grading_tint)) = gpu_color_grading(filters) {
                        instance.grading = grading;
                        instance.grading_tint = grading_tint;
                    }
                    if let Some(vignette) = gpu_vignette(filters) {
                        instance.vignette = vignette;
                    }
                    if let Some(mask_info) = mask_info {
                        apply_gpu_mask_info(instance, mask_info);
                    }
                }
            }

            SceneNode::Emoji {
                emoji,
                x,
                y,
                size,
                opacity: node_opacity,
            } => {
                if ![*x, *y, *size, *node_opacity]
                    .iter()
                    .all(|value| value.is_finite())
                    || *size <= 0.0
                    || *node_opacity < 0.0
                    || *node_opacity > 1.0
                {
                    return None;
                }
                let size_px = size.round().clamp(8.0, 1024.0) as u32;
                let image = crate::emoji::render_emoji(emoji, size_px)?;
                let mut instance =
                    GpuInstance::solid(Color::WHITE, opacity * *node_opacity, transform);
                instance.kind_data[0] = 5;
                instance.bounds = [*x, *y, *size, *size];
                instance.shape_bounds = instance.bounds;
                instance.params = [0.0, 0.0, 1.0, 1.0];
                output.push(DrawCommand::Emoji {
                    instance,
                    key: format!("emoji:{emoji}:{size_px}"),
                    image,
                });
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
                if ![*x, *y, *width, *height, *node_opacity]
                    .iter()
                    .all(|value| value.is_finite())
                    || *width <= 0.0
                    || *height <= 0.0
                    || !time.is_finite()
                    || *time < 0.0
                    || *node_opacity < 0.0
                    || *node_opacity > 1.0
                {
                    return None;
                }
                let target_w = width.round().max(1.0) as u32;
                let target_h = height.round().max(1.0) as u32;
                let image = font
                    .audio_visualizer_frame(src, target_w, target_h, *color, style, *time)
                    .ok()?;
                let mut instance =
                    GpuInstance::solid(Color::WHITE, opacity * *node_opacity, transform);
                instance.kind_data[0] = 5;
                instance.bounds = [*x, *y, *width, *height];
                instance.shape_bounds = instance.bounds;
                instance.params = [0.0, 0.0, 1.0, 1.0];
                output.push(DrawCommand::AudioVisualizer {
                    instance,
                    key: format!("audio-visualizer:{src}:{time:.9}:{target_w}x{target_h}"),
                    image,
                });
            }

            SceneNode::Audio { .. } => {}
            SceneNode::Layer { .. } | SceneNode::Shader { .. } => return None,
            _ => return None,
        }
    }
    Some(())
}

pub(crate) fn gpu_fallback_reason(scene: &Scene) -> &'static str {
    if scene.nodes.iter().any(|node| {
        matches!(
            node,
            SceneNode::Layer {
                opacity,
                blend_mode: crate::scene::BlendMode::Normal,
                children,
                ..
            } if *opacity < 1.0
                && !gpu_normal_texture_layer_supported(children, Transform::identity())
                && children.iter().any(|child| matches!(
                    child,
                    SceneNode::Image { .. }
                        | SceneNode::Video { .. }
                        | SceneNode::Lottie { .. }
                        | SceneNode::Group { .. }
                ))
        )
    }) {
        return "overlapping texture layer requires offscreen compositing";
    }
    "scene contains GPU-unsupported nodes or effects"
}

pub(crate) fn gpu_layer_effects(
    filters: &[crate::scene::SceneFilter],
    layer_opacity: f32,
) -> Option<(f32, f32, f32, f32, f32, f32, f32)> {
    if !layer_opacity.is_finite() {
        return None;
    }
    if filters
        .iter()
        .any(|filter| matches!(filter, crate::scene::SceneFilter::ColorGrading { .. }))
        && filters.iter().any(|filter| {
            matches!(
                filter,
                crate::scene::SceneFilter::Brightness { .. }
                    | crate::scene::SceneFilter::Grayscale { .. }
                    | crate::scene::SceneFilter::Contrast { .. }
                    | crate::scene::SceneFilter::Saturation { .. }
                    | crate::scene::SceneFilter::Invert { .. }
                    | crate::scene::SceneFilter::HueRotate { .. }
                    | crate::scene::SceneFilter::Tint { .. }
                    | crate::scene::SceneFilter::Duotone { .. }
            )
        })
    {
        // ColorGrading has its own gamma/contrast/saturation/tint order in
        // TinySkia. Do not claim GPU support for a chain whose CPU order
        // cannot be represented by the packed instance fields.
        return None;
    }
    filters.iter().try_fold(
        (layer_opacity, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0),
        |(opacity, brightness, grayscale, contrast, saturation, invert, hue), filter| match filter {
            crate::scene::SceneFilter::Opacity { amount }
                if amount.is_finite() && (0.0..=1.0).contains(amount) =>
            {
                Some((
                    opacity * amount,
                    brightness,
                    grayscale,
                    contrast,
                    saturation,
                    invert,
                    hue,
                ))
            }
            crate::scene::SceneFilter::Brightness { amount }
                if amount.is_finite() && (0.0..=10.0).contains(amount) =>
            {
                Some((
                    opacity,
                    brightness * amount,
                    grayscale,
                    contrast,
                    saturation,
                    invert,
                    hue,
                ))
            }
            crate::scene::SceneFilter::Grayscale { amount }
                if amount.is_finite() && (0.0..=1.0).contains(amount) =>
            {
                Some((
                    opacity,
                    brightness,
                    grayscale + amount - grayscale * amount,
                    contrast,
                    saturation,
                    invert,
                    hue,
                ))
            }
            crate::scene::SceneFilter::Contrast { factor }
                if factor.is_finite() && *factor >= 0.0 =>
            {
                Some((
                    opacity,
                    brightness,
                    grayscale,
                    contrast * factor,
                    saturation,
                    invert,
                    hue,
                ))
            }
            crate::scene::SceneFilter::Saturation { factor }
                if factor.is_finite() && *factor >= 0.0 =>
            {
                Some((
                    opacity,
                    brightness,
                    grayscale,
                    contrast,
                    saturation * factor,
                    invert,
                    hue,
                ))
            }
            crate::scene::SceneFilter::Vignette {
                offset,
                darkness,
                roundness,
            } if offset.is_finite()
                && darkness.is_finite()
                && roundness.is_finite()
                && (0.0..=2.0).contains(offset)
                && (0.0..=1.0).contains(darkness)
                && (0.0..=1.0).contains(roundness) =>
            {
                Some((
                    opacity, brightness, grayscale, contrast, saturation, invert, hue,
                ))
            }
            crate::scene::SceneFilter::Invert { amount }
                if amount.is_finite() && (0.0..=1.0).contains(amount) =>
            {
                // Applying two invert filters sequentially composes their
                // affine color transforms: I_b(I_a(x)) has amount
                // a + b - 2ab. Preserve that composition instead of
                // silently dropping every preceding invert filter.
                Some((
                    opacity,
                    brightness,
                    grayscale,
                    contrast,
                    saturation,
                    invert + *amount - 2.0 * invert * *amount,
                    hue,
                ))
            }
            crate::scene::SceneFilter::HueRotate { degrees } if degrees.is_finite() => Some((
                opacity,
                brightness,
                grayscale,
                contrast,
                saturation,
                invert,
                hue + *degrees,
            )),
            crate::scene::SceneFilter::Tint { color, amount }
                if amount.is_finite() && (0.0..=1.0).contains(amount) =>
            {
                Some((
                    opacity, brightness, grayscale, contrast, saturation, invert, hue,
                ))
            }
            crate::scene::SceneFilter::Duotone { .. } => Some((
                opacity, brightness, grayscale, contrast, saturation, invert, hue,
            )),
            crate::scene::SceneFilter::ColorGrading {
                contrast: grading_contrast,
                saturation: grading_saturation,
                gamma,
                ..
            } if grading_contrast.is_finite()
                && *grading_contrast >= 0.0
                && grading_saturation.is_finite()
                && *grading_saturation >= 0.0
                && gamma.is_finite()
                && *gamma > 0.0 =>
            {
                Some((
                    opacity, brightness, grayscale, contrast, saturation, invert, hue,
                ))
            }
            _ => None,
        },
    )
}

/// Non-normal blend modes operate directly on the ordered child draw calls,
/// rather than an offscreen layer. Opacity is safe in that representation
/// because it composes into each child instance; color-changing filters are
/// not, since they require filtering the already-composited layer.
pub(crate) fn gpu_blend_layer_filters_supported(filters: &[crate::scene::SceneFilter]) -> bool {
    filters.iter().all(|filter| {
        matches!(
            filter,
            crate::scene::SceneFilter::Opacity { amount }
                if amount.is_finite() && (0.0..=1.0).contains(amount)
        )
    })
}

fn gpu_tint(filters: &[crate::scene::SceneFilter]) -> Option<[f32; 4]> {
    let mut tint = None;
    for filter in filters {
        if let crate::scene::SceneFilter::Tint { color, amount } = filter {
            if !amount.is_finite() || !(0.0..=1.0).contains(amount) {
                return None;
            }
            tint = Some([
                f32::from(color[0]) / 255.0,
                f32::from(color[1]) / 255.0,
                f32::from(color[2]) / 255.0,
                f32::from(color[3]) / 255.0 * *amount,
            ]);
        }
    }
    tint
}

fn gpu_duotone(filters: &[crate::scene::SceneFilter]) -> Option<([f32; 4], [f32; 4])> {
    filters.iter().find_map(|filter| {
        if let crate::scene::SceneFilter::Duotone { primary, secondary } = filter {
            Some((
                [
                    f32::from(primary[0]) / 255.0,
                    f32::from(primary[1]) / 255.0,
                    f32::from(primary[2]) / 255.0,
                    1.0,
                ],
                [
                    f32::from(secondary[0]) / 255.0,
                    f32::from(secondary[1]) / 255.0,
                    f32::from(secondary[2]) / 255.0,
                    0.0,
                ],
            ))
        } else {
            None
        }
    })
}

fn gpu_color_grading(filters: &[crate::scene::SceneFilter]) -> Option<([f32; 4], [f32; 4])> {
    filters.iter().find_map(|filter| {
        if let crate::scene::SceneFilter::ColorGrading {
            contrast,
            saturation,
            gamma,
            tint,
        } = filter
        {
            if !contrast.is_finite()
                || *contrast < 0.0
                || !saturation.is_finite()
                || *saturation < 0.0
                || !gamma.is_finite()
                || *gamma <= 0.0
            {
                return None;
            }
            let tint = tint.map_or([0.0; 4], |color| {
                [
                    f32::from(color[0]) / 255.0,
                    f32::from(color[1]) / 255.0,
                    f32::from(color[2]) / 255.0,
                    f32::from(color[3]) / 255.0,
                ]
            });
            Some(([*contrast, *saturation, 1.0 / *gamma, 1.0], tint))
        } else {
            None
        }
    })
}

pub(crate) fn gpu_blend_children_supported(nodes: &[SceneNode]) -> bool {
    nodes.iter().all(|node| match node {
        SceneNode::Rect { .. }
        | SceneNode::Circle { .. }
        | SceneNode::Path { .. }
        | SceneNode::LinearGradient { .. }
        | SceneNode::RadialGradient { .. }
        | SceneNode::Text { .. } => true,
        SceneNode::Group { children, .. } => gpu_blend_children_supported(children),
        SceneNode::Layer { .. }
        | SceneNode::Image { .. }
        | SceneNode::Video { .. }
        | SceneNode::Lottie { .. }
        | SceneNode::Gif { .. }
        | SceneNode::Emoji { .. }
        | SceneNode::Audio { .. }
        | SceneNode::AudioVisualizer { .. }
        | SceneNode::Shader { .. } => false,
        _ => false,
    })
}

/// Texture children can carry the layer opacity directly on their GPU
/// instances when their axis-aligned bounds do not overlap. In that case the
/// group opacity cannot change a pixel more than once, so this is equivalent
/// to compositing the group into an offscreen surface first. Overlapping or
/// transformed children still require an offscreen layer.
pub(crate) fn gpu_normal_texture_layer_supported(nodes: &[SceneNode], parent: Transform) -> bool {
    let mut bounds = Vec::with_capacity(nodes.len());
    fn collect(nodes: &[SceneNode], transform: Transform, bounds: &mut Vec<[f32; 4]>) -> bool {
        for node in nodes {
            match node {
                SceneNode::Image { x, y, w, h, .. }
                | SceneNode::Video { x, y, w, h, .. }
                | SceneNode::Lottie { x, y, w, h, .. } => {
                    if ![*x, *y, *w, *h].iter().all(|value| value.is_finite())
                        || *w <= 0.0
                        || *h <= 0.0
                    {
                        return false;
                    }
                    let mut corners = [
                        tiny_skia::Point::from_xy(*x, *y),
                        tiny_skia::Point::from_xy(*x + *w, *y),
                        tiny_skia::Point::from_xy(*x, *y + *h),
                        tiny_skia::Point::from_xy(*x + *w, *y + *h),
                    ];
                    for corner in &mut corners {
                        transform.map_point(corner);
                    }
                    let min_x = corners
                        .iter()
                        .map(|point| point.x)
                        .fold(f32::INFINITY, f32::min);
                    let min_y = corners
                        .iter()
                        .map(|point| point.y)
                        .fold(f32::INFINITY, f32::min);
                    let max_x = corners
                        .iter()
                        .map(|point| point.x)
                        .fold(f32::NEG_INFINITY, f32::max);
                    let max_y = corners
                        .iter()
                        .map(|point| point.y)
                        .fold(f32::NEG_INFINITY, f32::max);
                    if ![min_x, min_y, max_x, max_y]
                        .iter()
                        .all(|value| value.is_finite())
                    {
                        return false;
                    }
                    bounds.push([min_x, min_y, max_x - min_x, max_y - min_y]);
                }
                SceneNode::Group {
                    transform: group_transform,
                    children,
                    ..
                } => {
                    if !collect(
                        children,
                        transform.post_concat(group_transform.to_tiny_skia()),
                        bounds,
                    ) {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        true
    }
    if !collect(nodes, parent, &mut bounds) {
        return false;
    }
    if bounds.is_empty() {
        return false;
    }
    for (index, first) in bounds.iter().enumerate() {
        for second in &bounds[index + 1..] {
            let separated = first[0] + first[2] <= second[0]
                || second[0] + second[2] <= first[0]
                || first[1] + first[3] <= second[1]
                || second[1] + second[3] <= first[1];
            if !separated {
                return false;
            }
        }
    }
    true
}

/// Return the narrow overlap case that can be rendered with one offscreen
/// texture and one explicit composite pass. More complex layer semantics stay
/// on the CPU path until their ordering and masking rules are implemented.
pub(crate) fn trailing_overlap_texture_layer(scene: &Scene) -> Option<(Scene, Scene, f32)> {
    let SceneNode::Layer {
        opacity,
        blend_mode: crate::scene::BlendMode::Normal,
        clip: None,
        mask: None,
        filters,
        shadow: None,
        children,
        ..
    } = scene.nodes.last()?
    else {
        return None;
    };
    if *opacity >= 1.0
        || !opacity.is_finite()
        || !filters.iter().all(|filter| {
            matches!(
                filter,
                crate::scene::SceneFilter::Opacity { amount }
                    if amount.is_finite() && (0.0..=1.0).contains(amount)
            )
        })
        || children.len() < 2
        || gpu_normal_texture_layer_supported(children, Transform::identity())
    {
        return None;
    }
    if !children.iter().any(|node| {
        matches!(
            node,
            SceneNode::Image { .. }
                | SceneNode::Video { .. }
                | SceneNode::Lottie { .. }
                | SceneNode::Group { .. }
        )
    }) {
        return None;
    }
    Some((
        Scene {
            nodes: scene.nodes[..scene.nodes.len() - 1].to_vec(),
        },
        Scene {
            nodes: children.clone(),
        },
        filters
            .iter()
            .fold(*opacity, |opacity, filter| match filter {
                crate::scene::SceneFilter::Opacity { amount } => opacity * amount,
                _ => opacity,
            }),
    ))
}

fn gpu_vignette(filters: &[crate::scene::SceneFilter]) -> Option<[f32; 4]> {
    let mut result = None;
    for filter in filters {
        if let crate::scene::SceneFilter::Vignette {
            offset,
            darkness,
            roundness,
        } = filter
        {
            if !offset.is_finite()
                || !darkness.is_finite()
                || !roundness.is_finite()
                || !(0.0..=2.0).contains(offset)
                || !(0.0..=1.0).contains(darkness)
                || !(0.0..=1.0).contains(roundness)
            {
                return None;
            }
            result = Some([*offset, *darkness, *roundness, 1.0]);
        }
    }
    result
}

type GpuMaskInfo = (
    [[f32; 4]; 4],
    [f32; 4],
    [u32; 4],
    [[f32; 4]; 4],
    [[f32; 4]; 4],
    [[f32; 4]; 4],
    [[f32; 4]; 4],
    [[f32; 4]; 4],
    [[f32; 4]; 4],
    [u32; 4],
);

fn gpu_clip_mask_info(
    clip: &crate::scene::ClipRegion,
    transform: Transform,
) -> Option<GpuMaskInfo> {
    let crate::scene::ClipRegion::Rect {
        x,
        y,
        w,
        h,
        corner_radius,
    } = clip
    else {
        return None;
    };
    let node = SceneNode::Rect {
        x: *x,
        y: *y,
        w: *w,
        h: *h,
        fill: Color::WHITE,
        stroke: None,
        stroke_width: 0.0,
        corner_radius: *corner_radius,
    };
    let mut info = gpu_mask_shapes(&[node], transform, crate::scene::MaskMode::Alpha)?;
    info.2.fill(6);
    Some(info)
}

fn merge_gpu_mask_info(mut base: GpuMaskInfo, extra: GpuMaskInfo) -> Option<GpuMaskInfo> {
    let (
        base_rects,
        base_opacity,
        base_kinds,
        base_shapes,
        base_color0,
        base_color1,
        base_color2,
        base_color3,
        base_positions,
        base_counts,
    ) = &mut base;
    let (
        extra_rects,
        extra_opacity,
        extra_kinds,
        extra_shapes,
        extra_color0,
        extra_color1,
        extra_color2,
        extra_color3,
        extra_positions,
        extra_counts,
    ) = extra;
    for index in 0..4 {
        if extra_opacity[index] < 0.0 {
            continue;
        }
        let slot = base_opacity.iter().position(|opacity| *opacity < 0.0)?;
        base_rects[slot] = extra_rects[index];
        base_opacity[slot] = extra_opacity[index];
        base_kinds[slot] = extra_kinds[index];
        base_shapes[slot] = extra_shapes[index];
        base_color0[slot] = extra_color0[index];
        base_color1[slot] = extra_color1[index];
        base_color2[slot] = extra_color2[index];
        base_color3[slot] = extra_color3[index];
        base_positions[slot] = extra_positions[index];
        base_counts[slot] = extra_counts[index];
    }
    Some(base)
}

fn apply_gpu_mask_info(instance: &mut GpuInstance, info: GpuMaskInfo) {
    let (
        clip_rects,
        mask_opacity,
        mask_kinds,
        mask_shapes,
        mask_color0,
        mask_color1,
        mask_color2,
        mask_color3,
        mask_stop_positions,
        mask_stop_counts,
    ) = info;
    instance.clip_rects = clip_rects;
    instance.mask_opacity = mask_opacity;
    instance.mask_kinds = mask_kinds;
    instance.mask_shapes = mask_shapes;
    instance.mask_color0 = mask_color0;
    instance.mask_color1 = mask_color1;
    instance.mask_color2 = mask_color2;
    instance.mask_color3 = mask_color3;
    instance.mask_stop_positions = mask_stop_positions;
    instance.mask_stop_counts = mask_stop_counts;
}

fn valid_gpu_gradient_stops(stops: &[crate::scene::GradientStop]) -> bool {
    if !(2..=4).contains(&stops.len())
        || (stops.first().map(|stop| stop.position) != Some(0.0))
        || (stops.last().map(|stop| stop.position) != Some(1.0))
    {
        return false;
    }
    stops.windows(2).all(|pair| {
        pair[0].position.is_finite()
            && pair[1].position.is_finite()
            && pair[0].position <= pair[1].position
    })
}

fn gpu_mask_shapes(
    mask: &[SceneNode],
    transform: Transform,
    mask_mode: crate::scene::MaskMode,
) -> Option<GpuMaskInfo> {
    if mask.is_empty() || mask.len() > 4 {
        return None;
    }
    let mut rects = [[-1.0; 4]; 4];
    let mut opacities = [-1.0; 4];
    let mut kinds = [0; 4];
    let mut shapes = [[0.0; 4]; 4];
    let mut colors0 = [[0.0; 4]; 4];
    let mut colors1 = [[0.0; 4]; 4];
    let mut colors2 = [[0.0; 4]; 4];
    let mut colors3 = [[0.0; 4]; 4];
    let mut stop_positions = [[0.0; 4]; 4];
    let mut stop_counts = [0; 4];
    for (index, node) in mask.iter().enumerate() {
        let (x, y, w, h, fill, kind, shape, color0, color1, color2, color3, positions, count) =
            match node {
                SceneNode::Rect {
                    x,
                    y,
                    w,
                    h,
                    fill,
                    stroke: None,
                    stroke_width,
                    corner_radius,
                } if [*x, *y, *w, *h, *stroke_width, *corner_radius]
                    .iter()
                    .all(|v| v.is_finite())
                    && *w > 0.0
                    && *h > 0.0
                    && *stroke_width == 0.0
                    && *corner_radius >= 0.0 =>
                {
                    let kind = if *corner_radius > 0.0 { 5 } else { 0 };
                    let shape = if kind == 5 {
                        [*corner_radius, 0.0, 0.0, 0.0]
                    } else {
                        [0.0; 4]
                    };
                    (
                        *x, *y, *w, *h, *fill, kind, shape, [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4],
                        [0.0; 4], 0,
                    )
                }
                SceneNode::Circle {
                    cx,
                    cy,
                    r,
                    fill,
                    stroke: None,
                    stroke_width,
                } if [*cx, *cy, *r, *stroke_width].iter().all(|v| v.is_finite())
                    && *r > 0.0
                    && *stroke_width == 0.0 =>
                {
                    (
                        *cx - *r,
                        *cy - *r,
                        *r * 2.0,
                        *r * 2.0,
                        *fill,
                        1,
                        [*cx, *cy, *r, 0.0],
                        [0.0; 4],
                        [0.0; 4],
                        [0.0; 4],
                        [0.0; 4],
                        [0.0; 4],
                        0,
                    )
                }
                SceneNode::LinearGradient {
                    x,
                    y,
                    w,
                    h,
                    angle_deg,
                    stops,
                } if [*x, *y, *w, *h, *angle_deg].iter().all(|v| v.is_finite())
                    && *w > 0.0
                    && *h > 0.0
                    && (2..=4).contains(&stops.len()) =>
                {
                    if !valid_gpu_gradient_stops(stops) {
                        return None;
                    }
                    let mut colors = [[0.0; 4]; 4];
                    let mut positions = [0.0; 4];
                    for (index, stop) in stops.iter().enumerate() {
                        colors[index] = color_to_f32(stop.color);
                        positions[index] = stop.position;
                    }
                    let half_diag = (*w * *w + *h * *h).sqrt() / 2.0;
                    let cx = *x + *w / 2.0;
                    let cy = *y + *h / 2.0;
                    let angle_rad = angle_deg.to_radians();
                    let dx = angle_rad.sin() * half_diag;
                    let dy = angle_rad.cos() * half_diag;
                    (
                        *x,
                        *y,
                        *w,
                        *h,
                        Color::WHITE,
                        2,
                        [cx - dx, cy - dy, cx + dx, cy + dy],
                        colors[0],
                        colors[1],
                        colors[2],
                        colors[3],
                        positions,
                        stops.len() as u32,
                    )
                }
                SceneNode::RadialGradient { cx, cy, r, stops }
                    if [*cx, *cy, *r].iter().all(|v| v.is_finite())
                        && *r > 0.0
                        && (2..=4).contains(&stops.len()) =>
                {
                    if !valid_gpu_gradient_stops(stops) {
                        return None;
                    }
                    let mut colors = [[0.0; 4]; 4];
                    let mut positions = [0.0; 4];
                    for (index, stop) in stops.iter().enumerate() {
                        colors[index] = color_to_f32(stop.color);
                        positions[index] = stop.position;
                    }
                    (
                        *cx - *r,
                        *cy - *r,
                        *r * 2.0,
                        *r * 2.0,
                        Color::WHITE,
                        4,
                        [*cx, *cy, *r, 0.0],
                        colors[0],
                        colors[1],
                        colors[2],
                        colors[3],
                        positions,
                        stops.len() as u32,
                    )
                }
                _ => return None,
            };
        let kind = if kind == 2 && mask_mode == crate::scene::MaskMode::Luminance {
            3
        } else {
            kind
        };
        let mask_opacity = match mask_mode {
            _ if kind == 2 => 1.0,
            crate::scene::MaskMode::Alpha if kind == 3 => 1.0,
            crate::scene::MaskMode::Luminance if kind == 2 => 1.0,
            crate::scene::MaskMode::Alpha => f32::from(fill.a) / 255.0,
            crate::scene::MaskMode::Luminance if fill.r == fill.g && fill.g == fill.b => {
                f32::from(fill.r) * f32::from(fill.a) / (255.0 * 255.0)
            }
            _ => return None,
        };
        // Translation and axis-aligned scale remain exact rectangular clips in
        // screen space. Rotation/shear falls back: a bounding box would overdraw.
        if !transform.sx.is_finite()
            || !transform.sy.is_finite()
            || !transform.tx.is_finite()
            || !transform.ty.is_finite()
            || transform.kx != 0.0
            || transform.ky != 0.0
        {
            return None;
        }
        if (kind == 1 || kind == 4 || kind == 5)
            && (transform.sx - transform.sy).abs() > f32::EPSILON
        {
            return None;
        }
        let x0 = x * transform.sx + transform.tx;
        let x1 = (x + w) * transform.sx + transform.tx;
        let y0 = y * transform.sy + transform.ty;
        let y1 = (y + h) * transform.sy + transform.ty;
        let width = (x1 - x0).abs();
        let height = (y1 - y0).abs();
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        rects[index] = [x0.min(x1), y0.min(y1), width, height];
        opacities[index] = mask_opacity;
        kinds[index] = kind;
        shapes[index] = if kind == 1 {
            [
                shape[0] * transform.sx + transform.tx,
                shape[1] * transform.sy + transform.ty,
                shape[2] * transform.sx.abs(),
                0.0,
            ]
        } else if kind == 2 || kind == 3 {
            [
                shape[0] * transform.sx + transform.tx,
                shape[1] * transform.sy + transform.ty,
                shape[2] * transform.sx + transform.tx,
                shape[3] * transform.sy + transform.ty,
            ]
        } else if kind == 4 {
            [
                shape[0] * transform.sx + transform.tx,
                shape[1] * transform.sy + transform.ty,
                shape[2] * transform.sx.abs(),
                0.0,
            ]
        } else if kind == 5 {
            [shape[0] * transform.sx.abs(), 0.0, 0.0, 0.0]
        } else {
            [0.0; 4]
        };
        colors0[index] = color0;
        colors1[index] = color1;
        colors2[index] = color2;
        colors3[index] = color3;
        stop_positions[index] = positions;
        stop_counts[index] = count;
    }
    Some((
        rects,
        opacities,
        kinds,
        shapes,
        colors0,
        colors1,
        colors2,
        colors3,
        stop_positions,
        stop_counts,
    ))
}

#[cfg(test)]
pub(crate) fn gpu_supports_scene(scene: &Scene) -> bool {
    if !scene.nodes.is_empty()
        && scene.nodes.iter().all(|node| {
            matches!(
                node,
                SceneNode::Shader {
                    x,
                    y,
                    w,
                    h,
                    opacity,
                    ..
                } if [*x, *y, *w, *h, *opacity]
                    .iter()
                    .all(|value| value.is_finite())
                    && *w > 0.0
                    && *h > 0.0
                    && (0.0..=1.0).contains(opacity)
            )
        })
    {
        return true;
    }
    compile_scene(scene, &TinySkiaBackend::new()).is_some()
}

fn gradient_instance(
    stops: &[GradientStop],
    opacity: f32,
    transform: Transform,
) -> Option<GpuInstance> {
    if stops.len() > MAX_GRADIENT_STOPS
        || stops.iter().any(|stop| !stop.position.is_finite())
        || !opacity.is_finite()
    {
        return None;
    }
    let mut sorted = stops.to_vec();
    sorted.sort_by(|left, right| left.position.total_cmp(&right.position));
    let mut instance = GpuInstance::solid(sorted[0].color, opacity, transform);
    instance.kind_data[1] = sorted.len() as u32;
    for (index, stop) in sorted.iter().enumerate() {
        instance.stop_positions[index][0] = stop.position.clamp(0.0, 1.0);
        instance.stop_colors[index] = color_to_f32(stop.color);
    }
    Some(instance)
}

fn mesh_command(
    path: &TinyPath,
    color: Color,
    opacity: f32,
    transform: Transform,
) -> Option<DrawCommand> {
    let lyon_path = tiny_path_to_lyon(path)?;
    let mut geometry: VertexBuffers<GpuVertex, u32> = VertexBuffers::new();
    FillTessellator::new()
        .tessellate_path(
            &lyon_path,
            &FillOptions::non_zero().with_tolerance(0.1),
            &mut BuffersBuilder::new(&mut geometry, PositionConstructor),
        )
        .ok()?;
    Some(DrawCommand::Mesh {
        instance: GpuInstance::solid(color, opacity, transform),
        vertices: geometry.vertices,
        indices: geometry.indices,
    })
}

fn gpu_path_mask_from_svg(d: &str, transform: Transform, color: Color) -> Option<GpuPathMask> {
    let path = svgpath_to_tiny_skia(d)?;
    gpu_path_mask_from_tiny_path(&path, transform, color)
}

fn gpu_path_mask_from_node(
    node: &SceneNode,
    transform: Transform,
    mask_mode: crate::scene::MaskMode,
) -> Option<GpuPathMask> {
    let SceneNode::Path {
        d,
        fill,
        stroke,
        stroke_width,
        opacity,
    } = node
    else {
        return None;
    };
    if !stroke_width.is_finite() || !opacity.is_finite() || *opacity < 0.0 {
        return None;
    }
    let path = svgpath_to_tiny_skia(d)?;
    let (mask_path, color_source) = match (fill, stroke) {
        (Some(_), Some(_)) => return None,
        (Some(fill), None) if *stroke_width == 0.0 => (&path, fill),
        (None, Some(stroke)) if *stroke_width > 0.0 => {
            let scale = transform
                .get_scale()
                .0
                .max(transform.get_scale().1)
                .max(1.0);
            let stroked = path.stroke(
                &Stroke {
                    width: *stroke_width,
                    ..Default::default()
                },
                scale,
            )?;
            return gpu_path_mask_from_tiny_path(
                &stroked,
                transform,
                path_mask_color(*stroke, mask_mode),
            )
            .map(|mut mask| {
                mask.instance.params[3] = *opacity;
                mask
            });
        }
        _ => return None,
    };
    gpu_path_mask_from_tiny_path(
        mask_path,
        transform,
        path_mask_color(*color_source, mask_mode),
    )
    .map(|mut mask| {
        mask.instance.params[3] = *opacity;
        mask
    })
}

/// Path masks are rendered into an R8 texture through `fs_solid`, whose
/// output alpha becomes the mask value. Encode luminance in alpha here;
/// storing it only in RGB would be lost during the path-mask pass.
fn path_mask_color(color: Color, mask_mode: crate::scene::MaskMode) -> Color {
    match mask_mode {
        crate::scene::MaskMode::Alpha => Color::rgba(255, 255, 255, color.a),
        crate::scene::MaskMode::AlphaInverted => Color::rgba(255, 255, 255, 255 - color.a),
        crate::scene::MaskMode::Luminance => {
            let luminance = (0.2126 * f32::from(color.r)
                + 0.7152 * f32::from(color.g)
                + 0.0722 * f32::from(color.b))
            .round()
            .clamp(0.0, 255.0) as u16;
            let alpha = (luminance * u16::from(color.a) + 127) / 255;
            Color::rgba(255, 255, 255, alpha as u8)
        }
        crate::scene::MaskMode::LuminanceInverted => {
            let luminance = (0.2126 * f32::from(color.r)
                + 0.7152 * f32::from(color.g)
                + 0.0722 * f32::from(color.b))
            .round()
            .clamp(0.0, 255.0) as u16;
            let alpha = (luminance * u16::from(color.a) + 127) / 255;
            Color::rgba(255, 255, 255, 255 - (alpha as u8))
        }
    }
}

fn gpu_path_mask_from_tiny_path(
    path: &TinyPath,
    transform: Transform,
    color: Color,
) -> Option<GpuPathMask> {
    let lyon_path = tiny_path_to_lyon(path)?;
    let mut geometry: VertexBuffers<GpuVertex, u32> = VertexBuffers::new();
    FillTessellator::new()
        .tessellate_path(
            &lyon_path,
            &FillOptions::non_zero().with_tolerance(0.1),
            &mut BuffersBuilder::new(&mut geometry, PositionConstructor),
        )
        .ok()?;
    if geometry.vertices.is_empty() || geometry.indices.is_empty() || !transform.is_finite() {
        return None;
    }
    Some(GpuPathMask {
        instance: GpuInstance::solid(color, 1.0, transform),
        vertices: geometry.vertices,
        indices: geometry.indices,
    })
}

fn tiny_path_to_lyon(path: &TinyPath) -> Option<LyonPath> {
    let mut builder = LyonPath::builder();
    let mut open = false;
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(position) => {
                if open {
                    builder.end(false);
                }
                builder.begin(point(position.x, position.y));
                open = true;
            }
            PathSegment::LineTo(position) => {
                builder.line_to(point(position.x, position.y));
            }
            PathSegment::QuadTo(control, position) => {
                builder.quadratic_bezier_to(
                    point(control.x, control.y),
                    point(position.x, position.y),
                );
            }
            PathSegment::CubicTo(control1, control2, position) => {
                builder.cubic_bezier_to(
                    point(control1.x, control1.y),
                    point(control2.x, control2.y),
                    point(position.x, position.y),
                );
            }
            PathSegment::Close => {
                builder.end(true);
                open = false;
            }
        }
    }
    if open {
        builder.end(false);
    }
    Some(builder.build())
}
