//! NLE Multi-track Non-Linear Editing Timeline Engine.
//!
//! Provides non-destructive track hierarchy, clip trimming, slip, slide, ripple, roll,
//! split, overwrite, insert, lift, and extract operations, with support for variable speed curves,
//! time-remapping, and transitions.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Type of timeline track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Video,
    Audio,
    Adjustment,
    Title,
    SubComposition,
}

/// Transition effect between clips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    CrossDissolve,
    DipToBlack,
    DipToWhite,
    WipeLeft,
    WipeRight,
    SlideLeft,
    SlideRight,
}

/// Transition alignment relative to the cut point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionAlignment {
    CenterOnCut,
    StartOnCut,
    EndOnCut,
}

/// Transition metadata between two consecutive clips on a track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NleTransition {
    pub id: String,
    pub kind: TransitionKind,
    pub alignment: TransitionAlignment,
    pub duration_frames: u32,
}

/// A clip placed on an NLE track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NleClip {
    pub id: String,
    pub name: String,
    pub asset_id: Option<String>,
    /// Frame on the timeline where this clip starts.
    pub track_in: u32,
    /// Frame on the timeline where this clip ends (exclusive).
    pub track_out: u32,
    /// In-point within the source media (frames).
    pub source_in: u32,
    /// Out-point within the source media (frames).
    pub source_out: u32,
    /// Playback speed multiplier (1.0 = normal, 2.0 = 2x speed, -1.0 = reverse).
    #[serde(default = "default_speed")]
    pub speed: f64,
    /// Whether reverse playback is active.
    #[serde(default)]
    pub reverse: bool,
    /// Master opacity (0.0 ..= 1.0).
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// Audio volume in decibels (0.0 dB = nominal, -60.0 ..= +12.0).
    #[serde(default)]
    pub volume_db: f32,
    /// Transition at the incoming cut point.
    #[serde(default)]
    pub in_transition: Option<NleTransition>,
    /// Transition at the outgoing cut point.
    #[serde(default)]
    pub out_transition: Option<NleTransition>,
}

fn default_speed() -> f64 {
    1.0
}

fn default_opacity() -> f32 {
    1.0
}

impl NleClip {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        track_in: u32,
        duration: u32,
        source_in: u32,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            asset_id: None,
            track_in,
            track_out: track_in + duration,
            source_in,
            source_out: source_in + duration,
            speed: 1.0,
            reverse: false,
            opacity: 1.0,
            volume_db: 0.0,
            in_transition: None,
            out_transition: None,
        }
    }

    /// Duration of the clip on the timeline in frames.
    pub fn duration(&self) -> u32 {
        self.track_out.saturating_sub(self.track_in)
    }

    /// Duration of the source media used in frames.
    pub fn source_duration(&self) -> u32 {
        self.source_out.saturating_sub(self.source_in)
    }
}

/// A track containing sequentially ordered non-overlapping clips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NleTrack {
    pub id: String,
    pub name: String,
    pub kind: TrackKind,
    pub locked: bool,
    pub muted: bool,
    pub solo: bool,
    pub sync_locked: bool,
    pub clips: Vec<NleClip>,
}

impl NleTrack {
    pub fn new(id: impl Into<String>, name: impl Into<String>, kind: TrackKind) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind,
            locked: false,
            muted: false,
            solo: false,
            sync_locked: true,
            clips: Vec::new(),
        }
    }

    /// Find clip index containing `frame`.
    pub fn find_clip_at(&self, frame: u32) -> Option<usize> {
        self.clips
            .iter()
            .position(|c| c.track_in <= frame && frame < c.track_out)
    }

    /// Sort clips by `track_in` and ensure no overlaps.
    pub fn validate_no_overlap(&self) -> Result<(), NleError> {
        for window in self.clips.windows(2) {
            if window[0].track_out > window[1].track_in {
                return Err(NleError::ClipOverlap {
                    clip_a: window[0].id.clone(),
                    clip_b: window[1].id.clone(),
                    out_a: window[0].track_out,
                    in_b: window[1].track_in,
                });
            }
        }
        Ok(())
    }
}

