//! Primitive scene emitters (text blocks, text, rectangles, gradients).

use super::traits::{SceneEmitter, SceneFrameContext};
use crate::CompositionError;
use rust_motion_rasterizer::{
    layout_text_box, Color, GradientStop, Scene, SceneNode, TextBox, TextHorizontalAlign,
    TextOverflow, TextVerticalAlign,
};
use serde_json::Value;

/// Font-aware multiline text that resolves into ordinary baseline Text nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneTextBlock {
    pub layout: TextBox,
    pub color: Color,
    pub font_weight: u16,
}

impl SceneTextBlock {
    pub fn new(
        text: impl Into<String>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        font_size: f32,
    ) -> Self {
        Self {
            layout: TextBox::new(text, x, y, width, height, font_size),
            color: Color::WHITE,
            font_weight: 400,
        }
    }

    pub fn with_min_font_size(mut self, min_font_size: f32) -> Self {
        self.layout.min_font_size = min_font_size;
        self
    }

    pub fn with_line_height(mut self, line_height: f32) -> Self {
        self.layout.line_height = line_height;
        self
    }

    pub fn with_max_lines(mut self, max_lines: usize) -> Self {
        self.layout.max_lines = Some(max_lines);
        self
    }

    pub fn with_alignment(
        mut self,
        horizontal: TextHorizontalAlign,
        vertical: TextVerticalAlign,
    ) -> Self {
        self.layout.horizontal_align = horizontal;
        self.layout.vertical_align = vertical;
        self
    }

    pub fn with_overflow(mut self, overflow: TextOverflow) -> Self {
        self.layout.overflow = overflow;
        self
    }

    pub fn with_font_sources(mut self, sources: impl IntoIterator<Item = String>) -> Self {
        self.layout.font_sources = sources.into_iter().collect();
        self
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn with_font_weight(mut self, font_weight: u16) -> Self {
        self.font_weight = font_weight;
        self
    }
}

impl SceneEmitter for SceneTextBlock {
    fn emit(
        &self,
        context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let layout = layout_text_box(&self.layout).map_err(|error| {
            CompositionError::render(
                context.global_frame,
                format!("failed to layout text block: {error}"),
            )
        })?;
        for line in layout.lines {
            if line.text.is_empty() {
                continue;
            }
            scene.push(SceneNode::Text {
                x: line.x,
                y: line.y,
                content: line.text,
                font_size: layout.font_size,
                color: self.color,
                font_weight: self.font_weight,
                font_sources: self.layout.font_sources.clone(),
            });
        }
        Ok(())
    }
}

/// Solid-colour rectangle placed directly on the scene.
///
/// Equivalent to emitting a [`SceneNode::Rect`] without having to construct it manually.
/// Use [`SceneTextBlock`] for multi-line text layout.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub fill: Color,
    pub stroke: Option<Color>,
    pub stroke_width: f32,
    pub corner_radius: f32,
}

impl SceneRect {
    pub fn new(x: f32, y: f32, w: f32, h: f32, fill: Color) -> Self {
        Self {
            x,
            y,
            w: w.max(0.0),
            h: h.max(0.0),
            fill,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }
    }

    pub fn with_stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some(color);
        self.stroke_width = width.max(0.0);
        self
    }

    pub fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius.max(0.0);
        self
    }
}

impl SceneEmitter for SceneRect {
    fn emit(
        &self,
        _context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        scene.push(SceneNode::Rect {
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.h,
            fill: self.fill,
            stroke: self.stroke,
            stroke_width: self.stroke_width,
            corner_radius: self.corner_radius,
        });
        Ok(())
    }
}

/// Single-line text placed directly on the scene.
///
/// Use [`SceneTextBlock`] when you need word-wrapping, multi-line layout, or
/// alignment/overflow control. `SceneText` is a zero-cost shortcut for placing
/// a pre-positioned single text run.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneText {
    pub x: f32,
    pub y: f32,
    pub content: String,
    pub font_size: f32,
    pub color: Color,
    pub font_weight: u16,
    pub font_sources: Vec<String>,
}

impl SceneText {
    pub fn new(content: impl Into<String>, x: f32, y: f32, font_size: f32, color: Color) -> Self {
        Self {
            x,
            y,
            content: content.into(),
            font_size: font_size.max(0.0),
            color,
            font_weight: 400,
            font_sources: Vec::new(),
        }
    }

    pub fn with_font_weight(mut self, weight: u16) -> Self {
        self.font_weight = weight;
        self
    }

    pub fn with_font_sources(mut self, sources: impl IntoIterator<Item = String>) -> Self {
        self.font_sources = sources.into_iter().collect();
        self
    }
}

impl SceneEmitter for SceneText {
    fn emit(
        &self,
        _context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        if self.content.is_empty() {
            return Ok(());
        }
        scene.push(SceneNode::Text {
            x: self.x,
            y: self.y,
            content: self.content.clone(),
            font_size: self.font_size,
            color: self.color,
            font_weight: self.font_weight,
            font_sources: self.font_sources.clone(),
        });
        Ok(())
    }
}

/// Linear gradient rectangle placed directly on the scene.
///
/// A convenience wrapper around [`SceneNode::LinearGradient`] for use inside
/// emitter trees.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneLinearGradient {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub angle_deg: f32,
    pub stops: Vec<GradientStop>,
}

impl SceneLinearGradient {
    /// Horizontal left-to-right gradient (0°) spanning the given rectangle.
    pub fn new(x: f32, y: f32, w: f32, h: f32, stops: Vec<GradientStop>) -> Self {
        Self {
            x,
            y,
            w: w.max(0.0),
            h: h.max(0.0),
            angle_deg: 0.0,
            stops,
        }
    }

    pub fn with_angle(mut self, angle_deg: f32) -> Self {
        self.angle_deg = angle_deg;
        self
    }
}

impl SceneEmitter for SceneLinearGradient {
    fn emit(
        &self,
        _context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        scene.push(SceneNode::LinearGradient {
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.h,
            angle_deg: self.angle_deg,
            stops: self.stops.clone(),
        });
        Ok(())
    }
}
