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
    next_frame: u64,
    inner: DecoderImpl,
}

enum DecoderImpl {
    NativeHevc(Box<NativeHevcDecoder>),
    Ffmpeg(Box<FfmpegDecoderSession>),
}

struct FfmpegDecoderSession {
    path: PathBuf,
    width: u32,
    height: u32,
    child: Child,
    stdout: ChildStdout,
    stderr: Arc<Mutex<Vec<u8>>>,
    stderr_thread: Option<JoinHandle<()>>,
}

struct NativeHevcDecoder {
    path: PathBuf,
    demuxer: rust_motion_media_container::Mp4Demuxer<std::fs::File>,
    track: rust_motion_media_container::DemuxedVideoTrack,
    decoder: rust_motion_hevc::Decoder,
    sampling_fps: f64,
    rotation: u16,
    next_sample_idx: usize,
    eof_flushed: bool,
    ready_pics: Vec<rust_motion_hevc::Picture>,
    sample_buf: Vec<u8>,
}

impl NativeHevcDecoder {
    fn try_open(
        path: &Path,
        metadata: &VideoMetadata,
        sampling_fps: f64,
        first_frame: u64,
    ) -> Result<Self, RasterError> {
        let demuxer = rust_motion_media_container::Mp4Demuxer::open_path(path).map_err(|e| {
            media_error(
                path.display().to_string(),
                format!("native MP4 demuxer failed: {e}"),
            )
        })?;
        let track = demuxer
            .video_tracks()
            .first()
            .filter(|t| {
                t.codec == rust_motion_media_core::VideoCodecId::Hevc && !t.codec_config.is_empty()
            })
            .cloned()
            .ok_or_else(|| {
                media_error(
                    path.display().to_string(),
                    "MP4 does not contain a natively decodable HEVC track with hvcC",
                )
            })?;
        let mut decoder = rust_motion_hevc::Decoder::with_threads(1);
        decoder.configure_hvcc(&track.codec_config).map_err(|e| {
            media_error(
                path.display().to_string(),
                format!("native HEVC decoder init failed: {e}"),
            )
        })?;

        let start_time = first_frame as f64 / sampling_fps;
        let target_sample = track.sample_index_for_time(start_time);
        let keyframe_sample = track.keyframe_index_before_or_at(target_sample);

        Ok(Self {
            path: path.to_path_buf(),
            demuxer,
            track,
            decoder,
            sampling_fps,
            rotation: metadata.rotation,
            next_sample_idx: keyframe_sample,
            eof_flushed: false,
            ready_pics: Vec::new(),
            sample_buf: Vec::new(),
        })
    }

    fn read_frame(&mut self, output_frame: u64) -> Result<RgbaImage, RasterError> {
        if self.track.samples.is_empty() {
            return Err(media_error(
                self.path.display().to_string(),
                "HEVC track contains no samples",
            ));
        }

        let target_tick = if self.track.timescale > 0 {
            let scaled =
                (output_frame as f64 / self.sampling_fps) * f64::from(self.track.timescale);
            if scaled >= i64::MAX as f64 {
                i64::MAX
            } else {
                scaled.floor() as i64
            }
        } else {
            0
        };

        // Decode samples sequentially until DPB has emitted pictures strictly past `target_tick`
        // (or flush at EOF), ensuring B-frame reordering is complete before selecting `target_tick`.
        while !self.eof_flushed {
            let have_passed_target = self.ready_pics.last().is_some_and(|p| p.pts > target_tick);
            if have_passed_target {
                break;
            }
            if self.next_sample_idx < self.track.samples.len() {
                let sample = self
                    .demuxer
                    .read_sample(0, self.next_sample_idx, &mut self.sample_buf)
                    .map_err(|e| {
                        media_error(
                            self.path.display().to_string(),
                            format!("MP4 sample read failed: {e}"),
                        )
                    })?;
                self.next_sample_idx += 1;
                let pics = self
                    .decoder
                    .decode(&self.sample_buf, sample.pts)
                    .map_err(|e| {
                        media_error(
                            self.path.display().to_string(),
                            format!("native HEVC decode error: {e}"),
                        )
                    })?;
                self.insert_pictures(pics);
            } else {
                let trailing = self.decoder.flush();
                self.insert_pictures(trailing);
                self.eof_flushed = true;
            }
        }

        if self.ready_pics.is_empty() {
            return Err(media_error(
                self.path.display().to_string(),
                format!("no decoded HEVC picture available at frame {output_frame}"),
            ));
        }

        let chosen_idx = self
            .ready_pics
            .iter()
            .rposition(|p| p.pts <= target_tick)
            .unwrap_or(0);
        if chosen_idx > 0 {
            self.ready_pics.drain(0..chosen_idx);
        }
        let pic = &self.ready_pics[0];
        Ok(hevc_picture_to_rgba(pic, &self.track.color, self.rotation))
    }

