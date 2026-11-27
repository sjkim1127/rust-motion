//! Native bounded ISO-BMFF (MP4 / MOV / M4A) metadata parser.
//!
//! Reads top-level container headers and the `moov` atom tree while seeking cleanly
//! past `mdat` payloads without reading compressed media frames into memory.

use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use rust_motion_media_core::{
    AudioCodecId, AudioStreamMetadata, ColorDescription, ColorPrimaries, ColorRange,
    ContainerFormat, MatrixCoefficients, MediaError, MediaMetadata, MediaProbe, Rational,
    TransferCharacteristics, VideoCodecId, VideoStreamMetadata,
};

const MAX_BOX_DEPTH: usize = 16;
const MAX_BOXES: usize = 4096;
const MAX_MOOV_BYTES: u64 = 16 * 1024 * 1024;
const MAX_STTS_ENTRIES: usize = 1_000_000;

/// Native ISO-BMFF (MP4 / MOV / M4A) metadata prober implementing [`MediaProbe`].
#[derive(Debug, Clone, Copy, Default)]
pub struct IsobmffProbe;

impl MediaProbe for IsobmffProbe {
    fn probe_bytes(&self, bytes: &[u8]) -> Result<MediaMetadata, MediaError> {
        let mut cursor = Cursor::new(bytes);
        probe_isobmff(&mut cursor)
    }
}

/// Probe container and track metadata from a local MP4/MOV/M4A file path.
pub fn probe_isobmff_path(path: impl AsRef<Path>) -> Result<MediaMetadata, MediaError> {
    let mut file = File::open(path)?;
    probe_isobmff(&mut file)
}

/// Probe container and track metadata from any seekable ISO-BMFF byte stream.
pub fn probe_isobmff<R: Read + Seek>(reader: &mut R) -> Result<MediaMetadata, MediaError> {
    let (format, moov) = read_moov_bytes(reader)?;
    parse_moov(&moov, format.or(Some(ContainerFormat::Mp4)))
}

pub(crate) fn read_moov_bytes<R: Read + Seek>(
    reader: &mut R,
) -> Result<(Option<ContainerFormat>, Vec<u8>), MediaError> {
    let end = reader.seek(SeekFrom::End(0))?;
    if end < 8 {
        return Err(MediaError::Container(
            "stream too short to be ISO-BMFF".into(),
        ));
    }
    reader.seek(SeekFrom::Start(0))?;

    let mut pos = 0u64;
    let mut top_boxes = 0usize;
    let mut saw_ftyp = false;
    let mut format: Option<ContainerFormat> = None;
    let mut moov_bytes: Option<Vec<u8>> = None;

    while pos < end {
        top_boxes += 1;
        if top_boxes > MAX_BOXES {
            return Err(MediaError::Container(
                "top-level box count exceeds safety limit".into(),
            ));
        }
        if end - pos < 8 {
            return Err(MediaError::Container(
                "truncated ISO-BMFF box header".into(),
            ));
        }
        reader.seek(SeekFrom::Start(pos))?;
        let mut hdr = [0u8; 8];
        reader.read_exact(&mut hdr)?;
        let size_u32 = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
        let kind: [u8; 4] = [hdr[4], hdr[5], hdr[6], hdr[7]];

        let (header_len, total_size) = match size_u32 {
            0 => (8u64, end - pos),
            1 => {
                if end - pos < 16 {
                    return Err(MediaError::Container(
                        "truncated 64-bit ISO-BMFF box header".into(),
                    ));
                }
                let mut ext = [0u8; 8];
                reader.read_exact(&mut ext)?;
                (16u64, u64::from_be_bytes(ext))
            }
            n => (8u64, u64::from(n)),
        };

        if total_size < header_len {
            return Err(MediaError::Container(
                "invalid ISO-BMFF box size smaller than header".into(),
            ));
        }
        let next_pos = pos
            .checked_add(total_size)
            .ok_or_else(|| MediaError::Container("ISO-BMFF box offset overflow".into()))?;
        if next_pos > end {
            return Err(MediaError::Container(
                "ISO-BMFF box extends past end of stream".into(),
            ));
        }

        let payload_size = total_size - header_len;
        match &kind {
            b"ftyp" => {
                saw_ftyp = true;
                if payload_size >= 4 {
                    let read_len = payload_size.min(64) as usize;
                    let mut buf = vec![0u8; read_len];
                    reader.read_exact(&mut buf)?;
                    format = Some(if &buf[0..4] == b"qt  " {
                        ContainerFormat::Mov
                    } else {
                        ContainerFormat::Mp4
                    });
                }
            }
            b"moov" => {
                if payload_size > MAX_MOOV_BYTES {
                    return Err(MediaError::Container(format!(
                        "moov atom ({payload_size} bytes) exceeds limit ({MAX_MOOV_BYTES})"
                    )));
                }
                let mut buf = vec![0u8; payload_size as usize];
                reader.read_exact(&mut buf)?;
                moov_bytes = Some(buf);
            }
            _ => {}
        }

        pos = next_pos;
    }

    if !saw_ftyp && moov_bytes.is_none() {
        return Err(MediaError::Container(
            "stream does not contain ISO-BMFF ftyp or moov boxes".into(),
        ));
    }

    let moov = moov_bytes
        .ok_or_else(|| MediaError::Container("ISO-BMFF stream missing moov box".into()))?;

    Ok((format, moov))
}

