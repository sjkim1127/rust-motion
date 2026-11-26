//! Colour range in Lab: the fuzziness box, its coverage and the 8-bit grid.

use super::*;

/// The colours of Color Range › Sampled Colors: the Lab box spanned by the eyedropper samples
/// (Photoshop records it in actions as the Minimum and Maximum Lab colours; "Add to Sample"
/// grows it). One sample is a box of size zero. CIE Lab (D50): L 0–100, a and b about −128–127.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LabRange {
    pub lo: [f32; 3],
    pub hi: [f32; 3],
}

impl LabRange {
    /// The box around one colour.
    pub fn point(lab: [f32; 3]) -> Self {
        Self { lo: lab, hi: lab }
    }

    /// The box spanning `colors`, `None` when there are none.
    pub fn spanning(colors: &[[f32; 3]]) -> Option<Self> {
        let first = *colors.first()?;
        Some(colors.iter().fold(Self::point(first), |r, c| r.grow(c)))
    }

    /// "Add to Sample": the box grows to take `lab` in.
    pub fn grow(self, lab: &[f32; 3]) -> Self {
        Self {
            lo: [0, 1, 2].map(|k| self.lo[k].min(lab[k])),
            hi: [0, 1, 2].map(|k| self.hi[k].max(lab[k])),
        }
    }

    /// "Subtract from Sample" the way Photoshop does it (observed through its Color Range dialog):
    /// it only reshapes the box, each Lab axis on its own, with `w` the axis's width at `fuzziness`
    /// (L: fuzziness/2.55 units, a and b: a third of fuzziness·256/255) and q = w/5.
    ///
    /// - Inside the box, the nearer edge moves onto the subtracted value (the upper one on a tie),
    ///   so the subtracted colour stays selected, at the new edge.
    /// - On an edge or outside it by less than w, that edge moves in to the first of
    ///   m + q, m + 2q, … m + 5q beyond it (m the subtracted value).
    /// - Further out, nothing changes. An edge never crosses the other one.
    pub fn subtract(self, lab: &[f32; 3], fuzziness: f32) -> Self {
        let fz = if fuzziness.is_finite() {
            fuzziness.max(0.0)
        } else {
            0.0
        };
        let mut r = self;
        for (k, &m) in lab.iter().enumerate() {
            // Measured: the lightness step has no 256/255 factor, the a/b steps do.
            let w = if k == 0 {
                fz / 2.55
            } else {
                fz * 256.0 / 255.0 / 3.0
            };
            let (lo, hi) = (r.lo[k], r.hi[k]);
            if !m.is_finite() {
                continue;
            }
            if m > lo && m < hi {
                if m - lo < hi - m {
                    r.lo[k] = m;
                } else {
                    r.hi[k] = m;
                }
                continue;
            }
            let q = w / 5.0;
            if q <= 0.0 {
                continue;
            }
            let d = if m <= lo { lo - m } else { m - hi };
            let n = (d / q).floor() + 1.0;
            if n > 5.0 {
                continue;
            }
            if m <= lo {
                r.lo[k] = (m + n * q).min(hi);
            } else {
                r.hi[k] = (m - n * q).max(lo);
            }
        }
        r
    }
}

/// How much a Lab colour belongs to `range` at `fuzziness` (0–200), the way Photoshop's
/// Select › Color Range › Sampled Colors decides it. Reconstructed by observation only (Color
/// Range run on synthetic Lab lattices and photos with Photoshop's own eyedropper, its masks
/// compared value by value): on an 8-bit sRGB photo it matches Photoshop 2024 to about half a
/// level of 255 on average.
///
/// - Per axis, the distance outside the box in 8-bit Lab units: L × 2.55, a and b as they are,
///   with a and b weighted ×3 (hue counts three times as much as lightness).
/// - Each axis falls off on its own and the coverage is their minimum: the falloff of the
///   largest of the three distances.
/// - The falloff is a quadratic B-spline of width fuzziness × 256/255: full at the box, half at
///   half the width, nothing beyond it, quantised to 256 levels like Photoshop's 8-bit masks.
///
/// Fuzziness 0 keeps only colours equal to the samples. For 8-bit documents use
/// [`lab_range_coverage_8bit`].
pub fn lab_range_coverage(lab: [f32; 3], range: &LabRange, fuzziness: f32) -> f32 {
    coverage(lab, range, fuzziness, false)
}

