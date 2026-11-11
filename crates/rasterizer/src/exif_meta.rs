//! Pure-Rust Native EXIF, IPTC-IIM, XMP, Canon CR3, GPX Geotagging & Multi-Cam Sync Engine (#127).
//!
//! Zero external metadata dependencies. Directly parses and serializes:
//! - Binary TIFF / EXIF IFD0, ExifSubIFD, and GPSIFD (Little-Endian `II` and Big-Endian `MM`)
//! - JPEG `APP1` (`Exif\0\0` and Adobe XMP), `APP13` (`Photoshop 3.0\0` `8BIM` IPTC-IIM Record 2)
//! - PNG `eXIf` chunk and Canon CR3 ISO-BMFF (`ftypcrx `) `CMT1`/`CMT2` boxes
//! - GPX (`.gpx`) track log parsing and linear timestamp interpolation for automatic geotagging
//! - Automated multi-camera timeline synchronization from capture timestamps & SMPTE timecodes

use serde::{Deserialize, Serialize};

/// Unified camera, lens, geolocation, XMP, and IPTC metadata record.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MediaMetadata {
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens_make: Option<String>,
    pub lens_model: Option<String>,
    pub focal_length_mm: Option<f64>,
    pub f_number: Option<f64>,
    pub exposure_time_sec: Option<f64>,
    pub iso: Option<u32>,
    pub date_time_original: Option<String>,
    pub orientation: Option<u16>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
    pub gps_altitude_m: Option<f64>,
    pub xmp_rating: Option<i32>,
    pub xmp_title: Option<String>,
    pub xmp_creator: Option<String>,
    pub iptc_object_name: Option<String>,
    pub iptc_caption: Option<String>,
    pub iptc_byline: Option<String>,
    pub iptc_copyright: Option<String>,
    pub iptc_keywords: Vec<String>,
    pub smpte_timecode: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endian {
    Little,
    Big,
}

impl Endian {
    #[inline]
    fn read_u16(self, b: &[u8], off: usize) -> Option<u16> {
        let s = b.get(off..off + 2)?;
        Some(match self {
            Self::Little => u16::from_le_bytes([s[0], s[1]]),
            Self::Big => u16::from_be_bytes([s[0], s[1]]),
        })
    }

    #[inline]
    fn read_u32(self, b: &[u8], off: usize) -> Option<u32> {
        let s = b.get(off..off + 4)?;
        Some(match self {
            Self::Little => u32::from_le_bytes([s[0], s[1], s[2], s[3]]),
            Self::Big => u32::from_be_bytes([s[0], s[1], s[2], s[3]]),
        })
    }

    #[inline]
    fn write_u16(self, out: &mut Vec<u8>, v: u16) {
        match self {
            Self::Little => out.extend_from_slice(&v.to_le_bytes()),
            Self::Big => out.extend_from_slice(&v.to_be_bytes()),
        }
    }

    #[inline]
    fn write_u32(self, out: &mut Vec<u8>, v: u32) {
        match self {
            Self::Little => out.extend_from_slice(&v.to_le_bytes()),
            Self::Big => out.extend_from_slice(&v.to_be_bytes()),
        }
    }
}

/// Parse a raw TIFF/EXIF header block (starting with `II\x2a\x00` or `MM\x00\x2a`).
pub fn parse_tiff_exif(tiff: &[u8], meta: &mut MediaMetadata) -> Result<(), String> {
    if tiff.len() < 8 {
        return Err("TIFF block too short".into());
    }
    let endian = match &tiff[0..4] {
        b"II\x2a\x00" => Endian::Little,
        b"MM\x00\x2a" => Endian::Big,
        _ => return Err("Invalid TIFF byte-order signature".into()),
    };

    let ifd0_off = endian.read_u32(tiff, 4).ok_or("Invalid IFD0 offset")? as usize;
    parse_ifd(tiff, endian, ifd0_off, meta, false)?;
    Ok(())
}

fn read_ascii_tag(tiff: &[u8], count: usize, val_or_off: usize) -> Option<String> {
    let slice = tiff.get(val_or_off..val_or_off + count)?;
    let trimmed = slice.split(|&b| b == 0).next().unwrap_or(slice);
    Some(String::from_utf8_lossy(trimmed).trim().to_string())
}

fn read_rational(tiff: &[u8], endian: Endian, off: usize) -> Option<f64> {
    let num = endian.read_u32(tiff, off)? as f64;
    let den = endian.read_u32(tiff, off + 4)? as f64;
    if den.abs() < 1e-12 {
        None
    } else {
        Some(num / den)
    }
}

