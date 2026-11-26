//! Tone mapping: Levels, Posterize and the combined per-channel tone tables.

use super::*;

/// Float RGB curve channels with no edit must preserve samples outside the LUT domain.
/// Edited curves retain the existing spline/LUT behaviour until HDR extrapolation has an oracle.
pub fn float_tone_identity(adj: &Adjustment, depth: Option<SampleType>) -> [bool; 3] {
    match adj {
        Adjustment::Curves {
            master,
            per_channel,
            space: ToneSpace::Rgb,
            ..
        } if depth == Some(SampleType::F32) => {
            std::array::from_fn(|i| is_identity_curve(master) && is_identity_curve(&per_channel[i]))
        }
        _ => [false; 3],
    }
}

/// Per-channel LUTs of a Levels or Curves adjustment: each channel's record first, then the
/// master (composite) record, as Photoshop does (psd-tools levels_rgb.psd and curves_rgb.psd,
/// #975). Four rows: the three `per_channel` channels then black
/// (identity unless the space is CMYK). In Lab there is no composite record, so the master is ignored.
pub fn tone_luts(adj: &Adjustment) -> [Vec<f32>; 4] {
    tone_luts_q(adj, None)
}

/// [`tone_luts`] for samples with `quantum` steps per unit (Levels works on whole levels; see
/// [`levels_q`]).
pub fn tone_luts_q(adj: &Adjustment, quantum: Option<f32>) -> [Vec<f32>; 4] {
    let x = |k: usize| k as f32 / (LUT_SIZE - 1) as f32;
    match adj {
        Adjustment::Levels {
            master,
            per_channel,
            space,
            black,
        } => {
            let ident = LevelsChannel::default();
            let m = if *space == ToneSpace::Lab {
                &ident
            } else {
                master
            };
            let row = |c: &LevelsChannel| {
                (0..LUT_SIZE)
                    .map(|k| levels_q(m, levels_q(c, x(k), quantum), quantum))
                    .collect()
            };
            [
                row(&per_channel[0]),
                row(&per_channel[1]),
                row(&per_channel[2]),
                row(if *space == ToneSpace::Cmyk {
                    black
                } else {
                    &ident
                }),
            ]
        }
        Adjustment::Curves {
            master,
            per_channel,
            space,
            black,
            pencil,
        } => {
            let m = if *space == ToneSpace::Lab {
                curve_lut(&[])
            } else {
                curve_lut_mode(master, *pencil)
            };
            let row = |c: &[CurvePoint]| {
                curve_lut_mode(c, *pencil)
                    .iter()
                    .map(|&v| lut(&m, v))
                    .collect()
            };
            [
                row(&per_channel[0]),
                row(&per_channel[1]),
                row(&per_channel[2]),
                row(if *space == ToneSpace::Cmyk {
                    black
                } else {
                    &[]
                }),
            ]
        }
        _ => std::array::from_fn(|_| (0..LUT_SIZE).map(x).collect()),
    }
}

/// [`tone_luts_q`] in a document of `depth` (`None`: unrounded). 32-bit Levels use
/// [`levels_float`], sampled on 0..1 and kept in 0..1 for LUT consumers. Float RGB rendering
/// evaluates the formula directly instead, so samples and results outside that domain survive.
pub fn tone_luts_depth(adj: &Adjustment, depth: Option<SampleType>) -> [Vec<f32>; 4] {
    match (adj, depth) {
        (
            Adjustment::Levels {
                master,
                per_channel,
                space: ToneSpace::Rgb,
                ..
            },
            Some(SampleType::F32),
        ) => {
            let x = |k: usize| k as f32 / (LUT_SIZE - 1) as f32;
            let row = |c: &LevelsChannel| {
                (0..LUT_SIZE)
                    .map(|k| levels_float(master, levels_float(c, x(k))).clamp(0.0, 1.0))
                    .collect()
            };
            [
                row(&per_channel[0]),
                row(&per_channel[1]),
                row(&per_channel[2]),
                (0..LUT_SIZE).map(x).collect(),
            ]
        }
        _ => tone_luts_q(adj, depth.and_then(adjustment_quantum)),
    }
}

/// Applies channel LUTs to a display-RGB colour in CMYK (ink brightness, 1 - ink) or Lab space,
/// through the conversions the document's surfaces use. The change is added as a difference of
/// two round trips, so channels a curve leaves alone (and out-of-gamut colours) stay exact.
pub(super) fn tone_in_space(space: ToneSpace, luts: &[Vec<f32>; 4], c: [f32; 3]) -> [f32; 3] {
    use crate::convert::{cmyk_to_rgb, rgb_to_cmyk};
    let (before, after) = match space {
        ToneSpace::Cmyk => {
            let ink = rgb_to_cmyk(c);
            let out: [f32; 4] = std::array::from_fn(|i| 1.0 - lut(&luts[i], 1.0 - ink[i]));
            if out == ink {
                return c;
            }
            (cmyk_to_rgb(ink), cmyk_to_rgb(out))
        }
        ToneSpace::Lab => {
            let l = srgb_to_lab(c);
            let n = [l[0] / 100.0, (l[1] + 128.0) / 255.0, (l[2] + 128.0) / 255.0];
            let o: [f32; 3] = std::array::from_fn(|i| lut(&luts[i], n[i]));
            if o == n {
                return c;
            }
            let back = |v: [f32; 3]| {
                lab_to_srgb([v[0] * 100.0, v[1] * 255.0 - 128.0, v[2] * 255.0 - 128.0])
            };
            (back(n), back(o))
        }
        ToneSpace::Rgb => return std::array::from_fn(|i| lut(&luts[i], c[i])),
    };
    std::array::from_fn(|i| (c[i] + after[i] - before[i]).clamp(0.0, 1.0))
}

