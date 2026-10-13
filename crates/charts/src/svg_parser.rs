//! High-performance SVG to Native [`SceneNode`] parser.
//!
//! Translates standard SVG markup produced by D3, Mermaid, and vector tools
//! into Rust Motion's hardware-accelerated scene graph.

use quick_xml::events::Event;
use quick_xml::Reader;
use rust_motion_rasterizer::{Color, Scene, SceneNode, Transform2D};
use std::collections::HashMap;

/// Result of parsing an SVG document.
#[derive(Debug, Clone, Default)]
pub struct SvgDocument {
    pub width: f32,
    pub height: f32,
    pub view_box: Option<(f32, f32, f32, f32)>,
    pub scene: Scene,
}

/// State maintained across nested `<g>` elements.
#[derive(Debug, Clone)]
struct GroupState {
    transform: Transform2D,
    opacity: f32,
    fill: Option<Option<Color>>,
    stroke: Option<Option<Color>>,
    stroke_width: Option<f32>,
    font_size: Option<f32>,
    font_weight: Option<u16>,
}

impl Default for GroupState {
    fn default() -> Self {
        Self {
            transform: Transform2D::identity(),
            opacity: 1.0,
            fill: None,
            stroke: None,
            stroke_width: None,
            font_size: None,
            font_weight: None,
        }
    }
}

/// Helper to combine two 2D affine transforms.
fn combine_transforms(a: &Transform2D, b: &Transform2D) -> Transform2D {
    Transform2D {
        tx: a.tx + b.tx * a.scale_x,
        ty: a.ty + b.ty * a.scale_y,
        scale_x: a.scale_x * b.scale_x,
        scale_y: a.scale_y * b.scale_y,
        rotate_deg: a.rotate_deg + b.rotate_deg,
    }
}

