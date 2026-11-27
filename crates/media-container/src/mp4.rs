//! Streaming ISO-BMFF (MP4) container muxer.

use std::io::{Read, Seek, SeekFrom, Write};

use rust_motion_media_core::{
    AudioCodecId, AudioTrackConfig, ColorDescription, ColorRange, ContainerMuxer, EncodedPacket,
    MediaError, TrackConfig, VideoCodecId, VideoTrackConfig,
};

const UNITY_MATRIX: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];
const FASTSTART_COPY_BUF_BYTES: usize = 256 * 1024;
const MDAT_EXTENDED_HEADER_BYTES: u64 = 16;

/// Configuration options for [`Mp4Muxer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mp4MuxerOptions {
    /// Place `moov` before `mdat` for progressive streaming playback.
    pub faststart: bool,
    /// Force 64-bit `co64` chunk offset boxes even when all offsets fit in `u32`.
    pub force_co64: bool,
}

impl Default for Mp4MuxerOptions {
    fn default() -> Self {
        Self {
            faststart: true,
            force_co64: false,
        }
    }
}

#[derive(Debug, Clone)]
struct RecordedSample {
    offset: u64,
    size: u32,
    cts: i64,
    duration: u32,
    is_keyframe: bool,
}

#[derive(Debug, Clone)]
struct TrackState {
    config: TrackConfig,
    samples: Vec<RecordedSample>,
}

/// Streaming ISO-BMFF / MP4 muxer writing compressed samples straight into `W`
/// while retaining only compact per-sample timing/offset metadata in memory.
pub struct Mp4Muxer<W> {
    writer: W,
    options: Mp4MuxerOptions,
    tracks: Vec<TrackState>,
    mdat_header_offset: u64,
    mdat_payload_cursor: u64,
    finished: bool,
}

impl<W: Read + Write + Seek> Mp4Muxer<W> {
    /// Initialize an MP4 container stream on `writer` and declare its tracks.
    pub fn new(
        mut writer: W,
        tracks: &[TrackConfig],
        options: Mp4MuxerOptions,
    ) -> Result<Self, MediaError> {
        if tracks.is_empty() {
            return Err(MediaError::InvalidConfig(
                "MP4 muxer requires at least one track".into(),
            ));
        }
        for track in tracks {
            validate_track_config(track)?;
        }

        let start_pos = writer.stream_position()?;
        let ftyp = build_ftyp(tracks);
        writer.write_all(&ftyp)?;

        let mdat_header_offset = start_pos + ftyp.len() as u64;
        // Reserve a 16-byte extended-size `mdat` header (`size == 1` + `b"mdat"` + u64 largesize)
        // so both sub-4 GiB and >4 GiB media payloads stream seamlessly without header resizing.
        let mut mdat_header = [0u8; 16];
        mdat_header[0..4].copy_from_slice(&1u32.to_be_bytes());
        mdat_header[4..8].copy_from_slice(b"mdat");
        writer.write_all(&mdat_header)?;

        let track_states = tracks
            .iter()
            .cloned()
            .map(|config| TrackState {
                config,
                samples: Vec::new(),
            })
            .collect();

        Ok(Self {
            writer,
            options,
            tracks: track_states,
            mdat_header_offset,
            mdat_payload_cursor: mdat_header_offset + MDAT_EXTENDED_HEADER_BYTES,
            finished: false,
        })
    }

    /// Set or update a video track's decoder configuration record (`avcC` / `hvcC`)
    /// before calling [`finish`](ContainerMuxer::finish).
    ///
    /// Allows streaming hardware encoders that emit parameter sets on the first encoded
    /// keyframe to write packets into `mdat` immediately and populate `stsd` at finalization.
    pub fn set_video_codec_config(
        &mut self,
        track_idx: usize,
        codec_config: Vec<u8>,
    ) -> Result<(), MediaError> {
        if self.finished {
            return Err(MediaError::Container(
                "cannot update codec_config after Mp4Muxer::finish".into(),
            ));
        }
        if codec_config.is_empty() {
            return Err(MediaError::InvalidConfig(
                "video codec_config must be non-empty".into(),
            ));
        }
        let Some(track) = self.tracks.get_mut(track_idx) else {
            return Err(MediaError::InvalidConfig(format!(
                "unknown track index {track_idx}"
            )));
        };
        match &mut track.config {
            TrackConfig::Video(v) => {
                v.codec_config = codec_config;
                Ok(())
            }
            TrackConfig::Audio(_) => Err(MediaError::InvalidConfig(format!(
                "track {track_idx} is an audio track, not a video track"
            ))),
        }
    }

    /// Finish writing the MP4 container and return the underlying writer.
    pub fn into_inner(mut self) -> Result<W, MediaError> {
        self.finish()?;
        Ok(self.writer)
    }

