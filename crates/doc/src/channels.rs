//! Alpha channels and the colour-indicates mode.

use crate::*;
use serde::{Deserialize, Serialize};

/// Which areas an alpha channel's overlay colour marks (Channel Options › Color Indicates).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColorIndicates {
    /// Colour shows over black (unselected) pixels: Photoshop's default.
    #[default]
    MaskedAreas,
    /// Colour shows over white (selected) pixels.
    SelectedAreas,
}

/// An extra (alpha or spot) channel, also used for the temporary Quick Mask channel.
///
/// The surface is grayscale at the document depth; white = selected (1.0), black = masked.
#[derive(Clone, Debug, PartialEq)]
pub struct AlphaChannel {
    pub name: String,
    pub surface: Surface,
    /// Spot colour channels carry their ink colour and solidity.
    pub spot: Option<(Color, f32)>,
    /// Overlay colour shown when the channel is viewed together with the composite.
    pub color: Color,
    /// Overlay opacity (0..1).
    pub opacity: f32,
    pub indicates: ColorIndicates,
}

impl AlphaChannel {
    /// Photoshop's default overlay: red at 50% over masked areas.
    pub const DEFAULT_COLOR: Color = Color::rgb(1.0, 0.0, 0.0);

    pub fn new(name: impl Into<String>, surface: Surface) -> Self {
        Self {
            name: name.into(),
            surface,
            spot: None,
            color: Self::DEFAULT_COLOR,
            opacity: 0.5,
            indicates: ColorIndicates::MaskedAreas,
        }
    }

    /// Overlay coverage (0..1, before opacity) for channel value `v`: spot channels show ink
    /// where the channel is white, alpha channels follow [`ColorIndicates`].
    pub fn overlay_coverage(&self, v: f32) -> f32 {
        if self.spot.is_some() || self.indicates == ColorIndicates::SelectedAreas {
            v
        } else {
            1.0 - v
        }
    }
}
