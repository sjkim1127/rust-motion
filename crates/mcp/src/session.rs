//! MCP editing session state: project, timeline tracks, clips, and the schematic preview.
//!
//! The NLE timeline is the canonical editing representation. The exportable [`Project`] is derived
//! from it after every edit, and an edit is committed only when the derived project validates, so
//! a rejected request changes neither representation.

use rust_motion_project::nle::{
    insert_clip, split_clip_at_index, trim_clip, NleClip, NleTimeline, NleTrack, TrackKind,
    TrimEdge, TrimMode,
};
use rust_motion_project::{BackendKind, Clip, Project, ProjectSettings, Track};
use rust_motion_rasterizer::backend::{FrameConfig, RasterizerBackend};
use rust_motion_rasterizer::lut3d::Lut3d;
use rust_motion_rasterizer::scene::{Color, Scene, SceneFilter, SceneNode};
use rust_motion_rasterizer::TinySkiaBackend;
use serde::{Deserialize, Serialize};

/// Largest frame index the session accepts, so that frame deltas always fit in `i32`.
pub const MAX_TIMELINE_FRAMES: u32 = i32::MAX as u32;
/// Highest accepted timeline frame rate.
pub const MAX_FPS: f64 = 1000.0;
/// Largest accepted project canvas width or height.
pub const MAX_CANVAS_SIDE: u32 = 16_384;
/// Largest accepted preview width or height.
pub const MAX_PREVIEW_SIDE: u32 = 4096;

