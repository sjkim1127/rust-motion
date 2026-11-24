//! Adjustment-layer structures: legacy effects, curves and gradient maps.

use super::*;

impl<'a> Tx<'a> {
    /// `lrFX`: version, count, then effects (`cmnS`, `dsdw`, `isdw`, `oglw`, `iglw`, `bevl`,
    /// `sofi`), each a signature, key, u32 size and fixed fields.
    pub(super) fn legacy_effects(&mut self) -> Result<()> {
        self.u16()?;
        let n = self.u16()?;
        for _ in 0..n {
            if self.remaining() < 12 {
                break;
            }
            self.key()?;
            let key = self.key()?;
            self.sized_region(false, |t| {
                // A colour: u16 space + 4 × u16 components.
                let colour = |t: &mut Tx<'_>| t.u16s(5);
                let blend = |t: &mut Tx<'_>| -> Result<()> {
                    t.key()?;
                    t.key()?;
                    Ok(())
                };
                match &key {
                    b"cmnS" => {
                        t.u32()?;
                        t.u8()?;
                        t.u16()?;
                    }
                    b"dsdw" | b"isdw" => {
                        t.u32()?;
                        t.i32s(4)?;
                        colour(t)?;
                        blend(t)?;
                        t.u8()?;
                        t.u8()?;
                        t.u8()?;
                        if t.remaining() >= 10 {
                            colour(t)?;
                        }
                    }
                    b"oglw" => {
                        t.u32()?;
                        t.i32s(2)?;
                        colour(t)?;
                        blend(t)?;
                        t.u8()?;
                        t.u8()?;
                        if t.remaining() >= 10 {
                            colour(t)?;
                        }
                    }
                    b"iglw" => {
                        t.u32()?;
                        t.i32s(2)?;
                        colour(t)?;
                        blend(t)?;
                        t.u8()?;
                        t.u8()?;
                        if t.remaining() >= 11 {
                            t.u8()?;
                            colour(t)?;
                        }
                    }
                    b"bevl" => {
                        t.u32()?;
                        t.i32s(3)?;
                        blend(t)?;
                        blend(t)?;
                        colour(t)?;
                        colour(t)?;
                        for _ in 0..6 {
                            t.u8()?;
                        }
                        if t.remaining() >= 20 {
                            colour(t)?;
                            colour(t)?;
                        }
                    }
                    b"sofi" => {
                        t.u32()?;
                        blend(t)?;
                        colour(t)?;
                        t.u8()?;
                        t.u8()?;
                        if t.remaining() >= 10 {
                            colour(t)?;
                        }
                    }
                    _ => {}
                }
                Ok(())
            })?;
        }
        Ok(())
    }

    /// `curv`: pad byte, version, channel bitmap, per set bit a point list; then an optional
    /// `Crv ` extension with per-channel point lists.
    pub(super) fn curves(&mut self) -> Result<()> {
        self.u8()?;
        self.u16()?;
        let bits = self.u32()?;
        for bit in 0..32 {
            if bits & (1 << bit) == 0 {
                continue;
            }
            if self.remaining() < 2 {
                break;
            }
            let n = self.u16()?;
            self.u16s(usize::from(n) * 2)?;
        }
        if self.remaining() >= 10 {
            self.key()?;
            self.u16()?;
            let n = self.u32()?;
            for _ in 0..n {
                if self.remaining() < 4 {
                    break;
                }
                self.u16()?;
                let pts = self.u16()?;
                self.u16s(usize::from(pts) * 2)?;
            }
        }
        Ok(())
    }

    /// `grdm`: version, reverse, dither, [method key], name, colour stops, transparency
    /// stops, then the noise-gradient fields (a u32 seed among u16s).
    pub(super) fn gradient_map(&mut self) -> Result<()> {
        let version = self.u16()?;
        self.u8()?;
        self.u8()?;
        if version == 3 {
            self.key()?;
        }
        self.unicode()?;
        let n = self.u16()?;
        for _ in 0..n {
            if self.remaining() < 20 {
                break;
            }
            self.u32()?;
            self.u32()?;
            self.u16s(6)?;
        }
        let nt = self.u16()?;
        for _ in 0..nt {
            if self.remaining() < 10 {
                break;
            }
            self.u32()?;
            self.u32()?;
            self.u16()?;
        }
        // expansion, interpolation, length, mode, u32 seed, then u16 fields.
        for _ in 0..4 {
            if self.remaining() < 2 {
                return Ok(());
            }
            self.u16()?;
        }
        if self.remaining() >= 4 {
            self.u32()?;
        }
        self.u16s_to_end()
    }
}
