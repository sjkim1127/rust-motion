//! Video encoding — stitches rendered frame PNGs into a video file via FFmpeg.

use crate::render_frames::RenderError;
use std::path::{Path, PathBuf};

/// Configuration for the encoding step.
#[derive(Debug, Clone)]
pub struct EncodeConfig {
    /// Directory containing `frame_%06d.png` files.
    pub frames_dir: PathBuf,
    /// Output video file path.
    pub output: PathBuf,
    /// Frames per second.
    pub fps: f64,
    /// CRF quality (lower = better; 0–51 for H.264).
    pub crf: u32,
    /// FFmpeg preset (e.g., `"fast"`, `"medium"`, `"ultrafast"`).
    pub preset: String,
    /// Video codec string for FFmpeg (e.g., `"libx264"`).
    pub codec: String,
    /// Pixel format string for FFmpeg (e.g., `"yuv420p"`).
    pub pixel_format: String,
    /// Video width in pixels (optional).
    pub width: Option<u32>,
    /// Video height in pixels (optional).
    pub height: Option<u32>,
    /// MP4 container flags for web optimization (e.g., `"+faststart"`).
    pub movflags: String,
    /// Automatically remove temporary frame directory after encoding succeeds.
    pub cleanup_after_encode: bool,
}

impl EncodeConfig {
    /// Create an H.264 config with sensible defaults.
    pub fn h264(frames_dir: impl Into<PathBuf>, output: impl Into<PathBuf>, fps: f64) -> Self {
        Self {
            frames_dir: frames_dir.into(),
            output: output.into(),
            fps,
            crf: 18,
            preset: "fast".to_string(),
            codec: "libx264".to_string(),
            pixel_format: "yuv420p".to_string(),
            width: None,
            height: None,
            movflags: "+faststart".to_string(),
            cleanup_after_encode: false,
        }
    }

    /// Set video resolution width and height.
    pub fn with_resolution(mut self, width: u32, height: u32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    /// Set encoding preset.
    pub fn with_preset(mut self, preset: impl Into<String>) -> Self {
        self.preset = preset.into();
        self
    }

    /// Enable or disable cleanup of frame directory after encoding.
    pub fn with_cleanup(mut self, cleanup: bool) -> Self {
        self.cleanup_after_encode = cleanup;
        self
    }
}

/// Constructs FFmpeg CLI argument list from an [`EncodeConfig`].
pub fn build_ffmpeg_args(config: &EncodeConfig) -> Vec<String> {
    let pattern_file = if config.frames_dir.join("frame-000000.png").exists() {
        "frame-%06d.png"
    } else {
        "frame_%06d.png"
    };

    let input_pattern = config
        .frames_dir
        .join(pattern_file)
        .to_string_lossy()
        .to_string();

    let mut args = vec![
        "-y".to_string(),
        "-framerate".to_string(),
        config.fps.to_string(),
        "-i".to_string(),
        input_pattern,
        "-c:v".to_string(),
        config.codec.clone(),
        "-crf".to_string(),
        config.crf.to_string(),
        "-preset".to_string(),
        config.preset.clone(),
        "-pix_fmt".to_string(),
        config.pixel_format.clone(),
    ];

    if let (Some(w), Some(h)) = (config.width, config.height) {
        args.push("-s".to_string());
        args.push(format!("{w}x{h}"));
    }

    args.push("-vf".to_string());
    args.push("scale=trunc(iw/2)*2:trunc(ih/2)*2".to_string());

    if !config.movflags.is_empty() {
        args.push("-movflags".to_string());
        args.push(config.movflags.clone());
    }

    args.push(config.output.to_string_lossy().to_string());
    args
}

/// Encode rendered PNG frames into an MP4 video file using FFmpeg.
///
/// Executes the command:
/// `ffmpeg -y -framerate <fps> -i <frames_dir>/frame_%06d.png -c:v libx264 -crf 18 -preset fast -pix_fmt yuv420p -s <width>x<height> -movflags +faststart <output_mp4>`
///
/// Logs invocation details and captures stderr output using `tracing`.
pub async fn encode_mp4(config: &EncodeConfig) -> Result<(), RenderError> {
    if config.codec == "pure-rust" || config.codec == "h264enc" {
        tracing::info!("Using zero-dependency pure-Rust H.264 encoder");
        return encode_h264_pure_rust(config);
    }

    let args = build_ffmpeg_args(config);
    let cmd_line = format!("ffmpeg {}", args.join(" "));

    tracing::info!("Invoking FFmpeg CLI: {cmd_line}");

    let res = tokio::process::Command::new("ffmpeg")
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await;

    let output = match res {
        Ok(out) => out,
        Err(e) => {
            tracing::warn!("FFmpeg not available ({e}), falling back to pure-Rust H.264 encoder");
            return encode_h264_pure_rust(config);
        }
    };

    let stderr_text = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        tracing::warn!(
            "FFmpeg failed ({}). Falling back to pure-Rust H.264 encoder...\nStderr:\n{}",
            output.status,
            stderr_text
        );
        return encode_h264_pure_rust(config);
    }

