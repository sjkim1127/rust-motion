//! Professional NLE Timeline Interchange Formats (CMX 3600 EDL, FCPXML, AAF).
//!
//! Provides import and export between Rust Motion's NLE timeline and industry-standard
//! post-production software: Adobe Premiere Pro, Final Cut Pro, DaVinci Resolve, and Avid Media Composer.

use crate::nle::{NleClip, NleTimeline, NleTrack, TrackKind};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use thiserror::Error;

/// Errors produced during interchange parsing or serialization.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InterchangeError {
    #[error("Invalid or unrecognized format: {0}")]
    InvalidFormat(String),
    #[error("Failed to parse timecode: {0}")]
    InvalidTimecode(String),
    #[error("XML parsing error: {0}")]
    XmlParseError(String),
    #[error("Empty or missing timeline data")]
    EmptyData,
}

/// Convert frames to SMPTE timecode `HH:MM:SS:FF`.
pub fn frames_to_smpte(frame: u32, fps: f64) -> String {
    let base = fps.round() as u32;
    let base = if base == 0 { 24 } else { base };
    let f = frame % base;
    let total_s = frame / base;
    let s = total_s % 60;
    let m = (total_s / 60) % 60;
    let h = total_s / 3600;
    format!("{h:02}:{m:02}:{s:02}:{f:02}")
}

/// Parse SMPTE timecode `HH:MM:SS:FF` (or with semicolons `HH;MM;SS;FF`) into frames.
pub fn smpte_to_frames(tc: &str, fps: f64) -> Result<u32, InterchangeError> {
    let parts: Vec<&str> = tc.split([':', ';', '.', ',']).collect();
    if parts.len() != 4 {
        return Err(InterchangeError::InvalidTimecode(tc.into()));
    }
    let h: u32 = parts[0].parse().map_err(|_| InterchangeError::InvalidTimecode(tc.into()))?;
    let m: u32 = parts[1].parse().map_err(|_| InterchangeError::InvalidTimecode(tc.into()))?;
    let s: u32 = parts[2].parse().map_err(|_| InterchangeError::InvalidTimecode(tc.into()))?;
    let f: u32 = parts[3].parse().map_err(|_| InterchangeError::InvalidTimecode(tc.into()))?;

    let base = fps.round() as u32;
    let base = if base == 0 { 24 } else { base };
    Ok(h * 3600 * base + m * 60 * base + s * base + f)
}

// -------------------------------------------------------------------------------------------------
// CMX 3600 EDL (Edit Decision List)
// -------------------------------------------------------------------------------------------------

/// Export a video track from `timeline` as a standard CMX 3600 EDL document.
pub fn export_cmx3600_edl(timeline: &NleTimeline, title: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "TITLE: {}", title.trim());
    let fcm = if (timeline.fps - 29.97).abs() < 0.01 || (timeline.fps - 59.94).abs() < 0.01 {
        "DROP FRAME"
    } else {
        "NON-DROP FRAME"
    };
    let _ = writeln!(out, "FCM: {fcm}\n");

    let mut event_num = 1;
    for track in &timeline.tracks {
        let channel_str = match track.kind {
            TrackKind::Video => "V",
            TrackKind::Audio => "A",
            _ => continue,
        };

        for clip in &track.clips {
            let reel = "AX";
            let trans = "C"; // Cut
            let src_in = frames_to_smpte(clip.source_in, timeline.fps);
            let src_out = frames_to_smpte(clip.source_out, timeline.fps);
            let rec_in = frames_to_smpte(clip.track_in, timeline.fps);
            let rec_out = frames_to_smpte(clip.track_out, timeline.fps);

            let _ = writeln!(
                out,
                "{event_num:03}  {reel:<8} {channel_str:<4} {trans:<4}        {src_in} {src_out} {rec_in} {rec_out}"
            );
            let _ = writeln!(out, "* FROM CLIP NAME: {}", clip.name);
            if let Some(ref asset) = clip.asset_id {
                let _ = writeln!(out, "* SOURCE FILE: {asset}");
            }
            let _ = writeln!(out);
            event_num += 1;
        }
    }
    out
}

