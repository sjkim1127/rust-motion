use crate::dom::NativeElement;
use rust_motion_rasterizer::{BlendMode, Color, GradientStop, ImageFit, SceneFilter, Transform2D};
use std::collections::HashMap;
use taffy::geometry::{Line, Point, Rect, Size};
use taffy::prelude::{
    auto, fr, length, line, percent, span, AlignContent, AlignItems, Dimension, Display,
    FlexDirection, FlexWrap, GridAutoFlow, GridPlacement, GridTemplateComponent, LengthPercentage,
    LengthPercentageAuto, Position, Style,
};
use taffy::style::Overflow;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CssError {
    #[error("invalid CSS rule: {0}")]
    InvalidRule(String),
    #[error("unsupported CSS selector: {0}")]
    UnsupportedSelector(String),
}

#[derive(Debug, Clone, Default)]
pub struct Stylesheet {
    rules: Vec<CssRule>,
}

#[derive(Debug, Clone)]
struct CssRule {
    selector: SimpleSelector,
    declarations: Vec<(String, String)>,
    order: usize,
}

#[derive(Debug, Clone, Default)]
struct SimpleSelector {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    universal: bool,
    ancestor: Option<Box<SimpleSelector>>,
    direct_parent: bool,
}

#[derive(Debug, Clone)]
pub struct ResolvedStyle {
    pub layout: Style,
    pub color: Color,
    pub background: Option<Color>,
    /// A supported CSS linear-gradient background: angle and color stops.
    pub background_gradient: Option<(f32, Vec<GradientStop>)>,
    /// A centered CSS radial-gradient background represented in normalized coordinates.
    pub background_radial_gradient: Option<(f32, f32, f32, Vec<GradientStop>)>,
    /// Supported CSS transform subset: translate, scale, and rotate.
    pub transform: Transform2D,
    /// Transform origin as normalized coordinates within the element's box.
    pub transform_origin: (TransformOriginValue, TransformOriginValue),
    pub border_color: Option<Color>,
    pub border_width: f32,
    pub border_radius: f32,
    pub box_shadow: Option<rust_motion_rasterizer::SceneShadow>,
    pub filters: Vec<SceneFilter>,
    pub blend_mode: BlendMode,
    pub font_size: f32,
    pub font_weight: u16,
    pub font_sources: Vec<String>,
    pub line_height: f32,
    pub opacity: f32,
    pub overflow_hidden: bool,
    pub object_fit: ImageFit,
}

