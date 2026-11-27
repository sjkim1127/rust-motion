//! Media metadata and asset resolution utilities — Remotion parity (`getVideoMetadata`, `staticFile`).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Error during media inspection or asset resolution.
#[derive(Debug, thiserror::Error)]
pub enum MediaMetadataError {
    #[error("File not found: {0}")]
    FileNotFound(String),
    #[error("Failed to execute ffprobe: {0}")]
    FfprobeExecution(String),
    #[error("Failed to parse ffprobe output: {0}")]
    FfprobeParse(String),
    #[error("Audio decode error: {0}")]
    AudioDecode(String),
    #[error("Image decode error: {0}")]
    ImageDecode(String),
    #[error("Invalid media byte range: {0}")]
    InvalidRange(String),
}

/// Pixel dimensions for a still image, matching Remotion's
/// `getImageDimensions()` contract for native hosts.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageDimensions {
    pub width: u32,
    pub height: u32,
}

/// Reads image dimensions without decoding the full pixel buffer.
pub fn get_image_dimensions(path: impl AsRef<Path>) -> Result<ImageDimensions, MediaMetadataError> {
    let path_ref = path.as_ref();
    if !path_ref.exists() {
        return Err(MediaMetadataError::FileNotFound(
            path_ref.display().to_string(),
        ));
    }
    let (width, height) = image::image_dimensions(path_ref)
        .map_err(|error| MediaMetadataError::ImageDecode(error.to_string()))?;
    Ok(ImageDimensions { width, height })
}

/// Video stream and container metadata matching Remotion's `getVideoMetadata()`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VideoMetadata {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_in_seconds: f64,
    pub duration_in_frames: u32,
    pub aspect_ratio: f64,
    pub is_landscape: bool,
}

/// Audio track metadata matching Remotion's `getAudioData()`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioMetadata {
    pub channels: u16,
    pub sample_rate: u32,
    pub duration_in_seconds: f64,
    pub duration_in_frames: u32,
}

/// Unified metadata result used by the native equivalent of Remotion
/// `parseMedia()`. Each track is optional because still images and silent
/// videos are valid media sources.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParsedMediaMetadata {
    pub duration_in_seconds: f64,
    pub video: Option<VideoMetadata>,
    pub audio: Option<AudioMetadata>,
    pub image: Option<ImageDimensions>,
}

fn max_known_track_duration(video: Option<&VideoMetadata>, audio: Option<&AudioMetadata>) -> f64 {
    let video_duration = video
        .map(|metadata| metadata.duration_in_seconds)
        .filter(|duration| duration.is_finite() && *duration >= 0.0);
    let audio_duration = audio
        .map(|metadata| metadata.duration_in_seconds)
        .filter(|duration| duration.is_finite() && *duration >= 0.0);

    match (video_duration, audio_duration) {
        (Some(video), Some(audio)) => video.max(audio),
        (Some(duration), None) | (None, Some(duration)) => duration,
        (None, None) => 0.0,
    }
}

/// Maximum single range read used by media parsers to avoid accidental whole
/// file allocations when inspecting untrusted assets.
pub const MAX_MEDIA_RANGE_BYTES: u64 = 16 * 1024 * 1024;

/// Read a bounded half-open byte range from a local media source.
pub fn read_media_range(
    path: impl AsRef<Path>,
    start: u64,
    end_exclusive: u64,
) -> Result<Vec<u8>, MediaMetadataError> {
    if start > end_exclusive {
        return Err(MediaMetadataError::InvalidRange(format!(
            "start {start} is greater than end {end_exclusive}"
        )));
    }
    let length = end_exclusive - start;
    if length > MAX_MEDIA_RANGE_BYTES {
        return Err(MediaMetadataError::InvalidRange(format!(
            "range length {length} exceeds {MAX_MEDIA_RANGE_BYTES} bytes"
        )));
    }
    let path_ref = path.as_ref();
    let mut file = std::fs::File::open(path_ref).map_err(|error| {
        MediaMetadataError::FileNotFound(format!("{}: {error}", path_ref.display()))
    })?;
    use std::io::{Read, Seek, SeekFrom};
    file.seek(SeekFrom::Start(start))
        .map_err(|error| MediaMetadataError::FfprobeExecution(error.to_string()))?;
    let mut bytes = vec![0; length as usize];
    file.read_exact(&mut bytes)
        .map_err(|error| MediaMetadataError::InvalidRange(error.to_string()))?;
    Ok(bytes)
}

