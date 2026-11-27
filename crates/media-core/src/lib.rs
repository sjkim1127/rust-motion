//! Backend-independent media core abstractions for Rust Motion.
//!
//! Defines shared pixel formats, ITU-T H.273 / CICP color metadata, codec identifiers,
//! rational frame timing, compressed packets, container/encoder traits, and error types.
//! Intentionally depends only on `thiserror` and `serde` so codecs, containers,
//! rasterizers, WASM players, and CLI tools can share contracts without dependency cycles.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Pixel storage format for uncompressed video frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PixelFormat {
    Rgba8,
    Bgra8,
    Yuv420p,
    Nv12,
    Yuv422p10le,
}

/// ISO/IEC 23091-2 (CICP) color primaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ColorPrimaries {
    #[default]
    Bt709,
    Bt601Ntsc,
    Bt601Pal,
    Bt2020,
    Unspecified,
}

impl ColorPrimaries {
    pub fn to_cicp_u16(self) -> u16 {
        match self {
            Self::Bt709 => 1,
            Self::Unspecified => 2,
            Self::Bt601Pal => 5,
            Self::Bt601Ntsc => 6,
            Self::Bt2020 => 9,
        }
    }

    pub fn from_cicp_u16(code: u16) -> Self {
        match code {
            1 => Self::Bt709,
            5 => Self::Bt601Pal,
            6 => Self::Bt601Ntsc,
            9 => Self::Bt2020,
            _ => Self::Unspecified,
        }
    }
}

/// ISO/IEC 23091-2 (CICP) transfer characteristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TransferCharacteristics {
    #[default]
    Bt709,
    Srgb,
    Pq,
    Hlg,
    Unspecified,
}

impl TransferCharacteristics {
    pub fn to_cicp_u16(self) -> u16 {
        match self {
            Self::Bt709 => 1,
            Self::Unspecified => 2,
            Self::Srgb => 13,
            Self::Pq => 16,
            Self::Hlg => 18,
        }
    }

    pub fn from_cicp_u16(code: u16) -> Self {
        match code {
            1 | 6 => Self::Bt709,
            13 => Self::Srgb,
            16 => Self::Pq,
            18 => Self::Hlg,
            _ => Self::Unspecified,
        }
    }
}

/// ISO/IEC 23091-2 (CICP) YCbCr matrix coefficients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum MatrixCoefficients {
    #[default]
    Bt709,
    Bt601,
    Bt2020Ncl,
    Unspecified,
}

impl MatrixCoefficients {
    pub fn to_cicp_u16(self) -> u16 {
        match self {
            Self::Bt709 => 1,
            Self::Unspecified => 2,
            Self::Bt601 => 6,
            Self::Bt2020Ncl => 9,
        }
    }

    pub fn from_cicp_u16(code: u16) -> Self {
        match code {
            1 => Self::Bt709,
            5 | 6 => Self::Bt601,
            9 => Self::Bt2020Ncl,
            _ => Self::Unspecified,
        }
    }
}

/// Sample value quantization range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ColorRange {
    #[default]
    Limited,
    Full,
}

/// Full color space metadata attached to streams and frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ColorDescription {
    pub primaries: ColorPrimaries,
    pub transfer: TransferCharacteristics,
    pub matrix: MatrixCoefficients,
    pub range: ColorRange,
}

impl Default for ColorDescription {
    fn default() -> Self {
        Self::bt709_limited()
    }
}

impl ColorDescription {
    /// Standard HD/UHD SDR delivery colorspace (Rec.709, studio/limited range).
    pub const fn bt709_limited() -> Self {
        Self {
            primaries: ColorPrimaries::Bt709,
            transfer: TransferCharacteristics::Bt709,
            matrix: MatrixCoefficients::Bt709,
            range: ColorRange::Limited,
        }
    }

    /// Standard sRGB full-range image representation.
    pub const fn srgb_full() -> Self {
        Self {
            primaries: ColorPrimaries::Bt709,
            transfer: TransferCharacteristics::Srgb,
            matrix: MatrixCoefficients::Bt709,
            range: ColorRange::Full,
        }
    }