pub(crate) struct BoxSlice<'a> {
    pub(crate) kind: [u8; 4],
    pub(crate) payload: &'a [u8],
}

pub(crate) fn iter_child_boxes<'a>(
    mut data: &'a [u8],
    depth: usize,
    total_boxes: &mut usize,
) -> Result<Vec<BoxSlice<'a>>, MediaError> {
    if depth > MAX_BOX_DEPTH {
        return Err(MediaError::Container(
            "ISO-BMFF box nesting depth exceeds limit".into(),
        ));
    }
    let mut out = Vec::new();
    while !data.is_empty() {
        *total_boxes += 1;
        if *total_boxes > MAX_BOXES {
            return Err(MediaError::Container(
                "ISO-BMFF total box count exceeds safety limit".into(),
            ));
        }
        if data.len() < 8 {
            return Err(MediaError::Container("truncated nested box header".into()));
        }
        let size_u32 = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let kind: [u8; 4] = [data[4], data[5], data[6], data[7]];
        let (header_len, total_size) = match size_u32 {
            0 => (8usize, data.len()),
            1 => {
                if data.len() < 16 {
                    return Err(MediaError::Container(
                        "truncated nested 64-bit box header".into(),
                    ));
                }
                let large = u64::from_be_bytes(data[8..16].try_into().unwrap());
                let total = usize::try_from(large)
                    .map_err(|_| MediaError::Container("box size overflow".into()))?;
                (16usize, total)
            }
            n => (8usize, n as usize),
        };
        if total_size < header_len || total_size > data.len() {
            return Err(MediaError::Container(
                "invalid nested ISO-BMFF box size".into(),
            ));
        }
        out.push(BoxSlice {
            kind,
            payload: &data[header_len..total_size],
        });
        data = &data[total_size..];
    }
    Ok(out)
}

fn parse_moov(
    moov_payload: &[u8],
    format: Option<ContainerFormat>,
) -> Result<MediaMetadata, MediaError> {
    let mut box_counter = 0usize;
    let children = iter_child_boxes(moov_payload, 1, &mut box_counter)?;

    let mut movie_duration_secs: Option<f64> = None;
    let mut video_streams = Vec::new();
    let mut audio_streams = Vec::new();

    for child in children {
        match &child.kind {
            b"mvhd" => {
                movie_duration_secs = parse_mvhd_duration(child.payload);
            }
            b"trak" => {
                parse_trak(
                    child.payload,
                    2,
                    &mut box_counter,
                    &mut video_streams,
                    &mut audio_streams,
                )?;
            }
            _ => {}
        }
    }

    let duration_secs = movie_duration_secs.or_else(|| {
        video_streams
            .iter()
            .filter_map(|v| v.duration_secs)
            .chain(audio_streams.iter().filter_map(|a| a.duration_secs))
            .fold(None, |acc: Option<f64>, d| {
                Some(acc.map_or(d, |cur| cur.max(d)))
            })
    });

    Ok(MediaMetadata {
        format,
        duration_secs,
        video_streams,
        audio_streams,
    })
}

