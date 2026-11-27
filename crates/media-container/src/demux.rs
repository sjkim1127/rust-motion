//! Bounded ISO-BMFF (MP4 / MOV) sample table demuxer for native codecs.
//!
//! Reads the `moov` sample tables (`stsd`, `stts`, `ctts`, `stsc`, `stsz`, `stco`/`co64`,
//! `stss`) once on open and provides random-access keyframe seeking plus bounded sample
//! reads into caller-owned buffers.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use rust_motion_media_core::{
    ColorDescription, ColorPrimaries, ColorRange, MatrixCoefficients, MediaError, Rational,
    TransferCharacteristics, VideoCodecId,
};

use crate::probe::{
    compute_fps_rational, iter_child_boxes, parse_mdhd, parse_tkhd, read_moov_bytes, TkhdInfo,
};

const MAX_SAMPLES: usize = 2_000_000;
const MAX_SAMPLE_BYTES: usize = 32 * 1024 * 1024;
const MAX_TABLE_ENTRIES: usize = 2_000_000;

/// Location and timing metadata for one demuxed elementary sample in decode order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleEntry {
    pub index: usize,
    pub file_offset: u64,
    pub size: u32,
    pub dts: i64,
    pub pts: i64,
    pub duration: u32,
    pub is_keyframe: bool,
}

/// Demuxed video track descriptor including decoder configuration record and sample index.
#[derive(Debug, Clone)]
pub struct DemuxedVideoTrack {
    pub track_index: usize,
    pub codec: VideoCodecId,
    pub width: u32,
    pub height: u32,
    pub timescale: u32,
    pub fps: Rational,
    pub duration_ticks: u64,
    pub rotation_degrees: i32,
    pub color: ColorDescription,
    /// Codec configuration record (`avcC` for H.264, `hvcC` for HEVC).
    pub codec_config: Vec<u8>,
    pub samples: Vec<SampleEntry>,
}

impl DemuxedVideoTrack {
    /// Locate the sample index (in decode order) whose presentation time best matches `time_secs`
    /// using latest-sample `pts <= target_tick` (clamped to the earliest presented sample if before start).
    pub fn sample_index_for_time(&self, time_secs: f64) -> usize {
        if self.samples.is_empty() {
            return 0;
        }
        let target_tick = if time_secs.is_finite() && time_secs > 0.0 && self.timescale > 0 {
            let scaled = time_secs * f64::from(self.timescale);
            if scaled >= i64::MAX as f64 {
                i64::MAX
            } else {
                scaled.floor() as i64
            }
        } else {
            0
        };

        if let Some(best) = self
            .samples
            .iter()
            .filter(|s| s.pts <= target_tick)
            .max_by_key(|s| (s.pts, s.index))
        {
            return best.index;
        }

        self.samples
            .iter()
            .min_by_key(|s| (s.pts, s.index))
            .map(|s| s.index)
            .unwrap_or(0)
    }

    /// Return the index of the nearest sync/keyframe sample at or before `sample_idx`.
    pub fn keyframe_index_before_or_at(&self, sample_idx: usize) -> usize {
        if self.samples.is_empty() {
            return 0;
        }
        let start = sample_idx.min(self.samples.len() - 1);
        for idx in (0..=start).rev() {
            if self.samples[idx].is_keyframe {
                return idx;
            }
        }
        0
    }
}

/// Random-access ISO-BMFF sample demuxer over any seekable byte stream.
pub struct Mp4Demuxer<R> {
    reader: R,
    video_tracks: Vec<DemuxedVideoTrack>,
}

impl Mp4Demuxer<File> {
    /// Open an MP4/MOV file from disk and parse its `moov` sample tables.
    pub fn open_path(path: impl AsRef<Path>) -> Result<Self, MediaError> {
        let file = File::open(path)?;
        Self::open(file)
    }
}

impl<R: Read + Seek> Mp4Demuxer<R> {
    /// Parse ISO-BMFF `moov` sample tables from `reader`.
    pub fn open(mut reader: R) -> Result<Self, MediaError> {
        let (_format, moov) = read_moov_bytes(&mut reader)?;
        let video_tracks = parse_demux_moov(&moov)?;
        Ok(Self {
            reader,
            video_tracks,
        })
    }

