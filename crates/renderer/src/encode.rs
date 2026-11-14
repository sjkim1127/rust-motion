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

    // FFmpeg converts RGB PNGs with BT.601 by default. Convert with BT.709 / limited range
    // instead and tag the stream accordingly, matching the pure-Rust fallback encoder. Only
    // applies to limited-range YUV outputs (`yuvj*` is full range, RGB has no matrix).
    let bt709_yuv =
        config.pixel_format.starts_with("yuv") && !config.pixel_format.starts_with("yuvj");
    args.push("-vf".to_string());
    if bt709_yuv {
        // `setparams` is needed for the tags to reach the encoder; the `-color_*` output
        // flags alone leave primaries/transfer unset.
        args.push(
            "scale=trunc(iw/2)*2:trunc(ih/2)*2:out_color_matrix=bt709:out_range=tv,\
             setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv"
                .to_string(),
        );
    } else {
        args.push("scale=trunc(iw/2)*2:trunc(ih/2)*2".to_string());
    }

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
            return fallback_encode(config, &format!("FFmpeg not available: {e}"));
        }
    };

    let stderr_text = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        tracing::warn!(
            "FFmpeg failed ({}). Falling back to pure-Rust H.264 encoder...\nStderr:\n{}",
            output.status,
            stderr_text
        );
        return fallback_encode(config, &format!("FFmpeg failed ({})", output.status));
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

/// Codec names the pure-Rust fallback can honor (it only produces H.264).
fn fallback_supports_codec(codec: &str) -> bool {
    matches!(
        codec.to_ascii_lowercase().as_str(),
        "libx264" | "h264" | "pure-rust" | "h264enc"
    )
}

/// Run the pure-Rust encoder after FFmpeg was unavailable or failed, refusing to silently
/// produce a different codec than the one requested.
fn fallback_encode(config: &EncodeConfig, reason: &str) -> Result<(), RenderError> {
    if !fallback_supports_codec(&config.codec) {
        return Err(RenderError::Encode(format!(
            "{reason}; the pure-Rust fallback only supports H.264 but codec '{}' was requested",
            config.codec
        )));
    }
    encode_h264_pure_rust(config)
}

/// Convert a frame rate to an exact `num/den` pair (e.g. 29.97 -> 30000/1001).
pub(crate) fn fps_to_ratio(fps: f64) -> (u32, u32) {
    fn gcd(a: u32, b: u32) -> u32 {
        if b == 0 {
            a
        } else {
            gcd(b, a % b)
        }
    }
    if !fps.is_finite() || fps <= 0.0 {
        return (30, 1);
    }
    let rounded = fps.round().max(1.0);
    if (fps - rounded).abs() < 1e-3 {
        return (rounded as u32, 1);
    }
    // NTSC-style rates: N * 1000 / 1001.
    let ntsc = (fps * 1.001).round();
    if ntsc >= 1.0 && (fps - ntsc * 1000.0 / 1001.0).abs() < 1e-3 {
        return (ntsc as u32 * 1000, 1001);
    }
    let num = (fps * 1000.0).round().clamp(1.0, u32::MAX as f64) as u32;
    let g = gcd(num, 1000);
    (num / g, 1000 / g)
}

fn map_preset(preset: &str) -> rust_motion_h264enc::Preset {
    use rust_motion_h264enc::Preset;
    match preset.to_ascii_lowercase().as_str() {
        "ultrafast" | "superfast" | "veryfast" | "faster" | "fast" => Preset::Speed,
        "slow" | "slower" | "veryslow" | "placebo" => Preset::Quality,
        _ => Preset::Balanced,
    }
}