fn parse_mvhd_duration(payload: &[u8]) -> Option<f64> {
    if payload.len() < 20 {
        return None;
    }
    let version = payload[0];
    let (timescale, duration) = match version {
        0 if payload.len() >= 20 => {
            let ts = u32::from_be_bytes(payload[12..16].try_into().ok()?);
            let dur = u32::from_be_bytes(payload[16..20].try_into().ok()?);
            if dur == u32::MAX {
                return None;
            }
            (ts, u64::from(dur))
        }
        1 if payload.len() >= 32 => {
            let ts = u32::from_be_bytes(payload[20..24].try_into().ok()?);
            let dur = u64::from_be_bytes(payload[24..32].try_into().ok()?);
            if dur == u64::MAX {
                return None;
            }
            (ts, dur)
        }
        _ => return None,
    };
    (timescale > 0).then(|| duration as f64 / f64::from(timescale))
}

#[derive(Default)]
pub(crate) struct TkhdInfo {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rotation_degrees: i32,
}

pub(crate) fn parse_tkhd(payload: &[u8]) -> Option<TkhdInfo> {
    if payload.len() < 84 {
        return None;
    }
    let version = payload[0];
    let matrix_offset = match version {
        0 if payload.len() >= 84 => 40,
        1 if payload.len() >= 96 => 52,
        _ => return None,
    };

    let read_i32 = |off: usize| -> Option<i32> {
        Some(i32::from_be_bytes(payload[off..off + 4].try_into().ok()?))
    };
    let read_u32 = |off: usize| -> Option<u32> {
        Some(u32::from_be_bytes(payload[off..off + 4].try_into().ok()?))
    };

    let a = read_i32(matrix_offset)? as f64;
    let b = read_i32(matrix_offset + 4)? as f64;
    let width_fp = read_u32(matrix_offset + 36)?;
    let height_fp = read_u32(matrix_offset + 40)?;

    let deg = b.atan2(a).to_degrees().round() as i32;
    let rotation_degrees = deg.rem_euclid(360);

    Some(TkhdInfo {
        width: width_fp >> 16,
        height: height_fp >> 16,
        rotation_degrees,
    })
}

pub(crate) fn parse_mdhd(payload: &[u8]) -> Option<(u32, u64)> {
    if payload.len() < 20 {
        return None;
    }
    match payload[0] {
        0 if payload.len() >= 20 => {
            let ts = u32::from_be_bytes(payload[12..16].try_into().ok()?);
            let dur = u32::from_be_bytes(payload[16..20].try_into().ok()?);
            Some((ts, if dur == u32::MAX { 0 } else { u64::from(dur) }))
        }
        1 if payload.len() >= 32 => {
            let ts = u32::from_be_bytes(payload[20..24].try_into().ok()?);
            let dur = u64::from_be_bytes(payload[24..32].try_into().ok()?);
            Some((ts, if dur == u64::MAX { 0 } else { dur }))
        }
        _ => None,
    }
}

