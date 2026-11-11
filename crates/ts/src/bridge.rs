//! Remotion rendering engine bridge.

use rust_motion_cli::{
    built_in_registry, execute_render_command_with_control, RenderBackend, RenderCodec,
    RenderRequest,
};
use rust_motion_rasterizer::{HwAccel, RenderControl};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsCompositionMeta {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_in_frames: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsRenderProgress {
    pub frame: u32,
    pub completed_frames: u32,
    pub total_frames: u32,
    pub progress: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsRenderRequest {
    pub output: String,
    #[serde(default)]
    pub composition: Option<String>,
    #[serde(default)]
    pub script: Option<String>,
    #[serde(default)]
    pub props: Option<serde_json::Value>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub fps: Option<f64>,
    #[serde(default)]
    pub duration_in_frames: Option<u32>,
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub crf: Option<u32>,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub hw_accel: Option<String>,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub frame_start: Option<u32>,
    #[serde(default)]
    pub frame_end: Option<u32>,
    #[serde(default)]
    pub audio: Option<Vec<String>>,
    #[serde(default)]
    pub permissive: Option<bool>,
    #[serde(default)]
    pub sandbox_roots: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsRenderResult {
    pub output: String,
    pub total_frames: u32,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsStillRequest {
    pub output: String,
    #[serde(default)]
    pub composition: Option<String>,
    #[serde(default)]
    pub script: Option<String>,
    #[serde(default)]
    pub frame: Option<u32>,
    #[serde(default)]
    pub props: Option<serde_json::Value>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub image_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsStillResult {
    pub output: String,
    pub frame: u32,
    pub width: u32,
    pub height: u32,
}

pub fn list_available_compositions() -> Vec<TsCompositionMeta> {
    let registry = built_in_registry();
    let mut list = Vec::new();
    for id in registry.ids() {
        list.push(TsCompositionMeta {
            id: id.to_string(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration_in_frames: 180,
        });
    }
    list
}

fn parse_codec(codec: Option<&str>, output: &str) -> RenderCodec {
    if let Some(c) = codec {
        match c.to_ascii_lowercase().as_str() {
            "h264" | "mp4" => RenderCodec::H264,
            "h265" | "hevc" => RenderCodec::H265,
            "vp9" | "webm" => RenderCodec::Vp9,
            "av1" => RenderCodec::Av1,
            "prores" | "mov" => RenderCodec::ProRes,
            "gif" => RenderCodec::Gif,
            "png" => RenderCodec::Png,
            "jpeg" | "jpg" => RenderCodec::Jpeg,
            "webp" => RenderCodec::Webp,
            _ => RenderCodec::H264,
        }
    } else {
        let path = std::path::Path::new(output);
        match path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "webm" => RenderCodec::Vp9,
            "gif" => RenderCodec::Gif,
            "mov" => RenderCodec::ProRes,
            "png" => RenderCodec::Png,
            "jpg" | "jpeg" => RenderCodec::Jpeg,
            "webp" => RenderCodec::Webp,
            _ => RenderCodec::H264,
        }
    }
}

fn parse_hw_accel(hw: Option<&str>) -> HwAccel {
    match hw.map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("disabled") | Some("none") | Some("sw") => HwAccel::Disabled,
        Some("videotoolbox") | Some("vt") | Some("apple") => HwAccel::VideoToolbox,
        Some("nvenc") | Some("nvidia") => HwAccel::Nvenc,
        _ => HwAccel::Auto,
    }
}

fn parse_backend(backend: Option<&str>) -> RenderBackend {
    match backend.map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("gpu") => RenderBackend::Gpu,
        Some("browser") | Some("chromium") => RenderBackend::Browser,
        _ => RenderBackend::Native,
    }
}

pub fn render_media_native(
    req: TsRenderRequest,
    progress_callback: Option<Arc<dyn Fn(TsRenderProgress) + Send + Sync>>,
) -> Result<TsRenderResult, String> {
    let width = req.width.unwrap_or(1920);
    let height = req.height.unwrap_or(1080);
    let fps = req.fps.unwrap_or(30.0);
    let duration = req.duration_in_frames.unwrap_or(180);
    let codec = parse_codec(req.codec.as_deref(), &req.output);
    let hw_accel = parse_hw_accel(req.hw_accel.as_deref());
    let backend = parse_backend(req.backend.as_deref());

    let props_path = if let Some(ref props_val) = req.props {
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!(
            "rust_motion_ts_props_{}_{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::write(&temp_file, serde_json::to_string(props_val).unwrap());
        Some(temp_file)
    } else {
        None
    };

    let audio_paths = req
        .audio
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .collect();

    let sandbox_roots = req
        .sandbox_roots
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .collect();

    let render_req = RenderRequest {
        composition: req.composition.clone(),
        script: req.script.map(PathBuf::from),
        props: props_path,
        output: PathBuf::from(&req.output),
        audio: audio_paths,
        width,
        height,
        scale: 1.0,
        fps,
        duration,
        backend,
        codec,
        frame_start: req.frame_start.unwrap_or(0),
        frame_end: req.frame_end,
        frame_step: 1,
        concurrency: None,
        timeout_seconds: None,
        crf: req.crf.unwrap_or(18),
        preset: req.preset.unwrap_or_else(|| "fast".to_string()),
        hw_accel,
        sandbox_roots,
        permissive: req.permissive.unwrap_or(true),
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("Failed to create tokio runtime: {e}"))?;

    let control = RenderControl::new();
    let control = if let Some(cb) = progress_callback {
        control.with_progress(move |p| {
            let pr = TsRenderProgress {
                frame: p.frame,
                completed_frames: p.completed_frames,
                total_frames: p.total_frames,
                progress: if p.total_frames > 0 {
                    p.completed_frames as f64 / p.total_frames as f64
                } else {
                    1.0
                },
            };
            cb(pr);
        })
    } else {
        control
    };

    rt.block_on(async {
        execute_render_command_with_control(&render_req, control)
            .await
            .map_err(|e| format!("Rendering failed: {e:#}"))
    })?;

    Ok(TsRenderResult {
        output: req.output,
        total_frames: duration,
        width,
        height,
        fps,
    })
}

pub fn render_still_native(req: TsStillRequest) -> Result<TsStillResult, String> {
    let frame = req.frame.unwrap_or(0);
    let width = req.width.unwrap_or(1920);
    let height = req.height.unwrap_or(1080);

    let format_ext = req
        .image_format
        .unwrap_or_else(|| {
            std::path::Path::new(&req.output)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("png")
                .to_string()
        })
        .to_ascii_lowercase();

    let codec = match format_ext.as_str() {
        "jpg" | "jpeg" => "jpeg",
        "webp" => "webp",
        _ => "png",
    };

    let render_req = TsRenderRequest {
        output: req.output.clone(),
        composition: req.composition,
        script: req.script,
        props: req.props,
        width: Some(width),
        height: Some(height),
        fps: Some(30.0),
        duration_in_frames: Some(frame + 1),
        codec: Some(codec.to_string()),
        crf: Some(18),
        preset: Some("fast".to_string()),
        hw_accel: Some("auto".to_string()),
        backend: Some("native".to_string()),
        frame_start: Some(frame),
        frame_end: Some(frame),
        audio: None,
        permissive: Some(true),
        sandbox_roots: None,
    };

    render_media_native(render_req, None)?;

    Ok(TsStillResult {
        output: req.output,
        frame,
        width,
        height,
    })
}
