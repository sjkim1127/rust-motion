//! Path masks: tessellating SVG and tiny-skia paths into GPU mesh data.

use super::*;

pub(super) fn gradient_instance(
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

pub(super) fn mesh_command(
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

pub(super) fn gpu_path_mask_from_svg(
    d: &str,
    transform: Transform,
    color: Color,
) -> Option<GpuPathMask> {
    let path = svgpath_to_tiny_skia(d)?;
    gpu_path_mask_from_tiny_path(&path, transform, color)
}

pub(super) fn gpu_path_mask_from_node(
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
pub(super) fn path_mask_color(color: Color, mask_mode: crate::scene::MaskMode) -> Color {
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

pub(super) fn gpu_path_mask_from_tiny_path(
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

pub(super) fn tiny_path_to_lyon(path: &TinyPath) -> Option<LyonPath> {
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