fn parse_trak(
    trak_payload: &[u8],
    depth: usize,
    box_counter: &mut usize,
    video_streams: &mut Vec<VideoStreamMetadata>,
    audio_streams: &mut Vec<AudioStreamMetadata>,
) -> Result<(), MediaError> {
    let children = iter_child_boxes(trak_payload, depth, box_counter)?;
    let mut tkhd = TkhdInfo::default();
    let mut mdia_payload: Option<&[u8]> = None;

    for c in children {
        match &c.kind {
            b"tkhd" => {
                if let Some(info) = parse_tkhd(c.payload) {
                    tkhd = info;
                }
            }
            b"mdia" => mdia_payload = Some(c.payload),
            _ => {}
        }
    }

    let Some(mdia) = mdia_payload else {
        return Ok(());
    };

    let mdia_children = iter_child_boxes(mdia, depth + 1, box_counter)?;
    let mut media_timescale = 0u32;
    let mut media_duration = 0u64;
    let mut handler_type = [0u8; 4];
    let mut minf_payload: Option<&[u8]> = None;

    for c in mdia_children {
        match &c.kind {
            b"mdhd" => {
                if let Some((ts, dur)) = parse_mdhd(c.payload) {
                    media_timescale = ts;
                    media_duration = dur;
                }
            }
            b"hdlr" => {
                if c.payload.len() >= 12 {
                    handler_type.copy_from_slice(&c.payload[8..12]);
                }
            }
            b"minf" => minf_payload = Some(c.payload),
            _ => {}
        }
    }

    let Some(minf) = minf_payload else {
        return Ok(());
    };
    let minf_children = iter_child_boxes(minf, depth + 2, box_counter)?;
    let Some(stbl) = minf_children
        .into_iter()
        .find(|c| &c.kind == b"stbl")
        .map(|c| c.payload)
    else {
        return Ok(());
    };

    let stbl_children = iter_child_boxes(stbl, depth + 3, box_counter)?;
    let mut stsd_payload: Option<&[u8]> = None;
    let mut stts_payload: Option<&[u8]> = None;

    for c in stbl_children {
        match &c.kind {
            b"stsd" => stsd_payload = Some(c.payload),
            b"stts" => stts_payload = Some(c.payload),
            _ => {}
        }
    }

    let (total_samples, stts_total_duration) =
        stts_payload.map(parse_stts).transpose()?.unwrap_or((0, 0));

    let effective_duration_ticks = if media_duration > 0 {
        media_duration
    } else {
        stts_total_duration
    };
    let track_duration_secs = (media_timescale > 0 && effective_duration_ticks > 0)
        .then(|| effective_duration_ticks as f64 / f64::from(media_timescale));

    match &handler_type {
        b"vide" => {
            let (codec, stsd_w, stsd_h, color) = stsd_payload
                .map(|p| parse_video_stsd(p, depth + 4, box_counter))
                .transpose()?
                .flatten()
                .unwrap_or((
                    VideoCodecId::Unknown,
                    0,
                    0,
                    ColorDescription::unspecified_limited(),
                ));
            let width = if stsd_w > 0 { stsd_w } else { tkhd.width };
            let height = if stsd_h > 0 { stsd_h } else { tkhd.height };
            let fps = compute_fps_rational(media_timescale, total_samples, stts_total_duration);
            video_streams.push(VideoStreamMetadata {
                codec,
                width,
                height,
                fps,
                duration_secs: track_duration_secs,
                rotation_degrees: tkhd.rotation_degrees,
                color,
            });
        }
        b"soun" => {
            if let Some((codec, channels, sample_rate)) = stsd_payload.and_then(parse_audio_stsd) {
                audio_streams.push(AudioStreamMetadata {
                    codec,
                    sample_rate: if sample_rate > 0 {
                        sample_rate
                    } else {
                        media_timescale
                    },
                    channels,
                    duration_secs: track_duration_secs,
                });
            }
        }
        _ => {}
    }

    Ok(())
}

fn parse_stts(payload: &[u8]) -> Result<(u64, u64), MediaError> {
    if payload.len() < 8 {
        return Ok((0, 0));
    }
    let entry_count = u32::from_be_bytes(payload[4..8].try_into().unwrap()) as usize;
    if entry_count > MAX_STTS_ENTRIES {
        return Err(MediaError::Container(
            "stts entry count exceeds safety limit".into(),
        ));
    }
    let available_entries = (payload.len() - 8) / 8;
    let count_to_read = entry_count.min(available_entries);
    let mut total_samples = 0u64;
    let mut total_delta = 0u64;
    for i in 0..count_to_read {
        let base = 8 + i * 8;
        let sample_count = u64::from(u32::from_be_bytes(
            payload[base..base + 4].try_into().unwrap(),
        ));
        let sample_delta = u64::from(u32::from_be_bytes(
            payload[base + 4..base + 8].try_into().unwrap(),
        ));
        total_samples = total_samples
            .checked_add(sample_count)
            .ok_or_else(|| MediaError::Container("stts sample count overflow".into()))?;
        let chunk_dur = sample_count
            .checked_mul(sample_delta)
            .ok_or_else(|| MediaError::Container("stts sample duration overflow".into()))?;
        total_delta = total_delta
            .checked_add(chunk_dur)
            .ok_or_else(|| MediaError::Container("stts total duration overflow".into()))?;
    }
    Ok((total_samples, total_delta))
}