/// What the preview does not show. Reported with every preview so an agent cannot mistake it for
/// the rendered composition.
pub const PREVIEW_LIMITATIONS: &[&str] = &[
    "Each visible clip is drawn as a colored block by track type; source media is not decoded",
    "Captions are drawn as plain text without animation",
    "Exposure is applied as a 2^EV multiplier on encoded values, not in scene-linear light",
    "Transitions, speed changes and audio are not represented",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorGradeParams {
    pub exposure: Option<f32>,
    pub contrast: Option<f32>,
    pub saturation: Option<f32>,
    pub gamma: Option<f32>,
    pub lut_cube: Option<String>,
}

impl ColorGradeParams {
    /// Reject values outside the ranges the tool schema documents, and unreadable LUTs.
    pub fn validate(&self) -> Result<(), String> {
        let ranges = [
            ("exposure", self.exposure, -5.0, 5.0),
            ("contrast", self.contrast, 0.5, 2.0),
            ("saturation", self.saturation, 0.0, 2.0),
            ("gamma", self.gamma, 0.5, 2.0),
        ];
        for (field, value, min, max) in ranges {
            if let Some(value) = value {
                if !value.is_finite() || value < min || value > max {
                    return Err(format!(
                        "invalid '{field}': {value} is outside {min}..={max}"
                    ));
                }
            }
        }
        if let Some(cube) = &self.lut_cube {
            Lut3d::parse_cube(cube).map_err(|error| {
                format!("invalid 'lut_cube': expected .cube file content ({error})")
            })?;
        }
        Ok(())
    }
}

pub struct McpSession {
    pub project: Option<Project>,
    pub timeline: NleTimeline,
    pub backend: TinySkiaBackend,
    pub active_grades: Vec<(String, ColorGradeParams)>,
    /// Caption text by clip ID.
    pub captions: Vec<(String, String)>,
    /// Requested composition length; edits can extend the timeline past it but not shorten it.
    base_duration_frames: u32,
}

/// A copy of everything an edit may change.
struct Draft {
    timeline: NleTimeline,
    grades: Vec<(String, ColorGradeParams)>,
    captions: Vec<(String, String)>,
}

impl Draft {
    /// Track and clip index of a clip ID.
    fn locate(&self, clip_id: &str) -> Result<(usize, usize), String> {
        self.timeline
            .tracks
            .iter()
            .enumerate()
            .find_map(|(track, t)| {
                t.clips
                    .iter()
                    .position(|clip| clip.id == clip_id)
                    .map(|clip| (track, clip))
            })
            .ok_or_else(|| format!("Clip '{clip_id}' not found"))
    }

    fn count_id(&self, clip_id: &str) -> usize {
        self.timeline
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .filter(|clip| clip.id == clip_id)
            .count()
    }

    /// `base`, or `base_2`, `base_3`, ... so that no clip on any track already has it.
    fn unused_clip_id(&self, base: &str) -> String {
        let mut candidate = base.to_string();
        let mut suffix = 2usize;
        while self.count_id(&candidate) > 0 {
            candidate = format!("{base}_{suffix}");
            suffix += 1;
        }
        candidate
    }

    /// Finish a split: give the right-hand part a timeline-unique ID and the original's metadata.
    fn adopt_split_part(&mut self, track: usize, original_id: &str, part_id: String) -> String {
        let unique = if self.count_id(&part_id) > 1 {
            let unique = self.unused_clip_id(&part_id);
            // Split parts follow their original, so the last match on this track is the new one.
            if let Some(clip) = self.timeline.tracks[track]
                .clips
                .iter_mut()
                .rev()
                .find(|clip| clip.id == part_id)
            {
                clip.id.clone_from(&unique);
            }
            unique
        } else {
            part_id
        };
        if let Some((_, grade)) = self.grades.iter().find(|(id, _)| id == original_id) {
            let grade = grade.clone();
            self.grades.push((unique.clone(), grade));
        }
        if let Some((_, text)) = self.captions.iter().find(|(id, _)| id == original_id) {
            let text = text.clone();
            self.captions.push((unique.clone(), text));
        }
        unique
    }
}

/// Convert a non-negative time in seconds to a frame index without silently clamping.
pub fn secs_to_frame(field: &str, secs: f64, fps: f64) -> Result<u32, String> {
    if !secs.is_finite() || secs < 0.0 {
        return Err(format!(
            "invalid '{field}': expected a finite time of zero or more seconds, got {secs}"
        ));
    }
    let frame = (secs * fps).round();
    if !frame.is_finite() || frame > f64::from(MAX_TIMELINE_FRAMES) {
        return Err(format!(
            "invalid '{field}': {secs}s at {fps}fps exceeds the limit of {MAX_TIMELINE_FRAMES} frames"
        ));
    }
    // In range by the checks above.
    Ok(frame as u32)
}

/// Like [`secs_to_frame`] for a length, which must cover at least one frame.
fn duration_to_frames(field: &str, secs: f64, fps: f64) -> Result<u32, String> {
    match secs_to_frame(field, secs, fps)? {
        0 => Err(format!(
            "invalid '{field}': {secs}s is shorter than one frame at {fps}fps"
        )),
        frames => Ok(frames),
    }
}

fn require_text(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("invalid '{field}': must not be empty"));
    }
    Ok(())
}

fn require_side(field: &str, value: u32, max: u32) -> Result<(), String> {
    if value == 0 || value > max {
        return Err(format!(
            "invalid '{field}': {value} is outside 1..={max} pixels"
        ));
    }
    Ok(())
}

/// Derive the exportable project from the edited timeline.
///
/// The project schema has no fields for source ranges, asset links, captions or grades, so they are
/// carried in each clip's `props`.
fn export_project(base: &Project, draft: &Draft) -> Result<Project, String> {
    let mut project = base.clone();
    project.settings.duration = draft.timeline.duration_frames;
    project.tracks = draft
        .timeline
        .tracks
        .iter()
        .map(|track| Track {
            id: track.id.clone(),
            clips: track
                .clips
                .iter()
                .map(|clip| Clip {
                    id: clip.id.clone(),
                    composition: clip.name.clone(),
                    start: clip.track_in,
                    duration: clip.duration(),
                    props: clip_props(clip, draft),
                })
                .collect(),
        })
        .collect();
    project
        .validate()
        .map_err(|error| format!("edit would produce an invalid project: {error}"))?;
    Ok(project)
}