    /// Video tracks discovered inside the container.
    pub fn video_tracks(&self) -> &[DemuxedVideoTrack] {
        &self.video_tracks
    }

    /// Read exact bytes of sample `(track_idx, sample_idx)` into `buf`, reusing capacity.
    pub fn read_sample(
        &mut self,
        track_idx: usize,
        sample_idx: usize,
        buf: &mut Vec<u8>,
    ) -> Result<SampleEntry, MediaError> {
        let sample = self
            .video_tracks
            .get(track_idx)
            .and_then(|t| t.samples.get(sample_idx))
            .ok_or_else(|| {
                MediaError::Container(format!("sample ({track_idx}, {sample_idx}) out of bounds"))
            })?
            .clone();
        let len = sample.size as usize;
        if len > MAX_SAMPLE_BYTES {
            return Err(MediaError::Container(format!(
                "sample ({track_idx}, {sample_idx}) size {len} exceeds limit ({MAX_SAMPLE_BYTES})"
            )));
        }
        buf.resize(len, 0);
        self.reader.seek(SeekFrom::Start(sample.file_offset))?;
        self.reader.read_exact(buf)?;
        Ok(sample)
    }
}

fn parse_demux_moov(moov_payload: &[u8]) -> Result<Vec<DemuxedVideoTrack>, MediaError> {
    let mut box_counter = 0usize;
    let children = iter_child_boxes(moov_payload, 1, &mut box_counter)?;
    let mut tracks = Vec::new();
    for c in children {
        if &c.kind == b"trak" {
            let track_index = tracks.len();
            if let Some(track) = parse_demux_trak(c.payload, track_index, 2, &mut box_counter)? {
                tracks.push(track);
            }
        }
    }
    Ok(tracks)
}

