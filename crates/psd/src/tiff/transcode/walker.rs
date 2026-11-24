//! The [`Tx`] walker: reads in the source order, writes in the target order, and the primitives and region helpers every structure is built from.

use super::*;

pub(super) struct Tx<'a> {
    pub(super) r: Reader<'a>,
    pub(super) out: Vec<u8>,
    pub(super) from: ByteOrder,
    pub(super) to: ByteOrder,
    pub(super) version: Version,
    pub(super) warnings: Vec<String>,
    pub(super) depth: usize,
}

macro_rules! copy_int {
    ($name:ident, $ty:ty) => {
        pub(super) fn $name(&mut self) -> Result<$ty> {
            let v = self.r.$name()?;
            self.out.extend_from_slice(&if self.to.is_little() {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            });
            Ok(v)
        }
    };
}

impl<'a> Tx<'a> {
    pub(super) fn new(data: &'a [u8], version: Version, from: ByteOrder, to: ByteOrder) -> Self {
        let r = if from.is_little() {
            Reader::new_little(data)
        } else {
            Reader::new(data)
        };
        Tx {
            r,
            out: Vec::with_capacity(data.len()),
            from,
            to,
            version,
            warnings: Vec::new(),
            depth: 0,
        }
    }

    pub(super) fn finish(self) -> (Vec<u8>, Vec<String>) {
        (self.out, self.warnings)
    }