fn clip_props(clip: &NleClip, draft: &Draft) -> serde_json::Value {
    let mut props = serde_json::Map::new();
    props.insert("source_in".into(), clip.source_in.into());
    props.insert("source_out".into(), clip.source_out.into());
    if let Some(asset_id) = &clip.asset_id {
        props.insert("asset_id".into(), asset_id.clone().into());
    }
    if let Some((_, text)) = draft.captions.iter().find(|(id, _)| *id == clip.id) {
        props.insert("caption".into(), text.clone().into());
    }
    if let Some((_, grade)) = draft.grades.iter().find(|(id, _)| *id == clip.id) {
        if let Ok(grade) = serde_json::to_value(grade) {
            props.insert("color_grade".into(), grade);
        }
    }
    serde_json::Value::Object(props)
}

impl Default for McpSession {
    fn default() -> Self {
        Self::new()
    }
}

impl McpSession {
    pub fn new() -> Self {
        Self {
            project: None,
            timeline: NleTimeline::new("Untitled", 30.0, 1920, 1080),
            backend: TinySkiaBackend::new(),
            active_grades: Vec::new(),
            captions: Vec::new(),
            base_duration_frames: 0,
        }
    }

    /// Apply `edit` to a copy of the session and keep the result only if everything succeeds.
    fn commit<T>(
        &mut self,
        edit: impl FnOnce(&mut Draft) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut draft = Draft {
            timeline: self.timeline.clone(),
            grades: self.active_grades.clone(),
            captions: self.captions.clone(),
        };
        let output = edit(&mut draft)?;

        let last_clip_end = draft
            .timeline
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .map(|clip| clip.track_out)
            .max()
            .unwrap_or(0);
        draft.timeline.duration_frames = self.base_duration_frames.max(last_clip_end);

        let project = match &self.project {
            Some(project) => Some(export_project(project, &draft)?),
            None => None,
        };
        self.project = project;
        self.timeline = draft.timeline;
        self.active_grades = draft.grades;
        self.captions = draft.captions;
        Ok(output)
    }

    /// Create a new project.
    pub fn create_project(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        fps: f64,
        duration_secs: f64,
    ) -> Result<String, String> {
        require_text("name", name)?;
        require_side("width", width, MAX_CANVAS_SIDE)?;
        require_side("height", height, MAX_CANVAS_SIDE)?;
        if !fps.is_finite() || fps <= 0.0 || fps > MAX_FPS {
            return Err(format!(
                "invalid 'fps': {fps} is not a positive rate of at most {MAX_FPS}"
            ));
        }
        let duration_frames = duration_to_frames("duration_secs", duration_secs, fps)?;
        let settings = ProjectSettings {
            width,
            height,
            fps,
            duration: duration_frames,
            scale: 1.0,
            crf: Some(18),
            preset: Some("medium".into()),
            concurrency: None,
            frame_step: 1,
            frame_start: None,
            frame_end: None,
            backend: BackendKind::Native,
            browser_image_format: None,
            browser_jpeg_quality: None,
            browser_frame_timeout_ms: None,
            browser_transport: None,
            browser_transport_retries: None,
        };

        let project = Project {
            version: 1,
            composition: name.to_string(),
            settings,
            props: serde_json::json!({}),
            assets: Vec::new(),
            tracks: Vec::new(),
            events: Vec::new(),
            voice_over_asset_id: None,
        };
        project.validate().map_err(|error| error.to_string())?;

        self.project = Some(project);
        self.timeline = NleTimeline::new(name, fps, width, height);
        self.timeline.duration_frames = duration_frames;
        self.base_duration_frames = duration_frames;
        self.active_grades.clear();
        self.captions.clear();

        Ok(format!(
            "Created project '{name}' ({width}x{height} @ {fps}fps, {duration_frames} frames)"
        ))
    }