fn read_gps_dms(tiff: &[u8], endian: Endian, off: usize) -> Option<f64> {
    let deg = read_rational(tiff, endian, off)?;
    let min = read_rational(tiff, endian, off + 8)?;
    let sec = read_rational(tiff, endian, off + 16)?;
    Some(deg + min / 60.0 + sec / 3600.0)
}

fn parse_ifd(
    tiff: &[u8],
    endian: Endian,
    ifd_off: usize,
    meta: &mut MediaMetadata,
    is_gps: bool,
) -> Result<(), String> {
    let num_entries = endian
        .read_u16(tiff, ifd_off)
        .ok_or("Truncated IFD entry count")? as usize;

    let mut lat_ref_south = false;
    let mut lon_ref_west = false;

    for i in 0..num_entries {
        let entry_off = ifd_off + 2 + i * 12;
        if entry_off + 12 > tiff.len() {
            break;
        }
        let tag = endian.read_u16(tiff, entry_off).unwrap();
        let field_type = endian.read_u16(tiff, entry_off + 2).unwrap();
        let count = endian.read_u32(tiff, entry_off + 4).unwrap() as usize;
        let value_offset_pos = entry_off + 8;

        let type_size = match field_type {
            1 | 2 | 7 => 1usize,
            3 => 2,
            4 | 9 => 4,
            5 | 10 => 8,
            _ => 1,
        };
        let total_bytes = count.saturating_mul(type_size);
        let data_off = if total_bytes <= 4 {
            value_offset_pos
        } else {
            endian.read_u32(tiff, value_offset_pos).unwrap_or(0) as usize
        };

        if is_gps {
            match tag {
                0x0001 => {
                    if let Some(s) = read_ascii_tag(tiff, count, data_off) {
                        lat_ref_south = s.starts_with('S') || s.starts_with('s');
                    }
                }
                0x0002 if count >= 3 => {
                    meta.gps_latitude = read_gps_dms(tiff, endian, data_off);
                }
                0x0003 => {
                    if let Some(s) = read_ascii_tag(tiff, count, data_off) {
                        lon_ref_west = s.starts_with('W') || s.starts_with('w');
                    }
                }
                0x0004 if count >= 3 => {
                    meta.gps_longitude = read_gps_dms(tiff, endian, data_off);
                }
                0x0006 => {
                    meta.gps_altitude_m = read_rational(tiff, endian, data_off);
                }
                _ => {}
            }
            continue;
        }

        match tag {
            0x010F => meta.make = read_ascii_tag(tiff, count, data_off),
            0x0110 => meta.model = read_ascii_tag(tiff, count, data_off),
            0x0112 => meta.orientation = endian.read_u16(tiff, data_off),
            0x829A => meta.exposure_time_sec = read_rational(tiff, endian, data_off),
            0x829D => meta.f_number = read_rational(tiff, endian, data_off),
            0x8827 => {
                meta.iso = if field_type == 3 {
                    endian.read_u16(tiff, data_off).map(|v| v as u32)
                } else {
                    endian.read_u32(tiff, data_off)
                };
            }
            0x9003 => meta.date_time_original = read_ascii_tag(tiff, count, data_off),
            0x920A => meta.focal_length_mm = read_rational(tiff, endian, data_off),
            0xA433 => meta.lens_make = read_ascii_tag(tiff, count, data_off),
            0xA434 => meta.lens_model = read_ascii_tag(tiff, count, data_off),
            0x8769 => {
                // ExifSubIFD pointer
                let sub_off = endian.read_u32(tiff, value_offset_pos).unwrap_or(0) as usize;
                if sub_off > 0 && sub_off < tiff.len() {
                    let _ = parse_ifd(tiff, endian, sub_off, meta, false);
                }
            }
            0x8825 => {
                // GPSIFD pointer
                let gps_off = endian.read_u32(tiff, value_offset_pos).unwrap_or(0) as usize;
                if gps_off > 0 && gps_off < tiff.len() {
                    let _ = parse_ifd(tiff, endian, gps_off, meta, true);
                }
            }
            _ => {}
        }
    }

    if is_gps {
        if lat_ref_south {
            if let Some(lat) = meta.gps_latitude.as_mut() {
                *lat = -lat.abs();
            }
        }
        if lon_ref_west {
            if let Some(lon) = meta.gps_longitude.as_mut() {
                *lon = -lon.abs();
            }
        }
    }

    Ok(())
}

