//! Mask shape extraction and mask-info merging for GPU layers.

use super::*;

pub(super) type GpuMaskInfo = (
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

pub(super) fn gpu_clip_mask_info(
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

pub(super) fn merge_gpu_mask_info(
    mut base: GpuMaskInfo,
    extra: GpuMaskInfo,
) -> Option<GpuMaskInfo> {
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

pub(super) fn apply_gpu_mask_info(instance: &mut GpuInstance, info: GpuMaskInfo) {
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

pub(super) fn valid_gpu_gradient_stops(stops: &[crate::scene::GradientStop]) -> bool {
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

pub(super) fn gpu_mask_shapes(
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