    fn write_packet_internal(&mut self, packet: &EncodedPacket) -> Result<(), MediaError> {
        if self.finished {
            return Err(MediaError::Container(
                "cannot write packets after Mp4Muxer::finish".into(),
            ));
        }
        let num_tracks = self.tracks.len();
        let track = self
            .tracks
            .get_mut(packet.stream_index as usize)
            .ok_or_else(|| {
                MediaError::InvalidConfig(format!(
                    "unknown stream_index {} for {num_tracks} track(s)",
                    packet.stream_index
                ))
            })?;

        let sample_size = u32::try_from(packet.data.len())
            .map_err(|_| MediaError::Container("encoded sample exceeds 4 GiB box limit".into()))?;
        let duration = u32::try_from(packet.duration.max(0))
            .map_err(|_| MediaError::Container("encoded sample duration exceeds u32".into()))?;

        self.writer.write_all(&packet.data)?;
        let sample_offset = self.mdat_payload_cursor;
        self.mdat_payload_cursor = self
            .mdat_payload_cursor
            .checked_add(u64::from(sample_size))
            .ok_or_else(|| MediaError::Container("MP4 file offset overflow".into()))?;

        track.samples.push(RecordedSample {
            offset: sample_offset,
            size: sample_size,
            cts: packet.pts.saturating_sub(packet.dts),
            duration,
            is_keyframe: packet.is_keyframe,
        });
        Ok(())
    }

    fn finish_internal(&mut self) -> Result<(), MediaError> {
        if self.finished {
            return Ok(());
        }
        let total_samples: usize = self.tracks.iter().map(|t| t.samples.len()).sum();
        if total_samples == 0 {
            return Err(MediaError::Container(
                "no samples written to MP4 muxer".into(),
            ));
        }
        for (idx, t) in self.tracks.iter().enumerate() {
            if let TrackConfig::Video(v) = &t.config {
                if v.codec_config.is_empty() {
                    return Err(MediaError::InvalidConfig(format!(
                        "video track {idx} ({:?}) is missing codec_config (avcC/hvcC)",
                        v.codec
                    )));
                }
            }
        }

        let mdat_end = self.mdat_payload_cursor;
        let mdat_total_size = mdat_end - self.mdat_header_offset;

        // Patch extended 64-bit `mdat` size.
        self.writer
            .seek(SeekFrom::Start(self.mdat_header_offset + 8))?;
        self.writer.write_all(&mdat_total_size.to_be_bytes())?;

        if self.options.faststart {
            // Fixed-point iteration: rebuilding `moov` with `offset_shift` normally keeps its
            // byte size identical unless shifting pushes a chunk offset past `u32::MAX` (`stco` -> `co64`).
            let initial_moov = build_moov(&self.tracks, 0, self.options.force_co64)?;
            let mut shift = initial_moov.len() as u64;
            let mut moov = build_moov(&self.tracks, shift, self.options.force_co64)?;
            if moov.len() as u64 != shift {
                shift = moov.len() as u64;
                moov = build_moov(&self.tracks, shift, self.options.force_co64)?;
                if moov.len() as u64 != shift {
                    return Err(MediaError::Container(
                        "failed to stabilize faststart moov box size".into(),
                    ));
                }
            }

            shift_region_forward(
                &mut self.writer,
                self.mdat_header_offset,
                mdat_total_size,
                shift,
            )?;
            self.writer.seek(SeekFrom::Start(self.mdat_header_offset))?;
            self.writer.write_all(&moov)?;
            self.writer.seek(SeekFrom::Start(mdat_end + shift))?;
        } else {
            let moov = build_moov(&self.tracks, 0, self.options.force_co64)?;
            self.writer.seek(SeekFrom::Start(mdat_end))?;
            self.writer.write_all(&moov)?;
        }

        self.writer.flush()?;
        self.finished = true;
        Ok(())
    }
}

impl<W: Read + Write + Seek> ContainerMuxer for Mp4Muxer<W> {
    fn write_packet(&mut self, packet: &EncodedPacket) -> Result<(), MediaError> {
        self.write_packet_internal(packet)
    }

    fn finish(&mut self) -> Result<(), MediaError> {
        self.finish_internal()
    }
}

fn validate_track_config(track: &TrackConfig) -> Result<(), MediaError> {
    match track {
        TrackConfig::Video(v) => {
            if !matches!(v.codec, VideoCodecId::H264 | VideoCodecId::Hevc) {
                return Err(MediaError::Unsupported(format!(
                    "MP4 muxer currently supports H.264 and HEVC video, got {:?}",
                    v.codec
                )));
            }
            if v.width == 0 || v.height == 0 || v.timescale == 0 {
                return Err(MediaError::InvalidConfig(
                    "video track width, height, and timescale must be > 0".into(),
                ));
            }
        }
        TrackConfig::Audio(a) => {
            if a.codec != AudioCodecId::Aac {
                return Err(MediaError::Unsupported(format!(
                    "MP4 muxer currently supports AAC audio, got {:?}",
                    a.codec
                )));
            }
            if a.sample_rate == 0 || a.channels == 0 || a.timescale == 0 {
                return Err(MediaError::InvalidConfig(
                    "audio track sample_rate, channels, and timescale must be > 0".into(),
                ));
            }
        }
    }
    Ok(())
}

/// Shift `[start .. start + len]` forward by `shift` bytes inside `rw` using a bounded stack/heap
/// buffer (back-to-front so overlapping regions never corrupt unread source bytes).
fn shift_region_forward<RW: Read + Write + Seek>(
    rw: &mut RW,
    start: u64,
    len: u64,
    shift: u64,
) -> Result<(), MediaError> {
    if len == 0 || shift == 0 {
        return Ok(());
    }
    let mut buf = vec![0u8; FASTSTART_COPY_BUF_BYTES];
    let mut remaining = len;
    while remaining > 0 {
        let chunk = (remaining as usize).min(buf.len());
        let src_pos = start + remaining - chunk as u64;
        rw.seek(SeekFrom::Start(src_pos))?;
        rw.read_exact(&mut buf[..chunk])?;
        rw.seek(SeekFrom::Start(src_pos + shift))?;
        rw.write_all(&buf[..chunk])?;
        remaining -= chunk as u64;
    }
    Ok(())
}

