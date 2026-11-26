//! Colour range sampling, hue families, skin tones and tonal ranges.

/// Colour Range: coverage falls off linearly with the per-channel colour
/// distance; `fuzziness` in levels (0–200).
pub fn color_range(px: &[[f32; 4]], color: [f32; 3], fuzziness: f32) -> Vec<f32> {
    let f = fuzziness.max(0.0) + 1.0;
    px.iter()
        .map(|p| {
            if p[3] <= 0.0 {
                return 0.0;
            }
            let d = (0..3)
                .map(|c| (p[c] - color[c]).abs() * 255.0)
                .fold(0.0f32, f32::max);
            (1.0 - d / f).clamp(0.0, 1.0)
        })
        .collect()
}

/// One Color Range sample (Sampled Colors): an eyedropper colour, and where it was picked
/// (needed for Localized Color Clusters; area-local pixel coordinates).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeSample {
    pub color: [f32; 3],
    pub at: Option<(f32, f32)>,
}

/// Color Range › Sampled Colors with several samples (the dialog's "Add to Sample" eyedropper):
/// each pixel takes its best match over the samples, each judged like [`color_range`]. With
/// `localized` (Localized Color Clusters) a sample also fades with distance from where it was
/// picked, reaching 0 at `localized` pixels, so only nearby matching colours are selected;
/// samples without a position aren't localized. `w` is the row width of `px`.
pub fn color_range_samples(
    px: &[[f32; 4]],
    w: usize,
    samples: &[RangeSample],
    fuzziness: f32,
    localized: Option<f32>,
) -> Vec<f32> {
    let f = fuzziness.max(0.0) + 1.0;
    px.iter()
        .enumerate()
        .map(|(i, &[r, g, b, a])| {
            if a <= 0.0 {
                return 0.0;
            }
            let (x, y) = ((i % w.max(1)) as f32, (i / w.max(1)) as f32);
            samples.iter().fold(0.0f32, |best, s| {
                let d = [r, g, b]
                    .iter()
                    .zip(&s.color)
                    .map(|(v, c)| (v - c).abs() * 255.0)
                    .fold(0.0f32, f32::max);
                let mut k = (1.0 - d / f).clamp(0.0, 1.0);
                if let (Some(radius), Some((sx, sy))) = (localized, s.at) {
                    let dist = ((x - sx).powi(2) + (y - sy).powi(2)).sqrt();
                    k *= (1.0 - dist / radius.max(1.0)).clamp(0.0, 1.0);
                }
                best.max(k)
            })
        })
        .collect()
}

/// Hue (degrees, 0..360) and chroma (max − min) of an RGB colour.
fn hue_chroma([r, g, b]: [f32; 3]) -> (f32, f32) {
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let d = mx - mn;
    if d <= 0.0 {
        return (0.0, 0.0);
    }
    let h = if mx == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, d)
}

/// Color Range › Select: Reds, Yellows, Greens, Cyans, Blues or Magentas (`center` = 0, 60,
/// 120, 180, 240, 300°). A pixel belongs to a family by how close its hue is (full at the
/// family's hue, none 60° away, so an orange is half red, half yellow) times its chroma
/// (max − min), so neutrals are never selected and muted colours only partly. Darker colours
/// weigh more, by `2 − max`: measured against Photoshop 2025's presets on 126 colours each for
/// Reds, Yellows and Blues (hue ±40°, saturation 5..100 %, brightness 33..100 %), this matches
/// its selection to within one level, where chroma alone fell up to 61 levels short (#5243).
pub fn hue_range(px: &[[f32; 4]], center: f32) -> Vec<f32> {
    px.iter()
        .map(|&[r, g, b, a]| {
            if a <= 0.0 {
                return 0.0;
            }
            let rgb = [r, g, b].map(|v| v.clamp(0.0, 1.0));
            let (h, chroma) = hue_chroma(rgb);
            let max = rgb[0].max(rgb[1]).max(rgb[2]);
            let dh = (h - center).rem_euclid(360.0);
            let dh = dh.min(360.0 - dh);
            ((1.0 - dh / 60.0).clamp(0.0, 1.0) * chroma * (2.0 - max)).clamp(0.0, 1.0)
        })
        .collect()
}

/// Color Range › Select: Skin Tones. Skin of every complexion falls in a compact region of the
/// YCbCr chroma plane (Chai & Ngan, "Face segmentation using skin-color map", 1999: Cb about
/// 77..127, Cr about 133..173, Rec. 601 on 0..255) that runs from pale (higher Cb, lower Cr) to
/// saturated (lower Cb, higher Cr). Pixels inside an ellipse along that run are fully selected;
/// outside, selection falls off linearly over `fuzziness` (0..200, as the dialog's Fuzziness: 100
/// is one ellipse radius). Pale neutrals such as cream or beige sit just off its pale end, so they
/// stay out. Near-black and near-white pixels, whose chroma is noise, are faded out.
pub fn skin_tones(px: &[[f32; 4]], fuzziness: f32) -> Vec<f32> {
    const CB: f32 = 104.0;
    const CR: f32 = 158.0;
    /// Radii along the run and across it, and the run's angle from the Cb axis.
    const ALONG: f32 = 22.0;
    const ACROSS: f32 = 14.0;
    const ANGLE: f32 = -40.0;
    let (sin, cos) = ANGLE.to_radians().sin_cos();
    let ramp = (fuzziness / 100.0).max(1e-3);
    px.iter()
        .map(|&[r, g, b, a]| {
            if a <= 0.0 {
                return 0.0;
            }
            let [r, g, b] = [r, g, b].map(|v| v.clamp(0.0, 1.0) * 255.0);
            let y = 0.299 * r + 0.587 * g + 0.114 * b;
            let cb = 128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b - CB;
            let cr = 128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b - CR;
            let d = ((cos * cb + sin * cr) / ALONG).hypot((cos * cr - sin * cb) / ACROSS);
            let chroma = (1.0 - (d - 1.0) / ramp).clamp(0.0, 1.0);
            let dark = ((y - 25.0) / 30.0).clamp(0.0, 1.0);
            let bright = ((250.0 - y) / 20.0).clamp(0.0, 1.0);
            chroma * dark * bright
        })
        .collect()
}

/// Color Range › Select: Highlights, Midtones or Shadows. Pixels whose luminance (Rec. 601
/// weights, 0..255) lies in `lo..=hi` are fully selected; outside, selection falls off linearly
/// to 0 over `falloff` levels (the dialog's Fuzziness).
pub fn tone_range(px: &[[f32; 4]], lo: f32, hi: f32, falloff: f32) -> Vec<f32> {
    let ramp = falloff.max(1e-3);
    px.iter()
        .map(|&[r, g, b, a]| {
            if a <= 0.0 {
                return 0.0;
            }
            let l = (0.299 * r + 0.587 * g + 0.114 * b).clamp(0.0, 1.0) * 255.0;
            let out = if l < lo {
                lo - l
            } else if l > hi {
                l - hi
            } else {
                0.0
            };
            (1.0 - out / ramp).clamp(0.0, 1.0)
        })
        .collect()
}
