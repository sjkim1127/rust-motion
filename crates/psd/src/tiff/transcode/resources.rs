//! Image resource blocks.

use super::*;

impl<'a> Tx<'a> {
    // ---- image resources (34377) ----

    pub(super) fn resources(&mut self) -> Result<()> {
        while self.remaining() >= 12 && is_sig(self.r.peek_rest(), self.from.is_little()) {
            self.key()?;
            let id = self.u16()?;
            self.pascal(2)?;
            let odd = self.r.clone().u32()? % 2 == 1;
            self.sized_region(false, |t| t.resource_data(id))?;
            if odd && self.remaining() > 0 {
                self.raw(1)?;
            }
        }
        self.rest()
    }

    /// Typed numeric resources; every other payload is a byte blob (ICC, XMP, EXIF, …) or
    /// unknown and is copied as stored.
    pub(super) fn resource_data(&mut self, id: u16) -> Result<()> {
        match id {
            // Resolution info: fixed 16.16 h res, u16 unit, u16 width unit, then the same for v.
            1005 => {
                self.u32()?;
                self.u16()?;
                self.u16()?;
                self.u32()?;
                self.u16()?;
                self.u16()?;
            }
            // Layer state (u16 target), layer group info (u16 per layer).
            1024 | 1026 => self.u16s_to_end()?,
            // Global angle / altitude.
            1037 | 1049 => {
                self.u32()?;
            }
            // Layer selection ids: u16 count + u32 ids.
            1069 => {
                self.u16()?;
                self.u32s_to_end()?;
            }
            // Version info: u32 version, u8 has real merged data, two Unicode strings, u32.
            1057 => {
                self.u32()?;
                self.u8()?;
                self.unicode()?;
                self.unicode()?;
                self.u32()?;
            }
            // Grid and guides: u32 version, 2 × u32 grid, u32 count, (u32 location, u8 direction)…
            1032 => {
                self.u32()?;
                self.u32()?;
                self.u32()?;
                let n = self.u32()?;
                for _ in 0..n {
                    if self.remaining() < 5 {
                        break;
                    }
                    self.u32()?;
                    self.u8()?;
                }
            }
            // Path resources: 26-byte records.
            2000..=2997 | 2999 => self.path_records()?,
            _ => {}
        }
        Ok(())
    }
}