fn parse_demux_trak(
    trak_payload: &[u8],
    track_index: usize,
    depth: usize,
    box_counter: &mut usize,
) -> Result<Option<DemuxedVideoTrack>, MediaError> {
    let children = iter_child_boxes(trak_payload, depth, box_counter)?;
    let mut tkhd = TkhdInfo::default();
    let mut edts_payload = None;
    let mut mdia_payload = None;
    for c in children {
        match &c.kind {
            b"tkhd" => {
                if let Some(info) = parse_tkhd(c.payload) {
                    tkhd = info;
                }
            }
            b"edts" => edts_payload = Some(c.payload),
            b"mdia" => mdia_payload = Some(c.payload),
            _ => {}
        }
    }
    let edit_media_time = edts_payload
        .map(|p| parse_edts_media_time(p, depth + 1, box_counter))
        .transpose()?
        .unwrap_or(0);
    let Some(mdia) = mdia_payload else {
        return Ok(None);
    };

    let mdia_children = iter_child_boxes(mdia, depth + 1, box_counter)?;
    let mut timescale = 0u32;
    let mut media_duration = 0u64;
    let mut is_video = false;
    let mut minf_payload = None;
    for c in mdia_children {
        match &c.kind {
            b"mdhd" => {
                if let Some((ts, dur)) = parse_mdhd(c.payload) {
                    timescale = ts;
                    media_duration = dur;
                }
            }
            b"hdlr" => {
                if c.payload.len() >= 12 && &c.payload[8..12] == b"vide" {
                    is_video = true;
                }
            }
            b"minf" => minf_payload = Some(c.payload),
            _ => {}
        }
    }
    if !is_video {
        return Ok(None);
    }
    let Some(minf) = minf_payload else {
        return Ok(None);
    };
    let minf_children = iter_child_boxes(minf, depth + 2, box_counter)?;
    let Some(stbl) = minf_children
        .into_iter()
        .find(|c| &c.kind == b"stbl")
        .map(|c| c.payload)
    else {
        return Ok(None);
    };

    let stbl_children = iter_child_boxes(stbl, depth + 3, box_counter)?;
    let mut stsd = None;
    let mut stts = None;
    let mut ctts = None;
    let mut stsc = None;
    let mut stsz = None;
    let mut stco = None;
    let mut co64 = None;
    let mut stss = None;

    for c in stbl_children {
        match &c.kind {
            b"stsd" => stsd = Some(c.payload),
            b"stts" => stts = Some(c.payload),
            b"ctts" => ctts = Some(c.payload),
            b"stsc" => stsc = Some(c.payload),
            b"stsz" => stsz = Some(c.payload),
            b"stco" => stco = Some(c.payload),
            b"co64" => co64 = Some(c.payload),
            b"stss" => stss = Some(c.payload),
            _ => {}
        }
    }

    let Some(stsd_payload) = stsd else {
        return Ok(None);
    };
    let Some((codec, stsd_w, stsd_h, color, codec_config)) =
        parse_demux_video_stsd(stsd_payload, depth + 4, box_counter)?
    else {
        return Ok(None);
    };

    let sizes = stsz.map(parse_stsz).transpose()?.unwrap_or_default();
    let sample_count = sizes.len();
    let chunk_offsets = if let Some(p) = co64 {
        parse_co64(p)?
    } else if let Some(p) = stco {
        parse_stco(p)?
    } else {
        Vec::new()
    };

    let sample_offsets = if sample_count > 0 {
        let Some(stsc_payload) = stsc else {
            return Err(MediaError::Container(
                "video track has samples but no stsc table".into(),
            ));
        };
        expand_chunk_offsets(stsc_payload, &chunk_offsets, &sizes)?
    } else {
        Vec::new()
    };

    let (durations, dts_list, stts_total_duration) = if let Some(p) = stts {
        expand_stts(p, sample_count)?
    } else {
        (vec![0; sample_count], vec![0; sample_count], 0)
    };

    let cts_offsets = if let Some(p) = ctts {
        expand_ctts(p, sample_count)?
    } else {
        vec![0i64; sample_count]
    };

    let mut keyframes = vec![stss.is_none(); sample_count];
    if let Some(p) = stss {
        mark_stss_keyframes(p, &mut keyframes)?;
    }

    let mut samples = Vec::with_capacity(sample_count);
    for i in 0..sample_count {
        let dts = dts_list[i];
        let pts = dts
            .saturating_add(cts_offsets[i])
            .saturating_sub(edit_media_time);
        samples.push(SampleEntry {
            index: i,
            file_offset: sample_offsets[i],
            size: sizes[i],
            dts,
            pts,
            duration: durations[i],
            is_keyframe: keyframes[i],
        });
    }

    let duration_ticks = if media_duration > 0 {
        media_duration
    } else {
        stts_total_duration
    };
    let fps = compute_fps_rational(timescale, sample_count as u64, stts_total_duration);
    let width = if stsd_w > 0 { stsd_w } else { tkhd.width };
    let height = if stsd_h > 0 { stsd_h } else { tkhd.height };

    Ok(Some(DemuxedVideoTrack {
        track_index,
        codec,
        width,
        height,
        timescale,
        fps,
        duration_ticks,
        rotation_degrees: tkhd.rotation_degrees,
        color,
        codec_config,
        samples,
    }))
}

fn parse_demux_video_stsd(
    payload: &[u8],
    depth: usize,
    box_counter: &mut usize,
) -> Result<Option<(VideoCodecId, u32, u32, ColorDescription, Vec<u8>)>, MediaError> {
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
    let mut codec_config = Vec::new();
    let sub_boxes = iter_child_boxes(&entry.payload[78..], depth + 1, box_counter)?;
    for sub in sub_boxes {
        match &sub.kind {
            b"avcC" | b"hvcC" => codec_config = sub.payload.to_vec(),
            b"colr" if sub.payload.len() >= 11 && &sub.payload[0..4] == b"nclx" => {
                color = ColorDescription {
                    primaries: ColorPrimaries::from_cicp_u16(u16::from_be_bytes([
                        sub.payload[4],
                        sub.payload[5],
                    ])),
                    transfer: TransferCharacteristics::from_cicp_u16(u16::from_be_bytes([
                        sub.payload[6],
                        sub.payload[7],
                    ])),
                    matrix: MatrixCoefficients::from_cicp_u16(u16::from_be_bytes([
                        sub.payload[8],
                        sub.payload[9],
                    ])),
                    range: if (sub.payload[10] & 0x80) != 0 {
                        ColorRange::Full
                    } else {
                        ColorRange::Limited
                    },
                };
            }
            _ => {}
        }
    }

    Ok(Some((codec, width, height, color, codec_config)))
}