/// Parse a CMX 3600 EDL string into an `NleTimeline`.
pub fn parse_cmx3600_edl(edl_text: &str, fps: f64) -> Result<NleTimeline, InterchangeError> {
    let mut title = "EDL Sequence".to_string();
    let mut video_track = NleTrack::new("v1", "Video 1", TrackKind::Video);
    let mut audio_track = NleTrack::new("a1", "Audio 1", TrackKind::Audio);

    let mut current_clip_name = String::new();
    let mut current_source_file = None;

    for line in edl_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("TITLE:") {
            title = rest.trim().to_string();
            continue;
        }
        if trimmed.starts_with("FCM:") {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("* FROM CLIP NAME:") {
            current_clip_name = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("* SOURCE FILE:") {
            current_source_file = Some(rest.trim().to_string());
            continue;
        }
        if trimmed.starts_with('*') {
            continue;
        }

        // Standard event line: "001  AX       V     C        00:00:00:00 00:00:05:00 00:00:00:00 00:00:05:00"
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 8 {
            let _event_id = parts[0];
            let _reel = parts[1];
            let chan = parts[2];
            let _trans = parts[3];

            // Source in, out, Rec in, out
            let src_in_str = parts[4];
            let src_out_str = parts[5];
            let rec_in_str = parts[6];
            let rec_out_str = parts[7];

            let src_in = smpte_to_frames(src_in_str, fps)?;
            let src_out = smpte_to_frames(src_out_str, fps)?;
            let rec_in = smpte_to_frames(rec_in_str, fps)?;
            let rec_out = smpte_to_frames(rec_out_str, fps)?;

            let name = if current_clip_name.is_empty() {
                format!("Clip_{}", parts[0])
            } else {
                std::mem::take(&mut current_clip_name)
            };

            let mut clip = NleClip::new(format!("clip_{}", parts[0]), name, rec_in, rec_out.saturating_sub(rec_in), src_in);
            clip.source_out = src_out;
            clip.asset_id = current_source_file.take();

            if chan.contains('V') {
                video_track.clips.push(clip.clone());
            }
            if chan.contains('A') {
                audio_track.clips.push(clip);
            }
        }
    }

    let mut timeline = NleTimeline::new(title, fps, 1920, 1080);
    if !video_track.clips.is_empty() {
        timeline.tracks.push(video_track);
    }
    if !audio_track.clips.is_empty() {
        timeline.tracks.push(audio_track);
    }
    timeline.update_duration();
    Ok(timeline)
}

// -------------------------------------------------------------------------------------------------
// Final Cut Pro FCPXML (1.9–1.11 dialect)
// -------------------------------------------------------------------------------------------------

/// Export timeline as standard FCPXML.
pub fn export_fcpxml(timeline: &NleTimeline) -> String {
    let mut out = String::new();
    let _ = writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(out, r#"<!DOCTYPE fcpxml>"#);
    let _ = writeln!(out, r#"<fcpxml version="1.10">"#);
    let _ = writeln!(out, r#"    <resources>"#);
    let frame_dur = if (timeline.fps - 23.976).abs() < 0.01 {
        "1001/24000s"
    } else if (timeline.fps - 29.97).abs() < 0.01 {
        "1001/30000s"
    } else {
        "1/30s"
    };
    let _ = writeln!(
        out,
        r#"        <format id="r1" name="FFVideoFormat{}p{}" frameDuration="{}" width="{}" height="{}"/>"#,
        timeline.height, timeline.fps.round() as u32, frame_dur, timeline.width, timeline.height
    );
    let _ = writeln!(out, r#"    </resources>"#);
    let _ = writeln!(out, r#"    <library>"#);
    let _ = writeln!(out, r#"        <event name="Rust Motion Event">"#);
    let _ = writeln!(
        out,
        r#"            <project name="{}">"#,
        timeline.title
    );
    let _ = writeln!(
        out,
        r#"                <sequence format="r1" duration="{}/{}s">"#,
        timeline.duration_frames, timeline.fps.round() as u32
    );
    let _ = writeln!(out, r#"                    <spine>"#);

    for track in &timeline.tracks {
        if track.kind == TrackKind::Video {
            for clip in &track.clips {
                let dur = clip.duration();
                let _ = writeln!(
                    out,
                    r#"                        <asset-clip name="{}" offset="{}/{}s" duration="{}/{}s" start="{}/{}s"/>"#,
                    clip.name,
                    clip.track_in,
                    timeline.fps.round() as u32,
                    dur,
                    timeline.fps.round() as u32,
                    clip.source_in,
                    timeline.fps.round() as u32
                );
            }
        }
    }

    let _ = writeln!(out, r#"                    </spine>"#);
    let _ = writeln!(out, r#"                </sequence>"#);
    let _ = writeln!(out, r#"            </project>"#);
    let _ = writeln!(out, r#"        </event>"#);
    let _ = writeln!(out, r#"    </library>"#);
    let _ = writeln!(out, r#"</fcpxml>"#);
    out
}

/// Parse FCPXML into an `NleTimeline`.
pub fn parse_fcpxml(xml_text: &str) -> Result<NleTimeline, InterchangeError> {
    if !xml_text.contains("<fcpxml") {
        return Err(InterchangeError::InvalidFormat("Missing <fcpxml root tag".into()));
    }

    let mut title = "FCPXML Sequence".to_string();
    if let Some(pos) = xml_text.find(r#"<project name=""#) {
        let rest = &xml_text[pos + 15..];
        if let Some(end) = rest.find('"') {
            title = rest[..end].to_string();
        }
    }

    let mut track = NleTrack::new("v1", "Video 1", TrackKind::Video);
    let mut cursor = 0;

    // Scan for <asset-clip or <clip tags
    while let Some(tag_pos) = xml_text[cursor..].find("<asset-clip") {
        let abs_pos = cursor + tag_pos;
        let rest = &xml_text[abs_pos..];
        let tag_end = rest.find('>').ok_or_else(|| InterchangeError::XmlParseError("Unclosed <asset-clip>".into()))?;
        let tag_str = &rest[..tag_end];

        let name = extract_attr(tag_str, "name").unwrap_or_else(|| "Untitled".to_string());
        let offset = extract_rational_seconds(tag_str, "offset").unwrap_or(0.0);
        let duration = extract_rational_seconds(tag_str, "duration").unwrap_or(5.0);
        let start = extract_rational_seconds(tag_str, "start").unwrap_or(0.0);

        let fps = 30.0;
        let track_in = (offset * fps).round() as u32;
        let dur_frames = (duration * fps).round() as u32;
        let src_in = (start * fps).round() as u32;

        let clip = NleClip::new(format!("clip_{}", track.clips.len() + 1), name, track_in, dur_frames, src_in);
        track.clips.push(clip);
        cursor = abs_pos + tag_end + 1;
    }

    let mut timeline = NleTimeline::new(title, 30.0, 1920, 1080);
    timeline.tracks.push(track);
    timeline.update_duration();
    Ok(timeline)
}

fn extract_attr(tag_str: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr}=\"");
    let pos = tag_str.find(&pattern)?;
    let rest = &tag_str[pos + pattern.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_rational_seconds(tag_str: &str, attr: &str) -> Option<f64> {
    let val = extract_attr(tag_str, attr)?;
    let val = val.trim_end_matches('s');
    if let Some(slash) = val.find('/') {
        let num: f64 = val[..slash].parse().ok()?;
        let den: f64 = val[slash + 1..].parse().ok()?;
        if den != 0.0 {
            Some(num / den)
        } else {
            None
        }
    } else {
        val.parse().ok()
    }
}

// -------------------------------------------------------------------------------------------------
// AAF Metadata Summary Descriptor
// -------------------------------------------------------------------------------------------------

/// AAF Project summary and composition mob structure for Avid / Premiere interchange.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AafMetadata {
    pub name: String,
    pub edit_rate: (u32, u32),
    pub duration_frames: u32,
    pub video_tracks_count: usize,
    pub audio_tracks_count: usize,
    pub total_clips: usize,
}

/// Generate AAF metadata descriptor for a timeline.
pub fn export_aaf_metadata(timeline: &NleTimeline) -> AafMetadata {
    let video_tracks_count = timeline.tracks.iter().filter(|t| t.kind == TrackKind::Video).count();
    let audio_tracks_count = timeline.tracks.iter().filter(|t| t.kind == TrackKind::Audio).count();
    let total_clips = timeline.tracks.iter().map(|t| t.clips.len()).sum();

    AafMetadata {
        name: timeline.title.clone(),
        edit_rate: ((timeline.fps * 1000.0).round() as u32, 1000),
        duration_frames: timeline.duration_frames,
        video_tracks_count,
        audio_tracks_count,
        total_clips,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smpte_conversions() {
        assert_eq!(frames_to_smpte(0, 24.0), "00:00:00:00");
        assert_eq!(frames_to_smpte(24, 24.0), "00:00:01:00");
        assert_eq!(frames_to_smpte(1440, 24.0), "00:01:00:00");
        assert_eq!(smpte_to_frames("00:01:00:00", 24.0).unwrap(), 1440);
        assert_eq!(smpte_to_frames("01:00:00:00", 30.0).unwrap(), 108000);
    }

    #[test]
    fn test_cmx3600_edl_roundtrip() {
        let mut timeline = NleTimeline::new("My Film", 24.0, 1920, 1080);
        let mut v_track = NleTrack::new("v1", "Video", TrackKind::Video);
        v_track.clips.push(NleClip::new("c1", "Scene1", 0, 72, 0));
        v_track.clips.push(NleClip::new("c2", "Scene2", 72, 48, 100));
        timeline.tracks.push(v_track);
        timeline.update_duration();

        let edl_str = export_cmx3600_edl(&timeline, "My Film");
        assert!(edl_str.contains("TITLE: My Film"));
        assert!(edl_str.contains("001  AX       V    C"));
        assert!(edl_str.contains("002  AX       V    C"));

        let parsed = parse_cmx3600_edl(&edl_str, 24.0).unwrap();
        assert_eq!(parsed.title, "My Film");
        assert_eq!(parsed.tracks.len(), 1);
        assert_eq!(parsed.tracks[0].clips.len(), 2);
        assert_eq!(parsed.tracks[0].clips[0].track_in, 0);
        assert_eq!(parsed.tracks[0].clips[0].track_out, 72);
        assert_eq!(parsed.tracks[0].clips[1].track_in, 72);
        assert_eq!(parsed.tracks[0].clips[1].track_out, 120);
    }

    #[test]
    fn test_fcpxml_roundtrip() {
        let mut timeline = NleTimeline::new("Commercial Project", 30.0, 1920, 1080);
        let mut v_track = NleTrack::new("v1", "Video", TrackKind::Video);
        v_track.clips.push(NleClip::new("c1", "Intro", 0, 60, 0));
        v_track.clips.push(NleClip::new("c2", "Product", 60, 90, 0));
        timeline.tracks.push(v_track);
        timeline.update_duration();

        let xml_str = export_fcpxml(&timeline);
        assert!(xml_str.contains("<fcpxml"));
        assert!(xml_str.contains("Commercial Project"));

        let parsed = parse_fcpxml(&xml_str).unwrap();
        assert_eq!(parsed.title, "Commercial Project");
        assert_eq!(parsed.tracks[0].clips.len(), 2);
        assert_eq!(parsed.tracks[0].clips[0].name, "Intro");
        assert_eq!(parsed.tracks[0].clips[1].name, "Product");
    }

    #[test]
    fn test_aaf_metadata_descriptor() {
        let mut timeline = NleTimeline::new("AAF Movie", 25.0, 3840, 2160);
        let mut v_track = NleTrack::new("v1", "V1", TrackKind::Video);
        v_track.clips.push(NleClip::new("c1", "Shot", 0, 100, 0));
        let mut a_track = NleTrack::new("a1", "A1", TrackKind::Audio);
        a_track.clips.push(NleClip::new("c2", "Audio", 0, 100, 0));
        timeline.tracks.push(v_track);
        timeline.tracks.push(a_track);
        timeline.update_duration();

        let aaf = export_aaf_metadata(&timeline);
        assert_eq!(aaf.name, "AAF Movie");
        assert_eq!(aaf.video_tracks_count, 1);
        assert_eq!(aaf.audio_tracks_count, 1);
        assert_eq!(aaf.total_clips, 2);
        assert_eq!(aaf.duration_frames, 100);
    }
}
