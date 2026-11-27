//! Minimal ISO-BMFF (MP4) muxer for a single H.264 video track.
//!
//! Delegates to [`rust_motion_media_container::Mp4Muxer`] while preserving the
//! existing `renderer::mp4` API surface for 1.x compatibility.

use std::io::Cursor;

use rust_motion_media_container::{Mp4Muxer, Mp4MuxerOptions};
use rust_motion_media_core::{
    ColorDescription, ContainerMuxer, EncodedPacket, Rational, TrackConfig, VideoCodecId,
    VideoTrackConfig,
};

/// Parameters describing the video track.
pub struct Mp4Params<'a> {
    pub width: u32,
    pub height: u32,
    /// Frame rate as `fps_num / fps_den`; it is also used as the media timescale.
    pub fps_num: u32,
    pub fps_den: u32,
    /// `AVCDecoderConfigurationRecord` payload (the content of the `avcC` box).
    pub avcc: &'a [u8],
    /// Place `moov` before `mdat` so playback can start before the file is fully downloaded.
    pub faststart: bool,
}

/// One coded access unit in length-prefixed form.
pub struct Sample {
    pub data: Vec<u8>,
    pub keyframe: bool,
}

/// Build a complete MP4 file in memory.
pub fn mux(p: &Mp4Params, samples: &[Sample]) -> Result<Vec<u8>, String> {
    if samples.is_empty() {
        return Err("no samples to mux".into());
    }
    if p.fps_num == 0 || p.fps_den == 0 {
        return Err("invalid frame rate".into());
    }

    let track = TrackConfig::Video(VideoTrackConfig {
        width: p.width,
        height: p.height,
        timescale: p.fps_num,
        fps: Rational::new(p.fps_num, p.fps_den),
        codec: VideoCodecId::H264,
        codec_config: p.avcc.to_vec(),
        color: ColorDescription::bt709_limited(),
    });
    let mut muxer = Mp4Muxer::new(
        Cursor::new(Vec::new()),
        &[track],
        Mp4MuxerOptions {
            faststart: p.faststart,
            force_co64: false,
        },
    )
    .map_err(|e| e.to_string())?;

    let den = i64::from(p.fps_den);
    for (idx, s) in samples.iter().enumerate() {
        let ts = idx as i64 * den;
        muxer
            .write_packet(&EncodedPacket {
                stream_index: 0,
                data: s.data.clone(),
                pts: ts,
                dts: ts,
                duration: den,
                is_keyframe: s.keyframe,
            })
            .map_err(|e| e.to_string())?;
    }

    muxer
        .into_inner()
        .map(Cursor::into_inner)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(buf: &[u8], kind: &[u8; 4]) -> Option<usize> {
        buf.windows(4).position(|w| w == kind)
    }

    fn params(faststart: bool, avcc: &[u8]) -> Mp4Params<'_> {
        Mp4Params {
            width: 64,
            height: 48,
            fps_num: 30000,
            fps_den: 1001,
            avcc,
            faststart,
        }
    }

    fn samples() -> Vec<Sample> {
        (0..3)
            .map(|i| Sample {
                data: vec![0, 0, 0, 2, 0x65, i as u8],
                keyframe: i == 0,
            })
            .collect()
    }

    #[test]
    fn starts_with_ftyp_and_contains_required_boxes() {
        let avcc = [1u8, 100, 0, 30, 0xFF, 0xE1];
        for faststart in [true, false] {
            let file = mux(&params(faststart, &avcc), &samples()).unwrap();
            assert_eq!(&file[4..8], b"ftyp");
            for kind in [
                b"moov", b"mdat", b"avcC", b"stts", b"stss", b"stsz", b"stco", b"avc1",
            ] {
                assert!(find(&file, kind).is_some(), "missing {kind:?}");
            }
            let moov = find(&file, b"moov").unwrap();
            let mdat = find(&file, b"mdat").unwrap();
            assert_eq!(moov < mdat, faststart);
        }
    }

    #[test]
    fn chunk_offset_points_at_first_sample() {
        let avcc = [1u8, 100, 0, 30, 0xFF, 0xE1];
        for faststart in [true, false] {
            let file = mux(&params(faststart, &avcc), &samples()).unwrap();
            let stco = find(&file, b"stco").unwrap();
            // stco: kind, version/flags, entry_count, offset
            let off = u32::from_be_bytes(file[stco + 12..stco + 16].try_into().unwrap()) as usize;
            assert_eq!(&file[off..off + 6], &[0, 0, 0, 2, 0x65, 0]);
        }
    }

    #[test]
    fn rejects_empty_sample_list() {
        assert!(mux(&params(true, &[1]), &[]).is_err());
    }
}
