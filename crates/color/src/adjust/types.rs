//! Data types: the adjustment enum, its parameter structs, and the pixel buffer they act on.

use super::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub fn new(x0: i32, y0: i32, w: i32, h: i32) -> Self {
        Self {
            x0,
            y0,
            x1: x0 + w,
            y1: y0 + h,
        }
    }
    pub fn width(self) -> i32 {
        self.x1 - self.x0
    }
    pub fn height(self) -> i32 {
        self.y1 - self.y0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Buffer {
    pub rect: Rect,
    pub px: Vec<[f32; 4]>,
}

impl Buffer {
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(0, 0, width, height),
            px: vec![[0.0; 4]; (width.max(0) * height.max(0)) as usize],
        }
    }
    pub fn filled(rect: Rect, c: [f32; 4]) -> Self {
        Self {
            rect,
            px: vec![c; (rect.width().max(0) * rect.height().max(0)) as usize],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub input: f32,
    pub output: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelsChannel {
    pub in_black: f32,
    pub in_white: f32,
    pub gamma: f32,
    pub out_black: f32,
    pub out_white: f32,
}

impl Default for LevelsChannel {
    fn default() -> Self {
        Self {
            in_black: 0.0,
            in_white: 1.0,
            gamma: 1.0,
            out_black: 0.0,
            out_white: 1.0,
        }
    }
}

/// Which document channels the per-channel records of Levels and Curves address. Compositing
/// happens in display RGB; `Cmyk` and `Lab` records are applied to the pixel converted into that
/// space (the same conversions the document's surfaces use), so ink and Lab channel edits behave
/// like Photoshop's in CMYK and Lab documents. Grayscale documents use `Rgb` with the master record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToneSpace {
    /// `per_channel` = red, green, blue.
    #[default]
    Rgb,
    /// `per_channel` = cyan, magenta, yellow plus `black`; values are channel brightness
    /// (1 - ink), and the master record applies to all four inks.
    Cmyk,
    /// `per_channel` = lightness, a, b (stored 0..=1 like Lab surfaces); there is no composite
    /// record in Lab, so the master is ignored.
    Lab,
}

/// One of Hue/Saturation's six colour ranges (reds, yellows, greens, cyans, blues, magentas).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HueRange {
    /// Hue shift in degrees (-180..=180).
    pub hue: f32,
    /// Saturation change in percent (-100..=100).
    pub saturation: f32,
    /// Lightness change in percent (-100..=100).
    pub lightness: f32,
    /// Hue degrees where the range starts fading in, is fully on, starts fading out and ends
    /// (Photoshop's four range sliders). May wrap past 0/360.
    pub bounds: [f32; 4],
}

impl HueRange {
    /// The neutral range `i` (0 = reds … 5 = magentas) with Photoshop's default extent: fully on
    /// for 30° around its centre, fading over 30° each side.
    pub fn neutral(i: usize) -> Self {
        let c = 60.0 * i as f32;
        Self {
            hue: 0.0,
            saturation: 0.0,
            lightness: 0.0,
            bounds: [c - 45.0, c - 15.0, c + 15.0, c + 45.0],
        }
    }

    pub fn defaults() -> [HueRange; 6] {
        std::array::from_fn(Self::neutral)
    }

    /// The canonical form of four range sliders: increasing, within one turn, centre in
    /// -30..330 (so the default reds are -45, -15, 15, 45 whichever way they were written).
    pub fn canonical_bounds(b: [f32; 4]) -> [f32; 4] {
        if b.iter().any(|v| !v.is_finite()) {
            return [-45.0, -15.0, 15.0, 45.0];
        }
        let mut out = [b[0]; 4];
        for k in 1..4 {
            out[k] = (out[k - 1] + (b[k] - out[k - 1]).rem_euclid(360.0)).min(out[0] + 359.0);
        }
        let centre = (out[1] + out[2]) / 2.0;
        let shift = (centre + 30.0).rem_euclid(360.0) - 30.0 - centre;
        out.map(|v| v + shift)
    }

    pub fn is_neutral(&self) -> bool {
        self.hue == 0.0 && self.saturation == 0.0 && self.lightness == 0.0
    }