    fn insert_pictures(&mut self, pics: Vec<rust_motion_hevc::Picture>) {
        if pics.is_empty() {
            return;
        }
        self.ready_pics.extend(pics);
        self.ready_pics.sort_by_key(|p| p.pts);
    }
}

impl DecoderSession {
    fn spawn(
        path: &Path,
        metadata: &VideoMetadata,
        sampling_fps: f64,
        first_frame: u64,
    ) -> Result<Self, RasterError> {
        if metadata.codec_name.as_deref() == Some("hevc") {
            if let Ok(native) =
                NativeHevcDecoder::try_open(path, metadata, sampling_fps, first_frame)
            {
                return Ok(Self {
                    next_frame: first_frame,
                    inner: DecoderImpl::NativeHevc(Box::new(native)),
                });
            }
        }

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
            next_frame: first_frame,
            inner: DecoderImpl::Ffmpeg(Box::new(FfmpegDecoderSession {
                path: path.to_path_buf(),
                width: metadata.display_width,
                height: metadata.display_height,
                child,
                stdout,
                stderr,
                stderr_thread: Some(stderr_thread),
            })),
        })
    }

    fn read_frame(&mut self) -> Result<RgbaImage, RasterError> {
        let frame_idx = self.next_frame;
        match &mut self.inner {
            DecoderImpl::NativeHevc(native) => native.read_frame(frame_idx),
            DecoderImpl::Ffmpeg(ff) => ff.read_frame(frame_idx),
        }
    }
}