/// Inspect a media source once and return the metadata fields available for
/// its container. Existing bounded image and ffprobe probes remain the
/// canonical implementations for each track.
pub fn parse_media(
    path: impl AsRef<Path>,
    fps: f64,
) -> Result<ParsedMediaMetadata, MediaMetadataError> {
    let path_ref = path.as_ref();
    if !path_ref.exists() {
        return Err(MediaMetadataError::FileNotFound(
            path_ref.display().to_string(),
        ));
    }
    if let Ok(image) = get_image_dimensions(path_ref) {
        return Ok(ParsedMediaMetadata {
            duration_in_seconds: 0.0,
            video: None,
            audio: None,
            image: Some(image),
        });
    }
    if let Ok(meta) = rust_motion_media_container::probe_isobmff_path(path_ref) {
        let video = video_metadata_from_container(&meta);
        let audio = audio_metadata_from_container(&meta, fps);
        if video.is_some() || audio.is_some() {
            let duration_in_seconds = max_known_track_duration(video.as_ref(), audio.as_ref());
            return Ok(ParsedMediaMetadata {
                duration_in_seconds,
                video,
                audio,
                image: None,
            });
        }
    }
    let video = get_video_metadata(path_ref).ok();
    let audio = get_audio_metadata(path_ref, fps).ok();
    if video.is_none() && audio.is_none() {
        return Err(MediaMetadataError::FfprobeParse(format!(
            "unsupported media source: {}",
            path_ref.display()
        )));
    }
    let duration_in_seconds = max_known_track_duration(video.as_ref(), audio.as_ref());
    Ok(ParsedMediaMetadata {
        duration_in_seconds,
        video,
        audio,
        image: None,
    })
}

/// Resolves an asset path relative to the public/assets directory, matching Remotion's `staticFile()`.
///
/// Looks in:
/// 1. `RUST_MOTION_PUBLIC_DIR` environment variable if set.
/// 2. `./public/<path>`
/// 3. `./assets/<path>`
/// 4. `./<path>`
pub fn static_file(relative_path: impl AsRef<Path>) -> Result<PathBuf, MediaMetadataError> {
    let rel = relative_path.as_ref();
    if rel.is_absolute() && rel.exists() {
        return Ok(rel.to_path_buf());
    }

    let candidates = [
        std::env::var("RUST_MOTION_PUBLIC_DIR")
            .ok()
            .map(PathBuf::from),
        Some(PathBuf::from("public")),
        Some(PathBuf::from("assets")),
        Some(PathBuf::from(".")),
    ];

    for candidate in candidates.into_iter().flatten() {
        let joined = candidate.join(rel);
        if joined.exists() {
            return Ok(joined);
        }
    }

    Err(MediaMetadataError::FileNotFound(rel.display().to_string()))
}

