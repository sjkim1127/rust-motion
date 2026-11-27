//! Built-in native MP4 video encoder sink (pure-Rust H.264 and Apple VideoToolbox H.264/HEVC).

use std::fs::{File, OpenOptions};

use rust_motion_h264enc::{
    ColorConfig, Encoder, EncoderConfig, PacketFormat, Preset, Profile, RateControl, YuvFrame,
};
use rust_motion_media_container::{Mp4Muxer, Mp4MuxerOptions};
use rust_motion_media_core::{
    ColorDescription, ContainerMuxer, EncodedPacket, Rational, TrackConfig, VideoCodecId,
    VideoEncodeConfig, VideoEncoder, VideoFrame, VideoFrameBuffer, VideoTrackConfig,
};
use rust_motion_media_hwaccel::{is_videotoolbox_available, HardwareVideoEncoder};

use super::*;

enum NativeVideoEncoderImpl {
    PureRustH264(Box<Encoder>),
    Hardware(Box<HardwareVideoEncoder>),
}

pub(super) struct NativeH264FrameSink<'a> {
    control: &'a RenderControl,
    started: Instant,
    input_width: u32,
    input_height: u32,
    output_width: u32,
    output_height: u32,
    total: u32,
    start_frame: u32,
    frame_step: u32,
    frame_duration_ticks: i64,
    y_stride: usize,
    uv_stride: usize,
    y_plane: Vec<u8>,
    u_plane: Vec<u8>,
    v_plane: Vec<u8>,
    encoder: NativeVideoEncoderImpl,
    muxer: Mp4Muxer<File>,
    codec_config_written: bool,
    encoded_count: u32,
}

impl<'a> NativeH264FrameSink<'a> {
    pub(super) fn new(
        config: &'a PipeConfig,
        output_width: u32,
        output_height: u32,
        started: Instant,
    ) -> Result<Self, RasterError> {
        if !matches!(config.codec, VideoCodec::H264 | VideoCodec::H265) {
            return Err(RasterError::ImageEncode(format!(
                "built-in native encoder currently supports H.264 and H.265 output, got {:?}",
                config.codec
            )));
        }
        if config.codec == VideoCodec::H265 && !is_videotoolbox_available() {
            return Err(RasterError::ImageEncode(
                "built-in native H.265 encoding requires Apple VideoToolbox on macOS".into(),
            ));
        }
        if !active_audio_tracks(config).is_empty() {
            return Err(RasterError::ImageEncode(
                "built-in native MP4 encoder does not yet support audio track mixing; install FFmpeg or render without audio tracks"
                    .into(),
            ));
        }

        let effective_fps = config.fps / f64::from(config.frame_step.max(1));
        let rational_fps = Rational::from_fps(effective_fps);
        let use_hardware = config.codec == VideoCodec::H265
            || (config.hw_accel == HwAccel::VideoToolbox && is_videotoolbox_available());
        let codec_id = match config.codec {
            VideoCodec::H265 => VideoCodecId::Hevc,
            _ => VideoCodecId::H264,
        };

        let (encoder, initial_codec_config, allocate_yuv) = if use_hardware {
            let mut hw = HardwareVideoEncoder::new(codec_id).map_err(|e| {
                RasterError::ImageEncode(format!("Failed to create hardware video encoder: {e}"))
            })?;
            hw.configure(&VideoEncodeConfig {
                width: output_width,
                height: output_height,
                fps: rational_fps,
                codec: codec_id,
                crf: config.crf.min(51),
                preset: config.preset.clone(),
                color: ColorDescription::bt709_limited(),
            })
            .map_err(|e| {
                RasterError::ImageEncode(format!("Failed to configure hardware video encoder: {e}"))
            })?;
            let cfg = hw.codec_config();
            (NativeVideoEncoderImpl::Hardware(Box::new(hw)), cfg, false)
        } else {
            let (preset, profile, bframes) = match config.preset.to_ascii_lowercase().as_str() {
                "ultrafast" | "superfast" | "veryfast" => (Preset::Speed, Profile::Baseline, 0),
                "faster" | "fast" => (Preset::Speed, Profile::Main, 0),
                "slow" | "slower" | "veryslow" | "placebo" => (Preset::Quality, Profile::High, 2),
                _ => (Preset::Balanced, Profile::High, 0),
            };

            let mut enc_cfg = EncoderConfig::new(
                output_width,
                output_height,
                rational_fps.num,
                rational_fps.den,
            );
            enc_cfg.rate = RateControl::Crf(config.crf.min(51) as f32);
            enc_cfg.preset = preset;
            enc_cfg.profile = profile;
            enc_cfg.bframes = bframes;
            enc_cfg.color = ColorConfig {
                primaries: 1,
                transfer: 1,
                matrix: 1,
                full_range: false,
            };
            enc_cfg.format = PacketFormat::LengthPrefixed;

            let encoder = Encoder::new(enc_cfg).map_err(|e| {
                RasterError::ImageEncode(format!("Failed to initialize native H.264 encoder: {e}"))
            })?;
            let avcc = encoder.avcc();
            (
                NativeVideoEncoderImpl::PureRustH264(Box::new(encoder)),
                avcc,
                true,
            )
        };

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&config.output)
            .map_err(|e| {
                RasterError::ImageEncode(format!(
                    "Failed to open native MP4 output {}: {e}",
                    config.output.display()
                ))
            })?;