impl FfmpegDecoderSession {
    fn read_frame(&mut self, next_frame: u64) -> Result<RgbaImage, RasterError> {
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
                format!("no decoded frame exists at frame {next_frame} ({error})")
            } else {
                format!("FFmpeg decode failed at frame {next_frame}: {details}")
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

impl Drop for FfmpegDecoderSession {
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

fn hevc_picture_to_rgba(
    pic: &rust_motion_hevc::Picture,
    container_color: &rust_motion_media_core::ColorDescription,
    rotation: u16,
) -> RgbaImage {
    use rust_motion_media_core::{ColorRange, MatrixCoefficients};

    let w = pic.width as usize;
    let h = pic.height as usize;
    let shift = pic.bit_depth.saturating_sub(8);
    let bit_scale = f32::from(1u16 << shift);

    let matrix = if pic.color.matrix != 2 && pic.color.matrix != 0 {
        MatrixCoefficients::from_cicp_u16(u16::from(pic.color.matrix))
    } else if container_color.matrix != MatrixCoefficients::Unspecified {
        container_color.matrix
    } else if w < 1280 && h < 720 {
        MatrixCoefficients::Bt601
    } else {
        MatrixCoefficients::Bt709
    };
    let full_range = if pic.color.matrix != 2 && pic.color.matrix != 0 {
        pic.color.full_range
    } else {
        container_color.range == ColorRange::Full
    };

    let (kr, kb) = match matrix {
        MatrixCoefficients::Bt601 => (0.2990f32, 0.1140f32),
        MatrixCoefficients::Bt2020Ncl => (0.2627f32, 0.0593f32),
        _ => (0.2126f32, 0.0722f32),
    };
    let kg = 1.0 - kr - kb;
    let cr_r = 2.0 * (1.0 - kr);
    let cb_b = 2.0 * (1.0 - kb);
    let cr_g = -2.0 * kr * (1.0 - kr) / kg;
    let cb_g = -2.0 * kb * (1.0 - kb) / kg;

    let chroma_w = (pic.chroma_width as usize).max(1);
    let chroma_h = (pic.chroma_height as usize).max(1);

    let (out_w, out_h) = if rotation == 90 || rotation == 270 {
        (h, w)
    } else {
        (w, h)
    };
    let mut out = vec![0u8; out_w * out_h * 4];

    for y in 0..h {
        let cy = (y >> 1).min(chroma_h - 1);
        let y_row = y * pic.y_stride;
        let uv_row = cy * pic.uv_stride;

        for x in 0..w {
            let cx = (x >> 1).min(chroma_w - 1);

            let y_val = f32::from(pic.y.get(y_row + x)) / bit_scale;
            let u_val = f32::from(pic.u.get(uv_row + cx)) / bit_scale;
            let v_val = f32::from(pic.v.get(uv_row + cx)) / bit_scale;

            let (y_lin, cb, cr) = if full_range {
                (
                    y_val / 255.0,
                    (u_val - 128.0) / 255.0,
                    (v_val - 128.0) / 255.0,
                )
            } else {
                (
                    (y_val - 16.0) / 219.0,
                    (u_val - 128.0) / 224.0,
                    (v_val - 128.0) / 224.0,
                )
            };

            let r = ((y_lin + cr_r * cr).clamp(0.0, 1.0) * 255.0).round() as u8;
            let g = ((y_lin + cr_g * cr + cb_g * cb).clamp(0.0, 1.0) * 255.0).round() as u8;
            let b = ((y_lin + cb_b * cb).clamp(0.0, 1.0) * 255.0).round() as u8;

            let (dx, dy) = match rotation {
                90 => (h - 1 - y, x),
                180 => (w - 1 - x, h - 1 - y),
                270 => (y, w - 1 - x),
                _ => (x, y),
            };
            let di = (dy * out_w + dx) * 4;
            out[di] = r;
            out[di + 1] = g;
            out[di + 2] = b;
            out[di + 3] = 255;
        }
    }

    RgbaImage::from_raw(out_w as u32, out_h as u32, out)
        .expect("output dimensions match buffer size")
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
    if let Ok(meta) = rust_motion_media_container::probe_isobmff_path(path) {
        if let Some(video) = meta.video_streams.first() {
            if video.width > 0 && video.height > 0 {
                let width = video.width;
                let height = video.height;
                let pixels = u64::from(width) * u64::from(height);
                if pixels > MAX_DECODED_VIDEO_PIXELS {
                    return Err(media_error(
                        path.display().to_string(),
                        format!(
                            "video dimensions {width}x{height} exceed the decoded-frame limit of {MAX_DECODED_VIDEO_PIXELS} pixels"
                        ),
                    ));
                }
                let rotation = video.rotation_degrees.rem_euclid(360) as u16;
                let (display_width, display_height) = if rotation == 90 || rotation == 270 {
                    (height, width)
                } else {
                    (width, height)
                };
                let fps = {
                    let rate = video.fps.as_f64();
                    (rate.is_finite() && rate > 0.0).then_some(rate)
                };
                let duration = video
                    .duration_secs
                    .or(meta.duration_secs)
                    .filter(|d| d.is_finite() && *d > 0.0);
                let codec_name = match video.codec {
                    rust_motion_media_core::VideoCodecId::H264 => Some("h264"),
                    rust_motion_media_core::VideoCodecId::Hevc => Some("hevc"),
                    rust_motion_media_core::VideoCodecId::Av1 => Some("av1"),
                    rust_motion_media_core::VideoCodecId::Vp9 => Some("vp9"),
                    rust_motion_media_core::VideoCodecId::ProRes => Some("prores"),
                    rust_motion_media_core::VideoCodecId::Mpeg4 => Some("mpeg4"),
                    _ => None,
                }
                .map(str::to_string);

                return Ok(VideoMetadata {
                    width,
                    height,
                    display_width,
                    display_height,
                    duration,
                    fps,
                    rotation,
                    video_stream_index: 0,
                    audio_stream_indices: (1..=meta.audio_streams.len()).collect(),
                    codec_name,
                    pixel_format: Some("yuv420p".into()),
                });
            }
        }
    }

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

    #[test]
    fn probe_video_metadata_resolves_mp4_without_ffprobe() {
        use rust_motion_media_container::{Mp4Muxer, Mp4MuxerOptions};
        use rust_motion_media_core::{
            ColorDescription, ContainerMuxer, EncodedPacket, Rational, TrackConfig, VideoCodecId,
            VideoTrackConfig,
        };

        let dir = std::env::temp_dir().join(format!(
            "rust-motion-native-video-probe-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mp4 = dir.join("clip.mp4");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&mp4)
            .unwrap();
        let mut muxer = Mp4Muxer::new(
            file,
            &[TrackConfig::Video(VideoTrackConfig {
                width: 192,
                height: 108,
                timescale: 24,
                fps: Rational::new(24, 1),
                codec: VideoCodecId::H264,
                codec_config: vec![1, 100, 0, 30, 0xFF, 0xE1],
                color: ColorDescription::bt709_limited(),
            })],
            Mp4MuxerOptions::default(),
        )
        .unwrap();
        for i in 0..6 {
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
        muxer.finish().unwrap();

        let meta = probe_video_metadata(mp4.to_str().unwrap()).unwrap();
        assert_eq!((meta.width, meta.height), (192, 108));
        assert_eq!((meta.display_width, meta.display_height), (192, 108));
        assert_eq!(meta.codec_name.as_deref(), Some("h264"));
        assert!((meta.fps.unwrap() - 24.0).abs() < 1e-6);
        assert!((meta.duration.unwrap() - 0.25).abs() < 1e-4);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn native_hevc_mp4_decodes_frames_with_seek_and_matches_ffmpeg() {
        let encoders = Command::new("ffmpeg")
            .args(["-hide_banner", "-encoders"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        let hevc_enc = if encoders.contains("libx265") {
            "libx265"
        } else if encoders.contains("hevc_videotoolbox") {
            "hevc_videotoolbox"
        } else {
            eprintln!("skipping native HEVC video_cache test: no HEVC encoder in ffmpeg");
            return;
        };

        let dir = std::env::temp_dir().join(format!(
            "rust-motion-native-hevc-cache-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mp4 = dir.join("hevc_clip.mp4");
        let generated = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=64x48:rate=10:duration=1",
                "-c:v",
                hevc_enc,
                "-tag:v",
                "hvc1",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&mp4)
            .status()
            .unwrap();
        assert!(generated.success());

        // Verify metadata resolves natively with codec=hevc.
        let meta = probe_video_metadata(mp4.to_str().unwrap()).unwrap();
        assert_eq!(meta.codec_name.as_deref(), Some("hevc"));
        assert_eq!((meta.width, meta.height), (64, 48));

        // Verify sequential + backward seek decode via pure-Rust NativeHevcDecoder.
        let cache = VideoFrameCache::default();
        let f0 = cache.load(mp4.to_str().unwrap(), 0.0, 10.0, false).unwrap();
        let f5 = cache.load(mp4.to_str().unwrap(), 0.5, 10.0, false).unwrap();
        // Force a backward seek beyond cached window after clearing cache state.
        cache.state.lock().unwrap().frames.clear();
        cache.state.lock().unwrap().bytes = 0;
        let f2 = cache.load(mp4.to_str().unwrap(), 0.2, 10.0, false).unwrap();

        assert_eq!(f0.dimensions(), (64, 48));
        assert_eq!(f5.dimensions(), (64, 48));
        assert_eq!(f2.dimensions(), (64, 48));
        assert!(cache.spawn_count() >= 2);
        {
            let pool = cache.decoders.lock().unwrap();
            let (_, dec_arc) = pool.sessions.back().unwrap();
            let dec = dec_arc.lock().unwrap();
            assert!(
                matches!(dec.inner, DecoderImpl::NativeHevc(_)),
                "expected NativeHevc decoder session"
            );
        }

        // Differential comparison against FFmpeg raw RGBA decode for frames 0, 2 (backward seek), and 5.
        let ff_all_rgba = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&mp4)
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .output()
            .unwrap();
        assert!(ff_all_rgba.status.success());
        let frame_rgba_len = 64 * 48 * 4;
        assert!(ff_all_rgba.stdout.len() >= 6 * frame_rgba_len);
        for (frame_idx, img) in [(0usize, &f0), (2usize, &f2), (5usize, &f5)] {
            let ff_slice =
                &ff_all_rgba.stdout[frame_idx * frame_rgba_len..(frame_idx + 1) * frame_rgba_len];
            let mut max_diff = 0u8;
            let mut max_coord = (0usize, 0usize);
            let mut max_actual = [0u8; 4];
            let mut max_expected = [0u8; 4];
            for (i, (actual_px, expected_px)) in img
                .as_raw()
                .chunks_exact(4)
                .zip(ff_slice.chunks_exact(4))
                .enumerate()
            {
                let d = actual_px
                    .iter()
                    .zip(expected_px.iter())
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap_or(0);
                if d > max_diff {
                    max_diff = d;
                    max_coord = (i % 64, i / 64);
                    max_actual.copy_from_slice(actual_px);
                    max_expected.copy_from_slice(expected_px);
                }
            }
            assert!(
                max_diff <= 10,
                "native HEVC vs FFmpeg RGBA max channel diff too large at frame {frame_idx}: {max_diff} at {max_coord:?} (actual={max_actual:?}, expected={max_expected:?})"
            );
        }

        cache.shutdown();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
