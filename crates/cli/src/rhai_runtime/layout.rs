//! Flex/grid layout support: `RhaiLayout` and the scene builder layout methods.

use super::*;

impl SceneBuilder {
    pub(super) fn add(&mut self, layout: RhaiLayout) -> RhaiResult<()> {
        layout
            .inner
            .compute_and_emit(layout.target_width, layout.target_height, &mut self.scene)
            .map_err(|e| runtime_error(format!("Taffy layout calculation failed: {e}")))?;
        Ok(())
    }

    pub(super) fn add_layout(&mut self, layout: RhaiLayout) -> RhaiResult<()> {
        self.add(layout)
    }

    pub(super) fn flex_row(&mut self, opts: Map) -> RhaiResult<RhaiLayout> {
        self.make_layout(FlexDirection::Row, opts)
    }

    pub(super) fn flex_col(&mut self, opts: Map) -> RhaiResult<RhaiLayout> {
        self.make_layout(FlexDirection::Column, opts)
    }

    pub(super) fn grid(&mut self, opts: Map) -> RhaiResult<RhaiLayout> {
        let cols = get_opt_f32(&opts, "cols", 2.0).max(1.0) as u16;
        let gap = get_opt_f32(&opts, "gap", 0.0);
        let mut box_layout = SceneGrid::columns(cols, gap);
        apply_layout_options(&mut box_layout, &opts)?;
        let width = get_opt_f32(&opts, "width", 1920.0);
        let height = get_opt_f32(&opts, "height", 1080.0);
        Ok(RhaiLayout {
            inner: box_layout,
            target_width: width,
            target_height: height,
        })
    }

    fn make_layout(&self, direction: FlexDirection, opts: Map) -> RhaiResult<RhaiLayout> {
        let gap = get_opt_f32(&opts, "gap", 0.0);
        let justify_str = get_opt_string(&opts, "justify");
        let justify = match justify_str.as_deref() {
            Some("center") => JustifyContent::CENTER,
            Some("end") | Some("flex-end") => JustifyContent::FLEX_END,
            Some("between") | Some("space-between") => JustifyContent::SPACE_BETWEEN,
            Some("around") | Some("space-around") => JustifyContent::SPACE_AROUND,
            Some("evenly") | Some("space-evenly") => JustifyContent::SPACE_EVENLY,
            _ => JustifyContent::FLEX_START,
        };
        let align_str = get_opt_string(&opts, "align");
        let align = match align_str.as_deref() {
            Some("center") => AlignItems::CENTER,
            Some("end") | Some("flex-end") => AlignItems::FLEX_END,
            Some("stretch") => AlignItems::STRETCH,
            Some("baseline") => AlignItems::BASELINE,
            _ => AlignItems::FLEX_START,
        };

        let mut box_layout = match direction {
            FlexDirection::Row => SceneFlex::row(gap, justify, align),
            FlexDirection::Column => SceneFlex::column(gap, justify, align),
            _ => LayoutBox::flex(direction).gap(gap),
        };

        apply_layout_options(&mut box_layout, &opts)?;

        let width = get_opt_f32(&opts, "width", 1920.0);
        let height = get_opt_f32(&opts, "height", 1080.0);

        Ok(RhaiLayout {
            inner: box_layout,
            target_width: width,
            target_height: height,
        })
    }
}

/// A script-accessible declarative layout container.
#[derive(Debug, Clone)]
pub struct RhaiLayout {
    pub(super) inner: LayoutBox,
    pub(super) target_width: f32,
    pub(super) target_height: f32,
}

impl RhaiLayout {
    pub(super) fn rect(&mut self, w: Dynamic, h: Dynamic, fill: &str) -> RhaiResult<()> {
        let node = SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: dynamic_to_non_negative_f32(&w, "width")?,
            h: dynamic_to_non_negative_f32(&h, "height")?,
            fill: parse_color(fill)?,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        };
        self.inner.push_node(node, None);
        Ok(())
    }

    pub(super) fn round_rect(
        &mut self,
        w: Dynamic,
        h: Dynamic,
        fill: &str,
        radius: Dynamic,
    ) -> RhaiResult<()> {
        let node = SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: dynamic_to_non_negative_f32(&w, "width")?,
            h: dynamic_to_non_negative_f32(&h, "height")?,
            fill: parse_color(fill)?,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: dynamic_to_non_negative_f32(&radius, "corner radius")?,
        };
        self.inner.push_node(node, None);
        Ok(())
    }

    pub(super) fn circle(&mut self, radius: Dynamic, fill: &str) -> RhaiResult<()> {
        let node = SceneNode::Circle {
            cx: 0.0,
            cy: 0.0,
            r: dynamic_to_non_negative_f32(&radius, "radius")?,
            fill: parse_color(fill)?,
            stroke: None,
            stroke_width: 0.0,
        };
        self.inner.push_node(node, None);
        Ok(())
    }

    pub(super) fn text(
        &mut self,
        content: ImmutableString,
        font_size: Dynamic,
        color: &str,
    ) -> RhaiResult<()> {
        let size = dynamic_to_non_negative_f32(&font_size, "font size")?;
        let col = parse_color(color)?;
        let approx_w = content.chars().count() as f32 * size * 0.6;
        let approx_h = size * 1.2;
        let node = SceneNode::Text {
            x: 0.0,
            y: 0.0,
            content: content.into_owned(),
            font_size: size,
            color: col,
            font_weight: 400,
            font_sources: Vec::new(),
        };
        let style = Style {
            size: Size {
                width: length(approx_w),
                height: length(approx_h),
            },
            ..Default::default()
        };
        self.inner.push_node(node, Some(style));
        Ok(())
    }

    pub(super) fn text_bold(
        &mut self,
        content: ImmutableString,
        font_size: Dynamic,
        color: &str,
    ) -> RhaiResult<()> {
        let size = dynamic_to_non_negative_f32(&font_size, "font size")?;
        let col = parse_color(color)?;
        let approx_w = content.chars().count() as f32 * size * 0.6;
        let approx_h = size * 1.2;
        let node = SceneNode::Text {
            x: 0.0,
            y: 0.0,
            content: content.into_owned(),
            font_size: size,
            color: col,
            font_weight: 700,
            font_sources: Vec::new(),
        };
        let style = Style {
            size: Size {
                width: length(approx_w),
                height: length(approx_h),
            },
            ..Default::default()
        };
        self.inner.push_node(node, Some(style));
        Ok(())
    }
}
