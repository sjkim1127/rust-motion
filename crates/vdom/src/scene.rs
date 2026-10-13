use crate::css::{ResolvedStyle, Stylesheet};
use crate::dom::{NativeDom, NativeDomError, NativeElement, NativeNodeKind, NodeKey};
use rust_motion_rasterizer::{
    layout_text_box, measure_text_width, AudioTrack, BlendMode, ClipRegion, Color, MaskMode, Scene,
    SceneNode, TextBox, Transform2D,
};
use std::collections::HashMap;
use taffy::geometry::Size;
use taffy::prelude::{AvailableSpace, Dimension, Display, NodeId, Style, TaffyTree};

#[derive(Debug, Clone)]
enum MeasureContext {
    Text {
        content: String,
        font_size: f32,
        line_height: f32,
        font_sources: Vec<String>,
    },
}

#[derive(Debug, Clone)]
struct LayoutRecord {
    node: NodeId,
    style: ResolvedStyle,
}

struct SceneLayout<'a> {
    dom: &'a NativeDom,
    stylesheet: &'a Stylesheet,
    tree: TaffyTree<MeasureContext>,
    records: HashMap<NodeKey, LayoutRecord>,
    selector_ancestors: Vec<NativeElement>,
}

pub(crate) fn dom_to_scene(
    dom: &NativeDom,
    width: u32,
    height: u32,
    stylesheet: &Stylesheet,
) -> Result<Scene, NativeDomError> {
    let mut layout = SceneLayout {
        dom,
        stylesheet,
        tree: TaffyTree::new(),
        records: HashMap::new(),
        selector_ancestors: Vec::new(),
    };
    let root_style = ResolvedStyle::default();
    let root_children = dom.node(dom.root)?.children.clone();
    let mut children = Vec::with_capacity(root_children.len());
    for child in &root_children {
        if let Some(node) = layout.build(*child, Some(&root_style))? {
            children.push(node);
        }
    }
    let taffy_style = Style {
        display: Display::Block,
        size: Size {
            width: Dimension::length(width as f32),
            height: Dimension::length(height as f32),
        },
        ..Default::default()
    };
    let root = layout
        .tree
        .new_with_children(taffy_style, &children)
        .map_err(layout_error)?;
    layout.records.insert(
        dom.root,
        LayoutRecord {
            node: root,
            style: root_style,
        },
    );

    layout
        .tree
        .compute_layout_with_measure(
            root,
            Size {
                width: AvailableSpace::Definite(width as f32),
                height: AvailableSpace::Definite(height as f32),
            },
            measure_node,
        )
        .map_err(layout_error)?;

    let mut scene = Scene::new();
    for child in root_children {
        scene.nodes.extend(layout.emit(child, 0.0, 0.0)?);
    }
    Ok(scene)
}