    /// Add a track to the timeline.
    pub fn add_track(&mut self, kind: TrackKind, name: &str) -> Result<String, String> {
        require_text("name", name)?;
        self.commit(|draft| {
            let track_id = format!(
                "track_{}_{}",
                name.to_lowercase().replace(' ', "_"),
                draft.timeline.tracks.len() + 1
            );
            draft
                .timeline
                .tracks
                .push(NleTrack::new(track_id.clone(), name, kind));
            Ok(track_id)
        })
    }

    /// Insert a clip onto a track, shifting later clips on that track to the right.
    pub fn insert_clip(
        &mut self,
        track_id: &str,
        name: &str,
        start_secs: f64,
        duration_secs: f64,
        asset_id: Option<String>,
    ) -> Result<String, String> {
        self.insert(track_id, name, start_secs, duration_secs, asset_id, None)
    }

    /// Insert a clip that displays `text` for its duration.
    pub fn insert_caption(
        &mut self,
        track_id: &str,
        text: &str,
        start_secs: f64,
        duration_secs: f64,
    ) -> Result<String, String> {
        require_text("text", text)?;
        self.insert(
            track_id,
            &format!("caption_{text}"),
            start_secs,
            duration_secs,
            None,
            Some(text.to_string()),
        )
    }

    fn insert(
        &mut self,
        track_id: &str,
        name: &str,
        start_secs: f64,
        duration_secs: f64,
        asset_id: Option<String>,
        caption: Option<String>,
    ) -> Result<String, String> {
        require_text("name", name)?;
        let fps = self.timeline.fps;
        let track_in = secs_to_frame("start_secs", start_secs, fps)?;
        let duration = duration_to_frames("duration_secs", duration_secs, fps)?;

        self.commit(|draft| {
            let track_index = draft
                .timeline
                .tracks
                .iter()
                .position(|track| track.id == track_id)
                .ok_or_else(|| format!("Track '{track_id}' not found"))?;
            let base = format!(
                "clip_{}_{}",
                name.to_lowercase().replace(' ', "_"),
                track_in
            );
            let clip_id = draft.unused_clip_id(&base);

            let track = &draft.timeline.tracks[track_index];
            let last_end = track.clips.iter().map(|c| c.track_out).max().unwrap_or(0);
            // Inserting shifts later clips right, so the shifted end must stay in range too.
            if u64::from(last_end.max(track_in)) + u64::from(duration)
                > u64::from(MAX_TIMELINE_FRAMES)
            {
                return Err(format!(
                    "invalid 'duration_secs': the track would exceed {MAX_TIMELINE_FRAMES} frames"
                ));
            }
            // The clip that the insertion point cuts in two, if any.
            let cut_clip = track
                .clips
                .iter()
                .find(|c| c.track_in < track_in && track_in < c.track_out)
                .map(|c| c.id.clone());
            let ids_before: Vec<String> = track.clips.iter().map(|c| c.id.clone()).collect();

            let mut clip = NleClip::new(clip_id.clone(), name, track_in, duration, 0);
            clip.asset_id = asset_id;
            insert_clip(&mut draft.timeline.tracks[track_index], track_in, clip)
                .map_err(|e| e.to_string())?;

            if let Some(original) = cut_clip {
                let part = draft.timeline.tracks[track_index]
                    .clips
                    .iter()
                    .find(|c| c.id != clip_id && !ids_before.contains(&c.id))
                    .map(|c| c.id.clone());
                if let Some(part) = part {
                    draft.adopt_split_part(track_index, &original, part);
                }
            }
            if let Some(text) = caption {
                draft.captions.push((clip_id.clone(), text));
            }
            Ok(clip_id)
        })
    }

