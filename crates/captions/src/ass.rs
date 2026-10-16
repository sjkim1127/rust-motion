//! Advanced SubStation Alpha (.ass / .ssa) Subtitle Engine with Karaoke Support.
//!
//! Handles parsing and generating styled SubStation Alpha subtitles, including
//! karaoke timing tags (`{\k<duration>}` / `{\K<duration>}` in centiseconds),
//! primary/secondary color definitions, outline, shadow, and positioning.

use crate::types::CaptionToken;
use std::fmt::Write as _;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AssError {
    #[error("Missing [Events] section in ASS subtitle")]
    MissingEventsSection,
    #[error("Invalid ASS timestamp format: {0}")]
    InvalidTimestamp(String),
}

/// Convert milliseconds into ASS timestamp format `H:MM:SS.cc` (centiseconds).
pub fn format_ass_timestamp(ms: u64) -> String {
    let cs = (ms % 1000) / 10;
    let total_s = ms / 1000;
    let s = total_s % 60;
    let m = (total_s / 60) % 60;
    let h = total_s / 3600;
    format!("{h}:{m:02}:{s:02}.{cs:02}")
}

/// Parse ASS timestamp `H:MM:SS.cc` or `HH:MM:SS.cc` into milliseconds.
pub fn parse_ass_timestamp(s: &str) -> Result<u64, AssError> {
    let parts: Vec<&str> = s.trim().split(':').collect();
    if parts.len() != 3 {
        return Err(AssError::InvalidTimestamp(s.into()));
    }
    let h: u64 = parts[0].parse().map_err(|_| AssError::InvalidTimestamp(s.into()))?;
    let m: u64 = parts[1].parse().map_err(|_| AssError::InvalidTimestamp(s.into()))?;

    let sec_parts: Vec<&str> = parts[2].split('.').collect();
    if sec_parts.is_empty() {
        return Err(AssError::InvalidTimestamp(s.into()));
    }
    let sec: u64 = sec_parts[0].parse().map_err(|_| AssError::InvalidTimestamp(s.into()))?;
    let cs: u64 = if sec_parts.len() > 1 {
        let frac_str = sec_parts[1];
        if frac_str.len() == 1 {
            frac_str.parse::<u64>().unwrap_or(0) * 10
        } else {
            frac_str[..2].parse().unwrap_or(0)
        }
    } else {
        0
    };

    Ok(h * 3600 * 1000 + m * 60 * 1000 + sec * 1000 + cs * 10)
}

/// A parsed ASS dialogue event line.
#[derive(Debug, Clone, PartialEq)]
pub struct AssDialogue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub style: String,
    pub text: String,
    /// Extracted timed words if karaoke tags `{\k...}` were present.
    pub karaoke_tokens: Vec<CaptionToken>,
}

/// Serialize a list of karaoke tokens into an ASS document with a styled karaoke header.
pub fn serialize_ass_karaoke(
    lines: &[Vec<CaptionToken>],
    title: &str,
    font_name: &str,
    font_size: u32,
    primary_color_bgr: &str, // e.g. "&H00FFFFFF"
    karaoke_color_bgr: &str, // e.g. "&H0000FFFF" (Yellow)
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "[Script Info]");
    let _ = writeln!(out, "Title: {}", title.trim());
    let _ = writeln!(out, "ScriptType: v4.00+");
    let _ = writeln!(out, "PlayResX: 1920");
    let _ = writeln!(out, "PlayResY: 1080");
    let _ = writeln!(out);

    let _ = writeln!(out, "[V4+ Styles]");
    let _ = writeln!(
        out,
        "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding"
    );
    let _ = writeln!(
        out,
        "Style: Karaoke,{font_name},{font_size},{primary_color_bgr},{karaoke_color_bgr},&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,3,2,2,20,20,40,1"
    );
    let _ = writeln!(out);

    let _ = writeln!(out, "[Events]");
    let _ = writeln!(out, "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text");

    for line_tokens in lines {
        if line_tokens.is_empty() {
            continue;
        }
        let line_start = line_tokens.first().unwrap().start_ms;
        let line_end = line_tokens.last().unwrap().end_ms;
        let start_str = format_ass_timestamp(line_start);
        let end_str = format_ass_timestamp(line_end);

        let mut text_part = String::new();
        for tok in line_tokens {
            let dur_cs = (tok.end_ms.saturating_sub(tok.start_ms)) / 10;
            let _ = write!(text_part, r"{{\k{dur_cs}}}{} ", tok.text.trim());
        }

        let _ = writeln!(
            out,
            "Dialogue: 0,{start_str},{end_str},Karaoke,,0,0,0,,{}",
            text_part.trim_end()
        );
    }

    out
}