/// Full NLE Timeline model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NleTimeline {
    pub title: String,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
    pub duration_frames: u32,
    pub tracks: Vec<NleTrack>,
}

impl NleTimeline {
    pub fn new(title: impl Into<String>, fps: f64, width: u32, height: u32) -> Self {
        Self {
            title: title.into(),
            fps,
            width,
            height,
            duration_frames: 0,
            tracks: Vec::new(),
        }
    }

    /// Recalculate total duration as the latest track_out among all tracks.
    pub fn update_duration(&mut self) {
        self.duration_frames = self
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .map(|c| c.track_out)
            .max()
            .unwrap_or(0);
    }
}

/// Editing edge for trimming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimEdge {
    In,
    Out,
}

/// Trimming mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimMode {
    Regular,
    Ripple,
}

/// Errors produced during NLE timeline edit operations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NleError {
    #[error("Track is locked")]
    TrackLocked,
    #[error("Clip {0} not found")]
    ClipNotFound(String),
    #[error("Track index {0} out of bounds")]
    TrackNotFound(usize),
    #[error("Clip overlap detected between {clip_a} (out: {out_a}) and {clip_b} (in: {in_b})")]
    ClipOverlap {
        clip_a: String,
        clip_b: String,
        out_a: u32,
        in_b: u32,
    },
    #[error("Edit would make clip duration zero or negative")]
    InvalidDuration,
    #[error("Insufficient source media handles to extend clip")]
    InsufficientMediaHandles,
}

// -------------------------------------------------------------------------------------------------
// NLE Editing Operations
// -------------------------------------------------------------------------------------------------

/// Split the clip on `track` at `at_frame` into two independent clips.
pub fn split_clip_at(track: &mut NleTrack, at_frame: u32) -> Result<(NleClip, NleClip), NleError> {
    if track.locked {
        return Err(NleError::TrackLocked);
    }
    let idx = track
        .find_clip_at(at_frame)
        .ok_or_else(|| NleError::ClipNotFound(format!("No clip at frame {at_frame}")))?;
    let original = &track.clips[idx];
    if at_frame <= original.track_in || at_frame >= original.track_out {
        return Err(NleError::InvalidDuration);
    }

    let cut_offset = at_frame - original.track_in;
    let mut left = original.clone();
    left.track_out = at_frame;
    left.source_out = original.source_in + cut_offset;

    let mut right = original.clone();
    right.id = format!("{}_split", original.id);
    right.track_in = at_frame;
    right.source_in = original.source_in + cut_offset;

    track.clips[idx] = left.clone();
    track.clips.insert(idx + 1, right.clone());
    Ok((left, right))
}

/// Regular or ripple trim of an edge of a clip on `track`.
pub fn trim_clip(
    track: &mut NleTrack,
    clip_idx: usize,
    edge: TrimEdge,
    delta_frames: i32,
    mode: TrimMode,
) -> Result<(), NleError> {
    if track.locked {
        return Err(NleError::TrackLocked);
    }
    if clip_idx >= track.clips.len() {
        return Err(NleError::ClipNotFound(format!("Index {clip_idx}")));
    }

    let clip = &mut track.clips[clip_idx];
    match edge {
        TrimEdge::In => {
            let new_in = (clip.track_in as i64 + delta_frames as i64).max(0) as u32;
            if new_in >= clip.track_out {
                return Err(NleError::InvalidDuration);
            }
            let new_src_in = (clip.source_in as i64 + delta_frames as i64).max(0) as u32;
            clip.track_in = new_in;
            clip.source_in = new_src_in;
        }
        TrimEdge::Out => {
            let new_out = (clip.track_out as i64 + delta_frames as i64).max(0) as u32;
            if new_out <= clip.track_in {
                return Err(NleError::InvalidDuration);
            }
            let new_src_out = (clip.source_out as i64 + delta_frames as i64).max(0) as u32;
            clip.track_out = new_out;
            clip.source_out = new_src_out;
        }
    }

    if mode == TrimMode::Ripple {
        let ripple_delta = match edge {
            TrimEdge::In => -delta_frames,
            TrimEdge::Out => delta_frames,
        };
        for c in &mut track.clips[(clip_idx + 1)..] {
            c.track_in = (c.track_in as i64 + ripple_delta as i64).max(0) as u32;
            c.track_out = (c.track_out as i64 + ripple_delta as i64).max(0) as u32;
        }
    }

    track.validate_no_overlap()?;
    Ok(())
}