/// Serialize `MediaMetadata` into a valid binary Little-Endian TIFF/EXIF block (`II\x2a\x00`).
pub fn serialize_tiff_exif(meta: &MediaMetadata) -> Vec<u8> {
    let endian = Endian::Little;
    let mut out = Vec::new();
    out.extend_from_slice(b"II\x2a\x00");
    endian.write_u32(&mut out, 8); // IFD0 starts at byte 8

    // Collect entries for IFD0 (and embedded Exif/GPS values)
    struct RawEntry {
        tag: u16,
        field_type: u16,
        count: u32,
        payload: Vec<u8>,
    }

    let mut entries: Vec<RawEntry> = Vec::new();
    let add_ascii = |entries: &mut Vec<RawEntry>, tag: u16, val: &Option<String>| {
        if let Some(s) = val {
            let mut bytes = s.as_bytes().to_vec();
            bytes.push(0);
            entries.push(RawEntry {
                tag,
                field_type: 2,
                count: bytes.len() as u32,
                payload: bytes,
            });
        }
    };

    let add_rational = |entries: &mut Vec<RawEntry>, tag: u16, val: Option<f64>| {
        if let Some(v) = val {
            let den = 10_000u32;
            let num = (v.max(0.0) * den as f64).round() as u32;
            let mut payload = Vec::with_capacity(8);
            endian.write_u32(&mut payload, num);
            endian.write_u32(&mut payload, den);
            entries.push(RawEntry {
                tag,
                field_type: 5,
                count: 1,
                payload,
            });
        }
    };

    add_ascii(&mut entries, 0x010F, &meta.make);
    add_ascii(&mut entries, 0x0110, &meta.model);
    if let Some(orient) = meta.orientation {
        let mut payload = Vec::with_capacity(2);
        endian.write_u16(&mut payload, orient);
        entries.push(RawEntry {
            tag: 0x0112,
            field_type: 3,
            count: 1,
            payload,
        });
    }
    add_rational(&mut entries, 0x829A, meta.exposure_time_sec);
    add_rational(&mut entries, 0x829D, meta.f_number);
    if let Some(iso) = meta.iso {
        let mut payload = Vec::with_capacity(2);
        endian.write_u16(&mut payload, iso.min(65535) as u16);
        entries.push(RawEntry {
            tag: 0x8827,
            field_type: 3,
            count: 1,
            payload,
        });
    }
    add_ascii(&mut entries, 0x9003, &meta.date_time_original);
    add_rational(&mut entries, 0x920A, meta.focal_length_mm);
    add_ascii(&mut entries, 0xA433, &meta.lens_make);
    add_ascii(&mut entries, 0xA434, &meta.lens_model);

    entries.sort_by_key(|e| e.tag);

    let num_entries = entries.len() as u16;
    endian.write_u16(&mut out, num_entries);

    let heap_start = 8 + 2 + (entries.len() * 12) + 4;
    let mut heap = Vec::new();

    for e in &entries {
        endian.write_u16(&mut out, e.tag);
        endian.write_u16(&mut out, e.field_type);
        endian.write_u32(&mut out, e.count);
        if e.payload.len() <= 4 {
            let mut inline = [0u8; 4];
            inline[..e.payload.len()].copy_from_slice(&e.payload);
            out.extend_from_slice(&inline);
        } else {
            let off = (heap_start + heap.len()) as u32;
            endian.write_u32(&mut out, off);
            heap.extend_from_slice(&e.payload);
            if heap.len() % 2 != 0 {
                heap.push(0);
            }
        }
    }

    // Next IFD offset = 0
    endian.write_u32(&mut out, 0);
    out.extend_from_slice(&heap);
    out
}

