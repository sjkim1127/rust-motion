//! `TextLayoutEngine` typography subsystem for browser-free video generation.
//!
//! Provides:
//! - HarfBuzz OpenType shaping via `rustybuzz`
//! - Unicode Bidirectional Algorithm (UAX #9) via `unicode-bidi`
//! - Unicode Line Breaking (UAX #14) via `unicode-linebreak`
//! - Deterministic font fallback and cluster-to-char mapping

pub mod bidi;
pub mod engine;
pub mod font_resolver;
pub mod layout;
pub mod line_breaker;
pub mod shaper;
pub mod style;

pub use bidi::{contains_rtl, segment_bidi_runs, BidiRun};
pub use engine::TextLayoutEngine;
pub use font_resolver::FontResolver;
pub use layout::{LayoutLine, PositionedGlyph, Rect, TextLayout};
pub use line_breaker::{break_lines, LineGlyphs};
pub use shaper::{shape_bidi_runs, ShapedGlyph, ShapedRun};
pub use style::{TextAlignment, TextDirection, TextStyle};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::FontCache;
    use std::sync::Arc;

    #[test]
    fn test_bidi_segmentation_pure_ltr() {
        let text = "Hello World 123";
        let runs = segment_bidi_runs(text, TextDirection::Auto);
        assert_eq!(runs.len(), 1);
        assert!(!runs[0].is_rtl);
        assert_eq!(runs[0].range, 0..text.len());
    }

    #[test]
    fn test_bidi_segmentation_pure_rtl() {
        let text = "مرحبا بالعالم"; // Arabic "Hello World"
        assert!(contains_rtl(text));
        let runs = segment_bidi_runs(text, TextDirection::Auto);
        assert!(!runs.is_empty());
        assert!(runs.iter().any(|r| r.is_rtl));
    }

    #[test]
    fn test_bidi_segmentation_mixed_ltr_and_rtl() {
        let text = "Hello مرحبا World";
        let runs = segment_bidi_runs(text, TextDirection::Auto);
        assert!(runs.len() >= 2);
        let has_ltr = runs.iter().any(|r| !r.is_rtl);
        let has_rtl = runs.iter().any(|r| r.is_rtl);
        assert!(has_ltr && has_rtl);
    }

    #[test]
    fn test_text_layout_engine_basic_layout() {
        let cache = Arc::new(FontCache::bundled());
        let engine = TextLayoutEngine::new(cache);
        let style = TextStyle::new(32.0);

        let layout = engine
            .layout("Rust Motion Engine", &style)
            .expect("layout should succeed");
        assert_eq!(layout.lines.len(), 1);
        assert!(layout.width > 50.0);
        assert!(layout.height > 20.0);
        assert!(layout.glyph_count() > 0);
    }

    #[test]
    fn test_text_layout_engine_line_wrapping() {
        let cache = Arc::new(FontCache::bundled());
        let engine = TextLayoutEngine::new(cache);
        let style = TextStyle::new(24.0).with_max_width(180.0);

        let long_text =
            "This is a long sentence rendered by the native typography engine to verify wrapping.";
        let layout = engine
            .layout(long_text, &style)
            .expect("wrapped layout should succeed");
        assert!(
            layout.lines.len() > 1,
            "text should wrap into multiple lines"
        );
        for line in &layout.lines {
            assert!(
                line.width <= 250.0,
                "line width {} exceeded tolerance",
                line.width
            );
        }
    }

    #[test]
    fn test_text_layout_engine_alignment_offsets() {
        let cache = Arc::new(FontCache::bundled());
        let engine = TextLayoutEngine::new(cache);

        let style_left = TextStyle::new(24.0)
            .with_max_width(500.0)
            .with_align(TextAlignment::Left);
        let layout_left = engine.layout("Aligned", &style_left).unwrap();

        let style_center = TextStyle::new(24.0)
            .with_max_width(500.0)
            .with_align(TextAlignment::Center);
        let layout_center = engine.layout("Aligned", &style_center).unwrap();

        let style_right = TextStyle::new(24.0)
            .with_max_width(500.0)
            .with_align(TextAlignment::Right);
        let layout_right = engine.layout("Aligned", &style_right).unwrap();

        assert_eq!(layout_left.lines[0].x, 0.0);
        assert!(layout_center.lines[0].x > 0.0);
        assert!(layout_right.lines[0].x > layout_center.lines[0].x);
    }

    #[test]
    fn test_text_layout_cjk_line_breaking() {
        let cache = Arc::new(FontCache::bundled());
        let engine = TextLayoutEngine::new(cache);
        let style = TextStyle::new(20.0).with_max_width(120.0);

        let korean_text = "안녕하세요 반값습니다 비디오 렌더러 테스트 문장입니다.";
        let layout = engine
            .layout(korean_text, &style)
            .expect("CJK layout should succeed");
        assert!(
            layout.lines.len() > 1,
            "Korean text should wrap into multiple lines"
        );
    }

    #[test]
    fn test_text_layout_empty_string() {
        let cache = Arc::new(FontCache::bundled());
        let engine = TextLayoutEngine::new(cache);
        let style = TextStyle::new(24.0);

        let layout = engine
            .layout("", &style)
            .expect("empty layout should succeed");
        assert_eq!(layout.lines.len(), 0);
        assert_eq!(layout.width, 0.0);
        assert_eq!(layout.height, 0.0);
    }
}
