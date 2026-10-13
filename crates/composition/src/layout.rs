//! Taffy-powered declarative layout engine for native video composition.
//!
//! Provides Flexbox and CSS Grid layout algorithms without requiring a web browser
//! or Chromium. Automatically computes coordinate placement and transforms for
//! [`SceneNode`] and nested layout hierarchies.

use crate::{CompositionError, SceneEmitter, SceneFrameContext};
use rust_motion_rasterizer::{Color, Scene, SceneNode, Transform2D};
use serde_json::Value;
use taffy::geometry::{Point, Rect, Size};
use taffy::prelude::{
    fr, length, AlignItems, AvailableSpace, Display, FlexDirection, FlexWrap, JustifyContent,
    LengthPercentageAuto, NodeId, Style, TaffyTree,
};
use taffy::TaffyError;

/// Re-exports of common Taffy layout types for ergonomic layout composition.
pub mod prelude {
    pub use super::{LayoutBox, LayoutChild, SceneFlex, SceneGrid};
    pub use taffy;
    pub use taffy::geometry::{Point, Rect, Size};
    pub use taffy::prelude::{
        auto, fr, length, percent, AlignContent, AlignItems, Display, FlexDirection, FlexWrap,
        JustifyContent, LengthPercentage, LengthPercentageAuto, Position, Style,
    };
}

/// Child item contained in a [`LayoutBox`].
#[derive(Debug, Clone)]
pub enum LayoutChild {
    /// A single scene node with optional explicit layout style.
    Node {
        node: SceneNode,
        style: Option<Style>,
    },
    /// An entire sub-scene positioned as a group with given layout style.
    Scene { scene: Scene, style: Style },
    /// A nested layout container (e.g. flex column inside a grid cell).
    Nested(LayoutBox),
}

unsafe impl Send for LayoutChild {}
unsafe impl Sync for LayoutChild {}

/// Declarative layout container powered by Taffy.
#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub style: Style,
    pub children: Vec<LayoutChild>,
    pub background: Option<Color>,
    pub border: Option<(Color, f32, f32)>, // color, width, radius
}

unsafe impl Send for LayoutBox {}
unsafe impl Sync for LayoutBox {}

impl Default for LayoutBox {
    fn default() -> Self {
        Self {
            style: Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                ..Default::default()
            },
            children: Vec::new(),
            background: None,
            border: None,
        }
    }
}