fn build_ftyp(tracks: &[TrackConfig]) -> Vec<u8> {
    let has_hevc = tracks
        .iter()
        .any(|t| matches!(t, TrackConfig::Video(v) if v.codec == VideoCodecId::Hevc));
    let mut payload = Vec::with_capacity(24);
    payload.extend_from_slice(b"isom");
    payload.extend_from_slice(&512u32.to_be_bytes());
    let brands: [&[u8; 4]; 4] = if has_hevc {
        [b"isom", b"iso2", b"hvc1", b"mp41"]
    } else {
        [b"isom", b"iso2", b"avc1", b"mp41"]
    };
    for brand in brands {
        payload.extend_from_slice(brand);
    }
    boxed(b"ftyp", &payload)
}

fn build_moov(
    tracks: &[TrackState],
    offset_shift: u64,
    force_co64: bool,
) -> Result<Vec<u8>, MediaError> {
    let movie_timescale = tracks
        .iter()
        .find_map(|t| match &t.config {
            TrackConfig::Video(v) => Some(v.timescale),
            TrackConfig::Audio(_) => None,
        })
        .unwrap_or_else(|| match &tracks[0].config {
            TrackConfig::Video(v) => v.timescale,
            TrackConfig::Audio(a) => a.timescale,
        });

    let mut movie_duration = 0u64;
    for t in tracks {
        let track_ts = track_timescale(&t.config);
        let track_dur: u64 = t.samples.iter().map(|s| u64::from(s.duration)).sum();
        let scaled = if track_ts == movie_timescale {
            track_dur
        } else {
            ((u128::from(track_dur) * u128::from(movie_timescale) + u128::from(track_ts) - 1)
                / u128::from(track_ts)) as u64
        };
        movie_duration = movie_duration.max(scaled);
    }

    let next_track_id = (tracks.len() as u32)
        .checked_add(1)
        .ok_or_else(|| MediaError::Container("too many tracks".into()))?;
    let mvhd = build_mvhd(movie_timescale, movie_duration, next_track_id);

    let mut moov = Vec::with_capacity(512 * tracks.len());
    moov.extend_from_slice(&mvhd);

    for (idx, track) in tracks.iter().enumerate() {
        let track_id = (idx + 1) as u32;
        let trak = build_trak(track_id, movie_timescale, track, offset_shift, force_co64)?;
        moov.extend_from_slice(&trak);
    }

    Ok(boxed(b"moov", &moov))
}

fn track_timescale(cfg: &TrackConfig) -> u32 {
    match cfg {
        TrackConfig::Video(v) => v.timescale,
        TrackConfig::Audio(a) => a.timescale,
    }
}

fn build_mvhd(timescale: u32, duration: u64, next_track_id: u32) -> Vec<u8> {
    let mut payload = Vec::new();
    let version = if duration > u64::from(u32::MAX) {
        payload.extend_from_slice(&0u64.to_be_bytes()); // creation_time
        payload.extend_from_slice(&0u64.to_be_bytes()); // modification_time
        payload.extend_from_slice(&timescale.to_be_bytes());
        payload.extend_from_slice(&duration.to_be_bytes());
        1u8
    } else {
        payload.extend_from_slice(&u32s(&[0, 0, timescale, duration as u32]));
        0u8
    };
    payload.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // rate = 1.0
    payload.extend_from_slice(&0x0100u16.to_be_bytes()); // volume = 1.0
    payload.extend_from_slice(&[0u8; 10]); // reserved
    payload.extend_from_slice(&u32s(&UNITY_MATRIX));
    payload.extend_from_slice(&[0u8; 24]); // pre_defined
    payload.extend_from_slice(&next_track_id.to_be_bytes());
    full_box(b"mvhd", version, 0, &payload)
}

