//! Minimal native video container probe for MP4/MOV/MKV.
//!
//! This is a best-effort metadata reader used when FFmpeg/FFprobe is unavailable.
//! It does not decode media.

use crate::backend::RasterError;
use crate::video_cache::{media_error, VideoMetadata};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub fn probe_native(path: &Path) -> Result<VideoMetadata, RasterError> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| media_error(path.display().to_string(), format!("open failed: {e}")))?;
    let mut head = [0u8; 12];
    file.read_exact(&mut head)
        .map_err(|e| media_error(path.display().to_string(), format!("read failed: {e}")))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|e| media_error(path.display().to_string(), format!("seek failed: {e}")))?;
    // MP4/MOV: ....ftyp / ....moov / ....free etc. EBML/MKV: 1A 45 DF A3
    if head[4..8] == *b"ftyp"
        || head[4..8] == *b"moov"
        || head[4..8] == *b"free"
        || head[4..8] == *b"mdat"
        || head[4..8] == *b"wide"
    {
        return probe_mp4(&mut file, path);
    }
    if head[0..4] == [0x1A, 0x45, 0xDF, 0xA3] {
        return probe_mkv(&mut file, path);
    }
    Err(media_error(
        path.display().to_string(),
        "unsupported container for native probe",
    ))
}

fn probe_mp4(file: &mut std::fs::File, path: &Path) -> Result<VideoMetadata, RasterError> {
    let len = file
        .metadata()
        .map_err(|e| media_error(path.display().to_string(), e.to_string()))?
        .len();
    let mut pos = 0u64;
    let mut moov: Option<(u64, u64)> = None;
    while pos + 8 <= len {
        let (size, typ) = read_box_header(file, pos, len)
            .map_err(|e| media_error(path.display().to_string(), e))?;
        if typ == *b"moov" {
            let h = header_size(file, pos, len)
                .map_err(|e| media_error(path.display().to_string(), e))?;
            moov = Some((pos + h, size.saturating_sub(h)));
            break;
        }
        if size == 0 {
            break;
        }
        pos = pos.saturating_add(size);
    }
    let (moov_off, moov_size) =
        moov.ok_or_else(|| media_error(path.display().to_string(), "mp4: no moov box"))?;
    parse_moov(file, moov_off, moov_size, path)
}

fn header_size(file: &mut std::fs::File, pos: u64, _len: u64) -> Result<u64, String> {
    let mut b = [0u8; 8];
    file.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;
    file.read_exact(&mut b).map_err(|e| e.to_string())?;
    let sz = u32::from_be_bytes(b[0..4].try_into().unwrap()) as u64;
    Ok(if sz == 1 { 16 } else { 8 })
}

fn read_box_header(file: &mut std::fs::File, pos: u64, len: u64) -> Result<(u64, [u8; 4]), String> {
    let mut b = [0u8; 8];
    file.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;
    file.read_exact(&mut b).map_err(|e| e.to_string())?;
    let mut size = u32::from_be_bytes(b[0..4].try_into().unwrap()) as u64;
    let typ: [u8; 4] = b[4..8].try_into().unwrap();
    if size == 1 {
        let mut b8 = [0u8; 8];
        file.read_exact(&mut b8).map_err(|e| e.to_string())?;
        size = u64::from_be_bytes(b8);
    } else if size == 0 {
        size = len.saturating_sub(pos);
    }
    if size < 8 {
        return Err("invalid box size".into());
    }
    Ok((size, typ))
}

#[derive(Default, Clone)]
struct TrackInfo {
    is_video: bool,
    is_audio: bool,
    width: u32,
    height: u32,
    duration_secs: Option<f64>,
    fps: Option<f64>,
    codec_name: Option<String>,
}

fn parse_moov(
    file: &mut std::fs::File,
    off: u64,
    size: u64,
    path: &Path,
) -> Result<VideoMetadata, RasterError> {
    let mut pos = off;
    let end = off.saturating_add(size);
    let mut mvhd_timescale: u32 = 0;
    let mut mvhd_duration: u64 = 0;
    let mut tracks: Vec<TrackInfo> = Vec::new();
    while pos + 8 <= end {
        let (sz, typ) = read_box_header(file, pos, end)
            .map_err(|e| media_error(path.display().to_string(), e))?;
        let h =
            header_size(file, pos, end).map_err(|e| media_error(path.display().to_string(), e))?;
        let payload_off = pos + h;
        let payload_sz = sz.saturating_sub(h);
        if typ == *b"mvhd" {
            if let Ok((ts, dur)) = parse_mvhd(file, payload_off, payload_sz) {
                mvhd_timescale = ts;
                mvhd_duration = dur;
            }
        } else if typ == *b"trak" {
            if let Ok(t) = parse_trak(file, payload_off, payload_sz) {
                tracks.push(t);
            }
        }
        if sz == 0 {
            break;
        }
        pos = pos.saturating_add(sz);
    }
    let mvhd_dur = if mvhd_timescale > 0 {
        Some(mvhd_duration as f64 / mvhd_timescale as f64)
    } else {
        None
    };
    let mut video = tracks
        .iter()
        .find(|t| t.is_video && t.width > 0 && t.height > 0)
        .cloned()
        .ok_or_else(|| media_error(path.display().to_string(), "mp4: no video track"))?;
    if video.duration_secs.is_none() {
        video.duration_secs = mvhd_dur;
    }
    let audio_indices: Vec<usize> = tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.is_audio)
        .map(|(i, _)| i)
        .collect();
    Ok(VideoMetadata {
        width: video.width,
        height: video.height,
        display_width: video.width,
        display_height: video.height,
        duration: video.duration_secs,
        fps: video.fps,
        rotation: 0,
        video_stream_index: tracks.iter().position(|t| t.is_video).unwrap_or(0),
        audio_stream_indices: audio_indices,
        codec_name: video.codec_name,
        pixel_format: None,
    })
}