fn parse_stsz(payload: &[u8]) -> Result<Vec<u32>, MediaError> {
    if payload.len() < 12 {
        return Err(MediaError::Container("stsz box truncated".into()));
    }
    let uniform = u32::from_be_bytes(payload[4..8].try_into().unwrap());
    let count = u32::from_be_bytes(payload[8..12].try_into().unwrap()) as usize;
    if count > MAX_SAMPLES {
        return Err(MediaError::Container(format!(
            "stsz sample count {count} exceeds limit ({MAX_SAMPLES})"
        )));
    }
    if uniform > 0 {
        if uniform as usize > MAX_SAMPLE_BYTES {
            return Err(MediaError::Container(
                "uniform stsz sample size exceeds limit".into(),
            ));
        }
        return Ok(vec![uniform; count]);
    }
    let required = 12usize
        .checked_add(count * 4)
        .ok_or_else(|| MediaError::Container("stsz byte length overflow".into()))?;
    if payload.len() < required {
        return Err(MediaError::Container("stsz size table truncated".into()));
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = 12 + i * 4;
        let sz = u32::from_be_bytes(payload[base..base + 4].try_into().unwrap());
        if sz as usize > MAX_SAMPLE_BYTES {
            return Err(MediaError::Container(format!(
                "stsz sample {i} size {sz} exceeds limit ({MAX_SAMPLE_BYTES})"
            )));
        }
        out.push(sz);
    }
    Ok(out)
}

fn parse_stco(payload: &[u8]) -> Result<Vec<u64>, MediaError> {
    if payload.len() < 8 {
        return Err(MediaError::Container("stco box truncated".into()));
    }
    let count = u32::from_be_bytes(payload[4..8].try_into().unwrap()) as usize;
    if count > MAX_TABLE_ENTRIES {
        return Err(MediaError::Container(
            "stco chunk count exceeds limit".into(),
        ));
    }
    let required = 8usize
        .checked_add(count * 4)
        .ok_or_else(|| MediaError::Container("stco byte length overflow".into()))?;
    if payload.len() < required {
        return Err(MediaError::Container("stco table truncated".into()));
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = 8 + i * 4;
        out.push(u64::from(u32::from_be_bytes(
            payload[base..base + 4].try_into().unwrap(),
        )));
    }
    Ok(out)
}

fn parse_co64(payload: &[u8]) -> Result<Vec<u64>, MediaError> {
    if payload.len() < 8 {
        return Err(MediaError::Container("co64 box truncated".into()));
    }
    let count = u32::from_be_bytes(payload[4..8].try_into().unwrap()) as usize;
    if count > MAX_TABLE_ENTRIES {
        return Err(MediaError::Container(
            "co64 chunk count exceeds limit".into(),
        ));
    }
    let required = 8usize
        .checked_add(count * 8)
        .ok_or_else(|| MediaError::Container("co64 byte length overflow".into()))?;
    if payload.len() < required {
        return Err(MediaError::Container("co64 table truncated".into()));
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = 8 + i * 8;
        out.push(u64::from_be_bytes(
            payload[base..base + 8].try_into().unwrap(),
        ));
    }
    Ok(out)
}