fn build_trak(
    track_id: u32,
    movie_timescale: u32,
    track: &TrackState,
    offset_shift: u64,
    force_co64: bool,
) -> Result<Vec<u8>, MediaError> {
    let track_ts = track_timescale(&track.config);
    let track_dur: u64 = track.samples.iter().map(|s| u64::from(s.duration)).sum();
    let tkhd_dur = if track_ts == movie_timescale {
        track_dur
    } else {
        ((u128::from(track_dur) * u128::from(movie_timescale) + u128::from(track_ts) - 1)
            / u128::from(track_ts)) as u64
    };

    let (width_fx, height_fx, volume, media_header, handler_type, handler_name, stsd) =
        match &track.config {
            TrackConfig::Video(v) => (
                v.width << 16,
                v.height << 16,
                0u16,
                full_box(b"vmhd", 0, 1, &[0u8; 8]),
                b"vide",
                b"VideoHandler\0".as_slice(),
                build_video_stsd(v),
            ),
            TrackConfig::Audio(a) => (
                0u32,
                0u32,
                0x0100u16,
                full_box(b"smhd", 0, 0, &[0u8; 4]),
                b"soun",
                b"SoundHandler\0".as_slice(),
                build_audio_stsd(a),
            ),
        };

    // tkhd (flags = 3: track enabled | in movie)
    let mut tkhd = Vec::new();
    let tkhd_version = if tkhd_dur > u64::from(u32::MAX) {
        tkhd.extend_from_slice(&0u64.to_be_bytes());
        tkhd.extend_from_slice(&0u64.to_be_bytes());
        tkhd.extend_from_slice(&track_id.to_be_bytes());
        tkhd.extend_from_slice(&0u32.to_be_bytes());
        tkhd.extend_from_slice(&tkhd_dur.to_be_bytes());
        1u8
    } else {
        tkhd.extend_from_slice(&u32s(&[0, 0, track_id, 0, tkhd_dur as u32]));
        0u8
    };
    tkhd.extend_from_slice(&[0u8; 8]); // reserved
    tkhd.extend_from_slice(&0u16.to_be_bytes()); // layer
    tkhd.extend_from_slice(&0u16.to_be_bytes()); // alternate_group
    tkhd.extend_from_slice(&volume.to_be_bytes());
    tkhd.extend_from_slice(&0u16.to_be_bytes()); // reserved
    tkhd.extend_from_slice(&u32s(&UNITY_MATRIX));
    tkhd.extend_from_slice(&u32s(&[width_fx, height_fx]));
    let tkhd = full_box(b"tkhd", tkhd_version, 3, &tkhd);

    // mdhd
    let mut mdhd = Vec::new();
    let mdhd_version = if track_dur > u64::from(u32::MAX) {
        mdhd.extend_from_slice(&0u64.to_be_bytes());
        mdhd.extend_from_slice(&0u64.to_be_bytes());
        mdhd.extend_from_slice(&track_ts.to_be_bytes());
        mdhd.extend_from_slice(&track_dur.to_be_bytes());
        1u8
    } else {
        mdhd.extend_from_slice(&u32s(&[0, 0, track_ts, track_dur as u32]));
        0u8
    };
    mdhd.extend_from_slice(&0x55c4u16.to_be_bytes()); // "und"
    mdhd.extend_from_slice(&0u16.to_be_bytes());
    let mdhd = full_box(b"mdhd", mdhd_version, 0, &mdhd);

    // hdlr
    let mut hdlr = u32s(&[0]);
    hdlr.extend_from_slice(handler_type);
    hdlr.extend_from_slice(&[0u8; 12]);
    hdlr.extend_from_slice(handler_name);
    let hdlr = full_box(b"hdlr", 0, 0, &hdlr);

    // dinf / dref / url
    let url = full_box(b"url ", 0, 1, &[]);
    let mut dref = u32s(&[1]);
    dref.extend_from_slice(&url);
    let dinf = boxed(b"dinf", &full_box(b"dref", 0, 0, &dref));

    // Sample tables
    let stts = build_stts(&track.samples);
    let ctts = build_ctts(&track.samples)?;
    let stss = match &track.config {
        TrackConfig::Video(_) => build_stss(&track.samples),
        TrackConfig::Audio(_) => None,
    };
    let (stsc, chunk_offsets_box) = build_chunks(&track.samples, offset_shift, force_co64)?;
    let stsz = build_stsz(&track.samples);

    let mut stbl = Vec::new();
    stbl.extend_from_slice(&stsd);
    stbl.extend_from_slice(&stts);
    if let Some(ref ctts_box) = ctts {
        stbl.extend_from_slice(ctts_box);
    }
    if let Some(ref stss_box) = stss {
        stbl.extend_from_slice(stss_box);
    }
    stbl.extend_from_slice(&stsc);
    stbl.extend_from_slice(&stsz);
    stbl.extend_from_slice(&chunk_offsets_box);

    let mut minf = Vec::new();
    minf.extend_from_slice(&media_header);
    minf.extend_from_slice(&dinf);
    minf.extend_from_slice(&boxed(b"stbl", &stbl));

    let mut mdia = Vec::new();
    mdia.extend_from_slice(&mdhd);
    mdia.extend_from_slice(&hdlr);
    mdia.extend_from_slice(&boxed(b"minf", &minf));

    let mut trak = Vec::new();
    trak.extend_from_slice(&tkhd);
    trak.extend_from_slice(&boxed(b"mdia", &mdia));
    Ok(boxed(b"trak", &trak))
}

fn build_video_stsd(v: &VideoTrackConfig) -> Vec<u8> {
    let (sample_entry_box, config_box) = match v.codec {
        VideoCodecId::Hevc => (b"hvc1", b"hvcC"),
        _ => (b"avc1", b"avcC"),
    };
    let mut entry = vec![0u8; 6];
    entry.extend_from_slice(&1u16.to_be_bytes()); // data_reference_index
    entry.extend_from_slice(&[0u8; 16]); // pre_defined / reserved
    entry.extend_from_slice(&(v.width as u16).to_be_bytes());
    entry.extend_from_slice(&(v.height as u16).to_be_bytes());
    entry.extend_from_slice(&u32s(&[0x0048_0000, 0x0048_0000, 0])); // 72 dpi
    entry.extend_from_slice(&1u16.to_be_bytes()); // frame_count
    entry.extend_from_slice(&[0u8; 32]); // compressorname
    entry.extend_from_slice(&0x0018u16.to_be_bytes()); // depth
    entry.extend_from_slice(&(-1i16).to_be_bytes()); // pre_defined
    entry.extend_from_slice(&boxed(config_box, &v.codec_config));
    if v.color.is_specified() {
        entry.extend_from_slice(&build_colr_nclx(v.color));
    }

    let mut stsd = u32s(&[1]);
    stsd.extend_from_slice(&boxed(sample_entry_box, &entry));
    full_box(b"stsd", 0, 0, &stsd)
}

