//! Hue/Saturation, Black & White and Vibrance.

use super::*;

/// Hue/Saturation's range edits as three hue-indexed tables (hue shift in degrees, saturation and
/// lightness as fractions), sampled at hue `k / (len - 1)` turns. Ranges add up where they overlap.
pub fn hue_range_tables(ranges: &[HueRange; 6]) -> [Vec<f32>; 3] {
    let at = |k: usize| {
        let deg = k as f32 / (LUT_SIZE - 1) as f32 * 360.0;
        ranges
            .iter()
            .filter(|r| !r.is_neutral())
            .fold([0.0f32; 3], |acc, r| {
                let w = r.weight(deg);
                [
                    acc[0] + w * r.hue,
                    acc[1] + w * r.saturation / 100.0,
                    acc[2] + w * r.lightness / 100.0,
                ]
            })
    };
    let rows: Vec<[f32; 3]> = (0..LUT_SIZE).map(at).collect();
    std::array::from_fn(|i| rows.iter().map(|r| r[i]).collect())
}

/// One Hue/Saturation evaluation: hue shift in degrees, saturation and lightness in -1..=1.
/// Colorize sets the hue and saturation instead of shifting them; at 0% it leaves only the
/// lightness, as in Photoshop (#2326). Ticking Colorize in the dialog starts saturation at 25%.
pub fn hue_saturation(c: [f32; 3], hue: f32, s: f32, l: f32, colorize: bool) -> [f32; 3] {
    let lighten = |v: f32| {
        if l > 0.0 {
            v + (1.0 - v) * l
        } else {
            v * (1.0 + l)
        }
    };
    if colorize {
        // Colorize builds the colour at the lightened (or darkened) lightness, so black with
        // Lightness +30 becomes a 30 % lightness colour, not grey (measured in Photoshop 27.11).
        let (_, _, ll) = rgb_to_hsl(c);
        return hsl_to_rgb(hue.rem_euclid(360.0) / 360.0, s.abs().min(1.0), lighten(ll));
    }
    // Photoshop applies Lightness first, then rotates the hue and scales the saturation of the
    // result: Hue +40, Saturation +25, Lightness -10 turns (240, 160, 30) into (173, 243, 0), not
    // (168, 230, 14) (Photoshop 27.11 corpus file adjustments/rgb8/hue-saturation).
    let (hh, ss, ll) = rgb_to_hsl(c.map(lighten));
    let hh = (hh + hue / 360.0).rem_euclid(1.0);
    // Raising saturation divides by 1 - s, lowering it multiplies by 1 + s: +50 % doubles it
    // (measured in Photoshop 27.11); +100 % saturates any colour that has some.
    let ss = if s > 0.0 {
        ss / (1.0 - s).max(1.0 / 512.0)
    } else {
        ss * (1.0 + s)
    }
    .clamp(0.0, 1.0);
    hsl_to_rgb(hh, ss, ll)
}

/// A colour range's Lightness, applied after the master edits. It moves each channel towards the
/// pixel's brightest channel when raised and towards its darkest when lowered, not towards white
/// or black: Blues +50 turns (0, 29, 115) into (57, 71, 115), Blues -50 turns (102, 128, 204) into
/// (102, 115, 153) (measured in Photoshop 27.11).
pub fn range_lightness(c: [f32; 3], l: f32) -> [f32; 3] {
    let l = l.clamp(-1.0, 1.0);
    let target = if l > 0.0 {
        c[0].max(c[1]).max(c[2])
    } else {
        c[0].min(c[1]).min(c[2])
    };
    c.map(|v| v + (target - v) * l.abs())
}

/// Black & White grey value. Each colour is the sum of a grey part (its smallest channel), a
/// secondary-colour part (yellow, cyan or magenta: middle − smallest) and a primary part (red,
/// green or blue: largest − middle); the sliders (percent, Photoshop defaults 40 60 40 60 20 80 for
/// reds, yellows, greens, cyans, blues, magentas) weight the two colour parts.
pub fn black_white_gray(c: [f32; 3], weights: &[f32; 6]) -> f32 {
    let (r, g, b) = (c[0], c[1], c[2]);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let mid = r + g + b - max - min;
    // Primary: the largest channel; secondary: the two largest channels together.
    let primary = if r >= g && r >= b {
        0
    } else if g >= b {
        2
    } else {
        4
    };
    let secondary = if b <= r && b <= g {
        1 // red + green = yellow
    } else if r <= g {
        3 // green + blue = cyan
    } else {
        5 // red + blue = magenta
    };
    (min + (mid - min) * weights[secondary] / 100.0 + (max - mid) * weights[primary] / 100.0)
        .clamp(0.0, 1.0)
}

