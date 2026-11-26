//! How far each effect reads around an output pixel.

use super::*;
use crate::artistic::{GalleryEffect, GalleryFilter as F};

/// Pixels an effect reads around an output pixel.
pub(crate) fn effect_reach(e: &GalleryEffect) -> i32 {
    let g = |k: &str| e.get(k);
    match e.filter {
        F::ColoredPencil => 2,
        F::Cutout => gs(cutout_sigma(e)),
        F::DryBrush | F::Fresco => kuwahara_reach(1 + g("brushSize") as usize) + gs(1.0) + 2,
        F::FilmGrain
        | F::DiffuseGlow
        | F::ChalkCharcoal
        | F::Reticulation
        | F::Grain
        | F::MosaicTiles
        | F::Texturizer => 0,
        F::RoughPastels => gs(0.6 + 1.2) + ((2.0 + g("strokeLength")) / 2.0) as i32 + 3,
        F::NeonGlow => gs(g("glowSize").abs() * 0.6 + 0.5),
        F::PaintDaubs => kuwahara_reach(daubs_radius(e)) + gs(1.5) + 1,
        F::PaletteKnife => {
            kuwahara_reach((g("strokeSize") / 3.0).max(1.0) as usize) + gs(g("softness") * 0.5)
        }
        F::PlasticWrap => gs(plastic_sigma(e)) + 2,
        F::PosterEdges => gs(0.6 + g("edgeThickness") * 0.6) + 2,
        F::SmudgeStick => (g("strokeLength") * 1.5) as i32 + 2,
        F::Sponge => kuwahara_reach(1 + g("brushSize") as usize),
        F::Underpainting => gs(0.5 + g("brushSize") * 0.4),
        F::Watercolor => kuwahara_reach(watercolor_radius(e)) + 2,
        F::AccentedEdges => gs(0.4 + g("smoothness") * 0.3) + gs(g("edgeWidth") * 0.5) + 2,
        F::AngledStrokes => gs(0.8) + (g("strokeLength").max(3.0) / 2.0) as i32 + gs(1.0) + 3,
        F::Crosshatch => (g("strokeLength") / 3.0) as i32 + gs(1.0) + 2,
        F::DarkStrokes => 9,
        F::InkOutlines => (g("strokeLength") / 2.0) as i32 + 3,
        F::Spatter => (g("sprayRadius") * 1.5).ceil() as i32 + 2,
        F::SprayedStrokes => g("sprayRadius").ceil() as i32 + (g("strokeLength") / 2.0) as i32 + 3,
        F::SumiE => gs(g("strokeWidth") * 0.2) + (g("strokeWidth") / 2.0) as i32 + 2,
        F::Glass => (g("distortion") * 2.0).ceil() as i32 + 2,
        F::OceanRipple => (g("rippleMagnitude") * 0.9).ceil() as i32 + 2,
        F::BasRelief => gs(basrelief_sigma(e)) + 2,
        F::Charcoal => gs(1.0) + 2,
        F::Chrome => gs(1.0 + g("smoothness") * 0.4) + 2,
        F::ConteCrayon => 0,
        F::GraphicPen => (g("strokeLength") / 2.0) as i32 + 2,
        F::HalftonePattern => gs(0.5 + g("size") * 0.3),
        F::NotePaper => gs(1.0) + gs(1.2) + 2,
        F::Photocopy => gs(1.0 + g("detail") * 0.5),
        F::Plaster => gs(0.5 + g("smoothness") * 0.8) + gs(1.5) + 2,
        F::Stamp => gs(0.3 + g("smoothness") * 0.3),
        F::TornEdges => gs(1.0),
        F::WaterPaper => 2 * (g("fiberLength") / 6.0) as i32 + 2,
        F::GlowingEdges => gs(0.3 + g("smoothness") * 0.25) + gs(g("edgeWidth") * 0.4) + 2,
        F::Craquelure => 0,
        F::Patchwork => patch_size(e).ceil() as i32 + 2,
        F::StainedGlass => (glass_cell(e) * 2.5).ceil() as i32 + 2,
    }
}

pub(super) fn cutout_sigma(e: &GalleryEffect) -> f32 {
    0.5 + e.get("edgeSimplicity") * (1.1 - 0.3 * e.get("edgeFidelity"))
}

pub(super) fn daubs_radius(e: &GalleryEffect) -> usize {
    let wide = matches!(e.choice("brushType"), "wideSharp" | "wideBlurry");
    let r = e.get("brushSize") / 2.0 * if wide { 1.5 } else { 1.0 };
    r.max(1.0) as usize
}

pub(super) fn plastic_sigma(e: &GalleryEffect) -> f32 {
    0.5 + e.get("smoothness") * 0.5 + (15.0 - e.get("detail")) * 0.2
}

pub(super) fn watercolor_radius(e: &GalleryEffect) -> usize {
    1 + ((15.0 - e.get("brushDetail")) / 2.5) as usize
}

pub(super) fn basrelief_sigma(e: &GalleryEffect) -> f32 {
    0.3 + e.get("smoothness") * 0.5 + (15.0 - e.get("detail")) * 0.12
}

pub(super) fn patch_size(e: &GalleryEffect) -> f32 {
    3.0 + e.get("squareSize") * 1.5
}

pub(super) fn glass_cell(e: &GalleryEffect) -> f32 {
    e.get("cellSize") * 1.6 + 1.0
}

/// Total reach of a stack of effects.
pub(crate) fn reach(effects: &[GalleryEffect]) -> i32 {
    effects.iter().map(effect_reach).sum()
}