    /// How much a pixel of hue `deg` (degrees) belongs to this range, 0..=1.
    pub fn weight(&self, deg: f32) -> f32 {
        let [a, b, c, d] = self.bounds;
        // Measure from the range's start so wrap-around ranges (reds) work.
        let rel = |x: f32| {
            if x.is_finite() {
                (x - a).rem_euclid(360.0)
            } else {
                0.0
            }
        };
        let b = rel(b);
        let c = rel(c).max(b);
        let d = rel(d).max(c);
        let h = rel(deg);
        if h <= b {
            if b <= 0.0 {
                1.0
            } else {
                h / b
            }
        } else if h <= c {
            1.0
        } else if h <= d {
            if d <= c {
                1.0
            } else {
                (d - h) / (d - c)
            }
        } else {
            0.0
        }
    }
}

pub(super) fn default_hue_ranges() -> [HueRange; 6] {
    HueRange::defaults()
}

/// Photoshop adjustment layers (Layer → New Adjustment Layer).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Adjustment {
    BrightnessContrast {
        brightness: f32,
        contrast: f32,
        legacy: bool,
    },
    /// R, G, B (or the channels `space` names) first, then the master (composite) record.
    /// In CMYK this also applies to `black`; in Lab the master is ignored.
    Levels {
        master: LevelsChannel,
        per_channel: [LevelsChannel; 3],
        #[serde(default)]
        space: ToneSpace,
        /// The black ink channel when `space` is [`ToneSpace::Cmyk`].
        #[serde(default)]
        black: LevelsChannel,
    },
    /// R, G, B curves (or the channels `space` names) first, then the master (composite) curve.
    /// In CMYK this also applies to `black`; in Lab the master is ignored.
    Curves {
        master: Vec<CurvePoint>,
        per_channel: [Vec<CurvePoint>; 3],
        #[serde(default)]
        space: ToneSpace,
        /// The black ink curve when `space` is [`ToneSpace::Cmyk`] (empty = identity).
        #[serde(default)]
        black: Vec<CurvePoint>,
        /// Drawn with the pencil (Photoshop's "Draw to modify the curve", PSD `curv` map form):
        /// every curve is a [`CURVE_MAP_LEN`]-entry map (see [`curve_map`]) evaluated
        /// piecewise-linearly instead of a spline through its points.
        #[serde(default)]
        pencil: bool,
    },
    Exposure {
        exposure: f32,
        offset: f32,
        gamma: f32,
    },
    Vibrance {
        vibrance: f32,
        saturation: f32,
    },
    /// Master hue / saturation / lightness (or the Colorize colour) plus six hue ranges.
    HueSaturation {
        hue: f32,
        saturation: f32,
        lightness: f32,
        colorize: bool,
        #[serde(default = "default_hue_ranges")]
        ranges: [HueRange; 6],
    },
    ColorBalance {
        shadows: [f32; 3],
        midtones: [f32; 3],
        highlights: [f32; 3],
        preserve_luminosity: bool,
    },
    BlackWhite {
        weights: [f32; 6],
        tint: Option<[f32; 3]>,
    },
    PhotoFilter {
        color: [f32; 3],
        density: f32,
        preserve_luminosity: bool,
    },
    ChannelMixer {
        matrix: [[f32; 4]; 3],
        monochrome: bool,
    },
    /// A 3D LUT: `size`³ RGB triplets, red varying fastest (`((b·size + g)·size + r)·3`), in 0..=1.
    /// `lut` is None for a lookup Photoshop stores as an ICC profile (identity here).
    ColorLookup {
        name: String,
        lut: Option<Arc<Vec<f32>>>,
        size: u32,
        /// Tetrahedral instead of trilinear interpolation.
        #[serde(default)]
        tetrahedral: bool,
        /// Ordered dither of ±½ an 8-bit step to hide banding (Photoshop's "Dither").
        #[serde(default)]
        dither: bool,
    },
    Invert,
    Posterize {
        levels: u32,
    },
    Threshold {
        level: f32,
    },
    GradientMap {
        stops: Vec<(f32, [f32; 3])>,
        reverse: bool,
        /// Ordered dither of ±½ an 8-bit step to hide banding.
        #[serde(default)]
        dither: bool,
    },
    /// Per range (reds, yellows, greens, cyans, blues, magentas, whites, neutrals, blacks) the
    /// cyan, magenta, yellow and black change in percent (-100..=100), as in Photoshop.
    SelectiveColor {
        relative: bool,
        adjustments: [[f32; 4]; 9],
    },
    /// A PSD adjustment we can't evaluate yet; preserved raw for round-trip.
    Unsupported {
        psd_key: String,
        raw: Vec<u8>,
    },
}

