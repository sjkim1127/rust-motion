//! MCP Tool definitions and execution dispatch.

use crate::args::Args;
use crate::session::{secs_to_frame, ColorGradeParams, McpSession, PREVIEW_LIMITATIONS};
use rust_motion_project::nle::TrackKind;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// Metadata describing an MCP tool.
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// Returned by `render_composition`, which is not connected to the renderer yet.
pub const RENDER_UNSUPPORTED: &str = "render_composition is not implemented yet: no video was \
rendered and nothing was written. Export the project and render it with the rust-motion CLI.";

/// All MCP tools exposed by the video/audio editing server.
pub fn get_tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "project_create",
            description: "Initialize a new video production project with canvas dimensions, fps, and duration.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Project title" },
                    "width": { "type": "integer", "default": 1920, "description": "Canvas width in pixels" },
                    "height": { "type": "integer", "default": 1080, "description": "Canvas height in pixels" },
                    "fps": { "type": "number", "default": 30.0, "description": "Timeline frames per second" },
                    "duration_secs": { "type": "number", "default": 10.0, "description": "Composition duration in seconds" }
                },
                "required": ["name"]
            }),
        },
        ToolDef {
            name: "timeline_add_track",
            description: "Add a track (video, audio, title/caption, adjustment) to the NLE timeline.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "track_type": { "type": "string", "enum": ["video", "audio", "title", "adjustment"], "description": "Type of track" },
                    "name": { "type": "string", "description": "User-facing track label" }
                },
                "required": ["track_type", "name"]
            }),
        },
        ToolDef {
            name: "clip_insert",
            description: "Insert a video, audio, or title clip at a precise timeline offset.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "track_id": { "type": "string", "description": "Target track ID" },
                    "name": { "type": "string", "description": "Clip label" },
                    "start_secs": { "type": "number", "description": "Timeline start position in seconds" },
                    "duration_secs": { "type": "number", "description": "Clip duration in seconds" },
                    "asset_id": { "type": "string", "description": "Optional asset reference ID" }
                },
                "required": ["track_id", "name", "start_secs", "duration_secs"]
            }),
        },
        ToolDef {
            name: "clip_trim",
            description: "Trim clip in-point or out-point without moving other timeline elements.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "clip_id": { "type": "string", "description": "ID of clip to trim" },
                    "edge_in": { "type": "boolean", "description": "True to trim in-point (head), false for out-point (tail)" },
                    "new_time_secs": { "type": "number", "description": "New edge time in seconds" }
                },
                "required": ["clip_id", "edge_in", "new_time_secs"]
            }),
        },
        ToolDef {
            name: "clip_split",
            description: "Split an existing clip into two distinct clips at the given timeline time offset.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "clip_id": { "type": "string", "description": "ID of clip to split" },
                    "split_time_secs": { "type": "number", "description": "Timeline cut position in seconds" }
                },
                "required": ["clip_id", "split_time_secs"]
            }),
        },
        ToolDef {
            name: "color_grade_apply",
            description: "Apply professional color grading (exposure, contrast, saturation, gamma, 3D LUT cube) to a clip.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "clip_id": { "type": "string", "description": "Target clip ID" },
                    "exposure": { "type": "number", "minimum": -5.0, "maximum": 5.0, "description": "Exposure EV adjustment (-5.0..+5.0)" },
                    "contrast": { "type": "number", "description": "Contrast scale factor (0.5..2.0)" },
                    "saturation": { "type": "number", "description": "Color saturation scale factor (0.0..2.0)" },
                    "gamma": { "type": "number", "description": "Gamma curve correction (0.5..2.0)" },
                    "lut_cube": { "type": "string", "description": "Optional 3D LUT as .cube file content (preset names are not supported)" }
                },
                "required": ["clip_id"]
            }),
        },
        ToolDef {
            name: "subtitles_generate",
            description: "Add a timed plain-text caption clip to a track. Animated caption styles are not implemented.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "track_id": { "type": "string", "description": "Track ID" },
                    "text": { "type": "string", "description": "Subtitle text" },
                    "start_secs": { "type": "number", "description": "Start timestamp in seconds" },
                    "duration_secs": { "type": "number", "description": "Duration in seconds" },
                    "style": { "type": "string", "enum": ["plain"], "default": "plain", "description": "Only plain captions are supported" }
                },
                "required": ["track_id", "text", "start_secs", "duration_secs"]
            }),
        },
        ToolDef {
            name: "preview_frame",
            description: "Render a SCHEMATIC preview of the timeline at a timestamp as a base64 PNG: colored blocks for visible clips plus plain caption text. It does not decode source media and is not the final render.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "time_secs": { "type": "number", "default": 0.0, "description": "Timeline playhead position in seconds" },
                    "width": { "type": "integer", "default": 640, "description": "Preview width in pixels" },
                    "height": { "type": "integer", "default": 360, "description": "Preview height in pixels" }
                }
            }),
        },
        ToolDef {
            name: "render_composition",
            description: "NOT IMPLEMENTED: always returns an error without rendering or writing any file.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "output_path": { "type": "string", "description": "Destination file path (.mp4, .webm, .mov)" },
                    "codec": { "type": "string", "default": "h264", "description": "Video encoder codec" },
                    "crf": { "type": "integer", "default": 18, "description": "Constant Rate Factor quality (0-51)" },
                    "fps": { "type": "number", "description": "Optional output frame rate override" }
                },
                "required": ["output_path"]
            }),
        },
    ]
}