fn read_u32(file: &mut std::fs::File, off: u64) -> Result<u32, String> {
    let mut b = [0u8; 4];
    file.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
    file.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(u32::from_be_bytes(b))
}
fn read_u64(file: &mut std::fs::File, off: u64) -> Result<u64, String> {
    let mut b = [0u8; 8];
    file.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
    file.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(u64::from_be_bytes(b))
}

fn parse_mvhd(file: &mut std::fs::File, off: u64, size: u64) -> Result<(u32, u64), String> {
    if size < 20 {
        return Err("mvhd too small".into());
    }
    let mut ver = [0u8; 1];
    file.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
    file.read_exact(&mut ver).map_err(|e| e.to_string())?;
    if ver[0] == 1 {
        if size < 32 {
            return Err("mvhd v1 too small".into());
        }
        let ts = read_u32(file, off + 20)?;
        let dur = read_u64(file, off + 24)?;
        Ok((ts, dur))
    } else {
        let ts = read_u32(file, off + 12)?;
        let dur = read_u32(file, off + 16)? as u64;
        Ok((ts, dur))
    }
}

fn parse_trak(file: &mut std::fs::File, off: u64, size: u64) -> Result<TrackInfo, String> {
    let mut pos = off;
    let end = off.saturating_add(size);
    let mut info = TrackInfo::default();
    while pos + 8 <= end {
        let (sz, typ) = read_box_header(file, pos, end)?;
        let h = header_size(file, pos, end)?;
        let p = pos + h;
        let ps = sz.saturating_sub(h);
        if typ == *b"tkhd" {
            if let Ok((w, hgt)) = parse_tkhd(file, p, ps) {
                if w > 0 && hgt > 0 {
                    info.width = w;
                    info.height = hgt;
                    info.is_video = true;
                }
            }
        } else if typ == *b"mdia" {
            if let Ok((is_a, is_v, dur, fps, codec)) = parse_mdia(file, p, ps) {
                info.is_audio |= is_a;
                info.is_video |= is_v;
                if info.duration_secs.is_none() {
                    info.duration_secs = dur;
                }
                if info.fps.is_none() {
                    info.fps = fps;
                }
                if info.codec_name.is_none() {
                    info.codec_name = codec;
                }
            }
        }
        if sz == 0 {
            break;
        }
        pos = pos.saturating_add(sz);
    }
    Ok(info)
}

fn parse_tkhd(file: &mut std::fs::File, off: u64, size: u64) -> Result<(u32, u32), String> {
    if size < 84 {
        return Err("tkhd small".into());
    }
    let mut ver = [0u8; 1];
    file.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
    file.read_exact(&mut ver).map_err(|e| e.to_string())?;
    let wh_off = if ver[0] == 1 { off + 84 } else { off + 76 };
    if wh_off + 8 > off + size {
        return Err("tkhd wh".into());
    }
    let w = read_u32(file, wh_off)? >> 16;
    let h = read_u32(file, wh_off + 4)? >> 16;
    Ok((w, h))
}

fn parse_mdia(
    file: &mut std::fs::File,
    off: u64,
    size: u64,
) -> Result<(bool, bool, Option<f64>, Option<f64>, Option<String>), String> {
    let mut pos = off;
    let end = off.saturating_add(size);
    let mut is_audio = false;
    let mut is_video = false;
    let mut dur = None;
    let mut fps = None;
    let mut codec = None;
    let mut mdhd_ts = 0u32;
    let mut mdhd_dur = 0u64;
    while pos + 8 <= end {
        let (sz, typ) = read_box_header(file, pos, end)?;
        let h = header_size(file, pos, end)?;
        let p = pos + h;
        let ps = sz.saturating_sub(h);
        if typ == *b"mdhd" {
            if let Ok((ts, d)) = parse_mdhd(file, p, ps) {
                mdhd_ts = ts;
                mdhd_dur = d;
            }
        } else if typ == *b"hdlr" {
            if let Ok(k) = parse_hdlr(file, p, ps) {
                if k == *b"vide" {
                    is_video = true;
                }
                if k == *b"soun" {
                    is_audio = true;
                }
            }
        } else if typ == *b"minf" {
            if let Ok((c, f)) = parse_minf(file, p, ps) {
                if codec.is_none() {
                    codec = c;
                }
                if fps.is_none() {
                    fps = f;
                }
            }
        }
        if sz == 0 {
            break;
        }
        pos = pos.saturating_add(sz);
    }
    if mdhd_ts > 0 {
        dur = Some(mdhd_dur as f64 / mdhd_ts as f64);
    }
    Ok((is_audio, is_video, dur, fps, codec))
}

