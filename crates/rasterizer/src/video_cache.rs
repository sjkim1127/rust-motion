//! Bounded video metadata, decoder-session, and decoded-frame caches.

use crate::backend::RasterError;
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

const MAX_CACHE_BYTES: usize = 128 * 1024 * 1024;
const MAX_DECODER_SOURCES: usize = 4;
const MAX_FORWARD_DECODE_SECONDS: f64 = 5.0;
const MAX_DECODED_VIDEO_PIXELS: u64 = 64 * 1024 * 1024;
const STDERR_LIMIT_BYTES: u64 = 64 * 1024;

/// Video stream information reported by FFprobe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoMetadata {
    pub width: u32,
    pub height: u32,
    /// Dimensions after applying the stream rotation metadata.
    pub display_width: u32,
    pub display_height: u32,
    pub duration: Option<f64>,
    pub fps: Option<f64>,
    /// Clockwise display rotation normalized to `0..360`.
    pub rotation: u16,
    pub video_stream_index: usize,
    pub audio_stream_indices: Vec<usize>,
    pub codec_name: Option<String>,
    pub pixel_format: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VideoFrameKey {
    path: PathBuf,
    sampling_rate_micros: u64,
    frame_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DecoderKey {
    path: PathBuf,
    sampling_rate_micros: u64,
}

#[derive(Default)]
struct CacheState {
    frames: VecDeque<(VideoFrameKey, Arc<RgbaImage>)>,
    bytes: usize,
}

#[derive(Default)]
struct DecoderPool {
    sessions: VecDeque<(DecoderKey, Arc<Mutex<DecoderSession>>)>,
}

#[derive(Default)]
pub(crate) struct VideoFrameCache {
    state: Mutex<CacheState>,
    metadata: Mutex<HashMap<PathBuf, Arc<VideoMetadata>>>,
    decoders: Mutex<DecoderPool>,
    #[cfg(test)]
    spawn_count: std::sync::atomic::AtomicUsize,
}

impl VideoFrameCache {
    #[cfg(feature = "gpu")]
    pub(crate) fn frame_index_for(
        &self,
        src: &str,
        time: f64,
        sampling_fps: f64,
        looped: bool,
    ) -> Result<u64, RasterError> {
        let path = canonical_local_path_with_policy(
            src,
            &crate::security::MediaSecurityPolicy::default(),
        )?;
        let metadata = self.metadata_for(&path)?;
        Ok(normalize_frame_index(
            time,
            sampling_fps,
            metadata.duration,
            looped,
        ))
    }

    #[allow(dead_code)]
    pub(crate) fn load(
        &self,
        src: &str,
        time: f64,
        sampling_fps: f64,
        looped: bool,
    ) -> Result<Arc<RgbaImage>, RasterError> {
        self.load_with_policy(
            src,
            time,
            sampling_fps,
            looped,
            &crate::security::MediaSecurityPolicy::default(),
        )
    }

    pub(crate) fn load_with_policy(
        &self,
        src: &str,
        time: f64,
        sampling_fps: f64,
        looped: bool,
        policy: &crate::security::MediaSecurityPolicy,
    ) -> Result<Arc<RgbaImage>, RasterError> {
        if !time.is_finite() || time < 0.0 {
            return Err(media_error(
                src,
                "video time must be finite and non-negative",
            ));
        }
        if !sampling_fps.is_finite() || sampling_fps <= 0.0 {
            return Err(media_error(
                src,
                "video sampling FPS must be finite and greater than zero",
            ));
        }

        let path = canonical_local_path_with_policy(src, policy)?;
        let metadata = self.metadata_for(&path)?;
        let sampling_rate_micros = fps_key(sampling_fps);
        let frame_index = normalize_frame_index(time, sampling_fps, metadata.duration, looped);
        let key = VideoFrameKey {
            path: path.clone(),
            sampling_rate_micros,
            frame_index,
        };

        if let Some(frame) = self.cached(&key) {
            return Ok(frame);
        }

        let decoder_key = DecoderKey {
            path: path.clone(),
            sampling_rate_micros,
        };
        let decoder = self.decoder_for(decoder_key, &metadata, sampling_fps, frame_index)?;
        let mut decoder = decoder.lock().expect("video decoder lock poisoned");

        if let Some(frame) = self.cached(&key) {
            return Ok(frame);
        }

        let should_restart = frame_index < decoder.next_frame
            || frame_index.saturating_sub(decoder.next_frame)
                > (sampling_fps * MAX_FORWARD_DECODE_SECONDS).ceil() as u64;
        if should_restart {
            *decoder = self.spawn_decoder(&path, &metadata, sampling_fps, frame_index)?;
        }

        while decoder.next_frame <= frame_index {
            let decoded_index = decoder.next_frame;
            let frame = Arc::new(decoder.read_frame()?);
            decoder.next_frame += 1;
            self.insert(
                VideoFrameKey {
                    path: path.clone(),
                    sampling_rate_micros,
                    frame_index: decoded_index,
                },
                Arc::clone(&frame),
            );
        }

        self.cached(&key).ok_or_else(|| {
            media_error(
                path.display().to_string(),
                format!("failed to cache decoded video frame {frame_index}"),
            )
        })
    }

    fn metadata_for(&self, path: &Path) -> Result<Arc<VideoMetadata>, RasterError> {
        if let Some(metadata) = self
            .metadata
            .lock()
            .expect("video metadata lock poisoned")
            .get(path)
            .cloned()
        {
            return Ok(metadata);
        }

        let metadata = Arc::new(probe_path(path)?);
        self.metadata
            .lock()
            .expect("video metadata lock poisoned")
            .insert(path.to_path_buf(), Arc::clone(&metadata));
        Ok(metadata)
    }

    fn decoder_for(
        &self,
        key: DecoderKey,
        metadata: &VideoMetadata,
        sampling_fps: f64,
        first_frame: u64,
    ) -> Result<Arc<Mutex<DecoderSession>>, RasterError> {
        let mut pool = self.decoders.lock().expect("video decoder pool poisoned");
        if let Some(index) = pool
            .sessions
            .iter()
            .position(|(candidate, _)| candidate == &key)
        {
            let entry = pool.sessions.remove(index).expect("decoder entry exists");
            let decoder = Arc::clone(&entry.1);
            pool.sessions.push_back(entry);
            return Ok(decoder);
        }

        if pool.sessions.len() >= MAX_DECODER_SOURCES {
            let evictable = pool
                .sessions
                .iter()
                .position(|(_, decoder)| Arc::strong_count(decoder) == 1)
                .ok_or_else(|| {
                    media_error(
                        key.path.display().to_string(),
                        format!(
                            "video decoder source limit ({MAX_DECODER_SOURCES}) reached; reduce render concurrency or active video sources"
                        ),
                    )
                })?;
            pool.sessions.remove(evictable);
        }

        let forward_window = (sampling_fps * MAX_FORWARD_DECODE_SECONDS).ceil() as u64;
        let decoder_start = if first_frame <= forward_window {
            0
        } else {
            first_frame
        };
        let decoder = Arc::new(Mutex::new(self.spawn_decoder(
            &key.path,
            metadata,
            sampling_fps,
            decoder_start,
        )?));
        pool.sessions.push_back((key, Arc::clone(&decoder)));
        Ok(decoder)
    }

    fn spawn_decoder(
        &self,
        path: &Path,
        metadata: &VideoMetadata,
        sampling_fps: f64,
        first_frame: u64,
    ) -> Result<DecoderSession, RasterError> {
        #[cfg(test)]
        self.spawn_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        DecoderSession::spawn(path, metadata, sampling_fps, first_frame)
    }

    fn cached(&self, key: &VideoFrameKey) -> Option<Arc<RgbaImage>> {
        let mut state = self.state.lock().expect("video cache lock poisoned");
        let index = state
            .frames
            .iter()
            .position(|(candidate, _)| candidate == key)?;
        let entry = state.frames.remove(index)?;
        let frame = Arc::clone(&entry.1);
        state.frames.push_back(entry);
        Some(frame)
    }

    fn insert(&self, key: VideoFrameKey, frame: Arc<RgbaImage>) {
        let frame_bytes = frame.as_raw().len();
        if frame_bytes > MAX_CACHE_BYTES {
            return;
        }

        let mut state = self.state.lock().expect("video cache lock poisoned");
        if let Some(index) = state
            .frames
            .iter()
            .position(|(candidate, _)| candidate == &key)
        {
            if let Some((_, duplicate)) = state.frames.remove(index) {
                state.bytes = state.bytes.saturating_sub(duplicate.as_raw().len());
            }
        }
        while state.bytes.saturating_add(frame_bytes) > MAX_CACHE_BYTES {
            let Some((_, evicted)) = state.frames.pop_front() else {
                break;
            };
            state.bytes = state.bytes.saturating_sub(evicted.as_raw().len());
        }
        state.bytes += frame_bytes;
        state.frames.push_back((key, frame));
    }

    pub(crate) fn shutdown(&self) {
        self.decoders
            .lock()
            .expect("video decoder pool poisoned")
            .sessions
            .clear();
    }

    #[cfg(test)]
    pub(crate) fn bytes(&self) -> usize {
        self.state.lock().expect("video cache lock poisoned").bytes
    }

    #[cfg(test)]
    pub(crate) fn decoder_count(&self) -> usize {
        self.decoders
            .lock()
            .expect("video decoder pool poisoned")
            .sessions
            .len()
    }

    #[cfg(test)]
    pub(crate) fn spawn_count(&self) -> usize {
        self.spawn_count.load(std::sync::atomic::Ordering::Relaxed)
    }
}

struct DecoderSession {
    path: PathBuf,
    width: u32,
    height: u32,
    next_frame: u64,
    child: Child,
    stdout: ChildStdout,
    stderr: Arc<Mutex<Vec<u8>>>,
    stderr_thread: Option<JoinHandle<()>>,
}

impl DecoderSession {
    fn spawn(
        path: &Path,
        metadata: &VideoMetadata,
        sampling_fps: f64,
        first_frame: u64,
    ) -> Result<Self, RasterError> {
        let seek_time = first_frame as f64 / sampling_fps;
        let mut command = Command::new("ffmpeg");
        command.args(["-hide_banner", "-loglevel", "error"]);
        if first_frame > 0 {
            command.args(["-ss", &format!("{seek_time:.9}")]);
        }
        let mut child = command
            .arg("-i")
            .arg(path)
            .args([
                "-map",
                "0:v:0",
                "-vf",
                &format!("fps={sampling_fps:.9}"),
                "-an",
                "-sn",
                "-dn",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                media_error(
                    path.display().to_string(),
                    format!("failed to start persistent FFmpeg decoder: {error}"),
                )
            })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            media_error(
                path.display().to_string(),
                "failed to capture FFmpeg decoder stdout",
            )
        })?;
        let child_stderr = child.stderr.take().ok_or_else(|| {
            media_error(
                path.display().to_string(),
                "failed to capture FFmpeg decoder stderr",
            )
        })?;
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let stderr_writer = Arc::clone(&stderr);
        let stderr_thread = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = child_stderr
                .take(STDERR_LIMIT_BYTES)
                .read_to_end(&mut bytes);
            *stderr_writer.lock().expect("FFmpeg stderr lock poisoned") = bytes;
        });

        Ok(Self {
            path: path.to_path_buf(),
            width: metadata.display_width,
            height: metadata.display_height,
            next_frame: first_frame,
            child,
            stdout,
            stderr,
            stderr_thread: Some(stderr_thread),
        })
    }

    fn read_frame(&mut self) -> Result<RgbaImage, RasterError> {
        let byte_len = u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or_else(|| {
                media_error(
                    self.path.display().to_string(),
                    "decoded video dimensions exceed the addressable frame size",
                )
            })?;
        let mut raw = vec![0; byte_len];
        if let Err(error) = self.stdout.read_exact(&mut raw) {
            let _ = self.child.wait();
            if let Some(thread) = self.stderr_thread.take() {
                let _ = thread.join();
            }
            let details =
                String::from_utf8_lossy(&self.stderr.lock().expect("FFmpeg stderr lock poisoned"))
                    .trim()
                    .to_string();
            let reason = if details.is_empty() {
                format!(
                    "no decoded frame exists at frame {} ({error})",
                    self.next_frame
                )
            } else {
                format!(
                    "FFmpeg decode failed at frame {}: {details}",
                    self.next_frame
                )
            };
            return Err(media_error(self.path.display().to_string(), reason));
        }

        RgbaImage::from_raw(self.width, self.height, raw).ok_or_else(|| {
            media_error(
                self.path.display().to_string(),
                "FFmpeg returned an invalid RGBA frame length",
            )
        })
    }
}