fn expand_chunk_offsets(
    stsc_payload: &[u8],
    chunk_offsets: &[u64],
    sizes: &[u32],
) -> Result<Vec<u64>, MediaError> {
    if stsc_payload.len() < 8 {
        return Err(MediaError::Container("stsc box truncated".into()));
    }
    let count = u32::from_be_bytes(stsc_payload[4..8].try_into().unwrap()) as usize;
    if count == 0 || chunk_offsets.is_empty() {
        return Err(MediaError::Container(
            "stsc or chunk offset table empty for non-empty track".into(),
        ));
    }
    if count > MAX_TABLE_ENTRIES {
        return Err(MediaError::Container(
            "stsc entry count exceeds limit".into(),
        ));
    }
    let required = 8usize
        .checked_add(count * 12)
        .ok_or_else(|| MediaError::Container("stsc byte length overflow".into()))?;
    if stsc_payload.len() < required {
        return Err(MediaError::Container("stsc table truncated".into()));
    }

    let mut runs = Vec::with_capacity(count);
    for i in 0..count {
        let base = 8 + i * 12;
        let first_chunk = u32::from_be_bytes(stsc_payload[base..base + 4].try_into().unwrap());
        let samples_per_chunk =
            u32::from_be_bytes(stsc_payload[base + 4..base + 8].try_into().unwrap());
        if first_chunk == 0 {
            return Err(MediaError::Container(
                "stsc first_chunk must be >= 1".into(),
            ));
        }
        runs.push((first_chunk, samples_per_chunk));
    }

    let mut sample_offsets = Vec::with_capacity(sizes.len());
    let mut run_idx = 0usize;
    let mut sample_idx = 0usize;

    for (chunk_zero, &chunk_base) in chunk_offsets.iter().enumerate() {
        let chunk_one = (chunk_zero + 1) as u32;
        while run_idx + 1 < runs.len() && runs[run_idx + 1].0 <= chunk_one {
            run_idx += 1;
        }
        let samples_in_chunk = runs[run_idx].1 as usize;
        let mut cursor = chunk_base;
        for _ in 0..samples_in_chunk {
            let Some(&sz) = sizes.get(sample_idx) else {
                return Err(MediaError::Container(
                    "stsc expands past stsz sample count".into(),
                ));
            };
            sample_offsets.push(cursor);
            cursor = cursor
                .checked_add(u64::from(sz))
                .ok_or_else(|| MediaError::Container("sample file offset overflow".into()))?;
            sample_idx += 1;
        }
    }

    if sample_offsets.len() != sizes.len() {
        return Err(MediaError::Container(format!(
            "stsc sample count ({}) != stsz sample count ({})",
            sample_offsets.len(),
            sizes.len()
        )));
    }
    Ok(sample_offsets)
}

fn expand_stts(
    payload: &[u8],
    sample_count: usize,
) -> Result<(Vec<u32>, Vec<i64>, u64), MediaError> {
    if payload.len() < 8 {
        return Err(MediaError::Container("stts box truncated".into()));
    }
    let entry_count = u32::from_be_bytes(payload[4..8].try_into().unwrap()) as usize;
    if entry_count > MAX_TABLE_ENTRIES {
        return Err(MediaError::Container(
            "stts entry count exceeds limit".into(),
        ));
    }
    let required = 8usize
        .checked_add(entry_count * 8)
        .ok_or_else(|| MediaError::Container("stts byte length overflow".into()))?;
    if payload.len() < required {
        return Err(MediaError::Container("stts table truncated".into()));
    }

    let mut durations = Vec::with_capacity(sample_count);
    let mut dts_list = Vec::with_capacity(sample_count);
    let mut cur_dts = 0i64;
    let mut total_dur = 0u64;

    for i in 0..entry_count {
        let base = 8 + i * 8;
        let cnt = u32::from_be_bytes(payload[base..base + 4].try_into().unwrap()) as usize;
        let delta = u32::from_be_bytes(payload[base + 4..base + 8].try_into().unwrap());
        if durations.len().saturating_add(cnt) > MAX_SAMPLES {
            return Err(MediaError::Container(
                "stts sample sum exceeds limit".into(),
            ));
        }
        for _ in 0..cnt {
            if durations.len() >= sample_count {
                break;
            }
            durations.push(delta);
            dts_list.push(cur_dts);
            cur_dts = cur_dts
                .checked_add(i64::from(delta))
                .ok_or_else(|| MediaError::Container("dts overflow".into()))?;
            total_dur = total_dur
                .checked_add(u64::from(delta))
                .ok_or_else(|| MediaError::Container("total duration overflow".into()))?;
        }
    }

    while durations.len() < sample_count {
        durations.push(0);
        dts_list.push(cur_dts);
    }

    Ok((durations, dts_list, total_dur))
}

