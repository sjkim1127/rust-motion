//! Scene-to-RenderGraph lowering.
//!
//! Translates the public 1.x [`Scene`] representation into an explicit,
//! backend-independent [`RenderGraph`] and [`ExecutionPlan`] without introducing
//! any graphics-API types.

use super::types::*;
use super::validate::{ExecutionPlan, GraphValidationError};
use crate::backend::FrameConfig;
use crate::scene::{BlendMode, ClipRegion, Color, Scene, SceneFilter, SceneNode};
use tiny_skia::Transform;

/// Target capability and resource constraints used while lowering a [`Scene`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LowerOptions {
    pub(crate) max_texture_dimension_2d: u32,
    pub(crate) capabilities: CapabilityFlags,
    pub(crate) msaa_samples: u32,
}

impl Default for LowerOptions {
    fn default() -> Self {
        Self {
            max_texture_dimension_2d: u32::MAX,
            capabilities: CapabilityFlags::RASTER_2D_ANALYTIC
                .union(CapabilityFlags::RASTER_2D_MESH)
                .union(CapabilityFlags::RASTER_2D_IMAGE)
                .union(CapabilityFlags::RASTER_2D_TEXT)
                .union(CapabilityFlags::BLEND_BASIC)
                .union(CapabilityFlags::BLEND_EXTENDED)
                .union(CapabilityFlags::OFFSCREEN_COMPOSITE)
                .union(CapabilityFlags::PATH_MASK)
                .union(CapabilityFlags::SHADER_PASS)
                .union(CapabilityFlags::COPY_OP)
                .union(CapabilityFlags::CPU_ISLAND)
                .union(CapabilityFlags::RASTER_EXTENDED),
            msaa_samples: 1,
        }
    }
}