pub(crate) fn compute_fps_rational(
    timescale: u32,
    total_samples: u64,
    total_delta: u64,
) -> Rational {
    fn gcd_u128(mut a: u128, mut b: u128) -> u128 {
        while b != 0 {
            let r = a % b;
            a = b;
            b = r;
        }
        a.max(1)
    }

    if timescale == 0 || total_samples == 0 || total_delta == 0 {
        return Rational::new(30, 1);
    }
    let num = u128::from(total_samples) * u128::from(timescale);
    let den = u128::from(total_delta);
    let g = gcd_u128(num, den);
    let rn = num / g;
    let rd = den / g;
    if let (Ok(n32), Ok(d32)) = (u32::try_from(rn), u32::try_from(rd)) {
        Rational::new(n32, d32)
    } else {
        Rational::from_fps((total_samples as f64) * f64::from(timescale) / (total_delta as f64))
    }
}

fn parse_video_stsd(
    payload: &[u8],
    depth: usize,
    box_counter: &mut usize,
) -> Result<Option<(VideoCodecId, u32, u32, ColorDescription)>, MediaError> {
    if payload.len() < 8 {
        return Ok(None);
    }
    let entries = iter_child_boxes(&payload[8..], depth, box_counter)?;
    let Some(entry) = entries.first() else {
        return Ok(None);
    };
    if entry.payload.len() < 78 {
        return Ok(None);
    }
    let width = u32::from(u16::from_be_bytes([entry.payload[24], entry.payload[25]]));
    let height = u32::from(u16::from_be_bytes([entry.payload[26], entry.payload[27]]));
    let codec = match &entry.kind {
        b"avc1" | b"avc3" => VideoCodecId::H264,
        b"hvc1" | b"hev1" => VideoCodecId::Hevc,
        b"av01" => VideoCodecId::Av1,
        b"vp09" => VideoCodecId::Vp9,
        b"apch" | b"apcn" | b"apcs" | b"apco" | b"ap4h" | b"ap4x" => VideoCodecId::ProRes,
        b"mp4v" => VideoCodecId::Mpeg4,
        _ => VideoCodecId::Unknown,
    };

    let mut color = ColorDescription::unspecified_limited();
    if let Ok(sub_boxes) = iter_child_boxes(&entry.payload[78..], depth + 1, box_counter) {
        for sub in sub_boxes {
            if &sub.kind == b"colr" && sub.payload.len() >= 11 && &sub.payload[0..4] == b"nclx" {
                let primaries = ColorPrimaries::from_cicp_u16(u16::from_be_bytes([
                    sub.payload[4],
                    sub.payload[5],
                ]));
                let transfer = TransferCharacteristics::from_cicp_u16(u16::from_be_bytes([
                    sub.payload[6],
                    sub.payload[7],
                ]));
                let matrix = MatrixCoefficients::from_cicp_u16(u16::from_be_bytes([
                    sub.payload[8],
                    sub.payload[9],
                ]));
                let range = if (sub.payload[10] & 0x80) != 0 {
                    ColorRange::Full
                } else {
                    ColorRange::Limited
                };
                color = ColorDescription {
                    primaries,
                    transfer,
                    matrix,
                    range,
                };
            }
        }
    }

    Ok(Some((codec, width, height, color)))
}