fn expand_ctts(payload: &[u8], sample_count: usize) -> Result<Vec<i64>, MediaError> {
    if payload.len() < 8 {
        return Err(MediaError::Container("ctts box truncated".into()));
    }
    let version = payload[0];
    let entry_count = u32::from_be_bytes(payload[4..8].try_into().unwrap()) as usize;
    if entry_count > MAX_TABLE_ENTRIES {
        return Err(MediaError::Container(
            "ctts entry count exceeds limit".into(),
        ));
    }
    let required = 8usize
        .checked_add(entry_count * 8)
        .ok_or_else(|| MediaError::Container("ctts byte length overflow".into()))?;
    if payload.len() < required {
        return Err(MediaError::Container("ctts table truncated".into()));
    }

    let mut out = Vec::with_capacity(sample_count);
    for i in 0..entry_count {
        let base = 8 + i * 8;
        let cnt = u32::from_be_bytes(payload[base..base + 4].try_into().unwrap()) as usize;
        let off = if version == 0 {
            i64::from(u32::from_be_bytes(
                payload[base + 4..base + 8].try_into().unwrap(),
            ))
        } else {
            i64::from(i32::from_be_bytes(
                payload[base + 4..base + 8].try_into().unwrap(),
            ))
        };
        if out.len().saturating_add(cnt) > MAX_SAMPLES {
            return Err(MediaError::Container(
                "ctts sample sum exceeds limit".into(),
            ));
        }
        for _ in 0..cnt {
            if out.len() >= sample_count {
                break;
            }
            out.push(off);
        }
    }
    while out.len() < sample_count {
        out.push(0);
    }
    Ok(out)
}

fn parse_edts_media_time(
    edts_payload: &[u8],
    depth: usize,
    box_counter: &mut usize,
) -> Result<i64, MediaError> {
    let children = iter_child_boxes(edts_payload, depth, box_counter)?;
    let Some(elst) = children.into_iter().find(|c| &c.kind == b"elst") else {
        return Ok(0);
    };
    let p = elst.payload;
    if p.len() < 8 {
        return Ok(0);
    }
    let version = p[0];
    let entry_count = u32::from_be_bytes(p[4..8].try_into().unwrap()) as usize;
    let entry_size = if version == 1 { 20 } else { 12 };
    let max_entries = (p.len() - 8) / entry_size;
    for i in 0..entry_count.min(max_entries) {
        let base = 8 + i * entry_size;
        let media_time = if version == 1 {
            i64::from_be_bytes(p[base + 8..base + 16].try_into().unwrap())
        } else {
            i64::from(i32::from_be_bytes(
                p[base + 4..base + 8].try_into().unwrap(),
            ))
        };
        if media_time >= 0 {
            return Ok(media_time);
        }
    }
    Ok(0)
}