    /// A walker over `data` (a nested structure) that writes into this walker's output: it
    /// takes the buffer, so a nested structure is written once, not copied up per level. Hand
    /// the buffer back with [`Self::adopt`] before returning, error or not.
    pub(super) fn nested<'b>(&mut self, data: &'b [u8], depth: usize) -> Tx<'b> {
        let r = if self.from.is_little() {
            Reader::new_little(data)
        } else {
            Reader::new(data)
        };
        Tx {
            r,
            out: std::mem::take(&mut self.out),
            from: self.from,
            to: self.to,
            version: self.version,
            warnings: Vec::new(),
            depth,
        }
    }

    /// Takes the output buffer back from a [`Self::nested`] walker.
    pub(super) fn adopt(&mut self, child: &mut Tx<'_>) {
        self.out = std::mem::take(&mut child.out);
    }

    pub(super) fn same_order(&self) -> bool {
        self.from == self.to
    }

    pub(super) fn remaining(&self) -> usize {
        self.r.remaining()
    }

    copy_int!(u16, u16);
    copy_int!(i16, i16);
    copy_int!(u32, u32);
    copy_int!(i32, i32);
    copy_int!(u64, u64);
    copy_int!(i64, i64);
    copy_int!(f64, f64);

    pub(super) fn u8(&mut self) -> Result<u8> {
        let v = self.r.u8()?;
        self.out.push(v);
        Ok(v)
    }

    pub(super) fn f32(&mut self) -> Result<()> {
        let b = self.r.array::<4>()?;
        let v = if self.from.is_little() {
            f32::from_le_bytes(b)
        } else {
            f32::from_be_bytes(b)
        };
        self.out.extend_from_slice(&if self.to.is_little() {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        });
        Ok(())
    }

    /// Bytes copied as they are.
    pub(super) fn raw(&mut self, n: usize) -> Result<()> {
        let b = self.r.bytes(n)?;
        self.out.extend_from_slice(b);
        Ok(())
    }

    pub(super) fn rest(&mut self) -> Result<()> {
        let n = self.remaining();
        self.raw(n)
    }

    /// A four-character signature or key: stored reversed in little-endian data. Returns the
    /// canonical (big-endian) spelling.
    pub(super) fn key(&mut self) -> Result<[u8; 4]> {
        let mut k = self.r.array::<4>()?;
        if self.from.is_little() {
            k.reverse();
        }
        let mut w = k;
        if self.to.is_little() {
            w.reverse();
        }
        self.out.extend_from_slice(&w);
        Ok(k)
    }

    pub(super) fn len(&mut self, long: bool) -> Result<u64> {
        if long {
            self.u64()
        } else {
            Ok(u64::from(self.u32()?))
        }
    }

    pub(super) fn u16s(&mut self, n: usize) -> Result<()> {
        for _ in 0..n {
            self.u16()?;
        }
        Ok(())
    }

    /// Up to `n` `u16`s: a block shorter than its full layout keeps what it has.
    pub(super) fn u16s_upto(&mut self, n: usize) -> Result<()> {
        for _ in 0..n {
            if self.remaining() < 2 {
                break;
            }
            self.u16()?;
        }
        Ok(())
    }

    pub(super) fn i32s(&mut self, n: usize) -> Result<()> {
        for _ in 0..n {
            self.i32()?;
        }
        Ok(())
    }

    pub(super) fn f64s(&mut self, n: usize) -> Result<()> {
        for _ in 0..n {
            self.f64()?;
        }
        Ok(())
    }

    /// `u16`s to the end (a lone odd byte is copied).
    pub(super) fn u16s_to_end(&mut self) -> Result<()> {
        while self.remaining() >= 2 {
            self.u16()?;
        }
        self.rest()
    }

    pub(super) fn u32s_to_end(&mut self) -> Result<()> {
        while self.remaining() >= 4 {
            self.u32()?;
        }
        self.rest()
    }

    /// A Photoshop Unicode string: u32 count + UTF-16 units.
    pub(super) fn unicode(&mut self) -> Result<()> {
        let n = self.u32()?;
        self.r.check_count(u64::from(n), 2)?;
        self.u16s(n as usize)
    }

    /// A Pascal string padded to `align`.
    pub(super) fn pascal(&mut self, align: usize) -> Result<()> {
        let n = usize::from(self.u8()?);
        self.raw(n)?;
        let total = 1 + n;
        self.raw((align - total % align) % align)
    }

    /// Transcodes the next `len` bytes with `f`; whatever `f` leaves is copied verbatim.
    pub(super) fn region(
        &mut self,
        len: u64,
        f: impl FnOnce(&mut Tx<'_>) -> Result<()>,
    ) -> Result<()> {
        let data = self.r.bytes_u64(len)?;
        let start = self.out.len();
        let mut child = self.nested(data, self.depth);
        let res = f(&mut child).and_then(|()| child.rest());
        self.adopt(&mut child);
        res?;
        if self.out.len() - start != data.len() {
            return Err(PsdError::invalid(
                "byte-order conversion changed a structure's length",
            ));
        }
        self.warnings.append(&mut child.warnings);
        Ok(())
    }

    /// Reads a length field (4 bytes, or 8 when `long`), transcodes that many bytes with `f`
    /// and writes the length of the result: blocks dropped inside shrink the field.
    pub(super) fn sized_region(
        &mut self,
        long: bool,
        f: impl FnOnce(&mut Tx<'_>) -> Result<()>,
    ) -> Result<()> {
        self.sized_region_counting(long, false, f)
    }

    /// [`Self::sized_region`] for a u32 length that counts its own 4 bytes.
    pub(super) fn sized_region_inclusive(
        &mut self,
        f: impl FnOnce(&mut Tx<'_>) -> Result<()>,
    ) -> Result<()> {
        self.sized_region_counting(false, true, f)
    }

    pub(super) fn sized_region_counting(
        &mut self,
        long: bool,
        inclusive: bool,
        f: impl FnOnce(&mut Tx<'_>) -> Result<()>,
    ) -> Result<()> {
        let mut len = self.r.len_field(long)?;
        let at = self.out.len();
        self.out
            .extend_from_slice(if long { &[0u8; 8] } else { &[0u8; 4] });
        if inclusive {
            len = len
                .checked_sub(4)
                .ok_or_else(|| PsdError::invalid("length shorter than its own field"))?;
        }
        let data = self.r.bytes_u64(len)?;
        let start = self.out.len();
        let mut child = self.nested(data, self.depth);
        let res = f(&mut child).and_then(|()| child.rest());
        self.adopt(&mut child);
        res?;
        self.warnings.append(&mut child.warnings);
        self.patch_len(
            at,
            long,
            (self.out.len() - start) as u64 + if inclusive { 4 } else { 0 },
        )?;
        Ok(())
    }

    /// Tries `f` on the rest of the data; it counts as a match only when at most three
    /// trailing zero bytes are left. On a miss nothing is consumed or written.
    pub(super) fn attempt(&mut self, f: impl FnOnce(&mut Tx<'_>) -> Result<()>) -> bool {
        let (reader, written) = (self.r.clone(), self.out.len());
        let ok =
            f(self).is_ok() && self.remaining() <= 3 && self.r.peek_rest().iter().all(|&b| b == 0);
        if !ok {
            self.r = reader;
            self.out.truncate(written);
        }
        ok
    }

    /// Blocks whose layout is not known by key are very often a versioned descriptor,
    /// optionally behind a u32 version (every Photoshop block introduced since CS6 is):
    /// recognized by parsing to the end.
    pub(super) fn descriptor_shaped(&mut self) -> bool {
        self.attempt(|t| t.versioned_descriptor())
            || self.attempt(|t| {
                t.u32()?;
                t.versioned_descriptor()
            })
    }

    /// Writes `n` into the length placeholder at `at`.
    pub(super) fn patch_len(&mut self, at: usize, long: bool, n: u64) -> Result<()> {
        if long {
            let b = if self.to.is_little() {
                n.to_le_bytes()
            } else {
                n.to_be_bytes()
            };
            if let Some(slot) = self.out.get_mut(at..at + 8) {
                slot.copy_from_slice(&b);
            }
        } else {
            let n = u32::try_from(n)
                .map_err(|_| PsdError::LimitExceeded("structure too large for a 32-bit length"))?;
            let b = if self.to.is_little() {
                n.to_le_bytes()
            } else {
                n.to_be_bytes()
            };
            if let Some(slot) = self.out.get_mut(at..at + 4) {
                slot.copy_from_slice(&b);
            }
        }
        Ok(())
    }
}