        let codec_config_written = !initial_codec_config.is_empty();
        let track = TrackConfig::Video(VideoTrackConfig {
            width: output_width,
            height: output_height,
            timescale: rational_fps.num,
            fps: rational_fps,
            codec: codec_id,
            codec_config: initial_codec_config,
            color: ColorDescription::bt709_limited(),
        });

        let muxer = Mp4Muxer::new(file, &[track], Mp4MuxerOptions::default()).map_err(|e| {
            RasterError::ImageEncode(format!("Failed to initialize native MP4 muxer: {e}"))
        })?;

        let y_stride = output_width as usize;
        let uv_stride = y_stride / 2;
        let (y_plane, u_plane, v_plane) = if allocate_yuv {
            let y_len = y_stride * (output_height as usize);
            let uv_len = uv_stride * ((output_height as usize) / 2);
            (vec![0u8; y_len], vec![0u8; uv_len], vec![0u8; uv_len])
        } else {
            (Vec::new(), Vec::new(), Vec::new())
        };

        Ok(Self {
            control: &config.control,
            started,
            input_width: config.width,
            input_height: config.height,
            output_width,
            output_height,
            total: config.duration_in_frames,
            start_frame: config.start_frame,
            frame_step: config.frame_step,
            frame_duration_ticks: i64::from(rational_fps.den),
            y_stride,
            uv_stride,
            y_plane,
            u_plane,
            v_plane,
            encoder,
            muxer,
            codec_config_written,
            encoded_count: 0,
        })
    }

    fn sync_codec_config(&mut self) -> Result<(), RasterError> {
        if self.codec_config_written {
            return Ok(());
        }
        if let NativeVideoEncoderImpl::Hardware(hw) = &self.encoder {
            let cfg = hw.codec_config();
            if !cfg.is_empty() {
                self.muxer.set_video_codec_config(0, cfg).map_err(|e| {
                    RasterError::ImageEncode(format!("Native MP4 codec config error: {e}"))
                })?;
                self.codec_config_written = true;
            }
        }
        Ok(())
    }

    fn write_media_packets(&mut self, packets: Vec<EncodedPacket>) -> Result<(), RasterError> {
        self.sync_codec_config()?;
        let den = self.frame_duration_ticks;
        for mut pkt in packets {
            pkt.stream_index = 0;
            pkt.pts = pkt.pts.saturating_mul(den);
            pkt.dts = pkt.dts.saturating_mul(den);
            pkt.duration = den;
            self.muxer.write_packet(&pkt).map_err(|e| {
                RasterError::ImageEncode(format!("Native MP4 mux write error: {e}"))
            })?;
            self.encoded_count = self.encoded_count.saturating_add(1).min(self.total);
            self.control.report_encoding_progress(EncodingProgress {
                encoded_frames: self.encoded_count,
                total_frames: self.total,
            });
        }
        Ok(())
    }

    fn write_h264enc_packets(
        &mut self,
        packets: Vec<rust_motion_h264enc::Packet>,
    ) -> Result<(), RasterError> {
        let converted = packets
            .into_iter()
            .map(|pkt| EncodedPacket {
                stream_index: 0,
                data: pkt.data,
                pts: pkt.pts,
                dts: pkt.dts,
                duration: 1,
                is_keyframe: pkt.keyframe,
            })
            .collect();
        self.write_media_packets(converted)
    }

    pub(super) fn finish(mut self) -> Result<(), RasterError> {
        match &mut self.encoder {
            NativeVideoEncoderImpl::PureRustH264(enc) => {
                let trailing = enc.flush();
                self.write_h264enc_packets(trailing)?;
            }
            NativeVideoEncoderImpl::Hardware(hw) => {
                let trailing = hw.flush().map_err(|e| {
                    RasterError::ImageEncode(format!("Hardware encoder flush error: {e}"))
                })?;
                self.write_media_packets(trailing)?;
            }
        }
        self.sync_codec_config()?;
        self.muxer
            .finish()
            .map_err(|e| RasterError::ImageEncode(format!("Native MP4 finish error: {e}")))
    }
}

