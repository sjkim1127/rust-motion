//! Brightness/Contrast: the legacy and the modern (CS3+) curves.

use super::*;

/// Legacy Brightness/Contrast on a 32-bit document's samples: unclipped, with contrast pivoting at
/// 127 of 255 (photoshop corpus rgb32 and gray32 brightness-contrast-legacy.psd: every pixel
/// exact, negative and over-white values included; the 8/16-bit pivot, 128, is 0.43 of a level
/// off there).
pub(super) fn legacy_brightness_contrast_hdr(
    brightness: f32,
    contrast: f32,
) -> impl Fn(f32) -> f32 {
    const PIVOT: f32 = 127.0 / 255.0;
    let b = brightness / 255.0;
    let c = contrast.clamp(-100.0, 99.0);
    let k = if c >= 0.0 {
        1.0 / (1.0 - c / 100.0)
    } else {
        1.0 + c / 100.0
    };
    let (pre, post) = if c >= 0.0 { (b, 0.0) } else { (0.0, b) };
    move |v: f32| (v + pre - PIVOT) * k + PIVOT + post
}

/// Legacy Brightness/Contrast as one channel's tone curve (see [`apply_depth`]).
pub(super) fn legacy_brightness_contrast(brightness: f32, contrast: f32) -> impl Fn(f32) -> f32 {
    legacy_brightness_contrast_at(brightness, contrast, LEGACY_PIVOT)
}

/// [`legacy_brightness_contrast`] with contrast pivoting at `pivot`.
pub(super) fn legacy_brightness_contrast_at(
    brightness: f32,
    contrast: f32,
    pivot: f32,
) -> impl Fn(f32) -> f32 {
    let b = brightness / 255.0;
    let c = contrast.clamp(-100.0, 99.0);
    let k = if c >= 0.0 {
        1.0 / (1.0 - c / 100.0)
    } else {
        1.0 + c / 100.0
    };
    let (pre, post) = if c >= 0.0 { (b, 0.0) } else { (0.0, b) };
    move |v: f32| ((v + pre - pivot) * k + pivot + post).clamp(0.0, 1.0)
}

/// Modern Brightness curve (one channel, `brightness` in [-150, 150]); pins pure black and white,
/// and `b = 0` is the identity. Raised, it is a line of slope `s = 1.375^(b/50)` from the origin
/// up to a knee at output 126/255, then a cubic Hermite to (1, 1) whose end slope is
/// `1 / (1 + 0.084 b)`; past about +110, where no such cubic stays below white, the roll-off is
/// `1 - (1 - y0)(1 - u)^α` instead (continuous slope `α` at the knee, flat at white). Lowered, it
/// is the inverse of the curve raised by as much.
///
/// Fit to every Photoshop measurement we have, all within about a level and a half: its ±50
/// samples (`modern_brightness_matches_photoshop`), the photoshop corpus's brightness-contrast.psd
/// (+25 with contrast +30) and psd-tools' brightnesscontrast_rgb.psd, whose masked layers give
/// +23, -31 and -95 (+150 within 6 levels there, where every curve family tried bottoms out).
pub fn modern_brightness(v: f32, brightness: f32) -> f32 {
    if brightness == 0.0 || !brightness.is_finite() {
        return v;
    }
    Brightness::new(brightness).eval(v)
}

/// [`modern_brightness`] for one slider value, its curve's constants worked out once.
pub(super) struct Brightness {
    raise: bool,
    s: f32,
    e: f32,
    x0: f32,
    /// The knee's slope over the roll-off's secant (`α` of Fritsch–Carlson); a power roll-off
    /// replaces the Hermite when α² + β² > 9.
    alpha: f32,
    hermite: bool,
}

/// Output level of the Brightness curve's knee.
pub(super) const BRIGHTNESS_KNEE: f32 = 126.0 / 255.0;

impl Brightness {
    fn new(brightness: f32) -> Self {
        let b = brightness.abs();
        let s = 1.375f32.powf(b / 50.0);
        let e = 1.0 / (1.0 + 0.084 * b);
        let x0 = (BRIGHTNESS_KNEE / s).min(1.0);
        let secant = (1.0 - BRIGHTNESS_KNEE) / (1.0 - x0).max(1e-6);
        let (alpha, beta) = (s / secant, e / secant);
        Brightness {
            raise: brightness > 0.0,
            s,
            e,
            x0,
            alpha,
            hermite: alpha * alpha + beta * beta <= 9.0,
        }
    }

