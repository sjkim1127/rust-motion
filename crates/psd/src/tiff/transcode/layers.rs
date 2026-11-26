//! Layer records, channel data and masks.

use super::*;

impl<'a> Tx<'a> {
    /// An RLE row-count table (`rows` entries of 2 bytes, 4 in PSB data).
    pub(super) fn rle_counts(&mut self, rows: usize) -> Result<()> {
        for _ in 0..rows {
            if self.version.is_psb() {
                if self.remaining() < 4 {
                    break;
                }
                self.u32()?;
            } else {
                if self.remaining() < 2 {
                    break;
                }
                self.u16()?;
            }
        }
        Ok(())
    }

    /// Encoded channel data for a plane of `rows` rows: the compression field, then the RLE
    /// row counts (container order) and the samples (always big-endian: copied).
    pub(super) fn channel_data(&mut self, rows: usize, long_counts: bool) -> Result<()> {
        let saved = self.version;
        if long_counts {
            self.version = Version::Psb;
        }
        let compression = self.u16()?;
        let res = if compression == 1 {
            self.rle_counts(rows)
        } else {
            Ok(())
        };
        self.version = saved;
        res?;
        self.rest()
    }

    // ---- layer info ----

    pub(super) fn layer_info(&mut self) -> Result<()> {
        let count = self.i16()?;
        let n = usize::from(count.unsigned_abs());
        self.r.check_count(n as u64, 34)?;
        // Per layer: the channel ids and lengths, and the heights of the layer, mask and
        // real-mask rects (what each channel's RLE count table spans).
        let mut records: Vec<RecordShape> = Vec::with_capacity(n);
        for _ in 0..n {
            let top = self.i32()?;
            self.i32()?;
            let bottom = self.i32()?;
            self.i32()?;
            let nch = self.u16()?;
            if nch > crate::layer::MAX_LAYER_CHANNELS {
                return Err(PsdError::LimitExceeded("too many channels in layer"));
            }
            let mut chans = Vec::with_capacity(usize::from(nch));
            let psb = self.version.is_psb();
            for _ in 0..nch {
                let id = self.i16()?;
                let len = self.len(psb)?;
                chans.push((id, len));
            }
            let sig = self.key()?;
            if &sig != b"8BIM" {
                return Err(PsdError::InvalidSignature {
                    expected: "8BIM (layer blend mode)",
                    found: sig,
                });
            }
            self.key()?;
            for _ in 0..4 {
                self.u8()?;
            }
            let mut heights = [height(top, bottom), 0, 0];
            let mut mask_heights = (0usize, 0usize);
            self.sized_region(false, |t| {
                let mut mh = (0, 0);
                t.sized_region(false, |m| {
                    mh = m.mask()?;
                    Ok(())
                })?;
                mask_heights = mh;
                // Blending ranges are bytes (black low/high, white low/high per channel), so
                // Photoshop stores them in the same order in either byte order: an Intel-order
                // layered TIFF holds the default ranges as `00 00 ff ff`. Swapping them as u32s
                // turned every range into `ff ff 00 00`, which hides every pixel of the layer.
                t.sized_region(false, |x| x.rest())?;
                t.pascal(4)?;
                t.blocks()
            })?;
            heights[1] = mask_heights.0;
            heights[2] = mask_heights.1;
            records.push((chans, heights));
        }
        for (chans, heights) in &records {
            for &(id, len) in chans {
                if len == 0 {
                    continue;
                }
                if len == 1 {
                    return Err(PsdError::invalid("channel data length 1"));
                }
                let rows = match id {
                    -2 => heights[1],
                    -3 => heights[2],
                    _ => heights[0],
                };
                self.region(len, |t| t.channel_data(rows, false))?;
            }
        }
        self.rest()
    }

    /// A layer mask record. Returns the heights of the mask and real-mask rects.
    pub(super) fn mask(&mut self) -> Result<(usize, usize)> {
        let whole = self.r.peek_rest();
        if whole.is_empty() {
            return Ok((0, 0));
        }
        let top = self.i32()?;
        self.i32()?;
        let bottom = self.i32()?;
        self.i32()?;
        self.u8()?;
        let flags = self.u8()?;
        let mut real_h = 0;
        if self.remaining() <= 2 {
            return Ok((height(top, bottom), 0));
        }
        let has_parameters = flags & LayerMask::FLAG_PARAMETERS != 0;
        // The helper only looks at lengths and the parameter-flags byte, so it reads the
        // same in either order.
        let real_first = has_parameters && LayerMask::real_before_parameters(whole);
        let mut read_real = |t: &mut Tx<'_>| -> Result<()> {
            if t.remaining() >= 18 {
                t.u8()?;
                t.u8()?;
                let rt = t.i32()?;
                t.i32()?;
                let rb = t.i32()?;
                t.i32()?;
                real_h = height(rt, rb);
            }
            Ok(())
        };
        if real_first {
            read_real(self)?;
        }
        if has_parameters {
            let pf = self.u8()?;
            let need = mask_parameter_bytes(pf);
            if need > self.remaining() {
                return Err(PsdError::UnexpectedEof {
                    offset: self.r.pos(),
                    needed: need - self.remaining(),
                });
            }
            if pf & 1 != 0 {
                self.u8()?;
            }
            if pf & 2 != 0 {
                self.f64()?;
            }
            if pf & 4 != 0 {
                self.u8()?;
            }
            if pf & 8 != 0 {
                self.f64()?;
            }
        }
        if !real_first {
            read_real(self)?;
        }
        Ok((height(top, bottom), real_h))
    }
}
