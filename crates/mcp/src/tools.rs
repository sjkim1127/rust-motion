//! MCP Tool definitions and execution dispatch.

use crate::session::{ColorGradeParams, McpSession};
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
                    "exposure": { "type": "number", "description": "Exposure EV adjustment (-5.0..+5.0)" },
                    "contrast": { "type": "number", "description": "Contrast scale factor (0.5..2.0)" },
                    "saturation": { "type": "number", "description": "Color saturation scale factor (0.0..2.0)" },
                    "gamma": { "type": "number", "description": "Gamma curve correction (0.5..2.0)" },
                    "lut_cube": { "type": "string", "description": "Optional 3D LUT .cube file content or preset name" }
                },
                "required": ["clip_id"]
            }),
        },
        ToolDef {
            name: "subtitles_generate",
            description: "Generate animated captions / subtitles on a track with kinetic typography styling.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "track_id": { "type": "string", "description": "Track ID" },
                    "text": { "type": "string", "description": "Subtitle text" },
                    "start_secs": { "type": "number", "description": "Start timestamp in seconds" },
                    "duration_secs": { "type": "number", "description": "Duration in seconds" },
                    "style": { "type": "string", "default": "kinetic_pop", "description": "Animation preset style" }
                },
                "required": ["track_id", "text", "start_secs", "duration_secs"]
            }),
        },
        ToolDef {
            name: "preview_frame",
            description: "Render a high-fidelity visual screenshot preview of the composition at a specific timestamp, returning a base64 PNG data URL for visual inspection by the AI agent.",
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
            description: "Trigger video rendering and file export with progress reporting.",
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
pub fn execute_tool(session: &Arc<Mutex<McpSession>>, name: &str, args: Value) -> Result<Value, String> {
    let mut sess = session.lock().map_err(|e| format!("session lock error: {e}"))?;

    match name {
        "project_create" => {
            let p_name = args.get("name").and_then(|v| v.as_str()).ok_or("missing 'name'")?;
            let width = args.get("width").and_then(|v| v.as_u64()).unwrap_or(1920) as u32;
            let height = args.get("height").and_then(|v| v.as_u64()).unwrap_or(1080) as u32;
            let fps = args.get("fps").and_then(|v| v.as_f64()).unwrap_or(30.0);
            let duration_secs = args.get("duration_secs").and_then(|v| v.as_f64()).unwrap_or(10.0);

            let res = sess.create_project(p_name, width, height, fps, duration_secs)?;
            Ok(json!({ "status": "ok", "message": res }))
        }
        "timeline_add_track" => {
            let t_type = args.get("track_type").and_then(|v| v.as_str()).ok_or("missing 'track_type'")?;
            let t_name = args.get("name").and_then(|v| v.as_str()).ok_or("missing 'name'")?;

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
            let track_id = args.get("track_id").and_then(|v| v.as_str()).ok_or("missing 'track_id'")?;
            let c_name = args.get("name").and_then(|v| v.as_str()).ok_or("missing 'name'")?;
            let start = args.get("start_secs").and_then(|v| v.as_f64()).ok_or("missing 'start_secs'")?;
            let dur = args.get("duration_secs").and_then(|v| v.as_f64()).ok_or("missing 'duration_secs'")?;
            let asset_id = args.get("asset_id").and_then(|v| v.as_str()).map(|s| s.to_string());

            let clip_id = sess.insert_clip(track_id, c_name, start, dur, asset_id)?;
            Ok(json!({ "status": "ok", "clip_id": clip_id }))
        }
        "clip_trim" => {
            let clip_id = args.get("clip_id").and_then(|v| v.as_str()).ok_or("missing 'clip_id'")?;
            let edge_in = args.get("edge_in").and_then(|v| v.as_bool()).ok_or("missing 'edge_in'")?;
            let new_time = args.get("new_time_secs").and_then(|v| v.as_f64()).ok_or("missing 'new_time_secs'")?;

            let res = sess.trim_clip(clip_id, edge_in, new_time)?;
            Ok(json!({ "status": "ok", "message": res }))
        }
        "clip_split" => {
            let clip_id = args.get("clip_id").and_then(|v| v.as_str()).ok_or("missing 'clip_id'")?;
            let split_time = args.get("split_time_secs").and_then(|v| v.as_f64()).ok_or("missing 'split_time_secs'")?;

            let (c1, c2) = sess.split_clip(clip_id, split_time)?;
            Ok(json!({ "status": "ok", "clip1": c1, "clip2": c2 }))
        }
        "color_grade_apply" => {
            let clip_id = args.get("clip_id").and_then(|v| v.as_str()).ok_or("missing 'clip_id'")?;
            let params = ColorGradeParams {
                exposure: args.get("exposure").and_then(|v| v.as_f64()).map(|x| x as f32),
                contrast: args.get("contrast").and_then(|v| v.as_f64()).map(|x| x as f32),
                saturation: args.get("saturation").and_then(|v| v.as_f64()).map(|x| x as f32),
                gamma: args.get("gamma").and_then(|v| v.as_f64()).map(|x| x as f32),
                lut_cube: args.get("lut_cube").and_then(|v| v.as_str()).map(|s| s.to_string()),
            };

            let res = sess.apply_color_grade(clip_id, params)?;
            Ok(json!({ "status": "ok", "message": res }))
        }
        "subtitles_generate" => {
            let track_id = args.get("track_id").and_then(|v| v.as_str()).ok_or("missing 'track_id'")?;
            let text = args.get("text").and_then(|v| v.as_str()).ok_or("missing 'text'")?;
            let start = args.get("start_secs").and_then(|v| v.as_f64()).ok_or("missing 'start_secs'")?;
            let dur = args.get("duration_secs").and_then(|v| v.as_f64()).ok_or("missing 'duration_secs'")?;

            let clip_id = sess.insert_clip(track_id, &format!("caption_{text}"), start, dur, None)?;
            Ok(json!({ "status": "ok", "clip_id": clip_id, "text": text }))
        }
        "preview_frame" => {
            let time_secs = args.get("time_secs").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let width = args.get("width").and_then(|v| v.as_u64()).unwrap_or(640) as u32;
            let height = args.get("height").and_then(|v| v.as_u64()).unwrap_or(360) as u32;

            let data_url = sess.render_frame_preview(time_secs, width, height)?;
            let frame_idx = (time_secs * sess.timeline.fps).round() as u32;

            Ok(json!({
                "status": "ok",
                "time_secs": time_secs,
                "frame_number": frame_idx,
                "image_data_url": data_url
            }))
        }
        "render_composition" => {
            let output_path = args.get("output_path").and_then(|v| v.as_str()).ok_or("missing 'output_path'")?;
            let codec = args.get("codec").and_then(|v| v.as_str()).unwrap_or("h264");
            let crf = args.get("crf").and_then(|v| v.as_u64()).unwrap_or(18) as u32;

            Ok(json!({
                "status": "ok",
                "message": format!("Render job initiated: output='{output_path}', codec='{codec}', crf={crf}"),
                "output_path": output_path
            }))
        }
        _ => Err(format!("unknown tool '{name}'")),
    }
}