fn video_metadata_from_container(
    meta: &rust_motion_media_core::MediaMetadata,
) -> Option<VideoMetadata> {
    let video = meta.video_streams.first()?;
    if video.width == 0 || video.height == 0 {
        return None;
    }
    let (width, height) = match video.rotation_degrees {
        90 | 270 => (video.height, video.width),
        _ => (video.width, video.height),
    };
    let fps = {
        let rate = video.fps.as_f64();
        if rate.is_finite() && rate > 0.0 {
            rate
        } else {
            30.0
        }
    };
    let duration_in_seconds = video
        .duration_secs
        .or(meta.duration_secs)
        .filter(|d| d.is_finite() && *d >= 0.0)
        .unwrap_or(0.0);
    let duration_in_frames = (duration_in_seconds * fps).round() as u32;
    let aspect_ratio = width as f64 / height as f64;
    Some(VideoMetadata {
        width,
        height,
        fps,
        duration_in_seconds,
        duration_in_frames,
        aspect_ratio,
        is_landscape: width >= height,
    })
}

fn audio_metadata_from_container(
    meta: &rust_motion_media_core::MediaMetadata,
    fps: f64,
) -> Option<AudioMetadata> {
    let audio = meta.audio_streams.first()?;
    let duration_secs = audio
        .duration_secs
        .or(meta.duration_secs)
        .filter(|d| d.is_finite() && *d >= 0.0)
        .unwrap_or(0.0);
    Some(AudioMetadata {
        channels: audio.channels,
        sample_rate: audio.sample_rate,
        duration_in_seconds: duration_secs,
        duration_in_frames: (duration_secs * fps).round() as u32,
    })
}

/// Probe container/stream metadata natively first, falling back to `ffprobe` for formats
/// not yet supported by native container parsers (e.g. WebM/MKV).
pub fn get_video_metadata(path: impl AsRef<Path>) -> Result<VideoMetadata, MediaMetadataError> {
    let path_ref = path.as_ref();
    if !path_ref.exists() {
        return Err(MediaMetadataError::FileNotFound(
            path_ref.display().to_string(),
        ));
    }

    if let Ok(meta) = rust_motion_media_container::probe_isobmff_path(path_ref) {
        if let Some(video) = video_metadata_from_container(&meta) {
            return Ok(video);
        }
    }

    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,avg_frame_rate,r_frame_rate,duration",
            "-show_entries",
            "format=duration",
            "-of",
            "json",
        ])
        .arg(path_ref)
        .output()
        .map_err(|e| MediaMetadataError::FfprobeExecution(e.to_string()))?;

    if !output.status.success() {
        return Err(MediaMetadataError::FfprobeExecution(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    let json_val: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| MediaMetadataError::FfprobeParse(e.to_string()))?;

    let stream = json_val
        .get("streams")
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.first())
        .ok_or_else(|| MediaMetadataError::FfprobeParse("No video stream found".into()))?;

    let width = stream.get("width").and_then(|w| w.as_u64()).unwrap_or(1920) as u32;

    let height = stream
        .get("height")
        .and_then(|h| h.as_u64())
        .unwrap_or(1080) as u32;

    let fps = stream
        .get("avg_frame_rate")
        .and_then(|r| r.as_str())
        .and_then(parse_frame_rate)
        .or_else(|| {
            stream
                .get("r_frame_rate")
                .and_then(|r| r.as_str())
                .and_then(parse_frame_rate)
        })
        .unwrap_or(30.0);

    let duration_in_seconds = stream
        .get("duration")
        .and_then(|d| d.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| {
            json_val
                .get("format")
                .and_then(|f| f.get("duration"))
                .and_then(|d| d.as_str())
                .and_then(|s| s.parse::<f64>().ok())
        })
        .unwrap_or(0.0);

    let duration_in_frames = (duration_in_seconds * fps).round() as u32;
    let aspect_ratio = if height > 0 {
        width as f64 / height as f64
    } else {
        16.0 / 9.0
    };

    Ok(VideoMetadata {
        width,
        height,
        fps,
        duration_in_seconds,
        duration_in_frames,
        aspect_ratio,
        is_landscape: width >= height,
    })
}

