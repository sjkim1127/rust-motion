//! TikTok / Short-form kinetic caption pagination.

use crate::types::{CaptionPage, CaptionToken};

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