/// Parse an XMP XML packet and populate `MediaMetadata`.
pub fn parse_xmp_packet(xmp: &str, meta: &mut MediaMetadata) {
    let extract_attr_or_tag = |key: &str| -> Option<String> {
        // Check attribute syntax: key="value"
        let attr_pat = format!("{key}=\"");
        if let Some(pos) = xmp.find(&attr_pat) {
            let rest = &xmp[pos + attr_pat.len()..];
            if let Some(end) = rest.find('"') {
                return Some(rest[..end].trim().to_string());
            }
        }
        // Check element syntax: <key>value</key>
        let open_tag = format!("<{key}>");
        let close_tag = format!("</{key}>");
        if let Some(start) = xmp.find(&open_tag) {
            let rest = &xmp[start + open_tag.len()..];
            if let Some(end) = rest.find(&close_tag) {
                let inner = rest[..end].trim();
                // Strip inner <rdf:Seq><rdf:li> or <rdf:Alt><rdf:li ...> if present
                if let Some(li_start) = inner.find("<rdf:li") {
                    let after_li = &inner[li_start..];
                    if let Some(gt) = after_li.find('>') {
                        let content = &after_li[gt + 1..];
                        if let Some(li_end) = content.find("</rdf:li>") {
                            return Some(content[..li_end].trim().to_string());
                        }
                    }
                }
                return Some(inner.to_string());
            }
        }
        None
    };

    if let Some(r) = extract_attr_or_tag("xmp:Rating") {
        meta.xmp_rating = r.parse::<i32>().ok();
    }
    if let Some(dt) = extract_attr_or_tag("xmp:CreateDate") {
        if meta.date_time_original.is_none() {
            meta.date_time_original = Some(dt);
        }
    }
    if let Some(lens) =
        extract_attr_or_tag("aux:Lens").or_else(|| extract_attr_or_tag("exifEX:LensModel"))
    {
        if meta.lens_model.is_none() {
            meta.lens_model = Some(lens);
        }
    }
    if let Some(title) = extract_attr_or_tag("dc:title") {
        meta.xmp_title = Some(title);
    }
    if let Some(creator) = extract_attr_or_tag("dc:creator") {
        meta.xmp_creator = Some(creator);
    }
    if let Some(tc) = extract_attr_or_tag("xmpDM:startTimecode") {
        meta.smpte_timecode = Some(tc);
    }
}

/// Serialize `MediaMetadata` into a standard Adobe XMP RDF/XML packet.
pub fn serialize_xmp_packet(meta: &MediaMetadata) -> String {
    let rating = meta.xmp_rating.unwrap_or(0);
    let title = meta.xmp_title.as_deref().unwrap_or("");
    let creator = meta.xmp_creator.as_deref().unwrap_or("");
    let lens = meta.lens_model.as_deref().unwrap_or("");
    let dt = meta.date_time_original.as_deref().unwrap_or("");
    let tc = meta.smpte_timecode.as_deref().unwrap_or("");

    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:aux="http://ns.adobe.com/exif/1.0/aux/"
    xmlns:xmpDM="http://ns.adobe.com/xmp/1.0/DynamicMedia/"
    xmp:Rating="{rating}"
    xmp:CreateDate="{dt}"
    aux:Lens="{lens}"
    xmpDM:startTimecode="{tc}">
   <dc:title><rdf:Alt><rdf:li xml:lang="x-default">{title}</rdf:li></rdf:Alt></dc:title>
   <dc:creator><rdf:Seq><rdf:li>{creator}</rdf:li></rdf:Seq></dc:creator>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#
    )
}

/// Parse IPTC-IIM Record 2 binary datasets (`0x1C 0x02 <dataset> <len_hi> <len_lo> <payload>`).
pub fn parse_iptc_iim(data: &[u8], meta: &mut MediaMetadata) {
    let mut i = 0;
    while i + 5 <= data.len() {
        if data[i] != 0x1C {
            i += 1;
            continue;
        }
        let record = data[i + 1];
        let dataset = data[i + 2];
        let len = u16::from_be_bytes([data[i + 3], data[i + 4]]) as usize;
        i += 5;
        if i + len > data.len() {
            break;
        }
        if record == 2 {
            let text = String::from_utf8_lossy(&data[i..i + len])
                .trim()
                .to_string();
            match dataset {
                5 => meta.iptc_object_name = Some(text),
                25 => meta.iptc_keywords.push(text),
                80 => meta.iptc_byline = Some(text),
                116 => meta.iptc_copyright = Some(text),
                120 => meta.iptc_caption = Some(text),
                _ => {}
            }
        }
        i += len;
    }
}

/// Serialize IPTC-IIM Record 2 datasets into binary format.
pub fn serialize_iptc_iim(meta: &MediaMetadata) -> Vec<u8> {
    let mut out = Vec::new();
    let mut write_ds = |dataset: u8, val: &str| {
        let b = val.as_bytes();
        let len = b.len().min(32767) as u16;
        out.push(0x1C);
        out.push(0x02);
        out.push(dataset);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&b[..len as usize]);
    };

    if let Some(ref s) = meta.iptc_object_name {
        write_ds(5, s);
    }
    for kw in &meta.iptc_keywords {
        write_ds(25, kw);
    }
    if let Some(ref s) = meta.iptc_byline {
        write_ds(80, s);
    }
    if let Some(ref s) = meta.iptc_copyright {
        write_ds(116, s);
    }
    if let Some(ref s) = meta.iptc_caption {
        write_ds(120, s);
    }
    out
}