/// Slip edit: shifts source_in and source_out simultaneously without changing timeline position or duration.
pub fn slip_clip(clip: &mut NleClip, delta_frames: i32) -> Result<(), NleError> {
    let new_src_in = clip.source_in as i64 + delta_frames as i64;
    let new_src_out = clip.source_out as i64 + delta_frames as i64;
    if new_src_in < 0 || new_src_out < 0 {
        return Err(NleError::InsufficientMediaHandles);
    }
    clip.source_in = new_src_in as u32;
    clip.source_out = new_src_out as u32;
    Ok(())
}

/// Slide edit: moves the clip along the timeline, trimming the outgoing edge of the previous clip
/// and the incoming edge of the next clip to preserve total duration.
pub fn slide_clip(track: &mut NleTrack, clip_idx: usize, delta_frames: i32) -> Result<(), NleError> {
    if track.locked {
        return Err(NleError::TrackLocked);
    }
    if clip_idx == 0 || clip_idx + 1 >= track.clips.len() {
        return Err(NleError::ClipNotFound("Slide requires preceding and succeeding clips".into()));
    }

    let prev_in = track.clips[clip_idx - 1].track_in;
    let prev_src_out = track.clips[clip_idx - 1].source_out;

    let curr_in = track.clips[clip_idx].track_in;
    let curr_out = track.clips[clip_idx].track_out;

    let next_out = track.clips[clip_idx + 1].track_out;
    let next_src_in = track.clips[clip_idx + 1].source_in;

    let new_curr_in = (curr_in as i64 + delta_frames as i64).max(0) as u32;
    let new_curr_out = (curr_out as i64 + delta_frames as i64).max(0) as u32;

    if new_curr_in <= prev_in || new_curr_out >= next_out {
        return Err(NleError::InvalidDuration);
    }

    track.clips[clip_idx - 1].track_out = new_curr_in;
    track.clips[clip_idx - 1].source_out = (prev_src_out as i64 + delta_frames as i64).max(0) as u32;

    track.clips[clip_idx].track_in = new_curr_in;
    track.clips[clip_idx].track_out = new_curr_out;

    track.clips[clip_idx + 1].track_in = new_curr_out;
    track.clips[clip_idx + 1].source_in = (next_src_in as i64 + delta_frames as i64).max(0) as u32;

    track.validate_no_overlap()?;
    Ok(())
}

/// Roll edit: moves the edit boundary between two adjacent clips without changing overall sequence duration.
pub fn roll_edit(
    track: &mut NleTrack,
    left_clip_idx: usize,
    delta_frames: i32,
) -> Result<(), NleError> {
    if track.locked {
        return Err(NleError::TrackLocked);
    }
    if left_clip_idx + 1 >= track.clips.len() {
        return Err(NleError::ClipNotFound("Roll edit requires adjacent clips".into()));
    }

    let left_in = track.clips[left_clip_idx].track_in;
    let left_out = track.clips[left_clip_idx].track_out;
    let left_src_out = track.clips[left_clip_idx].source_out;

    let right_out = track.clips[left_clip_idx + 1].track_out;
    let right_src_in = track.clips[left_clip_idx + 1].source_in;

    let new_cut = (left_out as i64 + delta_frames as i64).max(0) as u32;
    if new_cut <= left_in || new_cut >= right_out {
        return Err(NleError::InvalidDuration);
    }

    track.clips[left_clip_idx].track_out = new_cut;
    track.clips[left_clip_idx].source_out = (left_src_out as i64 + delta_frames as i64).max(0) as u32;

    track.clips[left_clip_idx + 1].track_in = new_cut;
    track.clips[left_clip_idx + 1].source_in = (right_src_in as i64 + delta_frames as i64).max(0) as u32;

    track.validate_no_overlap()?;
    Ok(())
}