/// Convert an RGBA image (composited over black) to planar BT.709 limited-range YUV 4:2:0,
/// matching the colour description the encoder writes to the VUI. Chroma is the average of
/// each 2x2 block. `width` and `height` must be even and equal the image size.
fn rgba_to_yuv420(
    img: &image::RgbaImage,
    y_plane: &mut [u8],
    u_plane: &mut [u8],
    v_plane: &mut [u8],
) {
    let width = img.width() as usize;
    let height = img.height() as usize;
    let uv_stride = width / 2;
    let raw = img.as_raw();

    let rgb = |x: usize, y: usize| -> (f32, f32, f32) {
        let i = (y * width + x) * 4;
        let a = raw[i + 3] as f32 / 255.0;
        (
            raw[i] as f32 * a,
            raw[i + 1] as f32 * a,
            raw[i + 2] as f32 * a,
        )
    };

    for y in 0..height {
        for x in 0..width {
            let (r, g, b) = rgb(x, y);
            let luma = 16.0 + (0.2126 * r + 0.7152 * g + 0.0722 * b) * (219.0 / 255.0);
            y_plane[y * width + x] = luma.round().clamp(16.0, 235.0) as u8;
        }
    }
    for cy in 0..height / 2 {
        for cx in 0..uv_stride {
            let mut sum = (0.0f32, 0.0f32, 0.0f32);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (r, g, b) = rgb(cx * 2 + dx, cy * 2 + dy);
                sum.0 += r;
                sum.1 += g;
                sum.2 += b;
            }
            let (r, g, b) = (sum.0 / 4.0, sum.1 / 4.0, sum.2 / 4.0);
            let k = 224.0 / 255.0;
            let cb = 128.0 + (-0.1146 * r - 0.3854 * g + 0.5 * b) * k;
            let cr = 128.0 + (0.5 * r - 0.4542 * g - 0.0458 * b) * k;
            u_plane[cy * uv_stride + cx] = cb.round().clamp(16.0, 240.0) as u8;
            v_plane[cy * uv_stride + cx] = cr.round().clamp(16.0, 240.0) as u8;
        }
    }
}