impl Default for ResolvedStyle {
    fn default() -> Self {
        let layout = Style {
            display: Display::Block,
            ..Default::default()
        };
        Self {
            layout,
            color: Color::BLACK,
            background: None,
            background_gradient: None,
            background_radial_gradient: None,
            transform: Transform2D::default(),
            transform_origin: (
                TransformOriginValue::percent(0.5),
                TransformOriginValue::percent(0.5),
            ),
            border_color: None,
            border_width: 0.0,
            border_radius: 0.0,
            box_shadow: None,
            filters: Vec::new(),
            blend_mode: BlendMode::Normal,
            font_size: 16.0,
            font_weight: 400,
            font_sources: Vec::new(),
            line_height: 1.2,
            opacity: 1.0,
            overflow_hidden: false,
            object_fit: ImageFit::Cover,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformOriginValue {
    pub fraction: f32,
    pub pixels: f32,
}

impl TransformOriginValue {
    const fn percent(fraction: f32) -> Self {
        Self {
            fraction,
            pixels: 0.0,
        }
    }

    const fn pixels(pixels: f32) -> Self {
        Self {
            fraction: 0.0,
            pixels,
        }
    }

    pub(crate) fn resolve(self, size: f32) -> f32 {
        size * self.fraction + self.pixels
    }
}

impl ResolvedStyle {
    pub fn inherited(&self) -> Self {
        Self {
            color: self.color,
            font_size: self.font_size,
            font_weight: self.font_weight,
            font_sources: self.font_sources.clone(),
            line_height: self.line_height,
            ..Self::default()
        }
    }
}

impl Stylesheet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse rules using simple tag, `.class`, `#id`, and compound selectors.
    pub fn parse(css: &str) -> Result<Self, CssError> {
        let css = strip_comments(css);
        let mut rules = Vec::new();
        let mut remainder = css.as_str();
        let mut order = 0;
        while let Some(open) = remainder.find('{') {
            let selector_text = remainder[..open].trim();
            let after_open = &remainder[open + 1..];
            let close = after_open.find('}').ok_or_else(|| {
                CssError::InvalidRule(format!("missing closing brace after {selector_text}"))
            })?;
            let declarations = parse_declarations(&after_open[..close]);
            for selector in selector_text.split(',').map(str::trim) {
                if selector.is_empty() {
                    continue;
                }
                rules.push(CssRule {
                    selector: SimpleSelector::parse(selector)?,
                    declarations: declarations.clone(),
                    order,
                });
                order += 1;
            }
            remainder = &after_open[close + 1..];
        }
        if !remainder.trim().is_empty() {
            return Err(CssError::InvalidRule(remainder.trim().to_string()));
        }
        Ok(Self { rules })
    }

    #[allow(dead_code)]
    pub(crate) fn resolve(
        &self,
        element: &NativeElement,
        parent: Option<&ResolvedStyle>,
    ) -> ResolvedStyle {
        self.resolve_with_ancestors(element, parent, &[])
    }

    pub(crate) fn resolve_with_ancestors(
        &self,
        element: &NativeElement,
        parent: Option<&ResolvedStyle>,
        ancestors: &[NativeElement],
    ) -> ResolvedStyle {
        let mut resolved = parent.map_or_else(ResolvedStyle::default, ResolvedStyle::inherited);
        apply_tag_defaults(&mut resolved, &element.tag);
        apply_html_attributes(&mut resolved, element);

        let mut winners: HashMap<String, (u32, usize, String)> = HashMap::new();
        for rule in &self.rules {
            if !rule.selector.matches(element, ancestors) {
                continue;
            }
            let specificity = rule.selector.specificity();
            for (name, value) in &rule.declarations {
                let replace = winners.get(name).is_none_or(|(score, order, _)| {
                    specificity > *score || (specificity == *score && rule.order >= *order)
                });
                if replace {
                    winners.insert(name.clone(), (specificity, rule.order, value.clone()));
                }
            }
        }
        let tailwind_base_order = self.rules.len() + 1;
        if let Some(classes) = element.attributes.get("class") {
            for (idx, class_token) in classes.split_whitespace().enumerate() {
                for (name, value) in crate::tailwind::parse_utility_class(class_token) {
                    let specificity = 10;
                    let order = tailwind_base_order + idx;
                    let replace = winners.get(&name).is_none_or(|(score, prev_order, _)| {
                        specificity > *score || (specificity == *score && order >= *prev_order)
                    });
                    if replace {
                        winners.insert(name, (specificity, order, value));
                    }
                }
            }
        }

        let inline_order = tailwind_base_order + 10_000;
        if let Some(style) = element.attributes.get("style") {
            for (name, value) in parse_declarations(style) {
                winners.insert(name, (1_000, inline_order, value));
            }
        }
        for (name, value) in &element.styles {
            winners.insert(
                normalize_property(name),
                (1_000, inline_order, value.clone()),
            );
        }
        if let Some(font) = element.attributes.get("data-font-src") {
            winners.insert(
                "--rust-motion-font-source".into(),
                (1_000, inline_order, font.clone()),
            );
        }

        let mut declarations = winners
            .into_iter()
            .map(|(name, (_, _, value))| (name, value))
            .collect::<Vec<_>>();
        declarations.sort_by(|left, right| left.0.cmp(&right.0));
        apply_declarations(&mut resolved, &declarations);
        resolved
    }
}

impl SimpleSelector {
    fn parse(selector: &str) -> Result<Self, CssError> {
        let selector = selector.trim();
        if let Some((ancestor, target)) = selector.split_once('>') {
            let ancestor = ancestor.trim();
            let target = target.trim();
            if ancestor.is_empty()
                || target.is_empty()
                || ancestor.contains('>')
                || target.contains('>')
                || target.contains('+')
                || target.contains('~')
            {
                return Err(CssError::UnsupportedSelector(selector.into()));
            }
            let mut parsed = Self::parse(target)?;
            parsed.ancestor = Some(Box::new(Self::parse(ancestor)?));
            parsed.direct_parent = true;
            return Ok(parsed);
        }
        let parts = selector.split_whitespace().collect::<Vec<_>>();
        if parts.len() > 1 {
            let target = parts[parts.len() - 1];
            let ancestor = parts[..parts.len() - 1].join(" ");
            let mut parsed = Self::parse(target)?;
            parsed.ancestor = Some(Box::new(Self::parse(&ancestor)?));
            return Ok(parsed);
        }
        if selector == "*" {
            return Ok(Self {
                universal: true,
                ..Default::default()
            });
        }
        if selector.contains(char::is_whitespace)
            || selector.contains('>')
            || selector.contains('+')
            || selector.contains('~')
            || selector.contains('[')
            || selector.contains(':')
        {
            return Err(CssError::UnsupportedSelector(selector.into()));
        }

        let mut output = Self::default();
        let bytes = selector.as_bytes();
        let mut start = 0;
        let mut mode = 't';
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'.' || *byte == b'#' {
                if index > start {
                    output.push_part(mode, &selector[start..index])?;
                }
                mode = if *byte == b'.' { 'c' } else { 'i' };
                start = index + 1;
            }
        }
        if start < selector.len() {
            output.push_part(mode, &selector[start..])?;
        }
        if output.tag.is_none() && output.id.is_none() && output.classes.is_empty() {
            return Err(CssError::UnsupportedSelector(selector.into()));
        }
        Ok(output)
    }

    fn push_part(&mut self, mode: char, value: &str) -> Result<(), CssError> {
        if value.is_empty() {
            return Err(CssError::UnsupportedSelector(
                "empty selector component".into(),
            ));
        }
        match mode {
            't' => self.tag = Some(value.to_ascii_lowercase()),
            'c' => self.classes.push(value.to_string()),
            'i' => self.id = Some(value.to_string()),
            _ => unreachable!(),
        }
        Ok(())
    }

    fn matches(&self, element: &NativeElement, ancestors: &[NativeElement]) -> bool {
        if let Some(ancestor) = self.ancestor.as_deref() {
            let matches_ancestor = if self.direct_parent {
                ancestors
                    .len()
                    .checked_sub(1)
                    .is_some_and(|index| ancestor.matches(&ancestors[index], &ancestors[..index]))
            } else {
                ancestors
                    .iter()
                    .enumerate()
                    .rev()
                    .any(|(index, candidate)| ancestor.matches(candidate, &ancestors[..index]))
            };
            if !matches_ancestor {
                return false;
            }
        }
        if self.universal {
            return true;
        }
        if self
            .tag
            .as_ref()
            .is_some_and(|tag| !element.tag.eq_ignore_ascii_case(tag))
        {
            return false;
        }
        if self.id.as_ref().is_some_and(|id| {
            element
                .attributes
                .get("id")
                .is_none_or(|actual| actual != id)
        }) {
            return false;
        }
        let classes = element
            .attributes
            .get("class")
            .map(|classes| classes.split_whitespace().collect::<Vec<_>>())
            .unwrap_or_default();
        self.classes
            .iter()
            .all(|required| classes.iter().any(|actual| *actual == required))
    }

    fn specificity(&self) -> u32 {
        u32::from(self.id.is_some()) * 100
            + self.classes.len() as u32 * 10
            + u32::from(self.tag.is_some())
            + self.ancestor.as_deref().map_or(0, Self::specificity)
    }
}

fn strip_comments(css: &str) -> String {
    let mut output = String::with_capacity(css.len());
    let mut remainder = css;
    while let Some(start) = remainder.find("/*") {
        output.push_str(&remainder[..start]);
        let after = &remainder[start + 2..];
        let Some(end) = after.find("*/") else {
            return output;
        };
        remainder = &after[end + 2..];
    }
    output.push_str(remainder);
    output
}

fn normalize_property(name: &str) -> String {
    name.trim().replace('_', "-").to_ascii_lowercase()
}

fn parse_declarations(input: &str) -> Vec<(String, String)> {
    input
        .split(';')
        .filter_map(|declaration| {
            let (name, value) = declaration.split_once(':')?;
            let name = normalize_property(name);
            let value = value.trim();
            (!name.is_empty() && !value.is_empty()).then(|| (name, value.to_string()))
        })
        .collect()
}

fn apply_tag_defaults(style: &mut ResolvedStyle, tag: &str) {
    style.layout.display = Display::Block;
    match tag.to_ascii_lowercase().as_str() {
        "h1" => {
            style.font_size = 32.0;
            style.font_weight = 700;
        }
        "h2" => {
            style.font_size = 24.0;
            style.font_weight = 700;
        }
        "h3" => {
            style.font_size = 20.0;
            style.font_weight = 700;
        }
        "strong" | "b" => style.font_weight = 700,
        "img" | "video" => style.layout.item_is_replaced = true,
        _ => {}
    }
}

fn apply_html_attributes(style: &mut ResolvedStyle, element: &NativeElement) {
    if let Some(width) = element.attributes.get("width") {
        style.layout.size.width = parse_dimension(width);
    }
    if let Some(height) = element.attributes.get("height") {
        style.layout.size.height = parse_dimension(height);
    }
}

pub(crate) fn apply_declarations(style: &mut ResolvedStyle, declarations: &[(String, String)]) {
    for (name, value) in declarations {
        match name.as_str() {
            "display" => {
                style.layout.display = match value.as_str() {
                    "flex" | "inline-flex" => Display::Flex,
                    "grid" | "inline-grid" => Display::Grid,
                    "none" => Display::None,
                    _ => Display::Block,
                }
            }
            "position" => {
                style.layout.position = if value == "absolute" {
                    Position::Absolute
                } else {
                    Position::Relative
                }
            }
            "width" => style.layout.size.width = parse_dimension(value),
            "height" => style.layout.size.height = parse_dimension(value),
            "min-width" => style.layout.min_size.width = parse_dimension(value),
            "min-height" => style.layout.min_size.height = parse_dimension(value),
            "max-width" => style.layout.max_size.width = parse_dimension(value),
            "max-height" => style.layout.max_size.height = parse_dimension(value),
            "top" => style.layout.inset.top = parse_length_auto(value),
            "right" => style.layout.inset.right = parse_length_auto(value),
            "bottom" => style.layout.inset.bottom = parse_length_auto(value),
            "left" => style.layout.inset.left = parse_length_auto(value),
            "margin" => style.layout.margin = parse_rect_auto(value),
            "margin-top" => style.layout.margin.top = parse_length_auto(value),
            "margin-right" => style.layout.margin.right = parse_length_auto(value),
            "margin-bottom" => style.layout.margin.bottom = parse_length_auto(value),
            "margin-left" => style.layout.margin.left = parse_length_auto(value),
            "padding" => style.layout.padding = parse_rect(value),
            "padding-top" => style.layout.padding.top = parse_length(value),
            "padding-right" => style.layout.padding.right = parse_length(value),
            "padding-bottom" => style.layout.padding.bottom = parse_length(value),
            "padding-left" => style.layout.padding.left = parse_length(value),
            "gap" => {
                let values = css_values(value);
                let row = parse_length(values.first().copied().unwrap_or("0"));
                let column = parse_length(
                    values
                        .get(1)
                        .copied()
                        .unwrap_or_else(|| values.first().copied().unwrap_or("0")),
                );
                style.layout.gap = Size {
                    width: column,
                    height: row,
                };
            }
            "row-gap" => style.layout.gap.height = parse_length(value),
            "column-gap" => style.layout.gap.width = parse_length(value),
            "flex-direction" => {
                style.layout.flex_direction = match value.as_str() {
                    "column" => FlexDirection::Column,
                    "row-reverse" => FlexDirection::RowReverse,
                    "column-reverse" => FlexDirection::ColumnReverse,
                    _ => FlexDirection::Row,
                }
            }
            "flex-wrap" => {
                style.layout.flex_wrap = match value.as_str() {
                    "wrap" => FlexWrap::Wrap,
                    "wrap-reverse" => FlexWrap::WrapReverse,
                    _ => FlexWrap::NoWrap,
                }
            }
            "flex-grow" => style.layout.flex_grow = parse_number(value, 0.0).max(0.0),
            "flex-shrink" => style.layout.flex_shrink = parse_number(value, 1.0).max(0.0),
            "flex-basis" => style.layout.flex_basis = parse_dimension(value),
            "align-items" => style.layout.align_items = parse_align_items(value),
            "align-self" => style.layout.align_self = parse_align_items(value),
            "align-content" => style.layout.align_content = parse_align_content(value),
            "justify-content" => style.layout.justify_content = parse_align_content(value),
            "grid-template-columns" => {
                style.layout.grid_template_columns = parse_grid_tracks(value)
            }
            "grid-template-rows" => style.layout.grid_template_rows = parse_grid_tracks(value),
            "grid-auto-flow" => {
                style.layout.grid_auto_flow = match value.as_str() {
                    "column" => GridAutoFlow::Column,
                    "row dense" => GridAutoFlow::RowDense,
                    "column dense" => GridAutoFlow::ColumnDense,
                    _ => GridAutoFlow::Row,
                }
            }
            "grid-row" => style.layout.grid_row = parse_grid_line(value),
            "grid-column" => style.layout.grid_column = parse_grid_line(value),
            "grid-row-start" => style.layout.grid_row.start = parse_grid_placement(value),
            "grid-row-end" => style.layout.grid_row.end = parse_grid_placement(value),
            "grid-column-start" => style.layout.grid_column.start = parse_grid_placement(value),
            "grid-column-end" => style.layout.grid_column.end = parse_grid_placement(value),
            "background" => {
                style.background = Color::from_css(value);
                style.background_gradient = parse_linear_gradient(value);
                style.background_radial_gradient = parse_radial_gradient(value);
            }
            "background-color" => style.background = Color::from_css(value),
            "background-image" => {
                style.background_gradient = parse_linear_gradient(value);
                style.background_radial_gradient = parse_radial_gradient(value);
            }
            "transform" => {
                if let Some(transform) = parse_transform(value) {
                    style.transform = transform;
                }
            }
            "transform-origin" => {
                if let Some(origin) = parse_transform_origin(value) {
                    style.transform_origin = origin;
                }
            }
            "color" => {
                if let Some(color) = Color::from_css(value) {
                    style.color = color;
                }
            }
            "border-color" => style.border_color = Color::from_css(value),
            "border" => {
                for part in css_values(value) {
                    if let Some(width) = parse_px(part) {
                        style.border_width = width.max(0.0);
                    } else if let Some(color) = Color::from_css(part) {
                        style.border_color = Some(color);
                    }
                }
                style.layout.border = Rect {
                    left: LengthPercentage::length(style.border_width),
                    right: LengthPercentage::length(style.border_width),
                    top: LengthPercentage::length(style.border_width),
                    bottom: LengthPercentage::length(style.border_width),
                };
            }
            "border-width" => {
                style.border_width = parse_px(value).unwrap_or(0.0).max(0.0);
                style.layout.border = Rect {
                    left: LengthPercentage::length(style.border_width),
                    right: LengthPercentage::length(style.border_width),
                    top: LengthPercentage::length(style.border_width),
                    bottom: LengthPercentage::length(style.border_width),
                };
            }
            "border-radius" => style.border_radius = parse_px(value).unwrap_or(0.0).max(0.0),
            "box-shadow" => style.box_shadow = parse_box_shadow(value),
            "filter" => {
                style.filters = parse_filters(value);
                if let Some(shadow) = parse_drop_shadow(value) {
                    style.box_shadow = Some(shadow);
                }
            }
            "mix-blend-mode" => style.blend_mode = parse_blend_mode(value),
            "font-size" => style.font_size = parse_px(value).unwrap_or(style.font_size).max(1.0),
            "font-weight" => {
                style.font_weight = match value.as_str() {
                    "bold" => 700,
                    "normal" => 400,
                    _ => value.parse().unwrap_or(style.font_weight),
                }
            }
            "line-height" => {
                let parsed = value
                    .trim_end_matches('%')
                    .trim_end_matches("px")
                    .parse::<f32>()
                    .ok();
                if let Some(parsed) = parsed {
                    style.line_height = if value.ends_with('%') {
                        parsed / 100.0
                    } else if value.ends_with("px") {
                        parsed / style.font_size
                    } else {
                        parsed
                    }
                    .max(0.1);
                }
            }
            "opacity" => style.opacity = parse_number(value, 1.0).clamp(0.0, 1.0),
            "overflow" => {
                let overflow = match value.as_str() {
                    "hidden" => Overflow::Hidden,
                    "clip" => Overflow::Clip,
                    "scroll" | "auto" => Overflow::Scroll,
                    _ => Overflow::Visible,
                };
                style.layout.overflow = Point {
                    x: overflow,
                    y: overflow,
                };
                style.overflow_hidden = overflow != Overflow::Visible;
            }
            "object-fit" => {
                style.object_fit = match value.as_str() {
                    "contain" => ImageFit::Contain,
                    "fill" => ImageFit::Fill,
                    "none" => ImageFit::None,
                    "scale-down" => ImageFit::ScaleDown,
                    _ => ImageFit::Cover,
                }
            }
            "aspect-ratio" => {
                style.layout.aspect_ratio = value.split_once('/').and_then(|(width, height)| {
                    let width = width.trim().parse::<f32>().ok()?;
                    let height = height.trim().parse::<f32>().ok()?;
                    (height > 0.0).then_some(width / height)
                });
            }
            "--rust-motion-font-source" => {
                style.font_sources = value
                    .split(',')
                    .map(|source| source.trim().trim_matches(['\'', '"']).to_string())
                    .filter(|source| !source.is_empty())
                    .collect();
            }
            _ => {}
        }
    }
}

fn parse_box_shadow(value: &str) -> Option<rust_motion_rasterizer::SceneShadow> {
    let parts = css_values(value);
    if parts.iter().any(|part| part.eq_ignore_ascii_case("inset")) {
        return None;
    }
    let lengths = parts
        .iter()
        .filter_map(|part| parse_px(part))
        .collect::<Vec<_>>();
    if lengths.len() < 2 {
        return None;
    }
    let color = if let Some(start) = value.find("rgba(").or_else(|| value.find("rgb(")) {
        let end = value[start..].find(')')? + start + 1;
        Color::from_css(&value[start..end])?
    } else {
        parts
            .iter()
            .find_map(|part| Color::from_css(part))
            .unwrap_or(Color::BLACK)
    };
    Some(rust_motion_rasterizer::SceneShadow {
        offset_x: lengths[0],
        offset_y: lengths[1],
        blur_sigma: lengths.get(2).copied().unwrap_or(0.0).max(0.0),
        color,
    })
}

fn parse_filters(value: &str) -> Vec<SceneFilter> {
    let mut filters = Vec::new();
    for token in value.split(')').filter_map(|part| part.split_once('(')) {
        let name = token.0.trim().to_ascii_lowercase();
        let argument = token.1.trim();
        let parsed = argument
            .trim_end_matches('%')
            .trim_end_matches("px")
            .trim_end_matches("deg")
            .parse::<f32>()
            .ok();
        let normalized = parsed.map(|value| {
            if argument.ends_with('%') {
                value / 100.0
            } else {
                value
            }
        });
        let filter = match name.as_str() {
            "blur" => parsed.map(|sigma| SceneFilter::Blur { sigma }),
            "brightness" => normalized.map(|amount| SceneFilter::Brightness { amount }),
            "grayscale" => normalized.map(|amount| SceneFilter::Grayscale { amount }),
            "opacity" => normalized.map(|amount| SceneFilter::Opacity { amount }),
            "contrast" => normalized.map(|factor| SceneFilter::Contrast { factor }),
            "saturate" => normalized.map(|factor| SceneFilter::Saturation { factor }),
            "hue-rotate" => parsed.map(|degrees| SceneFilter::HueRotate { degrees }),
            "invert" => normalized.map(|amount| SceneFilter::Invert { amount }),
            _ => None,
        };
        if let Some(filter) = filter {
            filters.push(filter);
        }
    }
    filters
}

fn parse_drop_shadow(value: &str) -> Option<rust_motion_rasterizer::SceneShadow> {
    let start = value.find("drop-shadow(")? + "drop-shadow(".len();
    let end = value.rfind(')')?;
    parse_box_shadow(&value[start..end])
}

fn parse_blend_mode(value: &str) -> BlendMode {
    match value.trim().to_ascii_lowercase().as_str() {
        "multiply" => BlendMode::Multiply,
        "screen" => BlendMode::Screen,
        "overlay" => BlendMode::Overlay,
        "darken" => BlendMode::Darken,
        "lighten" => BlendMode::Lighten,
        "color-dodge" => BlendMode::ColorDodge,
        "color-burn" => BlendMode::ColorBurn,
        "hard-light" => BlendMode::HardLight,
        "soft-light" => BlendMode::SoftLight,
        "difference" => BlendMode::Difference,
        "exclusion" => BlendMode::Exclusion,
        _ => BlendMode::Normal,
    }
}

fn css_values(value: &str) -> Vec<&str> {
    value.split_whitespace().collect()
}

fn parse_number(value: &str, default: f32) -> f32 {
    value.parse().unwrap_or(default)
}

fn parse_px(value: &str) -> Option<f32> {
    value.trim().trim_end_matches("px").parse::<f32>().ok()
}

fn parse_dimension(value: &str) -> Dimension {
    let value = value.trim();
    if value == "auto" || value.is_empty() {
        Dimension::auto()
    } else if let Some(percent) = value.strip_suffix('%') {
        Dimension::percent(percent.parse::<f32>().unwrap_or(0.0) / 100.0)
    } else {
        Dimension::length(parse_px(value).unwrap_or(0.0))
    }
}

fn parse_length(value: &str) -> LengthPercentage {
    let value = value.trim();
    if let Some(percent) = value.strip_suffix('%') {
        LengthPercentage::percent(percent.parse::<f32>().unwrap_or(0.0) / 100.0)
    } else {
        LengthPercentage::length(parse_px(value).unwrap_or(0.0))
    }
}

fn parse_length_auto(value: &str) -> LengthPercentageAuto {
    if value.trim() == "auto" {
        LengthPercentageAuto::auto()
    } else if let Some(percent) = value.trim().strip_suffix('%') {
        LengthPercentageAuto::percent(percent.parse::<f32>().unwrap_or(0.0) / 100.0)
    } else {
        LengthPercentageAuto::length(parse_px(value).unwrap_or(0.0))
    }
}

fn expand_four<'a>(values: &'a [&'a str]) -> [&'a str; 4] {
    match values {
        [all] => [all, all, all, all],
        [vertical, horizontal] => [vertical, horizontal, vertical, horizontal],
        [top, horizontal, bottom] => [top, horizontal, bottom, horizontal],
        [top, right, bottom, left, ..] => [top, right, bottom, left],
        [] => ["0", "0", "0", "0"],
    }
}

fn parse_rect(value: &str) -> Rect<LengthPercentage> {
    let values = css_values(value);
    let [top, right, bottom, left] = expand_four(&values);
    Rect {
        left: parse_length(left),
        right: parse_length(right),
        top: parse_length(top),
        bottom: parse_length(bottom),
    }
}

fn parse_rect_auto(value: &str) -> Rect<LengthPercentageAuto> {
    let values = css_values(value);
    let [top, right, bottom, left] = expand_four(&values);
    Rect {
        left: parse_length_auto(left),
        right: parse_length_auto(right),
        top: parse_length_auto(top),
        bottom: parse_length_auto(bottom),
    }
}

fn parse_align_items(value: &str) -> Option<AlignItems> {
    Some(match value {
        "start" => AlignItems::START,
        "end" => AlignItems::END,
        "flex-start" => AlignItems::FLEX_START,
        "flex-end" => AlignItems::FLEX_END,
        "center" => AlignItems::CENTER,
        "baseline" => AlignItems::BASELINE,
        "stretch" => AlignItems::STRETCH,
        _ => return None,
    })
}

fn parse_align_content(value: &str) -> Option<AlignContent> {
    Some(match value {
        "start" => AlignContent::START,
        "end" => AlignContent::END,
        "flex-start" => AlignContent::FLEX_START,
        "flex-end" => AlignContent::FLEX_END,
        "center" => AlignContent::CENTER,
        "stretch" => AlignContent::STRETCH,
        "space-between" => AlignContent::SPACE_BETWEEN,
        "space-around" => AlignContent::SPACE_AROUND,
        "space-evenly" => AlignContent::SPACE_EVENLY,
        _ => return None,
    })
}

fn parse_grid_tracks(value: &str) -> Vec<GridTemplateComponent<String>> {
    css_values(value)
        .into_iter()
        .filter_map(|value| {
            if value == "auto" {
                Some(auto())
            } else if let Some(fraction) = value.strip_suffix("fr") {
                Some(fr(fraction.parse::<f32>().unwrap_or(1.0).max(0.0)))
            } else if let Some(value) = value.strip_suffix('%') {
                Some(percent(value.parse::<f32>().ok()? / 100.0))
            } else {
                Some(length(parse_px(value)?))
            }
        })
        .collect()
}

fn parse_grid_line(value: &str) -> Line<GridPlacement<String>> {
    let mut parts = value.split('/').map(str::trim);
    Line {
        start: parts.next().map(parse_grid_placement).unwrap_or_default(),
        end: parts.next().map(parse_grid_placement).unwrap_or_default(),
    }
}

fn parse_grid_placement(value: &str) -> GridPlacement<String> {
    let value = value.trim();
    if value == "auto" || value.is_empty() {
        auto()
    } else if let Some(span_count) = value.strip_prefix("span ") {
        span(span_count.parse::<u16>().unwrap_or(1).max(1))
    } else {
        line(value.parse::<i16>().unwrap_or(0))
    }
}

fn parse_linear_gradient(value: &str) -> Option<(f32, Vec<GradientStop>)> {
    let contents = value
        .trim()
        .strip_prefix("linear-gradient(")?
        .strip_suffix(')')?;
    let mut parts = split_css_arguments(contents).into_iter();
    let first = parts.next()?;
    let (angle_deg, first_stop) = if let Some(angle) = first.strip_suffix("deg") {
        (angle.trim().parse().ok()?, parts.next()?)
    } else {
        match first {
            "to right" => (90.0, parts.next()?),
            "to left" => (270.0, parts.next()?),
            "to bottom" => (180.0, parts.next()?),
            "to top" => (0.0, parts.next()?),
            "to bottom right" => (135.0, parts.next()?),
            "to bottom left" => (225.0, parts.next()?),
            "to top right" => (45.0, parts.next()?),
            "to top left" => (315.0, parts.next()?),
            _ => (180.0, first),
        }
    };
    let mut stops = vec![parse_gradient_stop(first_stop)?];
    stops.extend(parts.map(parse_gradient_stop).collect::<Option<Vec<_>>>()?);
    if stops.len() < 2 {
        return None;
    }
    let last = (stops.len() - 1) as f32;
    let mut positions: Vec<Option<f32>> = stops.iter().map(|(_, position)| *position).collect();
    if positions.iter().all(Option::is_none) {
        positions = (0..stops.len())
            .map(|index| Some(index as f32 / last))
            .collect();
    } else {
        positions[0].get_or_insert(0.0);
        let last_index = positions.len() - 1;
        positions[last_index].get_or_insert(1.0);
        let mut index = 0;
        while index < positions.len() {
            if positions[index].is_some() {
                index += 1;
                continue;
            }
            let start = index - 1;
            let end = (index..positions.len())
                .find(|candidate| positions[*candidate].is_some())
                .unwrap_or(positions.len() - 1);
            let start_position = positions[start]?;
            let end_position = positions[end]?;
            let span = (end - start) as f32;
            for (offset, position) in positions[start + 1..end].iter_mut().enumerate() {
                *position = Some(
                    start_position + (end_position - start_position) * (offset as f32 + 1.0) / span,
                );
            }
            index = end;
        }
    }
    Some((
        angle_deg,
        stops
            .into_iter()
            .map(|(color, _)| color)
            .enumerate()
            .map(|(index, color)| {
                Some(GradientStop {
                    position: positions[index]?,
                    color,
                })
            })
            .collect::<Option<Vec<_>>>()?,
    ))
}

fn parse_radial_gradient(value: &str) -> Option<(f32, f32, f32, Vec<GradientStop>)> {
    let contents = value
        .trim()
        .strip_prefix("radial-gradient(")?
        .strip_suffix(')')?;
    let mut parts = split_css_arguments(contents).into_iter();
    let first = parts.next()?;
    let first_is_hint = first.eq_ignore_ascii_case("circle")
        || first.eq_ignore_ascii_case("ellipse")
        || first.starts_with("circle ")
        || first.starts_with("ellipse ");
    let (cx, cy) = if let Some(position) = first.split_once(" at ").map(|(_, p)| p) {
        let coordinates = position.split_whitespace().collect::<Vec<_>>();
        if coordinates.len() == 2 {
            (
                coordinates[0]
                    .strip_suffix('%')
                    .and_then(|v| v.parse::<f32>().ok())
                    .map(|v| (v / 100.0).clamp(0.0, 1.0))
                    .unwrap_or(0.5),
                coordinates[1]
                    .strip_suffix('%')
                    .and_then(|v| v.parse::<f32>().ok())
                    .map(|v| (v / 100.0).clamp(0.0, 1.0))
                    .unwrap_or(0.5),
            )
        } else {
            (0.5, 0.5)
        }
    } else {
        (0.5, 0.5)
    };
    let mut stop_values = if first_is_hint {
        parts.collect::<Vec<_>>()
    } else {
        std::iter::once(first).chain(parts).collect::<Vec<_>>()
    };
    if stop_values.len() < 2 {
        return None;
    }
    let parsed = stop_values
        .drain(..)
        .map(parse_gradient_stop)
        .collect::<Option<Vec<_>>>()?;
    let last = (parsed.len() - 1) as f32;
    let stops = parsed
        .into_iter()
        .enumerate()
        .map(|(index, (color, position))| GradientStop {
            position: position.unwrap_or(index as f32 / last),
            color,
        })
        .collect();
    Some((cx, cy, 0.5, stops))
}

fn parse_gradient_stop(value: &str) -> Option<(Color, Option<f32>)> {
    let value = value.trim();
    let mut fields = value.rsplitn(2, char::is_whitespace);
    if let (Some(position), Some(color)) = (fields.next(), fields.next()) {
        if let Some(position) = position
            .strip_suffix('%')
            .and_then(|v| v.parse::<f32>().ok())
        {
            return Some((
                Color::from_css(color.trim())?,
                Some((position / 100.0).clamp(0.0, 1.0)),
            ));
        }
    }
    Some((Color::from_css(value)?, None))
}

fn parse_transform(value: &str) -> Option<Transform2D> {
    let mut translate = None;
    let mut scale = None;
    let mut rotate = None;
    for function in value.split(')').filter_map(|part| part.split_once('(')) {
        let name = function.0.trim().to_ascii_lowercase();
        let args = split_css_arguments(function.1);
        match name.as_str() {
            "translate" => {
                let x = parse_px(args.first().copied()?)?;
                let y = args.get(1).and_then(|v| parse_px(v)).unwrap_or(0.0);
                translate = Some((x, y));
            }
            "translatex" => {
                let x = parse_px(args.first().copied()?)?;
                translate = Some((x, translate.map_or(0.0, |(_, y)| y)));
            }
            "translatey" => {
                let y = parse_px(args.first().copied()?)?;
                translate = Some((translate.map_or(0.0, |(x, _)| x), y));
            }
            "scale" => {
                let x = args.first()?.parse().ok()?;
                let y = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(x);
                scale = Some((x, y));
            }
            "scalex" => {
                let x = args.first()?.parse().ok()?;
                scale = Some((x, scale.map_or(1.0, |(_, y)| y)));
            }
            "scaley" => {
                let y = args.first()?.parse().ok()?;
                scale = Some((scale.map_or(1.0, |(x, _)| x), y));
            }
            "rotate" => rotate = Some(args.first()?.strip_suffix("deg")?.parse().ok()?),
            _ => {}
        }
    }
    (translate.is_some() || scale.is_some() || rotate.is_some())
        .then(|| Transform2D::make_transform(translate, scale, rotate))
}

fn parse_transform_origin(value: &str) -> Option<(TransformOriginValue, TransformOriginValue)> {
    let args = value.split_whitespace().collect::<Vec<_>>();
    let x = parse_origin_component(args.first().copied()?, true)?;
    let y = args
        .get(1)
        .and_then(|value| parse_origin_component(value, false))
        .unwrap_or(TransformOriginValue::percent(0.5));
    Some((x, y))
}

fn parse_origin_component(value: &str, horizontal: bool) -> Option<TransformOriginValue> {
    match value.to_ascii_lowercase().as_str() {
        "left" if horizontal => Some(TransformOriginValue::percent(0.0)),
        "center" => Some(TransformOriginValue::percent(0.5)),
        "right" if horizontal => Some(TransformOriginValue::percent(1.0)),
        "top" if !horizontal => Some(TransformOriginValue::percent(0.0)),
        "bottom" if !horizontal => Some(TransformOriginValue::percent(1.0)),
        _ => value
            .strip_suffix('%')
            .and_then(|value| value.parse::<f32>().ok())
            .map(|value| TransformOriginValue::percent(value / 100.0))
            .or_else(|| parse_px(value).map(TransformOriginValue::pixels)),
    }
}

fn split_css_arguments(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth: usize = 0;
    for (index, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(value[start..index].trim());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(value[start..].trim());
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascade_uses_specificity_and_inline_styles() {
        let stylesheet = Stylesheet::parse(
            "div { color: #111111; } .card { color: #222222; padding: 8px; } #hero { color: #333333; }",
        )
        .unwrap();
        let mut element = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        element.attributes.insert("class".into(), "card".into());
        element.attributes.insert("id".into(), "hero".into());
        element
            .attributes
            .insert("style".into(), "color: #444444; width: 50%".into());

        let style = stylesheet.resolve(&element, None);
        assert_eq!(style.color, Color::rgb(0x44, 0x44, 0x44));
        assert_eq!(style.layout.size.width, Dimension::percent(0.5));
        assert_eq!(style.layout.padding.left, LengthPercentage::length(8.0));
    }

    #[test]
    fn child_selector_matches_only_the_direct_parent() {
        let stylesheet = Stylesheet::parse(".card > span { color: #123456; }").unwrap();
        let mut card = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        card.attributes.insert("class".into(), "card".into());
        let span = NativeElement {
            tag: "span".into(),
            ..Default::default()
        };
        let nested = NativeElement {
            tag: "section".into(),
            ..Default::default()
        };

        assert_eq!(
            stylesheet
                .resolve_with_ancestors(&span, None, &[card.clone()])
                .color,
            Color::rgb(0x12, 0x34, 0x56)
        );
        assert_eq!(
            stylesheet
                .resolve_with_ancestors(&span, None, &[card, nested])
                .color,
            Color::BLACK
        );
    }

    #[test]
    fn descendant_selector_matches_any_ancestor_and_adds_specificity() {
        let stylesheet = Stylesheet::parse(".card .title { color: #123456; }").unwrap();
        let mut card = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        card.attributes.insert("class".into(), "card".into());
        let mut title = NativeElement {
            tag: "span".into(),
            ..Default::default()
        };
        title.attributes.insert("class".into(), "title".into());

        let style = stylesheet.resolve_with_ancestors(&title, None, &[card]);
        assert_eq!(style.color, Color::rgb(0x12, 0x34, 0x56));
    }

    #[test]
    fn descendant_selector_supports_multiple_ancestor_levels() {
        let stylesheet = Stylesheet::parse("main .card .title { color: #123456; }").unwrap();
        let main = NativeElement {
            tag: "main".into(),
            ..Default::default()
        };
        let mut card = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        card.attributes.insert("class".into(), "card".into());
        let mut title = NativeElement {
            tag: "span".into(),
            ..Default::default()
        };
        title.attributes.insert("class".into(), "title".into());

        let style = stylesheet.resolve_with_ancestors(&title, None, &[main, card]);
        assert_eq!(style.color, Color::rgb(0x12, 0x34, 0x56));
    }

    #[test]
    fn parses_grid_tracks_and_placements() {
        let stylesheet = Stylesheet::parse(
            ".grid { display: grid; grid-template-columns: 100px 1fr 25%; } .item { grid-column: 2 / span 2; }",
        )
        .unwrap();
        let mut grid = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        grid.attributes.insert("class".into(), "grid".into());
        let grid_style = stylesheet.resolve(&grid, None);
        assert_eq!(grid_style.layout.display, Display::Grid);
        assert_eq!(grid_style.layout.grid_template_columns.len(), 3);

        let mut item = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        item.attributes.insert("class".into(), "item".into());
        let item_style = stylesheet.resolve(&item, None);
        assert!(matches!(
            item_style.layout.grid_column.start,
            GridPlacement::Line(_)
        ));
        assert_eq!(item_style.layout.grid_column.end, GridPlacement::Span(2));
    }

    #[test]
    fn parses_linear_gradient_background() {
        let stylesheet =
            Stylesheet::parse(".hero { background: linear-gradient(to right, red, blue); }")
                .unwrap();
        let mut element = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        element.attributes.insert("class".into(), "hero".into());
        let style = stylesheet.resolve(&element, None);
        let (angle, stops) = style.background_gradient.expect("gradient");
        assert_eq!(angle, 90.0);
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].color, Color::rgb(255, 0, 0));
        assert_eq!(stops[1].color, Color::rgb(0, 0, 255));
    }

