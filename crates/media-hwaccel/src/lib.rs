//! Hardware-accelerated video encoding and decoding backends for Rust Motion.
//!
//! Currently implements native Apple VideoToolbox (`H.264` and `HEVC`) on macOS using
//! direct framework FFI with zero third-party runtime dependencies. Non-macOS and WASM
//! targets compile cleanly with `is_videotoolbox_available() == false`.

use rust_motion_media_core::{
    CodecCapabilities, EncodedPacket, MediaError, VideoCodecId, VideoEncodeConfig, VideoEncoder,
    VideoFrame,
};

#[cfg(target_os = "macos")]
mod audiotoolbox;
#[cfg(target_os = "macos")]
mod videotoolbox;

/// Decoded RGBA8 video frame produced by [`HardwareVideoDecoder`].
#[derive(Debug, Clone)]
pub struct DecodedVideoFrame {
    pub width: u32,
    pub height: u32,
    pub pts: i64,
    pub rgba: Vec<u8>,
}

/// Returns `true` when Apple VideoToolbox hardware acceleration is available on this platform.
pub const fn is_videotoolbox_available() -> bool {
    cfg!(target_os = "macos")
}

/// Returns `true` when Apple AudioToolbox hardware/OS AAC encoding is available on this platform.
pub const fn is_audiotoolbox_available() -> bool {
    cfg!(target_os = "macos")
}

/// Hardware/OS-accelerated AAC-LC audio encoder (`f32` interleaved PCM -> `mp4a` AAC access units).
pub struct HardwareAacEncoder {
    #[cfg(target_os = "macos")]
    inner: audiotoolbox::AtAacEncoder,
}