impl FrameSink for NativeH264FrameSink<'_> {
    fn consume(&mut self, frame: u32, rgba: &[u8]) -> Result<(), RasterError> {
        self.control.check(self.started)?;
        let scaled = scale_rgba_frame(
            rgba,
            self.input_width,
            self.input_height,
            self.output_width,
            self.output_height,
        )?;
        match &mut self.encoder {
            NativeVideoEncoderImpl::PureRustH264(enc) => {
                rgba_to_yuv420_bt709(
                    &scaled,
                    self.output_width as usize,
                    self.output_height as usize,
                    &mut self.y_plane,
                    &mut self.u_plane,
                    &mut self.v_plane,
                );
                let yuv = YuvFrame {
                    y: &self.y_plane,
                    u: &self.u_plane,
                    v: &self.v_plane,
                    y_stride: self.y_stride,
                    uv_stride: self.uv_stride,
                };
                let packets = enc.encode(&yuv, i64::from(frame)).map_err(|e| {
                    RasterError::ImageEncode(format!(
                        "Native H.264 encode error on frame {frame}: {e}"
                    ))
                })?;
                self.write_h264enc_packets(packets)?;
            }
            NativeVideoEncoderImpl::Hardware(hw) => {
                let video_frame = VideoFrame {
                    width: self.output_width,
                    height: self.output_height,
                    pts: i64::from(frame),
                    duration: 1,
                    color: ColorDescription::bt709_limited(),
                    buffer: VideoFrameBuffer::CpuRgba(scaled),
                };
                let packets = hw.encode(&video_frame).map_err(|e| {
                    RasterError::ImageEncode(format!(
                        "Hardware video encode error on frame {frame}: {e}"
                    ))
                })?;
                self.write_media_packets(packets)?;
            }
        }

        if let Some(callback) = &self.control.progress {
            callback(RenderProgress {
                completed_frames: frame + 1,
                total_frames: self.total,
                frame: self.start_frame + frame * self.frame_step,
            });
        }
        Ok(())
    }
}

/// Convert RGBA8 (straight alpha composited over black) into BT.709 limited-range planar YUV 4:2:0.
fn rgba_to_yuv420_bt709(
    rgba: &[u8],
    width: usize,
    height: usize,
    y_plane: &mut [u8],
    u_plane: &mut [u8],
    v_plane: &mut [u8],
) {
    let uv_stride = width / 2;
    let rgb = |x: usize, y: usize| -> (f32, f32, f32) {
        let i = (y * width + x) * 4;
        let alpha = rgba[i + 3];
        if alpha == 255 {
            (
                f32::from(rgba[i]),
                f32::from(rgba[i + 1]),
                f32::from(rgba[i + 2]),
            )
        } else {
            let a = f32::from(alpha) / 255.0;
            (
                f32::from(rgba[i]) * a,
                f32::from(rgba[i + 1]) * a,
                f32::from(rgba[i + 2]) * a,
            )
        }
    };

    for y in 0..height {
        for x in 0..width {
            let (r, g, b) = rgb(x, y);
            let luma = 16.0 + (0.2126 * r + 0.7152 * g + 0.0722 * b) * (219.0 / 255.0);
            y_plane[y * width + x] = luma.round().clamp(16.0, 235.0) as u8;
        }
    }
    let k = 224.0 / 255.0;
    for cy in 0..height / 2 {
        for cx in 0..uv_stride {
            let (r0, g0, b0) = rgb(cx * 2, cy * 2);
            let (r1, g1, b1) = rgb(cx * 2 + 1, cy * 2);
            let (r2, g2, b2) = rgb(cx * 2, cy * 2 + 1);
            let (r3, g3, b3) = rgb(cx * 2 + 1, cy * 2 + 1);
            let r = (r0 + r1 + r2 + r3) * 0.25;
            let g = (g0 + g1 + g2 + g3) * 0.25;
            let b = (b0 + b1 + b2 + b3) * 0.25;
            let cb = 128.0 + (-0.1146 * r - 0.3854 * g + 0.5 * b) * k;
            let cr = 128.0 + (0.5 * r - 0.4542 * g - 0.0458 * b) * k;
            u_plane[cy * uv_stride + cx] = cb.round().clamp(16.0, 240.0) as u8;
            v_plane[cy * uv_stride + cx] = cr.round().clamp(16.0, 240.0) as u8;
        }
    }
}