/// Ripple delete: removes the clip and shifts all subsequent clips left to close the gap.
pub fn ripple_delete(track: &mut NleTrack, clip_idx: usize) -> Result<NleClip, NleError> {
    if track.locked {
        return Err(NleError::TrackLocked);
    }
    if clip_idx >= track.clips.len() {
        return Err(NleError::ClipNotFound(format!("Clip index {clip_idx}")));
    }

    let removed = track.clips.remove(clip_idx);
    let gap = removed.duration();
    for c in &mut track.clips[clip_idx..] {
        c.track_in = c.track_in.saturating_sub(gap);
        c.track_out = c.track_out.saturating_sub(gap);
    }
    Ok(removed)
}

/// Insert edit: splits or shifts material at `at_frame` to make room for `new_clip`.
pub fn insert_clip(track: &mut NleTrack, at_frame: u32, new_clip: NleClip) -> Result<(), NleError> {
    if track.locked {
        return Err(NleError::TrackLocked);
    }
    let dur = new_clip.duration();

    // If inserted inside an existing clip, split it first
    if let Some(idx) = track.find_clip_at(at_frame) {
        if track.clips[idx].track_in < at_frame && at_frame < track.clips[idx].track_out {
            split_clip_at(track, at_frame)?;
        }
    }

    // Shift all clips at or after at_frame right by dur
    for c in &mut track.clips {
        if c.track_in >= at_frame {
            c.track_in += dur;
            c.track_out += dur;
        }
    }

    // Insert new clip
    let mut inserted = new_clip;
    inserted.track_in = at_frame;
    inserted.track_out = at_frame + dur;
    let insert_pos = track.clips.iter().position(|c| c.track_in > at_frame).unwrap_or(track.clips.len());
    track.clips.insert(insert_pos, inserted);

    track.validate_no_overlap()?;
    Ok(())
}