impl HardwareAacEncoder {
    /// Construct an AAC-LC encoder targeting `sample_rate` Hz (e.g. 48000), `channels` (1 or 2),
    /// and target bitrate in bps (e.g. `192_000`).
    pub fn new(sample_rate: u32, channels: u16, bitrate: u32) -> Result<Self, MediaError> {
        #[cfg(target_os = "macos")]
        {
            Ok(Self {
                inner: audiotoolbox::AtAacEncoder::new(sample_rate, channels, bitrate)?,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (sample_rate, channels, bitrate);
            Err(MediaError::Unsupported(
                "AAC encoding via AudioToolbox is only available on macOS".into(),
            ))
        }
    }

    pub fn sample_rate(&self) -> u32 {
        #[cfg(target_os = "macos")]
        {
            self.inner.sample_rate()
        }
        #[cfg(not(target_os = "macos"))]
        {
            0
        }
    }

    pub fn channels(&self) -> u16 {
        #[cfg(target_os = "macos")]
        {
            self.inner.channels()
        }
        #[cfg(not(target_os = "macos"))]
        {
            0
        }
    }

    /// ISO 14496-3 `AudioSpecificConfig` bytes for the MP4 `esds` descriptor.
    pub fn audio_specific_config(&self) -> Vec<u8> {
        #[cfg(target_os = "macos")]
        {
            self.inner.audio_specific_config()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Vec::new()
        }
    }

    /// Feed interleaved 32-bit float PCM samples (`frames * channels`) and drain complete AAC packets.
    pub fn encode_interleaved_f32(
        &mut self,
        samples: &[f32],
    ) -> Result<Vec<EncodedPacket>, MediaError> {
        #[cfg(target_os = "macos")]
        {
            self.inner.encode_interleaved_f32(samples)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = samples;
            Err(MediaError::Unsupported(
                "AAC encoding via AudioToolbox is only available on macOS".into(),
            ))
        }
    }

    /// Zero-pad any partial frame and drain trailing AAC access units.
    pub fn flush(&mut self) -> Result<Vec<EncodedPacket>, MediaError> {
        #[cfg(target_os = "macos")]
        {
            self.inner.flush()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Ok(Vec::new())
        }
    }
}

/// Returns the list of codecs supported by native hardware acceleration on this host.
pub fn hardware_video_codecs() -> Vec<VideoCodecId> {
    if is_videotoolbox_available() {
        vec![VideoCodecId::H264, VideoCodecId::Hevc]
    } else {
        Vec::new()
    }
}

/// Hardware-accelerated streaming video encoder implementing [`VideoEncoder`].
pub struct HardwareVideoEncoder {
    #[cfg(target_os = "macos")]
    inner: videotoolbox::VtEncoder,
    #[cfg(not(target_os = "macos"))]
    codec: VideoCodecId,
}

impl HardwareVideoEncoder {
    /// Construct a hardware encoder targeting `codec` (`H264` or `Hevc`).
    pub fn new(codec: VideoCodecId) -> Result<Self, MediaError> {
        #[cfg(target_os = "macos")]
        {
            Ok(Self {
                inner: videotoolbox::VtEncoder::new(codec)?,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = codec;
            Err(MediaError::Unsupported(
                "hardware video encoding via VideoToolbox is only available on macOS".into(),
            ))
        }
    }
}

impl VideoEncoder for HardwareVideoEncoder {
    fn capabilities(&self) -> CodecCapabilities {
        #[cfg(target_os = "macos")]
        {
            self.inner.capabilities()
        }
        #[cfg(not(target_os = "macos"))]
        {
            CodecCapabilities {
                codec: self.codec,
                hardware_accelerated: false,
                supports_bframes: false,
                supported_pixel_formats: Vec::new(),
            }
        }
    }

    fn configure(&mut self, config: &VideoEncodeConfig) -> Result<(), MediaError> {
        #[cfg(target_os = "macos")]
        {
            self.inner.configure(config)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = config;
            Err(MediaError::Unsupported(
                "hardware video encoding via VideoToolbox is only available on macOS".into(),
            ))
        }
    }

    fn codec_config(&self) -> Vec<u8> {
        #[cfg(target_os = "macos")]
        {
            self.inner.codec_config()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Vec::new()
        }
    }

    fn encode(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, MediaError> {
        #[cfg(target_os = "macos")]
        {
            self.inner.encode(frame)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = frame;
            Err(MediaError::Unsupported(
                "hardware video encoding via VideoToolbox is only available on macOS".into(),
            ))
        }
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>, MediaError> {
        #[cfg(target_os = "macos")]
        {
            self.inner.flush()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Ok(Vec::new())
        }
    }
}

/// Hardware-accelerated length-prefixed MP4/MOV video decoder (`avcC`/`hvcC`).
pub struct HardwareVideoDecoder {
    #[cfg(target_os = "macos")]
    inner: videotoolbox::VtDecoder,
}

impl HardwareVideoDecoder {
    /// Initialize a hardware decoder from an MP4 `avcC` or `hvcC` configuration record.
    pub fn new(
        codec: VideoCodecId,
        width: u32,
        height: u32,
        timescale: u32,
        codec_config: &[u8],
    ) -> Result<Self, MediaError> {
        #[cfg(target_os = "macos")]
        {
            Ok(Self {
                inner: videotoolbox::VtDecoder::new(codec, width, height, timescale, codec_config)?,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (codec, width, height, timescale, codec_config);
            Err(MediaError::Unsupported(
                "hardware video decoding via VideoToolbox is only available on macOS".into(),
            ))
        }
    }

    /// Decode one length-prefixed access unit and return any newly output RGBA frames.
    pub fn decode(
        &mut self,
        sample_bytes: &[u8],
        pts: i64,
        dts: i64,
        duration: i64,
    ) -> Result<Vec<DecodedVideoFrame>, MediaError> {
        #[cfg(target_os = "macos")]
        {
            self.inner.decode(sample_bytes, pts, dts, duration)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (sample_bytes, pts, dts, duration);
            Err(MediaError::Unsupported(
                "hardware video decoding via VideoToolbox is only available on macOS".into(),
            ))
        }
    }

    /// Wait for any in-flight hardware decode frames and return them.
    pub fn flush(&mut self) -> Result<Vec<DecodedVideoFrame>, MediaError> {
        #[cfg(target_os = "macos")]
        {
            self.inner.flush()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Ok(Vec::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_reporting_matches_target_platform() {
        let codecs = hardware_video_codecs();
        if cfg!(target_os = "macos") {
            assert!(is_videotoolbox_available());
            assert_eq!(codecs, vec![VideoCodecId::H264, VideoCodecId::Hevc]);
        } else {
            assert!(!is_videotoolbox_available());
            assert!(codecs.is_empty());
        }
    }

    #[cfg(target_os = "macos")]
    mod macos_videotoolbox_tests {
        use super::*;
        use rust_motion_media_container::{Mp4Demuxer, Mp4Muxer, Mp4MuxerOptions};
        use rust_motion_media_core::{
            ColorDescription, ContainerMuxer, Rational, TrackConfig, VideoFrameBuffer,
            VideoTrackConfig,
        };
        use std::io::Cursor;
        use std::process::Command;

        fn synthetic_rgba_frames(width: u32, height: u32, count: usize) -> Vec<Vec<u8>> {
            (0..count)
                .map(|frame_idx| {
                    let mut buf = vec![0u8; (width as usize) * (height as usize) * 4];
                    for y in 0..height as usize {
                        for x in 0..width as usize {
                            let i = (y * (width as usize) + x) * 4;
                            let bar = ((x * 4) / (width as usize)).min(3);
                            let shift = (frame_idx * 12) as u8;
                            let (r, g, b) = match bar {
                                0 => (220u8.saturating_sub(shift), 40, 40),
                                1 => (40, 200u8.saturating_sub(shift), 40),
                                2 => (40, 40, 210u8.saturating_sub(shift)),
                                _ => (220, 220, 40u8.saturating_add(shift)),
                            };
                            buf[i] = r;
                            buf[i + 1] = g;
                            buf[i + 2] = b;
                            buf[i + 3] = 255;
                        }
                    }
                    buf
                })
                .collect()
        }

        #[test]
        fn videotoolbox_h264_encode_mux_and_decode_roundtrip() {
            let (w, h) = (128u32, 96u32);
            let mut encoder = HardwareVideoEncoder::new(VideoCodecId::H264).unwrap();
            encoder
                .configure(&VideoEncodeConfig {
                    width: w,
                    height: h,
                    fps: Rational::new(24, 1),
                    codec: VideoCodecId::H264,
                    crf: 18,
                    preset: "fast".into(),
                    color: ColorDescription::bt709_limited(),
                })
                .unwrap();

            let dir = tempfile::tempdir().unwrap();
            let mp4_path = dir.path().join("vt_h264.mp4");
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&mp4_path)
                .unwrap();
            let track = TrackConfig::Video(VideoTrackConfig {
                width: w,
                height: h,
                timescale: 24,
                fps: Rational::new(24, 1),
                codec: VideoCodecId::H264,
                codec_config: Vec::new(),
                color: ColorDescription::bt709_limited(),
            });
            let mut muxer = Mp4Muxer::new(file, &[track], Mp4MuxerOptions::default()).unwrap();

            let frames = synthetic_rgba_frames(w, h, 8);
            let mut wrote_config = false;
            for (idx, rgba) in frames.iter().enumerate() {
                let pkts = encoder
                    .encode(&VideoFrame {
                        width: w,
                        height: h,
                        pts: idx as i64,
                        duration: 1,
                        color: ColorDescription::bt709_limited(),
                        buffer: VideoFrameBuffer::CpuRgba(rgba.clone()),
                    })
                    .unwrap();
                if !wrote_config {
                    let cfg = encoder.codec_config();
                    if !cfg.is_empty() {
                        muxer.set_video_codec_config(0, cfg).unwrap();
                        wrote_config = true;
                    }
                }
                for pkt in pkts {
                    muxer.write_packet(&pkt).unwrap();
                }
            }
            for pkt in encoder.flush().unwrap() {
                muxer.write_packet(&pkt).unwrap();
            }
            if !wrote_config {
                muxer
                    .set_video_codec_config(0, encoder.codec_config())
                    .unwrap();
            }
            muxer.finish().unwrap();

            if Command::new("ffprobe")
                .arg("-version")
                .output()
                .is_ok_and(|o| o.status.success())
            {
                let probe = Command::new("ffprobe")
                    .args([
                        "-v",
                        "error",
                        "-show_entries",
                        "stream=codec_name,width,height,nb_frames",
                        "-of",
                        "default=noprint_wrappers=1",
                    ])
                    .arg(&mp4_path)
                    .output()
                    .unwrap();
                assert!(probe.status.success());
                let txt = String::from_utf8_lossy(&probe.stdout);
                assert!(txt.contains("codec_name=h264"), "{txt}");
                assert!(txt.contains("width=128"), "{txt}");
                assert!(txt.contains("height=96"), "{txt}");
                assert!(txt.contains("nb_frames=8"), "{txt}");
            }

            // Decode back with VideoToolbox HardwareVideoDecoder.
            let mut demuxer = Mp4Demuxer::open_path(&mp4_path).unwrap();
            let track = demuxer.video_tracks()[0].clone();
            assert_eq!(track.codec, VideoCodecId::H264);
            assert!(!track.codec_config.is_empty());
            assert_eq!(track.samples.len(), 8);

            let mut decoder = HardwareVideoDecoder::new(
                track.codec,
                track.width,
                track.height,
                track.timescale,
                &track.codec_config,
            )
            .unwrap();
            let mut sample_buf = Vec::new();
            let mut decoded = Vec::new();
            for i in 0..track.samples.len() {
                let s = demuxer.read_sample(0, i, &mut sample_buf).unwrap();
                decoded.extend(
                    decoder
                        .decode(&sample_buf, s.pts, s.dts, i64::from(s.duration))
                        .unwrap(),
                );
            }
            decoded.extend(decoder.flush().unwrap());
            assert_eq!(decoded.len(), 8);
            for (idx, frame) in decoded.iter().enumerate() {
                assert_eq!((frame.width, frame.height), (w, h));
                assert_eq!(frame.pts, idx as i64);
                let expected_r = 220u8 - (idx * 12) as u8;
                let px_idx = ((48 * w as usize) + 16) * 4;
                let r = frame.rgba[px_idx];
                let g = frame.rgba[px_idx + 1];
                let b = frame.rgba[px_idx + 2];
                assert!(
                    r.abs_diff(expected_r) <= 25 && r > g.saturating_add(80) && b < 80,
                    "unexpected red bar pixel at frame {idx} (expected R~{expected_r}): {:?}",
                    &frame.rgba[px_idx..px_idx + 4]
                );
            }
        }

        #[test]
        fn videotoolbox_hevc_encode_mux_and_pure_rust_hevc_decode_roundtrip() {
            let (w, h) = (128u32, 96u32);
            let mut encoder = HardwareVideoEncoder::new(VideoCodecId::Hevc).unwrap();
            encoder
                .configure(&VideoEncodeConfig {
                    width: w,
                    height: h,
                    fps: Rational::new(24, 1),
                    codec: VideoCodecId::Hevc,
                    crf: 20,
                    preset: "medium".into(),
                    color: ColorDescription::bt709_limited(),
                })
                .unwrap();

            let mut muxer = Mp4Muxer::new(
                Cursor::new(Vec::new()),
                &[TrackConfig::Video(VideoTrackConfig {
                    width: w,
                    height: h,
                    timescale: 24,
                    fps: Rational::new(24, 1),
                    codec: VideoCodecId::Hevc,
                    codec_config: Vec::new(),
                    color: ColorDescription::bt709_limited(),
                })],
                Mp4MuxerOptions::default(),
            )
            .unwrap();

            let frames = synthetic_rgba_frames(w, h, 8);
            for (idx, rgba) in frames.iter().enumerate() {
                for pkt in encoder
                    .encode(&VideoFrame {
                        width: w,
                        height: h,
                        pts: idx as i64,
                        duration: 1,
                        color: ColorDescription::bt709_limited(),
                        buffer: VideoFrameBuffer::CpuRgba(rgba.clone()),
                    })
                    .unwrap()
                {
                    muxer.write_packet(&pkt).unwrap();
                }
            }
            for pkt in encoder.flush().unwrap() {
                muxer.write_packet(&pkt).unwrap();
            }
            muxer
                .set_video_codec_config(0, encoder.codec_config())
                .unwrap();
            let mp4_bytes = muxer.into_inner().unwrap().into_inner();

            let mut demuxer = Mp4Demuxer::open(Cursor::new(mp4_bytes)).unwrap();
            let track = demuxer.video_tracks()[0].clone();
            assert_eq!(track.codec, VideoCodecId::Hevc);
            assert!(!track.codec_config.is_empty());
            assert_eq!(track.samples.len(), 8);

            let mut pure_hevc = rust_motion_hevc::Decoder::with_threads(1);
            pure_hevc.configure_hvcc(&track.codec_config).unwrap();
            let mut sample_buf = Vec::new();
            let mut pics = Vec::new();
            for i in 0..track.samples.len() {
                let s = demuxer.read_sample(0, i, &mut sample_buf).unwrap();
                pics.extend(pure_hevc.decode(&sample_buf, s.pts).unwrap());
            }
            pics.extend(pure_hevc.flush());
            assert_eq!(pics.len(), 8);
            for pic in &pics {
                assert_eq!((pic.width, pic.height), (w, h));
            }
        }

        #[test]
        fn videotoolbox_h264_decoder_matches_ffmpeg_with_bframes() {
            let ffmpeg_ok = Command::new("ffmpeg")
                .arg("-version")
                .output()
                .is_ok_and(|o| o.status.success());
            if !ffmpeg_ok {
                return;
            }
            let encoders = Command::new("ffmpeg")
                .args(["-hide_banner", "-encoders"])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default();
            let h264_enc = if encoders.contains("libx264") {
                "libx264"
            } else if encoders.contains("h264_videotoolbox") {
                "h264_videotoolbox"
            } else {
                return;
            };

            let dir = tempfile::tempdir().unwrap();
            let mp4 = dir.path().join("testsrc2_h264.mp4");
            let gen = Command::new("ffmpeg")
                .args([
                    "-y",
                    "-loglevel",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=64x48:rate=10:duration=1",
                    "-c:v",
                    h264_enc,
                    "-pix_fmt",
                    "yuv420p",
                ])
                .arg(&mp4)
                .status()
                .unwrap();
            assert!(gen.success());

            let mut demuxer = Mp4Demuxer::open_path(&mp4).unwrap();
            let track = demuxer.video_tracks()[0].clone();
            assert_eq!(track.codec, VideoCodecId::H264);
            assert_eq!(track.samples.len(), 10);

            let mut decoder = HardwareVideoDecoder::new(
                track.codec,
                track.width,
                track.height,
                track.timescale,
                &track.codec_config,
            )
            .unwrap();
            let mut sample_buf = Vec::new();
            let mut decoded = Vec::new();
            for i in 0..track.samples.len() {
                let s = demuxer.read_sample(0, i, &mut sample_buf).unwrap();
                decoded.extend(
                    decoder
                        .decode(&sample_buf, s.pts, s.dts, i64::from(s.duration))
                        .unwrap(),
                );
            }
            decoded.extend(decoder.flush().unwrap());
            decoded.sort_by_key(|f| f.pts);
            assert_eq!(decoded.len(), 10);

            let ff = Command::new("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(&mp4)
                .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
                .output()
                .unwrap();
            assert!(ff.status.success());
            let frame_len = 64 * 48 * 4;
            assert!(ff.stdout.len() >= 6 * frame_len);

            // Compare BT.601 / CoreVideo color conversion against FFmpeg on frames 0, 2, 5.
            // (CoreVideo chroma upsampling smoothly filters 4:2:0 edges, so allow chroma-edge delta
            // on 1px sharp transitions while verifying non-edge pixels & timing match closely.)
            for idx in [0usize, 2usize, 5usize] {
                let actual = &decoded[idx].rgba;
                let expected = &ff.stdout[idx * frame_len..(idx + 1) * frame_len];
                let mean_abs: f64 = actual
                    .iter()
                    .zip(expected.iter())
                    .map(|(a, b)| f64::from(a.abs_diff(*b)))
                    .sum::<f64>()
                    / (frame_len as f64);
                assert!(
                    mean_abs < 8.0,
                    "VideoToolbox H.264 vs FFmpeg mean abs channel diff too large at frame {idx}: {mean_abs}"
                );
            }
        }

        #[test]
        #[ignore = "throughput benchmark run explicitly via cargo test --release -- --ignored"]
        fn videotoolbox_vs_pure_rust_1080p_throughput() {
            use rust_motion_h264enc::{
                Encoder as PureEncoder, EncoderConfig as PureConfig, PacketFormat, Preset, Profile,
                RateControl, YuvFrame,
            };
            use std::time::Instant;

            let (w, h, frames) = (1920u32, 1080u32, 60usize);
            let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let i = (y * (w as usize) + x) * 4;
                    rgba[i] = ((x * 255) / w as usize) as u8;
                    rgba[i + 1] = ((y * 255) / h as usize) as u8;
                    rgba[i + 2] = 160;
                    rgba[i + 3] = 255;
                }
            }
            let y_plane = vec![128u8; (w as usize) * (h as usize)];
            let uv_plane = vec![128u8; ((w as usize) / 2) * ((h as usize) / 2)];

            // Pure-Rust h264enc (1080p, Preset::Speed)
            let mut pure_cfg = PureConfig::new(w, h, 30, 1);
            pure_cfg.preset = Preset::Speed;
            pure_cfg.profile = Profile::High;
            pure_cfg.rate = RateControl::Crf(22.0);
            pure_cfg.format = PacketFormat::LengthPrefixed;
            let mut pure_enc = PureEncoder::new(pure_cfg).unwrap();
            let t0 = Instant::now();
            let mut pure_bytes = 0usize;
            for f in 0..frames {
                let yuv = YuvFrame {
                    y: &y_plane,
                    u: &uv_plane,
                    v: &uv_plane,
                    y_stride: w as usize,
                    uv_stride: (w / 2) as usize,
                };
                for pkt in pure_enc.encode(&yuv, f as i64).unwrap() {
                    pure_bytes += pkt.data.len();
                }
            }
            for pkt in pure_enc.flush() {
                pure_bytes += pkt.data.len();
            }
            let pure_secs = t0.elapsed().as_secs_f64();

            // VideoToolbox H.264 & HEVC (1080p RGBA directly)
            for (label, codec) in [
                ("vt_h264", VideoCodecId::H264),
                ("vt_hevc", VideoCodecId::Hevc),
            ] {
                let mut enc = HardwareVideoEncoder::new(codec).unwrap();
                enc.configure(&VideoEncodeConfig {
                    width: w,
                    height: h,
                    fps: Rational::new(30, 1),
                    codec,
                    crf: 22,
                    preset: "fast".into(),
                    color: ColorDescription::bt709_limited(),
                })
                .unwrap();
                let start = Instant::now();
                let mut total_bytes = 0usize;
                for f in 0..frames {
                    for pkt in enc
                        .encode(&VideoFrame {
                            width: w,
                            height: h,
                            pts: f as i64,
                            duration: 1,
                            color: ColorDescription::bt709_limited(),
                            buffer: VideoFrameBuffer::CpuRgba(rgba.clone()),
                        })
                        .unwrap()
                    {
                        total_bytes += pkt.data.len();
                    }
                }
                for pkt in enc.flush().unwrap() {
                    total_bytes += pkt.data.len();
                }
                let secs = start.elapsed().as_secs_f64();
                eprintln!(
                    "BENCH 1080p60 {label}: {:.1} fps ({secs:.3}s, {} KiB)",
                    frames as f64 / secs,
                    total_bytes / 1024
                );
            }
            eprintln!(
                "BENCH 1080p60 pure_rust_h264: {:.1} fps ({pure_secs:.3}s, {} KiB)",
                frames as f64 / pure_secs,
                pure_bytes / 1024
            );
        }

        #[test]
        fn audiotoolbox_aac_encodes_sine_wave_and_muxes_playable_mp4() {
            use rust_motion_media_container::probe_isobmff_path;
            use rust_motion_media_core::{AudioCodecId, AudioTrackConfig};

            let sr = 48_000u32;
            let channels = 2u16;
            let mut aac = HardwareAacEncoder::new(sr, channels, 192_000).unwrap();
            let asc = aac.audio_specific_config();
            assert!(!asc.is_empty());

            // Generate 0.5s (24,000 frames) of 440 Hz stereo sine wave.
            let num_frames = sr as usize / 2;
            let mut pcm = Vec::with_capacity(num_frames * 2);
            for n in 0..num_frames {
                let sample =
                    (2.0 * std::f32::consts::PI * 440.0 * (n as f32) / (sr as f32)).sin() * 0.5;
                pcm.push(sample);
                pcm.push(sample);
            }

            let mut audio_pkts = aac.encode_interleaved_f32(&pcm).unwrap();
            audio_pkts.extend(aac.flush().unwrap());
            assert!(
                audio_pkts.len() >= 20,
                "expected ~24 AAC access units for 0.5s @ 48kHz, got {}",
                audio_pkts.len()
            );

            // Pair with an 8-frame VideoToolbox H.264 stream into an MP4.
            let (w, h) = (128u32, 96u32);
            let mut venc = HardwareVideoEncoder::new(VideoCodecId::H264).unwrap();
            venc.configure(&VideoEncodeConfig {
                width: w,
                height: h,
                fps: Rational::new(16, 1),
                codec: VideoCodecId::H264,
                crf: 22,
                preset: "fast".into(),
                color: ColorDescription::bt709_limited(),
            })
            .unwrap();

            let dir = tempfile::tempdir().unwrap();
            let mp4_path = dir.path().join("av_audiotoolbox.mp4");
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&mp4_path)
                .unwrap();

            let tracks = [
                TrackConfig::Video(VideoTrackConfig {
                    width: w,
                    height: h,
                    timescale: 16,
                    fps: Rational::new(16, 1),
                    codec: VideoCodecId::H264,
                    codec_config: Vec::new(),
                    color: ColorDescription::bt709_limited(),
                }),
                TrackConfig::Audio(AudioTrackConfig {
                    codec: AudioCodecId::Aac,
                    sample_rate: sr,
                    channels,
                    timescale: sr,
                    codec_config: asc,
                }),
            ];
            let mut muxer = Mp4Muxer::new(file, &tracks, Mp4MuxerOptions::default()).unwrap();

            let frames = synthetic_rgba_frames(w, h, 8);
            for (idx, rgba) in frames.iter().enumerate() {
                for pkt in venc
                    .encode(&VideoFrame {
                        width: w,
                        height: h,
                        pts: idx as i64,
                        duration: 1,
                        color: ColorDescription::bt709_limited(),
                        buffer: VideoFrameBuffer::CpuRgba(rgba.clone()),
                    })
                    .unwrap()
                {
                    muxer.write_packet(&pkt).unwrap();
                }
            }
            for pkt in venc.flush().unwrap() {
                muxer.write_packet(&pkt).unwrap();
            }
            muxer
                .set_video_codec_config(0, venc.codec_config())
                .unwrap();
            for pkt in &audio_pkts {
                muxer.write_packet(pkt).unwrap();
            }
            muxer.finish().unwrap();

            let meta = probe_isobmff_path(&mp4_path).unwrap();
            assert_eq!(meta.video_streams.len(), 1);
            assert_eq!(meta.audio_streams.len(), 1);
            assert_eq!(meta.audio_streams[0].codec, AudioCodecId::Aac);
            assert_eq!(meta.audio_streams[0].sample_rate, 48_000);
            assert_eq!(meta.audio_streams[0].channels, 2);

            if Command::new("ffprobe")
                .arg("-version")
                .output()
                .is_ok_and(|o| o.status.success())
            {
                let probe = Command::new("ffprobe")
                    .args([
                        "-v",
                        "error",
                        "-select_streams",
                        "a:0",
                        "-show_entries",
                        "stream=codec_name,sample_rate,channels",
                        "-of",
                        "default=noprint_wrappers=1",
                    ])
                    .arg(&mp4_path)
                    .output()
                    .unwrap();
                assert!(probe.status.success());
                let txt = String::from_utf8_lossy(&probe.stdout);
                assert!(txt.contains("codec_name=aac"), "{txt}");
                assert!(txt.contains("sample_rate=48000"), "{txt}");
                assert!(txt.contains("channels=2"), "{txt}");
            }

            if Command::new("ffmpeg")
                .arg("-version")
                .output()
                .is_ok_and(|o| o.status.success())
            {
                let dec = Command::new("ffmpeg")
                    .args(["-v", "error", "-i"])
                    .arg(&mp4_path)
                    .args(["-vn", "-f", "f32le", "-acodec", "pcm_f32le", "-"])
                    .output()
                    .unwrap();
                assert!(
                    dec.status.success(),
                    "ffmpeg AAC decode failed: {}",
                    String::from_utf8_lossy(&dec.stderr)
                );
                let rms: f64 = dec
                    .stdout
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| {
                        let v = f64::from(f32::from_le_bytes(*b));
                        v * v
                    })
                    .sum::<f64>()
                    / ((dec.stdout.len() / 4).max(1) as f64);
                assert!(
                    rms.sqrt() > 0.1,
                    "expected audible decoded sine wave RMS > 0.1, got {}",
                    rms.sqrt()
                );
            }
        }
    }
}
