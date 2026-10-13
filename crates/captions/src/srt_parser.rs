//! SRT subtitle format parser and serializer.

use crate::types::CaptionToken;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum CaptionParseError {
    #[error("Invalid timestamp format '{0}'")]
    InvalidTimestamp(String),
    #[error("Malformed SRT block near index {0}")]
    MalformedBlock(usize),
}

/// Parses SubRip (.srt) subtitle string content into a list of [`CaptionToken`]s.
pub fn parse_srt(srt_content: &str) -> Result<Vec<CaptionToken>, CaptionParseError> {
    let mut tokens = Vec::new();
    let normalized = srt_content.replace("\r\n", "\n");
    let blocks = normalized
        .split("\n\n")
        .map(|s| s.trim())
        .collect::<Vec<_>>();

    for block in blocks {
        if block.is_empty() {
            continue;
        }

        let lines = block.lines().collect::<Vec<_>>();
        if lines.len() < 2 {
            continue;
        }

        // Line 0 is sequence index (optional/ignored), Line 1 is timestamp range
        let time_line = if lines[0].contains("-->") {
            lines[0]
        } else if lines.len() >= 2 && lines[1].contains("-->") {
            lines[1]
        } else {
            continue;
        };

        let time_parts = time_line.split("-->").map(|s| s.trim()).collect::<Vec<_>>();
        if time_parts.len() != 2 {
            return Err(CaptionParseError::InvalidTimestamp(time_line.to_string()));
        }

        let start_ms = parse_srt_timestamp(time_parts[0])?;
        let end_ms = parse_srt_timestamp(time_parts[1])?;

        // Text lines (skip index and timestamp line)
        let text_lines: Vec<&str> = lines
            .iter()
            .copied()
            .filter(|l| !l.contains("-->") && !l.parse::<usize>().is_ok())
            .collect();
        let full_text = text_lines.join(" ");

        if full_text.trim().is_empty() {
            continue;
        }

        // Split subtitle phrase into word tokens evenly across the duration
        let words: Vec<&str> = full_text.split_whitespace().collect();
        if words.is_empty() {
            continue;
        }

        let total_duration = end_ms.saturating_sub(start_ms);
        let word_duration = (total_duration as f64 / words.len() as f64).max(1.0);

        for (w_idx, word) in words.iter().enumerate() {
            let w_start = start_ms + (w_idx as f64 * word_duration) as u64;
            let w_end = if w_idx == words.len() - 1 {
                end_ms
            } else {
                start_ms + ((w_idx + 1) as f64 * word_duration) as u64
            };

            tokens.push(CaptionToken::new(*word, w_start, w_end));
        }
    }

    Ok(tokens)
}

fn parse_srt_timestamp(ts: &str) -> Result<u64, CaptionParseError> {
    // Format: "00:01:23,456" or "00:01:23.456"
    let clean = ts.replace(',', ".");
    let parts: Vec<&str> = clean.split(':').collect();
    if parts.len() != 3 {
        return Err(CaptionParseError::InvalidTimestamp(ts.to_string()));
    }

    let hours: u64 = parts[0]
        .trim()
        .parse()
        .map_err(|_| CaptionParseError::InvalidTimestamp(ts.to_string()))?;
    let mins: u64 = parts[1]
        .trim()
        .parse()
        .map_err(|_| CaptionParseError::InvalidTimestamp(ts.to_string()))?;

    let sec_parts: Vec<&str> = parts[2].split('.').collect();
    if sec_parts.len() != 2 {
        return Err(CaptionParseError::InvalidTimestamp(ts.to_string()));
    }

    let secs: u64 = sec_parts[0]
        .trim()
        .parse()
        .map_err(|_| CaptionParseError::InvalidTimestamp(ts.to_string()))?;
    let ms: u64 = sec_parts[1]
        .trim()
        .parse()
        .map_err(|_| CaptionParseError::InvalidTimestamp(ts.to_string()))?;

    Ok(hours * 3_600_000 + mins * 60_000 + secs * 1_000 + ms)
}

/// Formats milliseconds into standard SRT timestamp string `"HH:MM:SS,mmm"`.
pub fn format_srt_timestamp(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let rem = ms % 3_600_000;
    let mins = rem / 60_000;
    let rem = rem % 60_000;
    let secs = rem / 1_000;
    let millis = rem % 1_000;

    format!("{hours:02}:{mins:02}:{secs:02},{millis:03}")
}

/// Serializes a list of [`CaptionToken`]s into a standard SRT string.
pub fn serialize_srt(tokens: &[CaptionToken]) -> String {
    let mut blocks = Vec::new();

    for (idx, token) in tokens.iter().enumerate() {
        let start_str = format_srt_timestamp(token.start_ms);
        let end_str = format_srt_timestamp(token.end_ms);
        let block = format!("{}\n{} --> {}\n{}", idx + 1, start_str, end_str, token.text);
        blocks.push(block);
    }

    blocks.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_and_serialize_srt() {
        let srt = r#"1
00:00:01,000 --> 00:00:03,000
Hello Rust-Motion

2
00:00:04,500 --> 00:00:06,000
Fast video rendering"#;

        let tokens = parse_srt(srt).expect("SRT parse failed");
        assert_eq!(tokens.len(), 5); // ["Hello", "Rust-Motion", "Fast", "video", "rendering"]
        assert_eq!(tokens[0].text, "Hello");
        assert_eq!(tokens[0].start_ms, 1000);
        assert_eq!(tokens[1].text, "Rust-Motion");

        let serialized = serialize_srt(&tokens[0..2]);
        assert!(serialized.contains("00:00:01,000 --> 00:00:02,000\nHello"));
    }
}