/// Parse an ASS subtitle file and extract dialogue lines and karaoke timings.
pub fn parse_ass(ass_text: &str) -> Result<Vec<AssDialogue>, AssError> {
    let mut in_events = false;
    let mut format_indices: Option<(usize, usize, usize, usize)> = None;
    let mut dialogues = Vec::new();

    for line in ass_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_events = trimmed.eq_ignore_ascii_case("[Events]");
            continue;
        }

        if !in_events || trimmed.is_empty() || trimmed.starts_with(';') {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("Format:") {
            let cols: Vec<&str> = rest.split(',').map(|c| c.trim()).collect();
            let mut start_idx = None;
            let mut end_idx = None;
            let mut style_idx = None;
            let mut text_idx = None;

            for (i, &col) in cols.iter().enumerate() {
                if col.eq_ignore_ascii_case("Start") {
                    start_idx = Some(i);
                } else if col.eq_ignore_ascii_case("End") {
                    end_idx = Some(i);
                } else if col.eq_ignore_ascii_case("Style") {
                    style_idx = Some(i);
                } else if col.eq_ignore_ascii_case("Text") {
                    text_idx = Some(i);
                }
            }

            if let (Some(s), Some(e), Some(st), Some(t)) = (start_idx, end_idx, style_idx, text_idx) {
                format_indices = Some((s, e, st, t));
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("Dialogue:") {
            let (s_col, e_col, st_col, t_col) = format_indices.unwrap_or((1, 2, 3, 9));
            // Split with limit to keep text intact if it contains commas
            let parts: Vec<&str> = rest.splitn(t_col + 1, ',').map(|p| p.trim()).collect();
            if parts.len() <= t_col {
                continue;
            }

            let start_ms = parse_ass_timestamp(parts[s_col])?;
            let end_ms = parse_ass_timestamp(parts[e_col])?;
            let style = parts[st_col].to_string();
            let raw_text = parts[t_col].to_string();

            // Extract karaoke tokens if present
            let mut karaoke_tokens = Vec::new();
            let mut current_time = start_ms;

            let mut cursor = 0;
            while let Some(tag_start) = raw_text[cursor..].find(r"{\k") {
                let abs_start = cursor + tag_start;
                if let Some(tag_end) = raw_text[abs_start..].find('}') {
                    let tag_content = &raw_text[abs_start + 3..abs_start + tag_end];
                    let cs: u64 = tag_content.parse().unwrap_or(0);
                    let word_dur_ms = cs * 10;

                    let word_start = abs_start + tag_end + 1;
                    let next_tag = raw_text[word_start..].find(r"{\k").unwrap_or(raw_text.len() - word_start);
                    let word_text = raw_text[word_start..word_start + next_tag].trim();

                    if !word_text.is_empty() {
                        karaoke_tokens.push(CaptionToken::new(
                            word_text,
                            current_time,
                            current_time + word_dur_ms,
                        ));
                        current_time += word_dur_ms;
                    }
                    cursor = word_start;
                } else {
                    break;
                }
            }

            dialogues.push(AssDialogue {
                start_ms,
                end_ms,
                style,
                text: raw_text,
                karaoke_tokens,
            });
        }
    }

    Ok(dialogues)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ass_timestamp_roundtrip() {
        assert_eq!(format_ass_timestamp(0), "0:00:00.00");
        assert_eq!(format_ass_timestamp(1250), "0:00:01.25");
        assert_eq!(format_ass_timestamp(3661450), "1:01:01.45");

        assert_eq!(parse_ass_timestamp("0:00:01.25").unwrap(), 1250);
        assert_eq!(parse_ass_timestamp("1:01:01.45").unwrap(), 3661450);
    }

    #[test]
    fn test_ass_karaoke_roundtrip() {
        let tokens = vec![
            vec![
                CaptionToken::new("Make", 1000, 1500),
                CaptionToken::new("motion", 1500, 2200),
                CaptionToken::new("magic", 2200, 3000),
            ]
        ];

        let ass = serialize_ass_karaoke(&tokens, "Demo", "Arial", 64, "&H00FFFFFF", "&H0000FFFF");
        assert!(ass.contains("[Script Info]"));
        assert!(ass.contains("Dialogue: 0,0:00:01.00,0:00:03.00,Karaoke,,0,0,0,,{\\k50}Make {\\k70}motion {\\k80}magic"));

        let dialogues = parse_ass(&ass).unwrap();
        assert_eq!(dialogues.len(), 1);
        assert_eq!(dialogues[0].start_ms, 1000);
        assert_eq!(dialogues[0].end_ms, 3000);
        assert_eq!(dialogues[0].karaoke_tokens.len(), 3);
        assert_eq!(dialogues[0].karaoke_tokens[0].text, "Make");
        assert_eq!(dialogues[0].karaoke_tokens[0].start_ms, 1000);
        assert_eq!(dialogues[0].karaoke_tokens[0].end_ms, 1500);
    }
}