    if !stderr_text.is_empty() {
        tracing::info!("FFmpeg output:\n{}", stderr_text);
    }
    tracing::info!("Encode complete → {}", config.output.display());

    if config.cleanup_after_encode {
        cleanup_frames(&config.frames_dir)?;
    }

    Ok(())
}

/// Pure-Rust standalone H.264 encoder implementation (zero external FFmpeg dependencies).
/// Converts rendered PNG frames to YUV420p and encodes them via `rust_motion_h264enc`.
pub fn encode_h264_pure_rust(config: &EncodeConfig) -> Result<(), RenderError> {
    use rust_motion_h264enc::{Encoder, EncoderConfig, RateControl, YuvFrame};
    use std::fs::File;
    use std::io::Write;

    let mut frame_paths = Vec::new();
    let entries = std::fs::read_dir(&config.frames_dir)
        .map_err(|e| RenderError::Encode(format!("Failed to read frames dir: {e}")))?;
    for entry in entries {
        let entry = entry.map_err(|e| RenderError::Encode(e.to_string()))?;
        let p = entry.path();
        if p.extension().map_or(false, |ext| ext.eq_ignore_ascii_case("png")) {
            frame_paths.push(p);
        }
    }
    frame_paths.sort();
    if frame_paths.is_empty() {
        return Err(RenderError::Encode("No PNG frames found to encode".into()));
    }

    let first_img = image::open(&frame_paths[0])
        .map_err(|e| RenderError::Encode(format!("Failed to open image {:?}: {e}", frame_paths[0])))?
        .to_rgba8();
    let width = config.width.unwrap_or(first_img.width());
    let height = config.height.unwrap_or(first_img.height());
    // H.264 macroblocks require even dimensions
    let width = (width / 2) * 2;
    let height = (height / 2) * 2;

    let fps_round = config.fps.round().max(1.0) as u32;
    let mut enc_cfg = EncoderConfig::new(width, height, fps_round, 1);
    enc_cfg.rate = RateControl::Crf(config.crf as f32);

    let mut encoder = Encoder::new(enc_cfg)
        .map_err(|e| RenderError::Encode(format!("Failed to initialize H.264 encoder: {e}")))?;

    let mut out_file = File::create(&config.output)
        .map_err(|e| RenderError::Encode(format!("Failed to create output file: {e}")))?;

    let y_stride = width as usize;
    let uv_stride = (width as usize) / 2;
    let y_len = y_stride * (height as usize);
    let uv_len = uv_stride * ((height as usize) / 2);

    let mut y_plane = vec![0u8; y_len];
    let mut u_plane = vec![0u8; uv_len];
    let mut v_plane = vec![0u8; uv_len];

    for (f_idx, path) in frame_paths.iter().enumerate() {
        let raw_img = image::open(path)
            .map_err(|e| RenderError::Encode(format!("Failed to open frame {:?}: {e}", path)))?
            .to_rgba8();

        let img = if raw_img.width() != width || raw_img.height() != height {
            image::imageops::resize(&raw_img, width, height, image::imageops::FilterType::Nearest)
        } else {
            raw_img
        };

        for r in 0..height as usize {
            for c in 0..width as usize {
                let p = img.get_pixel(c as u32, r as u32);
                let (r_val, g_val, b_val) = (p[0] as f32, p[1] as f32, p[2] as f32);
                let y_val = 0.299 * r_val + 0.587 * g_val + 0.114 * b_val;
                y_plane[r * y_stride + c] = y_val.clamp(0.0, 255.0) as u8;

                if r % 2 == 0 && c % 2 == 0 {
                    let u_val = -0.169 * r_val - 0.331 * g_val + 0.500 * b_val + 128.0;
                    let v_val = 0.500 * r_val - 0.419 * g_val - 0.081 * b_val + 128.0;
                    let uv_idx = (r / 2) * uv_stride + (c / 2);
                    u_plane[uv_idx] = u_val.clamp(0.0, 255.0) as u8;
                    v_plane[uv_idx] = v_val.clamp(0.0, 255.0) as u8;
                }
            }
        }

        let yuv = YuvFrame {
            y: &y_plane,
            u: &u_plane,
            v: &v_plane,
            y_stride,
            uv_stride,
        };

        let packets = encoder.encode(&yuv, f_idx as i64)
            .map_err(|e| RenderError::Encode(format!("H.264 encode error: {e}")))?;
        for pkt in packets {
            out_file.write_all(&pkt.data)
                .map_err(|e| RenderError::Encode(e.to_string()))?;
        }
    }

    let flushed = encoder.flush();
    for pkt in flushed {
        out_file.write_all(&pkt.data)
            .map_err(|e| RenderError::Encode(e.to_string()))?;
    }
    out_file.flush()
        .map_err(|e| RenderError::Encode(e.to_string()))?;

    if config.cleanup_after_encode {
        cleanup_frames(&config.frames_dir)?;
    }

    tracing::info!("Pure Rust H.264 encode complete -> {}", config.output.display());
    Ok(())
}

