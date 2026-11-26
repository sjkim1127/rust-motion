//! Text layout data types: alignment, box options, fit results and layout errors.

use super::*;

/// Text horizontal alignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl From<TextAlign> for TextHorizontalAlign {
    fn from(align: TextAlign) -> Self {
        match align {
            TextAlign::Left => TextHorizontalAlign::Start,
            TextAlign::Center => TextHorizontalAlign::Center,
            TextAlign::Right => TextHorizontalAlign::End,
        }
    }
}

impl From<TextHorizontalAlign> for TextAlign {
    fn from(align: TextHorizontalAlign) -> Self {
        match align {
            TextHorizontalAlign::Start => TextAlign::Left,
            TextHorizontalAlign::Center => TextAlign::Center,
            TextHorizontalAlign::End => TextAlign::Right,
        }
    }
}

/// Options for multi-line text auto-scaling (`fit_text_on_n_lines`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitTextOnNLinesOptions {
    pub max_lines: usize,
    pub max_box_width: f32,
    pub max_box_height: Option<f32>,
    pub min_font_size: f32,
    pub max_font_size: f32,
}

impl Default for FitTextOnNLinesOptions {
    fn default() -> Self {
        Self {
            max_lines: 1,
            max_box_width: 1920.0,
            max_box_height: None,
            min_font_size: 1.0,
            max_font_size: 200.0,
        }
    }
}

/// Result of auto-fitting text across multiple lines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextFitResult {
    pub font_size: f32,
    pub lines: Vec<String>,
    pub total_height: f32,
    pub max_line_width: f32,
}

/// Options for multi-corner parametric rounded text box generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoundedTextBoxOptions {
    pub padding_x: f32,
    pub padding_y: f32,
    pub border_radius: f32,
    pub align: TextAlign,
}

impl Default for RoundedTextBoxOptions {
    fn default() -> Self {
        Self {
            padding_x: 16.0,
            padding_y: 12.0,
            border_radius: 8.0,
            align: TextAlign::Left,
        }
    }
}

/// Layout error for text fitting and layout calculation failures.
#[derive(Debug, thiserror::Error, PartialEq, Clone)]
pub enum LayoutError {
    #[error("Invalid constraints: {0}")]
    InvalidConstraints(String),
    #[error("Text could not be fit into the specified bounds: {0}")]
    CannotFit(String),
    #[error("Layout error: {0}")]
    Generic(String),
}

impl From<LayoutError> for RasterError {
    fn from(err: LayoutError) -> Self {
        RasterError::Scene(err.to_string())
    }
}

/// Measurement of a single line of text for parametric path generation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TextLineDimension {
    pub width: f32,
    pub height: f32,
}

impl TextLineDimension {
    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// Horizontal alignment inside a resolved text box.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextHorizontalAlign {
    #[default]
    Start,
    Center,
    End,
}

/// Vertical alignment inside a resolved text box.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextVerticalAlign {
    #[default]
    Start,
    Center,
    End,
}

/// Behavior when text still exceeds its box at the minimum font size.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextOverflow {
    #[default]
    Clip,
    Ellipsis,
}

/// Font-aware text box request resolved before nodes are added to a Scene.
#[derive(Debug, Clone, PartialEq)]
pub struct TextBox {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub font_size: f32,
    pub min_font_size: f32,
    /// Multiplier applied to the resolved font size.
    pub line_height: f32,
    pub max_lines: Option<usize>,
    pub horizontal_align: TextHorizontalAlign,
    pub vertical_align: TextVerticalAlign,
    pub overflow: TextOverflow,
    pub font_sources: Vec<String>,
}

impl TextBox {
    pub fn new(
        text: impl Into<String>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        font_size: f32,
    ) -> Self {
        Self {
            text: text.into(),
            x,
            y,
            width,
            height,
            font_size,
            min_font_size: font_size,
            line_height: 1.2,
            max_lines: None,
            horizontal_align: TextHorizontalAlign::Start,
            vertical_align: TextVerticalAlign::Start,
            overflow: TextOverflow::Clip,
            font_sources: Vec::new(),
        }
    }
}

/// One baseline-positioned line produced by [`layout_text_box`].
#[derive(Debug, Clone, PartialEq)]
pub struct PositionedTextLine {
    pub text: String,
    pub x: f32,
    pub y: f32,
}

/// Resolved size and lines for deterministic native and Player rendering.
#[derive(Debug, Clone, PartialEq)]
pub struct TextBoxLayout {
    pub font_size: f32,
    pub line_height: f32,
    pub lines: Vec<PositionedTextLine>,
}