    fn eval(&self, v: f32) -> f32 {
        let v = v.clamp(0.0, 1.0);
        if self.raise {
            return self.raised(v);
        }
        if v <= 0.0 || v >= 1.0 {
            return v;
        }
        // The inverse of the raised curve (monotone on [0, 1]), by bisection.
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..32 {
            let mid = 0.5 * (lo + hi);
            if self.raised(mid) < v {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    /// The curve raised: the line, then a monotone roll-off to (1, 1).
    fn raised(&self, x: f32) -> f32 {
        let Brightness {
            s,
            e,
            x0,
            alpha,
            hermite,
            ..
        } = *self;
        let y0 = BRIGHTNESS_KNEE;
        if x <= x0 {
            return y0.min(s * x);
        }
        let h = (1.0 - x0).max(1e-6);
        let t = ((x - x0) / h).clamp(0.0, 1.0);
        let y = if hermite {
            let (t2, t3) = (t * t, t * t * t);
            (2.0 * t3 - 3.0 * t2 + 1.0) * y0
                + (t3 - 2.0 * t2 + t) * h * s
                + (-2.0 * t3 + 3.0 * t2)
                + (t3 - t2) * h * e
        } else {
            1.0 - (1.0 - y0) * (1.0 - t).powf(alpha)
        };
        y.clamp(0.0, 1.0)
    }
}

/// Modern Brightness/Contrast as one channel's lookup table (4096 samples over 0..1):
/// [`modern_brightness`] then [`modern_contrast`], as Photoshop applies them.
pub fn modern_lut(brightness: f32, contrast: f32) -> Vec<f32> {
    let step = |k: usize, n: usize| k as f32 / (n - 1) as f32;
    let bright: Vec<f32> = if brightness == 0.0 || !brightness.is_finite() {
        (0..LUT_SIZE).map(|k| step(k, LUT_SIZE)).collect()
    } else if brightness > 0.0 {
        let curve = Brightness::new(brightness);
        (0..LUT_SIZE)
            .map(|k| curve.raised(step(k, LUT_SIZE)))
            .collect()
    } else {
        // Lowered: invert the raised curve, sampled four times finer, in one monotone pass.
        let curve = Brightness::new(-brightness);
        let fine = 4 * LUT_SIZE;
        let ys: Vec<f32> = (0..fine).map(|j| curve.raised(step(j, fine))).collect();
        let mut j = 0;
        (0..LUT_SIZE)
            .map(|k| {
                let y = step(k, LUT_SIZE);
                while j + 2 < fine && ys.get(j + 1).is_some_and(|&n| n < y) {
                    j += 1;
                }
                let (y0, y1) = (
                    ys.get(j).copied().unwrap_or(0.0),
                    ys.get(j + 1).copied().unwrap_or(1.0),
                );
                let f = if y1 > y0 {
                    ((y - y0) / (y1 - y0)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (step(j, fine) + f / (fine - 1) as f32).clamp(0.0, 1.0)
            })
            .collect()
    };
    bright
        .into_iter()
        .map(|b| modern_contrast(b, contrast).clamp(0.0, 1.0))
        .collect()
}

/// Modern Contrast curve (one channel, `contrast` in [-50, 100]). A symmetric cubic-Hermite S-curve
/// pivoting at 0.5: endpoint slope `1 - c/128`, centre slope `1 + c/128`. Near-exact vs Photoshop
/// (≤0.5/255). `c = 0` is the identity; pins 0, 0.5 and 1.
pub fn modern_contrast(v: f32, contrast: f32) -> f32 {
    if contrast == 0.0 {
        return v;
    }
    let k = contrast / 128.0;
    let (end, mid) = (1.0 - k, 1.0 + k);
    // Cubic Hermite on [0, 0.5]: pinned (0,0) slope `end`, (0.5,0.5) slope `mid`. `t` in [0,1].
    let half = |t: f32| {
        end * 0.5 * (t * t * t - 2.0 * t * t + t)
            + (-2.0 * t * t * t + 3.0 * t * t) * 0.5
            + mid * 0.5 * (t * t * t - t * t)
    };
    let v = v.clamp(0.0, 1.0);
    if v <= 0.5 {
        half(v / 0.5)
    } else {
        1.0 - half((1.0 - v) / 0.5)
    }
}

/// Legacy Brightness/Contrast scales contrast around 128 of 255 (contrast 80 keeps 128 at 128,
/// measured on Photoshop 27.11; a 0.5 pivot moved it to 130).
pub const LEGACY_PIVOT: f32 = 128.0 / 255.0;