impl SceneLayout<'_> {
    fn build(
        &mut self,
        key: NodeKey,
        parent_style: Option<&ResolvedStyle>,
    ) -> Result<Option<NodeId>, NativeDomError> {
        let native = self.dom.node(key)?;
        match &native.kind {
            NativeNodeKind::Placeholder => Ok(None),
            NativeNodeKind::Root => unreachable!("the synthetic root is built separately"),
            NativeNodeKind::Text(content) => {
                let style = parent_style
                    .map(ResolvedStyle::inherited)
                    .unwrap_or_default();
                let node = self
                    .tree
                    .new_leaf_with_context(
                        Style::default(),
                        MeasureContext::Text {
                            content: content.clone(),
                            font_size: style.font_size,
                            line_height: style.line_height,
                            font_sources: style.font_sources.clone(),
                        },
                    )
                    .map_err(layout_error)?;
                self.records.insert(key, LayoutRecord { node, style });
                Ok(Some(node))
            }
            NativeNodeKind::Element(element) => {
                let style = self.stylesheet.resolve_with_ancestors(
                    element,
                    parent_style,
                    &self.selector_ancestors,
                );
                self.selector_ancestors.push(element.clone());
                let mut children = Vec::with_capacity(native.children.len());
                for child in &native.children {
                    if let Some(node) = self.build(*child, Some(&style))? {
                        children.push(node);
                    }
                }
                self.selector_ancestors.pop();
                let node = self
                    .tree
                    .new_with_children(style.layout.clone(), &children)
                    .map_err(layout_error)?;
                self.records.insert(key, LayoutRecord { node, style });
                Ok(Some(node))
            }
        }
    }

    fn emit(
        &self,
        key: NodeKey,
        parent_x: f32,
        parent_y: f32,
    ) -> Result<Vec<SceneNode>, NativeDomError> {
        let native = self.dom.node(key)?;
        let Some(record) = self.records.get(&key) else {
            return Ok(Vec::new());
        };
        if record.style.layout.display == Display::None {
            return Ok(Vec::new());
        }
        let box_layout = self.tree.layout(record.node).map_err(layout_error)?;
        let x = parent_x + box_layout.location.x;
        let y = parent_y + box_layout.location.y;
        let width = box_layout.size.width.max(0.0);
        let height = box_layout.size.height.max(0.0);

        match &native.kind {
            NativeNodeKind::Text(content) => emit_text(content, x, y, width, height, &record.style),
            NativeNodeKind::Element(element) => {
                let mut nodes = Vec::new();
                if let Some((cx, cy, radius, stops)) = &record.style.background_radial_gradient {
                    nodes.push(SceneNode::RadialGradient {
                        cx: x + width * *cx,
                        cy: y + height * *cy,
                        r: width.max(height) * *radius,
                        stops: stops.clone(),
                    });
                } else if let Some((angle_deg, stops)) = &record.style.background_gradient {
                    nodes.push(SceneNode::LinearGradient {
                        x,
                        y,
                        w: width,
                        h: height,
                        angle_deg: *angle_deg,
                        stops: stops.clone(),
                    });
                } else if let Some(background) = record.style.background {
                    nodes.push(SceneNode::Rect {
                        x,
                        y,
                        w: width,
                        h: height,
                        fill: background,
                        stroke: record
                            .style
                            .border_color
                            .or((record.style.border_width > 0.0).then_some(record.style.color)),
                        stroke_width: record.style.border_width,
                        corner_radius: record.style.border_radius,
                    });
                } else if record.style.border_width > 0.0 {
                    nodes.push(SceneNode::Rect {
                        x,
                        y,
                        w: width,
                        h: height,
                        fill: Color::TRANSPARENT,
                        stroke: record.style.border_color.or(Some(record.style.color)),
                        stroke_width: record.style.border_width,
                        corner_radius: record.style.border_radius,
                    });
                }

                emit_element_media(element, x, y, width, height, &record.style, &mut nodes);
                for child in &native.children {
                    nodes.extend(self.emit(*child, x, y)?);
                }
                if record.style.transform != Transform2D::default() {
                    let transform = transform_with_origin(
                        record.style.transform,
                        record.style.transform_origin,
                        x,
                        y,
                        width,
                        height,
                    );
                    nodes = vec![SceneNode::Group {
                        transform,
                        opacity: 1.0,
                        children: nodes,
                    }];
                }

                if record.style.overflow_hidden {
                    Ok(vec![SceneNode::Layer {
                        opacity: record.style.opacity,
                        blend_mode: record.style.blend_mode,
                        clip: Some(ClipRegion::Rect {
                            x,
                            y,
                            w: width,
                            h: height,
                            corner_radius: record.style.border_radius,
                        }),
                        mask: None,
                        mask_mode: MaskMode::Alpha,
                        filters: record.style.filters.clone(),
                        shadow: record.style.box_shadow.clone(),
                        children: nodes,
                    }])
                } else if let Some(shadow) = record.style.box_shadow.clone() {
                    Ok(vec![SceneNode::Layer {
                        opacity: record.style.opacity,
                        blend_mode: record.style.blend_mode,
                        clip: None,
                        mask: None,
                        mask_mode: MaskMode::Alpha,
                        filters: record.style.filters.clone(),
                        shadow: Some(shadow),
                        children: nodes,
                    }])
                } else if !record.style.filters.is_empty()
                    || record.style.blend_mode != BlendMode::Normal
                {
                    Ok(vec![SceneNode::Layer {
                        opacity: record.style.opacity,
                        blend_mode: record.style.blend_mode,
                        clip: None,
                        mask: None,
                        mask_mode: MaskMode::Alpha,
                        filters: record.style.filters.clone(),
                        shadow: None,
                        children: nodes,
                    }])
                } else if record.style.opacity < 1.0 {
                    Ok(vec![SceneNode::Group {
                        transform: Transform2D::default(),
                        opacity: record.style.opacity,
                        children: nodes,
                    }])
                } else {
                    Ok(nodes)
                }
            }
            NativeNodeKind::Root | NativeNodeKind::Placeholder => Ok(Vec::new()),
        }
    }
}

