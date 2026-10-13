//! Mermaid diagram parser and native vector [`SceneNode`] generator.
//!
//! Supports Flowcharts (`graph TD`, `graph LR`, etc.) and Sequence Diagrams (`sequenceDiagram`)
//! with automatic graph layout and zero Chromium overhead.

use crate::svg_parser::SvgDocument;
use rust_motion_rasterizer::{Color, Scene, SceneNode};
use std::collections::{HashMap, VecDeque};

/// Diagram orientation for flowcharts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    TopToBottom,
    BottomToTop,
    LeftToRight,
    RightToLeft,
}

/// Node shape representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodeShape {
    #[default]
    Rectangle, // [text]
    Rounded,  // (text)
    Stadium,  // ([text])
    Database, // [(text)]
    Circle,   // ((text))
    Diamond,  // {text}
    Hexagon,  // {{text}}
}

/// Edge connector style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdgeStyle {
    #[default]
    Arrow, // -->
    Open,   // ---
    Dotted, // -.->
    Thick,  // ==>
}

/// Parsed node in a flowchart.
#[derive(Debug, Clone)]
pub struct FlowNode {
    pub id: String,
    pub label: String,
    pub shape: NodeShape,
}

/// Parsed directed edge in a flowchart.
#[derive(Debug, Clone)]
pub struct FlowEdge {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
    pub style: EdgeStyle,
}

/// Theme settings for diagram rendering.
#[derive(Debug, Clone)]
pub struct DiagramTheme {
    pub background: Option<Color>,
    pub node_fill: Color,
    pub node_stroke: Color,
    pub text_color: Color,
    pub edge_color: Color,
    pub font_size: f32,
    pub line_width: f32,
}

impl Default for DiagramTheme {
    fn default() -> Self {
        Self::dark()
    }
}

impl DiagramTheme {
    pub fn dark() -> Self {
        Self {
            background: Some(Color::rgb(0x0f, 0x17, 0x2a)), // slate-900
            node_fill: Color::rgb(0x1e, 0x29, 0x3b),        // slate-800
            node_stroke: Color::rgb(0x3b, 0x82, 0xf6),      // blue-500
            text_color: Color::rgb(0xf8, 0xfa, 0xfc),       // slate-50
            edge_color: Color::rgb(0x94, 0xa3, 0xb8),       // slate-400
            font_size: 14.0,
            line_width: 2.0,
        }
    }

    pub fn light() -> Self {
        Self {
            background: Some(Color::rgb(0xf8, 0xfa, 0xfc)),
            node_fill: Color::rgb(0xff, 0xff, 0xff),
            node_stroke: Color::rgb(0x25, 0x63, 0xeb),
            text_color: Color::rgb(0x0f, 0x17, 0x2a),
            edge_color: Color::rgb(0x64, 0x74, 0x8b),
            font_size: 14.0,
            line_width: 2.0,
        }
    }
}

/// Parse Mermaid code and render directly to a native [`Scene`].
pub fn render_mermaid(code: &str, theme: Option<&DiagramTheme>) -> Result<Scene, String> {
    let default_theme = DiagramTheme::default();
    let theme = theme.unwrap_or(&default_theme);

    let trimmed = code.trim();
    if trimmed.starts_with("sequenceDiagram") {
        render_sequence_diagram(trimmed, theme)
    } else if trimmed.starts_with("graph ") || trimmed.starts_with("flowchart ") {
        render_flowchart(trimmed, theme)
    } else {
        // Fallback: attempt to parse as flowchart
        render_flowchart(trimmed, theme)
    }
}

/// Parse Mermaid code and render as an [`SvgDocument`].
pub fn render_mermaid_svg(code: &str, theme: Option<&DiagramTheme>) -> Result<SvgDocument, String> {
    let scene = render_mermaid(code, theme)?;
    let (mut max_w, mut max_h) = (800.0f32, 600.0f32);

    for node in &scene.nodes {
        match node {
            SceneNode::Rect { x, y, w, h, .. } => {
                max_w = max_w.max(x + w + 40.0);
                max_h = max_h.max(y + h + 40.0);
            }
            SceneNode::Text { x, y, .. } => {
                max_w = max_w.max(x + 100.0);
                max_h = max_h.max(y + 30.0);
            }
            _ => {}
        }
    }

    Ok(SvgDocument {
        width: max_w,
        height: max_h,
        view_box: Some((0.0, 0.0, max_w, max_h)),
        scene,
    })
}

