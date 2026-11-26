//! Fill-layer content: solid, gradient and pattern fills.

use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Fill {
    Solid(Color),
    /// Gradient between stops at `angle` degrees (`style` geometry, optionally reversed), laid
    /// out in the layer's frame (or the canvas when `align` is off), its centre moved by `offset`.
    Gradient {
        stops: Vec<(f32, Color)>,
        angle: f32,
        scale: f32,
        style: GradientStyle,
        reverse: bool,
        /// Opacity stops `(location, opacity)` (both `0..=1`); empty = each colour stop's alpha.
        /// When set, a pixel's alpha is the colour's alpha times the opacity at that location.
        #[serde(default)]
        opacity_stops: Vec<(f32, f32)>,
        /// Colour midpoints, one per segment between consecutive (sorted) colour stops: where
        /// the segment is half-way (`0..=1` of the segment). Missing entries are 0.5.
        #[serde(default)]
        midpoints: Vec<f32>,
        /// Centre offset as a fraction of the frame (PSD `Ofst`).
        #[serde(default)]
        offset: (f32, f32),
        /// Dither (breaks 8-bit banding with one level of noise).
        #[serde(default)]
        dither: bool,
        /// "Align with layer": lay out in the layer's frame (its masks' bounds), else the canvas.
        #[serde(default = "effects::yes")]
        align: bool,
    },
    /// A pattern from [`Document::patterns`] (looked up by `id`, then `name`).
    Pattern {
        name: String,
        scale: f32,
        #[serde(default)]
        id: String,
        /// Rotation in degrees (counter-clockwise).
        #[serde(default)]
        angle: f32,
        /// "Link with Layer": the pattern origin follows the layer's frame (else the canvas).
        #[serde(default = "effects::yes")]
        link: bool,
        /// Phase (origin offset) in pixels.
        #[serde(default)]
        phase: (f32, f32),
    },
}

impl Fill {
    /// A gradient fill with the classic settings: opaque stops at their alpha, 50 % midpoints,
    /// centred, no dither, aligned with the layer.
    pub fn gradient(
        stops: Vec<(f32, Color)>,
        angle: f32,
        scale: f32,
        style: GradientStyle,
        reverse: bool,
    ) -> Fill {
        Fill::Gradient {
            stops,
            angle,
            scale,
            style,
            reverse,
            opacity_stops: Vec::new(),
            midpoints: Vec::new(),
            offset: (0.0, 0.0),
            dither: false,
            align: true,
        }
    }

    /// This fill with its colours in `mode`'s model (see [`Color::in_mode`]): a fill layer's
    /// colours are in its document's mode, which the compositors read them as.
    pub fn in_mode(mut self, mode: ColorMode) -> Fill {
        match &mut self {
            Fill::Solid(c) => *c = c.in_mode(mode),
            Fill::Gradient { stops, .. } => {
                stops.iter_mut().for_each(|(_, c)| *c = c.in_mode(mode))
            }
            Fill::Pattern { .. } => {}
        }
        self
    }
}