impl Adjustment {
    pub fn label(&self) -> &str {
        match self {
            Adjustment::BrightnessContrast { .. } => "Brightness/Contrast",
            Adjustment::Levels { .. } => "Levels",
            Adjustment::Curves { .. } => "Curves",
            Adjustment::Exposure { .. } => "Exposure",
            Adjustment::Vibrance { .. } => "Vibrance",
            Adjustment::HueSaturation { .. } => "Hue/Saturation",
            Adjustment::ColorBalance { .. } => "Color Balance",
            Adjustment::BlackWhite { .. } => "Black & White",
            Adjustment::PhotoFilter { .. } => "Photo Filter",
            Adjustment::ChannelMixer { .. } => "Channel Mixer",
            Adjustment::ColorLookup { .. } => "Color Lookup",
            Adjustment::Invert => "Invert",
            Adjustment::Posterize { .. } => "Posterize",
            Adjustment::Threshold { .. } => "Threshold",
            Adjustment::GradientMap { .. } => "Gradient Map",
            Adjustment::SelectiveColor { .. } => "Selective Color",
            Adjustment::Unsupported { .. } => "Adjustment",
        }
    }

    pub fn default_hue_saturation() -> Self {
        Adjustment::HueSaturation {
            hue: 0.0,
            saturation: 0.0,
            lightness: 0.0,
            colorize: false,
            ranges: HueRange::defaults(),
        }
    }

    pub fn identity_curves() -> Self {
        let line = || {
            vec![
                CurvePoint {
                    input: 0.0,
                    output: 0.0,
                },
                CurvePoint {
                    input: 1.0,
                    output: 1.0,
                },
            ]
        };
        Adjustment::Curves {
            master: line(),
            per_channel: [line(), line(), line()],
            space: ToneSpace::Rgb,
            black: Vec::new(),
            pencil: false,
        }
    }

    pub fn identity_levels() -> Self {
        Adjustment::Levels {
            master: LevelsChannel::default(),
            per_channel: Default::default(),
            space: ToneSpace::Rgb,
            black: LevelsChannel::default(),
        }
    }
}

/// The document tone curve used to linearize values for adjustments that
/// work in linear light (Exposure). Until ICC profiles are wired in (M8) this
/// approximates the Photoshop defaults: sRGB for colour documents and the
/// "Dot Gain 20%" grey profile (≈ gamma 1.73, fitted on the corpus) for
/// grayscale documents. 32-bit documents store linear light already.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Transfer {
    /// sRGB piecewise curve.
    Srgb,
    /// Pure power law with this exponent.
    Gamma(f32),
}

impl Transfer {
    /// Default transfer for a document's colour mode and depth. 32-bit samples are linear, and
    /// Photoshop's Exposure works on them as stored (photoshop corpus rgb32 and gray32
    /// exposure.psd: within 0.5/255, against 69/255 through the 2.2 curve).
    pub fn for_document(mode: ColorMode, depth: SampleType) -> Self {
        if depth == SampleType::F32 {
            return Transfer::Gamma(1.0);
        }
        match mode {
            ColorMode::Grayscale | ColorMode::Duotone | ColorMode::Bitmap => Transfer::Gamma(1.732),
            _ => Transfer::Srgb,
        }
    }
    /// The tone curve Exposure linearises through: RGB documents use a pure 2.2 power, not the
    /// sRGB curve (psd-tools adjustment_nested_composition_4: an offset of 0.1738 lifts 76 to 134,
    /// not 136; ag-psd masks), grey documents their dot-gain gamma.
    pub fn for_exposure(self) -> Self {
        match self {
            Transfer::Srgb => Transfer::Gamma(2.2),
            t => t,
        }
    }
    /// Tone-encoded value → linear light.
    pub fn decode(self, v: f32) -> f32 {
        match self {
            Transfer::Srgb => crate::convert::srgb_to_linear(v.max(0.0)),
            Transfer::Gamma(1.0) => v.max(0.0),
            Transfer::Gamma(g) => v.max(0.0).powf(g),
        }
    }
    /// Linear light → tone-encoded value.
    pub fn encode(self, v: f32) -> f32 {
        match self {
            Transfer::Srgb => crate::convert::linear_to_srgb(v.max(0.0)),
            Transfer::Gamma(1.0) => v.max(0.0),
            Transfer::Gamma(g) => v.max(0.0).powf(1.0 / g),
        }
    }
}