// -----------------------------------------------------------------------------
// Flowchart Parser & Renderer
// -----------------------------------------------------------------------------

fn render_flowchart(code: &str, theme: &DiagramTheme) -> Result<Scene, String> {
    let mut dir = Direction::TopToBottom;
    let mut nodes: HashMap<String, FlowNode> = HashMap::new();
    let mut edges: Vec<FlowEdge> = Vec::new();

    for line in code.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }

        // Split multiple statements on one line separated by `;`
        for statement in line.split(';') {
            let stmt = statement.trim();
            if stmt.is_empty() {
                continue;
            }

            if stmt.starts_with("graph ") || stmt.starts_with("flowchart ") {
                let parts: Vec<&str> = stmt.split_whitespace().collect();
                if parts.len() >= 2 {
                    dir = match parts[1].to_ascii_uppercase().as_str() {
                        "LR" => Direction::LeftToRight,
                        "RL" => Direction::RightToLeft,
                        "BT" => Direction::BottomToTop,
                        _ => Direction::TopToBottom,
                    };
                }
                continue;
            }

            parse_flowchart_statement(stmt, &mut nodes, &mut edges);
        }
    }

    if nodes.is_empty() {
        return Ok(Scene::default());
    }

    // Assign topological layers (ranks)
    let node_ids: Vec<String> = nodes.keys().cloned().collect();
    let mut in_degrees: HashMap<String, usize> = HashMap::new();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();

    for id in &node_ids {
        in_degrees.insert(id.clone(), 0);
        adj.insert(id.clone(), Vec::new());
    }
    for e in &edges {
        if let Some(deg) = in_degrees.get_mut(&e.to) {
            *deg += 1;
        }
        if let Some(list) = adj.get_mut(&e.from) {
            list.push(e.to.clone());
        }
    }

    // Kahn's algorithm for layering
    let mut queue = VecDeque::new();
    let mut node_ranks: HashMap<String, usize> = HashMap::new();

    for (id, &deg) in &in_degrees {
        if deg == 0 {
            queue.push_back(id.clone());
            node_ranks.insert(id.clone(), 0);
        }
    }

    while let Some(u) = queue.pop_front() {
        let current_rank = *node_ranks.get(&u).unwrap_or(&0);
        if let Some(neighbors) = adj.get(&u) {
            for v in neighbors {
                let next_rank = current_rank + 1;
                let existing = node_ranks.entry(v.clone()).or_insert(0);
                *existing = (*existing).max(next_rank);
                let deg = in_degrees.get_mut(v).unwrap();
                *deg = deg.saturating_sub(1);
                if *deg == 0 {
                    queue.push_back(v.clone());
                }
            }
        }
    }

    // For any unvisited nodes (cycles or islands), default to rank 0
    for id in &node_ids {
        node_ranks.entry(id.clone()).or_insert(0);
    }

    // Group nodes by rank
    let mut max_rank = 0;
    let mut rank_groups: HashMap<usize, Vec<String>> = HashMap::new();
    for (id, &rank) in &node_ranks {
        max_rank = max_rank.max(rank);
        rank_groups.entry(rank).or_default().push(id.clone());
    }

    // Compute layout positions
    let mut scene = Scene::default();
    let mut node_positions: HashMap<String, (f32, f32, f32, f32)> = HashMap::new(); // (center_x, center_y, width, height)

    let layer_gap = 120.0f32;
    let node_gap = 40.0f32;
    let margin = 50.0f32;

    match dir {
        Direction::TopToBottom | Direction::BottomToTop => {
            for rank in 0..=max_rank {
                let current_rank = if dir == Direction::BottomToTop {
                    max_rank - rank
                } else {
                    rank
                };
                let group = rank_groups.get(&current_rank).cloned().unwrap_or_default();
                let cy = margin + rank as f32 * layer_gap + 40.0;

                let mut total_row_w = 0.0f32;
                let mut node_sizes = Vec::new();
                for id in &group {
                    let node = &nodes[id];
                    let text_w =
                        (node.label.chars().count() as f32 * theme.font_size * 0.65).max(60.0);
                    let nw = text_w + 36.0;
                    let nh = 46.0f32;
                    node_sizes.push((nw, nh));
                    total_row_w += nw;
                }
                total_row_w += (group.len().saturating_sub(1) as f32) * node_gap;

                let mut start_x = margin + 400.0 - (total_row_w / 2.0);
                if start_x < margin {
                    start_x = margin;
                }

                for (i, id) in group.iter().enumerate() {
                    let (nw, nh) = node_sizes[i];
                    let cx = start_x + nw / 2.0;
                    node_positions.insert(id.clone(), (cx, cy, nw, nh));
                    start_x += nw + node_gap;
                }
            }
        }
        Direction::LeftToRight | Direction::RightToLeft => {
            for rank in 0..=max_rank {
                let current_rank = if dir == Direction::RightToLeft {
                    max_rank - rank
                } else {
                    rank
                };
                let group = rank_groups.get(&current_rank).cloned().unwrap_or_default();
                let cx = margin + rank as f32 * (layer_gap + 80.0) + 70.0;

                let mut total_col_h = 0.0f32;
                let mut node_sizes = Vec::new();
                for id in &group {
                    let node = &nodes[id];
                    let text_w =
                        (node.label.chars().count() as f32 * theme.font_size * 0.65).max(60.0);
                    let nw = text_w + 36.0;
                    let nh = 46.0f32;
                    node_sizes.push((nw, nh));
                    total_col_h += nh;
                }
                total_col_h += (group.len().saturating_sub(1) as f32) * node_gap;

                let mut start_y = margin + 300.0 - (total_col_h / 2.0);
                if start_y < margin {
                    start_y = margin;
                }

                for (i, id) in group.iter().enumerate() {
                    let (nw, nh) = node_sizes[i];
                    let cy = start_y + nh / 2.0;
                    node_positions.insert(id.clone(), (cx, cy, nw, nh));
                    start_y += nh + node_gap;
                }
            }
        }
    }

    // Draw edges first so they sit below nodes
    for edge in &edges {
        if let (Some(&from_pos), Some(&to_pos)) =
            (node_positions.get(&edge.from), node_positions.get(&edge.to))
        {
            let (x1, y1, x2, y2) = match dir {
                Direction::TopToBottom => (
                    from_pos.0,
                    from_pos.1 + from_pos.3 / 2.0,
                    to_pos.0,
                    to_pos.1 - to_pos.3 / 2.0,
                ),
                Direction::BottomToTop => (
                    from_pos.0,
                    from_pos.1 - from_pos.3 / 2.0,
                    to_pos.0,
                    to_pos.1 + to_pos.3 / 2.0,
                ),
                Direction::LeftToRight => (
                    from_pos.0 + from_pos.2 / 2.0,
                    from_pos.1,
                    to_pos.0 - to_pos.2 / 2.0,
                    to_pos.1,
                ),
                Direction::RightToLeft => (
                    from_pos.0 - from_pos.2 / 2.0,
                    from_pos.1,
                    to_pos.0 + to_pos.2 / 2.0,
                    to_pos.1,
                ),
            };

            // Connecting curve / line
            let d = if (x1 - x2).abs() < 5.0 || (y1 - y2).abs() < 5.0 {
                format!("M {:.1} {:.1} L {:.1} {:.1}", x1, y1, x2, y2)
            } else {
                let mx = (x1 + x2) / 2.0;
                let my = (y1 + y2) / 2.0;
                match dir {
                    Direction::TopToBottom | Direction::BottomToTop => {
                        format!(
                            "M {:.1} {:.1} C {:.1} {:.1}, {:.1} {:.1}, {:.1} {:.1}",
                            x1, y1, x1, my, x2, my, x2, y2
                        )
                    }
                    Direction::LeftToRight | Direction::RightToLeft => {
                        format!(
                            "M {:.1} {:.1} C {:.1} {:.1}, {:.1} {:.1}, {:.1} {:.1}",
                            x1, y1, mx, y1, mx, y2, x2, y2
                        )
                    }
                }
            };

            let stroke_width = if edge.style == EdgeStyle::Thick {
                3.5
            } else {
                theme.line_width
            };
            scene.push(SceneNode::Path {
                d,
                fill: None,
                stroke: Some(theme.edge_color),
                stroke_width,
                opacity: 1.0,
            });

            // Arrowhead at endpoint
            if edge.style != EdgeStyle::Open {
                let arrow_size = 7.0f32;
                let (ax, ay) = (x2, y2);
                let (tip_d, left_d, right_d) = match dir {
                    Direction::TopToBottom => (
                        (0.0, 0.0),
                        (-arrow_size, -arrow_size * 1.5),
                        (arrow_size, -arrow_size * 1.5),
                    ),
                    Direction::BottomToTop => (
                        (0.0, 0.0),
                        (-arrow_size, arrow_size * 1.5),
                        (arrow_size, arrow_size * 1.5),
                    ),
                    Direction::LeftToRight => (
                        (0.0, 0.0),
                        (-arrow_size * 1.5, -arrow_size),
                        (-arrow_size * 1.5, arrow_size),
                    ),
                    Direction::RightToLeft => (
                        (0.0, 0.0),
                        (arrow_size * 1.5, -arrow_size),
                        (arrow_size * 1.5, arrow_size),
                    ),
                };
                let head_d = format!(
                    "M {:.1} {:.1} L {:.1} {:.1} L {:.1} {:.1} Z",
                    ax + tip_d.0,
                    ay + tip_d.1,
                    ax + left_d.0,
                    ay + left_d.1,
                    ax + right_d.0,
                    ay + right_d.1
                );
                scene.push(SceneNode::Path {
                    d: head_d,
                    fill: Some(theme.edge_color),
                    stroke: None,
                    stroke_width: 0.0,
                    opacity: 1.0,
                });
            }

            // Edge label
            if let Some(ref lbl) = edge.label {
                let lx = (x1 + x2) / 2.0 - (lbl.len() as f32 * 3.5);
                let ly = (y1 + y2) / 2.0 - 10.0;
                scene.push(SceneNode::Text {
                    x: lx,
                    y: ly,
                    content: lbl.clone(),
                    font_size: 11.0,
                    color: theme.edge_color,
                    font_weight: 500,
                    font_sources: Vec::new(),
                });
            }
        }
    }

    // Draw nodes
    for (id, node) in &nodes {
        if let Some(&(cx, cy, nw, nh)) = node_positions.get(id) {
            let bx = cx - nw / 2.0;
            let by = cy - nh / 2.0;

            let corner_radius = match node.shape {
                NodeShape::Rectangle => 4.0,
                NodeShape::Rounded => 12.0,
                NodeShape::Stadium => nh / 2.0,
                NodeShape::Circle => nw / 2.0,
                NodeShape::Database => 6.0,
                NodeShape::Diamond => 0.0,
                NodeShape::Hexagon => 6.0,
            };

            // Node box
            scene.push(SceneNode::Rect {
                x: bx,
                y: by,
                w: nw,
                h: nh,
                fill: theme.node_fill,
                stroke: Some(theme.node_stroke),
                stroke_width: 2.0,
                corner_radius,
            });

            // If database shape, add top decorative ellipse curve
            if node.shape == NodeShape::Database {
                let d = format!(
                    "M {:.1} {:.1} A {:.1} 6.0 0 0 1 {:.1} {:.1}",
                    bx,
                    by + 10.0,
                    nw / 2.0,
                    bx + nw,
                    by + 10.0
                );
                scene.push(SceneNode::Path {
                    d,
                    fill: None,
                    stroke: Some(theme.node_stroke),
                    stroke_width: 1.5,
                    opacity: 0.7,
                });
            }

            // Text inside node
            let approx_text_w = node.label.chars().count() as f32 * theme.font_size * 0.55;
            let tx = cx - (approx_text_w / 2.0);
            let ty = cy - (theme.font_size * 0.4);

            scene.push(SceneNode::Text {
                x: tx,
                y: ty,
                content: node.label.clone(),
                font_size: theme.font_size,
                color: theme.text_color,
                font_weight: 600,
                font_sources: Vec::new(),
            });
        }
    }

    Ok(scene)
}