fn parse_audio_stsd(payload: &[u8]) -> Option<(AudioCodecId, u16, u32)> {
    if payload.len() < 8 {
        return None;
    }
    let mut box_counter = 0usize;
    let entries = iter_child_boxes(&payload[8..], 1, &mut box_counter).ok()?;
    let entry = entries.first()?;
    if entry.payload.len() < 28 {
        return None;
    }
    let channels = u16::from_be_bytes([entry.payload[16], entry.payload[17]]).max(1);
    let sample_rate = u32::from_be_bytes(entry.payload[24..28].try_into().ok()?) >> 16;
    let codec = match &entry.kind {
        b"mp4a" => AudioCodecId::Aac,
        b"Opus" => AudioCodecId::Opus,
        b"fLaC" => AudioCodecId::Flac,
        b".mp3" => AudioCodecId::Mp3,
        b"lpcm" | b"sowt" | b"twos" | b"in24" => AudioCodecId::PcmS16Le,
        b"fl32" => AudioCodecId::PcmF32Le,
        _ => AudioCodecId::Unknown,
    };
    Some((codec, channels, sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Mp4Muxer, Mp4MuxerOptions};
    use rust_motion_media_core::{
        AudioTrackConfig, ContainerMuxer, EncodedPacket, TrackConfig, VideoTrackConfig,
    };
    use std::process::Command;

    #[test]
    fn roundtrip_mux_and_probe_video_and_audio_tracks() {
        let avcc = [1u8, 100, 0, 30, 0xFF, 0xE1];
        let v_track = TrackConfig::Video(VideoTrackConfig {
            width: 320,
            height: 180,
            timescale: 30000,
            fps: Rational::new(30000, 1001),
            codec: VideoCodecId::H264,
            codec_config: avcc.to_vec(),
            color: ColorDescription {
                primaries: ColorPrimaries::Bt709,
                transfer: TransferCharacteristics::Bt709,
                matrix: MatrixCoefficients::Bt709,
                range: ColorRange::Limited,
            },
        });
        let a_track = TrackConfig::Audio(AudioTrackConfig {
            codec: AudioCodecId::Aac,
            sample_rate: 48000,
            channels: 2,
            timescale: 48000,
            codec_config: vec![0x11, 0x90],
        });

        for faststart in [true, false] {
            let mut muxer = Mp4Muxer::new(
                Cursor::new(Vec::new()),
                &[v_track.clone(), a_track.clone()],
                Mp4MuxerOptions {
                    faststart,
                    force_co64: false,
                },
            )
            .unwrap();
            for i in 0..3 {
                muxer
                    .write_packet(&EncodedPacket {
                        stream_index: 0,
                        data: vec![0, 0, 0, 2, 0x65, i as u8],
                        pts: i * 1001,
                        dts: i * 1001,
                        duration: 1001,
                        is_keyframe: i == 0,
                    })
                    .unwrap();
            }
            muxer
                .write_packet(&EncodedPacket {
                    stream_index: 1,
                    data: vec![0xAA; 16],
                    pts: 0,
                    dts: 0,
                    duration: 1024,
                    is_keyframe: true,
                })
                .unwrap();

            let bytes = muxer.into_inner().unwrap().into_inner();
            let meta = IsobmffProbe.probe_bytes(&bytes).unwrap();
            assert_eq!(meta.format, Some(ContainerFormat::Mp4));
            assert_eq!(meta.video_streams.len(), 1);
            assert_eq!(meta.audio_streams.len(), 1);

            let v = &meta.video_streams[0];
            assert_eq!(v.codec, VideoCodecId::H264);
            assert_eq!((v.width, v.height), (320, 180));
            assert_eq!(v.fps, Rational::new(30000, 1001));
            assert_eq!(v.rotation_degrees, 0);
            assert_eq!(v.color, ColorDescription::bt709_limited());
            assert!((v.duration_secs.unwrap() - 0.1001).abs() < 1e-4);

            let a = &meta.audio_streams[0];
            assert_eq!(a.codec, AudioCodecId::Aac);
            assert_eq!(a.sample_rate, 48000);
            assert_eq!(a.channels, 2);
        }
    }

    #[test]
    fn tkhd_rotation_matrix_decodes_all_quadrants() {
        let make_tkhd = |a: i32, b: i32, c: i32, d: i32| {
            let mut buf = vec![0u8; 84];
            buf[0] = 0; // version 0
            buf[40..44].copy_from_slice(&a.to_be_bytes());
            buf[44..48].copy_from_slice(&b.to_be_bytes());
            buf[52..56].copy_from_slice(&c.to_be_bytes());
            buf[56..60].copy_from_slice(&d.to_be_bytes());
            buf[76..80].copy_from_slice(&(1920u32 << 16).to_be_bytes());
            buf[80..84].copy_from_slice(&(1080u32 << 16).to_be_bytes());
            buf
        };
        let one = 0x0001_0000i32;
        assert_eq!(
            parse_tkhd(&make_tkhd(one, 0, 0, one))
                .unwrap()
                .rotation_degrees,
            0
        );
        assert_eq!(
            parse_tkhd(&make_tkhd(0, one, -one, 0))
                .unwrap()
                .rotation_degrees,
            90
        );
        assert_eq!(
            parse_tkhd(&make_tkhd(-one, 0, 0, -one))
                .unwrap()
                .rotation_degrees,
            180
        );
        assert_eq!(
            parse_tkhd(&make_tkhd(0, -one, one, 0))
                .unwrap()
                .rotation_degrees,
            270
        );
    }

    #[test]
    fn rejects_adversarial_and_non_isobmff_inputs_without_panic() {
        assert!(IsobmffProbe.probe_bytes(&[]).is_err());
        assert!(IsobmffProbe.probe_bytes(b"RIFF0000WAVE").is_err());
        // Box size smaller than header (size = 3).
        assert!(IsobmffProbe
            .probe_bytes(&[0, 0, 0, 3, b'f', b't', b'y', b'p'])
            .is_err());
        // Huge 64-bit box size past EOF.
        let mut huge = Vec::new();
        huge.extend_from_slice(&1u32.to_be_bytes());
        huge.extend_from_slice(b"moov");
        huge.extend_from_slice(&u64::MAX.to_be_bytes());
        assert!(IsobmffProbe.probe_bytes(&huge).is_err());
        // Child box inside moov whose declared size exceeds parent moov slice.
        let bad_nested = [
            0, 0, 0, 12, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm', // ftyp
            0, 0, 0, 20, b'm', b'o', b'o', b'v', // moov (payload = 12 bytes)
            0, 0, 0, 64, b't', b'r', b'a', b'k', // trak claims 64 bytes > 12 bytes
            0, 0, 0, 0,
        ];
        assert!(IsobmffProbe.probe_bytes(&bad_nested).is_err());

        // Huge stts entry_count exceeding MAX_STTS_ENTRIES.
        let mut stts_payload = vec![0, 0, 0, 0];
        stts_payload.extend_from_slice(&u32::MAX.to_be_bytes());
        assert!(parse_stts(&stts_payload).is_err());
    }

    #[test]
    fn differential_probe_matches_ffprobe_on_ffmpeg_mp4_with_audio() {
        let ffprobe_ok = Command::new("ffprobe")
            .arg("-version")
            .output()
            .is_ok_and(|o| o.status.success());
        let ffmpeg_ok = Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_ok_and(|o| o.status.success());
        if !ffprobe_ok || !ffmpeg_ok {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let mp4 = dir.path().join("sample_av.mp4");
        let gen = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=128x72:rate=24000/1001:duration=0.5",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=44100:duration=0.5",
                "-c:v",
                "mpeg4",
                "-c:a",
                "aac",
            ])
            .arg(&mp4)
            .status()
            .unwrap();
        assert!(gen.success());

        let probe_json = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type,width,height,channels,sample_rate,avg_frame_rate:format=duration",
                "-of",
                "json",
            ])
            .arg(&mp4)
            .output()
            .unwrap();
        assert!(probe_json.status.success());
        let val: serde_json::Value = serde_json::from_slice(&probe_json.stdout).unwrap();
        let streams = val["streams"].as_array().unwrap();
        let ff_v = streams.iter().find(|s| s["codec_type"] == "video").unwrap();
        let ff_a = streams.iter().find(|s| s["codec_type"] == "audio").unwrap();

        let native = probe_isobmff_path(&mp4).unwrap();
        let nv = &native.video_streams[0];
        let na = &native.audio_streams[0];

        assert_eq!(nv.width as u64, ff_v["width"].as_u64().unwrap());
        assert_eq!(nv.height as u64, ff_v["height"].as_u64().unwrap());
        assert_eq!(nv.codec, VideoCodecId::Mpeg4);
        assert_eq!(na.channels as u64, ff_a["channels"].as_u64().unwrap());
        assert_eq!(
            na.sample_rate.to_string(),
            ff_a["sample_rate"].as_str().unwrap()
        );
        assert!((nv.fps.as_f64() - (24000.0 / 1001.0)).abs() < 0.05);

        let ff_dur: f64 = val["format"]["duration"].as_str().unwrap().parse().unwrap();
        assert!((native.duration_secs.unwrap() - ff_dur).abs() < 0.05);
    }
}
