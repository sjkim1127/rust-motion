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

pub(crate) use crate::render_ir::gpu_normal_texture_layer_supported;
#[cfg(test)]
pub(crate) use crate::render_ir::trailing_overlap_texture_layer;

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
