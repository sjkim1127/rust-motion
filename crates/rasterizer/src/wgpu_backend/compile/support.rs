//! Capability checks deciding whether a scene or layer can be rendered on the GPU path.

use super::*;

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