fn build_colr_nclx(color: ColorDescription) -> Vec<u8> {
    let mut payload = Vec::with_capacity(11);
    payload.extend_from_slice(b"nclx");
    payload.extend_from_slice(&color.primaries.to_cicp_u16().to_be_bytes());
    payload.extend_from_slice(&color.transfer.to_cicp_u16().to_be_bytes());
    payload.extend_from_slice(&color.matrix.to_cicp_u16().to_be_bytes());
    payload.push(if color.range == ColorRange::Full {
        0x80
    } else {
        0x00
    });
    boxed(b"colr", &payload)
}

fn build_audio_stsd(a: &AudioTrackConfig) -> Vec<u8> {
    let mut mp4a = vec![0u8; 6];
    mp4a.extend_from_slice(&1u16.to_be_bytes()); // data_reference_index
    mp4a.extend_from_slice(&[0u8; 8]); // reserved
    mp4a.extend_from_slice(&a.channels.to_be_bytes());
    mp4a.extend_from_slice(&16u16.to_be_bytes()); // sample_size
    mp4a.extend_from_slice(&0u16.to_be_bytes()); // pre_defined
    mp4a.extend_from_slice(&0u16.to_be_bytes()); // reserved
    mp4a.extend_from_slice(&(a.sample_rate << 16).to_be_bytes());

    let esds = build_esds(&a.codec_config);
    mp4a.extend_from_slice(&esds);

    let mut stsd = u32s(&[1]);
    stsd.extend_from_slice(&boxed(b"mp4a", &mp4a));
    full_box(b"stsd", 0, 0, &stsd)
}

fn build_esds(asc: &[u8]) -> Vec<u8> {
    // Minimal ES_Descriptor (tag 0x03) -> DecoderConfigDescriptor (0x04) -> DecoderSpecificInfo (0x05)
    let dec_spec_len = asc.len().min(127);
    let dec_spec_bytes = 2 + dec_spec_len;
    let dec_cfg_payload_len = 13 + dec_spec_bytes;
    let sl_cfg_bytes = 3; // tag 0x06, len 1, predef 0x02
    let es_descr_payload_len = 3 + 2 + dec_cfg_payload_len + sl_cfg_bytes;

    let mut payload = Vec::with_capacity(2 + es_descr_payload_len);
    payload.push(0x03);
    payload.push(es_descr_payload_len as u8);
    payload.extend_from_slice(&2u16.to_be_bytes()); // ES_ID
    payload.push(0x00); // flags

    payload.push(0x04);
    payload.push(dec_cfg_payload_len as u8);
    payload.push(0x40); // MPEG-4 Audio
    payload.push(0x15); // streamType = AudioStream (0x05 << 2 | 1)
    payload.extend_from_slice(&[0u8; 3]); // bufferSizeDB
    payload.extend_from_slice(&128_000u32.to_be_bytes()); // maxBitrate
    payload.extend_from_slice(&128_000u32.to_be_bytes()); // avgBitrate

    payload.push(0x05);
    payload.push(dec_spec_len as u8);
    payload.extend_from_slice(&asc[..dec_spec_len]);

    payload.push(0x06);
    payload.push(0x01);
    payload.push(0x02);

    full_box(b"esds", 0, 0, &payload)
}

fn build_stts(samples: &[RecordedSample]) -> Vec<u8> {
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for s in samples {
        if let Some(last) = runs.last_mut() {
            if last.1 == s.duration {
                last.0 += 1;
                continue;
            }
        }
        runs.push((1, s.duration));
    }
    let mut payload = Vec::with_capacity(4 + runs.len() * 8);
    payload.extend_from_slice(&(runs.len() as u32).to_be_bytes());
    for (count, delta) in runs {
        payload.extend_from_slice(&count.to_be_bytes());
        payload.extend_from_slice(&delta.to_be_bytes());
    }
    full_box(b"stts", 0, 0, &payload)
}

fn build_ctts(samples: &[RecordedSample]) -> Result<Option<Vec<u8>>, MediaError> {
    if samples.iter().all(|s| s.cts == 0) {
        return Ok(None);
    }
    let has_negative = samples.iter().any(|s| s.cts < 0);
    let mut runs: Vec<(u32, i64)> = Vec::new();
    for s in samples {
        if let Some(last) = runs.last_mut() {
            if last.1 == s.cts {
                last.0 += 1;
                continue;
            }
        }
        runs.push((1, s.cts));
    }
    let mut payload = Vec::with_capacity(4 + runs.len() * 8);
    payload.extend_from_slice(&(runs.len() as u32).to_be_bytes());
    let version = if has_negative {
        for (count, offset) in runs {
            let signed = i32::try_from(offset)
                .map_err(|_| MediaError::Container("ctts offset overflows i32".into()))?;
            payload.extend_from_slice(&count.to_be_bytes());
            payload.extend_from_slice(&signed.to_be_bytes());
        }
        1u8
    } else {
        for (count, offset) in runs {
            let unsigned = u32::try_from(offset)
                .map_err(|_| MediaError::Container("ctts offset overflows u32".into()))?;
            payload.extend_from_slice(&count.to_be_bytes());
            payload.extend_from_slice(&unsigned.to_be_bytes());
        }
        0u8
    };
    Ok(Some(full_box(b"ctts", version, 0, &payload)))
}