fn parse_flowchart_statement(
    stmt: &str,
    nodes: &mut HashMap<String, FlowNode>,
    edges: &mut Vec<FlowEdge>,
) {
    // Check for link patterns: `-->|label|`, `-- label -->`, `-->`, `---`, `-.->`, `==>`
    let link_patterns = ["==>", "-.->", "-->", "---"];

    let mut found_pattern = None;
    for &pat in &link_patterns {
        if let Some(pos) = stmt.find(pat) {
            found_pattern = Some((pat, pos));
            break;
        }
    }

    if let Some((pat, pos)) = found_pattern {
        let left_part = stmt[..pos].trim();
        let right_part = stmt[pos + pat.len()..].trim();

        // Check if there is an inline link label: `-->|label| Target` or `-- label --> Target`
        let mut edge_label = None;
        let mut final_right = right_part;

        if let Some(stripped) = right_part.strip_prefix('|') {
            if let Some(close_bar) = stripped.find('|') {
                edge_label = Some(stripped[..close_bar].trim().to_string());
                final_right = stripped[close_bar + 1..].trim();
            }
        }

        let from_node = parse_node_spec(left_part);
        let to_node = parse_node_spec(final_right);

        let style = match pat {
            "==>" => EdgeStyle::Thick,
            "-.->" => EdgeStyle::Dotted,
            "---" => EdgeStyle::Open,
            _ => EdgeStyle::Arrow,
        };

        edges.push(FlowEdge {
            from: from_node.id.clone(),
            to: to_node.id.clone(),
            label: edge_label,
            style,
        });

        nodes.entry(from_node.id.clone()).or_insert(from_node);
        nodes.entry(to_node.id.clone()).or_insert(to_node);
    } else {
        // Single node statement (e.g. `A[Hello World]`)
        let node = parse_node_spec(stmt);
        if !node.id.is_empty() {
            nodes.insert(node.id.clone(), node);
        }
    }
}