fn transform_with_origin(
    transform: Transform2D,
    origin: (
        crate::css::TransformOriginValue,
        crate::css::TransformOriginValue,
    ),
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> Transform2D {
    let ox = x + origin.0.resolve(width);
    let oy = y + origin.1.resolve(height);
    let radians = transform.rotate_deg.to_radians();
    let (sin, cos) = radians.sin_cos();
    let mx = transform.scale_x * cos;
    let my = transform.scale_x * sin;
    let nx = -transform.scale_y * sin;
    let ny = transform.scale_y * cos;
    transform.with_translate(
        transform.tx + ox - (mx * ox + nx * oy),
        transform.ty + oy - (my * ox + ny * oy),
    )
}

fn measure_node(
    known: Size<Option<f32>>,
    available: Size<AvailableSpace>,
    _node: NodeId,
    context: Option<&mut MeasureContext>,
    _style: &Style,
) -> Size<f32> {
    let Some(MeasureContext::Text {
        content,
        font_size,
        line_height,
        font_sources,
    }) = context
    else {
        return Size {
            width: known.width.unwrap_or(0.0),
            height: known.height.unwrap_or(0.0),
        };
    };

    let natural_width = measure_text_width(content, *font_size, font_sources)
        .map(|width| width.ceil() + 1.0)
        .unwrap_or_else(|_| content.chars().count() as f32 * *font_size * 0.6);
    let width = known.width.unwrap_or_else(|| match available.width {
        AvailableSpace::Definite(limit) => natural_width.min(limit.max(1.0)),
        AvailableSpace::MinContent => font_size.max(1.0),
        AvailableSpace::MaxContent => natural_width,
    });
    let height = known.height.unwrap_or_else(|| {
        let mut request = TextBox::new(
            content.clone(),
            0.0,
            0.0,
            width.max(1.0),
            1_000_000.0,
            *font_size,
        );
        request.line_height = *line_height;
        request.font_sources = font_sources.clone();
        layout_text_box(&request)
            .map(|layout| layout.lines.len().max(1) as f32 * layout.line_height)
            .unwrap_or(*font_size * *line_height)
    });
    Size { width, height }
}

fn emit_text(
    content: &str,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ResolvedStyle,
) -> Result<Vec<SceneNode>, NativeDomError> {
    if content.is_empty() || width <= 0.0 || height <= 0.0 {
        return Ok(Vec::new());
    }
    let mut request = TextBox::new(
        content,
        x,
        y,
        width.max(1.0),
        height.max(1.0),
        style.font_size,
    );
    request.line_height = style.line_height;
    request.font_sources = style.font_sources.clone();
    let layout =
        layout_text_box(&request).map_err(|error| NativeDomError::Scene(error.to_string()))?;
    Ok(layout
        .lines
        .into_iter()
        .map(|line| SceneNode::Text {
            x: line.x,
            y: line.y,
            content: line.text,
            font_size: layout.font_size,
            color: style.color,
            font_weight: style.font_weight,
            font_sources: style.font_sources.clone(),
        })
        .collect())
}

fn emit_element_media(
    element: &NativeElement,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    style: &ResolvedStyle,
    nodes: &mut Vec<SceneNode>,
) {
    let attr = |name: &str| element.attributes.get(name).map(String::as_str);
    let is_svg = element
        .namespace
        .as_deref()
        .is_some_and(|namespace| namespace.contains("svg"));
    match element.tag.to_ascii_lowercase().as_str() {
        "canvas" => {
            if let Some(src) = attr("data-src").or_else(|| attr("src")) {
                nodes.push(SceneNode::Image {
                    src: src.to_string(),
                    x,
                    y,
                    w: width,
                    h: height,
                    fit: style.object_fit,
                    opacity: 1.0,
                });
            }
        }
        "img" => {
            if let Some(src) = attr("src") {
                nodes.push(SceneNode::Image {
                    src: src.to_string(),
                    x,
                    y,
                    w: width,
                    h: height,
                    fit: style.object_fit,
                    opacity: 1.0,
                });
            }
        }
        "video" => {
            if let Some(src) = attr("src") {
                nodes.push(SceneNode::Video {
                    src: src.to_string(),
                    time: attr("data-time")
                        .or_else(|| attr("current-time"))
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0.0),
                    looped: bool_attr(attr("loop")),
                    x,
                    y,
                    w: width,
                    h: height,
                    fit: style.object_fit,
                    opacity: 1.0,
                });
            }
        }
        "audio" => {
            if let Some(src) = attr("src") {
                let mut track = AudioTrack::new(src);
                track.start_from = number_attr(attr("data-start-from"), 0.0);
                track.timeline_start = number_attr(attr("data-timeline-start"), 0.0);
                track.duration = attr("data-duration").and_then(|value| value.parse().ok());
                track.volume = number_attr(attr("data-volume").or_else(|| attr("volume")), 1.0)
                    .clamp(0.0, 1.0);
                track.playback_rate = number_attr(attr("playback-rate"), 1.0);
                track.looped = bool_attr(attr("loop"));
                nodes.push(SceneNode::Audio { track });
            }
        }
        "path" if is_svg => {
            if let Some(d) = attr("d") {
                let path = SceneNode::Path {
                    d: d.to_string(),
                    fill: attr("fill").and_then(Color::from_css),
                    stroke: attr("stroke").and_then(Color::from_css),
                    stroke_width: number_attr(attr("stroke-width"), 0.0) as f32,
                    opacity: number_attr(attr("opacity"), 1.0) as f32,
                };
                nodes.push(SceneNode::Group {
                    transform: Transform2D {
                        tx: x,
                        ty: y,
                        ..Default::default()
                    },
                    opacity: 1.0,
                    children: vec![path],
                });
            }
        }
        "rect" if is_svg => nodes.push(SceneNode::Rect {
            x: x + number_attr(attr("x"), 0.0) as f32,
            y: y + number_attr(attr("y"), 0.0) as f32,
            w: attr("width")
                .and_then(|value| value.parse().ok())
                .unwrap_or(width),
            h: attr("height")
                .and_then(|value| value.parse().ok())
                .unwrap_or(height),
            fill: attr("fill")
                .and_then(Color::from_css)
                .unwrap_or(Color::TRANSPARENT),
            stroke: attr("stroke").and_then(Color::from_css),
            stroke_width: number_attr(attr("stroke-width"), 0.0) as f32,
            corner_radius: number_attr(attr("rx"), 0.0) as f32,
        }),
        "circle" if is_svg => nodes.push(SceneNode::Circle {
            cx: x + number_attr(attr("cx"), width as f64 * 0.5) as f32,
            cy: y + number_attr(attr("cy"), height as f64 * 0.5) as f32,
            r: number_attr(attr("r"), width.min(height) as f64 * 0.5) as f32,
            fill: attr("fill")
                .and_then(Color::from_css)
                .unwrap_or(Color::TRANSPARENT),
            stroke: attr("stroke").and_then(Color::from_css),
            stroke_width: number_attr(attr("stroke-width"), 0.0) as f32,
        }),
        _ => {}
    }
}

fn number_attr(value: Option<&str>, default: f64) -> f64 {
    value
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn bool_attr(value: Option<&str>) -> bool {
    value.is_some_and(|value| value.is_empty() || value == "true" || value == "loop")
}

fn layout_error(error: impl std::fmt::Display) -> NativeDomError {
    NativeDomError::Layout(error.to_string())
}
