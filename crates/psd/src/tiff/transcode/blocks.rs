//! Tagged blocks: the block walker and the per-key block bodies.

use super::*;

impl<'a> Tx<'a> {
    // ---- tagged blocks ----

    /// Tagged blocks to the end; bytes that do not form a block are copied.
    pub(super) fn blocks(&mut self) -> Result<()> {
        loop {
            let rest = self.r.peek_rest();
            if rest.len() < 12 || !is_sig(rest, self.from.is_little()) {
                return self.rest();
            }
            self.block()?;
        }
    }

    pub(super) fn block(&mut self) -> Result<()> {
        let before = self.out.len();
        // A `MIB8` / `46B8` marker in big-endian data: a block kept verbatim from a
        // little-endian file. Written back as it was into little-endian output (its `8BIM`
        // signature reversed, as every signature there), copied as is otherwise.
        if !self.from.is_little() && TaggedBlock::is_foreign_order_signature(self.r.peek_rest()) {
            let sig = self.r.array::<4>()?;
            let mut key = self.r.array::<4>()?;
            let long = uses_long_length(self.version, &{
                let mut k = key;
                if self.to.is_little() {
                    k.reverse();
                }
                k
            });
            let len = self.r.len_field(long)?;
            let data = self.r.bytes_u64(len)?;
            if self.to.is_little() {
                key.reverse();
            }
            self.out.extend_from_slice(&sig);
            self.out.extend_from_slice(&key);
            let n = self.out.len();
            self.out
                .extend_from_slice(if long { &[0u8; 8] } else { &[0u8; 4] });
            self.patch_len(n, long, len)?;
            self.out.extend_from_slice(data);
            let rest = self.r.peek_rest();
            let pad = (0..=3usize)
                .find(|&k| {
                    k == rest.len() || (k < rest.len() && is_sig(&rest[k..], self.from.is_little()))
                })
                .unwrap_or(0);
            let pad_bytes = self.r.bytes(pad)?;
            self.out.extend_from_slice(pad_bytes);
            return Ok(());
        }
        let sig_at = self.out.len();
        let sig = self.key()?;
        let key = self.key()?;
        let long = uses_long_length(self.version, &key);
        let len = self.r.len_field(long)?;
        let len_at = self.out.len();
        self.out
            .extend_from_slice(if long { &[0u8; 8] } else { &[0u8; 4] });
        let data = self.r.bytes_u64(len)?;
        // The body is transcoded straight into this output, after the length placeholder.
        let data_at = self.out.len();
        let mut child = self.nested(data, self.depth + 1);
        let handled = if child.depth > MAX_DEPTH {
            Err(PsdError::LimitExceeded("tagged block nesting too deep"))
        } else if data.is_empty() {
            Ok(true)
        } else {
            child.block_body(&key)
        };
        let rest = if matches!(handled, Ok(true)) {
            child.rest()
        } else {
            Ok(())
        };
        self.adopt(&mut child);
        rest?;
        if !matches!(handled, Ok(true)) {
            // A body that was not transcoded leaves nothing: it is copied as stored or dropped.
            self.out.truncate(data_at);
        }
        // Padding: the smallest k in 0..=3 after which the next block (or the end) starts.
        let rest = self.r.peek_rest();
        let pad = (0..=3usize)
            .find(|&k| {
                k == rest.len() || (k < rest.len() && is_sig(&rest[k..], self.from.is_little()))
            })
            .unwrap_or(0);
        let pad_bytes = self.r.bytes(pad)?;
        match handled {
            Ok(true) => {
                self.warnings.append(&mut child.warnings);
                self.patch_len(len_at, long, (self.out.len() - data_at) as u64)?;
                self.out.extend_from_slice(pad_bytes);
            }
            Ok(false) if self.same_order() => {
                self.patch_len(len_at, long, data.len() as u64)?;
                self.out.extend_from_slice(data);
                self.out.extend_from_slice(pad_bytes);
            }
            // Little-endian in, big-endian model: keep the bytes as they are, marked.
            Ok(false) if self.from.is_little() => {
                let marker: &[u8; 4] = if &sig == b"8B64" { b"46B8" } else { b"MIB8" };
                self.out[sig_at..sig_at + 4].copy_from_slice(marker);
                self.patch_len(len_at, long, data.len() as u64)?;
                self.out.extend_from_slice(data);
                self.out.extend_from_slice(pad_bytes);
            }
            Ok(false) => {
                self.out.truncate(before);
                self.warnings.push(format!(
                    "{} block ({} bytes) dropped: its layout is unknown and cannot be converted to the file's byte order",
                    String::from_utf8_lossy(&key),
                    data.len()
                ));
            }
            Err(e) if self.same_order() => {
                return Err(PsdError::invalid(format!(
                    "{} block: {e}",
                    String::from_utf8_lossy(&key)
                )))
            }
            Err(e) if self.from.is_little() => {
                let marker: &[u8; 4] = if &sig == b"8B64" { b"46B8" } else { b"MIB8" };
                self.out[sig_at..sig_at + 4].copy_from_slice(marker);
                self.patch_len(len_at, long, data.len() as u64)?;
                self.out.extend_from_slice(data);
                self.out.extend_from_slice(pad_bytes);
                self.warnings.push(format!(
                    "{} block ({} bytes) kept as stored, in little-endian layout: {e}",
                    String::from_utf8_lossy(&key),
                    data.len()
                ));
            }
            Err(e) => {
                self.out.truncate(before);
                self.warnings.push(format!(
                    "{} block ({} bytes) dropped: {e}",
                    String::from_utf8_lossy(&key),
                    data.len()
                ));
            }
        }
        Ok(())
    }

