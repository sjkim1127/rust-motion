//! One-dimensional lookup-table sampling and 3D colour-lookup (LUT) interpolation.

/// Number of values in the table of a `size`³ Color Lookup (`size³ × 3`). `None` for a size
/// below 2 or one whose table length overflows: a stored document's size is untrusted.
pub fn lut3d_len(size: u32) -> Option<usize> {
    if size < 2 {
        return None;
    }
    usize::try_from(size).ok()?.checked_pow(3)?.checked_mul(3)
}

/// Samples a 3D LUT (`n`³ RGB triplets, red fastest) at `c`, trilinear or tetrahedral.
pub fn lut3d_sample(lut: &[f32], n: usize, c: [f32; 3], tetrahedral: bool) -> [f32; 3] {
    // A degenerate or short table can't be sampled: leave the colour alone.
    if n < 2
        || n.checked_pow(3)
            .and_then(|c| c.checked_mul(3))
            .is_none_or(|len| lut.len() < len)
    {
        return c;
    }
    let m = (n - 1) as f32;
    let pos = c.map(|v| v.clamp(0.0, 1.0) * m);
    let i0 = pos.map(|p| (p.floor() as usize).min(n - 2));
    let f: [f32; 3] = std::array::from_fn(|k| pos[k] - i0[k] as f32);
    let at = |dr: usize, dg: usize, db: usize| -> [f32; 3] {
        let idx = (((i0[2] + db) * n + i0[1] + dg) * n + i0[0] + dr) * 3;
        [lut[idx], lut[idx + 1], lut[idx + 2]]
    };
    let mix = |w: &[(f32, [f32; 3])]| -> [f32; 3] {
        std::array::from_fn(|k| w.iter().map(|(a, v)| a * v[k]).sum())
    };
    let (fr, fg, fb) = (f[0], f[1], f[2]);
    if tetrahedral {
        let (c000, c111) = (at(0, 0, 0), at(1, 1, 1));
        if fr > fg {
            if fg > fb {
                mix(&[
                    (1.0 - fr, c000),
                    (fr - fg, at(1, 0, 0)),
                    (fg - fb, at(1, 1, 0)),
                    (fb, c111),
                ])
            } else if fr > fb {
                mix(&[
                    (1.0 - fr, c000),
                    (fr - fb, at(1, 0, 0)),
                    (fb - fg, at(1, 0, 1)),
                    (fg, c111),
                ])
            } else {
                mix(&[
                    (1.0 - fb, c000),
                    (fb - fr, at(0, 0, 1)),
                    (fr - fg, at(1, 0, 1)),
                    (fg, c111),
                ])
            }
        } else if fb > fg {
            mix(&[
                (1.0 - fb, c000),
                (fb - fg, at(0, 0, 1)),
                (fg - fr, at(0, 1, 1)),
                (fr, c111),
            ])
        } else if fb > fr {
            mix(&[
                (1.0 - fg, c000),
                (fg - fb, at(0, 1, 0)),
                (fb - fr, at(0, 1, 1)),
                (fr, c111),
            ])
        } else {
            mix(&[
                (1.0 - fg, c000),
                (fg - fr, at(0, 1, 0)),
                (fr - fb, at(1, 1, 0)),
                (fb, c111),
            ])
        }
    } else {
        let lerp = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
            std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t)
        };
        let c00 = lerp(at(0, 0, 0), at(1, 0, 0), fr);
        let c10 = lerp(at(0, 1, 0), at(1, 1, 0), fr);
        let c01 = lerp(at(0, 0, 1), at(1, 0, 1), fr);
        let c11 = lerp(at(0, 1, 1), at(1, 1, 1), fr);
        lerp(lerp(c00, c10, fg), lerp(c01, c11, fg), fb)
    }
}

pub(super) const LUT_SIZE: usize = 4096;

#[inline]
pub(super) fn lut(table: &[f32], v: f32) -> f32 {
    let x = v.clamp(0.0, 1.0) * (table.len() - 1) as f32;
    let i = x.floor() as usize;
    let j = (i + 1).min(table.len() - 1);
    let f = x - i as f32;
    table[i] * (1.0 - f) + table[j] * f
}