/// Photoshop posterize: `n` equal input bins over 0..=255, output levels
/// `floor(k * 255 / (n - 1))` (exact on the corpus for 3, 7, 13 and 21 levels).
pub fn posterize(v: f32, levels: u32) -> f32 {
    let n = levels.clamp(2, 255) as f32;
    let x = (v.clamp(0.0, 1.0) * 255.0).round();
    let bin = (x * n / 256.0).floor().min(n - 1.0);
    (bin * 255.0 / (n - 1.0)).floor() / 255.0
}

/// [`posterize`] in a document of `depth` (`None`: unknown, treated as 8-bit). At 16 bits
/// Photoshop bins the 16-bit sample (`⌊v · n / 32768⌋`) and lands on 8-bit levels through a
/// 16-bit step: `⌊b · 32768 / (n - 1)⌋`, converted with `⌊(o · 255 + 16384) / 32768⌋`, so
/// 127.5 comes out as 128 but 42.5 as 42 (8 bits floor them all). Exact on every sample of
/// psd-tools posterize_16bits_rgb.psd and posterize_16bits_grayscale.psd (levels 2 to 21),
/// which the 8-bit rule missed by up to half the range.
pub fn posterize_in(v: f32, levels: u32, depth: Option<SampleType>) -> f32 {
    if depth != Some(SampleType::U16) {
        return posterize(v, levels);
    }
    let n = f64::from(levels.clamp(2, 255));
    let x = (f64::from(v.clamp(0.0, 1.0)) * 32768.0).round();
    let bin = (x * n / 32768.0).floor().min(n - 1.0);
    let step = (bin * 32768.0 / (n - 1.0)).floor();
    (((step * 255.0 + 16384.0) / 32768.0).floor() / 255.0) as f32
}

pub fn levels(ch: &LevelsChannel, v: f32) -> f32 {
    levels_q(ch, v, None)
}

/// [`levels`] on samples with `quantum` steps per unit (8-bit: 255). Photoshop works on whole
/// levels in three steps, each rounded half up to a whole level: the input-range stretch, the
/// midtone gamma ([`levels_gamma`]) and the output range. So a 44..214 range maps 45, 46, 47, 48
/// → 1.5, 3, 4.5, 6 → 2, 3, 5, 6 (psd-tools levels_grayscale, where the gamma toe magnifies the
/// steps), and an output range narrower than 0..255 maps neighbouring gamma levels to one level
/// (psd-tools levels_rgb: 43..48 → 58 through a 0.47 channel gamma and the master's 48..255).
/// In a LUT the result is a staircase that is exact at every level.
pub fn levels_q(ch: &LevelsChannel, v: f32, quantum: Option<f32>) -> f32 {
    let range = (ch.in_white - ch.in_black).max(1e-6);
    let quantum = quantum.filter(|q| q.is_finite() && *q >= 1.0);
    let whole = |x: f32| match quantum {
        Some(q) => (x * q + 0.5).floor() / q,
        None => x,
    };
    let u = match quantum {
        Some(q) => {
            let v = (v * q).round() / q;
            (((v - ch.in_black) / range).clamp(0.0, 1.0) * q + 0.5 + 1e-3).floor() / q
        }
        None => ((v - ch.in_black) / range).clamp(0.0, 1.0),
    };
    let t = whole(levels_gamma(u, ch.gamma.max(0.01)));
    whole(ch.out_black + t * (ch.out_white - ch.out_black))
}

/// Exponent of the shadow toe in [`levels_gamma`] (fitted; see there).
pub(super) const LEVELS_TOE_POWER: f32 = 2.2;

/// Levels' midtone gamma `g` on `u` in 0..1: `u^(1/g)`, except that above 1, where that power
/// curve is infinitely steep at black, Photoshop starts along a toe `a·u − b·u^q` of slope
/// `a = 2^g` that joins the curve with the same value and slope. `q` = [`LEVELS_TOE_POWER`] is
/// fitted on whole-level oracles at gamma 1.16, 1.22, 1.3, 1.78 and 2.0 (psd-tools levels_rgb
/// and levels_grayscale, the photoshop corpus's rgb8 and gray8 levels.psd, a measured 0..255
/// ramp): all but 2 of their 1118 levels exact, the others within one. Below 1 the power curve is
/// exact (gamma 0.67 and 0.7: every level).
pub fn levels_gamma(u: f32, g: f32) -> f32 {
    let c = 1.0 / g;
    if g <= 1.0 {
        return u.powf(c);
    }
    let (a, q) = (2f32.powf(g), LEVELS_TOE_POWER);
    // Where the toe meets the curve: a·u1 − b·u1^q = u1^c and a − q·b·u1^(q−1) = c·u1^(c−1).
    let u1 = ((1.0 - c / q) / (a * (1.0 - 1.0 / q))).powf(1.0 / (1.0 - c));
    if u >= u1 || u1 <= 0.0 {
        return u.powf(c);
    }
    let b = (a * u1 - u1.powf(c)) / u1.powf(q);
    a * u - b * u.powf(q)
}

/// [`levels`] in a 32-bit document. Neither the input is clipped to the input range nor the result
/// to the output range there, and the midtone gamma is a plain power curve, mirrored below the
/// black point (the oracle corpus' rgb32 and gray32 levels.psd: within 0.5/255, 10/255 off with
/// the clipped curve).
pub fn levels_float(ch: &LevelsChannel, v: f32) -> f32 {
    let t = (v - ch.in_black) / (ch.in_white - ch.in_black).max(1e-6);
    let t = if ch.gamma == 1.0 {
        t
    } else {
        t.signum() * t.abs().powf(1.0 / ch.gamma.max(0.01))
    };
    ch.out_black + t * (ch.out_white - ch.out_black)
}
