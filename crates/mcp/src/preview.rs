//! Single-frame preview of the timeline: decoded media where a clip has an asset, placeholders
//! where it does not.

use crate::session::{require_side, secs_to_frame, ColorGradeParams, McpSession, MAX_PREVIEW_SIDE};
use rust_motion_project::nle::{NleClip, TrackKind};
use rust_motion_project::{AssetKind, AssetRef};
use rust_motion_rasterizer::backend::{FrameConfig, RasterizerBackend};
use rust_motion_rasterizer::scene::{
    BlendMode, Color, ImageFit, MaskMode, Scene, SceneFilter, SceneNode,
};
use rust_motion_rasterizer::TinySkiaBackend;
use serde::Serialize;

/// What the preview does not show. Reported with every preview so an agent cannot mistake it for
/// the rendered composition.
pub const PREVIEW_LIMITATIONS: &[&str] = &[
    "This is not a final render: render_composition is not implemented, and the rust-motion CLI does not composite this timeline's media assets",
    "Image and video clips are fitted inside the canvas; position, scale and rotation cannot be set",
    "Animated images show their first frame, and a video clip running past the end of its source holds the last frame",
    "Clips without an asset or caption are drawn as colored placeholder blocks",
    "Captions are drawn as plain text without animation",
    "Exposure is applied as a 2^EV multiplier on encoded values, not in scene-linear light",
    "Audio and adjustment tracks are not represented, and media clips with speed changes, reverse or transitions are rejected",
    "When video decoding is unavailable (for example an ffmpeg-free build or unsupported codec), affected video clips degrade to placeholders",
];

/// How much of a preview comes from real content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewKind {
    /// No placeholders: everything drawn is decoded media or caption text.
    Media,
    /// Decoded media together with placeholders.
    Mixed,
    /// Placeholders (and possibly caption text) only.
    Schematic,
}

#[derive(Debug, Clone)]
pub struct Preview {
    /// The frame as a base64 PNG data URL.
    pub data_url: String,
    pub frame: u32,
    pub kind: PreviewKind,
    /// Clips whose source media was decoded.
    pub media_clips: Vec<String>,
    /// Clips drawn as placeholder blocks.
    pub placeholder_clips: Vec<String>,
    /// Clips visible at this frame that the preview does not draw.
    pub omitted_clips: Vec<String>,
}

/// The project canvas fitted into the preview.
#[derive(Clone, Copy)]
struct Canvas {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl McpSession {
    /// Render the timeline at `time_secs`.
    ///
    /// See [`PREVIEW_LIMITATIONS`] for what it does not show.
    pub fn render_frame_preview(
        &self,
        time_secs: f64,
        width: u32,
        height: u32,
    ) -> Result<Preview, String> {
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

        let scale = (width as f32 / self.timeline.width as f32)
            .min(height as f32 / self.timeline.height as f32);
        let canvas_w = self.timeline.width as f32 * scale;
        let canvas_h = self.timeline.height as f32 * scale;
        let canvas = Canvas {
            x: (width as f32 - canvas_w) / 2.0,
            y: (height as f32 - canvas_h) / 2.0,
            w: canvas_w,
            h: canvas_h,
        };

        let font_size = (height as f32 * 0.07).max(8.0);
        let build =
            |allow_video: bool| -> Result<(Scene, Vec<String>, Vec<String>, Vec<String>), String> {
                let mut scene = Scene::new();
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
                let mut captions: Vec<(String, f32, Option<&ColorGradeParams>)> = Vec::new();
                let mut media_clips = Vec::new();
                let mut placeholder_clips = Vec::new();
                let mut omitted_clips = Vec::new();
                for track in self.timeline.tracks.iter().filter(|track| !track.muted) {
                    let visible = track
                        .clips
                        .iter()
                        .filter(|clip| frame >= clip.track_in && frame < clip.track_out);
                    for clip in visible {
                        let asset = self.asset_of(clip)?;
                        if matches!(track.kind, TrackKind::Audio | TrackKind::Adjustment)
                            || asset.is_some_and(|asset| asset.kind == AssetKind::Audio)
                        {
                            omitted_clips.push(clip.id.clone());
                            continue;
                        }
                        let grade = self
                            .active_grades
                            .iter()
                            .find(|(id, _)| id == &clip.id)
                            .map(|(_, grade)| grade);
                        let caption = self.captions.iter().find(|(id, _)| id == &clip.id);
                        if let Some(asset) = asset {
                            if !allow_video && asset.kind == AssetKind::Video {
                                let node = placeholder(track.kind, width, height);
                                scene.push(graded_layer(clip.opacity, grade, node));
                                placeholder_clips.push(clip.id.clone());
                            } else {
                                let node = self.media_node(clip, asset, frame, canvas)?;
                                scene.push(graded_layer(clip.opacity, grade, node));
                                media_clips.push(clip.id.clone());
                            }
                        } else if caption.is_none() {
                            let node = placeholder(track.kind, width, height);
                            scene.push(graded_layer(clip.opacity, grade, node));
                            placeholder_clips.push(clip.id.clone());
                        }
                        if let Some((_, text)) = caption {
                            captions.push((text.clone(), clip.opacity, grade));
                        }
                    }
                }
                for (line, (content, opacity, grade)) in captions.into_iter().enumerate() {
                    let text = SceneNode::Text {
                        x: width as f32 * 0.08,
                        y: height as f32 * 0.72 - line as f32 * font_size * 1.3,
                        content,
                        font_size,
                        color: Color::WHITE,
                        font_weight: 700,
                        font_sources: Vec::new(),
                    };
                    scene.push(graded_layer(opacity, grade, text));
                }
                Ok((scene, media_clips, placeholder_clips, omitted_clips))
            };

        let backend = TinySkiaBackend::new_with_policy(self.media_policy());
        let (scene, mut media_clips, mut placeholder_clips, omitted_clips) = build(true)?;
        let rendered = backend.render_frame(&scene, &FrameConfig::new(width, height, frame, fps));
        let img = match rendered {
            Ok(img) => img,
            Err(error) => {
                let msg = error.to_string();
                let has_video = self
                    .timeline
                    .tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .any(|clip| {
                        frame >= clip.track_in
                            && frame < clip.track_out
                            && self
                                .asset_of(clip)
                                .ok()
                                .flatten()
                                .is_some_and(|a| a.kind == AssetKind::Video)
                    });
                if has_video {
                    let (scene2, m2, p2, _o2) = build(false)?;
                    media_clips = m2;
                    placeholder_clips = p2;
                    let backend2 = TinySkiaBackend::new_with_policy(self.media_policy());
                    backend2
                        .render_frame(&scene2, &FrameConfig::new(width, height, frame, fps))
                        .map_err(|e| format!("preview failed: {e}; original video error: {msg}"))?
                } else {
                    return Err(format!("preview failed: {msg}"));
                }
            }
        };

        let mut png_bytes = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| e.to_string())?;
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png_bytes);

