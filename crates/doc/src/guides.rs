//! Ruler guides.

use serde::{Deserialize, Serialize};

/// Ruler guides in document pixels. f64 like the rest of the view geometry: a guide placed in
/// ruler units (1 cm at 300 ppi is 118.110236… px) or read from a PSD's 1/32 px positions keeps
/// its exact place.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Guides {
    pub horizontal: Vec<f64>,
    pub vertical: Vec<f64>,
    /// Guides given their own colour (View › Guides › New Guide… › Color). The rest use
    /// Preferences › Guides, Grid & Slices › Guide colour.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub colors: Vec<GuideColor>,
}

/// One guide's own colour. It is keyed by orientation and position, not by index, so code that
/// drops or reorders guide positions (crop, artboards, guide layouts) can never move a colour onto
/// another guide: a guide whose entry no longer matches simply uses the preference colour.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GuideColor {
    pub vertical: bool,
    pub position: f64,
    /// sRGB, 0–255.
    pub rgb: [u8; 3],
}

impl Guides {
    /// The guide's own colour, if it has one.
    pub fn color(&self, vertical: bool, position: f64) -> Option<[u8; 3]> {
        self.colors
            .iter()
            .find(|c| c.vertical == vertical && c.position == position)
            .map(|c| c.rgb)
    }

    /// Give the guide at `position` its own colour (`None` returns it to the preference colour).
    pub fn set_color(&mut self, vertical: bool, position: f64, rgb: Option<[u8; 3]>) {
        self.colors
            .retain(|c| !(c.vertical == vertical && c.position == position));
        if let Some(rgb) = rgb {
            self.colors.push(GuideColor {
                vertical,
                position,
                rgb,
            });
        }
    }

    /// Drop colours whose guide no longer exists (after a guide moved, was deleted or cleared).
    pub fn prune_colors(&mut self) {
        let (h, v) = (&self.horizontal, &self.vertical);
        self.colors.retain(|c| {
            if c.vertical {
                v.contains(&c.position)
            } else {
                h.contains(&c.position)
            }
        });
    }
}