/// Probe an audio file natively (`hound`, ISO-BMFF container, or `symphonia`), falling back to `ffprobe`.
pub fn get_audio_metadata(
    path: impl AsRef<Path>,
    fps: f64,
) -> Result<AudioMetadata, MediaMetadataError> {
    let path_ref = path.as_ref();
    if !path_ref.exists() {
        return Err(MediaMetadataError::FileNotFound(
            path_ref.display().to_string(),
        ));
    }

    // Try hound for WAV first
    if let Ok(reader) = hound::WavReader::open(path_ref) {
        let spec = reader.spec();
        let samples = reader.duration();
        let duration_secs = if spec.sample_rate > 0 {
            samples as f64 / spec.sample_rate as f64
        } else {
            0.0
        };
        return Ok(AudioMetadata {
            channels: spec.channels,
            sample_rate: spec.sample_rate,
            duration_in_seconds: duration_secs,
            duration_in_frames: (duration_secs * fps).round() as u32,
        });
    }

    // Try native ISO-BMFF (MP4 / M4A / MOV) container probe next
    if let Ok(meta) = rust_motion_media_container::probe_isobmff_path(path_ref) {
        if let Some(audio) = audio_metadata_from_container(&meta, fps) {
            return Ok(audio);
        }
        return Err(MediaMetadataError::AudioDecode(
            "ISO-BMFF container has no audio stream".into(),
        ));
    }

    // Try Symphonia for MP3 / AAC / ISO-MP4 / PCM streams
    if let Some((channels, sample_rate, duration_secs)) = probe_audio_with_symphonia(path_ref) {
        return Ok(AudioMetadata {
            channels,
            sample_rate,
            duration_in_seconds: duration_secs,
            duration_in_frames: (duration_secs * fps).round() as u32,
        });
    }

    // Fallback to ffprobe
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=channels,sample_rate,duration",
            "-show_entries",
            "format=duration",
            "-of",
            "json",
        ])
        .arg(path_ref)
        .output()
        .map_err(|e| MediaMetadataError::FfprobeExecution(e.to_string()))?;

    if !output.status.success() {
        return Err(MediaMetadataError::FfprobeExecution(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    let json_val: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| MediaMetadataError::FfprobeParse(e.to_string()))?;

    let stream = json_val
        .get("streams")
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.first())
        .ok_or_else(|| MediaMetadataError::FfprobeParse("No audio stream found".into()))?;

    let channels = stream.get("channels").and_then(|c| c.as_u64()).unwrap_or(2) as u16;

    let sample_rate = stream
        .get("sample_rate")
        .and_then(|s| s.as_str())
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(44100);

    let duration_in_seconds = stream
        .get("duration")
        .and_then(|d| d.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| {
            json_val
                .get("format")
                .and_then(|f| f.get("duration"))
                .and_then(|d| d.as_str())
                .and_then(|s| s.parse::<f64>().ok())
        })
        .unwrap_or(0.0);

    Ok(AudioMetadata {
        channels,
        sample_rate,
        duration_in_seconds,
        duration_in_frames: (duration_in_seconds * fps).round() as u32,
    })
}

fn probe_audio_with_symphonia(path: &Path) -> Option<(u16, u32, f64)> {
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;
    let track = probed
        .format
        .tracks()
        .iter()
        .find(|t| t.codec_params.channels.is_some() && t.codec_params.sample_rate.is_some())?;
    let channels = track.codec_params.channels?.count() as u16;
    let sample_rate = track.codec_params.sample_rate?;
    let duration_secs = match (track.codec_params.n_frames, track.codec_params.time_base) {
        (Some(n_frames), Some(tb)) => {
            let t = tb.calc_time(n_frames);
            t.seconds as f64 + t.frac
        }
        (Some(n_frames), None) if sample_rate > 0 => n_frames as f64 / f64::from(sample_rate),
        _ => return None,
    };
    Some((channels, sample_rate, duration_secs))
}