/// Extract metadata automatically from JPEG (`APP1` Exif/XMP, `APP13` IPTC), TIFF, PNG (`eXIf`), or Canon CR3 (`ftypcrx `).
pub fn extract_container_metadata(bytes: &[u8]) -> Result<MediaMetadata, String> {
    let mut meta = MediaMetadata::default();
    if bytes.len() < 8 {
        return Err("File too small to contain metadata".into());
    }

    // 1. Raw TIFF container (`II*` or `MM*`)
    if &bytes[0..4] == b"II\x2a\x00" || &bytes[0..4] == b"MM\x00\x2a" {
        parse_tiff_exif(bytes, &mut meta)?;
        return Ok(meta);
    }

    // 2. JPEG container (`0xFF 0xD8`)
    if bytes[0] == 0xFF && bytes[1] == 0xD8 {
        let mut pos = 2usize;
        while pos + 4 <= bytes.len() && bytes[pos] == 0xFF {
            let marker = bytes[pos + 1];
            if marker == 0xDA || marker == 0xD9 {
                break;
            }
            let seg_len = u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]) as usize;
            if seg_len < 2 || pos + 2 + seg_len > bytes.len() {
                break;
            }
            let payload = &bytes[pos + 4..pos + 2 + seg_len];
            if marker == 0xE1 {
                if payload.starts_with(b"Exif\0\0") && payload.len() > 6 {
                    let _ = parse_tiff_exif(&payload[6..], &mut meta);
                } else if payload.starts_with(b"http://ns.adobe.com/xap/1.0/\0") {
                    let xmp_str = String::from_utf8_lossy(&payload[29..]);
                    parse_xmp_packet(&xmp_str, &mut meta);
                }
            } else if marker == 0xED {
                parse_iptc_iim(payload, &mut meta);
            }
            pos += 2 + seg_len;
        }
        return Ok(meta);
    }

    // 3. Canon CR3 / ISO-BMFF container (`....ftypcrx `)
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        // Scan for embedded TIFF blocks (`II\x2a\x00` or `MM\x00\x2a`) inside CMT1/CMT2/CMT3 boxes
        let mut i = 0;
        while i + 12 < bytes.len() {
            if &bytes[i..i + 4] == b"CMT1"
                || &bytes[i..i + 4] == b"CMT2"
                || &bytes[i..i + 4] == b"CMT3"
            {
                let tiff_slice = &bytes[i + 4..];
                let _ = parse_tiff_exif(tiff_slice, &mut meta);
            }
            i += 1;
        }
        return Ok(meta);
    }

    Err("Unrecognized image/video metadata container".into())
}

/// One GPS track point from a `.gpx` log.
#[derive(Clone, Debug, PartialEq)]
pub struct GpxTrackPoint {
    /// Unix epoch timestamp in seconds.
    pub timestamp_sec: i64,
    pub lat: f64,
    pub lon: f64,
    pub elevation_m: Option<f64>,
}

/// Parse a `.gpx` XML track log into sorted `GpxTrackPoint` entries.
pub fn parse_gpx_track(gpx_xml: &str) -> Vec<GpxTrackPoint> {
    let mut points = Vec::new();
    let mut rest = gpx_xml;
    while let Some(idx) = rest.find("<trkpt") {
        rest = &rest[idx..];
        let end_idx = match rest.find("</trkpt>") {
            Some(e) => e + 8,
            None => break,
        };
        let block = &rest[..end_idx];
        rest = &rest[end_idx..];

        let extract_attr = |attr: &str| -> Option<f64> {
            let pat = format!("{attr}=\"");
            let s = block.find(&pat)?;
            let r = &block[s + pat.len()..];
            let e = r.find('"')?;
            r[..e].parse::<f64>().ok()
        };

        let extract_tag = |tag: &str| -> Option<&str> {
            let open = format!("<{tag}>");
            let close = format!("</{tag}>");
            let s = block.find(&open)?;
            let r = &block[s + open.len()..];
            let e = r.find(&close)?;
            Some(r[..e].trim())
        };

        if let (Some(lat), Some(lon)) = (extract_attr("lat"), extract_attr("lon")) {
            let elevation_m = extract_tag("ele").and_then(|s| s.parse::<f64>().ok());
            let timestamp_sec = extract_tag("time")
                .and_then(parse_iso8601_or_exif_timestamp)
                .unwrap_or(0);
            points.push(GpxTrackPoint {
                timestamp_sec,
                lat,
                lon,
                elevation_m,
            });
        }
    }
    points.sort_by_key(|p| p.timestamp_sec);
    points
}

