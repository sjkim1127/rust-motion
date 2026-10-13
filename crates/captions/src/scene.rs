//! Native Scene emitter for timed kinetic captions.

use crate::layout::wrap_caption_tokens_to_lines;
use crate::{create_tiktok_style_captions, CaptionToken};
use rust_motion_composition::{CompositionError, SceneEmitter, SceneFrameContext};
use rust_motion_rasterizer::{measure_text_width, Color, Scene, SceneNode};
use serde_json::Value;

/// Native counterpart of [`crate::TikTokCaptions`].
#[derive(Debug, Clone, PartialEq)]
pub struct SceneCaptions {
    pub tokens: Vec<CaptionToken>,
    pub max_words_per_page: usize,
    pub center_x: f32,
    pub baseline_y: f32,
    pub font_size: f32,
    pub font_weight: u16,
    /// Ordered local TTF/OTF sources used by native export and Player preview.
    pub font_sources: Vec<String>,
    pub active_color: String,
    pub inactive_color: String,
    pub active_scale: f32,
    pub word_gap: f32,
    /// Maximum bounding width for automatic multi-line wrapping.
    pub max_width: Option<f32>,
    /// Line height multiplier (e.g. 1.3).
    pub line_height_mult: f32,
    /// Optional background pill/box color behind words (e.g. "rgba(0,0,0,0.7)").
    pub bg_color: Option<String>,
    /// Padding (x, y) for background pill.
    pub bg_padding: (f32, f32),
    /// Corner radius for background pill.
    pub bg_radius: f32,
}

impl SceneCaptions {
    pub fn new(tokens: Vec<CaptionToken>, center_x: f32, baseline_y: f32) -> Self {
        Self {
            tokens,
            max_words_per_page: 3,
            center_x,
            baseline_y,
            font_size: 48.0,
            font_weight: 800,
            font_sources: Vec::new(),
            active_color: "#ffe600".into(),
            inactive_color: "#ffffff".into(),
            active_scale: 1.15,
            word_gap: 14.0,
            max_width: None,
            line_height_mult: 1.3,
            bg_color: None,
            bg_padding: (16.0, 8.0),
            bg_radius: 12.0,
        }
    }

    /// Sets maximum bounding width for auto-wrapping.
    pub fn with_max_width(mut self, width: f32) -> Self {
        self.max_width = Some(width);
        self
    }

    /// Sets background pill styling.
    pub fn with_background(
        mut self,
        color: impl Into<String>,
        padding: (f32, f32),
        radius: f32,
    ) -> Self {
        self.bg_color = Some(color.into());
        self.bg_padding = padding;
        self.bg_radius = radius;
        self
    }
}