    /// Unspecified primaries/transfer/matrix with limited range (default when `colr` is absent).
    pub const fn unspecified_limited() -> Self {
        Self {
            primaries: ColorPrimaries::Unspecified,
            transfer: TransferCharacteristics::Unspecified,
            matrix: MatrixCoefficients::Unspecified,
            range: ColorRange::Limited,
        }
    }

    /// Whether any non-unspecified color tag is present.
    pub fn is_specified(&self) -> bool {
        self.primaries != ColorPrimaries::Unspecified
            || self.transfer != TransferCharacteristics::Unspecified
            || self.matrix != MatrixCoefficients::Unspecified
    }
}

/// Logical video codec identifiers independent of backend implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VideoCodecId {
    H264,
    Hevc,
    Apv,
    Av1,
    Vp9,
    ProRes,
    Mpeg4,
    Gif,
    Unknown,
}

/// Logical audio codec identifiers independent of backend implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AudioCodecId {
    Aac,
    Opus,
    PcmS16Le,
    PcmF32Le,
    Mp3,
    Flac,
    Unknown,
}

/// Media container formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContainerFormat {
    Mp4,
    Mov,
    Mxf,
    Mkv,
    WebM,
    Wav,
}

/// Exact rational timing ratio `num / den`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rational {
    pub num: u32,
    pub den: u32,
}

impl Rational {
    pub const fn new(num: u32, den: u32) -> Self {
        Self { num, den }
    }

    pub fn as_f64(self) -> f64 {
        if self.den == 0 {
            0.0
        } else {
            f64::from(self.num) / f64::from(self.den)
        }
    }

    /// Convert a floating-point frames-per-second rate into an exact rational `num / den`.
    /// Recognizes integer frame rates (`30 -> 30/1`) and standard NTSC rates (`29.97 -> 30000/1001`).
    pub fn from_fps(fps: f64) -> Self {
        fn gcd(mut a: u32, mut b: u32) -> u32 {
            while b != 0 {
                let r = a % b;
                a = b;
                b = r;
            }
            a.max(1)
        }

        if !fps.is_finite() || fps <= 0.0 {
            return Self::new(30, 1);
        }
        let rounded = fps.round().max(1.0);
        if (fps - rounded).abs() < 1e-3 && rounded <= f64::from(u32::MAX) {
            return Self::new(rounded as u32, 1);
        }
        let ntsc = (fps * 1.001).round();
        if ntsc >= 1.0
            && ntsc * 1000.0 <= f64::from(u32::MAX)
            && (fps - ntsc * 1000.0 / 1001.0).abs() < 1e-3
        {
            return Self::new(ntsc as u32 * 1000, 1001);
        }
        let num = (fps * 1000.0).round().clamp(1.0, f64::from(u32::MAX)) as u32;
        let g = gcd(num, 1000);
        Self::new(num / g, 1000 / g)
    }
}

/// CPU-accessible video pixel storage variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoFrameBuffer {
    CpuRgba(Vec<u8>),
    CpuYuv420 {
        y: Vec<u8>,
        u: Vec<u8>,
        v: Vec<u8>,
        y_stride: usize,
        uv_stride: usize,
    },
}

/// Decoded or pre-encode video frame with presentation metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub pts: i64,
    pub duration: i64,
    pub color: ColorDescription,
    pub buffer: VideoFrameBuffer,
}

/// One compressed elementary stream access unit / packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedPacket {
    pub stream_index: u32,
    pub data: Vec<u8>,
    pub pts: i64,
    pub dts: i64,
    pub duration: i64,
    pub is_keyframe: bool,
}

/// Encoder configuration parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoEncodeConfig {
    pub width: u32,
    pub height: u32,
    pub fps: Rational,
    pub codec: VideoCodecId,
    pub crf: u32,
    pub preset: String,
    pub color: ColorDescription,
}

/// Capabilities exposed by a concrete encoder implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecCapabilities {
    pub codec: VideoCodecId,
    pub hardware_accelerated: bool,
    pub supports_bframes: bool,
    pub supported_pixel_formats: Vec<PixelFormat>,
}