fn build_stss(samples: &[RecordedSample]) -> Option<Vec<u8>> {
    let sync: Vec<u32> = samples
        .iter()
        .enumerate()
        .filter(|(_, s)| s.is_keyframe)
        .map(|(i, _)| i as u32 + 1)
        .collect();
    if sync.is_empty() {
        return None;
    }
    let mut payload = Vec::with_capacity(4 + sync.len() * 4);
    payload.extend_from_slice(&(sync.len() as u32).to_be_bytes());
    payload.extend_from_slice(&u32s(&sync));
    Some(full_box(b"stss", 0, 0, &payload))
}

fn build_chunks(
    samples: &[RecordedSample],
    offset_shift: u64,
    force_co64: bool,
) -> Result<(Vec<u8>, Vec<u8>), MediaError> {
    struct Chunk {
        offset: u64,
        sample_count: u32,
    }
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut expected_next_offset = None::<u64>;

    for s in samples {
        let shifted = s
            .offset
            .checked_add(offset_shift)
            .ok_or_else(|| MediaError::Container("chunk offset overflow".into()))?;
        if expected_next_offset == Some(s.offset) {
            if let Some(last) = chunks.last_mut() {
                last.sample_count += 1;
            }
        } else {
            chunks.push(Chunk {
                offset: shifted,
                sample_count: 1,
            });
        }
        expected_next_offset = Some(s.offset + u64::from(s.size));
    }

    // Build stsc (RLE over sample_count).
    let mut stsc_entries: Vec<(u32, u32, u32)> = Vec::new();
    for (idx, chunk) in chunks.iter().enumerate() {
        let first_chunk = idx as u32 + 1;
        if stsc_entries
            .last()
            .is_none_or(|&(_, prev_cnt, _)| prev_cnt != chunk.sample_count)
        {
            stsc_entries.push((first_chunk, chunk.sample_count, 1));
        }
    }
    let mut stsc_payload = Vec::with_capacity(4 + stsc_entries.len() * 12);
    stsc_payload.extend_from_slice(&(stsc_entries.len() as u32).to_be_bytes());
    for (first, count, desc) in stsc_entries {
        stsc_payload.extend_from_slice(&u32s(&[first, count, desc]));
    }
    let stsc = full_box(b"stsc", 0, 0, &stsc_payload);

    // Emit co64 when any offset exceeds u32::MAX or force_co64 is enabled.
    let use_co64 = force_co64 || chunks.iter().any(|c| c.offset > u64::from(u32::MAX));
    let chunk_offsets_box = if use_co64 {
        let mut payload = Vec::with_capacity(4 + chunks.len() * 8);
        payload.extend_from_slice(&(chunks.len() as u32).to_be_bytes());
        for c in &chunks {
            payload.extend_from_slice(&c.offset.to_be_bytes());
        }
        full_box(b"co64", 0, 0, &payload)
    } else {
        let mut payload = Vec::with_capacity(4 + chunks.len() * 4);
        payload.extend_from_slice(&(chunks.len() as u32).to_be_bytes());
        for c in &chunks {
            payload.extend_from_slice(&(c.offset as u32).to_be_bytes());
        }
        full_box(b"stco", 0, 0, &payload)
    };

    Ok((stsc, chunk_offsets_box))
}

fn build_stsz(samples: &[RecordedSample]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(8 + samples.len() * 4);
    payload.extend_from_slice(&0u32.to_be_bytes()); // uniform sample size = 0 (per-entry table)
    payload.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    for s in samples {
        payload.extend_from_slice(&s.size.to_be_bytes());
    }
    full_box(b"stsz", 0, 0, &payload)
}

fn boxed(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 8);
    out.extend_from_slice(&((payload.len() as u32 + 8).to_be_bytes()));
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    out
}

fn full_box(kind: &[u8; 4], version: u8, flags: u32, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(payload.len() + 4);
    body.push(version);
    body.extend_from_slice(&flags.to_be_bytes()[1..]);
    body.extend_from_slice(payload);
    boxed(kind, &body)
}