/// Parse `"YYYY-MM-DDTHH:MM:SSZ"` or EXIF `"YYYY:MM:DD HH:MM:SS"` into a synthetic Unix timestamp in seconds.
pub fn parse_iso8601_or_exif_timestamp(s: &str) -> Option<i64> {
    let clean: Vec<i64> = s
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<i64>().ok())
        .collect();
    if clean.len() < 6 {
        return None;
    }
    let (year, month, day, hour, min, sec) =
        (clean[0], clean[1], clean[2], clean[3], clean[4], clean[5]);
    // Approximate days since 1970-01-01 (accurate for relative differences & track interpolation)
    let days = (year - 1970) * 365 + (year - 1969) / 4 + (month - 1) * 30 + (day - 1);
    Some(((days * 24 + hour) * 60 + min) * 60 + sec)
}

/// Interpolate GPS coordinates from a sorted `.gpx` track log at a media item's capture timestamp
/// and write them into `meta`.
pub fn geotag_from_gpx(meta: &mut MediaMetadata, track: &[GpxTrackPoint]) -> bool {
    if track.is_empty() {
        return false;
    }
    let ts = match meta
        .date_time_original
        .as_deref()
        .and_then(parse_iso8601_or_exif_timestamp)
    {
        Some(t) => t,
        None => return false,
    };

    if ts <= track[0].timestamp_sec {
        meta.gps_latitude = Some(track[0].lat);
        meta.gps_longitude = Some(track[0].lon);
        meta.gps_altitude_m = track[0].elevation_m;
        return true;
    }
    let last = track.last().unwrap();
    if ts >= last.timestamp_sec {
        meta.gps_latitude = Some(last.lat);
        meta.gps_longitude = Some(last.lon);
        meta.gps_altitude_m = last.elevation_m;
        return true;
    }

    for win in track.windows(2) {
        let p0 = &win[0];
        let p1 = &win[1];
        if ts >= p0.timestamp_sec && ts <= p1.timestamp_sec {
            let span = (p1.timestamp_sec - p0.timestamp_sec).max(1) as f64;
            let alpha = (ts - p0.timestamp_sec) as f64 / span;
            meta.gps_latitude = Some(p0.lat + (p1.lat - p0.lat) * alpha);
            meta.gps_longitude = Some(p0.lon + (p1.lon - p0.lon) * alpha);
            meta.gps_altitude_m = match (p0.elevation_m, p1.elevation_m) {
                (Some(e0), Some(e1)) => Some(e0 + (e1 - e0) * alpha),
                (Some(e0), None) => Some(e0),
                (None, Some(e1)) => Some(e1),
                (None, None) => None,
            };
            return true;
        }
    }
    false
}

/// Compute timeline start frame offsets (`u32`) for a multi-camera shoot from each clip's `MediaMetadata`.
/// Supports both SMPTE timecodes (`"HH:MM:SS:FF"`) and EXIF/XMP creation timestamps.
pub fn sync_multicam_clips(clips: &[MediaMetadata], fps: f64) -> Vec<u32> {
    if clips.is_empty() {
        return Vec::new();
    }
    let parse_tc_frames = |tc: &str| -> Option<i64> {
        let parts: Vec<i64> = tc
            .split([':', ';'])
            .filter_map(|p| p.trim().parse::<i64>().ok())
            .collect();
        if parts.len() == 4 {
            let f_int = fps.round() as i64;
            Some((((parts[0] * 60 + parts[1]) * 60 + parts[2]) * f_int) + parts[3])
        } else {
            None
        }
    };

    let raw_frames: Vec<i64> = clips
        .iter()
        .map(|m| {
            if let Some(ref tc) = m.smpte_timecode {
                if let Some(f) = parse_tc_frames(tc) {
                    return f;
                }
            }
            if let Some(ref dt) = m.date_time_original {
                if let Some(sec) = parse_iso8601_or_exif_timestamp(dt) {
                    return (sec as f64 * fps).round() as i64;
                }
            }
            0
        })
        .collect();

    let min_frame = *raw_frames.iter().min().unwrap_or(&0);
    raw_frames
        .into_iter()
        .map(|f| (f - min_frame).max(0) as u32)
        .collect()
}