fn mark_stss_keyframes(payload: &[u8], keyframes: &mut [bool]) -> Result<(), MediaError> {
    if payload.len() < 8 {
        return Err(MediaError::Container("stss box truncated".into()));
    }
    let count = u32::from_be_bytes(payload[4..8].try_into().unwrap()) as usize;
    if count > MAX_TABLE_ENTRIES {
        return Err(MediaError::Container(
            "stss entry count exceeds limit".into(),
        ));
    }
    let required = 8usize
        .checked_add(count * 4)
        .ok_or_else(|| MediaError::Container("stss byte length overflow".into()))?;
    if payload.len() < required {
        return Err(MediaError::Container("stss table truncated".into()));
    }
    for i in 0..count {
        let base = 8 + i * 4;
        let sample_1based =
            u32::from_be_bytes(payload[base..base + 4].try_into().unwrap()) as usize;
        if let Some(idx) = sample_1based.checked_sub(1) {
            if let Some(slot) = keyframes.get_mut(idx) {
                *slot = true;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Mp4Muxer, Mp4MuxerOptions};
    use rust_motion_media_core::{ContainerMuxer, EncodedPacket, TrackConfig, VideoTrackConfig};
    use std::io::Cursor;
    use std::process::Command;

    #[test]
    fn roundtrip_mux_demux_preserves_bframe_pts_dts_co64_and_sample_bytes() {
        let avcc = [1u8, 100, 0, 30, 0xFF, 0xE1];
        let v_track = TrackConfig::Video(VideoTrackConfig {
            width: 128,
            height: 72,
            timescale: 24,
            fps: Rational::new(24, 1),
            codec: VideoCodecId::H264,
            codec_config: avcc.to_vec(),
            color: ColorDescription::bt709_limited(),
        });

        for faststart in [true, false] {
            let mut muxer = Mp4Muxer::new(
                Cursor::new(Vec::new()),
                std::slice::from_ref(&v_track),
                Mp4MuxerOptions {
                    faststart,
                    force_co64: true,
                },
            )
            .unwrap();
            let input_pkts = vec![
                EncodedPacket {
                    stream_index: 0,
                    data: vec![0, 0, 0, 3, 0x65, 1, 10],
                    pts: 2,
                    dts: 0,
                    duration: 1,
                    is_keyframe: true,
                },
                EncodedPacket {
                    stream_index: 0,
                    data: vec![0, 0, 0, 2, 0x41, 20],
                    pts: 1,
                    dts: 1,
                    duration: 1,
                    is_keyframe: false,
                },
                EncodedPacket {
                    stream_index: 0,
                    data: vec![0, 0, 0, 3, 0x41, 30, 31],
                    pts: 3,
                    dts: 2,
                    duration: 1,
                    is_keyframe: false,
                },
                EncodedPacket {
                    stream_index: 0,
                    data: vec![0, 0, 0, 2, 0x65, 40],
                    pts: 4,
                    dts: 3,
                    duration: 1,
                    is_keyframe: true,
                },
            ];
            for p in &input_pkts {
                muxer.write_packet(p).unwrap();
            }
            let mp4_bytes = muxer.into_inner().unwrap().into_inner();

            let mut demuxer = Mp4Demuxer::open(Cursor::new(mp4_bytes)).unwrap();
            let track = demuxer.video_tracks()[0].clone();
            assert_eq!(track.codec, VideoCodecId::H264);
            assert_eq!((track.width, track.height), (128, 72));
            assert_eq!(track.codec_config, avcc);
            assert_eq!(track.samples.len(), 4);
            assert_eq!(track.samples[0].pts, 2);
            assert_eq!(track.samples[1].pts, 1);
            assert!(track.samples[0].is_keyframe);
            assert!(!track.samples[1].is_keyframe);
            assert!(track.samples[3].is_keyframe);

            // Presentation time 1/24s maps to decode index 1 (pts=1); nearest keyframe is index 0.
            let sidx = track.sample_index_for_time(1.0 / 24.0);
            assert_eq!(sidx, 1);
            assert_eq!(track.keyframe_index_before_or_at(sidx), 0);
            assert_eq!(track.keyframe_index_before_or_at(3), 3);

            let mut buf = Vec::new();
            for (i, expected) in input_pkts.iter().enumerate() {
                let s = demuxer.read_sample(0, i, &mut buf).unwrap();
                assert_eq!(s.size as usize, expected.data.len());
                assert_eq!(buf, expected.data);
            }
        }
    }

    #[test]
    fn demuxer_rejects_malformed_tables_safely() {
        assert!(parse_stsz(&[0; 4]).is_err());
        let mut oversized_stsz = vec![0u8; 12];
        oversized_stsz[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(parse_stsz(&oversized_stsz).is_err());
        assert!(expand_chunk_offsets(&[0; 4], &[10], &[5]).is_err());
    }

    #[test]
    fn demuxes_and_decodes_real_hevc_mp4_via_pure_rust_decoder() {
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
            return;
        };

        let dir = tempfile::tempdir().unwrap();
        let mp4_path = dir.path().join("hevc_test.mp4");
        let gen = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=96x64:rate=10:duration=1",
                "-c:v",
                hevc_enc,
                "-tag:v",
                "hvc1",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&mp4_path)
            .status()
            .unwrap();
        assert!(gen.success());

        let mut demuxer = Mp4Demuxer::open_path(&mp4_path).unwrap();
        let track = demuxer.video_tracks()[0].clone();
        assert_eq!(track.codec, VideoCodecId::Hevc);
        assert_eq!((track.width, track.height), (96, 64));
        assert!(!track.codec_config.is_empty());
        assert_eq!(track.samples.len(), 10);
        assert_eq!(
            track.samples.iter().map(|s| s.pts).min(),
            Some(0),
            "expected elst-rebased presentation timeline to start at 0"
        );

        let mut dec = rust_motion_hevc::Decoder::from_hvcc(&track.codec_config).unwrap();
        let mut buf = Vec::new();
        let mut decoded = Vec::new();
        for i in 0..track.samples.len() {
            let entry = demuxer.read_sample(0, i, &mut buf).unwrap();
            decoded.extend(dec.decode(&buf, entry.pts).unwrap());
        }
        decoded.extend(dec.flush());

        assert_eq!(decoded.len(), 10);
        for pic in &decoded {
            assert_eq!((pic.width, pic.height), (96, 64));
        }
    }
}