    /// Transcodes a block's data by key. `Ok(false)` for keys without a known layout.
    pub(super) fn block_body(&mut self, key: &[u8; 4]) -> Result<bool> {
        match key {
            b"Layr" | b"Lr16" | b"Lr32" => self.layer_info()?,
            b"luni" => self.unicode()?,
            b"lsct" | b"lsdk" => {
                self.u32()?;
                if self.remaining() >= 8 {
                    self.key()?;
                    self.key()?;
                }
                if self.remaining() >= 4 {
                    self.u32()?;
                }
            }
            // lyvr layer version, sn2P and vowv (shape and vector flags).
            b"lyid" | b"lspf" | b"lyvr" | b"sn2P" | b"vowv" => {
                self.u32()?;
            }
            // Text-engine global data: a textual structure, no byte order.
            b"Txt2" => self.rest()?,
            b"lnsr" => {
                self.key()?;
            }
            // Advanced Blending flags and fill opacity: one byte, then padding (Adobe spec,
            // "Additional Layer Information"; `lmgm`/`vmgm` = layer/vector mask hides effects).
            b"clbl" | b"infx" | b"knko" | b"tsly" | b"lmgm" | b"vmgm" | b"iOpa" => {
                self.u8()?;
            }
            b"lclr" => self.u16s_upto(4)?,
            b"fxrp" => {
                while self.remaining() >= 8 {
                    self.f64()?;
                }
            }
            b"brst" => self.u32s_to_end()?,
            b"LMsk" => self.u16s_upto(6)?,
            b"FMsk" => self.u16s_to_end()?,
            b"shmd" => self.metadata_setting()?,
            // Effects (lfxs: the effects as applied to a group), content credentials (CAI).
            b"lfx2" | b"lmfx" | b"lfxs" | b"CAI " => {
                self.u32()?;
                self.versioned_descriptor()?;
            }
            b"SoCo" | b"GdFl" | b"PtFl" | b"vstk" | b"blwh" | b"vibA" | b"CgEd" | b"artb"
            | b"artd" | b"cinf" | b"cust" | b"PxSD" | b"GenI" | b"OCIO" | b"extn" | b"pths" => {
                self.versioned_descriptor()?;
            }
            // Color Lookup: u16 version, then the descriptor.
            b"clrL" => {
                self.u16()?;
                self.versioned_descriptor()?;
            }
            b"lnk2" | b"lnk3" | b"lnkD" | b"lnkE" => self.linked_layers()?,
            b"Anno" => self.annotations()?,
            b"vogk" => {
                self.u32()?;
                self.versioned_descriptor()?;
            }
            b"vscg" => {
                self.key()?;
                self.versioned_descriptor()?;
            }
            b"SoLd" | b"SoLE" => {
                self.key()?;
                self.u32()?;
                self.versioned_descriptor()?;
            }
            b"PlLd" => {
                self.key()?;
                self.u32()?;
                self.pascal(1)?;
                for _ in 0..4 {
                    self.u32()?;
                }
                self.f64s(8)?;
                self.u32()?;
                self.versioned_descriptor()?;
            }
            // Type: version, transform, text version + descriptor, warp version + descriptor,
            // and the bounds as four 32-bit values (the specification says doubles; Photoshop
            // writes 16 bytes).
            b"TySh" => {
                self.u16()?;
                self.f64s(6)?;
                self.u16()?;
                self.versioned_descriptor()?;
                self.u16()?;
                self.versioned_descriptor()?;
                self.i32s(4)?;
            }
            b"vmsk" | b"vsms" => {
                self.u32()?;
                self.u32()?;
                self.path_records()?;
            }
            b"Patt" | b"Pat2" | b"Pat3" => {
                while self.remaining() >= 4 {
                    let len = self.r.clone().u32()?;
                    if len == 0 {
                        self.u32()?;
                        continue;
                    }
                    self.sized_region(false, Tx::pattern)?;
                    // Each pattern is padded to a multiple of 4.
                    let pad = ((4 - len % 4) % 4) as usize;
                    self.raw(pad.min(self.remaining()))?;
                }
            }
            b"FEid" | b"FXid" => self.filter_effects()?,
            b"lrFX" => self.legacy_effects()?,
            b"levl" => {
                self.u16()?;
                for _ in 0..29 * 5 {
                    if self.remaining() < 2 {
                        break;
                    }
                    self.u16()?;
                }
                if self.remaining() >= 4 {
                    self.key()?;
                }
                self.u16s_to_end()?;
            }
            b"curv" => self.curves()?,
            b"hue2" => {
                self.u16()?;
                self.u8()?;
                self.u8()?;
                self.u16s_to_end()?;
            }
            b"brit" => self.u16s_upto(3)?,
            b"thrs" | b"post" => {
                self.u16()?;
            }
            b"nvrt" => {}
            b"expA" => {
                self.u16()?;
                for _ in 0..3 {
                    self.f32()?;
                }
            }
            b"blnc" => self.u16s_upto(9)?,
            b"mixr" | b"selc" => self.u16s_to_end()?,
            b"phfl" => match self.u16()? {
                2 => {
                    self.u16s(5)?;
                    self.u32()?;
                    self.u8()?;
                }
                3 => {
                    for _ in 0..4 {
                        self.u32()?;
                    }
                    self.u8()?;
                }
                v => return Err(PsdError::Unsupported(format!("photo filter version {v}"))),
            },
            b"grdm" => self.gradient_map()?,
            _ if !self.same_order() => return Ok(self.descriptor_shaped()),
            _ => return Ok(false),
        }
        Ok(true)
    }
}