fn parse_node_spec(spec: &str) -> FlowNode {
    let s = spec.trim();

    // Check for `A[(Database)]`
    if let Some(start) = s.find("[(") {
        if let Some(end) = s[start..].rfind(")]") {
            let id = s[..start].trim().to_string();
            let label = s[start + 2..start + end].trim().to_string();
            return FlowNode {
                id,
                label,
                shape: NodeShape::Database,
            };
        }
    }
    // `A([Stadium])`
    if let Some(start) = s.find("([") {
        if let Some(end) = s[start..].rfind("])") {
            let id = s[..start].trim().to_string();
            let label = s[start + 2..start + end].trim().to_string();
            return FlowNode {
                id,
                label,
                shape: NodeShape::Stadium,
            };
        }
    }
    // `A((Circle))`
    if let Some(start) = s.find("((") {
        if let Some(end) = s[start..].rfind("))") {
            let id = s[..start].trim().to_string();
            let label = s[start + 2..start + end].trim().to_string();
            return FlowNode {
                id,
                label,
                shape: NodeShape::Circle,
            };
        }
    }
    // `A{{Hexagon}}`
    if let Some(start) = s.find("{{") {
        if let Some(end) = s[start..].rfind("}}") {
            let id = s[..start].trim().to_string();
            let label = s[start + 2..start + end].trim().to_string();
            return FlowNode {
                id,
                label,
                shape: NodeShape::Hexagon,
            };
        }
    }
    // `A{Diamond}`
    if let Some(start) = s.find('{') {
        if let Some(end) = s[start..].rfind('}') {
            let id = s[..start].trim().to_string();
            let label = s[start + 1..start + end].trim().to_string();
            return FlowNode {
                id,
                label,
                shape: NodeShape::Diamond,
            };
        }
    }
    // `A(Rounded)`
    if let Some(start) = s.find('(') {
        if let Some(end) = s[start..].rfind(')') {
            let id = s[..start].trim().to_string();
            let label = s[start + 1..start + end].trim().to_string();
            return FlowNode {
                id,
                label,
                shape: NodeShape::Rounded,
            };
        }
    }
    // `A[Rectangle]`
    if let Some(start) = s.find('[') {
        if let Some(end) = s[start..].rfind(']') {
            let id = s[..start].trim().to_string();
            let label = s[start + 1..start + end].trim().to_string();
            return FlowNode {
                id,
                label,
                shape: NodeShape::Rectangle,
            };
        }
    }

    // Bare identifier
    FlowNode {
        id: s.to_string(),
        label: s.to_string(),
        shape: NodeShape::Rectangle,
    }
}