impl Drop for DecoderSession {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
    }
}

/// Probe a local video with FFprobe.
pub fn probe_video_metadata(src: &str) -> Result<VideoMetadata, RasterError> {
    let path = canonical_local_path(src)?;
    probe_path(&path)
}

pub(crate) fn canonical_local_path(src: &str) -> Result<PathBuf, RasterError> {
    canonical_local_path_with_policy(src, &crate::security::MediaSecurityPolicy::default())
}

pub(crate) fn canonical_local_path_with_policy(
    src: &str,
    policy: &crate::security::MediaSecurityPolicy,
) -> Result<PathBuf, RasterError> {
    policy.validate_path(src)
}

fn probe_path(path: &Path) -> Result<VideoMetadata, RasterError> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=index,codec_type,codec_name,width,height,avg_frame_rate,r_frame_rate,pix_fmt,duration:stream_tags=rotate:stream_side_data=rotation:format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|error| {
            media_error(
                path.display().to_string(),
                format!("failed to run FFprobe: {error}"),
            )
        })?;

    if !output.status.success() {
        return Err(media_error(
            path.display().to_string(),
            format!(
                "FFprobe failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }

    let probe: ProbeOutput = serde_json::from_slice(&output.stdout).map_err(|error| {
        media_error(
            path.display().to_string(),
            format!("invalid FFprobe JSON: {error}"),
        )
    })?;
    let video = probe
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("video"))
        .ok_or_else(|| media_error(path.display().to_string(), "no video stream found"))?;
    let width = video
        .width
        .filter(|value| *value > 0)
        .ok_or_else(|| media_error(path.display().to_string(), "video width is missing"))?;
    let height = video
        .height
        .filter(|value| *value > 0)
        .ok_or_else(|| media_error(path.display().to_string(), "video height is missing"))?;
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_DECODED_VIDEO_PIXELS {
        return Err(media_error(
            path.display().to_string(),
            format!(
                "video dimensions {width}x{height} exceed the decoded-frame limit of {MAX_DECODED_VIDEO_PIXELS} pixels"
            ),
        ));
    }
    let rotation = stream_rotation(video);
    let (display_width, display_height) = if rotation == 90 || rotation == 270 {
        (height, width)
    } else {
        (width, height)
    };
    let duration = positive_finite(video.duration.as_deref())
        .or_else(|| positive_finite(probe.format.duration.as_deref()));
    let fps = parse_rate(video.avg_frame_rate.as_deref())
        .or_else(|| parse_rate(video.r_frame_rate.as_deref()));

    Ok(VideoMetadata {
        width,
        height,
        display_width,
        display_height,
        duration,
        fps,
        rotation,
        video_stream_index: video.index,
        audio_stream_indices: probe
            .streams
            .iter()
            .filter(|stream| stream.codec_type.as_deref() == Some("audio"))
            .map(|stream| stream.index)
            .collect(),
        codec_name: video.codec_name.clone(),
        pixel_format: video.pix_fmt.clone(),
    })
}

fn normalize_frame_index(time: f64, sampling_fps: f64, duration: Option<f64>, looped: bool) -> u64 {
    let normalized_time = match duration.filter(|duration| duration.is_finite() && *duration > 0.0)
    {
        Some(duration) if looped => time % duration,
        Some(duration) => time.min(duration),
        None => time,
    };
    let requested = (normalized_time * sampling_fps)
        .round()
        .clamp(0.0, u64::MAX as f64) as u64;
    if let Some(duration) = duration.filter(|duration| duration.is_finite() && *duration > 0.0) {
        let frame_count = (duration * sampling_fps).ceil().clamp(1.0, u64::MAX as f64) as u64;
        requested.min(frame_count - 1)
    } else {
        requested
    }
}

fn fps_key(fps: f64) -> u64 {
    (fps * 1_000_000.0).round().clamp(1.0, u64::MAX as f64) as u64
}

fn positive_finite(value: Option<&str>) -> Option<f64> {
    value?
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)
}

fn parse_rate(value: Option<&str>) -> Option<f64> {
    let value = value?;
    let (numerator, denominator) = value.split_once('/')?;
    let numerator = numerator.parse::<f64>().ok()?;
    let denominator = denominator.parse::<f64>().ok()?;
    let rate = numerator / denominator;
    (rate.is_finite() && rate > 0.0).then_some(rate)
}

fn stream_rotation(stream: &ProbeStream) -> u16 {
    let rotation = stream
        .side_data_list
        .iter()
        .find_map(|data| data.rotation)
        .or_else(|| {
            stream
                .tags
                .get("rotate")
                .and_then(|value| value.parse::<i32>().ok())
        })
        .unwrap_or(0);
    rotation.rem_euclid(360) as u16
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    #[serde(default)]
    format: ProbeFormat,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    index: usize,
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    pix_fmt: Option<String>,
    duration: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
    #[serde(default)]
    side_data_list: Vec<ProbeSideData>,
}

#[derive(Debug, Default, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeSideData {
    rotation: Option<i32>,
}