impl LayoutBox {
    /// Create a new empty layout box with default Flexbox row styling.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a flexbox container with the specified direction.
    pub fn flex(direction: FlexDirection) -> Self {
        Self {
            style: Style {
                display: Display::Flex,
                flex_direction: direction,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create a horizontal flex row (`flex-direction: row`).
    pub fn flex_row() -> Self {
        Self::flex(FlexDirection::Row)
    }

    /// Create a vertical flex column (`flex-direction: column`).
    pub fn flex_col() -> Self {
        Self::flex(FlexDirection::Column)
    }

    /// Create a CSS Grid container with a fixed number of equal fractional columns (`N * 1fr`).
    pub fn grid_columns(cols: u16, gap: f32) -> Self {
        let template = vec![fr(1.0_f32); cols.max(1) as usize];

        Self {
            style: Style {
                display: Display::Grid,
                grid_template_columns: template,
                gap: Size {
                    width: length(gap),
                    height: length(gap),
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Set container width and height in pixels.
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.style.size = Size {
            width: length(width),
            height: length(height),
        };
        self
    }

    /// Set container width in pixels.
    pub fn width(mut self, width: f32) -> Self {
        self.style.size.width = length(width);
        self
    }

    /// Set container height in pixels.
    pub fn height(mut self, height: f32) -> Self {
        self.style.size.height = length(height);
        self
    }

    /// Set uniform gap between children in pixels.
    pub fn gap(mut self, gap: f32) -> Self {
        self.style.gap = Size {
            width: length(gap),
            height: length(gap),
        };
        self
    }

    /// Set separate horizontal and vertical gaps.
    pub fn gap_xy(mut self, x: f32, y: f32) -> Self {
        self.style.gap = Size {
            width: length(x),
            height: length(y),
        };
        self
    }

    /// Set uniform padding on all sides.
    pub fn padding(mut self, p: f32) -> Self {
        self.style.padding = Rect {
            left: length(p),
            right: length(p),
            top: length(p),
            bottom: length(p),
        };
        self
    }

    /// Set padding per side: `(top, right, bottom, left)`.
    pub fn padding_rect(mut self, top: f32, right: f32, bottom: f32, left: f32) -> Self {
        self.style.padding = Rect {
            left: length(left),
            right: length(right),
            top: length(top),
            bottom: length(bottom),
        };
        self
    }

    /// Set uniform margin on all sides.
    pub fn margin(mut self, m: f32) -> Self {
        self.style.margin = Rect {
            left: LengthPercentageAuto::length(m),
            right: LengthPercentageAuto::length(m),
            top: LengthPercentageAuto::length(m),
            bottom: LengthPercentageAuto::length(m),
        };
        self
    }

    /// Set cross-axis alignment (`align-items`).
    pub fn align_items(mut self, align: AlignItems) -> Self {
        self.style.align_items = Some(align);
        self
    }

    /// Set main-axis justification (`justify-content`).
    pub fn justify_content(mut self, justify: JustifyContent) -> Self {
        self.style.justify_content = Some(justify);
        self
    }

    /// Set flex wrap mode.
    pub fn flex_wrap(mut self, wrap: FlexWrap) -> Self {
        self.style.flex_wrap = wrap;
        self
    }

    /// Set background fill color.
    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// Set border properties: `(color, stroke_width, corner_radius)`.
    pub fn border(mut self, color: Color, width: f32, radius: f32) -> Self {
        self.border = Some((color, width, radius));
        self
    }

    /// Add a child [`SceneNode`]. If `style` is `None`, intrinsic dimensions
    /// (e.g. from Rect width/height or Circle diameter) will be inferred automatically.
    pub fn push_node(&mut self, node: SceneNode, style: Option<Style>) -> &mut Self {
        self.children.push(LayoutChild::Node { node, style });
        self
    }

    /// Add a child [`SceneNode`] with builder chaining.
    pub fn with_node(mut self, node: SceneNode, style: Option<Style>) -> Self {
        self.push_node(node, style);
        self
    }

    /// Add a child sub-scene with given explicit layout style.
    pub fn push_scene(&mut self, scene: Scene, style: Style) -> &mut Self {
        self.children.push(LayoutChild::Scene { scene, style });
        self
    }

    /// Add a nested [`LayoutBox`].
    pub fn push_nested(&mut self, child: LayoutBox) -> &mut Self {
        self.children.push(LayoutChild::Nested(child));
        self
    }

    /// Add a nested [`LayoutBox`] with builder chaining.
    pub fn with_nested(mut self, child: LayoutBox) -> Self {
        self.push_nested(child);
        self
    }

    /// Compute layout using Taffy within the specified available bounds `(width, height)`
    /// and emit positioned nodes into `scene`.
    pub fn compute_and_emit(
        &self,
        available_width: f32,
        available_height: f32,
        scene: &mut Scene,
    ) -> Result<(), TaffyError> {
        let mut tree = TaffyTree::new();
        let root = build_taffy_tree(&mut tree, self)?;

        tree.compute_layout(
            root,
            Size {
                width: AvailableSpace::Definite(available_width),
                height: AvailableSpace::Definite(available_height),
            },
        )?;

        let mut emitted_nodes = Vec::new();
        emit_taffy_node(&tree, root, self, Point::ZERO, &mut emitted_nodes)?;
        scene.nodes.extend(emitted_nodes);

        Ok(())
    }
}

impl SceneEmitter for LayoutBox {
    fn emit(
        &self,
        context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let w = context.composition.width as f32;
        let h = context.composition.height as f32;
        self.compute_and_emit(w, h, scene).map_err(|e| {
            CompositionError::render(context.frame, format!("Taffy layout error: {e}"))
        })
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Convenience Wrappers
// ────────────────────────────────────────────────────────────────────────────

/// Ergonomic builder for Flexbox layouts.
pub struct SceneFlex;

impl SceneFlex {
    /// Create a flex row with common options.
    pub fn row(gap: f32, justify: JustifyContent, align: AlignItems) -> LayoutBox {
        LayoutBox::flex_row()
            .gap(gap)
            .justify_content(justify)
            .align_items(align)
    }

    /// Create a flex column with common options.
    pub fn column(gap: f32, justify: JustifyContent, align: AlignItems) -> LayoutBox {
        LayoutBox::flex_col()
            .gap(gap)
            .justify_content(justify)
            .align_items(align)
    }
}

/// Ergonomic builder for CSS Grid layouts.
pub struct SceneGrid;

impl SceneGrid {
    /// Create a grid with N equal-width columns and gap.
    pub fn columns(cols: u16, gap: f32) -> LayoutBox {
        LayoutBox::grid_columns(cols, gap)
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Internal Taffy Tree Builder & Emitter
// ────────────────────────────────────────────────────────────────────────────

fn build_taffy_tree(
    tree: &mut TaffyTree<()>,
    layout_box: &LayoutBox,
) -> Result<NodeId, TaffyError> {
    let mut child_ids = Vec::with_capacity(layout_box.children.len());

    for child in &layout_box.children {
        match child {
            LayoutChild::Node { node, style } => {
                let computed_style = style.clone().unwrap_or_else(|| infer_node_style(node));
                let id = tree.new_leaf(computed_style)?;
                child_ids.push(id);
            }
            LayoutChild::Scene { style, .. } => {
                let id = tree.new_leaf(style.clone())?;
                child_ids.push(id);
            }
            LayoutChild::Nested(nested) => {
                let id = build_taffy_tree(tree, nested)?;
                child_ids.push(id);
            }
        }
    }

    tree.new_with_children(layout_box.style.clone(), &child_ids)
}

fn emit_taffy_node(
    tree: &TaffyTree<()>,
    node_id: NodeId,
    layout_box: &LayoutBox,
    pos: Point<f32>,
    out: &mut Vec<SceneNode>,
) -> Result<(), TaffyError> {
    let layout = tree.layout(node_id)?;
    let pos_x = pos.x;
    let pos_y = pos.y;
    let w = layout.size.width;
    let h = layout.size.height;

    // Draw background if present
    if let Some(bg) = layout_box.background {
        let (stroke, stroke_width, corner_radius) = match layout_box.border {
            Some((c, sw, cr)) => (Some(c), sw, cr),
            None => (None, 0.0, 0.0),
        };

        out.push(SceneNode::Rect {
            x: pos_x,
            y: pos_y,
            w,
            h,
            fill: bg,
            stroke,
            stroke_width,
            corner_radius,
        });
    } else if let Some((c, sw, cr)) = layout_box.border {
        out.push(SceneNode::Rect {
            x: pos_x,
            y: pos_y,
            w,
            h,
            fill: Color::TRANSPARENT,
            stroke: Some(c),
            stroke_width: sw,
            corner_radius: cr,
        });
    }

    let children = tree.children(node_id)?;

    for (child_idx, child) in layout_box.children.iter().enumerate() {
        if child_idx >= children.len() {
            break;
        }
        let child_node_id = children[child_idx];
        let child_layout = tree.layout(child_node_id)?;
        let child_x = pos_x + child_layout.location.x;
        let child_y = pos_y + child_layout.location.y;

        match child {
            LayoutChild::Node { node, .. } => {
                let positioned = offset_scene_node(node, child_x, child_y);
                out.push(positioned);
            }
            LayoutChild::Scene { scene, .. } => {
                out.push(SceneNode::Group {
                    transform: Transform2D::translate(child_x, child_y),
                    opacity: 1.0,
                    children: scene.nodes.clone(),
                });
            }
            LayoutChild::Nested(nested) => {
                emit_taffy_node(
                    tree,
                    child_node_id,
                    nested,
                    Point {
                        x: child_x,
                        y: child_y,
                    },
                    out,
                )?;
            }
        }
    }

    Ok(())
}

/// Infer intrinsic layout style for common [`SceneNode`] types if not explicitly provided.
fn infer_node_style(node: &SceneNode) -> Style {
    match node {
        SceneNode::Rect { w, h, .. } => Style {
            size: Size {
                width: length(*w),
                height: length(*h),
            },
            ..Default::default()
        },
        SceneNode::Circle { r, .. } => Style {
            size: Size {
                width: length(r * 2.0),
                height: length(r * 2.0),
            },
            ..Default::default()
        },
        SceneNode::Image { w, h, .. } => Style {
            size: Size {
                width: length(*w),
                height: length(*h),
            },
            ..Default::default()
        },
        SceneNode::Video { w, h, .. } => Style {
            size: Size {
                width: length(*w),
                height: length(*h),
            },
            ..Default::default()
        },
        SceneNode::Gif { w, h, .. } => Style {
            size: Size {
                width: length(*w),
                height: length(*h),
            },
            ..Default::default()
        },
        SceneNode::Emoji { size, .. } => Style {
            size: Size {
                width: length(*size),
                height: length(*size),
            },
            ..Default::default()
        },
        SceneNode::Lottie { w, h, .. } => Style {
            size: Size {
                width: length(*w),
                height: length(*h),
            },
            ..Default::default()
        },
        SceneNode::LinearGradient { w, h, .. } => Style {
            size: Size {
                width: length(*w),
                height: length(*h),
            },
            ..Default::default()
        },
        SceneNode::RadialGradient { r, .. } => Style {
            size: Size {
                width: length(r * 2.0),
                height: length(r * 2.0),
            },
            ..Default::default()
        },
        SceneNode::AudioVisualizer { width, height, .. } => Style {
            size: Size {
                width: length(*width),
                height: length(*height),
            },
            ..Default::default()
        },
        _ => Style::default(),
    }
}

/// Offset a [`SceneNode`] by placing it at `(offset_x, offset_y)`.
fn offset_scene_node(node: &SceneNode, offset_x: f32, offset_y: f32) -> SceneNode {
    let mut cloned = node.clone();
    match &mut cloned {
        SceneNode::Rect { x, y, .. } => {
            *x += offset_x;
            *y += offset_y;
        }
        SceneNode::Circle { cx, cy, .. } => {
            *cx += offset_x;
            *cy += offset_y;
        }
        SceneNode::Text { x, y, .. } => {
            *x += offset_x;
            *y += offset_y;
        }
        SceneNode::Image { x, y, .. } => {
            *x += offset_x;
            *y += offset_y;
        }
        SceneNode::Video { x, y, .. } => {
            *x += offset_x;
            *y += offset_y;
        }
        SceneNode::Gif { x, y, .. } => {
            *x += offset_x;
            *y += offset_y;
        }
        SceneNode::Emoji { x, y, .. } => {
            *x += offset_x;
            *y += offset_y;
        }
        SceneNode::Lottie { x, y, .. } => {
            *x += offset_x;
            *y += offset_y;
        }
        SceneNode::LinearGradient { x, y, .. } => {
            *x += offset_x;
            *y += offset_y;
        }
        SceneNode::RadialGradient { cx, cy, .. } => {
            *cx += offset_x;
            *cy += offset_y;
        }
        SceneNode::AudioVisualizer { x, y, .. } => {
            *x += offset_x;
            *y += offset_y;
        }
        SceneNode::Group { transform, .. } => {
            transform.tx += offset_x;
            transform.ty += offset_y;
        }
        _ => {
            return SceneNode::Group {
                transform: Transform2D::translate(offset_x, offset_y),
                opacity: 1.0,
                children: vec![cloned],
            };
        }
    }
    cloned
}