fn u32s(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_motion_media_core::Rational;
    use std::io::Cursor;
    use std::process::Command;

    fn find_box_offset(buf: &[u8], kind: &[u8; 4]) -> Option<usize> {
        buf.windows(4).position(|w| w == kind)
    }

    fn video_track(avcc: &[u8]) -> TrackConfig {
        TrackConfig::Video(VideoTrackConfig {
            width: 64,
            height: 48,
            timescale: 30000,
            fps: Rational::new(30000, 1001),
            codec: VideoCodecId::H264,
            codec_config: avcc.to_vec(),
            color: ColorDescription::bt709_limited(),
        })
    }

    fn sample_packet(stream_index: u32, idx: i64, keyframe: bool, data: Vec<u8>) -> EncodedPacket {
        EncodedPacket {
            stream_index,
            data,
            pts: idx * 1001,
            dts: idx * 1001,
            duration: 1001,
            is_keyframe: keyframe,
        }
    }

    #[test]
    fn faststart_and_non_faststart_place_moov_and_colr_correctly() {
        let avcc = [1u8, 100, 0, 30, 0xFF, 0xE1];
        for faststart in [true, false] {
            let mut muxer = Mp4Muxer::new(
                Cursor::new(Vec::new()),
                &[video_track(&avcc)],
                Mp4MuxerOptions {
                    faststart,
                    force_co64: false,
                },
            )
            .unwrap();
            for i in 0..3 {
                muxer
                    .write_packet(&sample_packet(
                        0,
                        i,
                        i == 0,
                        vec![0, 0, 0, 2, 0x65, i as u8],
                    ))
                    .unwrap();
            }
            let file = muxer.into_inner().unwrap().into_inner();
            assert_eq!(&file[4..8], b"ftyp");
            for kind in [
                b"moov", b"mdat", b"avc1", b"avcC", b"colr", b"nclx", b"stts", b"stss", b"stsc",
                b"stsz", b"stco",
            ] {
                assert!(find_box_offset(&file, kind).is_some(), "missing {kind:?}");
            }
            let moov = find_box_offset(&file, b"moov").unwrap();
            let mdat = find_box_offset(&file, b"mdat").unwrap();
            assert_eq!(moov < mdat, faststart);

            let stco = find_box_offset(&file, b"stco").unwrap();
            let off = u32::from_be_bytes(file[stco + 12..stco + 16].try_into().unwrap()) as usize;
            assert_eq!(&file[off..off + 6], &[0, 0, 0, 2, 0x65, 0]);
        }
    }

    #[test]
    fn emits_ctts_for_bframe_reordering_and_co64_when_enabled() {
        let avcc = [1u8, 100, 0, 30, 0xFF, 0xE1];
        let mut muxer = Mp4Muxer::new(
            Cursor::new(Vec::new()),
            &[video_track(&avcc)],
            Mp4MuxerOptions {
                faststart: true,
                force_co64: true,
            },
        )
        .unwrap();
        let packets = [
            EncodedPacket {
                stream_index: 0,
                data: vec![0, 0, 0, 2, 0x65, 0],
                pts: 2002,
                dts: 0,
                duration: 1001,
                is_keyframe: true,
            },
            EncodedPacket {
                stream_index: 0,
                data: vec![0, 0, 0, 2, 0x41, 1],
                pts: 1001,
                dts: 1001,
                duration: 1001,
                is_keyframe: false,
            },
        ];
        for pkt in &packets {
            muxer.write_packet(pkt).unwrap();
        }
        let file = muxer.into_inner().unwrap().into_inner();
        assert!(find_box_offset(&file, b"ctts").is_some(), "missing ctts");
        let co64 = find_box_offset(&file, b"co64").expect("missing co64");
        assert!(find_box_offset(&file, b"stco").is_none(), "unexpected stco");
        let off = u64::from_be_bytes(file[co64 + 12..co64 + 20].try_into().unwrap()) as usize;
        assert_eq!(&file[off..off + 6], &[0, 0, 0, 2, 0x65, 0]);
    }

    #[test]
    fn interleaved_video_and_audio_tracks_have_exact_chunk_offsets() {
        let avcc = [1u8, 100, 0, 30, 0xFF, 0xE1];
        let audio = TrackConfig::Audio(AudioTrackConfig {
            codec: AudioCodecId::Aac,
            sample_rate: 48000,
            channels: 2,
            timescale: 48000,
            codec_config: vec![0x11, 0x90],
        });
        let mut muxer = Mp4Muxer::new(
            Cursor::new(Vec::new()),
            &[video_track(&avcc), audio],
            Mp4MuxerOptions {
                faststart: true,
                force_co64: false,
            },
        )
        .unwrap();
        // Interleave V0, A0, V1, V2, A1 -> Video has 2 chunks (1 sample, 2 samples); Audio has 2 chunks.
        muxer
            .write_packet(&sample_packet(0, 0, true, vec![0xAA; 6]))
            .unwrap();
        muxer
            .write_packet(&sample_packet(1, 0, true, vec![0xBB; 4]))
            .unwrap();
        muxer
            .write_packet(&sample_packet(0, 1, false, vec![0xCC; 6]))
            .unwrap();
        muxer
            .write_packet(&sample_packet(0, 2, false, vec![0xDD; 6]))
            .unwrap();
        muxer
            .write_packet(&sample_packet(1, 1, true, vec![0xEE; 5]))
            .unwrap();

        let file = muxer.into_inner().unwrap().into_inner();
        assert!(find_box_offset(&file, b"mp4a").is_some());
        assert!(find_box_offset(&file, b"esds").is_some());

        // Check first track stco has 2 chunk offsets pointing at 0xAA.. and 0xCC..
        let first_stco = find_box_offset(&file, b"stco").unwrap();
        let chunk_count =
            u32::from_be_bytes(file[first_stco + 8..first_stco + 12].try_into().unwrap());
        assert_eq!(chunk_count, 2);
        let c0 =
            u32::from_be_bytes(file[first_stco + 12..first_stco + 16].try_into().unwrap()) as usize;
        let c1 =
            u32::from_be_bytes(file[first_stco + 16..first_stco + 20].try_into().unwrap()) as usize;
        assert_eq!(&file[c0..c0 + 6], &[0xAA; 6]);
        assert_eq!(&file[c1..c1 + 6], &[0xCC; 6]);

        // Check second track stco points at 0xBB.. and 0xEE..
        let second_stco =
            first_stco + 4 + find_box_offset(&file[first_stco + 4..], b"stco").unwrap();
        let a_count =
            u32::from_be_bytes(file[second_stco + 8..second_stco + 12].try_into().unwrap());
        assert_eq!(a_count, 2);
        let a0 = u32::from_be_bytes(file[second_stco + 12..second_stco + 16].try_into().unwrap())
            as usize;
        let a1 = u32::from_be_bytes(file[second_stco + 16..second_stco + 20].try_into().unwrap())
            as usize;
        assert_eq!(&file[a0..a0 + 4], &[0xBB; 4]);
        assert_eq!(&file[a1..a1 + 5], &[0xEE; 5]);
    }

    #[test]
    fn differential_conformance_with_h264enc_and_ffprobe() {
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

        use rust_motion_h264enc::{Encoder, EncoderConfig, PacketFormat, RateControl, YuvFrame};
        let (w, h) = (96u32, 64u32);
        let mut cfg = EncoderConfig::new(w, h, 24, 1);
        cfg.rate = RateControl::Crf(22.0);
        cfg.bframes = 2;
        cfg.format = PacketFormat::LengthPrefixed;
        let mut encoder = Encoder::new(cfg).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let mp4_path = dir.path().join("conformance.mp4");
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
            codec_config: encoder.avcc(),
            color: ColorDescription::bt709_limited(),
        });
        let mut muxer = Mp4Muxer::new(file, &[track], Mp4MuxerOptions::default()).unwrap();

        let y_len = (w * h) as usize;
        let uv_len = y_len / 4;
        for f in 0..8i64 {
            let y = vec![(32 + f * 18) as u8; y_len];
            let u = vec![100u8; uv_len];
            let v = vec![(140 - f * 8) as u8; uv_len];
            let frame = YuvFrame {
                y: &y,
                u: &u,
                v: &v,
                y_stride: w as usize,
                uv_stride: (w / 2) as usize,
            };
            for pkt in encoder.encode(&frame, f).unwrap() {
                muxer
                    .write_packet(&EncodedPacket {
                        stream_index: 0,
                        data: pkt.data,
                        pts: pkt.pts,
                        dts: pkt.dts,
                        duration: 1,
                        is_keyframe: pkt.keyframe,
                    })
                    .unwrap();
            }
        }
        for pkt in encoder.flush() {
            muxer
                .write_packet(&EncodedPacket {
                    stream_index: 0,
                    data: pkt.data,
                    pts: pkt.pts,
                    dts: pkt.dts,
                    duration: 1,
                    is_keyframe: pkt.keyframe,
                })
                .unwrap();
        }
        muxer.finish().unwrap();

        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name,width,height,nb_frames,color_primaries",
                "-of",
                "default=noprint_wrappers=1",
            ])
            .arg(&mp4_path)
            .output()
            .unwrap();
        assert!(probe.status.success(), "ffprobe failed");
        let stdout = String::from_utf8_lossy(&probe.stdout);
        assert!(stdout.contains("codec_name=h264"), "stdout={stdout}");
        assert!(stdout.contains("width=96"), "stdout={stdout}");
        assert!(stdout.contains("height=64"), "stdout={stdout}");
        assert!(stdout.contains("nb_frames=8"), "stdout={stdout}");
        assert!(stdout.contains("color_primaries=bt709"), "stdout={stdout}");

        // Decode every frame through FFmpeg to verify bitstream + ctts conformance end-to-end.
        let decode = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&mp4_path)
            .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
            .output()
            .unwrap();
        assert!(
            decode.status.success(),
            "ffmpeg decode failed: {}",
            String::from_utf8_lossy(&decode.stderr)
        );
        assert_eq!(decode.stdout.len(), 8 * (w as usize) * (h as usize) * 3);
    }

    #[test]
    fn hevc_hvc1_mux_and_deferred_codec_config_roundtrips_through_demuxer() {
        let hvcc = vec![
            1u8, 1, 0x60, 0, 0, 0, 0, 0, 0, 0, 0, 0, 90, 0xF0, 0, 0xFC, 0xFD, 0xF8, 0xF8, 0, 0, 3,
            0,
        ];
        let track = TrackConfig::Video(VideoTrackConfig {
            width: 128,
            height: 72,
            timescale: 24,
            fps: Rational::new(24, 1),
            codec: VideoCodecId::Hevc,
            codec_config: Vec::new(),
            color: ColorDescription::bt709_limited(),
        });

        let mut muxer = Mp4Muxer::new(
            Cursor::new(Vec::new()),
            &[track],
            Mp4MuxerOptions::default(),
        )
        .unwrap();
        muxer
            .write_packet(&sample_packet(
                0,
                0,
                true,
                vec![0, 0, 0, 3, 0x26, 0x01, 0xAF],
            ))
            .unwrap();
        muxer
            .write_packet(&sample_packet(0, 1, false, vec![0, 0, 0, 2, 0x02, 0x01]))
            .unwrap();
        muxer.set_video_codec_config(0, hvcc.clone()).unwrap();

        let mp4_bytes = muxer.into_inner().unwrap().into_inner();
        assert!(find_box_offset(&mp4_bytes, b"hvc1").is_some());
        assert!(find_box_offset(&mp4_bytes, b"hvcC").is_some());
        assert_eq!(&mp4_bytes[16..28], b"isomiso2hvc1");

        let mut demuxer = crate::Mp4Demuxer::open(Cursor::new(mp4_bytes)).unwrap();
        let demuxed = &demuxer.video_tracks()[0];
        assert_eq!(demuxed.codec, VideoCodecId::Hevc);
        assert_eq!((demuxed.width, demuxed.height), (128, 72));
        assert_eq!(demuxed.codec_config, hvcc);
        assert_eq!(demuxed.samples.len(), 2);

        let mut buf = Vec::new();
        let s0 = demuxer.read_sample(0, 0, &mut buf).unwrap();
        assert!(s0.is_keyframe);
        assert_eq!(buf, vec![0, 0, 0, 3, 0x26, 0x01, 0xAF]);
    }
}