        let kind = if placeholder_clips.is_empty() {
            PreviewKind::Media
        } else if media_clips.is_empty() {
            PreviewKind::Schematic
        } else {
            PreviewKind::Mixed
        };
        Ok(Preview {
            data_url: format!("data:image/png;base64,{b64}"),
            frame,
            kind,
            media_clips,
            placeholder_clips,
            omitted_clips,
        })
    }

    /// The registered asset a clip refers to, if it refers to one.
    fn asset_of(&self, clip: &NleClip) -> Result<Option<&AssetRef>, String> {
        let Some(asset_id) = &clip.asset_id else {
            return Ok(None);
        };
        self.project
            .iter()
            .flat_map(|project| &project.assets)
            .find(|asset| &asset.id == asset_id)
            .map(Some)
            .ok_or_else(|| {
                format!(
                    "clip '{}' refers to unregistered asset '{asset_id}'",
                    clip.id
                )
            })
    }

    /// The scene node showing `clip`'s source media at timeline frame `frame`.
    fn media_node(
        &self,
        clip: &NleClip,
        asset: &AssetRef,
        frame: u32,
        canvas: Canvas,
    ) -> Result<SceneNode, String> {
        if clip.speed != 1.0
            || clip.reverse
            || clip.in_transition.is_some()
            || clip.out_transition.is_some()
        {
            return Err(format!(
                "clip '{}' uses a speed change, reverse or transition, which the preview does not support",
                clip.id
            ));
        }
        let src = self
            .resolve_media(&asset.path)
            .map_err(|error| format!("clip '{}', asset '{}': {error}", clip.id, asset.id))?
            .display()
            .to_string();
        let Canvas { x, y, w, h } = canvas;
        match asset.kind {
            AssetKind::Image => Ok(SceneNode::Image {
                src,
                x,
                y,
                w,
                h,
                fit: ImageFit::Contain,
                opacity: 1.0,
            }),
            AssetKind::Video => {
                let source_frame = u64::from(clip.source_in) + u64::from(frame - clip.track_in);
                Ok(SceneNode::Video {
                    src,
                    time: source_frame as f64 / self.timeline.fps,
                    looped: false,
                    x,
                    y,
                    w,
                    h,
                    fit: ImageFit::Contain,
                    opacity: 1.0,
                    interpolation: Default::default(),
                    time_remap: None,
                })
            }
            other => Err(format!(
                "clip '{}': {other:?} assets cannot be previewed",
                clip.id
            )),
        }
    }
}

/// A block standing in for a clip that has no media.
fn placeholder(kind: TrackKind, width: u32, height: u32) -> SceneNode {
    SceneNode::Rect {
        x: 50.0,
        y: 50.0,
        w: (width as f32 - 100.0).max(10.0),
        h: (height as f32 - 100.0).max(10.0),
        fill: match kind {
            TrackKind::Video => Color::rgb(45, 120, 220),
            TrackKind::Title => Color::rgb(220, 160, 40),
            _ => Color::rgb(120, 120, 140),
        },
        stroke: Some(Color::WHITE),
        stroke_width: 2.0,
        corner_radius: 8.0,
    }
}

/// `child` composited with a clip's opacity and color grade.
fn graded_layer(opacity: f32, grade: Option<&ColorGradeParams>, child: SceneNode) -> SceneNode {
    let mut filters = Vec::new();
    if let Some(grade) = grade {
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
        if grade.contrast.is_some() || grade.saturation.is_some() || grade.gamma.is_some() {
            filters.push(SceneFilter::ColorGrading {
                contrast: grade.contrast.unwrap_or(1.0),
                saturation: grade.saturation.unwrap_or(1.0),
                gamma: grade.gamma.unwrap_or(1.0),
                tint: None,
            });
        }
    }
    SceneNode::Layer {
        opacity,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters,
        shadow: None,
        children: vec![child],
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