    /// Trim clip edge.
    pub fn trim_clip(
        &mut self,
        clip_id: &str,
        edge_in: bool,
        new_time_secs: f64,
    ) -> Result<String, String> {
        let fps = self.timeline.fps;
        let target_frame = secs_to_frame("new_time_secs", new_time_secs, fps)?;
        let edge = if edge_in { TrimEdge::In } else { TrimEdge::Out };

        self.commit(|draft| {
            let (track_index, clip_index) = draft.locate(clip_id)?;
            let clip = &draft.timeline.tracks[track_index].clips[clip_index];
            let current_frame = match edge {
                TrimEdge::In if target_frame >= clip.track_out => {
                    return Err(format!(
                        "invalid 'new_time_secs': frame {target_frame} is not before the out-point of clip '{clip_id}' (frame {})",
                        clip.track_out
                    ));
                }
                TrimEdge::Out if target_frame <= clip.track_in => {
                    return Err(format!(
                        "invalid 'new_time_secs': frame {target_frame} is not after the in-point of clip '{clip_id}' (frame {})",
                        clip.track_in
                    ));
                }
                TrimEdge::In => clip.track_in,
                TrimEdge::Out => clip.track_out,
            };
            let delta = i64::from(target_frame) - i64::from(current_frame);
            if edge_in && clip.asset_id.is_some() && i64::from(clip.source_in) + delta < 0 {
                return Err(format!(
                    "invalid 'new_time_secs': clip '{clip_id}' has only {} frames of source before its in-point",
                    clip.source_in
                ));
            }
            let delta = i32::try_from(delta)
                .map_err(|_| "invalid 'new_time_secs': trim distance is too large".to_string())?;
            trim_clip(
                &mut draft.timeline.tracks[track_index],
                clip_index,
                edge,
                delta,
                TrimMode::Regular,
            )
            .map_err(|e| e.to_string())?;
            Ok(format!("Trimmed clip '{clip_id}' to frame {target_frame}"))
        })
    }

    /// Split the clip `clip_id` at a time strictly inside it.
    pub fn split_clip(
        &mut self,
        clip_id: &str,
        split_time_secs: f64,
    ) -> Result<(String, String), String> {
        let fps = self.timeline.fps;
        let split_frame = secs_to_frame("split_time_secs", split_time_secs, fps)?;

        self.commit(|draft| {
            let (track_index, clip_index) = draft.locate(clip_id)?;
            let clip = &draft.timeline.tracks[track_index].clips[clip_index];
            if split_frame <= clip.track_in || split_frame >= clip.track_out {
                return Err(format!(
                    "invalid 'split_time_secs': frame {split_frame} is not strictly inside clip '{clip_id}' (frames {}..{})",
                    clip.track_in, clip.track_out
                ));
            }
            let (left, right) = split_clip_at_index(
                &mut draft.timeline.tracks[track_index],
                clip_index,
                split_frame,
            )
            .map_err(|e| e.to_string())?;
            let right_id = draft.adopt_split_part(track_index, clip_id, right.id);
            Ok((left.id, right_id))
        })
    }

    /// Apply color grade parameters to a clip.
    pub fn apply_color_grade(
        &mut self,
        clip_id: &str,
        params: ColorGradeParams,
    ) -> Result<String, String> {
        params.validate()?;
        self.commit(|draft| {
            draft.locate(clip_id)?;
            draft.grades.retain(|(id, _)| id != clip_id);
            draft.grades.push((clip_id.to_string(), params));
            Ok(format!("Applied color grading to clip '{clip_id}'"))
        })
    }