// -----------------------------------------------------------------------------
// Sequence Diagram Parser & Renderer
// -----------------------------------------------------------------------------

fn render_sequence_diagram(code: &str, theme: &DiagramTheme) -> Result<Scene, String> {
    let mut participants: Vec<String> = Vec::new();
    let mut messages: Vec<(String, String, String, bool)> = Vec::new(); // (from, to, text, is_dashed)

    for line in code.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with("%%")
            || line == "sequenceDiagram"
            || line == "autonumber"
        {
            continue;
        }

        if line.starts_with("participant ") || line.starts_with("actor ") {
            let name = line.split_whitespace().nth(1).unwrap_or("").trim();
            if !name.is_empty() && !participants.contains(&name.to_string()) {
                participants.push(name.to_string());
            }
            continue;
        }

        // Message arrows: `->>` (solid), `-->>` (dashed), `->`
        let (from_to, is_dashed) = if let Some(idx) = line.find("-->>") {
            ((&line[..idx], &line[idx + 4..]), true)
        } else if let Some(idx) = line.find("->>") {
            ((&line[..idx], &line[idx + 3..]), false)
        } else if let Some(idx) = line.find("->") {
            ((&line[..idx], &line[idx + 2..]), false)
        } else {
            continue;
        };

        let from = from_to.0.trim().to_string();
        let (to, msg_text) = if let Some((to_part, text_part)) = from_to.1.split_once(':') {
            (to_part.trim().to_string(), text_part.trim().to_string())
        } else {
            (from_to.1.trim().to_string(), String::new())
        };

        if !participants.contains(&from) {
            participants.push(from.clone());
        }
        if !participants.contains(&to) {
            participants.push(to.clone());
        }

        messages.push((from, to, msg_text, is_dashed));
    }

    if participants.is_empty() {
        return Ok(Scene::default());
    }

    let mut scene = Scene::default();
    let margin = 60.0f32;
    let col_gap = 180.0f32;
    let row_gap = 60.0f32;
    let box_w = 120.0f32;
    let box_h = 44.0f32;

    let total_h = margin + box_h + 30.0 + (messages.len() as f32 * row_gap) + 50.0;

    let mut col_x: HashMap<String, f32> = HashMap::new();

    // Draw participant boxes and lifelines
    for (i, p) in participants.iter().enumerate() {
        let cx = margin + (i as f32 * col_gap) + box_w / 2.0;
        col_x.insert(p.clone(), cx);

        let bx = cx - box_w / 2.0;
        let by = margin;

        // Top Participant Box
        scene.push(SceneNode::Rect {
            x: bx,
            y: by,
            w: box_w,
            h: box_h,
            fill: theme.node_fill,
            stroke: Some(theme.node_stroke),
            stroke_width: 2.0,
            corner_radius: 6.0,
        });

        // Participant label
        let text_w = p.chars().count() as f32 * theme.font_size * 0.55;
        scene.push(SceneNode::Text {
            x: cx - text_w / 2.0,
            y: by + (box_h - theme.font_size) / 2.0,
            content: p.clone(),
            font_size: theme.font_size,
            color: theme.text_color,
            font_weight: 600,
            font_sources: Vec::new(),
        });

        // Vertical Lifeline
        scene.push(SceneNode::Path {
            d: format!("M {:.1} {:.1} L {:.1} {:.1}", cx, by + box_h, cx, total_h),
            fill: None,
            stroke: Some(Color::rgba(0x64, 0x74, 0x8b, 0x80)), // muted slate-500
            stroke_width: 1.5,
            opacity: 1.0,
        });
    }

    // Draw message arrows
    for (i, (from, to, text, _is_dashed)) in messages.iter().enumerate() {
        if let (Some(&x1), Some(&x2)) = (col_x.get(from), col_x.get(to)) {
            let my = margin + box_h + 40.0 + (i as f32 * row_gap);

            // Arrow line
            scene.push(SceneNode::Path {
                d: format!("M {:.1} {:.1} L {:.1} {:.1}", x1, my, x2, my),
                fill: None,
                stroke: Some(theme.edge_color),
                stroke_width: theme.line_width,
                opacity: 1.0,
            });

            // Arrowhead at endpoint
            let dir_sign = if x2 > x1 { 1.0f32 } else { -1.0f32 };
            let arrow_size = 7.0f32;
            let head_d = format!(
                "M {:.1} {:.1} L {:.1} {:.1} L {:.1} {:.1} Z",
                x2,
                my,
                x2 - dir_sign * arrow_size * 1.5,
                my - arrow_size,
                x2 - dir_sign * arrow_size * 1.5,
                my + arrow_size
            );
            scene.push(SceneNode::Path {
                d: head_d,
                fill: Some(theme.edge_color),
                stroke: None,
                stroke_width: 0.0,
                opacity: 1.0,
            });

            // Text label above arrow
            if !text.is_empty() {
                let text_w = text.chars().count() as f32 * 12.0 * 0.55;
                let tx = (x1 + x2) / 2.0 - (text_w / 2.0);
                scene.push(SceneNode::Text {
                    x: tx,
                    y: my - 16.0,
                    content: text.clone(),
                    font_size: 12.0,
                    color: theme.text_color,
                    font_weight: 500,
                    font_sources: Vec::new(),
                });
            }
        }
    }

    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_flowchart_td() {
        let code = r#"
            graph TD
                A[Client] --> B(API Gateway)
                B --> C[(Database)]
                B --> D[Cache]
        "#;

        let scene = render_mermaid(code, None).expect("render flowchart");
        let rect_count = scene
            .nodes
            .iter()
            .filter(|n| matches!(n, SceneNode::Rect { .. }))
            .count();
        assert_eq!(rect_count, 4, "Expected 4 node boxes");

        let text_count = scene
            .nodes
            .iter()
            .filter(|n| matches!(n, SceneNode::Text { .. }))
            .count();
        assert!(text_count >= 4, "Expected at least 4 text labels");
    }

    #[test]
    fn test_render_flowchart_lr_with_labels() {
        let code = r#"
            graph LR
                Ingest -->|Video stream| GPU_Render
                GPU_Render ==>|Metal/NVENC| Hardware_Encode
        "#;

        let scene = render_mermaid(code, None).expect("render flowchart LR");
        assert!(!scene.nodes.is_empty());
    }

    #[test]
    fn test_render_sequence_diagram() {
        let code = r#"
            sequenceDiagram
                Alice->>Bob: Hello Bob
                Bob-->>Alice: Hi Alice
        "#;

        let scene = render_mermaid(code, None).expect("render sequence");
        let rect_count = scene
            .nodes
            .iter()
            .filter(|n| matches!(n, SceneNode::Rect { .. }))
            .count();
        assert_eq!(rect_count, 2, "Expected 2 participant boxes");
    }
}
