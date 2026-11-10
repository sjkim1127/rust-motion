//! Pure-Rust Deterministic Mathematical Typesetting Engine (`MathTex`) with Addressable Formula Parts (#50).
//!
//! Parses and layouts mathematical expressions into addressable vector/scene groups without
//! external LaTeX/TeX binaries:
//! - Superscripts (`^`), subscripts (`_`), nested braces `{...}`
//! - Fractions (`\frac{num}{den}`) with vector fraction rules
//! - Roots (`\sqrt{...}`) with vector radical strokes and vinculum overbars
//! - Matrices (`\begin{pmatrix} a & b \\ c & d \end{pmatrix}`, `bmatrix`, `matrix`)
//! - Aligned multi-line equations (`\begin{aligned} a &= b \\ c &= d \end{aligned}`)
//! - Named addressable groups (`\part{id}{expr}`) and per-part styling (`color`, `opacity`, `scale`, `offset`)

use crate::scene::{Color, Scene, SceneNode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Abstract Syntax Tree node for a mathematical formula.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MathAst {
    Symbol(String),
    Row(Vec<MathAst>),
    SupSub {
        base: Box<MathAst>,
        sup: Option<Box<MathAst>>,
        sub: Option<Box<MathAst>>,
    },
    Fraction {
        num: Box<MathAst>,
        den: Box<MathAst>,
    },
    Sqrt {
        inner: Box<MathAst>,
    },
    Matrix {
        bracket: MatrixBracket,
        rows: Vec<Vec<MathAst>>,
    },
    Aligned {
        rows: Vec<(MathAst, MathAst)>,
    },
    Part {
        id: String,
        inner: Box<MathAst>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatrixBracket {
    None,
    Paren,
    Square,
}

/// Visual styling and animation override for an addressable formula part (`\part{id}{...}`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MathPartStyle {
    pub color: Option<Color>,
    pub opacity: f32,
    pub scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

impl Default for MathPartStyle {
    fn default() -> Self {
        Self {
            color: None,
            opacity: 1.0,
            scale: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
        }
    }
}

/// Positioned glyph, rule, or vector delimiter produced by the math typesetter.
#[derive(Clone, Debug, PartialEq)]
pub struct PositionedMathElement {
    /// Addressable part identifier (inherited from enclosing `\part{id}{...}` or `"root"`).
    pub part_id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub kind: MathElementKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MathElementKind {
    Text { text: String, font_size: f32 },
    HorizontalRule { thickness: f32 },
    RadicalSign { thickness: f32 },
    Bracket { is_left: bool, bracket: MatrixBracket, thickness: f32 },
}

/// Complete typeset layout of a mathematical formula.
#[derive(Clone, Debug)]
pub struct TypesetFormula {
    pub width: f32,
    pub height: f32,
    pub elements: Vec<PositionedMathElement>,
}

impl TypesetFormula {
    /// Return the bounding box `(x, y, width, height)` of all elements belonging to `part_id`.
    pub fn part_bounds(&self, part_id: &str) -> Option<[f32; 4]> {
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        let mut found = false;

        for el in &self.elements {
            if el.part_id == part_id {
                found = true;
                min_x = min_x.min(el.x);
                min_y = min_y.min(el.y);
                max_x = max_x.max(el.x + el.width);
                max_y = max_y.max(el.y + el.height);
            }
        }
        if found {
            Some([min_x, min_y, max_x - min_x, max_y - min_y])
        } else {
            None
        }
    }

    /// Emit this typeset formula into a Rust Motion `Scene` at `(origin_x, origin_y)` with
    /// optional per-part styles (`styles`) for selective coloring, scaling, and reveal animations.
    pub fn emit_to_scene(
        &self,
        scene: &mut Scene,
        origin_x: f32,
        origin_y: f32,
        default_color: Color,
        styles: &HashMap<String, MathPartStyle>,
    ) {
        for el in &self.elements {
            let style = styles.get(&el.part_id).copied().unwrap_or_default();
            if style.opacity <= 0.001 || style.scale <= 0.001 {
                continue;
            }
            let base_col = style.color.unwrap_or(default_color);
            let col = Color::rgba(
                base_col.r,
                base_col.g,
                base_col.b,
                ((base_col.a as f32) * style.opacity.clamp(0.0, 1.0)).round() as u8,
            );

            let x = origin_x + el.x + style.offset_x;
            let y = origin_y + el.y + style.offset_y;
            let w = el.width * style.scale;
            let h = el.height * style.scale;

            match &el.kind {
                MathElementKind::Text { text, font_size } => {
                    scene.push(SceneNode::Text {
                        content: text.clone(),
                        x,
                        y,
                        font_size: font_size * style.scale,
                        color: col,
                        font_weight: 400,
                        font_sources: Vec::new(),
                    });
                }
                MathElementKind::HorizontalRule { thickness } => {
                    scene.push(SceneNode::Rect {
                        x,
                        y,
                        w,
                        h: (*thickness * style.scale).max(1.0),
                        fill: col,
                        stroke: None,
                        stroke_width: 0.0,
                        corner_radius: 0.0,
                    });
                }
                MathElementKind::RadicalSign { thickness } => {
                    let sw = (*thickness * style.scale).max(1.5);
                    let d = format!(
                        "M {:.2} {:.2} L {:.2} {:.2} L {:.2} {:.2}",
                        x,
                        y + h * 0.55,
                        x + w * 0.42,
                        y + h,
                        x + w,
                        y
                    );
                    scene.push(SceneNode::Path {
                        d,
                        fill: None,
                        stroke: Some(col),
                        stroke_width: sw,
                        opacity: style.opacity.clamp(0.0, 1.0),
                    });
                }
                MathElementKind::Bracket {
                    is_left,
                    bracket,
                    thickness,
                } => {
                    if *bracket == MatrixBracket::None {
                        continue;
                    }
                    let sw = (*thickness * style.scale).max(1.5);
                    let d = if *is_left {
                        format!(
                            "M {:.2} {:.2} L {:.2} {:.2} L {:.2} {:.2} L {:.2} {:.2}",
                            x + w,
                            y,
                            x,
                            y,
                            x,
                            y + h,
                            x + w,
                            y + h
                        )
                    } else {
                        format!(
                            "M {:.2} {:.2} L {:.2} {:.2} L {:.2} {:.2} L {:.2} {:.2}",
                            x,
                            y,
                            x + w,
                            y,
                            x + w,
                            y + h,
                            x,
                            y + h
                        )
                    };
                    scene.push(SceneNode::Path {
                        d,
                        fill: None,
                        stroke: Some(col),
                        stroke_width: sw,
                        opacity: style.opacity.clamp(0.0, 1.0),
                    });
                }
            }
        }
    }
}

fn map_latex_symbol(cmd: &str) -> &str {
    match cmd {
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "pi" => "π",
        "sigma" => "σ",
        "theta" => "θ",
        "lambda" => "λ",
        "mu" => "μ",
        "omega" => "ω",
        "Delta" => "Δ",
        "Omega" => "Ω",
        "Sigma" => "Σ",
        "sum" => "∑",
        "int" => "∫",
        "pm" => "±",
        "times" => "×",
        "cdot" => "·",
        "leq" => "≤",
        "geq" => "≥",
        "neq" => "≠",
        "infty" => "∞",
        "rightarrow" | "to" => "→",
        _ => cmd,
    }
}

/// Parse a LaTeX math string into a `MathAst`.
pub fn parse_math_tex(input: &str) -> MathAst {
    let chars: Vec<char> = input.chars().collect();
    let mut pos = 0usize;
    parse_expr_list(&chars, &mut pos, None)
}

fn skip_ws(chars: &[char], pos: &mut usize) {
    while *pos < chars.len() && chars[*pos].is_whitespace() {
        *pos += 1;
    }
}

fn read_braced_string(chars: &[char], pos: &mut usize) -> String {
    skip_ws(chars, pos);
    if *pos >= chars.len() || chars[*pos] != '{' {
        return String::new();
    }
    *pos += 1;
    let mut depth = 1usize;
    let mut out = String::new();
    while *pos < chars.len() && depth > 0 {
        let c = chars[*pos];
        *pos += 1;
        if c == '{' {
            depth += 1;
            out.push(c);
        } else if c == '}' {
            depth -= 1;
            if depth > 0 {
                out.push(c);
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn parse_group_or_atom(chars: &[char], pos: &mut usize) -> MathAst {
    skip_ws(chars, pos);
    if *pos >= chars.len() {
        return MathAst::Symbol(String::new());
    }
    if chars[*pos] == '{' {
        let inner = read_braced_string(chars, pos);
        return parse_math_tex(&inner);
    }
    if chars[*pos] == '\\' {
        *pos += 1;
        let mut cmd = String::new();
        while *pos < chars.len() && chars[*pos].is_ascii_alphabetic() {
            cmd.push(chars[*pos]);
            *pos += 1;
        }
        match cmd.as_str() {
            "frac" => {
                let num = parse_group_or_atom(chars, pos);
                let den = parse_group_or_atom(chars, pos);
                MathAst::Fraction {
                    num: Box::new(num),
                    den: Box::new(den),
                }
            }
            "sqrt" => {
                let inner = parse_group_or_atom(chars, pos);
                MathAst::Sqrt {
                    inner: Box::new(inner),
                }
            }
            "part" | "id" => {
                let id = read_braced_string(chars, pos);
                let inner = parse_group_or_atom(chars, pos);
                MathAst::Part {
                    id,
                    inner: Box::new(inner),
                }
            }
            "begin" => {
                let env = read_braced_string(chars, pos);
                let end_marker = format!("\\end{{{env}}}");
                let rest_str: String = chars[*pos..].iter().collect();
                let body = if let Some(idx) = rest_str.find(&end_marker) {
                    let b = &rest_str[..idx];
                    *pos += b.chars().count() + end_marker.chars().count();
                    b.to_string()
                } else {
                    *pos = chars.len();
                    rest_str
                };

                if env == "aligned" || env == "align" {
                    let rows = body
                        .split("\\\\")
                        .filter(|r| !r.trim().is_empty())
                        .map(|row| {
                            let mut parts = row.splitn(2, '&');
                            let left = parse_math_tex(parts.next().unwrap_or(""));
                            let right = parse_math_tex(parts.next().unwrap_or(""));
                            (left, right)
                        })
                        .collect();
                    MathAst::Aligned { rows }
                } else {
                    let bracket = match env.as_str() {
                        "pmatrix" => MatrixBracket::Paren,
                        "bmatrix" => MatrixBracket::Square,
                        _ => MatrixBracket::None,
                    };
                    let rows = body
                        .split("\\\\")
                        .filter(|r| !r.trim().is_empty())
                        .map(|row| row.split('&').map(|c| parse_math_tex(c.trim())).collect())
                        .collect();
                    MathAst::Matrix { bracket, rows }
                }
            }
            _ => MathAst::Symbol(map_latex_symbol(&cmd).to_string()),
        }
    } else {
        let c = chars[*pos];
        *pos += 1;
        MathAst::Symbol(c.to_string())
    }
}

fn parse_expr_list(chars: &[char], pos: &mut usize, stop_char: Option<char>) -> MathAst {
    let mut items = Vec::new();
    while *pos < chars.len() {
        skip_ws(chars, pos);
        if *pos >= chars.len() {
            break;
        }
        if Some(chars[*pos]) == stop_char {
            *pos += 1;
            break;
        }
        let mut atom = parse_group_or_atom(chars, pos);
        let mut sup = None;
        let mut sub = None;

        loop {
            skip_ws(chars, pos);
            if *pos < chars.len() && chars[*pos] == '^' {
                *pos += 1;
                sup = Some(Box::new(parse_group_or_atom(chars, pos)));
            } else if *pos < chars.len() && chars[*pos] == '_' {
                *pos += 1;
                sub = Some(Box::new(parse_group_or_atom(chars, pos)));
            } else {
                break;
            }
        }

        if sup.is_some() || sub.is_some() {
            atom = MathAst::SupSub {
                base: Box::new(atom),
                sup,
                sub,
            };
        }
        items.push(atom);
    }

    if items.len() == 1 {
        items.pop().unwrap()
    } else {
        MathAst::Row(items)
    }
}

/// Typeset a parsed `MathAst` at `base_font_size` into a `TypesetFormula`.
pub fn typeset_math(ast: &MathAst, base_font_size: f32) -> TypesetFormula {
    let mut elements = Vec::new();
    let (w, h) = layout_node(ast, 0.0, 0.0, base_font_size, "root", &mut elements);
    TypesetFormula {
        width: w,
        height: h,
        elements,
    }
}

fn translate_range(elements: &mut [PositionedMathElement], start_idx: usize, dx: f32, dy: f32) {
    for el in &mut elements[start_idx..] {
        el.x += dx;
        el.y += dy;
    }
}

fn layout_node(
    node: &MathAst,
    x: f32,
    y: f32,
    font_size: f32,
    part_id: &str,
    out: &mut Vec<PositionedMathElement>,
) -> (f32, f32) {
    match node {
        MathAst::Symbol(s) => {
            if s.is_empty() {
                return (0.0, font_size);
            }
            let char_count = s.chars().count() as f32;
            let w = char_count * font_size * 0.62;
            let h = font_size;
            out.push(PositionedMathElement {
                part_id: part_id.to_string(),
                x,
                y,
                width: w,
                height: h,
                kind: MathElementKind::Text {
                    text: s.clone(),
                    font_size,
                },
            });
            (w, h)
        }
        MathAst::Part { id, inner } => layout_node(inner, x, y, font_size, id, out),
        MathAst::Row(items) => {
            let mut cur_x = x;
            let mut max_h = font_size;
            let gap = font_size * 0.08;
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    cur_x += gap;
                }
                let (w, h) = layout_node(item, cur_x, y, font_size, part_id, out);
                cur_x += w;
                max_h = max_h.max(h);
            }
            (cur_x - x, max_h)
        }
        MathAst::SupSub { base, sup, sub } => {
            let (bw, bh) = layout_node(base, x, y, font_size, part_id, out);
            let script_size = font_size * 0.68;
            let mut max_script_w = 0.0f32;
            let mut total_h = bh;

            if let Some(sup_node) = sup {
                let (sw, _) = layout_node(
                    sup_node,
                    x + bw + font_size * 0.04,
                    y - font_size * 0.25,
                    script_size,
                    part_id,
                    out,
                );
                max_script_w = max_script_w.max(sw);
                total_h = total_h.max(bh + font_size * 0.25);
            }
            if let Some(sub_node) = sub {
                let (sw, sh) = layout_node(
                    sub_node,
                    x + bw + font_size * 0.04,
                    y + bh * 0.55,
                    script_size,
                    part_id,
                    out,
                );
                max_script_w = max_script_w.max(sw);
                total_h = total_h.max(bh * 0.55 + sh);
            }
            (bw + max_script_w + font_size * 0.05, total_h)
        }
        MathAst::Fraction { num, den } => {
            let child_size = font_size * 0.85;
            let num_start = out.len();
            let (nw, nh) = layout_node(num, x, y, child_size, part_id, out);
            let den_start = out.len();
            let (dw, dh) = layout_node(den, x, y, child_size, part_id, out);

            let pad = font_size * 0.15;
            let bar_w = nw.max(dw) + pad * 2.0;
            let rule_h = (font_size * 0.06).max(1.5);
            let gap = font_size * 0.12;

            // Center numerator at top, denominator below fraction rule
            translate_range(&mut out[..den_start], num_start, (bar_w - nw) * 0.5, 0.0);
            let rule_y = y + nh + gap;
            translate_range(out, den_start, (bar_w - dw) * 0.5, nh + gap * 2.0 + rule_h);

            out.push(PositionedMathElement {
                part_id: part_id.to_string(),
                x,
                y: rule_y,
                width: bar_w,
                height: rule_h,
                kind: MathElementKind::HorizontalRule { thickness: rule_h },
            });

            (bar_w, nh + dh + gap * 2.0 + rule_h)
        }
        MathAst::Sqrt { inner } => {
            let rad_w = font_size * 0.55;
            let pad_top = font_size * 0.16;
            let inner_start = out.len();
            let (iw, ih) = layout_node(inner, x + rad_w, y + pad_top, font_size, part_id, out);
            let _ = inner_start;
            let total_h = ih + pad_top;
            let rule_t = (font_size * 0.06).max(1.5);

            out.push(PositionedMathElement {
                part_id: part_id.to_string(),
                x,
                y,
                width: rad_w,
                height: total_h,
                kind: MathElementKind::RadicalSign { thickness: rule_t },
            });
            out.push(PositionedMathElement {
                part_id: part_id.to_string(),
                x: x + rad_w,
                y,
                width: iw + font_size * 0.1,
                height: rule_t,
                kind: MathElementKind::HorizontalRule { thickness: rule_t },
            });

            (rad_w + iw + font_size * 0.1, total_h)
        }
        MathAst::Matrix { bracket, rows } => {
            let cell_size = font_size * 0.85;
            let col_gap = font_size * 0.45;
            let row_gap = font_size * 0.25;
            let brk_w = if *bracket == MatrixBracket::None {
                0.0
            } else {
                font_size * 0.25
            };

            let mut cur_y = y;
            let mut max_row_w = 0.0f32;
            for (r_idx, row) in rows.iter().enumerate() {
                if r_idx > 0 {
                    cur_y += row_gap;
                }
                let mut cur_x = x + brk_w + font_size * 0.1;
                let mut row_h = cell_size;
                for (c_idx, cell) in row.iter().enumerate() {
                    if c_idx > 0 {
                        cur_x += col_gap;
                    }
                    let (cw, ch) = layout_node(cell, cur_x, cur_y, cell_size, part_id, out);
                    cur_x += cw;
                    row_h = row_h.max(ch);
                }
                max_row_w = max_row_w.max(cur_x - (x + brk_w + font_size * 0.1));
                cur_y += row_h;
            }

            let total_h = (cur_y - y).max(font_size);
            let total_w = max_row_w + brk_w * 2.0 + font_size * 0.2;
            let rule_t = (font_size * 0.06).max(1.5);

            if *bracket != MatrixBracket::None {
                out.push(PositionedMathElement {
                    part_id: part_id.to_string(),
                    x,
                    y,
                    width: brk_w,
                    height: total_h,
                    kind: MathElementKind::Bracket {
                        is_left: true,
                        bracket: *bracket,
                        thickness: rule_t,
                    },
                });
                out.push(PositionedMathElement {
                    part_id: part_id.to_string(),
                    x: x + total_w - brk_w,
                    y,
                    width: brk_w,
                    height: total_h,
                    kind: MathElementKind::Bracket {
                        is_left: false,
                        bracket: *bracket,
                        thickness: rule_t,
                    },
                });
            }

            (total_w, total_h)
        }
        MathAst::Aligned { rows } => {
            let row_gap = font_size * 0.30;
            let mut lhs_widths = Vec::with_capacity(rows.len());
            let mut row_records = Vec::with_capacity(rows.len());

            // Measure each row's LHS and RHS so we can align all `&` columns at `max_lhs_w`
            let mut max_lhs_w = 0.0f32;
            let mut cur_y = y;
            for (r_idx, (lhs, rhs)) in rows.iter().enumerate() {
                if r_idx > 0 {
                    cur_y += row_gap;
                }
                let lhs_start = out.len();
                let (lw, lh) = layout_node(lhs, x, cur_y, font_size, part_id, out);
                let rhs_start = out.len();
                let (rw, rh) = layout_node(rhs, x, cur_y, font_size, part_id, out);
                let end_idx = out.len();
                max_lhs_w = max_lhs_w.max(lw);
                lhs_widths.push(lw);
                row_records.push((lhs_start, rhs_start, end_idx, lw, rw, lh.max(rh)));
                cur_y += lh.max(rh);
            }

            let align_gap = font_size * 0.25;
            let mut total_w = 0.0f32;
            for (lhs_start, rhs_start, end_idx, lw, rw, _) in row_records {
                // Right-align LHS against the alignment column `max_lhs_w`
                let lhs_dx = max_lhs_w - lw;
                translate_range(&mut out[..rhs_start], lhs_start, lhs_dx, 0.0);
                // Place RHS immediately after `max_lhs_w + align_gap`
                let rhs_dx = max_lhs_w + align_gap;
                translate_range(&mut out[..end_idx], rhs_start, rhs_dx, 0.0);
                total_w = total_w.max(rhs_dx + rw);
            }

            (total_w, (cur_y - y).max(font_size))
        }
    }
}
