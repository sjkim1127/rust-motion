//! Block bodies with their own layout: linked layers, annotations, paths, patterns and filter effects.

use super::*;

impl<'a> Tx<'a> {
    /// `lnk2` / `lnk3` / `lnkD` / `lnkE` (linked and embedded smart-object files): items of
    /// u64 length + data, each padded to 4. Item: type (`liFD` embedded, `liFE` external,
    /// `liFA` alias), version 1–7, Pascal uuid, Unicode name, file type and creator keys, u64
    /// data length, open-descriptor flag (+ descriptor), per-type fields, the file bytes for
    /// `liFD`, then the fields later versions added.
    pub(super) fn linked_layers(&mut self) -> Result<()> {
        while self.remaining() >= 8 {
            let len = self.r.clone().u64()?;
            self.sized_region(true, |t| {
                let kind = t.key()?;
                let version = t.u32()?;
                t.pascal(1)?;
                t.unicode()?;
                t.key()?;
                t.key()?;
                let data_len = t.u64()?;
                if t.u8()? != 0 {
                    t.versioned_descriptor()?;
                }
                if &kind == b"liFE" && version > 3 {
                    t.i32()?;
                    for _ in 0..4 {
                        t.u8()?;
                    }
                    t.f64()?;
                }
                if &kind == b"liFE" {
                    t.u64()?;
                }
                if &kind == b"liFA" {
                    t.raw(8)?;
                }
                let data_len = usize::try_from(data_len)
                    .map_err(|_| PsdError::LimitExceeded("linked file length"))?;
                if &kind == b"liFD" {
                    t.raw(data_len)?;
                }
                if version >= 5 {
                    t.unicode()?;
                }
                if version >= 6 {
                    t.f64()?;
                }
                if version >= 7 {
                    t.u8()?;
                }
                if &kind == b"liFE" && version == 2 {
                    t.raw(data_len)?;
                }
                Ok(())
            })?;
            let pad = ((4 - len % 4) % 4) as usize;
            self.raw(pad.min(self.remaining()))?;
        }
        Ok(())
    }

    /// `Anno`: u16 major, u16 minor, u32 count, then per annotation a self-counting u32
    /// length, type key, open and flags bytes, u16 optional blocks, icon and popup rects, a
    /// colour, three even-padded Pascal strings, and a self-counting data block (key, u32
    /// size, text with its own byte-order mark).
    pub(super) fn annotations(&mut self) -> Result<()> {
        self.u16()?;
        self.u16()?;
        let n = self.u32()?;
        for _ in 0..n {
            if self.remaining() < 4 {
                break;
            }
            self.sized_region_inclusive(|t| {
                t.key()?;
                t.u8()?;
                t.u8()?;
                t.u16()?;
                t.i32s(8)?;
                t.u16s(5)?;
                t.pascal(2)?;
                t.pascal(2)?;
                t.pascal(2)?;
                t.sized_region_inclusive(|d| {
                    d.key()?;
                    d.u32()?;
                    Ok(())
                })
            })?;
        }
        Ok(())
    }

    /// 26-byte path records: a u16 selector then, for knots, six fixed-point coordinates.
    pub(super) fn path_records(&mut self) -> Result<()> {
        while self.remaining() >= 26 {
            let selector = self.u16()?;
            match selector {
                1 | 2 | 4 | 5 => self.i32s(6)?,
                // Length records: knot count, path operation, then fields Photoshop writes as
                // 16-bit values (zero in practice).
                0 | 3 => self.u16s(12)?,
                // Clipboard: four fixed-point coordinates and a resolution.
                7 => {
                    self.i32s(5)?;
                    self.raw(4)?;
                }
                8 => {
                    self.u16()?;
                    self.raw(22)?;
                }
                _ => self.raw(24)?,
            }
        }
        Ok(())
    }

    /// One pattern of a `Patt` block (the data after its u32 length).
    pub(super) fn pattern(t: &mut Tx<'_>) -> Result<()> {
        t.u32()?;
        let mode = t.u32()?;
        t.u16()?;
        t.u16()?;
        t.unicode()?;
        t.pascal(1)?;
        if mode == 2 {
            t.raw(768)?;
            // Some writers follow the palette with 4 extra bytes before the array list version.
            let next_is_version = t.r.clone().u32().is_ok_and(|v| v == 3);
            if !next_is_version && t.remaining() >= 4 {
                t.raw(4)?;
            }
        }
        t.u32()?;
        t.sized_region(false, |v| {
            v.i32s(4)?;
            let count = v.u32()?;
            for _ in 0..count.saturating_add(2) {
                if v.remaining() < 4 {
                    break;
                }
                if v.u32()? == 0 {
                    continue;
                }
                if v.r.clone().u32()? == 0 {
                    v.u32()?;
                    continue;
                }
                v.sized_region(false, |c| {
                    c.u32()?;
                    let ct = c.i32()?;
                    c.i32()?;
                    let cb = c.i32()?;
                    c.i32()?;
                    c.u16()?;
                    let comp = c.u8()?;
                    if comp == 1 {
                        let saved = c.version;
                        c.version = Version::Psd;
                        let res = c.rle_counts(height(ct, cb));
                        c.version = saved;
                        res?;
                    }
                    Ok(())
                })?;
            }
            Ok(())
        })
    }

    /// `FEid` / `FXid`: version, u64 length, items with per-channel planes (4-byte RLE counts).
    pub(super) fn filter_effects(&mut self) -> Result<()> {
        self.u32()?;
        self.sized_region(true, |t| {
            while !t.r.is_empty() {
                t.pascal(1)?;
                t.u32()?;
                t.sized_region(true, |it| {
                    let top = it.i32()?;
                    it.i32()?;
                    let bottom = it.i32()?;
                    it.i32()?;
                    let rect_h = height(top, bottom);
                    it.u32()?;
                    let max = it.u32()?;
                    if max > crate::filter_effects::MAX_SLOTS {
                        return Err(PsdError::LimitExceeded("filter effects: too many channels"));
                    }
                    for _ in 0..max + 2 {
                        if it.u32()? != 0 {
                            it.effects_plane(rect_h)?;
                        }
                    }
                    Ok(())
                })?;
                if !t.r.is_empty() && t.u8()? != 0 {
                    let top = t.i32()?;
                    t.i32()?;
                    let bottom = t.i32()?;
                    t.i32()?;
                    t.effects_plane(height(top, bottom))?;
                }
            }
            Ok(())
        })
    }

    pub(super) fn effects_plane(&mut self, rows: usize) -> Result<()> {
        self.sized_region(true, |p| p.channel_data(rows, true))
    }
}