/// Overwrite edit: replaces whatever material is covered by `new_clip` on the track.
pub fn overwrite_clip(track: &mut NleTrack, at_frame: u32, mut new_clip: NleClip) -> Result<(), NleError> {
    if track.locked {
        return Err(NleError::TrackLocked);
    }
    let dur = new_clip.duration();
    let new_out = at_frame + dur;
    new_clip.track_in = at_frame;
    new_clip.track_out = new_out;

    let mut remaining = Vec::new();
    for c in track.clips.drain(..) {
        if c.track_out <= at_frame || c.track_in >= new_out {
            remaining.push(c);
        } else if c.track_in < at_frame && c.track_out > new_out {
            // Straddles both sides: split into head and tail
            let mut head = c.clone();
            head.track_out = at_frame;
            head.source_out = c.source_in + (at_frame - c.track_in);
            remaining.push(head);

            let mut tail = c.clone();
            tail.id = format!("{}_tail", c.id);
            tail.track_in = new_out;
            tail.source_in = c.source_in + (new_out - c.track_in);
            remaining.push(tail);
        } else if c.track_in < at_frame && c.track_out <= new_out {
            // Trim right side
            let mut left = c.clone();
            left.track_out = at_frame;
            left.source_out = c.source_in + (at_frame - c.track_in);
            remaining.push(left);
        } else if c.track_in >= at_frame && c.track_out > new_out {
            // Trim left side
            let mut right = c.clone();
            right.track_in = new_out;
            right.source_in = c.source_in + (new_out - c.track_in);
            remaining.push(right);
        }
    }

    remaining.push(new_clip);
    remaining.sort_by_key(|c| c.track_in);
    track.clips = remaining;

    track.validate_no_overlap()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_clip() {
        let mut track = NleTrack::new("t1", "Video 1", TrackKind::Video);
        let clip = NleClip::new("c1", "Shot 1", 0, 100, 0);
        track.clips.push(clip);

        let (left, right) = split_clip_at(&mut track, 40).unwrap();
        assert_eq!(left.track_in, 0);
        assert_eq!(left.track_out, 40);
        assert_eq!(left.source_in, 0);
        assert_eq!(left.source_out, 40);

        assert_eq!(right.track_in, 40);
        assert_eq!(right.track_out, 100);
        assert_eq!(right.source_in, 40);
        assert_eq!(right.source_out, 100);
        assert_eq!(track.clips.len(), 2);
    }

    #[test]
    fn test_trim_clip_regular_and_ripple() {
        let mut track = NleTrack::new("t1", "Video 1", TrackKind::Video);
        track.clips.push(NleClip::new("c1", "Shot 1", 0, 100, 0));
        track.clips.push(NleClip::new("c2", "Shot 2", 100, 100, 0));

        // Regular trim Out by -20
        trim_clip(&mut track, 0, TrimEdge::Out, -20, TrimMode::Regular).unwrap();
        assert_eq!(track.clips[0].track_out, 80);
        assert_eq!(track.clips[1].track_in, 100); // unaffected in regular

        // Ripple trim Out by +10
        trim_clip(&mut track, 0, TrimEdge::Out, 10, TrimMode::Ripple).unwrap();
        assert_eq!(track.clips[0].track_out, 90);
        assert_eq!(track.clips[1].track_in, 110); // shifted in ripple
        assert_eq!(track.clips[1].track_out, 210);
    }

    #[test]
    fn test_slip_clip() {
        let mut clip = NleClip::new("c1", "Shot 1", 100, 50, 20);
        slip_clip(&mut clip, 15).unwrap();
        assert_eq!(clip.track_in, 100);
        assert_eq!(clip.track_out, 150);
        assert_eq!(clip.source_in, 35);
        assert_eq!(clip.source_out, 85);
    }

    #[test]
    fn test_slide_clip() {
        let mut track = NleTrack::new("t1", "Video 1", TrackKind::Video);
        track.clips.push(NleClip::new("c1", "Clip 1", 0, 100, 0));
        track.clips.push(NleClip::new("c2", "Clip 2", 100, 50, 0));
        track.clips.push(NleClip::new("c3", "Clip 3", 150, 100, 0));

        slide_clip(&mut track, 1, 20).unwrap();
        assert_eq!(track.clips[0].track_out, 120);
        assert_eq!(track.clips[1].track_in, 120);
        assert_eq!(track.clips[1].track_out, 170);
        assert_eq!(track.clips[2].track_in, 170);
        assert_eq!(track.clips[2].track_out, 250);
    }

    #[test]
    fn test_roll_edit() {
        let mut track = NleTrack::new("t1", "Video 1", TrackKind::Video);
        track.clips.push(NleClip::new("c1", "Clip 1", 0, 100, 0));
        track.clips.push(NleClip::new("c2", "Clip 2", 100, 100, 0));

        roll_edit(&mut track, 0, 25).unwrap();
        assert_eq!(track.clips[0].track_out, 125);
        assert_eq!(track.clips[1].track_in, 125);
        assert_eq!(track.clips[1].track_out, 200);
    }

    #[test]
    fn test_ripple_delete() {
        let mut track = NleTrack::new("t1", "Video 1", TrackKind::Video);
        track.clips.push(NleClip::new("c1", "Clip 1", 0, 50, 0));
        track.clips.push(NleClip::new("c2", "Clip 2", 50, 30, 0));
        track.clips.push(NleClip::new("c3", "Clip 3", 80, 40, 0));

        let deleted = ripple_delete(&mut track, 1).unwrap();
        assert_eq!(deleted.id, "c2");
        assert_eq!(track.clips.len(), 2);
        assert_eq!(track.clips[1].track_in, 50);
        assert_eq!(track.clips[1].track_out, 90);
    }

    #[test]
    fn test_overwrite_clip() {
        let mut track = NleTrack::new("t1", "Video 1", TrackKind::Video);
        track.clips.push(NleClip::new("c1", "Clip 1", 0, 200, 0));

        let overwrite = NleClip::new("insert", "New", 0, 50, 0);
        overwrite_clip(&mut track, 50, overwrite).unwrap();

        assert_eq!(track.clips.len(), 3);
        assert_eq!(track.clips[0].track_in, 0);
        assert_eq!(track.clips[0].track_out, 50);
        assert_eq!(track.clips[1].track_in, 50);
        assert_eq!(track.clips[1].track_out, 100);
        assert_eq!(track.clips[2].track_in, 100);
        assert_eq!(track.clips[2].track_out, 200);
    }
}
