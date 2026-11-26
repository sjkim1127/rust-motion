//! The layer tree: masks, groups, layer content and the layer itself.

use crate::*;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct LayerMask {
    /// Grayscale surface; untouched pixels read as `default` (0 = hide, 1 = reveal).
    pub surface: Surface,
    pub enabled: bool,
    pub linked: bool,
    pub density: f32,
    pub feather: f32,
}

impl LayerMask {
    pub fn reveal_all() -> Self {
        Self {
            surface: Surface::with_default(PixelFormat::GRAY8, &[1.0]),
            enabled: true,
            linked: true,
            density: 1.0,
            feather: 0.0,
        }
    }
    pub fn hide_all() -> Self {
        Self {
            surface: Surface::with_default(PixelFormat::GRAY8, &[0.0]),
            ..Self::reveal_all()
        }
    }
    /// Effective mask value at a pixel, including density.
    pub fn value(&self, x: i32, y: i32) -> f32 {
        if !self.enabled {
            return 1.0;
        }
        let v = self.surface.sample_channel(x, y, 0);
        1.0 - self.density * (1.0 - v)
    }

    /// Effective mask values over `r` (row-major) into `out`, including
    /// density; all ones when disabled.
    pub fn values_into(&self, r: Rect, out: &mut Vec<f32>) {
        let n = r.width() as usize * r.height() as usize;
        if !self.enabled {
            out.clear();
            out.resize(n, 1.0);
            return;
        }
        self.surface.read_region_into(r, out);
        let ch = self.surface.channels();
        if ch > 1 {
            let v: Vec<f32> = out.chunks_exact(ch).map(|p| p[0]).collect();
            *out = v;
        }
        if self.density < 1.0 {
            for v in out.iter_mut() {
                *v = 1.0 - self.density * (1.0 - *v);
            }
        }
    }
}

/// Layer styles. Parsed PSD effect data is kept raw until the effects engine lands (M9).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Effects {
    pub enabled: bool,
    pub items: Vec<Effect>,
    /// Original PSD `lfx2` (`lfxs` on groups) block data (object-effects version + descriptor),
    /// written back verbatim on PSD export. Not duplicated in `Layer::psd_blocks`.
    pub psd_raw: Option<Arc<Vec<u8>>>,
    /// Effects reference point (PSD `fxrp`): where linked patterns (Pattern Overlay, pattern
    /// strokes and glows) anchor their tiling. It moves with the layer. `None` = the layer's
    /// top-left.
    pub reference: Option<(f64, f64)>,
}

/// Deepest group nesting a document may hold: a layer inside this many nested groups is the
/// deepest legal one. Everything that walks the layer tree (engine lookups, the layers panel,
/// PSD export, `.pcraft` save and load) recurses once per level, so deeper trees risk
/// overflowing the 1 MiB main-thread stacks of Windows and wasm. PSD import and the engine
/// commands that nest layers (`layer.groupLayers`, `layer.moveTo`, Artboard from Layers) enforce
/// this up front; `photocraft-format` refuses to save deeper trees.
pub const MAX_GROUP_DEPTH: usize = 100;

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub children: Vec<Layer>,
    pub expanded: bool,
    /// Set when this (top-level) group is an artboard: children are clipped to its rect.
    pub artboard: Option<Artboard>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum LayerContent {
    Raster(Surface),
    Group(Group),
    Adjustment(Adjustment),
    Fill(Fill),
    Text(TextLayer),
    Shape(ShapeLayer),
    Smart(SmartObject),
}