/// [`lab_range_coverage`] for 8-bit documents, which Photoshop compares on the 8-bit Lab grid
/// (pixels and samples through [`quantize_lab8`] first). There, a and b steps are uneven: a step
/// above the samples' range counts half a unit more, one below it half a unit less, while
/// colours inside the range (the sampled colour itself) stay fully selected. Measured on
/// Photoshop's eyedropper masks of a photo, this halves the error of the plain distance.
pub fn lab_range_coverage_8bit(lab: [f32; 3], range: &LabRange, fuzziness: f32) -> f32 {
    coverage(lab, range, fuzziness, true)
}

pub(super) fn coverage(lab: [f32; 3], range: &LabRange, fuzziness: f32, grid8: bool) -> f32 {
    let outside = |v: f32, lo: f32, hi: f32| (lo - v).max(v - hi).max(0.0);
    if fuzziness.is_nan() || fuzziness <= 0.0 {
        let d = (outside(lab[0], range.lo[0], range.hi[0]) * 2.55)
            .max(outside(lab[1], range.lo[1], range.hi[1]))
            .max(outside(lab[2], range.lo[2], range.hi[2]));
        return if d < 0.5 { 1.0 } else { 0.0 };
    }
    let ab = |v: f32, lo: f32, hi: f32| {
        if !grid8 {
            outside(v, lo, hi)
        } else if v > hi {
            v - hi + 0.5
        } else if v < lo {
            (lo - v - 0.5).max(0.0)
        } else {
            0.0
        }
    };
    let dl = outside(lab[0], range.lo[0], range.hi[0]) * 2.55;
    let da = 3.0 * ab(lab[1], range.lo[1], range.hi[1]);
    let db = 3.0 * ab(lab[2], range.lo[2], range.hi[2]);
    let t = dl.max(da).max(db) / (fuzziness * 256.0 / 255.0);
    let v = if t <= 0.5 {
        1.0 - 2.0 * t * t
    } else if t < 1.0 {
        2.0 * (1.0 - t) * (1.0 - t)
    } else {
        0.0
    };
    (v * 256.0).floor().clamp(0.0, 255.0) / 255.0
}

/// A Lab colour on Photoshop's 8-bit Lab grid (L in 1/2.55 steps, a and b whole units), the
/// precision its Color Range compares 8-bit documents at (pixels and samples alike).
pub fn quantize_lab8(lab: [f32; 3]) -> [f32; 3] {
    [
        (lab[0] * 2.55).round() / 2.55,
        lab[1].round(),
        lab[2].round(),
    ]
}

/// Color Range › Sampled Colors over Lab pixels (`lab[i]` = L, a, b, alpha; `w` the row width):
/// each pixel's [`lab_range_coverage`] ([`lab_range_coverage_8bit`] with `grid8`), 0 where
/// transparent. With `localized` (Localized Color
/// Clusters: a radius in pixels and the sample positions) coverage also fades with the distance
/// to the nearest sample, reaching 0 at the radius, so only nearby matching colours are selected.
pub fn color_range_lab(
    lab: &[[f32; 4]],
    w: usize,
    range: &LabRange,
    fuzziness: f32,
    grid8: bool,
    localized: Option<(f32, &[(f32, f32)])>,
) -> Vec<f32> {
    let w = w.max(1);
    let localized = localized.filter(|(_, at)| !at.is_empty());
    let mut out = vec![0.0f32; lab.len()];
    par_rows(&mut out, w, 1, |y, row| {
        for (x, m) in row.iter_mut().enumerate() {
            let Some(&[l, a, b, alpha]) = lab.get(y * w + x) else {
                continue;
            };
            if alpha.is_nan() || alpha <= 0.0 {
                continue;
            }
            let mut k = coverage([l, a, b], range, fuzziness, grid8);
            if let Some((radius, at)) = localized {
                if k > 0.0 {
                    let (px, py) = (x as f32, y as f32);
                    let near = at
                        .iter()
                        .map(|&(sx, sy)| {
                            (1.0 - ((px - sx).powi(2) + (py - sy).powi(2)).sqrt() / radius.max(1.0))
                                .clamp(0.0, 1.0)
                        })
                        .fold(0.0f32, f32::max);
                    k *= near;
                }
            }
            *m = k;
        }
    });
    out
}