/// Execute an MCP tool call against the session.
pub fn execute_tool(
    session: &Arc<Mutex<McpSession>>,
    name: &str,
    args: Value,
) -> Result<Value, String> {
    let args = Args::new(args)?;
    let mut sess = session
        .lock()
        .map_err(|e| format!("session lock error: {e}"))?;

    match name {
        "project_create" => {
            let p_name = args.str("name")?;
            let width = args.opt_u32("width")?.unwrap_or(1920);
            let height = args.opt_u32("height")?.unwrap_or(1080);
            let fps = args.opt_f64("fps")?.unwrap_or(30.0);
            let duration_secs = args.opt_f64("duration_secs")?.unwrap_or(10.0);

            let res = sess.create_project(p_name, width, height, fps, duration_secs)?;
            Ok(json!({ "status": "ok", "message": res }))
        }
        "timeline_add_track" => {
            let t_type = args.str("track_type")?;
            let t_name = args.str("name")?;

            let kind = match t_type.to_ascii_lowercase().as_str() {
                "video" => TrackKind::Video,
                "audio" => TrackKind::Audio,
                "title" => TrackKind::Title,
                "adjustment" => TrackKind::Adjustment,
                _ => return Err(format!("unknown track_type '{t_type}'")),
            };

            let track_id = sess.add_track(kind, t_name)?;
            Ok(json!({ "status": "ok", "track_id": track_id }))
        }
        "clip_insert" => {
            let track_id = args.str("track_id")?;
            let c_name = args.str("name")?;
            let start = args.f64("start_secs")?;
            let dur = args.f64("duration_secs")?;
            let asset_id = args.opt_str("asset_id")?.map(str::to_string);

            let clip_id = sess.insert_clip(track_id, c_name, start, dur, asset_id)?;
            Ok(json!({ "status": "ok", "clip_id": clip_id }))
        }
        "clip_trim" => {
            let clip_id = args.str("clip_id")?;
            let edge_in = args.bool("edge_in")?;
            let new_time = args.f64("new_time_secs")?;

            let res = sess.trim_clip(clip_id, edge_in, new_time)?;
            Ok(json!({ "status": "ok", "message": res }))
        }
        "clip_split" => {
            let clip_id = args.str("clip_id")?;
            let split_time = args.f64("split_time_secs")?;

            let (c1, c2) = sess.split_clip(clip_id, split_time)?;
            Ok(json!({ "status": "ok", "clip1": c1, "clip2": c2 }))
        }
        "color_grade_apply" => {
            let clip_id = args.str("clip_id")?;
            let params = ColorGradeParams {
                exposure: args.opt_f32("exposure")?,
                contrast: args.opt_f32("contrast")?,
                saturation: args.opt_f32("saturation")?,
                gamma: args.opt_f32("gamma")?,
                lut_cube: args.opt_str("lut_cube")?.map(str::to_string),
            };

            let res = sess.apply_color_grade(clip_id, params)?;
            Ok(json!({ "status": "ok", "message": res }))
        }
        "subtitles_generate" => {
            let track_id = args.str("track_id")?;
            let text = args.str("text")?;
            let start = args.f64("start_secs")?;
            let dur = args.f64("duration_secs")?;
            if let Some(style) = args.opt_str("style")? {
                if style != "plain" {
                    return Err(format!(
                        "unsupported 'style': '{style}' is not implemented; only 'plain' captions are available"
                    ));
                }
            }

            let clip_id = sess.insert_caption(track_id, text, start, dur)?;
            Ok(json!({ "status": "ok", "clip_id": clip_id, "text": text, "style": "plain" }))
        }
        "preview_frame" => {
            let time_secs = args.opt_f64("time_secs")?.unwrap_or(0.0);
            let width = args.opt_u32("width")?.unwrap_or(640);
            let height = args.opt_u32("height")?.unwrap_or(360);

            let data_url = sess.render_frame_preview(time_secs, width, height)?;
            let frame_idx = secs_to_frame("time_secs", time_secs, sess.timeline.fps)?;

            Ok(json!({
                "status": "ok",
                "preview_kind": "schematic",
                "limitations": PREVIEW_LIMITATIONS,
                "time_secs": time_secs,
                "frame_number": frame_idx,
                "image_data_url": data_url
            }))
        }
        "render_composition" => {
            // Validate the request anyway, so that callers learn about bad input first.
            let output_path = args.str("output_path")?;
            if output_path.trim().is_empty() {
                return Err("invalid 'output_path': must not be empty".to_string());
            }
            args.opt_str("codec")?;
            if let Some(crf) = args.opt_u32("crf")? {
                if crf > 51 {
                    return Err(format!("invalid 'crf': {crf} is outside 0..=51"));
                }
            }
            if let Some(fps) = args.opt_f64("fps")? {
                if fps <= 0.0 {
                    return Err(format!("invalid 'fps': {fps} must be positive"));
                }
            }
            Err(RENDER_UNSUPPORTED.to_string())
        }
        _ => Err(format!("unknown tool '{name}'")),
    }
}
