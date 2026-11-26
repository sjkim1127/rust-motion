//! Puppet Warp data types: modes, mesh density, pins and the warp description.

use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PuppetMode {
    Rigid,
    #[default]
    Normal,
    Distort,
}

impl PuppetMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "rigid" => Some(PuppetMode::Rigid),
            "normal" => Some(PuppetMode::Normal),
            "distort" => Some(PuppetMode::Distort),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PuppetDensity {
    Fewer,
    #[default]
    Normal,
    More,
}

impl PuppetDensity {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "fewer" | "fewerpoints" => Some(PuppetDensity::Fewer),
            "normal" => Some(PuppetDensity::Normal),
            "more" | "morepoints" => Some(PuppetDensity::More),
            _ => None,
        }
    }

    pub(super) fn cells(self) -> f64 {
        match self {
            PuppetDensity::Fewer => 12.0,
            PuppetDensity::Normal => 20.0,
            PuppetDensity::More => 32.0,
        }
    }
}

/// A pin: where it was placed (`src`) and where it is now (`dst`), document pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PuppetPin {
    pub src: [f64; 2],
    pub dst: [f64; 2],
    /// Fixed rotation of the mesh around the pin (degrees); `None` = automatic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate: Option<f64>,
    /// Pin depth: where the mesh overlaps itself, triangles nearest a higher pin draw on top.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub depth: i32,
}

fn is_zero(v: &i32) -> bool {
    *v == 0
}

fn d_expansion() -> f64 {
    2.0
}

/// A complete puppet warp (the command's params and a smart filter's stored data).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PuppetWarp {
    #[serde(default)]
    pub pins: Vec<PuppetPin>,
    #[serde(default)]
    pub mode: PuppetMode,
    #[serde(default)]
    pub density: PuppetDensity,
    /// Grows (or with a negative value shrinks) the mesh beyond the opaque region, px.
    #[serde(default = "d_expansion")]
    pub expansion: f64,
}

impl PuppetWarp {
    pub fn is_identity(&self) -> bool {
        self.pins
            .iter()
            .all(|p| p.src == p.dst && p.rotate.is_none_or(|r| r.rem_euclid(360.0) == 0.0))
    }
}

/// A triangle mesh in document pixels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PuppetMesh {
    pub verts: Vec<[f64; 2]>,
    pub tris: Vec<[usize; 3]>,
    /// Grid spacing (px).
    pub spacing: f64,
}