    /// Render a schematic preview at `time_secs` and return it as a base64 PNG data URL.
    ///
    /// See [`PREVIEW_LIMITATIONS`] for what it does not show.
    pub fn render_frame_preview(
        &self,
        time_secs: f64,
        width: u32,
        height: u32,
    ) -> Result<String, String> {
        require_side("width", width, MAX_PREVIEW_SIDE)?;
        require_side("height", height, MAX_PREVIEW_SIDE)?;
        let fps = self.timeline.fps;
        let frame = secs_to_frame("time_secs", time_secs, fps)?;

        let mut scene = Scene::new();

        // Background
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: width as f32,
            h: height as f32,
            fill: Color::rgb(18, 18, 24),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });

        let font_size = (height as f32 * 0.07).max(8.0);
        let mut captions = Vec::new();

        // Collect visible clips at this frame
        for track in &self.timeline.tracks {
            if track.muted {
                continue;
            }
            for clip in &track.clips {
                if frame >= clip.track_in && frame < clip.track_out {
                    let mut filters = Vec::new();

                    // Check if clip has active color grading
                    if let Some((_, grade)) =
                        self.active_grades.iter().find(|(id, _)| id == &clip.id)
                    {
                        if let Some(exposure) = grade.exposure {
                            push_exposure(&mut filters, exposure);
                        }
                        if let Some(cube) = &grade.lut_cube {
                            filters.push(SceneFilter::Lut3D {
                                cube_content: cube.clone(),
                                amount: 1.0,
                                tetrahedral: true,
                            });
                        }
                        if grade.contrast.is_some()
                            || grade.saturation.is_some()
                            || grade.gamma.is_some()
                        {
                            filters.push(SceneFilter::ColorGrading {
                                contrast: grade.contrast.unwrap_or(1.0),
                                saturation: grade.saturation.unwrap_or(1.0),
                                gamma: grade.gamma.unwrap_or(1.0),
                                tint: None,
                            });
                        }
                    }

                    scene.push(SceneNode::Layer {
                        opacity: clip.opacity,
                        blend_mode: rust_motion_rasterizer::scene::BlendMode::Normal,
                        clip: None,
                        mask: None,
                        mask_mode: rust_motion_rasterizer::scene::MaskMode::Alpha,
                        filters,
                        shadow: None,
                        children: vec![SceneNode::Rect {
                            x: 50.0,
                            y: 50.0,
                            w: (width as f32 - 100.0).max(10.0),
                            h: (height as f32 - 100.0).max(10.0),
                            fill: match track.kind {
                                TrackKind::Video => Color::rgb(45, 120, 220),
                                TrackKind::Audio => Color::rgb(50, 180, 100),
                                TrackKind::Title => Color::rgb(220, 160, 40),
                                _ => Color::rgb(120, 120, 140),
                            },
                            stroke: Some(Color::WHITE),
                            stroke_width: 2.0,
                            corner_radius: 8.0,
                        }],
                    });

                    if let Some((_, text)) = self.captions.iter().find(|(id, _)| id == &clip.id) {
                        captions.push(text.clone());
                    }
                }
            }
        }

        // Captions go on top of every block, stacked upwards from the lower part of the frame.
        for (line, content) in captions.into_iter().enumerate() {
            scene.push(SceneNode::Text {
                x: width as f32 * 0.08,
                y: height as f32 * 0.72 - line as f32 * font_size * 1.3,
                content,
                font_size,
                color: Color::WHITE,
                font_weight: 700,
                font_sources: Vec::new(),
            });
        }

        let config = FrameConfig::new(width, height, frame, fps);
        let img = self
            .backend
            .render_frame(&scene, &config)
            .map_err(|e| e.to_string())?;

        let mut png_bytes = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| e.to_string())?;

        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png_bytes);
        Ok(format!("data:image/png;base64,{b64}"))
    }
}

/// Express `exposure` EV as brightness multipliers (each filter accepts at most 10x).
fn push_exposure(filters: &mut Vec<SceneFilter>, exposure: f32) {
    let gain = exposure.exp2();
    if gain > 10.0 {
        let half = gain.sqrt();
        filters.push(SceneFilter::Brightness { amount: half });
        filters.push(SceneFilter::Brightness { amount: half });
    } else {
        filters.push(SceneFilter::Brightness { amount: gain });
    }
}
