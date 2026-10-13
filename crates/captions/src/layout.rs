//! Caption token layout and pixel-width multi-line wrapping.

use crate::types::CaptionToken;
use rust_motion_rasterizer::measure_text_width;

/// A laid-out line of caption tokens with measured dimensions.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptionLineLayout {
    pub tokens: Vec<CaptionToken>,
    pub token_widths: Vec<f32>,
    pub total_width: f32,
    pub height: f32,
}

/// Wraps caption tokens into multiple lines such that no line exceeds `max_width_px`.
pub fn wrap_caption_tokens_to_lines(
    tokens: &[CaptionToken],
    max_width_px: f32,
    font_size: f32,
    word_gap: f32,
    font_sources: &[String],
) -> Vec<CaptionLineLayout> {
    if tokens.is_empty() {
        return Vec::new();
    }

    let max_w = max_width_px.max(50.0);
    let gap = word_gap.max(0.0);

    // Measure each token width
    let measured_tokens: Vec<(CaptionToken, f32)> = tokens
        .iter()
        .map(|t| {
            let width = measure_text_width(&t.text, font_size, font_sources)
                .unwrap_or_else(|_| t.text.chars().count() as f32 * font_size * 0.55);
            (t.clone(), width)
        })
        .collect();

    let mut lines = Vec::new();
    let mut current_line_tokens = Vec::new();
    let mut current_line_widths = Vec::new();
    let mut current_line_w = 0.0;

    for (token, width) in measured_tokens {
        let needed_w = if current_line_tokens.is_empty() {
            width
        } else {
            current_line_w + gap + width
        };

        if needed_w > max_w && !current_line_tokens.is_empty() {
            // Push current line and start a new one
            lines.push(CaptionLineLayout {
                tokens: std::mem::take(&mut current_line_tokens),
                token_widths: std::mem::take(&mut current_line_widths),
                total_width: current_line_w,
                height: font_size,
            });
            current_line_w = 0.0;
        }

        current_line_w = if current_line_tokens.is_empty() {
            width
        } else {
            current_line_w + gap + width
        };
        current_line_tokens.push(token);
        current_line_widths.push(width);
    }

    if !current_line_tokens.is_empty() {
        lines.push(CaptionLineLayout {
            tokens: current_line_tokens,
            token_widths: current_line_widths,
            total_width: current_line_w,
            height: font_size,
        });
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_tokens_to_lines() {
        let tokens = vec![
            CaptionToken::new("The", 0, 200),
            CaptionToken::new("quick", 200, 400),
            CaptionToken::new("brown", 400, 600),
            CaptionToken::new("fox", 600, 800),
            CaptionToken::new("jumps", 800, 1000),
        ];

        // Small max_width forces multiple lines
        let lines = wrap_caption_tokens_to_lines(&tokens, 150.0, 32.0, 10.0, &[]);
        assert!(lines.len() > 1);
        let total_tokens: usize = lines.iter().map(|l| l.tokens.len()).sum();
        assert_eq!(total_tokens, 5);
    }
}
