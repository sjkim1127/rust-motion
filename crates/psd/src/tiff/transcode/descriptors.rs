//! Descriptors, their values and the metadata-setting items.

use super::*;

impl<'a> Tx<'a> {
    // ---- descriptors ----

    pub(super) fn versioned_descriptor(&mut self) -> Result<()> {
        self.u32()?;
        self.descriptor()
    }

    pub(super) fn descriptor(&mut self) -> Result<()> {
        if self.depth > MAX_DEPTH {
            return Err(PsdError::LimitExceeded("descriptor nesting too deep"));
        }
        self.depth += 1;
        self.unicode()?;
        self.id()?;
        let n = self.u32()?;
        self.r.check_count(u64::from(n), 10)?;
        for _ in 0..n {
            self.id()?;
            self.value()?;
        }
        self.depth -= 1;
        Ok(())
    }

    pub(super) fn id(&mut self) -> Result<()> {
        let n = self.u32()?;
        if n == 0 {
            self.key()?;
        } else {
            self.raw(usize::try_from(n).map_err(|_| PsdError::LimitExceeded("id length"))?)?;
        }
        Ok(())
    }

    pub(super) fn class(&mut self) -> Result<()> {
        self.unicode()?;
        self.id()
    }

    pub(super) fn value(&mut self) -> Result<()> {
        if self.depth > MAX_DEPTH {
            return Err(PsdError::LimitExceeded("descriptor nesting too deep"));
        }
        let ty = self.key()?;
        match &ty {
            b"obj " => {
                let n = self.u32()?;
                self.r.check_count(u64::from(n), 8)?;
                for _ in 0..n {
                    self.reference_item()?;
                }
            }
            b"Objc" | b"GlbO" => self.descriptor()?,
            b"VlLs" => {
                let n = self.u32()?;
                self.r.check_count(u64::from(n), 4)?;
                self.depth += 1;
                for _ in 0..n {
                    self.value()?;
                }
                self.depth -= 1;
            }
            b"doub" => {
                self.f64()?;
            }
            b"UntF" => {
                self.key()?;
                self.f64()?;
            }
            b"UnFl" => {
                self.key()?;
                let n = self.u32()?;
                self.r.check_count(u64::from(n), 8)?;
                self.f64s(n as usize)?;
            }
            b"TEXT" => self.unicode()?,
            b"enum" => {
                self.id()?;
                self.id()?;
            }
            b"long" => {
                self.i32()?;
            }
            b"comp" => {
                self.i64()?;
            }
            b"bool" => {
                self.u8()?;
            }
            b"type" | b"GlbC" => self.class()?,
            b"alis" | b"Pth " | b"tdta" => {
                let n = self.u32()?;
                self.raw(
                    usize::try_from(n)
                        .map_err(|_| PsdError::LimitExceeded("descriptor data length"))?,
                )?;
            }
            b"ObAr" => {
                self.u32()?;
                self.descriptor()?;
            }
            other => {
                return Err(PsdError::Unsupported(format!(
                    "descriptor OSType {:?}",
                    String::from_utf8_lossy(other)
                )))
            }
        }
        Ok(())
    }

    pub(super) fn reference_item(&mut self) -> Result<()> {
        let ty = self.key()?;
        match &ty {
            b"prop" => {
                self.class()?;
                self.id()?;
            }
            b"Clss" => self.class()?,
            b"Enmr" => {
                self.class()?;
                self.id()?;
                self.id()?;
            }
            b"rele" => {
                self.class()?;
                self.i32()?;
            }
            b"Idnt" | b"indx" => {
                self.u32()?;
            }
            b"name" => {
                self.class()?;
                self.unicode()?;
            }
            other => {
                return Err(PsdError::Unsupported(format!(
                    "reference item type {:?}",
                    String::from_utf8_lossy(other)
                )))
            }
        }
        Ok(())
    }

    // ---- other block layouts ----

    /// `shmd`: u32 count, then per item signature, key, copy flag, 3 bytes, u32 length, data.
    /// An item whose layout is unknown is dropped on its own (and the count patched) rather
    /// than taking the whole block with it.
    pub(super) fn metadata_setting(&mut self) -> Result<()> {
        let n = self.r.u32()?;
        self.r.check_count(u64::from(n), 16)?;
        let count_at = self.out.len();
        self.out.extend_from_slice(&[0; 4]);
        let mut kept = 0u32;
        for _ in 0..n {
            let before = self.out.len();
            self.key()?;
            let key = self.key()?;
            self.u8()?;
            self.raw(3)?;
            let same = self.same_order();
            let res = self.sized_region(false, |t| match &key {
                b"cmls" | b"mlst" | b"cust" | b"tmln" | b"sgrp" | b"cinf" => {
                    t.versioned_descriptor()
                }
                // mdyn (dynamic metadata) is one u32.
                b"mdyn" => t.u32s_to_end(),
                _ if same => Ok(()),
                _ if t.descriptor_shaped() => Ok(()),
                _ => Err(PsdError::Unsupported(format!(
                    "metadata item {:?}",
                    String::from_utf8_lossy(&key)
                ))),
            });
            match res {
                Ok(()) => kept += 1,
                Err(e) if same => return Err(e),
                Err(e) => {
                    self.out.truncate(before);
                    self.warnings.push(format!(
                        "shmd item {} dropped: {e}",
                        String::from_utf8_lossy(&key)
                    ));
                }
            }
        }
        self.patch_len(count_at, false, u64::from(kept))
    }
}
