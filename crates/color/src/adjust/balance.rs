//! Colour Balance, Selective Color and Photo Filter.

use super::*;

/// Color Balance on channel `ch` (0 red, 1 green, 2 blue) of `v` (0..1); `tones` are the
/// shadows, midtones and highlights slider triples (cyan–red, magenta–green, yellow–blue,
/// −100..100).
///
/// Reverse-engineered from Photoshop (a 16-bit gray ramp through every tone at ±10..±100, pairs of
/// tones, and random settings of all nine sliders, Preserve Luminosity on and off): the three
/// tones fold into one Levels per channel (input black point, input white point, midtone gamma),
/// within 0.8 of 255 everywhere. Folding matters: a lowered shadows and a raised highlights
/// slider cancel each other's gamma, which applying the tones one after another doesn't (#3423).
/// - Preserve Luminosity off: a slider only moves its own channel. Lowered shadows set the black
///   point to |shadows| levels, raised highlights set the white point to 255 − highlights levels,
///   and the gamma is 2^(m / 100) with m = midtones + (shadows + highlights) / 2.
/// - Preserve Luminosity on: each channel's black point is (max − own) shadows levels, its white
///   point 255 − (own − min) highlights levels, and m = own − (max + min) / 2 midtones.
///
/// A gamma > 1 starts with slope 2^gamma at black ([`balance_gamma`]: up to 18 of 255 brighter
/// near black without it). When the black point reaches the white point, input
/// at or above the white point is white and the rest black.
pub fn color_balance_channel(
    tones: [[f32; 3]; 3],
    preserve_luminosity: bool,
    ch: usize,
    v: f32,
) -> f32 {
    let [shadows, midtones, highlights] = tones.map(|t| {
        t.map(|s| {
            if s.is_finite() {
                s.clamp(-100.0, 100.0)
            } else {
                0.0
            }
        })
    });
    let own = |t: [f32; 3]| t.get(ch).copied().unwrap_or(0.0);
    let max = |t: [f32; 3]| t[0].max(t[1]).max(t[2]);
    let min = |t: [f32; 3]| t[0].min(t[1]).min(t[2]);
    let (black, white, m) = if preserve_luminosity {
        (
            max(shadows) - own(shadows),
            own(highlights) - min(highlights),
            own(midtones) - (max(midtones) + min(midtones)) / 2.0,
        )
    } else {
        (
            (-own(shadows)).max(0.0),
            own(highlights).max(0.0),
            own(midtones) + (own(shadows) + own(highlights)) / 2.0,
        )
    };
    let (b, w) = (black / 255.0, 1.0 - white / 255.0);
    let v = if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if v >= w {
        return 1.0;
    }
    if v <= b {
        return 0.0;
    }
    balance_gamma((v - b) / (w - b), 2f32.powf(m / 100.0))
}

/// The midtone gamma of Color Balance's per-channel Levels (`u` in 0..1): `u^(1/gamma)`, except
/// that a gamma > 1 (which would start with infinite slope) starts at slope 2^gamma and joins the
/// power curve in a cubic Hermite at twice the input where that line meets it (Photoshop,
/// measured on a 16-bit ramp at gamma 1.07..2: within 0.02 of 255).
pub(super) fn balance_gamma(u: f32, gamma: f32) -> f32 {
    let e = 1.0 / gamma.max(0.01);
    let power = u.powf(e);
    if gamma <= 1.0 {
        return power;
    }
    let s0 = 2f32.powf(gamma);
    let u0 = (2.0 * s0.powf(1.0 / (e - 1.0))).min(1.0);
    if u0.is_nan() || u0 <= 0.0 || u >= u0 {
        return power;
    }
    let (p, d, t) = (u0.powf(e), e * u0.powf(e - 1.0), u / u0);
    let (t2, t3) = (t * t, t * t * t);
    ((t3 - 2.0 * t2 + t) * u0 * s0 + (3.0 * t2 - 2.0 * t3) * p + (t3 - t2) * u0 * d).clamp(0.0, 1.0)
}

/// How much a colour belongs to each Selective Color range: reds, yellows, greens, cyans,
/// blues, magentas (by chroma within the hue sector), whites, neutrals and blacks (by lightness).
pub fn selective_color_weights(c: [f32; 3]) -> [f32; 9] {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    let mid = c[0] + c[1] + c[2] - max - min;
    let top = |i: usize| if c[i] >= max { max - mid } else { 0.0 };
    let bottom = |i: usize| if c[i] <= min { mid - min } else { 0.0 };
    [
        top(0),
        bottom(2),
        top(1),
        bottom(0),
        top(2),
        bottom(1),
        ((min - 0.5) * 2.0).max(0.0),
        (1.0 - (max - 0.5).abs() - (min - 0.5).abs()).clamp(0.0, 1.0),
        ((0.5 - max) * 2.0).max(0.0),
    ]
}

/// Photoshop Selective Color: each range shifts the cyan, magenta and yellow ink (1 − R, G, B) by
/// its percentage, plus its black percentage on every ink, weighted by how much the colour belongs
/// to the range. Relative mode scales the shift by the ink already present (so it can't tint pure
/// white). Absolute mode scales it by the room the ink has: adding ink by min(1, 2 × (1 − ink)),
/// removing it by min(1, 2 × ink), so Neutrals Cyan +50 darkens red 64 by 32, not 64 (all 72
/// absolute range × ink cases within 1 level of Photoshop 27.11).
pub fn selective_color(c: [f32; 3], relative: bool, adj: &[[f32; 4]; 9]) -> [f32; 3] {
    let w = selective_color_weights(c);
    let mut delta = [0.0f32; 3];
    for (r, wr) in w.iter().enumerate() {
        if *wr <= 0.0 {
            continue;
        }
        let a = adj[r].map(|v| v / 100.0);
        for i in 0..3 {
            let ink = 1.0 - c[i];
            let d = a[i] + a[3];
            let d = if relative {
                d * ink
            } else if d > 0.0 {
                d * (2.0 * (1.0 - ink)).min(1.0)
            } else {
                d * (2.0 * ink).min(1.0)
            };
            delta[i] += d * wr;
        }
    }
    std::array::from_fn(|i| (c[i] - delta[i]).clamp(0.0, 1.0))
}

/// Photo Filter's linear-light RGB matrix. Photoshop multiplies the pixel's D50 XYZ, relative to
/// white, by the filter colour's (mixed in by `density`): a fixed linear map, so it is folded into
/// one matrix. With `normalize_y` the map keeps luminance Y (32-bit Preserve Luminosity; 8/16-bit
/// documents instead restore the encoded luminosity like the Luminosity blend mode). Fitted on the
/// photoshop corpus `photo-filter.psd`: rgb16 within 0.6/255 everywhere.
pub fn photo_filter_matrix(color: [f32; 3], density: f32, normalize_y: bool) -> [[f32; 3]; 3] {
    let d = if density.is_finite() {
        density.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let xyz = mat3_mul(
        &SRGB_TO_XYZ_D50,
        color.map(|v| srgb_to_linear(v.clamp(0.0, 1.0))),
    );
    let mut s: [f32; 3] = std::array::from_fn(|i| 1.0 - d + d * xyz[i] / D50[i]);
    if normalize_y && s[1] > 1e-6 {
        let y = s[1];
        s = s.map(|v| v / y);
    }
    // XYZ_D50_TO_SRGB · diag(s) · SRGB_TO_XYZ_D50
    std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            (0..3)
                .map(|k| XYZ_D50_TO_SRGB[r][k] * s[k] * SRGB_TO_XYZ_D50[k][c])
                .sum()
        })
    })
}