impl SceneEmitter for SceneCaptions {
    fn emit(
        &self,
        context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let current_ms = (context.time_secs().max(0.0) * 1000.0).round() as u64;
        let pages = create_tiktok_style_captions(&self.tokens, self.max_words_per_page);
        let Some(page) = pages
            .iter()
            .find(|page| current_ms >= page.start_ms && current_ms <= page.end_ms)
        else {
            return Ok(());
        };

        let active_color = parse_color(&self.active_color, context)?;
        let inactive_color = parse_color(&self.inactive_color, context)?;
        let bg_color = if let Some(ref bg) = self.bg_color {
            Some(parse_color(bg, context)?)
        } else {
            None
        };

        let line_height = self.font_size * self.line_height_mult;

        // Break tokens into lines if max_width is provided
        let lines = if let Some(max_w) = self.max_width {
            wrap_caption_tokens_to_lines(
                &page.tokens,
                max_w,
                self.font_size,
                self.word_gap,
                &self.font_sources,
            )
        } else {
            vec![crate::layout::CaptionLineLayout {
                tokens: page.tokens.clone(),
                token_widths: Vec::new(),
                total_width: 0.0,
                height: self.font_size,
            }]
        };

        let total_block_height = lines.len() as f32 * line_height;
        let start_y = self.baseline_y - (total_block_height * 0.5) + (line_height * 0.5);

        for (line_idx, line) in lines.iter().enumerate() {
            let line_y = start_y + (line_idx as f32 * line_height);

            let metrics =
                line.tokens
                    .iter()
                    .map(|token| {
                        let active = current_ms >= token.start_ms && current_ms <= token.end_ms;
                        let size = if active {
                            self.font_size * self.active_scale.max(0.0)
                        } else {
                            self.font_size
                        };
                        let width = measure_text_width(&token.text, size, &self.font_sources)
                            .map_err(|error| {
                                CompositionError::render(
                                    context.global_frame,
                                    format!("failed to measure native caption text: {error}"),
                                )
                            })?;
                        Ok((token, active, size, width))
                    })
                    .collect::<Result<Vec<_>, CompositionError>>()?;

            let row_width = metrics.iter().map(|(_, _, _, width)| *width).sum::<f32>()
                + self.word_gap.max(0.0) * metrics.len().saturating_sub(1) as f32;

            let line_start_x = self.center_x - row_width * 0.5;

            // Draw background pill if requested
            if let Some(bg) = bg_color {
                let (pad_x, pad_y) = self.bg_padding;
                scene.push(SceneNode::Rect {
                    x: line_start_x - pad_x,
                    y: line_y - self.font_size * 0.85 - pad_y,
                    w: row_width + pad_x * 2.0,
                    h: self.font_size * 1.1 + pad_y * 2.0,
                    fill: bg,
                    stroke: None,
                    stroke_width: 0.0,
                    corner_radius: self.bg_radius,
                });
            }

            let mut x = line_start_x;
            for (token, active, size, width) in metrics {
                scene.push(SceneNode::Text {
                    x,
                    y: line_y,
                    content: token.text.clone(),
                    font_size: size,
                    color: if active { active_color } else { inactive_color },
                    font_weight: self.font_weight,
                    font_sources: self.font_sources.clone(),
                });
                x += width + self.word_gap.max(0.0);
            }
        }

        Ok(())
    }
}

fn parse_color(value: &str, context: SceneFrameContext) -> Result<Color, CompositionError> {
    Color::from_css(value).ok_or_else(|| {
        CompositionError::render(
            context.global_frame,
            format!("unsupported native caption color '{value}'"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_motion_composition::{
        NativeComposition, NativeCompositionContext, SceneEmitterComposition,
    };

    fn context() -> NativeCompositionContext {
        NativeCompositionContext {
            width: 320,
            height: 180,
            fps: 10.0,
            duration_in_frames: 30,
        }
    }

    #[test]
    fn active_caption_page_emits_individually_colored_words() {
        let captions = SceneCaptions::new(
            vec![
                CaptionToken::new("Hello", 0, 500),
                CaptionToken::new("world", 500, 1000),
            ],
            160.0,
            120.0,
        );
        let composition = SceneEmitterComposition::new("captions", captions);
        let scene = composition.render(7, &Value::Null, context()).unwrap();

        assert_eq!(scene.nodes.len(), 2);
        assert!(matches!(
            &scene.nodes[0],
            SceneNode::Text { content, color, .. }
                if content == "Hello" && *color == Color::WHITE
        ));
        assert!(matches!(
            &scene.nodes[1],
            SceneNode::Text { content, color, font_size, .. }
                if content == "world"
                    && *color == Color::rgb(255, 230, 0)
                    && (*font_size - 55.2).abs() < 0.01
        ));
    }

    #[test]
    fn captions_with_background_emits_pill_rect() {
        let captions = SceneCaptions::new(vec![CaptionToken::new("Pill", 0, 1000)], 160.0, 120.0)
            .with_background("rgba(0,0,0,0.8)", (12.0, 6.0), 8.0);

        let composition = SceneEmitterComposition::new("captions", captions);
        let scene = composition.render(5, &Value::Null, context()).unwrap();

        // 1 Rect (background pill) + 1 Text
        assert_eq!(scene.nodes.len(), 2);
        assert!(matches!(&scene.nodes[0], SceneNode::Rect { .. }));
        assert!(matches!(&scene.nodes[1], SceneNode::Text { content, .. } if content == "Pill"));
    }
}