fn parse_mdhd(file: &mut std::fs::File, off: u64, size: u64) -> Result<(u32, u64), String> {
    if size < 20 {
        return Err("mdhd small".into());
    }
    let mut ver = [0u8; 1];
    file.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
    file.read_exact(&mut ver).map_err(|e| e.to_string())?;
    if ver[0] == 1 {
        if size < 32 {
            return Err("mdhd v1".into());
        }
        Ok((read_u32(file, off + 20)?, read_u64(file, off + 24)?))
    } else {
        Ok((read_u32(file, off + 12)?, read_u32(file, off + 16)? as u64))
    }
}

fn parse_hdlr(file: &mut std::fs::File, off: u64, size: u64) -> Result<[u8; 4], String> {
    if size < 12 {
        return Err("hdlr small".into());
    }
    let mut b = [0u8; 4];
    file.seek(SeekFrom::Start(off + 8))
        .map_err(|e| e.to_string())?;
    file.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(b)
}

fn parse_minf(
    file: &mut std::fs::File,
    off: u64,
    size: u64,
) -> Result<(Option<String>, Option<f64>), String> {
    let mut pos = off;
    let end = off.saturating_add(size);
    let mut codec = None;
    let mut fps = None;
    while pos + 8 <= end {
        let (sz, typ) = read_box_header(file, pos, end)?;
        let h = header_size(file, pos, end)?;
        let p = pos + h;
        let ps = sz.saturating_sub(h);
        if typ == *b"stbl" {
            if let Ok((c, f)) = parse_stbl(file, p, ps) {
                codec = c;
                fps = f;
            }
        }
        if sz == 0 {
            break;
        }
        pos = pos.saturating_add(sz);
    }
    Ok((codec, fps))
}

fn parse_stbl(
    file: &mut std::fs::File,
    off: u64,
    size: u64,
) -> Result<(Option<String>, Option<f64>), String> {
    let mut pos = off;
    let end = off.saturating_add(size);
    let mut codec = None;
    let mut fps = None;
    while pos + 8 <= end {
        let (sz, typ) = read_box_header(file, pos, end)?;
        let h = header_size(file, pos, end)?;
        let p = pos + h;
        let ps = sz.saturating_sub(h);
        if typ == *b"stsd" {
            codec = parse_stsd_codec(file, p, ps).ok().flatten();
        } else if typ == *b"stts" {
            fps = parse_stts_fps(file, p, ps).ok().flatten();
        }
        if sz == 0 {
            break;
        }
        pos = pos.saturating_add(sz);
    }
    Ok((codec, fps))
}

fn parse_stsd_codec(
    file: &mut std::fs::File,
    off: u64,
    size: u64,
) -> Result<Option<String>, String> {
    if size < 16 {
        return Ok(None);
    }
    let mut b = [0u8; 8];
    file.seek(SeekFrom::Start(off + 8))
        .map_err(|e| e.to_string())?;
    file.read_exact(&mut b).map_err(|e| e.to_string())?;
    let name = String::from_utf8_lossy(&b[4..8]).trim().to_string();
    if name.is_empty() {
        Ok(None)
    } else {
        Ok(Some(name))
    }
}

fn parse_stts_fps(file: &mut std::fs::File, off: u64, size: u64) -> Result<Option<f64>, String> {
    if size < 12 {
        return Ok(None);
    }
    let count = read_u32(file, off + 4)?;
    if count == 0 {
        return Ok(None);
    }
    // Use first entry as representative (common case)
    let sample_count = read_u32(file, off + 8)?;
    let sample_delta = read_u32(file, off + 12)?;
    if sample_count == 0 || sample_delta == 0 {
        return Ok(None);
    }
    // fps requires timescale; caller can compute better from mdhd. Return delta-based hint only when sensible.
    let _ = sample_count;
    Ok(None)
}

fn probe_mkv(_file: &mut std::fs::File, path: &Path) -> Result<VideoMetadata, RasterError> {
    Err(media_error(
        path.display().to_string(),
        "native MKV/WebM probe not yet implemented; ffprobe required",
    ))
}
