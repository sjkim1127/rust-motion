//! Scene compilation entry points: turns a scene tree into GPU draw commands.

use super::*;

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
