//! Liquify data types: the tool enum and the replayable brush stroke.

use super::*;

/// The Liquify tools (Photoshop's left tool strip, minus Hand/Zoom/Face).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LiquifyTool {
    #[default]
    ForwardWarp,
    Reconstruct,
    Smooth,
    TwirlCw,
    TwirlCcw,
    Pucker,
    Bloat,
    PushLeft,
    Freeze,
    Thaw,
    LassoMask,
    /// The Reconstruct… button: scales the whole (unfrozen) field toward zero by `amount` %.
    ReconstructAll,
    /// Mask Options › Mask All / None / Invert All (whole-field freeze edits).
    FreezeAll,
    ThawAll,
    InvertFreeze,
}

impl LiquifyTool {
    pub const ALL: [LiquifyTool; 11] = [
        LiquifyTool::ForwardWarp,
        LiquifyTool::Reconstruct,
        LiquifyTool::Smooth,
        LiquifyTool::TwirlCw,
        LiquifyTool::TwirlCcw,
        LiquifyTool::Pucker,
        LiquifyTool::Bloat,
        LiquifyTool::PushLeft,
        LiquifyTool::Freeze,
        LiquifyTool::Thaw,
        LiquifyTool::LassoMask,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LiquifyTool::ForwardWarp => "Forward Warp",
            LiquifyTool::Reconstruct => "Reconstruct",
            LiquifyTool::Smooth => "Smooth",
            LiquifyTool::TwirlCw => "Twirl Clockwise",
            LiquifyTool::TwirlCcw => "Twirl Counterclockwise",
            LiquifyTool::Pucker => "Pucker",
            LiquifyTool::Bloat => "Bloat",
            LiquifyTool::PushLeft => "Push Left",
            LiquifyTool::Freeze => "Freeze Mask",
            LiquifyTool::Thaw => "Thaw Mask",
            LiquifyTool::LassoMask => "Freeze Lasso",
            LiquifyTool::ReconstructAll => "Reconstruct All",
            LiquifyTool::FreezeAll => "Mask All",
            LiquifyTool::ThawAll => "Mask None",
            LiquifyTool::InvertFreeze => "Invert Mask",
        }
    }

    /// Whole-field operations (no brush, no points).
    pub fn is_global(self) -> bool {
        matches!(
            self,
            LiquifyTool::ReconstructAll
                | LiquifyTool::FreezeAll
                | LiquifyTool::ThawAll
                | LiquifyTool::InvertFreeze
        )
    }

    /// Tools that act while the brush is held still (each recorded point is a dab).
    pub fn is_stationary(self) -> bool {
        !matches!(
            self,
            LiquifyTool::ForwardWarp | LiquifyTool::PushLeft | LiquifyTool::LassoMask
        )
    }
}

fn d_size() -> f64 {
    100.0
}
fn d_density() -> f64 {
    50.0
}
fn d_pressure() -> f64 {
    100.0
}
fn d_rate() -> f64 {
    80.0
}

/// One brush stroke: the tool, its brush options and the points it passed through
/// (`[x, y]` or `[x, y, pressure 0..1]`, document pixels).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiquifyStroke {
    #[serde(default)]
    pub tool: LiquifyTool,
    /// Brush diameter (px).
    #[serde(default = "d_size")]
    pub size: f64,
    /// How far the full effect reaches toward the brush edge, 0 (only the centre; feathered)
    /// ..100 (most of the brush). It always fades to nothing at the edge ([`falloff`]).
    #[serde(default = "d_density")]
    pub density: f64,
    /// Overall strength, 0..100.
    #[serde(default = "d_pressure")]
    pub pressure: f64,
    /// Speed of the stationary tools, 0..100.
    #[serde(default = "d_rate")]
    pub rate: f64,
    #[serde(default)]
    pub points: Vec<Vec<f64>>,
    /// `reconstructAll` only: how much to restore, 0..100. `lassoMask`: 1.0/none freezes the
    /// polygon, 0.0 thaws it.
    #[serde(default)]
    pub amount: Option<f64>,
}

impl LiquifyStroke {
    pub fn new(tool: LiquifyTool, size: f64) -> Self {
        LiquifyStroke {
            tool,
            size,
            density: d_density(),
            pressure: d_pressure(),
            rate: d_rate(),
            points: Vec::new(),
            amount: None,
        }
    }

    /// Validates the stroke (finite numbers, 2 or 3 per point).
    pub fn validate(&self) -> Result<(), String> {
        if !(self.size.is_finite() && self.size >= 1.0 && self.size <= 15000.0) {
            return Err(format!("brush size {} out of range 1..15000", self.size));
        }
        for p in &self.points {
            if !(2..=3).contains(&p.len()) || p.iter().any(|v| !v.is_finite()) {
                return Err("stroke points are [x, y] or [x, y, pressure]".into());
            }
        }
        Ok(())
    }
}