/// Video elementary track descriptor for container muxing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoTrackConfig {
    pub width: u32,
    pub height: u32,
    pub timescale: u32,
    pub fps: Rational,
    pub codec: VideoCodecId,
    pub codec_config: Vec<u8>,
    pub color: ColorDescription,
}

/// Audio elementary track descriptor for container muxing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTrackConfig {
    pub codec: AudioCodecId,
    pub sample_rate: u32,
    pub channels: u16,
    pub timescale: u32,
    pub codec_config: Vec<u8>,
}

/// Unified container track configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackConfig {
    Video(VideoTrackConfig),
    Audio(AudioTrackConfig),
}

/// Probed container + stream metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaMetadata {
    pub format: Option<ContainerFormat>,
    pub duration_secs: Option<f64>,
    pub video_streams: Vec<VideoStreamMetadata>,
    pub audio_streams: Vec<AudioStreamMetadata>,
}

/// Probed metadata for one video stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoStreamMetadata {
    pub codec: VideoCodecId,
    pub width: u32,
    pub height: u32,
    pub fps: Rational,
    pub duration_secs: Option<f64>,
    pub rotation_degrees: i32,
    pub color: ColorDescription,
}

/// Probed metadata for one audio stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioStreamMetadata {
    pub codec: AudioCodecId,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_secs: Option<f64>,
}

/// Unified media engine error type.
#[derive(Debug, Error)]
pub enum MediaError {
    #[error("invalid media configuration: {0}")]
    InvalidConfig(String),
    #[error("unsupported media feature: {0}")]
    Unsupported(String),
    #[error("codec error: {0}")]
    Codec(String),
    #[error("container error: {0}")]
    Container(String),
    #[error("media I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Streaming video encoder contract.
pub trait VideoEncoder: Send {
    fn capabilities(&self) -> CodecCapabilities;
    fn configure(&mut self, config: &VideoEncodeConfig) -> Result<(), MediaError>;
    fn codec_config(&self) -> Vec<u8>;
    fn encode(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, MediaError>;
    fn flush(&mut self) -> Result<Vec<EncodedPacket>, MediaError>;
}

/// Streaming container muxer contract.
pub trait ContainerMuxer {
    fn write_packet(&mut self, packet: &EncodedPacket) -> Result<(), MediaError>;
    fn finish(&mut self) -> Result<(), MediaError>;
}

/// Native metadata probe contract.
pub trait MediaProbe {
    fn probe_bytes(&self, bytes: &[u8]) -> Result<MediaMetadata, MediaError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_from_fps_preserves_ntsc_and_integer_rates() {
        assert_eq!(Rational::from_fps(30.0), Rational::new(30, 1));
        assert_eq!(Rational::from_fps(60.0), Rational::new(60, 1));
        assert_eq!(Rational::from_fps(29.97), Rational::new(30000, 1001));
        assert_eq!(Rational::from_fps(23.976), Rational::new(24000, 1001));
        assert_eq!(Rational::from_fps(59.94), Rational::new(60000, 1001));
        assert_eq!(Rational::from_fps(12.5), Rational::new(25, 2));
        assert_eq!(Rational::from_fps(f64::NAN), Rational::new(30, 1));
    }

    #[test]
    fn color_cicp_roundtrips_and_bt709_defaults() {
        let bt709 = ColorDescription::bt709_limited();
        assert!(bt709.is_specified());
        assert_eq!(bt709.primaries.to_cicp_u16(), 1);
        assert_eq!(bt709.transfer.to_cicp_u16(), 1);
        assert_eq!(bt709.matrix.to_cicp_u16(), 1);
        assert_eq!(bt709.range, ColorRange::Limited);

        for p in [
            ColorPrimaries::Bt709,
            ColorPrimaries::Bt601Ntsc,
            ColorPrimaries::Bt601Pal,
            ColorPrimaries::Bt2020,
            ColorPrimaries::Unspecified,
        ] {
            assert_eq!(ColorPrimaries::from_cicp_u16(p.to_cicp_u16()), p);
        }
    }

    #[test]
    fn media_error_formats_clearly() {
        let err = MediaError::Unsupported("vp9 native mux".into());
        assert!(err.to_string().contains("vp9 native mux"));
    }
}
