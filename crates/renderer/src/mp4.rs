//! Minimal ISO-BMFF (MP4) muxer for a single H.264 video track.
//!
//! Used by the pure-Rust encoder fallback so that files named `*.mp4` really are MP4
//! containers instead of raw Annex-B elementary streams. Only what is needed for
//! progressive playback is implemented: one track, one chunk, no B-frame reordering
//! (`dts == pts`), 4-byte length-prefixed samples.

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

const UNITY_MATRIX: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];

fn build_moov(p: &Mp4Params, samples: &[Sample], mdat_data_offset: u32) -> Result<Vec<u8>, String> {
    let n = samples.len() as u32;
    let duration = u32::try_from(u64::from(n) * u64::from(p.fps_den))
        .map_err(|_| "MP4 duration overflows 32 bits".to_string())?;
    let timescale = p.fps_num;

    // mvhd
    let mut mvhd = u32s(&[0, 0, timescale, duration, 0x0001_0000]);
    mvhd.extend_from_slice(&0x0100u16.to_be_bytes()); // volume
    mvhd.extend_from_slice(&[0; 10]); // reserved
    mvhd.extend_from_slice(&u32s(&UNITY_MATRIX));
    mvhd.extend_from_slice(&[0; 24]); // pre_defined
    mvhd.extend_from_slice(&2u32.to_be_bytes()); // next_track_ID
    let mvhd = full_box(b"mvhd", 0, 0, &mvhd);

    // tkhd (enabled | in movie)
    let mut tkhd = u32s(&[0, 0, 1, 0, duration]);
    tkhd.extend_from_slice(&[0; 8]); // reserved
    tkhd.extend_from_slice(&[0; 8]); // layer, alternate_group, volume, reserved
    tkhd.extend_from_slice(&u32s(&UNITY_MATRIX));
    tkhd.extend_from_slice(&u32s(&[p.width << 16, p.height << 16]));
    let tkhd = full_box(b"tkhd", 0, 3, &tkhd);

    // mdhd
    let mut mdhd = u32s(&[0, 0, timescale, duration]);
    mdhd.extend_from_slice(&0x55c4u16.to_be_bytes()); // language "und"
    mdhd.extend_from_slice(&0u16.to_be_bytes());
    let mdhd = full_box(b"mdhd", 0, 0, &mdhd);

    // hdlr
    let mut hdlr = u32s(&[0]);
    hdlr.extend_from_slice(b"vide");
    hdlr.extend_from_slice(&[0; 12]);
    hdlr.extend_from_slice(b"VideoHandler\0");
    let hdlr = full_box(b"hdlr", 0, 0, &hdlr);

    // minf: vmhd + dinf + stbl
    let vmhd = full_box(b"vmhd", 0, 1, &[0; 8]);
    let url = full_box(b"url ", 0, 1, &[]);
    let mut dref = u32s(&[1]);
    dref.extend_from_slice(&url);
    let dinf = boxed(b"dinf", &full_box(b"dref", 0, 0, &dref));

    // stsd / avc1 / avcC
    let mut avc1 = vec![0u8; 6];
    avc1.extend_from_slice(&1u16.to_be_bytes()); // data_reference_index
    avc1.extend_from_slice(&[0; 16]); // pre_defined / reserved
    avc1.extend_from_slice(&(p.width as u16).to_be_bytes());
    avc1.extend_from_slice(&(p.height as u16).to_be_bytes());
    avc1.extend_from_slice(&u32s(&[0x0048_0000, 0x0048_0000, 0]));
    avc1.extend_from_slice(&1u16.to_be_bytes()); // frame_count
    avc1.extend_from_slice(&[0; 32]); // compressorname
    avc1.extend_from_slice(&0x0018u16.to_be_bytes()); // depth
    avc1.extend_from_slice(&(-1i16).to_be_bytes()); // pre_defined
    avc1.extend_from_slice(&boxed(b"avcC", p.avcc));
    let mut stsd = u32s(&[1]);
    stsd.extend_from_slice(&boxed(b"avc1", &avc1));
    let stsd = full_box(b"stsd", 0, 0, &stsd);

    let stts = full_box(b"stts", 0, 0, &u32s(&[1, n, p.fps_den]));

    let sync: Vec<u32> = samples
        .iter()
        .enumerate()
        .filter(|(_, s)| s.keyframe)
        .map(|(i, _)| i as u32 + 1)
        .collect();
    let mut stss = u32s(&[sync.len() as u32]);
    stss.extend_from_slice(&u32s(&sync));
    let stss = full_box(b"stss", 0, 0, &stss);

    let stsc = full_box(b"stsc", 0, 0, &u32s(&[1, 1, n, 1]));

    let mut stsz = u32s(&[0, n]);
    for s in samples {
        let len = u32::try_from(s.data.len()).map_err(|_| "sample too large".to_string())?;
        stsz.extend_from_slice(&len.to_be_bytes());
    }
    let stsz = full_box(b"stsz", 0, 0, &stsz);

    let stco = full_box(b"stco", 0, 0, &u32s(&[1, mdat_data_offset]));

    let mut stbl = Vec::new();
    for part in [&stsd, &stts, &stss, &stsc, &stsz, &stco] {
        stbl.extend_from_slice(part);
    }
    let mut minf = Vec::new();
    minf.extend_from_slice(&vmhd);
    minf.extend_from_slice(&dinf);
    minf.extend_from_slice(&boxed(b"stbl", &stbl));

    let mut mdia = Vec::new();
    mdia.extend_from_slice(&mdhd);
    mdia.extend_from_slice(&hdlr);
    mdia.extend_from_slice(&boxed(b"minf", &minf));

    let mut trak = Vec::new();
    trak.extend_from_slice(&tkhd);
    trak.extend_from_slice(&boxed(b"mdia", &mdia));

    let mut moov = Vec::new();
    moov.extend_from_slice(&mvhd);
    moov.extend_from_slice(&boxed(b"trak", &trak));
    Ok(boxed(b"moov", &moov))
}