/// Parse an SVG XML string into an [`SvgDocument`].
pub fn parse_svg(svg_content: &str) -> Result<SvgDocument, String> {
    let mut reader = Reader::from_str(svg_content);
    reader.config_mut().trim_text(true);

    let mut doc = SvgDocument::default();
    let mut group_stack: Vec<GroupState> = vec![GroupState::default()];
    let mut current_text: Option<TextElementState> = None;

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag_name = e.name().as_ref().to_ascii_lowercase();
                let attrs = extract_attributes(e);

                match tag_name.as_str() {
                    "svg" => {
                        parse_svg_root(&attrs, &mut doc);
                    }
                    "g" => {
                        let parent = group_stack.last().cloned().unwrap_or_default();
                        let new_group = parse_group_state(&attrs, &parent);
                        group_stack.push(new_group);
                    }
                    "text" => {
                        let parent = group_stack.last().unwrap();
                        current_text = Some(TextElementState::new(&attrs, parent));
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag_name = e.name().as_ref().to_ascii_lowercase();
                let attrs = extract_attributes(e);
                let current_group = group_stack.last().unwrap();

                match tag_name.as_str() {
                    "svg" => {
                        parse_svg_root(&attrs, &mut doc);
                    }
                    "path" => {
                        if let Some(node) = parse_path_element(&attrs, current_group) {
                            doc.scene.push(node);
                        }
                    }
                    "rect" => {
                        if let Some(node) = parse_rect_element(&attrs, current_group) {
                            doc.scene.push(node);
                        }
                    }
                    "circle" => {
                        if let Some(node) = parse_circle_element(&attrs, current_group) {
                            doc.scene.push(node);
                        }
                    }
                    "line" => {
                        if let Some(node) = parse_line_element(&attrs, current_group) {
                            doc.scene.push(node);
                        }
                    }
                    "polygon" | "polyline" => {
                        let is_closed = tag_name == "polygon";
                        if let Some(node) = parse_poly_element(&attrs, current_group, is_closed) {
                            doc.scene.push(node);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                if let Some(ref mut text_state) = current_text {
                    let text = e.as_ref();
                    text_state.content.push_str(text);
                }
            }
            Ok(Event::End(ref e)) => {
                let tag_name = e.name().as_ref().to_ascii_lowercase();
                match tag_name.as_str() {
                    "g" => {
                        if group_stack.len() > 1 {
                            group_stack.pop();
                        }
                    }
                    "text" => {
                        if let Some(text_state) = current_text.take() {
                            if let Some(node) = text_state.into_scene_node() {
                                doc.scene.push(node);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("SVG XML parse error: {e}")),
            _ => {}
        }
        buf.clear();
    }

    if doc.width <= 0.0 {
        doc.width = doc.view_box.map(|vb| vb.2).unwrap_or(800.0);
    }
    if doc.height <= 0.0 {
        doc.height = doc.view_box.map(|vb| vb.3).unwrap_or(600.0);
    }

    Ok(doc)
}

fn extract_attributes(e: &quick_xml::events::BytesStart) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for attr in e.attributes().flatten() {
        let key = attr.key.as_ref().to_string();
        let value = attr.value.to_string();
        map.insert(key, value);
    }
    map
}

fn parse_svg_root(attrs: &HashMap<String, String>, doc: &mut SvgDocument) {
    if let Some(w) = attrs.get("width").and_then(|s| parse_float_unit(s)) {
        doc.width = w;
    }
    if let Some(h) = attrs.get("height").and_then(|s| parse_float_unit(s)) {
        doc.height = h;
    }
    if let Some(vb) = attrs.get("viewBox").or_else(|| attrs.get("viewbox")) {
        let parts: Vec<f32> = vb
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter_map(|s| s.trim().parse::<f32>().ok())
            .collect();
        if parts.len() == 4 {
            doc.view_box = Some((parts[0], parts[1], parts[2], parts[3]));
            if doc.width <= 0.0 {
                doc.width = parts[2];
            }
            if doc.height <= 0.0 {
                doc.height = parts[3];
            }
        }
    }
}

fn parse_group_state(attrs: &HashMap<String, String>, parent: &GroupState) -> GroupState {
    let mut state = parent.clone();

    if let Some(t_str) = attrs.get("transform") {
        let local_transform = parse_transform(t_str);
        state.transform = combine_transforms(&state.transform, &local_transform);
    }

    let mut styles = HashMap::new();
    if let Some(style_str) = attrs.get("style") {
        parse_inline_style(style_str, &mut styles);
    }

    if let Some(op_str) = styles.get("opacity").or_else(|| attrs.get("opacity")) {
        if let Ok(op) = op_str.parse::<f32>() {
            state.opacity *= op.clamp(0.0, 1.0);
        }
    }

    if let Some(fill_str) = styles.get("fill").or_else(|| attrs.get("fill")) {
        state.fill = Some(parse_svg_color(fill_str));
    }
    if let Some(stroke_str) = styles.get("stroke").or_else(|| attrs.get("stroke")) {
        state.stroke = Some(parse_svg_color(stroke_str));
    }
    if let Some(sw_str) = styles
        .get("stroke-width")
        .or_else(|| attrs.get("stroke-width"))
    {
        if let Some(sw) = parse_float_unit(sw_str) {
            state.stroke_width = Some(sw);
        }
    }
    if let Some(fs_str) = styles.get("font-size").or_else(|| attrs.get("font-size")) {
        if let Some(fs) = parse_float_unit(fs_str) {
            state.font_size = Some(fs);
        }
    }
    if let Some(fw_str) = styles
        .get("font-weight")
        .or_else(|| attrs.get("font-weight"))
    {
        state.font_weight = Some(parse_font_weight(fw_str));
    }

    state
}

fn parse_path_element(attrs: &HashMap<String, String>, group: &GroupState) -> Option<SceneNode> {
    let d = attrs.get("d")?;
    if d.trim().is_empty() {
        return None;
    }

    let mut styles = HashMap::new();
    if let Some(s) = attrs.get("style") {
        parse_inline_style(s, &mut styles);
    }

    let fill_color = styles
        .get("fill")
        .or_else(|| attrs.get("fill"))
        .map(|s| parse_svg_color(s))
        .or(group.fill)
        .unwrap_or(Some(Color::BLACK));

    let stroke_color = styles
        .get("stroke")
        .or_else(|| attrs.get("stroke"))
        .map(|s| parse_svg_color(s))
        .or(group.stroke)
        .unwrap_or(None);

    let stroke_width = styles
        .get("stroke-width")
        .or_else(|| attrs.get("stroke-width"))
        .and_then(|s| parse_float_unit(s))
        .or(group.stroke_width)
        .unwrap_or(1.0);

    let opacity = styles
        .get("opacity")
        .or_else(|| attrs.get("opacity"))
        .and_then(|s| s.parse::<f32>().ok())
        .map(|op| op * group.opacity)
        .unwrap_or(group.opacity);

    let path_node = SceneNode::Path {
        d: d.clone(),
        fill: fill_color,
        stroke: stroke_color,
        stroke_width: if stroke_color.is_some() {
            stroke_width
        } else {
            0.0
        },
        opacity,
    };

    if let Some(t_str) = attrs.get("transform") {
        let local_t = parse_transform(t_str);
        let total_t = combine_transforms(&group.transform, &local_t);
        wrap_transformed(path_node, total_t)
    } else if group.transform != Transform2D::identity() {
        wrap_transformed(path_node, group.transform)
    } else {
        Some(path_node)
    }
}

fn parse_rect_element(attrs: &HashMap<String, String>, group: &GroupState) -> Option<SceneNode> {
    let x = attrs
        .get("x")
        .and_then(|s| parse_float_unit(s))
        .unwrap_or(0.0);
    let y = attrs
        .get("y")
        .and_then(|s| parse_float_unit(s))
        .unwrap_or(0.0);
    let w = attrs.get("width").and_then(|s| parse_float_unit(s))?;
    let h = attrs.get("height").and_then(|s| parse_float_unit(s))?;

    let rx = attrs
        .get("rx")
        .or_else(|| attrs.get("ry"))
        .and_then(|s| parse_float_unit(s))
        .unwrap_or(0.0);

    let mut styles = HashMap::new();
    if let Some(s) = attrs.get("style") {
        parse_inline_style(s, &mut styles);
    }

    let fill_color = styles
        .get("fill")
        .or_else(|| attrs.get("fill"))
        .map(|s| parse_svg_color(s))
        .or(group.fill)
        .unwrap_or(Some(Color::BLACK))
        .unwrap_or(Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        });

    let stroke_color = styles
        .get("stroke")
        .or_else(|| attrs.get("stroke"))
        .map(|s| parse_svg_color(s))
        .or(group.stroke)
        .unwrap_or(None);

    let stroke_width = styles
        .get("stroke-width")
        .or_else(|| attrs.get("stroke-width"))
        .and_then(|s| parse_float_unit(s))
        .or(group.stroke_width)
        .unwrap_or(0.0);

    let node = SceneNode::Rect {
        x,
        y,
        w,
        h,
        fill: fill_color,
        stroke: stroke_color,
        stroke_width,
        corner_radius: rx,
    };

    if let Some(t_str) = attrs.get("transform") {
        let local_t = parse_transform(t_str);
        let total_t = combine_transforms(&group.transform, &local_t);
        wrap_transformed(node, total_t)
    } else if group.transform != Transform2D::identity() {
        wrap_transformed(node, group.transform)
    } else {
        Some(node)
    }
}

fn parse_circle_element(attrs: &HashMap<String, String>, group: &GroupState) -> Option<SceneNode> {
    let cx = attrs
        .get("cx")
        .and_then(|s| parse_float_unit(s))
        .unwrap_or(0.0);
    let cy = attrs
        .get("cy")
        .and_then(|s| parse_float_unit(s))
        .unwrap_or(0.0);
    let r = attrs.get("r").and_then(|s| parse_float_unit(s))?;

    let mut styles = HashMap::new();
    if let Some(s) = attrs.get("style") {
        parse_inline_style(s, &mut styles);
    }

    let fill_color = styles
        .get("fill")
        .or_else(|| attrs.get("fill"))
        .map(|s| parse_svg_color(s))
        .or(group.fill)
        .unwrap_or(Some(Color::BLACK))
        .unwrap_or(Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        });

    let stroke_color = styles
        .get("stroke")
        .or_else(|| attrs.get("stroke"))
        .map(|s| parse_svg_color(s))
        .or(group.stroke)
        .unwrap_or(None);

    let stroke_width = styles
        .get("stroke-width")
        .or_else(|| attrs.get("stroke-width"))
        .and_then(|s| parse_float_unit(s))
        .or(group.stroke_width)
        .unwrap_or(0.0);

    let node = SceneNode::Circle {
        cx,
        cy,
        r,
        fill: fill_color,
        stroke: stroke_color,
        stroke_width,
    };

    if let Some(t_str) = attrs.get("transform") {
        let local_t = parse_transform(t_str);
        let total_t = combine_transforms(&group.transform, &local_t);
        wrap_transformed(node, total_t)
    } else if group.transform != Transform2D::identity() {
        wrap_transformed(node, group.transform)
    } else {
        Some(node)
    }
}

fn parse_line_element(attrs: &HashMap<String, String>, group: &GroupState) -> Option<SceneNode> {
    let x1 = attrs
        .get("x1")
        .and_then(|s| parse_float_unit(s))
        .unwrap_or(0.0);
    let y1 = attrs
        .get("y1")
        .and_then(|s| parse_float_unit(s))
        .unwrap_or(0.0);
    let x2 = attrs
        .get("x2")
        .and_then(|s| parse_float_unit(s))
        .unwrap_or(0.0);
    let y2 = attrs
        .get("y2")
        .and_then(|s| parse_float_unit(s))
        .unwrap_or(0.0);

    let d = format!("M {x1} {y1} L {x2} {y2}");

    let mut styles = HashMap::new();
    if let Some(s) = attrs.get("style") {
        parse_inline_style(s, &mut styles);
    }

    let stroke_color = styles
        .get("stroke")
        .or_else(|| attrs.get("stroke"))
        .map(|s| parse_svg_color(s))
        .or(group.stroke)
        .unwrap_or(Some(Color::BLACK));

    let stroke_width = styles
        .get("stroke-width")
        .or_else(|| attrs.get("stroke-width"))
        .and_then(|s| parse_float_unit(s))
        .or(group.stroke_width)
        .unwrap_or(1.0);

    let node = SceneNode::Path {
        d,
        fill: None,
        stroke: stroke_color,
        stroke_width,
        opacity: group.opacity,
    };

    if let Some(t_str) = attrs.get("transform") {
        let local_t = parse_transform(t_str);
        let total_t = combine_transforms(&group.transform, &local_t);
        wrap_transformed(node, total_t)
    } else if group.transform != Transform2D::identity() {
        wrap_transformed(node, group.transform)
    } else {
        Some(node)
    }
}

fn parse_poly_element(
    attrs: &HashMap<String, String>,
    group: &GroupState,
    closed: bool,
) -> Option<SceneNode> {
    let points_str = attrs.get("points")?;
    let nums: Vec<f32> = points_str
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter_map(|s| s.trim().parse::<f32>().ok())
        .collect();

    if nums.len() < 4 {
        return None;
    }

    let mut d = format!("M {} {}", nums[0], nums[1]);
    for chunk in nums[2..].chunks(2) {
        if chunk.len() == 2 {
            d.push_str(&format!(" L {} {}", chunk[0], chunk[1]));
        }
    }
    if closed {
        d.push_str(" Z");
    }

    let mut styles = HashMap::new();
    if let Some(s) = attrs.get("style") {
        parse_inline_style(s, &mut styles);
    }

    let fill_color = styles
        .get("fill")
        .or_else(|| attrs.get("fill"))
        .map(|s| parse_svg_color(s))
        .or(group.fill)
        .unwrap_or(if closed { Some(Color::BLACK) } else { None });

    let stroke_color = styles
        .get("stroke")
        .or_else(|| attrs.get("stroke"))
        .map(|s| parse_svg_color(s))
        .or(group.stroke)
        .unwrap_or(None);

    let stroke_width = styles
        .get("stroke-width")
        .or_else(|| attrs.get("stroke-width"))
        .and_then(|s| parse_float_unit(s))
        .or(group.stroke_width)
        .unwrap_or(1.0);

    let node = SceneNode::Path {
        d,
        fill: fill_color,
        stroke: stroke_color,
        stroke_width: if stroke_color.is_some() {
            stroke_width
        } else {
            0.0
        },
        opacity: group.opacity,
    };

    if let Some(t_str) = attrs.get("transform") {
        let local_t = parse_transform(t_str);
        let total_t = combine_transforms(&group.transform, &local_t);
        wrap_transformed(node, total_t)
    } else if group.transform != Transform2D::identity() {
        wrap_transformed(node, group.transform)
    } else {
        Some(node)
    }
}

#[derive(Debug)]
struct TextElementState {
    x: f32,
    y: f32,
    content: String,
    font_size: f32,
    font_weight: u16,
    color: Color,
    anchor: String,
    transform: Transform2D,
}

impl TextElementState {
    fn new(attrs: &HashMap<String, String>, group: &GroupState) -> Self {
        let x = attrs
            .get("x")
            .and_then(|s| parse_float_unit(s))
            .unwrap_or(0.0);
        let y = attrs
            .get("y")
            .and_then(|s| parse_float_unit(s))
            .unwrap_or(0.0);

        let mut styles = HashMap::new();
        if let Some(s) = attrs.get("style") {
            parse_inline_style(s, &mut styles);
        }

        let font_size = styles
            .get("font-size")
            .or_else(|| attrs.get("font-size"))
            .and_then(|s| parse_float_unit(s))
            .or(group.font_size)
            .unwrap_or(16.0);

        let font_weight = styles
            .get("font-weight")
            .or_else(|| attrs.get("font-weight"))
            .map(|s| parse_font_weight(s))
            .or(group.font_weight)
            .unwrap_or(400);

        let color = styles
            .get("fill")
            .or_else(|| attrs.get("fill"))
            .and_then(|s| parse_svg_color(s))
            .or_else(|| group.fill.flatten())
            .unwrap_or(Color::BLACK);

        let anchor = styles
            .get("text-anchor")
            .or_else(|| attrs.get("text-anchor"))
            .cloned()
            .unwrap_or_else(|| "start".into());

        let transform = if let Some(t_str) = attrs.get("transform") {
            let local_t = parse_transform(t_str);
            combine_transforms(&group.transform, &local_t)
        } else {
            group.transform
        };

        Self {
            x,
            y,
            content: String::new(),
            font_size,
            font_weight,
            color,
            anchor,
            transform,
        }
    }

    fn into_scene_node(self) -> Option<SceneNode> {
        let content = self.content.trim().to_string();
        if content.is_empty() {
            return None;
        }

        // Adjust horizontal placement based on text-anchor
        let approx_width = content.chars().count() as f32 * self.font_size * 0.55;
        let final_x = match self.anchor.as_str() {
            "middle" => self.x - (approx_width / 2.0),
            "end" => self.x - approx_width,
            _ => self.x,
        };

        // In SVG, y is the text baseline; offset slightly to top-left origin
        let final_y = (self.y - self.font_size * 0.8).max(0.0);

        let node = SceneNode::Text {
            x: final_x,
            y: final_y,
            content,
            font_size: self.font_size,
            color: self.color,
            font_weight: self.font_weight,
            font_sources: Vec::new(),
        };

        if self.transform != Transform2D::identity() {
            wrap_transformed(node, self.transform)
        } else {
            Some(node)
        }
    }
}

fn wrap_transformed(node: SceneNode, transform: Transform2D) -> Option<SceneNode> {
    if transform == Transform2D::identity() {
        Some(node)
    } else {
        Some(SceneNode::Group {
            transform,
            opacity: 1.0,
            children: vec![node],
        })
    }
}

fn parse_inline_style(style_str: &str, map: &mut HashMap<String, String>) {
    for decl in style_str.split(';') {
        if let Some((k, v)) = decl.split_once(':') {
            map.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
}

fn parse_float_unit(s: &str) -> Option<f32> {
    let s = s.trim().trim_end_matches("px").trim_end_matches("pt");
    s.parse::<f32>().ok()
}

fn parse_font_weight(s: &str) -> u16 {
    match s.trim().to_ascii_lowercase().as_str() {
        "bold" => 700,
        "normal" => 400,
        "lighter" => 300,
        "bolder" => 800,
        val => val.parse::<u16>().unwrap_or(400),
    }
}

/// Parse SVG color: hex `#fff`, `#112233`, rgb, named colors, or `none`.
pub fn parse_svg_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") || s.eq_ignore_ascii_case("transparent") {
        return None;
    }
    Color::from_css(s)
}

/// Parse standard SVG transform string:
/// `translate(x, [y])`, `scale(sx, [sy])`, `rotate(angle, [cx, cy])`
pub fn parse_transform(transform_str: &str) -> Transform2D {
    let mut current = Transform2D::identity();
    let s = transform_str.trim();

    let mut start = 0;
    while let Some(open) = s[start..].find('(') {
        let open_idx = start + open;
        let func_name = s[start..open_idx].trim().to_ascii_lowercase();

        if let Some(close) = s[open_idx..].find(')') {
            let close_idx = open_idx + close;
            let args_str = &s[open_idx + 1..close_idx];
            let args: Vec<f32> = args_str
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter_map(|p| p.trim().parse::<f32>().ok())
                .collect();

            match func_name.as_str() {
                "translate" => {
                    let tx = args.first().copied().unwrap_or(0.0);
                    let ty = args.get(1).copied().unwrap_or(0.0);
                    let t = Transform2D::translate(tx, ty);
                    current = combine_transforms(&current, &t);
                }
                "scale" => {
                    let sx = args.first().copied().unwrap_or(1.0);
                    let sy = args.get(1).copied().unwrap_or(sx);
                    let t = Transform2D::scale(sx, sy);
                    current = combine_transforms(&current, &t);
                }
                "rotate" => {
                    let deg = args.first().copied().unwrap_or(0.0);
                    if args.len() >= 3 {
                        let cx = args[1];
                        let cy = args[2];
                        let t1 = Transform2D::translate(cx, cy);
                        let t2 = Transform2D::rotate(deg);
                        let t3 = Transform2D::translate(-cx, -cy);
                        let rot_center = combine_transforms(&combine_transforms(&t1, &t2), &t3);
                        current = combine_transforms(&current, &rot_center);
                    } else {
                        let t = Transform2D::rotate(deg);
                        current = combine_transforms(&current, &t);
                    }
                }
                _ => {}
            }
            start = close_idx + 1;
        } else {
            break;
        }
    }

    current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_svg_rect_and_circle() {
        let svg = r##"
            <svg width="400" height="300" viewBox="0 0 400 300">
                <rect x="10" y="20" width="100" height="50" rx="4" fill="#ff0000" stroke="#000000" stroke-width="2"/>
                <circle cx="200" cy="150" r="30" fill="#00ff00"/>
            </svg>
        "##;

        let doc = parse_svg(svg).expect("parse svg");
        assert_eq!(doc.width, 400.0);
        assert_eq!(doc.height, 300.0);
        assert_eq!(doc.scene.nodes.len(), 2);

        match &doc.scene.nodes[0] {
            SceneNode::Rect {
                x,
                y,
                w,
                h,
                fill,
                corner_radius,
                ..
            } => {
                assert_eq!(*x, 10.0);
                assert_eq!(*y, 20.0);
                assert_eq!(*w, 100.0);
                assert_eq!(*h, 50.0);
                assert_eq!(*corner_radius, 4.0);
                assert_eq!(
                    *fill,
                    Color {
                        r: 255,
                        g: 0,
                        b: 0,
                        a: 255
                    }
                );
            }
            _ => panic!("Expected Rect"),
        }

        match &doc.scene.nodes[1] {
            SceneNode::Circle {
                cx, cy, r, fill, ..
            } => {
                assert_eq!(*cx, 200.0);
                assert_eq!(*cy, 150.0);
                assert_eq!(*r, 30.0);
                assert_eq!(
                    *fill,
                    Color {
                        r: 0,
                        g: 255,
                        b: 0,
                        a: 255
                    }
                );
            }
            _ => panic!("Expected Circle"),
        }
    }

    #[test]
    fn test_parse_svg_path_and_transform_group() {
        let svg = r##"
            <svg width="600" height="400">
                <g transform="translate(50, 60)">
                    <path d="M 0 0 L 100 0 L 50 100 Z" fill="#3b82f6" stroke="#1d4ed8" stroke-width="3"/>
                </g>
            </svg>
        "##;

        let doc = parse_svg(svg).expect("parse svg");
        assert_eq!(doc.scene.nodes.len(), 1);

        match &doc.scene.nodes[0] {
            SceneNode::Group {
                transform,
                children,
                ..
            } => {
                assert_eq!(*transform, Transform2D::translate(50.0, 60.0));
                assert_eq!(children.len(), 1);
                match &children[0] {
                    SceneNode::Path {
                        d,
                        fill,
                        stroke,
                        stroke_width,
                        ..
                    } => {
                        assert_eq!(d, "M 0 0 L 100 0 L 50 100 Z");
                        assert_eq!(*fill, Some(Color::rgb(0x3b, 0x82, 0xf6)));
                        assert_eq!(*stroke, Some(Color::rgb(0x1d, 0x4e, 0xd8)));
                        assert_eq!(*stroke_width, 3.0);
                    }
                    _ => panic!("Expected Path inside group"),
                }
            }
            _ => panic!("Expected Group"),
        }
    }

    #[test]
    fn test_parse_svg_text_element() {
        let svg = r##"
            <svg width="500" height="200">
                <text x="100" y="50" font-size="24" font-weight="bold" fill="#ef4444" text-anchor="middle">
                    Quarterly Growth
                </text>
            </svg>
        "##;

        let doc = parse_svg(svg).expect("parse svg text");
        assert_eq!(doc.scene.nodes.len(), 1);

        match &doc.scene.nodes[0] {
            SceneNode::Text {
                content,
                font_size,
                font_weight,
                color,
                ..
            } => {
                assert_eq!(content, "Quarterly Growth");
                assert_eq!(*font_size, 24.0);
                assert_eq!(*font_weight, 700);
                assert_eq!(*color, Color::rgb(0xef, 0x44, 0x44));
            }
            _ => panic!("Expected Text node"),
        }
    }
}