fn parse_frame_rate(s: &str) -> Option<f64> {
    let rate = if let Some((num, den)) = s.split_once('/') {
        let n: f64 = num.parse().ok()?;
        let d: f64 = den.parse().ok()?;
        if d <= 0.0 {
            return None;
        }
        n / d
    } else {
        s.parse().ok()?
    };
    (rate.is_finite() && rate > 0.0).then_some(rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frame_rate() {
        assert!((parse_frame_rate("30/1").unwrap() - 30.0).abs() < 1e-4);
        assert!((parse_frame_rate("60000/1001").unwrap() - 59.94).abs() < 0.01);
        assert!((parse_frame_rate("24").unwrap() - 24.0).abs() < 1e-4);
        assert_eq!(parse_frame_rate("0/0"), None);
        assert_eq!(parse_frame_rate("-30/1"), None);
    }

    #[test]
    fn test_static_file_resolution() {
        let res = static_file("Cargo.toml");
        assert!(res.is_ok());
    }

    #[test]
    fn test_image_dimensions() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/logo.png");
        let dimensions = get_image_dimensions(path).unwrap();
        assert!(dimensions.width > 0);
        assert!(dimensions.height > 0);
    }

    #[test]
    fn test_parse_media_returns_image_variant() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/logo.png");
        let parsed = parse_media(path, 30.0).unwrap();
        assert!(parsed.image.is_some());
        assert!(parsed.video.is_none());
        assert!(parsed.audio.is_none());
        assert_eq!(parsed.duration_in_seconds, 0.0);
    }

    #[test]
    fn test_parse_media_duration_uses_maximum_valid_track_duration() {
        let video = |duration_in_seconds| VideoMetadata {
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration_in_seconds,
            duration_in_frames: 0,
            aspect_ratio: 16.0 / 9.0,
            is_landscape: true,
        };
        let audio = |duration_in_seconds| AudioMetadata {
            channels: 2,
            sample_rate: 48_000,
            duration_in_seconds,
            duration_in_frames: 0,
        };

        assert_eq!(max_known_track_duration(None, Some(&audio(3.0))), 3.0);
        assert_eq!(max_known_track_duration(Some(&video(2.0)), None), 2.0);
        assert_eq!(
            max_known_track_duration(Some(&video(2.0)), Some(&audio(2.0))),
            2.0
        );
        assert_eq!(
            max_known_track_duration(Some(&video(2.0)), Some(&audio(3.0))),
            3.0
        );
        assert_eq!(
            max_known_track_duration(Some(&video(3.0)), Some(&audio(2.0))),
            3.0
        );
        assert_eq!(
            max_known_track_duration(Some(&video(f64::NAN)), Some(&audio(f64::INFINITY))),
            0.0
        );
    }

    #[test]
    fn test_get_video_metadata_matches_rasterizer_for_vfr_input() {
        if Command::new("ffmpeg").arg("-version").output().is_err()
            || Command::new("ffprobe").arg("-version").output().is_err()
        {
            eprintln!("skipping VFR metadata test: FFmpeg or FFprobe is unavailable");
            return;
        }

        let directory = std::env::temp_dir().join(format!(
            "rust-motion-media-vfr-metadata-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("vfr.mp4");
        let generated = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=16x16:rate=10:duration=1",
                "-vf",
                "select=eq(n\\,0)+eq(n\\,1)+eq(n\\,4)+eq(n\\,9)",
                "-fps_mode",
                "vfr",
                "-c:v",
                "mpeg4",
            ])
            .arg(&source)
            .status()
            .unwrap();
        assert!(generated.success());

        let ffprobe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=avg_frame_rate,r_frame_rate",
                "-of",
                "json",
            ])
            .arg(&source)
            .output()
            .unwrap();
        assert!(ffprobe.status.success());
        let probe: serde_json::Value = serde_json::from_slice(&ffprobe.stdout).unwrap();
        let stream = &probe["streams"][0];
        let average_fps = parse_frame_rate(stream["avg_frame_rate"].as_str().unwrap()).unwrap();
        let nominal_fps = parse_frame_rate(stream["r_frame_rate"].as_str().unwrap()).unwrap();
        assert!((average_fps - nominal_fps).abs() > 0.01);

        let media = get_video_metadata(&source).unwrap();
        let rasterizer =
            rust_motion_rasterizer::probe_video_metadata(source.to_str().unwrap()).unwrap();
        let rasterizer_fps = rasterizer.fps.unwrap();
        assert!((media.fps - average_fps).abs() < 1e-6);
        assert!((media.fps - rasterizer_fps).abs() < 1e-6);
        assert_eq!(
            media.duration_in_frames,
            (media.duration_in_seconds * average_fps).round() as u32
        );

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn test_read_media_range_is_bounded_and_half_open() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let bytes = read_media_range(&path, 0, 4).unwrap();
        assert_eq!(bytes.len(), 4);
        assert!(read_media_range(&path, 4, 3).is_err());
        assert!(read_media_range(&path, 0, MAX_MEDIA_RANGE_BYTES + 1).is_err());
    }

    #[test]
    fn test_native_isobmff_probe_and_parse_media_without_ffprobe() {
        use rust_motion_media_container::{Mp4Muxer, Mp4MuxerOptions};
        use rust_motion_media_core::{
            AudioCodecId, AudioTrackConfig, ColorDescription, ContainerMuxer, EncodedPacket,
            Rational, TrackConfig, VideoCodecId, VideoTrackConfig,
        };

        let dir = std::env::temp_dir().join(format!(
            "rust_motion_native_probe_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mp4_path = dir.join("native.mp4");

        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&mp4_path)
            .unwrap();
        let avcc = [1u8, 100, 0, 30, 0xFF, 0xE1];
        let v = TrackConfig::Video(VideoTrackConfig {
            width: 320,
            height: 240,
            timescale: 24,
            fps: Rational::new(24, 1),
            codec: VideoCodecId::H264,
            codec_config: avcc.to_vec(),
            color: ColorDescription::bt709_limited(),
        });
        let a = TrackConfig::Audio(AudioTrackConfig {
            codec: AudioCodecId::Aac,
            sample_rate: 48000,
            channels: 2,
            timescale: 48000,
            codec_config: vec![0x11, 0x90],
        });
        let mut muxer = Mp4Muxer::new(file, &[v, a], Mp4MuxerOptions::default()).unwrap();
        for i in 0..12 {
            muxer
                .write_packet(&EncodedPacket {
                    stream_index: 0,
                    data: vec![0, 0, 0, 2, 0x65, i as u8],
                    pts: i,
                    dts: i,
                    duration: 1,
                    is_keyframe: i == 0,
                })
                .unwrap();
        }
        muxer
            .write_packet(&EncodedPacket {
                stream_index: 1,
                data: vec![0xAA; 8],
                pts: 0,
                dts: 0,
                duration: 24000,
                is_keyframe: true,
            })
            .unwrap();
        muxer.finish().unwrap();

        let vmeta = get_video_metadata(&mp4_path).unwrap();
        assert_eq!((vmeta.width, vmeta.height), (320, 240));
        assert!((vmeta.fps - 24.0).abs() < 1e-6);
        assert!((vmeta.duration_in_seconds - 0.5).abs() < 1e-4);
        assert_eq!(vmeta.duration_in_frames, 12);

        let ameta = get_audio_metadata(&mp4_path, 24.0).unwrap();
        assert_eq!(ameta.channels, 2);
        assert_eq!(ameta.sample_rate, 48000);
        assert!((ameta.duration_in_seconds - 0.5).abs() < 1e-4);

        let parsed = parse_media(&mp4_path, 24.0).unwrap();
        assert!(parsed.video.is_some());
        assert!(parsed.audio.is_some());
        assert!((parsed.duration_in_seconds - 0.5).abs() < 1e-4);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
