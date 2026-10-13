//! TikTok / Short-form kinetic caption pagination and Dioxus component.

use crate::types::{CaptionPage, CaptionToken};
#[cfg(feature = "dioxus")]
use dioxus::prelude::*;
#[cfg(feature = "dioxus")]
use rust_motion_core::hooks::{use_current_frame, use_video_config};

/// Groups a flat list of [`CaptionToken`]s into pages containing at most `max_words_per_page` tokens.
pub fn create_tiktok_style_captions(
    tokens: &[CaptionToken],
    max_words_per_page: usize,
) -> Vec<CaptionPage> {
    let word_limit = max_words_per_page.max(1);
    let mut pages = Vec::new();

    for chunk in tokens.chunks(word_limit) {
        if chunk.is_empty() {
            continue;
        }

        let page_start = chunk.first().unwrap().start_ms;
        let page_end = chunk.last().unwrap().end_ms;

        pages.push(CaptionPage {
            tokens: chunk.to_vec(),
            start_ms: page_start,
            end_ms: page_end,
        });
    }

    pages
}

/// Props for the `<TikTokCaptions>` component.
#[cfg(feature = "dioxus")]
#[derive(Props, Clone, PartialEq)]
pub struct TikTokCaptionsProps {
    /// Subtitle tokens.
    pub tokens: Vec<CaptionToken>,
    /// Maximum words to display per caption line/page.
    #[props(default = 3)]
    pub max_words_per_page: usize,
    /// Color of the currently spoken active word (e.g. `"#ffe600"`).
    #[props(default = "#ffe600".to_string())]
    pub active_color: String,
    /// Color of inactive words in the page.
    #[props(default = "#ffffff".to_string())]
    pub inactive_color: String,
    /// Scale factor for active word bounce effect.
    #[props(default = 1.15)]
    pub active_scale: f64,
    /// Font size in pixels.
    #[props(default = 48.0)]
    pub font_size: f64,
    /// Font weight.
    #[props(default = "800".to_string())]
    pub font_weight: String,
    /// Text shadow CSS.
    #[props(default = "0px 4px 12px rgba(0,0,0,0.8)".to_string())]
    pub text_shadow: String,
    /// Additional CSS style.
    #[props(default)]
    pub style: String,
}

/// Dioxus component for rendering kinetic TikTok-style animated subtitles.
#[cfg(feature = "dioxus")]
#[component]
pub fn TikTokCaptions(props: TikTokCaptionsProps) -> Element {
    let frame = use_current_frame();
    let config = use_video_config();

    // Convert current frame to current playback timestamp in milliseconds
    let current_ms = ((frame as f64 / config.fps) * 1000.0) as u64;

    let pages = create_tiktok_style_captions(&props.tokens, props.max_words_per_page);

    // Find page active at current playback timestamp
    let current_page = pages
        .iter()
        .find(|p| current_ms >= p.start_ms && current_ms <= p.end_ms);

    let container_style = format!(
        "display: flex; align-items: center; justify-content: center; gap: 14px; \
         font-size: {}px; font-weight: {}; text-shadow: {}; {};",
        props.font_size, props.font_weight, props.text_shadow, props.style
    );

    rsx! {
        if let Some(page) = current_page {
            div {
                style: "{container_style}",
                for token in page.tokens.iter() {
                    {
                        let is_active = current_ms >= token.start_ms && current_ms <= token.end_ms;
                        let color = if is_active { &props.active_color } else { &props.inactive_color };
                        let scale = if is_active { props.active_scale } else { 1.0 };
                        let word_style = format!(
                            "color: {}; transform: scale({}); transition: transform 0.1s ease, color 0.1s ease; display: inline-block;",
                            color, scale
                        );
                        rsx! {
                            span {
                                key: "{token.start_ms}-{token.text}",
                                style: "{word_style}",
                                "{token.text}"
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_tiktok_style_captions() {
        let tokens = vec![
            CaptionToken::new("One", 0, 500),
            CaptionToken::new("Two", 500, 1000),
            CaptionToken::new("Three", 1000, 1500),
            CaptionToken::new("Four", 1500, 2000),
        ];

        let pages = create_tiktok_style_captions(&tokens, 2);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].tokens.len(), 2);
        assert_eq!(pages[0].start_ms, 0);
        assert_eq!(pages[0].end_ms, 1000);

        assert_eq!(pages[1].tokens.len(), 2);
        assert_eq!(pages[1].start_ms, 1000);
        assert_eq!(pages[1].end_ms, 2000);
    }
}
