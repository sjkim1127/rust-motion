//! MCP Editing Session state: projects, timeline tracks, clips, and preview cache.

use rust_motion_project::nle::{
    insert_clip, split_clip_at, trim_clip, NleClip, NleTimeline, NleTrack, TrackKind, TrimEdge,
    TrimMode,
};
use rust_motion_project::{BackendKind, Clip, Project, ProjectSettings, Track};
use rust_motion_rasterizer::backend::{FrameConfig, RasterizerBackend};
use rust_motion_rasterizer::scene::{Color, Scene, SceneFilter, SceneNode};
use rust_motion_rasterizer::TinySkiaBackend;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorGradeParams {
    pub exposure: Option<f32>,
    pub contrast: Option<f32>,
    pub saturation: Option<f32>,
    pub gamma: Option<f32>,
    pub lut_cube: Option<String>,
}

pub struct McpSession {
    pub project: Option<Project>,
    pub timeline: NleTimeline,
    pub backend: TinySkiaBackend,
    pub active_grades: Vec<(String, ColorGradeParams)>,
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
        }
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
        let duration_frames = (duration_secs * fps).round().max(1.0) as u32;
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

        self.project = Some(project);
        self.timeline = NleTimeline::new(name, fps, width, height);
        self.timeline.duration_frames = duration_frames;

        Ok(format!("Created project '{name}' ({width}x{height} @ {fps}fps, {duration_frames} frames)"))
    }

    /// Add a track to the timeline.
    pub fn add_track(&mut self, kind: TrackKind, name: &str) -> Result<String, String> {
        let track_id = format!("track_{}_{}", name.to_lowercase().replace(' ', "_"), self.timeline.tracks.len() + 1);
        let track = NleTrack::new(track_id.clone(), name, kind);
        self.timeline.tracks.push(track);

        if let Some(ref mut proj) = self.project {
            proj.tracks.push(Track {
                id: track_id.clone(),
                clips: Vec::new(),
            });
        }

        Ok(track_id)
    }

    /// Insert a clip onto a track.
    pub fn insert_clip(
        &mut self,
        track_id: &str,
        name: &str,
        start_secs: f64,
        duration_secs: f64,
        asset_id: Option<String>,
    ) -> Result<String, String> {
        let fps = self.timeline.fps;
        let track_in = (start_secs * fps).round() as u32;
        let duration = (duration_secs * fps).round().max(1.0) as u32;

        let clip_id = format!("clip_{}_{}", name.to_lowercase().replace(' ', "_"), track_in);
        let mut clip = NleClip::new(clip_id.clone(), name, track_in, duration, 0);
        clip.asset_id = asset_id;

        let track = self
            .timeline
            .tracks
            .iter_mut()
            .find(|t| t.id == track_id)
            .ok_or_else(|| format!("Track '{track_id}' not found"))?;

        insert_clip(track, track_in, clip).map_err(|e| e.to_string())?;

        if let Some(ref mut proj) = self.project {
            if let Some(t) = proj.tracks.iter_mut().find(|t| t.id == track_id) {
                t.clips.push(Clip {
                    id: clip_id.clone(),
                    composition: name.to_string(),
                    start: track_in,
                    duration,
                    props: serde_json::json!({}),
                });
            }
        }

        self.timeline.update_duration();
        Ok(clip_id)
    }

    /// Trim clip edge.
    pub fn trim_clip(&mut self, clip_id: &str, edge_in: bool, new_time_secs: f64) -> Result<String, String> {
        let fps = self.timeline.fps;
        let target_frame = (new_time_secs * fps).round() as u32;
        let edge = if edge_in { TrimEdge::In } else { TrimEdge::Out };

        for track in &mut self.timeline.tracks {
            if let Some(idx) = track.clips.iter().position(|c| c.id == clip_id) {
                let current_frame = match edge {
                    TrimEdge::In => track.clips[idx].track_in,
                    TrimEdge::Out => track.clips[idx].track_out,
                };
                let delta = target_frame as i32 - current_frame as i32;
                trim_clip(track, idx, edge, delta, TrimMode::Regular).map_err(|e| e.to_string())?;
                self.timeline.update_duration();
                return Ok(format!("Trimmed clip '{clip_id}' to frame {target_frame}"));
            }
        }

        Err(format!("Clip '{clip_id}' not found"))
    }

    /// Split clip at time offset.
    pub fn split_clip(&mut self, clip_id: &str, split_time_secs: f64) -> Result<(String, String), String> {
        let fps = self.timeline.fps;
        let split_frame = (split_time_secs * fps).round() as u32;

        for track in &mut self.timeline.tracks {
            if track.clips.iter().any(|c| c.id == clip_id) {
                let (c1, c2) = split_clip_at(track, split_frame).map_err(|e| e.to_string())?;
                return Ok((c1.id, c2.id));
            }
        }

        Err(format!("Clip '{clip_id}' not found"))
    }

    /// Apply color grade parameters to a clip.
    pub fn apply_color_grade(&mut self, clip_id: &str, params: ColorGradeParams) -> Result<String, String> {
        self.active_grades.retain(|(id, _)| id != clip_id);
        self.active_grades.push((clip_id.to_string(), params));
        Ok(format!("Applied color grading to clip '{clip_id}'"))
    }

    /// Render frame preview at time (secs) and return base64 PNG data URL.
    pub fn render_frame_preview(&self, time_secs: f64, width: u32, height: u32) -> Result<String, String> {
        let fps = self.timeline.fps;
        let frame = (time_secs * fps).round() as u32;

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

        // Collect visible clips at this frame
        for track in &self.timeline.tracks {
            if track.muted {
                continue;
            }
            for clip in &track.clips {
                if frame >= clip.track_in && frame < clip.track_out {
                    let mut filters = Vec::new();

                    // Check if clip has active color grading
                    if let Some((_, grade)) = self.active_grades.iter().find(|(id, _)| id == &clip.id) {
                        if let Some(cube) = &grade.lut_cube {
                            filters.push(SceneFilter::Lut3D {
                                cube_content: cube.clone(),
                                amount: 1.0,
                                tetrahedral: true,
                            });
                        }
                        if grade.contrast.is_some() || grade.saturation.is_some() || grade.gamma.is_some() {
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
                }
            }
        }

        let config = FrameConfig::new(width, height, frame, fps);
        let img = self.backend.render_frame(&scene, &config).map_err(|e| e.to_string())?;

        let mut png_bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;

        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png_bytes);
        Ok(format!("data:image/png;base64,{b64}"))
    }
}