/// Encode rendered frames into a video file using FFmpeg.
pub async fn encode_frames(config: &EncodeConfig) -> Result<(), RenderError> {
    encode_mp4(config).await
}

/// Remove temporary frame directory and its contents.
pub fn cleanup_frames(frames_dir: impl AsRef<Path>) -> Result<(), RenderError> {
    let path = frames_dir.as_ref();
    if path.exists() {
        tracing::info!("Cleaning up temporary frames directory: {}", path.display());
        std::fs::remove_dir_all(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const MINIMAL_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x08, 0x08, 0x02, 0x00, 0x00, 0x00, 0x4b,
        0x6d, 0x29, 0xdc, 0x00, 0x00, 0x00, 0x12, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8,
        0xcf, 0xc0, 0x80, 0x15, 0x61, 0x17, 0x1d, 0xb4, 0x12, 0x00, 0x28, 0xff, 0x3f, 0xc1, 0x6e,
        0xec, 0xdf, 0x61, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn test_build_ffmpeg_args_default() {
        let frames_dir = PathBuf::from("/tmp/frames");
        let output = PathBuf::from("/tmp/output.mp4");
        let config = EncodeConfig::h264(&frames_dir, &output, 30.0);

        let args = build_ffmpeg_args(&config);
        let input_pattern = frames_dir.join("frame_%06d.png");
        let input_pattern = input_pattern.to_string_lossy();
        assert_eq!(
            args,
            vec![
                "-y",
                "-framerate",
                "30",
                "-i",
                input_pattern.as_ref(),
                "-c:v",
                "libx264",
                "-crf",
                "18",
                "-preset",
                "fast",
                "-pix_fmt",
                "yuv420p",
                "-vf",
                "scale=trunc(iw/2)*2:trunc(ih/2)*2",
                "-movflags",
                "+faststart",
                "/tmp/output.mp4"
            ]
        );
    }

    #[test]
    fn test_build_ffmpeg_args_with_resolution() {
        let frames_dir = PathBuf::from("/tmp/frames");
        let output = PathBuf::from("/tmp/output.mp4");
        let config = EncodeConfig::h264(&frames_dir, &output, 60.0)
            .with_resolution(1920, 1080)
            .with_preset("medium");

        let args = build_ffmpeg_args(&config);
        let input_pattern = frames_dir.join("frame_%06d.png");
        let input_pattern = input_pattern.to_string_lossy();
        assert_eq!(
            args,
            vec![
                "-y",
                "-framerate",
                "60",
                "-i",
                input_pattern.as_ref(),
                "-c:v",
                "libx264",
                "-crf",
                "18",
                "-preset",
                "medium",
                "-pix_fmt",
                "yuv420p",
                "-s",
                "1920x1080",
                "-vf",
                "scale=trunc(iw/2)*2:trunc(ih/2)*2",
                "-movflags",
                "+faststart",
                "/tmp/output.mp4"
            ]
        );
    }

    #[test]
    fn test_cleanup_frames() {
        let temp_dir = std::env::temp_dir().join(format!(
            "rust_motion_test_cleanup_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&temp_dir).unwrap();
        let frame1 = temp_dir.join("frame_000000.png");
        fs::write(&frame1, b"fake_png_data").unwrap();
        assert!(frame1.exists());

        cleanup_frames(&temp_dir).expect("cleanup_frames failed");
        assert!(!temp_dir.exists());
    }

    #[tokio::test]
    async fn test_encode_mp4_synthetic_frames() {
        let temp_dir = std::env::temp_dir().join(format!(
            "rust_motion_test_encode_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let frames_dir = temp_dir.join("frames");
        fs::create_dir_all(&frames_dir).unwrap();

        for idx in 0..10 {
            let path = frames_dir.join(format!("frame_{idx:06}.png"));
            fs::write(&path, MINIMAL_PNG).expect("Failed to write synthetic PNG frame");
        }

        let output_mp4 = temp_dir.join("output.mp4");
        let config = EncodeConfig::h264(&frames_dir, &output_mp4, 30.0)
            .with_resolution(320, 240)
            .with_cleanup(true);

        let res = encode_mp4(&config).await;

        if let Err(ref e) = res {
            eprintln!("encode_mp4 failed: {e:?}");
        }

        assert!(res.is_ok(), "encode_mp4 failed on synthetic frames");
        assert!(output_mp4.exists(), "Output MP4 file was not created");
        assert!(!frames_dir.exists(), "Frames directory was not cleaned up");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_encode_pure_rust_synthetic_frames() {
        let temp_dir = std::env::temp_dir().join(format!(
            "rust_motion_test_purerust_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let frames_dir = temp_dir.join("frames");
        fs::create_dir_all(&frames_dir).unwrap();

        for idx in 0..6 {
            let path = frames_dir.join(format!("frame_{idx:06}.png"));
            fs::write(&path, MINIMAL_PNG).expect("Failed to write synthetic PNG frame");
        }

        let output_h264 = temp_dir.join("output.h264");
        let mut config = EncodeConfig::h264(&frames_dir, &output_h264, 30.0)
            .with_resolution(64, 64)
            .with_cleanup(true);
        config.codec = "pure-rust".to_string();

        let res = encode_mp4(&config).await;
        assert!(res.is_ok(), "encode_mp4 pure-rust failed: {res:?}");
        assert!(output_h264.exists(), "Output pure-rust H.264 file was not created");
        assert!(output_h264.metadata().unwrap().len() > 0, "Output file is empty");

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