/// Whether inline opacity rewriting is supported for an expression-style WGSL shader snippet.
pub(crate) fn shader_inline_opacity_supported(source: &str) -> bool {
    !source.contains("@fragment") && source.contains("return ")
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
/// texture and one explicit composite pass.
pub(crate) fn trailing_overlap_texture_layer(scene: &Scene) -> Option<(Scene, Scene, f32)> {
    let SceneNode::Layer {
        opacity,
        blend_mode: BlendMode::Normal,
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

/// Inspect a slice of [`SceneNode`]s and summarize the capabilities required to rasterize them.
pub(crate) fn required_capabilities_for_nodes(nodes: &[SceneNode]) -> CapabilityFlags {
    let mut caps = CapabilityFlags::NONE;
    for node in nodes {
        match node {
            SceneNode::Rect { .. }
            | SceneNode::Circle { .. }
            | SceneNode::LinearGradient { .. }
            | SceneNode::RadialGradient { .. } => {
                caps = caps.union(CapabilityFlags::RASTER_2D_ANALYTIC);
            }
            SceneNode::Path { .. } => {
                caps = caps.union(CapabilityFlags::RASTER_2D_MESH);
            }
            SceneNode::Image { .. }
            | SceneNode::Video { .. }
            | SceneNode::Lottie { .. }
            | SceneNode::Gif { .. }
            | SceneNode::Emoji { .. }
            | SceneNode::AudioVisualizer { .. } => {
                caps = caps.union(CapabilityFlags::RASTER_2D_IMAGE);
            }
            SceneNode::Text { .. } => {
                caps = caps.union(CapabilityFlags::RASTER_2D_TEXT);
            }
            SceneNode::Shader { .. } => {
                caps = caps.union(CapabilityFlags::SHADER_PASS);
            }
            SceneNode::Group { children, .. } | SceneNode::NullObject { children, .. } => {
                caps = caps.union(required_capabilities_for_nodes(children));
            }
            SceneNode::Layer {
                blend_mode,
                clip,
                mask,
                children,
                ..
            } => {
                match blend_mode {
                    BlendMode::Normal => {}
                    BlendMode::Multiply
                    | BlendMode::Screen
                    | BlendMode::Darken
                    | BlendMode::Lighten => {
                        caps = caps.union(CapabilityFlags::BLEND_BASIC);
                    }
                    _ => {
                        caps = caps.union(CapabilityFlags::BLEND_EXTENDED);
                    }
                }
                if !crate::render_ir::validate::layer_has_only_gpu_basic_effects(node) {
                    caps = caps.union(CapabilityFlags::RASTER_EXTENDED);
                }
                if matches!(clip, Some(ClipRegion::Path { .. }))
                    || mask
                        .as_deref()
                        .is_some_and(|m| m.iter().any(|n| matches!(n, SceneNode::Path { .. })))
                {
                    caps = caps.union(CapabilityFlags::PATH_MASK);
                }
                caps = caps.union(required_capabilities_for_nodes(children));
            }
            SceneNode::TrackMatte { target, matte, .. } => {
                caps = caps.union(CapabilityFlags::BLEND_EXTENDED);
                caps = caps.union(required_capabilities_for_nodes(std::slice::from_ref(
                    target.as_ref(),
                )));
                caps = caps.union(required_capabilities_for_nodes(std::slice::from_ref(
                    matte.as_ref(),
                )));
            }
            SceneNode::Audio { .. } => {}
            SceneNode::FractalNoise { .. }
            | SceneNode::AnimatedStroke { .. }
            | SceneNode::VectorShape { .. }
            | SceneNode::Scene3D { .. } => {
                caps = caps.union(CapabilityFlags::RASTER_EXTENDED);
            }
        }
    }
    caps
}

fn gpu_blur_layer_parts<'a>(
    node: &'a SceneNode,
    opts: &LowerOptions,
) -> Option<(f32, u32, &'a [SceneNode])> {
    if !opts
        .capabilities
        .contains(CapabilityFlags::COMPUTE_PASS.union(CapabilityFlags::OFFSCREEN_COMPOSITE))
    {
        return None;
    }
    let SceneNode::Layer {
        opacity,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        filters,
        shadow: None,
        children,
        ..
    } = node
    else {
        return None;
    };
    if !opacity.is_finite() || !(0.0..=1.0).contains(opacity) {
        return None;
    }
    let [SceneFilter::Blur { sigma }] = filters.as_slice() else {
        return None;
    };
    if !sigma.is_finite() || !(0.0..=100.0).contains(sigma) {
        return None;
    }
    let child_caps = required_capabilities_for_nodes(children);
    if child_caps.contains(CapabilityFlags::SHADER_PASS)
        || !child_caps.missing(opts.capabilities).is_empty()
    {
        return None;
    }
    let radius = (sigma * 1.5).ceil().clamp(0.0, 128.0) as u32;
    Some((*opacity, radius, children.as_slice()))
}

fn isolatable_cpu_node_bounds(
    node: &SceneNode,
    canvas_w: u32,
    canvas_h: u32,
) -> Option<RectBounds> {
    let (x, y, w, h) = match node {
        SceneNode::FractalNoise { x, y, w, h, .. } | SceneNode::Scene3D { x, y, w, h, .. } => {
            (*x, *y, *w, *h)
        }
        _ => return None,
    };
    if ![x, y, w, h].iter().all(|v| v.is_finite()) || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let max_w = canvas_w as f32;
    let max_h = canvas_h as f32;
    let x0 = x.floor().clamp(0.0, max_w);
    let y0 = y.floor().clamp(0.0, max_h);
    let x1 = (x + w).ceil().clamp(0.0, max_w);
    let y1 = (y + h).ceil().clamp(0.0, max_h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(RectBounds {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

fn color_output_desc(
    id: ResourceId,
    label: &str,
    width: u32,
    height: u32,
    sample_count: u32,
    lifetime: ResourceLifetime,
    extra_usage: TextureUsage,
) -> ResourceDesc {
    ResourceDesc {
        id,
        label: label.to_string(),
        width,
        height,
        format: PixelFormat::Rgba8Unorm,
        color: ColorMetadata::srgb_premultiplied(),
        sample_count,
        usage: TextureUsage::RENDER_TARGET
            .union(TextureUsage::COPY_SRC)
            .union(extra_usage),
        lifetime,
    }
}

/// Lower a 1.x [`Scene`] into a declarative [`RenderGraph`].
pub(crate) fn lower_scene_to_graph(
    scene: &Scene,
    config: &FrameConfig,
    opts: &LowerOptions,
) -> RenderGraph {
    let mut graph = RenderGraph::new(config.width, config.height);
    let msaa = if opts.msaa_samples == 4 { 4 } else { 1 };

    // 1. Dimensions exceeding the device texture limit must stay on CPU.
    if config.width > opts.max_texture_dimension_2d || config.height > opts.max_texture_dimension_2d
    {
        let out = graph.add_resource(color_output_desc(
            ResourceId(0),
            "frame_output",
            config.width,
            config.height,
            1,
            ResourceLifetime::Output,
            TextureUsage::CPU_READBACK,
        ));
        graph.set_output(out);
        graph.add_pass(PassNode {
            id: PassId(0),
            label: "cpu_oversized_frame".into(),
            op: PassOp::CpuIsland {
                scene: scene.clone(),
                bounds: None,
                reason: "frame dimensions exceed the GPU texture limit".into(),
            },
            reads: Vec::new(),
            writes: vec![out],
            order_deps: Vec::new(),
            required_capabilities: CapabilityFlags::CPU_ISLAND,
        });
        return graph;
    }

    // 2. Pure shader-node scene.
    if !scene.nodes.is_empty()
        && scene
            .nodes
            .iter()
            .all(|node| matches!(node, SceneNode::Shader { .. }))
        && opts.capabilities.contains(CapabilityFlags::SHADER_PASS)
    {
        let out = graph.add_resource(color_output_desc(
            ResourceId(0),
            "shader_output",
            config.width,
            config.height,
            1,
            ResourceLifetime::Output,
            TextureUsage::CPU_READBACK,
        ));
        graph.set_output(out);
        for (idx, node) in scene.nodes.iter().enumerate() {
            let SceneNode::Shader {
                x,
                y,
                w,
                h,
                source,
                time,
                params,
                opacity,
            } = node
            else {
                unreachable!();
            };
            graph.add_pass(PassNode {
                id: PassId(idx as u32),
                label: format!("shader_{idx}"),
                op: PassOp::Shader {
                    x: *x,
                    y: *y,
                    w: *w,
                    h: *h,
                    source: source.clone(),
                    time: *time,
                    params: *params,
                    opacity: *opacity,
                    clear: if idx == 0 {
                        ClearOp::Clear(Color::TRANSPARENT)
                    } else {
                        ClearOp::Load
                    },
                },
                reads: if idx == 0 { Vec::new() } else { vec![out] },
                writes: vec![out],
                order_deps: Vec::new(),
                required_capabilities: CapabilityFlags::SHADER_PASS,
            });
        }
        return graph;
    }

    // 3. Trailing valid shader suffix after regular scene nodes.
    let trailing_shader_suffix_start = scene
        .nodes
        .iter()
        .position(|node| matches!(node, SceneNode::Shader { .. }))
        .filter(|start| {
            *start > 0
                && scene.nodes[*start..].iter().all(|node| {
                    matches!(
                        node,
                        SceneNode::Shader { x, y, w, h, source, opacity, .. }
                        if *opacity >= 0.0
                            && *opacity <= 1.0
                            && shader_inline_opacity_supported(source)
                            && *x >= 0.0
                            && *y >= 0.0
                            && *w > 0.0
                            && *h > 0.0
                            && *x + *w <= config.width as f32
                            && *y + *h <= config.height as f32
                    )
                })
        });

    // 4. Interleaved shaders (shader present, not a pure trailing suffix).
    if scene
        .nodes
        .iter()
        .any(|node| matches!(node, SceneNode::Shader { .. }))
        && trailing_shader_suffix_start.is_none()
        && opts.capabilities.contains(CapabilityFlags::SHADER_PASS)
    {
        let out = graph.add_resource(color_output_desc(
            ResourceId(0),
            "interleaved_output",
            config.width,
            config.height,
            msaa,
            ResourceLifetime::Output,
            TextureUsage::CPU_READBACK,
        ));
        graph.set_output(out);

        let mut regular_nodes = Vec::new();
        let mut next_pass_id = 0u32;
        let push_regular =
            |graph: &mut RenderGraph, nodes: Vec<SceneNode>, pass_id: &mut u32, out: ResourceId| {
                let pid = PassId(*pass_id);
                *pass_id += 1;
                let clear = if pid.0 == 0 {
                    ClearOp::Clear(Color::TRANSPARENT)
                } else {
                    ClearOp::Load
                };
                let caps = required_capabilities_for_nodes(&nodes);
                graph.add_pass(PassNode {
                    id: pid,
                    label: format!("raster_segment_{}", pid.0),
                    op: PassOp::Raster2D {
                        scene: Scene { nodes },
                        clear,
                        path_mask: None,
                    },
                    reads: if pid.0 == 0 { Vec::new() } else { vec![out] },
                    writes: vec![out],
                    order_deps: Vec::new(),
                    required_capabilities: caps,
                });
            };

        for node in &scene.nodes {
            if let SceneNode::Shader {
                x,
                y,
                w,
                h,
                source,
                time,
                params,
                opacity,
            } = node
            {
                if !regular_nodes.is_empty() {
                    push_regular(
                        &mut graph,
                        std::mem::take(&mut regular_nodes),
                        &mut next_pass_id,
                        out,
                    );
                }
                let pid = PassId(next_pass_id);
                next_pass_id += 1;
                graph.add_pass(PassNode {
                    id: pid,
                    label: format!("shader_segment_{}", pid.0),
                    op: PassOp::Shader {
                        x: *x,
                        y: *y,
                        w: *w,
                        h: *h,
                        source: source.clone(),
                        time: *time,
                        params: *params,
                        opacity: *opacity,
                        clear: if pid.0 == 0 {
                            ClearOp::Clear(Color::TRANSPARENT)
                        } else {
                            ClearOp::Load
                        },
                    },
                    reads: if pid.0 == 0 { Vec::new() } else { vec![out] },
                    writes: vec![out],
                    order_deps: Vec::new(),
                    required_capabilities: CapabilityFlags::SHADER_PASS,
                });
            } else {
                regular_nodes.push(node.clone());
            }
        }
        if !regular_nodes.is_empty() {
            push_regular(&mut graph, regular_nodes, &mut next_pass_id, out);
        }
        return graph;
    }

    // 5. Trailing overlapping texture layer -> 3-pass offscreen composite graph.
    if trailing_shader_suffix_start.is_none()
        && opts
            .capabilities
            .contains(CapabilityFlags::OFFSCREEN_COMPOSITE)
    {
        if let Some((base_scene, layer_scene, layer_opacity)) =
            trailing_overlap_texture_layer(scene)
        {
            let out = graph.add_resource(color_output_desc(
                ResourceId(0),
                "frame_output",
                config.width,
                config.height,
                msaa,
                ResourceLifetime::Output,
                TextureUsage::CPU_READBACK,
            ));
            let layer_surf = graph.add_resource(color_output_desc(
                ResourceId(1),
                "offscreen_layer",
                config.width,
                config.height,
                msaa,
                ResourceLifetime::Transient,
                TextureUsage::SAMPLED,
            ));
            graph.set_output(out);

            let base_caps = required_capabilities_for_nodes(&base_scene.nodes);
            let layer_caps = required_capabilities_for_nodes(&layer_scene.nodes);

            let p_base = graph.add_pass(PassNode {
                id: PassId(0),
                label: "base_scene".into(),
                op: PassOp::Raster2D {
                    scene: base_scene,
                    clear: ClearOp::Clear(Color::TRANSPARENT),
                    path_mask: None,
                },
                reads: Vec::new(),
                writes: vec![out],
                order_deps: Vec::new(),
                required_capabilities: base_caps,
            });
            let p_layer = graph.add_pass(PassNode {
                id: PassId(1),
                label: "overlap_layer".into(),
                op: PassOp::Raster2D {
                    scene: layer_scene,
                    clear: ClearOp::Clear(Color::TRANSPARENT),
                    path_mask: None,
                },
                reads: Vec::new(),
                writes: vec![layer_surf],
                order_deps: Vec::new(),
                required_capabilities: layer_caps,
            });
            graph.add_pass(PassNode {
                id: PassId(2),
                label: "composite_overlap_layer".into(),
                op: PassOp::Composite {
                    source: layer_surf,
                    opacity: layer_opacity,
                    blend_mode: BlendMode::Normal,
                    region: None,
                },
                reads: vec![layer_surf, out],
                writes: vec![out],
                order_deps: vec![p_base, p_layer],
                required_capabilities: CapabilityFlags::OFFSCREEN_COMPOSITE,
            });
            return graph;
        }
    }

    // 6. Regular scene + optional trailing shader suffix.
    if let Some(suffix_start) = trailing_shader_suffix_start {
        if opts.capabilities.contains(CapabilityFlags::SHADER_PASS) {
            let out = graph.add_resource(color_output_desc(
                ResourceId(0),
                "frame_output",
                config.width,
                config.height,
                msaa,
                ResourceLifetime::Output,
                TextureUsage::CPU_READBACK,
            ));
            graph.set_output(out);
            let prefix_nodes = scene.nodes[..suffix_start].to_vec();
            let prefix_caps = required_capabilities_for_nodes(&prefix_nodes);
            graph.add_pass(PassNode {
                id: PassId(0),
                label: "main_scene".into(),
                op: PassOp::Raster2D {
                    scene: Scene {
                        nodes: prefix_nodes,
                    },
                    clear: ClearOp::Clear(Color::TRANSPARENT),
                    path_mask: None,
                },
                reads: Vec::new(),
                writes: vec![out],
                order_deps: Vec::new(),
                required_capabilities: prefix_caps,
            });
            for (offset, node) in scene.nodes[suffix_start..].iter().enumerate() {
                let SceneNode::Shader {
                    x,
                    y,
                    w,
                    h,
                    source,
                    time,
                    params,
                    opacity,
                } = node
                else {
                    unreachable!();
                };
                let pid = PassId((offset + 1) as u32);
                graph.add_pass(PassNode {
                    id: pid,
                    label: format!("shader_suffix_{offset}"),
                    op: PassOp::Shader {
                        x: *x,
                        y: *y,
                        w: *w,
                        h: *h,
                        source: source.clone(),
                        time: *time,
                        params: *params,
                        opacity: *opacity,
                        clear: ClearOp::Load,
                    },
                    reads: vec![out],
                    writes: vec![out],
                    order_deps: Vec::new(),
                    required_capabilities: CapabilityFlags::SHADER_PASS,
                });
            }
            return graph;
        }
    }

    // 7. Partition scenes with GPU compute blur layers and/or isolatable bounded CPU islands
    // (preserving painter order and falling back whole-frame if any node is non-isolatable).
    enum PartitionItem<'a> {
        GpuNode(&'a SceneNode),
        GpuBlurLayer {
            opacity: f32,
            radius: u32,
            children: &'a [SceneNode],
        },
        CpuIsland {
            node: &'a SceneNode,
            bounds: RectBounds,
        },
    }

    let mut items = Vec::with_capacity(scene.nodes.len());
    let mut has_blur_layer = false;
    let mut has_bounded_island = false;
    let mut gpu_node_count = 0usize;
    let mut non_isolatable = false;

    for node in &scene.nodes {
        if let Some((opacity, radius, children)) = gpu_blur_layer_parts(node, opts) {
            has_blur_layer = true;
            items.push(PartitionItem::GpuBlurLayer {
                opacity,
                radius,
                children,
            });
        } else {
            let node_caps = required_capabilities_for_nodes(std::slice::from_ref(node));
            if node_caps.missing(opts.capabilities).is_empty() {
                gpu_node_count += 1;
                items.push(PartitionItem::GpuNode(node));
            } else if opts
                .capabilities
                .contains(CapabilityFlags::OFFSCREEN_COMPOSITE.union(CapabilityFlags::CPU_ISLAND))
            {
                if let Some(bounds) = isolatable_cpu_node_bounds(node, config.width, config.height)
                {
                    has_bounded_island = true;
                    items.push(PartitionItem::CpuIsland { node, bounds });
                } else {
                    non_isolatable = true;
                    break;
                }
            } else {
                non_isolatable = true;
                break;
            }
        }
    }

    if !non_isolatable && (has_blur_layer || (has_bounded_island && gpu_node_count > 0)) {
        let out = graph.add_resource(color_output_desc(
            ResourceId(0),
            "frame_output",
            config.width,
            config.height,
            msaa,
            ResourceLifetime::Output,
            TextureUsage::CPU_READBACK,
        ));
        graph.set_output(out);

        let mut next_res_id = 1u32;
        let mut next_pass_id = 0u32;
        let mut output_initialized = false;
        let mut last_out_writer: Option<PassId> = None;
        let mut pending_gpu: Vec<SceneNode> = Vec::new();

        let ensure_output_initialized =
            |graph: &mut RenderGraph,
             output_initialized: &mut bool,
             last_out_writer: &mut Option<PassId>,
             next_pass_id: &mut u32| {
                if !*output_initialized {
                    let pid = PassId(*next_pass_id);
                    *next_pass_id += 1;
                    graph.add_pass(PassNode {
                        id: pid,
                        label: "clear_output".into(),
                        op: PassOp::Raster2D {
                            scene: Scene::new(),
                            clear: ClearOp::Clear(Color::TRANSPARENT),
                            path_mask: None,
                        },
                        reads: Vec::new(),
                        writes: vec![out],
                        order_deps: Vec::new(),
                        required_capabilities: CapabilityFlags::RASTER_2D_ANALYTIC,
                    });
                    *output_initialized = true;
                    *last_out_writer = Some(pid);
                }
            };

        let flush_gpu = |graph: &mut RenderGraph,
                         pending: &mut Vec<SceneNode>,
                         output_initialized: &mut bool,
                         last_out_writer: &mut Option<PassId>,
                         next_res_id: &mut u32,
                         next_pass_id: &mut u32| {
            if pending.is_empty() {
                return;
            }
            let seg_nodes = std::mem::take(pending);
            let seg_caps = required_capabilities_for_nodes(&seg_nodes);
            if !*output_initialized {
                let pid = PassId(*next_pass_id);
                *next_pass_id += 1;
                graph.add_pass(PassNode {
                    id: pid,
                    label: "gpu_base_segment".into(),
                    op: PassOp::Raster2D {
                        scene: Scene { nodes: seg_nodes },
                        clear: ClearOp::Clear(Color::TRANSPARENT),
                        path_mask: None,
                    },
                    reads: Vec::new(),
                    writes: vec![out],
                    order_deps: Vec::new(),
                    required_capabilities: seg_caps,
                });
                *output_initialized = true;
                *last_out_writer = Some(pid);
            } else {
                let seg_res = graph.add_resource(color_output_desc(
                    ResourceId(*next_res_id),
                    "gpu_segment",
                    config.width,
                    config.height,
                    msaa,
                    ResourceLifetime::Transient,
                    TextureUsage::SAMPLED,
                ));
                *next_res_id += 1;
                let p_draw = PassId(*next_pass_id);
                *next_pass_id += 1;
                graph.add_pass(PassNode {
                    id: p_draw,
                    label: "gpu_segment_raster".into(),
                    op: PassOp::Raster2D {
                        scene: Scene { nodes: seg_nodes },
                        clear: ClearOp::Clear(Color::TRANSPARENT),
                        path_mask: None,
                    },
                    reads: Vec::new(),
                    writes: vec![seg_res],
                    order_deps: Vec::new(),
                    required_capabilities: seg_caps,
                });
                let p_comp = PassId(*next_pass_id);
                *next_pass_id += 1;
                graph.add_pass(PassNode {
                    id: p_comp,
                    label: "composite_gpu_segment".into(),
                    op: PassOp::Composite {
                        source: seg_res,
                        opacity: 1.0,
                        blend_mode: BlendMode::Normal,
                        region: None,
                    },
                    reads: vec![seg_res, out],
                    writes: vec![out],
                    order_deps: (*last_out_writer).into_iter().chain(Some(p_draw)).collect(),
                    required_capabilities: CapabilityFlags::OFFSCREEN_COMPOSITE,
                });
                *last_out_writer = Some(p_comp);
            }
        };

        for item in items {
            match item {
                PartitionItem::GpuNode(node) => {
                    pending_gpu.push(node.clone());
                }
                PartitionItem::GpuBlurLayer {
                    opacity,
                    radius,
                    children,
                } => {
                    flush_gpu(
                        &mut graph,
                        &mut pending_gpu,
                        &mut output_initialized,
                        &mut last_out_writer,
                        &mut next_res_id,
                        &mut next_pass_id,
                    );
                    ensure_output_initialized(
                        &mut graph,
                        &mut output_initialized,
                        &mut last_out_writer,
                        &mut next_pass_id,
                    );
                    let layer_res = graph.add_resource(color_output_desc(
                        ResourceId(next_res_id),
                        "blur_layer_src",
                        config.width,
                        config.height,
                        msaa,
                        ResourceLifetime::Transient,
                        TextureUsage::SAMPLED,
                    ));
                    next_res_id += 1;
                    let p_layer = PassId(next_pass_id);
                    next_pass_id += 1;
                    let child_caps = required_capabilities_for_nodes(children);
                    graph.add_pass(PassNode {
                        id: p_layer,
                        label: "blur_layer_raster".into(),
                        op: PassOp::Raster2D {
                            scene: Scene {
                                nodes: children.to_vec(),
                            },
                            clear: ClearOp::Clear(Color::TRANSPARENT),
                            path_mask: None,
                        },
                        reads: Vec::new(),
                        writes: vec![layer_res],
                        order_deps: Vec::new(),
                        required_capabilities: child_caps,
                    });

                    let (comp_src, comp_dep) = if radius > 0 {
                        let blur_h_res = graph.add_resource(ResourceDesc {
                            id: ResourceId(next_res_id),
                            label: "blur_h".into(),
                            width: config.width,
                            height: config.height,
                            format: PixelFormat::Rgba8Unorm,
                            color: ColorMetadata::srgb_premultiplied(),
                            sample_count: 1,
                            usage: TextureUsage::STORAGE.union(TextureUsage::SAMPLED),
                            lifetime: ResourceLifetime::Transient,
                        });
                        next_res_id += 1;
                        let blur_v_res = graph.add_resource(ResourceDesc {
                            id: ResourceId(next_res_id),
                            label: "blur_v".into(),
                            width: config.width,
                            height: config.height,
                            format: PixelFormat::Rgba8Unorm,
                            color: ColorMetadata::srgb_premultiplied(),
                            sample_count: 1,
                            usage: TextureUsage::STORAGE.union(TextureUsage::SAMPLED),
                            lifetime: ResourceLifetime::Transient,
                        });
                        next_res_id += 1;
                        let wg = [config.width.div_ceil(8), config.height.div_ceil(8), 1];
                        let p_h = PassId(next_pass_id);
                        next_pass_id += 1;
                        graph.add_pass(PassNode {
                            id: p_h,
                            label: "compute_blur_h".into(),
                            op: PassOp::Compute {
                                kernel: KERNEL_SEPARABLE_BOX_BLUR_H.into(),
                                workgroups: wg,
                                params: vec![radius as f32],
                            },
                            reads: vec![layer_res],
                            writes: vec![blur_h_res],
                            order_deps: vec![p_layer],
                            required_capabilities: CapabilityFlags::COMPUTE_PASS,
                        });
                        let p_v = PassId(next_pass_id);
                        next_pass_id += 1;
                        graph.add_pass(PassNode {
                            id: p_v,
                            label: "compute_blur_v".into(),
                            op: PassOp::Compute {
                                kernel: KERNEL_SEPARABLE_BOX_BLUR_V.into(),
                                workgroups: wg,
                                params: vec![radius as f32],
                            },
                            reads: vec![blur_h_res],
                            writes: vec![blur_v_res],
                            order_deps: vec![p_h],
                            required_capabilities: CapabilityFlags::COMPUTE_PASS,
                        });
                        (blur_v_res, p_v)
                    } else {
                        (layer_res, p_layer)
                    };

                    let p_comp = PassId(next_pass_id);
                    next_pass_id += 1;
                    graph.add_pass(PassNode {
                        id: p_comp,
                        label: "composite_blur_layer".into(),
                        op: PassOp::Composite {
                            source: comp_src,
                            opacity,
                            blend_mode: BlendMode::Normal,
                            region: None,
                        },
                        reads: vec![comp_src, out],
                        writes: vec![out],
                        order_deps: last_out_writer.into_iter().chain(Some(comp_dep)).collect(),
                        required_capabilities: CapabilityFlags::OFFSCREEN_COMPOSITE,
                    });
                    last_out_writer = Some(p_comp);
                }
                PartitionItem::CpuIsland { node, bounds } => {
                    flush_gpu(
                        &mut graph,
                        &mut pending_gpu,
                        &mut output_initialized,
                        &mut last_out_writer,
                        &mut next_res_id,
                        &mut next_pass_id,
                    );
                    ensure_output_initialized(
                        &mut graph,
                        &mut output_initialized,
                        &mut last_out_writer,
                        &mut next_pass_id,
                    );
                    let iw = (bounds.w as u32).max(1);
                    let ih = (bounds.h as u32).max(1);
                    let island_res = graph.add_resource(color_output_desc(
                        ResourceId(next_res_id),
                        "cpu_island_surface",
                        iw,
                        ih,
                        1,
                        ResourceLifetime::Transient,
                        TextureUsage::SAMPLED.union(TextureUsage::COPY_DST),
                    ));
                    next_res_id += 1;
                    let p_island = PassId(next_pass_id);
                    next_pass_id += 1;
                    graph.add_pass(PassNode {
                        id: p_island,
                        label: "bounded_cpu_island".into(),
                        op: PassOp::CpuIsland {
                            scene: Scene {
                                nodes: vec![node.clone()],
                            },
                            bounds: Some(bounds),
                            reason: "bounded CPU island for GPU-unsupported node".into(),
                        },
                        reads: Vec::new(),
                        writes: vec![island_res],
                        order_deps: Vec::new(),
                        required_capabilities: CapabilityFlags::CPU_ISLAND,
                    });
                    let p_comp = PassId(next_pass_id);
                    next_pass_id += 1;
                    graph.add_pass(PassNode {
                        id: p_comp,
                        label: "composite_cpu_island".into(),
                        op: PassOp::Composite {
                            source: island_res,
                            opacity: 1.0,
                            blend_mode: BlendMode::Normal,
                            region: Some(bounds),
                        },
                        reads: vec![island_res, out],
                        writes: vec![out],
                        order_deps: last_out_writer.into_iter().chain(Some(p_island)).collect(),
                        required_capabilities: CapabilityFlags::OFFSCREEN_COMPOSITE,
                    });
                    last_out_writer = Some(p_comp);
                }
            }
        }
        flush_gpu(
            &mut graph,
            &mut pending_gpu,
            &mut output_initialized,
            &mut last_out_writer,
            &mut next_res_id,
            &mut next_pass_id,
        );
        return graph;
    }

    // Default single-pass scene rasterization (or CPU island if required capabilities
    // exceed non-CPU capabilities and CPU_ISLAND is available).
    let out = graph.add_resource(color_output_desc(
        ResourceId(0),
        "frame_output",
        config.width,
        config.height,
        msaa,
        ResourceLifetime::Output,
        TextureUsage::CPU_READBACK,
    ));
    graph.set_output(out);
    let scene_caps = required_capabilities_for_nodes(&scene.nodes);
    let missing = scene_caps.missing(opts.capabilities);
    if !missing.is_empty() && opts.capabilities.contains(CapabilityFlags::CPU_ISLAND) {
        graph.add_pass(PassNode {
            id: PassId(0),
            label: "cpu_fallback_scene".into(),
            op: PassOp::CpuIsland {
                scene: scene.clone(),
                bounds: None,
                reason: "scene contains GPU-unsupported nodes or effects".into(),
            },
            reads: Vec::new(),
            writes: vec![out],
            order_deps: Vec::new(),
            required_capabilities: CapabilityFlags::CPU_ISLAND,
        });
    } else {
        graph.add_pass(PassNode {
            id: PassId(0),
            label: "main_scene".into(),
            op: PassOp::Raster2D {
                scene: scene.clone(),
                clear: ClearOp::Clear(Color::TRANSPARENT),
                path_mask: None,
            },
            reads: Vec::new(),
            writes: vec![out],
            order_deps: Vec::new(),
            required_capabilities: scene_caps,
        });
    }

    graph
}

/// Lower a [`Scene`] and validate/schedule it into an [`ExecutionPlan`].
pub(crate) fn lower_scene_to_plan(
    scene: &Scene,
    config: &FrameConfig,
    opts: &LowerOptions,
) -> Result<ExecutionPlan, GraphValidationError> {
    let graph = lower_scene_to_graph(scene, config, opts);
    ExecutionPlan::compile(graph)
}