/// Pure-Rust standalone H.264 encoder implementation (zero external FFmpeg dependencies).
/// Converts rendered PNG frames to BT.709 limited-range YUV 4:2:0 and encodes them via
/// `rust_motion_h264enc`.
///
/// The output container follows the file extension: `.h264` / `.264` get a raw Annex-B
/// elementary stream, everything else (e.g. `.mp4`, `.mov`) a real MP4 container. The
/// requested preset and CRF are honored and fractional frame rates are kept exact. Only
/// H.264 / yuv420p can be produced; other pixel formats are ignored with a warning.
pub fn encode_h264_pure_rust(config: &EncodeConfig) -> Result<(), RenderError> {
    use rust_motion_h264enc::{Encoder, EncoderConfig, PacketFormat, RateControl, YuvFrame};

    if !fallback_supports_codec(&config.codec) {
        return Err(RenderError::Encode(format!(
            "pure-Rust encoder only supports H.264 but codec '{}' was requested",
            config.codec
        )));
    }
    if !config.pixel_format.eq_ignore_ascii_case("yuv420p") {
        tracing::warn!(
            "pure-Rust encoder only supports yuv420p; ignoring pixel format '{}'",
            config.pixel_format
        );
    }

    let mut frame_paths = Vec::new();
    let entries = std::fs::read_dir(&config.frames_dir)
        .map_err(|e| RenderError::Encode(format!("Failed to read frames dir: {e}")))?;
    for entry in entries {
        let entry = entry.map_err(|e| RenderError::Encode(e.to_string()))?;
        let p = entry.path();
        if p.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
        {
            frame_paths.push(p);
        }
    }
    frame_paths.sort();
    if frame_paths.is_empty() {
        return Err(RenderError::Encode("No PNG frames found to encode".into()));
    }

    let first_img = image::open(&frame_paths[0])
        .map_err(|e| {
            RenderError::Encode(format!("Failed to open image {:?}: {e}", frame_paths[0]))
        })?
        .to_rgba8();
    let width = config.width.unwrap_or(first_img.width());
    let height = config.height.unwrap_or(first_img.height());
    // H.264 macroblocks require even dimensions
    let width = (width / 2) * 2;
    let height = (height / 2) * 2;

    let raw_stream = config
        .output
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "h264" | "264"));

    let (fps_num, fps_den) = fps_to_ratio(config.fps);
    let mut enc_cfg = EncoderConfig::new(width, height, fps_num, fps_den);
    enc_cfg.rate = RateControl::Crf(config.crf as f32);
    enc_cfg.preset = map_preset(&config.preset);
    // No B-frame reordering: keeps dts == pts so the MP4 needs no composition offsets.
    enc_cfg.bframes = 0;
    enc_cfg.format = if raw_stream {
        PacketFormat::AnnexB
    } else {
        PacketFormat::LengthPrefixed
    };

    let mut encoder = Encoder::new(enc_cfg)
        .map_err(|e| RenderError::Encode(format!("Failed to initialize H.264 encoder: {e}")))?;

    let y_stride = width as usize;
    let uv_stride = (width as usize) / 2;
    let y_len = y_stride * (height as usize);
    let uv_len = uv_stride * ((height as usize) / 2);

    let mut y_plane = vec![0u8; y_len];
    let mut u_plane = vec![0u8; uv_len];
    let mut v_plane = vec![0u8; uv_len];

    let mut packets = Vec::new();
    for (f_idx, path) in frame_paths.iter().enumerate() {
        let raw_img = image::open(path)
            .map_err(|e| RenderError::Encode(format!("Failed to open frame {:?}: {e}", path)))?
            .to_rgba8();

        let img = if raw_img.width() != width || raw_img.height() != height {
            image::imageops::resize(
                &raw_img,
                width,
                height,
                image::imageops::FilterType::Nearest,
            )
        } else {
            raw_img
        };

        rgba_to_yuv420(&img, &mut y_plane, &mut u_plane, &mut v_plane);

        let yuv = YuvFrame {
            y: &y_plane,
            u: &u_plane,
            v: &v_plane,
            y_stride,
            uv_stride,
        };

        packets.extend(
            encoder
                .encode(&yuv, f_idx as i64)
                .map_err(|e| RenderError::Encode(format!("H.264 encode error: {e}")))?,
        );
    }
    packets.extend(encoder.flush());

    if raw_stream {
        let mut bytes = Vec::new();
        for pkt in &packets {
            bytes.extend_from_slice(&pkt.data);
        }
        std::fs::write(&config.output, bytes)
            .map_err(|e| RenderError::Encode(format!("Failed to write output file: {e}")))?;
    } else {
        let avcc = encoder.avcc();
        let samples: Vec<crate::mp4::Sample> = packets
            .into_iter()
            .map(|p| crate::mp4::Sample {
                data: p.data,
                keyframe: p.keyframe,
            })
            .collect();
        let file = crate::mp4::mux(
            &crate::mp4::Mp4Params {
                width,
                height,
                fps_num,
                fps_den,
                avcc: &avcc,
                faststart: config.movflags.contains("faststart"),
            },
            &samples,
        )
        .map_err(|e| RenderError::Encode(format!("MP4 mux error: {e}")))?;
        std::fs::write(&config.output, file)
            .map_err(|e| RenderError::Encode(format!("Failed to write output file: {e}")))?;
    }

    if config.cleanup_after_encode {
        cleanup_frames(&config.frames_dir)?;
    }

    tracing::info!(
        "Pure Rust H.264 encode complete -> {}",
        config.output.display()
    );
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
                "scale=trunc(iw/2)*2:trunc(ih/2)*2:out_color_matrix=bt709:out_range=tv,setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv",
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
                "scale=trunc(iw/2)*2:trunc(ih/2)*2:out_color_matrix=bt709:out_range=tv,setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv",
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
        assert!(
            output_h264.exists(),
            "Output pure-rust H.264 file was not created"
        );
        assert!(
            output_h264.metadata().unwrap().len() > 0,
            "Output file is empty"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    fn synthetic_frames(prefix: &str, count: usize) -> (PathBuf, PathBuf) {
        let temp_dir = std::env::temp_dir().join(format!(
            "rust_motion_test_{prefix}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let frames_dir = temp_dir.join("frames");
        fs::create_dir_all(&frames_dir).unwrap();
        for idx in 0..count {
            fs::write(frames_dir.join(format!("frame_{idx:06}.png")), MINIMAL_PNG).unwrap();
        }
        (temp_dir, frames_dir)
    }

    #[test]
    fn pure_rust_fallback_writes_a_real_mp4_container() {
        let (temp_dir, frames_dir) = synthetic_frames("fallback_mp4", 6);
        let output = temp_dir.join("out.mp4");
        let config = EncodeConfig::h264(&frames_dir, &output, 29.97).with_resolution(64, 64);

        encode_h264_pure_rust(&config).expect("fallback encode failed");

        let bytes = fs::read(&output).unwrap();
        assert_eq!(&bytes[4..8], b"ftyp", "fallback output is not an MP4");
        assert!(bytes.windows(4).any(|w| w == b"moov"));
        assert!(bytes.windows(4).any(|w| w == b"mdat"));
        assert!(bytes.windows(4).any(|w| w == b"avcC"));
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn pure_rust_fallback_keeps_raw_stream_for_h264_extension() {
        let (temp_dir, frames_dir) = synthetic_frames("fallback_raw", 3);
        let output = temp_dir.join("out.h264");
        let config = EncodeConfig::h264(&frames_dir, &output, 30.0).with_resolution(64, 64);

        encode_h264_pure_rust(&config).unwrap();

        let bytes = fs::read(&output).unwrap();
        assert!(
            bytes.starts_with(&[0, 0, 0, 1]) || bytes.starts_with(&[0, 0, 1]),
            "expected Annex-B start code"
        );
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn pure_rust_fallback_rejects_non_h264_codec() {
        let (temp_dir, frames_dir) = synthetic_frames("fallback_codec", 2);
        let output = temp_dir.join("out.mp4");
        let mut config = EncodeConfig::h264(&frames_dir, &output, 30.0).with_resolution(64, 64);
        config.codec = "libx265".to_string();

        assert!(fallback_encode(&config, "ffmpeg missing").is_err());
        assert!(encode_h264_pure_rust(&config).is_err());
        assert!(!output.exists());
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn fps_ratio_is_exact() {
        assert_eq!(fps_to_ratio(30.0), (30, 1));
        assert_eq!(fps_to_ratio(60.0), (60, 1));
        assert_eq!(fps_to_ratio(29.97), (30000, 1001));
        assert_eq!(fps_to_ratio(23.976), (24000, 1001));
        assert_eq!(fps_to_ratio(59.94), (60000, 1001));
        assert_eq!(fps_to_ratio(12.5), (25, 2));
        assert_eq!(fps_to_ratio(0.0), (30, 1));
        assert_eq!(fps_to_ratio(f64::NAN), (30, 1));
    }

    #[test]
    fn yuv_conversion_is_bt709_limited_range_with_chroma_average() {
        let mut img = image::RgbaImage::new(2, 2);
        // Black, white, transparent (-> black), white: luma per pixel, averaged chroma.
        img.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));
        img.put_pixel(1, 0, image::Rgba([255, 255, 255, 255]));
        img.put_pixel(0, 1, image::Rgba([255, 0, 0, 0]));
        img.put_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
        let (mut y, mut u, mut v) = (vec![0u8; 4], vec![0u8; 1], vec![0u8; 1]);
        rgba_to_yuv420(&img, &mut y, &mut u, &mut v);
        assert_eq!(y, vec![16, 235, 16, 235]);
        // Grey average (127.5 each) has no chroma.
        assert_eq!((u[0], v[0]), (128, 128));

        let mut red = image::RgbaImage::new(2, 2);
        for p in red.pixels_mut() {
            *p = image::Rgba([255, 0, 0, 255]);
        }
        rgba_to_yuv420(&red, &mut y, &mut u, &mut v);
        assert_eq!(y[0], 63); // 16 + 219 * 0.2126
        assert!(v[0] > 200 && u[0] < 128);
    }

    #[test]
    fn test_build_ffmpeg_args_color_tagging_only_for_limited_range_yuv() {
        let mk = |fmt: &str| {
            let mut c = EncodeConfig::h264("/tmp/frames", "/tmp/o.mp4", 30.0);
            c.pixel_format = fmt.to_string();
            build_ffmpeg_args(&c)
        };
        let tagged = |args: &Vec<String>| args.iter().any(|a| a.contains("setparams="));
        assert!(tagged(&mk("yuv420p")));
        assert!(tagged(&mk("yuv444p")));
        for fmt in ["yuvj420p", "rgb24", "gbrp"] {
            let args = mk(fmt);
            assert!(!args.iter().any(|a| a.contains("setparams=")), "{fmt}");
            assert!(args.contains(&"scale=trunc(iw/2)*2:trunc(ih/2)*2".to_string()));
        }
    }
}