/// Photoshop's Vibrance on one pixel (`v`, `s`: the sliders / 100; `hi`: the sample ceiling).
///
/// Measured black-box against Photoshop 25.4 on 7,400-colour charts (8- and 16-bit; sRGB, Adobe RGB
/// and ProPhoto documents). Both sliders work in linear light through the document's tone curve,
/// Vibrance first, then Saturation:
/// - Saturation moves each colour away from (or towards) the grey `0.288 R + 0.712 G` (blue has no
///   weight, in every profile) by `1 + s`, clipping each channel: within 0.07/255 at every `s`.
/// - Vibrance is an HSV change in linear light that keeps the hue: see [`vibrance_lin`].
pub fn vibrance_px(c: [f32; 3], v: f32, s: f32, transfer: Transfer, hi: f32) -> [f32; 3] {
    let mut l = c.map(|x| transfer.decode(x));
    if v != 0.0 {
        l = vibrance_lin(l, v);
    }
    if s != 0.0 {
        let g = VIBRANCE_GREY[0] * l[0] + VIBRANCE_GREY[1] * l[1];
        l = l.map(|x| (g + (1.0 + s) * (x - g)).clamp(0.0, hi));
    }
    l.map(|x| transfer.encode(x.clamp(0.0, hi)))
}

/// Weights of the grey Photoshop's Vibrance › Saturation desaturates to (R, G; blue has none).
pub const VIBRANCE_GREY: [f32; 2] = [0.288, 0.712];

/// Positive Vibrance's saturation boost, fitted to the measurements (see [`vibrance_lin`]).
pub const VIBRANCE_BOOST: [f32; 7] = [0.5001, 0.0887, 0.4374, 0.7573, -0.8113, 0.5154, 2.8026];

/// The Vibrance slider (`v` in [-1, 1]) on a linear-light pixel. With `t = 1 - min/max` (HSV
/// saturation) and `V = max`:
/// - below 0 (`a = -v`): `t' = t (1 - a/4) (1 - a + a t (1 + t) / 2)` and
///   `V' = V - a t (1 - t) (2 - t + t²) V (1 - V)`; exact (0.2/255 mean) except in the deepest
///   shadows (`V` under 0.05), which Photoshop desaturates less.
/// - above 0: `V' = V + v/4 t (1 - t) (2 - t + t²) V (1 - V)` (exact) and a saturation boost that
///   grows as `t` and `V` fall, damped on reds and skin (HSV hue 0–30°, fading out by 45° and in
///   from 300°): a fit, 0.4 (+10) to 5 (+100) /255 off on average.
pub fn vibrance_lin(c: [f32; 3], v: f32) -> [f32; 3] {
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    if mx <= 0.0 || mx - mn <= 1e-9 {
        return c;
    }
    let t = 1.0 - mn / mx;
    let shape = t * (1.0 - t) * (2.0 - t + t * t) * mx * (1.0 - mx).max(0.0);
    let (t2, v2) = if v < 0.0 {
        let a = -v;
        (
            t * (1.0 - a / 4.0) * (1.0 - a + a * t * (1.0 + t) / 2.0),
            mx - a * shape,
        )
    } else {
        let [c0, c1, p, q, r0, ra, rt] = VIBRANCE_BOOST;
        let w = vibrance_skin(hsv_hue(c, mx, mn));
        let a = v * (1.0 - w) + 0.64 * v.powf(1.6) * w;
        let boost = (c0 * a + c1 * a * a)
            * t.powf(p)
            * (1.0 - t).powf(q)
            * ((r0 + ra * a + rt * (1.0 - t)) * (1.0 - mx).max(0.0)).exp();
        ((t * (1.0 + boost)).min(1.0), mx + v / 4.0 * shape)
    };
    let m2 = v2 * (1.0 - t2);
    let k = (v2 - m2) / (mx - mn);
    c.map(|x| m2 + (x - mn) * k)
}

/// How much positive Vibrance treats a hue (degrees) as skin: 1 on 0–30°, fading to 0 by 45° and
/// in again from 300°.
pub(super) fn vibrance_skin(h: f32) -> f32 {
    if h <= 30.0 {
        1.0
    } else if h < 45.0 {
        (45.0 - h) / 15.0
    } else if h < 300.0 {
        0.0
    } else {
        (h - 300.0) / 60.0
    }
}

/// HSV hue in degrees of `c` (its max `mx` and min `mn` given, `mx > mn`).
pub(super) fn hsv_hue(c: [f32; 3], mx: f32, mn: f32) -> f32 {
    let d = mx - mn;
    let h = if mx == c[0] {
        ((c[1] - c[2]) / d).rem_euclid(6.0)
    } else if mx == c[1] {
        (c[2] - c[0]) / d + 2.0
    } else {
        (c[0] - c[1]) / d + 4.0
    };
    h * 60.0
}

pub fn rgb_to_hsl(c: [f32; 3]) -> (f32, f32, f32) {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    let l = (max + min) / 2.0;
    if (max - min).abs() < 1e-7 {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == c[0] {
        ((c[1] - c[2]) / d).rem_euclid(6.0)
    } else if max == c[1] {
        (c[2] - c[0]) / d + 2.0
    } else {
        (c[0] - c[1]) / d + 4.0
    };
    (h / 6.0, s, l)
}

pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    if s <= 0.0 {
        return [l; 3];
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let f = |mut t: f32| {
        t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0)]
}