    #[test]
    fn parses_linear_gradient_with_rgba_stop() {
        let stylesheet = Stylesheet::parse(
            ".hero { background-image: linear-gradient(90deg, rgba(255, 0, 0, 0.5), blue); }",
        )
        .unwrap();
        let mut element = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        element.attributes.insert("class".into(), "hero".into());
        let style = stylesheet.resolve(&element, None);
        let (_, stops) = style.background_gradient.expect("gradient");
        assert_eq!(stops[0].color, Color::rgba(255, 0, 0, 128));
        assert_eq!(stops[1].color, Color::rgb(0, 0, 255));
    }

    #[test]
    fn parses_radial_gradient_background() {
        let stylesheet = Stylesheet::parse(
            ".hero { background: radial-gradient(circle at 30% 40%, white, blue); }",
        )
        .unwrap();
        let mut element = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        element.attributes.insert("class".into(), "hero".into());
        let style = stylesheet.resolve(&element, None);
        let (cx, cy, radius, stops) = style.background_radial_gradient.expect("radial gradient");
        assert_eq!((cx, cy, radius), (0.3, 0.4, 0.5));
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].color, Color::WHITE);
    }

    #[test]
    fn parses_diagonal_gradient_direction() {
        let stylesheet =
            Stylesheet::parse(".hero { background: linear-gradient(to bottom right, red, blue); }")
                .unwrap();
        let mut element = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        element.attributes.insert("class".into(), "hero".into());
        let style = stylesheet.resolve(&element, None);
        assert_eq!(style.background_gradient.unwrap().0, 135.0);
    }

    #[test]
    fn preserves_gradient_stop_positions() {
        let stylesheet =
            Stylesheet::parse(".hero { background: linear-gradient(red 10%, blue 90%); }").unwrap();
        let mut element = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        element.attributes.insert("class".into(), "hero".into());
        let style = stylesheet.resolve(&element, None);
        let (_, stops) = style.background_gradient.expect("gradient");
        assert!((stops[0].position - 0.1).abs() < f32::EPSILON);
        assert!((stops[1].position - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn interpolates_unpositioned_gradient_stops() {
        let stylesheet =
            Stylesheet::parse(".hero { background: linear-gradient(red 10%, green, blue 90%); }")
                .unwrap();
        let mut element = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        element.attributes.insert("class".into(), "hero".into());
        let style = stylesheet.resolve(&element, None);
        let (_, stops) = style.background_gradient.expect("gradient");
        assert!((stops[1].position - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn parses_basic_transform_functions() {
        let stylesheet =
            Stylesheet::parse(".hero { transform: translate(12px, 8px) scale(2) rotate(15deg); }")
                .unwrap();
        let mut element = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        element.attributes.insert("class".into(), "hero".into());
        let style = stylesheet.resolve(&element, None);
        assert_eq!(style.transform.tx, 12.0);
        assert_eq!(style.transform.ty, 8.0);
        assert_eq!(style.transform.scale_x, 2.0);
        assert_eq!(style.transform.scale_y, 2.0);
        assert_eq!(style.transform.rotate_deg, 15.0);
    }

    #[test]
    fn parses_axis_transform_functions() {
        let stylesheet = Stylesheet::parse(
            ".hero { transform: translateX(12px) translateY(8px) scaleX(2) scaleY(0.5); }",
        )
        .unwrap();
        let mut element = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        element.attributes.insert("class".into(), "hero".into());
        let style = stylesheet.resolve(&element, None);
        assert_eq!(style.transform.tx, 12.0);
        assert_eq!(style.transform.ty, 8.0);
        assert_eq!(style.transform.scale_x, 2.0);
        assert_eq!(style.transform.scale_y, 0.5);
    }

    #[test]
    fn parses_transform_origin_keywords_and_percentages() {
        let stylesheet = Stylesheet::parse(
            ".hero { transform-origin: left 25%; } .center { transform-origin: center bottom; }",
        )
        .unwrap();
        let hero = stylesheet.resolve(
            &NativeElement {
                tag: "div".into(),
                attributes: [("class".into(), "hero".into())].into_iter().collect(),
                namespace: None,
                ..Default::default()
            },
            None,
        );
        assert_eq!(hero.transform_origin.0.resolve(100.0), 0.0);
        assert_eq!(hero.transform_origin.1.resolve(100.0), 25.0);

        let center = stylesheet.resolve(
            &NativeElement {
                tag: "div".into(),
                attributes: [("class".into(), "center".into())].into_iter().collect(),
                namespace: None,
                ..Default::default()
            },
            None,
        );
        assert_eq!(center.transform_origin.0.resolve(100.0), 50.0);
        assert_eq!(center.transform_origin.1.resolve(100.0), 100.0);

        let px = Stylesheet::parse(".px { transform-origin: 12px 8px; }").unwrap();
        let style = px.resolve(
            &NativeElement {
                tag: "div".into(),
                attributes: [("class".into(), "px".into())].into_iter().collect(),
                namespace: None,
                ..Default::default()
            },
            None,
        );
        assert_eq!(style.transform_origin.0.resolve(100.0), 12.0);
        assert_eq!(style.transform_origin.1.resolve(100.0), 8.0);
    }

    #[test]
    fn parses_box_shadow_for_native_layer() {
        let stylesheet =
            Stylesheet::parse(".hero { box-shadow: 4px 6px 8px rgba(10, 20, 30, 0.5); }").unwrap();
        let mut element = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        element.attributes.insert("class".into(), "hero".into());
        let style = stylesheet.resolve(&element, None);
        let shadow = style.box_shadow.expect("box shadow");
        assert_eq!(
            (shadow.offset_x, shadow.offset_y, shadow.blur_sigma),
            (4.0, 6.0, 8.0)
        );
        assert_eq!(shadow.color, Color::rgba(10, 20, 30, 128));
    }

    #[test]
    fn parses_css_filter_chain_for_native_layer() {
        let stylesheet = Stylesheet::parse(
            ".hero { filter: blur(4px) grayscale(50%) hue-rotate(15deg) opacity(80%); }",
        )
        .unwrap();
        let mut element = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        element.attributes.insert("class".into(), "hero".into());
        let style = stylesheet.resolve(&element, None);
        assert!(matches!(style.filters[0], SceneFilter::Blur { sigma } if sigma == 4.0));
        assert!(matches!(style.filters[1], SceneFilter::Grayscale { amount } if amount == 0.5));
        assert!(matches!(style.filters[2], SceneFilter::HueRotate { degrees } if degrees == 15.0));
        assert!(matches!(style.filters[3], SceneFilter::Opacity { amount } if amount == 0.8));
    }

    #[test]
    fn parses_mix_blend_mode() {
        let stylesheet = Stylesheet::parse(".hero { mix-blend-mode: screen; }").unwrap();
        let mut element = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        element.attributes.insert("class".into(), "hero".into());
        assert_eq!(
            stylesheet.resolve(&element, None).blend_mode,
            BlendMode::Screen
        );
    }

    #[test]
    fn parses_drop_shadow_filter_as_native_shadow() {
        let stylesheet =
            Stylesheet::parse(".hero { filter: drop-shadow(3px 4px 5px rgba(1, 2, 3, 0.5)); }")
                .unwrap();
        let mut element = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        element.attributes.insert("class".into(), "hero".into());
        let shadow = stylesheet
            .resolve(&element, None)
            .box_shadow
            .expect("drop shadow");
        assert_eq!(
            (shadow.offset_x, shadow.offset_y, shadow.blur_sigma),
            (3.0, 4.0, 5.0)
        );
        assert_eq!(shadow.color, Color::rgba(1, 2, 3, 128));
    }

    #[test]
    fn shadow_color_defaults_to_black_when_omitted() {
        let stylesheet = Stylesheet::parse(".hero { box-shadow: 2px 3px 4px; }").unwrap();
        let mut element = NativeElement {
            tag: "div".into(),
            ..Default::default()
        };
        element.attributes.insert("class".into(), "hero".into());
        assert_eq!(
            stylesheet.resolve(&element, None).box_shadow.unwrap().color,
            Color::BLACK
        );
    }
}
