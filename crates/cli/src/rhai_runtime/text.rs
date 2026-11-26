//! Scene builder methods for plain and boxed text.

use super::*;

impl SceneBuilder {
    pub(super) fn text(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        content: ImmutableString,
        font_size: FLOAT,
        color: &str,
    ) -> RhaiResult<()> {
        self.push_text(x, y, content, font_size, color, 400)
    }

    pub(super) fn text_bold(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        content: ImmutableString,
        font_size: FLOAT,
        color: &str,
    ) -> RhaiResult<()> {
        self.push_text(x, y, content, font_size, color, 700)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn text_font(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        content: ImmutableString,
        font_size: FLOAT,
        color: &str,
        font_source: ImmutableString,
    ) -> RhaiResult<()> {
        let font_source = font_source.trim();
        if font_source.is_empty() {
            return Err(runtime_error("font source path must not be empty".into()));
        }
        self.scene.push(SceneNode::Text {
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            content: content.into_owned(),
            font_size: non_negative_f32("font size", font_size)?,
            color: parse_color(color)?,
            font_weight: 400,
            font_sources: vec![font_source.to_string()],
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn text_box(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        width: FLOAT,
        height: FLOAT,
        content: ImmutableString,
        font_size: FLOAT,
        min_font_size: FLOAT,
        max_lines: INT,
        color: &str,
        font_source: ImmutableString,
        align: &str,
    ) -> RhaiResult<()> {
        if max_lines <= 0 {
            return Err(runtime_error("text box max lines must be positive".into()));
        }
        let font_sources = if font_source.trim().is_empty() {
            Vec::new()
        } else {
            vec![font_source.trim().to_string()]
        };
        let mut request = TextBox::new(
            content.into_owned(),
            finite_f32("x", x)?,
            finite_f32("y", y)?,
            finite_f32("width", width)?,
            finite_f32("height", height)?,
            finite_f32("font size", font_size)?,
        );
        request.min_font_size = finite_f32("minimum font size", min_font_size)?;
        request.max_lines = Some(max_lines as usize);
        request.horizontal_align = match align.trim().to_ascii_lowercase().as_str() {
            "left" | "start" => TextHorizontalAlign::Start,
            "center" => TextHorizontalAlign::Center,
            "right" | "end" => TextHorizontalAlign::End,
            _ => {
                return Err(runtime_error(format!(
                    "text box alignment must be start, center, or end, got '{align}'"
                )))
            }
        };
        request.overflow = TextOverflow::Ellipsis;
        request.font_sources = font_sources.clone();
        let layout = layout_text_box(&request)
            .map_err(|error| runtime_error(format!("failed to layout text box: {error}")))?;
        let color = parse_color(color)?;
        for line in layout.lines {
            if line.text.is_empty() {
                continue;
            }
            self.scene.push(SceneNode::Text {
                x: line.x,
                y: line.y,
                content: line.text,
                font_size: layout.font_size,
                color,
                font_weight: 400,
                font_sources: font_sources.clone(),
            });
        }
        Ok(())
    }

    fn push_text(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        content: ImmutableString,
        font_size: FLOAT,
        color: &str,
        font_weight: u16,
    ) -> RhaiResult<()> {
        self.scene.push(SceneNode::Text {
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            content: content.into_owned(),
            font_size: non_negative_f32("font size", font_size)?,
            color: parse_color(color)?,
            font_weight,
            font_sources: Vec::new(),
        });
        Ok(())
    }
}