impl LayerContent {
    pub fn kind_name(&self) -> &'static str {
        match self {
            LayerContent::Raster(_) => "Pixel",
            LayerContent::Group(_) => "Group",
            LayerContent::Adjustment(_) => "Adjustment",
            LayerContent::Fill(_) => "Fill",
            LayerContent::Text(_) => "Type",
            LayerContent::Shape(_) => "Shape",
            LayerContent::Smart(_) => "Smart Object",
        }
    }

    /// English indefinite article for this layer kind ("an" for Adjustment, "a" for all others).
    pub fn article(&self) -> &'static str {
        match self {
            LayerContent::Adjustment(_) => "an",
            _ => "a",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub locks: Locks,
    pub blend: BlendMode,
    /// Layer opacity (applies to content and effects).
    pub opacity: f32,
    /// Fill opacity (applies to content but not effects).
    pub fill_opacity: f32,
    /// Clipped to the layer below (clipping mask).
    pub clipped: bool,
    pub mask: Option<LayerMask>,
    /// Vector mask (Layer › Vector Mask), applied together with `mask`. Shape layers keep their
    /// outline in [`ShapeLayer::path`] instead.
    pub vector_mask: Option<VectorMask>,
    pub effects: Effects,
    pub label: LabelColor,
    pub content: LayerContent,
    /// Preserved PSD additional-layer-info blocks (key, data) for lossless
    /// round-trip of anything not modelled above: vector masks, blending
    /// options, layer version, and the original adjustment/fill blocks (the
    /// PSD writer reuses an adjustment or fill block verbatim while it still
    /// decodes to the layer's current parameters, so Levels/Curves/Hue-Sat
    /// extras survive). Keys regenerated from the fields above (`luni`,
    /// `lyid`, `lsct`, `lsdk`, `iOpa`, `lspf`, `lclr`, `lfx2`) are not stored.
    pub psd_blocks: Vec<([u8; 4], Arc<Vec<u8>>)>,
    /// PSD layer id (`lyid`), kept so ids stay stable across round trips.
    /// Cleared by [`Layer::duplicate`].
    pub psd_id: Option<u32>,
    /// Photoshop-rendered pixels for fill layers (see [`FillCache`]).
    pub fill_cache: Option<FillCache>,
    /// Layer › Link Layers: layers sharing a link group move together. `None` = not linked.
    pub link_group: Option<u64>,
    /// Blending Options › Advanced Blending › Channels: bit `i` set = colour channel `i` of the
    /// document's mode (R, G, B / C, M, Y, K / L, a, b / Gray) is left out of blending, so the
    /// backdrop's value is kept there. 0 = every channel blends (the default). PSD `brst`.
    pub excluded_channels: u32,
    /// Blending Options › Blend If: value ranges of this layer and of the layers beneath it
    /// outside which the layer's pixels are hidden. Default = everything blends. PSD layer-record
    /// blending ranges.
    pub blend_if: BlendIf,
    /// Blending Options › Advanced Blending: knockout, blend interior effects / clipped layers as
    /// group, transparency shapes layer, layer / vector mask hides effects. Default = Photoshop's.
    pub advanced: AdvancedBlending,
    /// Layer › Video Layers frame stack (None for a normal layer).
    pub video: Option<VideoData>,
}

impl Layer {
    /// Whether the layer has the Background's own properties: an unclipped pixel layer with
    /// transparency and position locked. Only the bottom layer of a document with them is the
    /// Background: use [`Document::background`] / [`Document::is_background`] to identify it.
    pub fn has_background_traits(&self) -> bool {
        self.locks.transparency
            && self.locks.position
            && !self.clipped
            && matches!(self.content, LayerContent::Raster(_))
    }

    pub fn new(name: impl Into<String>, content: LayerContent) -> Self {
        Layer {
            id: LayerId::fresh(),
            name: name.into(),
            visible: true,
            locks: Locks::default(),
            blend: BlendMode::Normal,
            opacity: 1.0,
            fill_opacity: 1.0,
            clipped: false,
            mask: None,
            vector_mask: None,
            effects: Effects {
                enabled: true,
                ..Default::default()
            },
            label: LabelColor::None,
            content,
            psd_blocks: Vec::new(),
            psd_id: None,
            fill_cache: None,
            link_group: None,
            excluded_channels: 0,
            blend_if: BlendIf::default(),
            advanced: AdvancedBlending::default(),
            video: None,
        }
    }
    pub fn raster(name: impl Into<String>, format: PixelFormat) -> Self {
        Self::new(name, LayerContent::Raster(Surface::new(format)))
    }
    pub fn group(name: impl Into<String>, children: Vec<Layer>) -> Self {
        let mut l = Self::new(
            name,
            LayerContent::Group(Group {
                children,
                expanded: true,
                artboard: None,
            }),
        );
        l.blend = BlendMode::PassThrough;
        l
    }
    pub fn is_group(&self) -> bool {
        matches!(self.content, LayerContent::Group(_))
    }
    pub fn children(&self) -> Option<&[Layer]> {
        match &self.content {
            LayerContent::Group(g) => Some(&g.children),
            _ => None,
        }
    }
    pub fn children_mut(&mut self) -> Option<&mut Vec<Layer>> {
        match &mut self.content {
            LayerContent::Group(g) => Some(&mut g.children),
            _ => None,
        }
    }
    pub fn surface(&self) -> Option<&Surface> {
        match &self.content {
            LayerContent::Raster(s) => Some(s),
            LayerContent::Text(t) => t.cache.as_ref(),
            LayerContent::Shape(s) => s.cache.as_ref(),
            LayerContent::Smart(s) => s.cache.as_ref(),
            _ => None,
        }
    }
    pub fn surface_mut(&mut self) -> Option<&mut Surface> {
        match &mut self.content {
            LayerContent::Raster(s) => Some(s),
            _ => None,
        }
    }
    /// Deep copy with fresh ids (for Duplicate Layer).
    pub fn duplicate(&self) -> Layer {
        let mut l = self.clone();
        l.reassign_ids();
        l
    }
    fn reassign_ids(&mut self) {
        self.id = LayerId::fresh();
        self.psd_id = None;
        if let Some(ch) = self.children_mut() {
            for c in ch {
                c.reassign_ids();
            }
        }
    }
}