fn media_error(path: impl Into<String>, reason: impl Into<String>) -> RasterError {
    RasterError::MediaAsset {
        path: path.into(),
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_remote_video_sources() {
        let error = canonical_local_path("https://example.com/video.mp4").unwrap_err();
        assert!(error.to_string().contains("only local paths"));
    }

    #[test]
    fn replacing_a_cached_key_does_not_double_count_bytes() {
        let cache = VideoFrameCache::default();
        let key = VideoFrameKey {
            path: PathBuf::from("clip.mp4"),
            sampling_rate_micros: 30_000_000,
            frame_index: 0,
        };
        let frame = Arc::new(RgbaImage::new(2, 2));
        cache.insert(key.clone(), Arc::clone(&frame));
        cache.insert(key, frame);

        let state = cache.state.lock().unwrap();
        assert_eq!(state.frames.len(), 1);
        assert_eq!(state.bytes, 16);
    }

    #[test]
    fn frame_time_is_clamped_or_looped_at_eof() {
        assert_eq!(normalize_frame_index(4.0, 2.0, Some(1.0), false), 1);
        assert_eq!(normalize_frame_index(1.25, 4.0, Some(1.0), true), 1);
        assert_eq!(normalize_frame_index(2.0, 4.0, Some(1.0), true), 0);
    }

    #[test]
    fn rational_frame_rates_are_parsed() {
        let rate = parse_rate(Some("30000/1001")).unwrap();
        assert!((rate - 29.970_029_97).abs() < 0.000_001);
        assert_eq!(parse_rate(Some("0/0")), None);
    }

    #[test]
    fn variable_frame_rate_input_is_sampled_on_the_output_timeline() {
        if Command::new("ffmpeg").arg("-version").output().is_err()
            || Command::new("ffprobe").arg("-version").output().is_err()
        {
            eprintln!("skipping VFR decode test: FFmpeg or FFprobe is unavailable");
            return;
        }

        let dir =
            std::env::temp_dir().join(format!("rust-motion-vfr-video-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("vfr.mkv");
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
                "ffv1",
            ])
            .arg(&source)
            .status()
            .unwrap();
        assert!(generated.success());

        let cache = VideoFrameCache::default();
        for time in [0.0, 0.2, 0.4, 0.6, 0.8] {
            let frame = cache
                .load(source.to_str().unwrap(), time, 5.0, false)
                .unwrap();
            assert_eq!(frame.dimensions(), (16, 16));
        }
        assert_eq!(cache.spawn_count(), 1);
        cache.shutdown();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rotated_video_reports_and_decodes_display_dimensions() {
        if Command::new("ffmpeg").arg("-version").output().is_err()
            || Command::new("ffprobe").arg("-version").output().is_err()
        {
            eprintln!("skipping rotation test: FFmpeg or FFprobe is unavailable");
            return;
        }

        let dir = std::env::temp_dir().join(format!(
            "rust-motion-rotated-video-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("rotated.mp4");
        let generated = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:size=8x4:rate=1:duration=1",
                "-vf",
                "drawbox=x=0:y=0:w=4:h=2:color=red:t=fill,drawbox=x=4:y=0:w=4:h=2:color=green:t=fill,drawbox=x=0:y=2:w=4:h=2:color=blue:t=fill,drawbox=x=4:y=2:w=4:h=2:color=white:t=fill",
                "-metadata:s:v:0",
                "rotate=90",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&source)
            .status()
            .unwrap();
        assert!(generated.success());

        let metadata = match probe_video_metadata(source.to_str().unwrap()) {
            Ok(metadata) => metadata,
            Err(error) => {
                eprintln!(
                    "skipping rotation test: FFmpeg did not preserve rotation metadata: {error}"
                );
                let _ = std::fs::remove_dir_all(&dir);
                return;
            }
        };
        if metadata.rotation != 90 {
            eprintln!(
                "skipping rotation test: generated fixture reported rotation {}",
                metadata.rotation
            );
            std::fs::remove_dir_all(&dir).unwrap();
            return;
        }
        assert_eq!((metadata.width, metadata.height), (8, 4));
        assert_eq!((metadata.display_width, metadata.display_height), (4, 8));

        let cache = VideoFrameCache::default();
        let frame = cache
            .load(source.to_str().unwrap(), 0.0, 1.0, false)
            .unwrap();
        assert_eq!(frame.dimensions(), (4, 8));
        let corner = |x: u32, y: u32| frame.get_pixel(x, y).0;
        let is_red = |pixel: [u8; 4]| pixel[0] > 150 && pixel[1] < 100 && pixel[2] < 100;
        let is_green = |pixel: [u8; 4]| pixel[1] > 100 && pixel[0] < 100 && pixel[2] < 100;
        let is_blue = |pixel: [u8; 4]| pixel[2] > 100 && pixel[0] < 100 && pixel[1] < 100;
        let is_white = |pixel: [u8; 4]| pixel[0] > 150 && pixel[1] > 150 && pixel[2] > 150;
        assert!(is_blue(corner(0, 0)));
        assert!(is_red(corner(3, 0)));
        assert!(is_white(corner(0, 7)));
        assert!(is_green(corner(3, 7)));
        cache.shutdown();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn variable_frame_rate_display_rotation_preserves_orientation() {
        if Command::new("ffmpeg").arg("-version").output().is_err()
            || Command::new("ffprobe").arg("-version").output().is_err()
        {
            eprintln!("skipping VFR rotation test: FFmpeg or FFprobe is unavailable");
            return;
        }

        let dir = std::env::temp_dir().join(format!(
            "rust-motion-vfr-display-rotation-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("vfr-rotated.mp4");
        let generated = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-display_rotation:v:0",
                "90",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=16x8:rate=10:duration=1",
                "-vf",
                "select=eq(n\\,0)+eq(n\\,1)+eq(n\\,4)+eq(n\\,9)",
                "-fps_mode",
                "vfr",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&source)
            .status()
            .unwrap();
        assert!(generated.success());

        let cache = VideoFrameCache::default();
        for time in [0.0, 0.2, 0.4, 0.6, 0.8] {
            let frame = cache
                .load(source.to_str().unwrap(), time, 5.0, false)
                .unwrap();
            assert_eq!(frame.dimensions(), (8, 16));
        }
        assert_eq!(cache.spawn_count(), 1);
        cache.shutdown();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