/// Build a complete MP4 file in memory.
pub fn mux(p: &Mp4Params, samples: &[Sample]) -> Result<Vec<u8>, String> {
    if samples.is_empty() {
        return Err("no samples to mux".into());
    }
    if p.fps_num == 0 || p.fps_den == 0 {
        return Err("invalid frame rate".into());
    }
    let ftyp = boxed(b"ftyp", &{
        let mut v = Vec::new();
        v.extend_from_slice(b"isom");
        v.extend_from_slice(&512u32.to_be_bytes());
        for brand in [b"isom", b"iso2", b"avc1", b"mp41"] {
            v.extend_from_slice(brand);
        }
        v
    });

    let payload_len: usize = samples.iter().map(|s| s.data.len()).sum();
    let mdat_len = u32::try_from(payload_len + 8)
        .map_err(|_| "media data exceeds 4 GiB; not supported by this muxer".to_string())?;

    // moov has a fixed size regardless of the chunk offset, so build it twice.
    let moov_len = build_moov(p, samples, 0)?.len();
    let data_offset = if p.faststart {
        ftyp.len() + moov_len + 8
    } else {
        ftyp.len() + 8
    };
    let data_offset = u32::try_from(data_offset).map_err(|_| "file too large".to_string())?;
    let moov = build_moov(p, samples, data_offset)?;
    debug_assert_eq!(moov.len(), moov_len);

    let mut out = Vec::with_capacity(ftyp.len() + moov.len() + mdat_len as usize);
    out.extend_from_slice(&ftyp);
    if p.faststart {
        out.extend_from_slice(&moov);
    }
    out.extend_from_slice(&mdat_len.to_be_bytes());
    out.extend_from_slice(b"mdat");
    for s in samples {
        out.extend_from_slice(&s.data);
    }
    if !p.faststart {
        out.extend_from_slice(&moov);
    }
    Ok(out)
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
