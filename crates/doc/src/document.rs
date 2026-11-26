//! The document: metadata, file encoding and the layer-tree operations.

use crate::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A raw PSD global tagged block: (signature, key, data).
pub type PsdGlobalBlock = ([u8; 4], [u8; 4], Arc<Vec<u8>>);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Metadata {
    pub xmp: Option<String>,
    pub exif: Option<Arc<Vec<u8>>>,
    /// Free-form image text (PNG tEXt/zTXt/iTXt, TIFF ASCII tags), including duplicate keys.
    pub text: Vec<(String, String)>,
    /// Raw PSD image resources we don't model yet: (id, name, data), for lossless round-trip.
    pub psd_resources: Vec<(u16, String, Arc<Vec<u8>>)>,
    /// Raw PSD global additional-layer-info blocks: (signature, key, data),
    /// e.g. `lnk2` embedded smart-object files, `Patt` patterns, `Txt2`.
    pub psd_global_blocks: Vec<PsdGlobalBlock>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub id: DocId,
    pub name: String,
    pub size: Size,
    pub resolution_dpi: f32,
    pub mode: ColorMode,
    pub depth: SampleType,
    pub icc_profile: Option<Arc<Vec<u8>>>,
    /// Bottom-to-top.
    pub layers: Vec<Layer>,
    pub channels: Vec<AlphaChannel>,
    pub guides: Guides,
    /// The rulers' zero point in document pixels (`[x, y]`), dragged from the ruler corner. The
    /// rulers, the Info panel and other position readouts measure from it; `[0, 0]` (the
    /// image's top-left) is the default.
    pub ruler_origin: [f64; 2],
    /// Active selection as a grayscale coverage surface (None = no selection).
    pub selection: Option<Surface>,
    pub metadata: Metadata,
    /// Global light used by layer effects (PSD resources 1037/1049).
    pub global_light: GlobalLight,
    /// Saved paths (Paths panel), top to bottom.
    pub paths: Vec<NamedPath>,
    /// The (unsaved) work path.
    pub work_path: Option<Path>,
    /// Clipping path used on export (names one of `paths`).
    pub clipping_path: Option<ClippingPath>,
    /// Select › Edit in Quick Mask Mode: the temporary "Quick Mask" channel while the mode is on.
    /// Part of the document so entering and leaving are history steps (as in Photoshop).
    pub quick_mask: Option<AlphaChannel>,
    /// Quick Mask was entered with no selection, so an untouched white mask (Color Indicates
    /// Masked Areas) leaves no selection rather than Select All.
    pub quick_mask_unselected: bool,
    /// Patterns stored with the document (PSD `Patt`/`Pat2`/`Pat3`): those its layers use, plus
    /// any imported with it.
    pub patterns: Vec<Pattern>,
    /// Indexed Color palette (Image › Mode › Color Table). Set while `mode` is Indexed.
    pub color_table: Option<ColorTable>,
    /// Duotone inks. Set while `mode` is Duotone.
    pub duotone: Option<Duotone>,
    /// Window › Layer Comps, top to bottom as listed in the panel.
    pub layer_comps: Vec<LayerComp>,
    /// Id of the comp applied last (PSD `lastAppliedComp`); None = the document's own state.
    pub last_applied_comp: Option<u32>,
    /// "Last Document State": layer state saved when a comp is applied over the document's own
    /// state, restored by `layerComp.restoreLastDocumentState`. Its id is 0.
    pub last_document_state: Option<LayerComp>,
    /// Image › Analysis: measurement scale, Count tool groups and the Ruler line.
    pub measurement: Measurement,
    /// Note tool annotations (PSD `Anno`).
    pub notes: Vec<Note>,
    /// Character and paragraph styles (Window › Character Styles / Paragraph Styles).
    pub text_styles: TextStyles,
    /// Web slices (Slice tool, layer-based slices; PSD resource 1050). Auto slices are derived.
    pub slices: Slices,
    /// Image › Variables and Data Sets (data-driven graphics).
    pub variables: Variables,
    /// Window › Timeline (None until a video timeline is created).
    pub timeline: Option<Timeline>,
    /// How the document's lossy file was encoded (its JPEG quality, WebP lossy or lossless), so
    /// a save writes it back the same way rather than at fixed defaults.
    pub file_encoding: FileEncoding,
}

/// The encoder settings a document's file was opened or saved with: what File › Save and the
/// Save As defaults reuse for that format. Empty for a new document or a file without lossy
/// options. Qualities are on the codecs' 1–100 scale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FileEncoding {
    /// JPEG quality (estimated from the file's quantisation tables on open).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jpeg_quality: Option<u8>,
    /// WebP: `Some(true)` lossless (VP8L), `Some(false)` lossy (VP8 at [`Self::webp_quality`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webp_lossless: Option<bool>,
    /// Lossy WebP quality.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webp_quality: Option<u8>,
}

impl FileEncoding {
    pub fn is_empty(&self) -> bool {
        *self == FileEncoding::default()
    }
}

/// Where a layer lives in the tree: indices from the root down.
pub type LayerPath = Vec<usize>;

impl Document {
    pub fn new(name: impl Into<String>, size: Size, mode: ColorMode, depth: SampleType) -> Self {
        Document {
            id: DocId::fresh(),
            name: name.into(),
            size,
            resolution_dpi: 72.0,
            mode,
            depth,
            icc_profile: None,
            layers: Vec::new(),
            channels: Vec::new(),
            guides: Guides::default(),
            ruler_origin: [0.0, 0.0],
            selection: None,
            metadata: Metadata::default(),
            global_light: GlobalLight::default(),
            paths: Vec::new(),
            work_path: None,
            clipping_path: None,
            quick_mask: None,
            quick_mask_unselected: false,
            patterns: Vec::new(),
            color_table: None,
            duotone: None,
            layer_comps: Vec::new(),
            last_applied_comp: None,
            last_document_state: None,
            measurement: Measurement::default(),
            notes: Vec::new(),
            text_styles: TextStyles::default(),
            slices: Slices::default(),
            variables: Variables::default(),
            timeline: None,
            file_encoding: FileEncoding::default(),
        }
    }

    /// New document with a filled "Background" layer (like File → New).
    pub fn with_background(
        name: impl Into<String>,
        size: Size,
        mode: ColorMode,
        depth: SampleType,
        fill: Color,
    ) -> Self {
        let mut d = Self::new(name, size, mode, depth);
        let mut bg = Layer::raster("Background", d.pixel_format());
        bg.locks.transparency = true;
        bg.locks.position = true;
        if let Some(s) = bg.surface_mut() {
            let rgba = fill.to_rgb();
            let px =
                rust_motion_raster::from_rgba(&s.format(), [rgba[0], rgba[1], rgba[2], fill.alpha]);
            s.fill_rect(d.bounds(), &px);
        }
        d.layers.push(bg);
        d
    }

    /// Pixel format for new raster layers in this document.
    pub fn pixel_format(&self) -> PixelFormat {
        let mode = match self.mode {
            ColorMode::Indexed | ColorMode::Multichannel => ColorMode::Rgb,
            ColorMode::Bitmap | ColorMode::Duotone => ColorMode::Grayscale,
            m => m,
        };
        PixelFormat::new(mode, self.depth, true)
    }

    pub fn bounds(&self) -> Rect {
        Rect::from_size(self.size)
    }

    /// Nesting depth of the deepest layer: 0 for a root-level layer, 1 inside one group, and so
    /// on. Computed over an explicit stack, so it cannot overflow on any tree it measures.
    pub fn max_group_depth(&self) -> usize {
        let mut max = 0;
        let mut stack: Vec<(&[Layer], usize)> = vec![(&self.layers, 0)];
        while let Some((layers, depth)) = stack.pop() {
            if layers.is_empty() {
                continue;
            }
            max = max.max(depth);
            for l in layers {
                if let Some(ch) = l.children() {
                    if !ch.is_empty() {
                        stack.push((ch, depth + 1));
                    }
                }
            }
        }
        max
    }

    /// Depth-first walk yielding `(path, depth, layer)` bottom-to-top.
    pub fn walk(&self) -> Vec<(LayerPath, usize, &Layer)> {
        fn rec<'a>(
            layers: &'a [Layer],
            prefix: &mut LayerPath,
            out: &mut Vec<(LayerPath, usize, &'a Layer)>,
        ) {
            for (i, l) in layers.iter().enumerate() {
                prefix.push(i);
                out.push((prefix.clone(), prefix.len() - 1, l));
                if let Some(ch) = l.children() {
                    rec(ch, prefix, out);
                }
                prefix.pop();
            }
        }
        let mut out = Vec::new();
        rec(&self.layers, &mut Vec::new(), &mut out);
        out
    }

    pub fn layer_count(&self) -> usize {
        self.walk().len()
    }

    pub fn path_of(&self, id: LayerId) -> Option<LayerPath> {
        fn rec(layers: &[Layer], id: LayerId, path: &mut LayerPath) -> bool {
            for (i, l) in layers.iter().enumerate() {
                path.push(i);
                if l.id == id || l.children().is_some_and(|ch| rec(ch, id, path)) {
                    return true;
                }
                path.pop();
            }
            false
        }
        let mut path = Vec::new();
        rec(&self.layers, id, &mut path).then_some(path)
    }

    /// The locks in force on the layer at `path`: its own and those of every group around it,
    /// since locking a group locks its contents.
    pub fn locks_at(&self, path: &[usize]) -> Locks {
        (1..=path.len())
            .filter_map(|n| self.layer_at(path.get(..n)?))
            .fold(Locks::default(), |a, l| a.union(l.locks))
    }

    /// Whether the layer at `path` is shown: it and every group around it are visible.
    pub fn shown_at(&self, path: &[usize]) -> bool {
        (1..=path.len()).all(|n| {
            path.get(..n)
                .and_then(|p| self.layer_at(p))
                .is_some_and(|l| l.visible)
        })
    }

    /// [`Self::locks_at`] for the layer `id`.
    pub fn effective_locks(&self, id: LayerId) -> Locks {
        self.path_of(id)
            .map_or_else(Locks::default, |p| self.locks_at(&p))
    }

    /// The Background layer, identified structurally as Photoshop does, never by its name (a file
    /// from a German or Japanese Photoshop names it "Hintergrund" or "背景"): the bottom layer of
    /// the stack, a pixel layer that is not clipped, with transparency and position locked (a PSD
    /// stores it without a transparency channel and transparency-protected; File › New and opaque
    /// flat files create it the same way).
    pub fn background(&self) -> Option<&Layer> {
        self.layers
            .first()
            .filter(|l| Layer::has_background_traits(l))
    }

    /// [`Self::background`], mutably.
    pub fn background_mut(&mut self) -> Option<&mut Layer> {
        self.layers
            .first_mut()
            .filter(|l| Layer::has_background_traits(l))
    }

    /// Whether `id` is the document's Background layer ([`Self::background`]).
    pub fn is_background(&self, id: LayerId) -> bool {
        self.background().is_some_and(|b| b.id == id)
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        fn rec(layers: &[Layer], id: LayerId) -> Option<&Layer> {
            layers.iter().find_map(|l| {
                if l.id == id {
                    Some(l)
                } else {
                    l.children().and_then(|ch| rec(ch, id))
                }
            })
        }
        rec(&self.layers, id)
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        let path = self.path_of(id)?;
        self.layer_at_mut(&path)
    }

    /// A [`Layer::link_group`] id no layer uses yet: one above the highest stored id, or, when a
    /// loaded file already holds `u64::MAX` (#961), the smallest free id from 1. Never wraps, so a
    /// new link can't join an unrelated group. `None` only if every id is taken.
    pub fn unused_link_group(&self) -> Option<u64> {
        let mut used: Vec<u64> = self
            .walk()
            .iter()
            .filter_map(|(_, _, l)| l.link_group)
            .collect();
        used.sort_unstable();
        used.dedup();
        if let Some(next) = used.last().map_or(Some(1), |g| g.checked_add(1)) {
            return Some(next);
        }
        let mut free = 1u64;
        for g in used {
            if g > free {
                break;
            }
            if g == free {
                free = free.checked_add(1)?;
            }
        }
        Some(free)
    }

    pub fn layer_at(&self, path: &[usize]) -> Option<&Layer> {
        let (first, rest) = path.split_first()?;
        let mut cur = self.layers.get(*first)?;
        for &i in rest {
            cur = cur.children()?.get(i)?;
        }
        Some(cur)
    }

    pub fn layer_at_mut(&mut self, path: &[usize]) -> Option<&mut Layer> {
        let (first, rest) = path.split_first()?;
        let mut cur = self.layers.get_mut(*first)?;
        for &i in rest {
            cur = cur.children_mut()?.get_mut(i)?;
        }
        Some(cur)
    }

    /// The sibling list containing `path` and the index within it.
    fn siblings_mut(&mut self, path: &[usize]) -> Option<(&mut Vec<Layer>, usize)> {
        let (&last, parent) = path.split_last()?;
        if parent.is_empty() {
            return Some((&mut self.layers, last));
        }
        let p = self.layer_at_mut(parent)?;
        Some((p.children_mut()?, last))
    }

    /// Insert `layer` directly above the layer `above` (or at the top of the root if None).
    pub fn insert_above(&mut self, above: Option<LayerId>, layer: Layer) -> LayerId {
        let id = layer.id;
        match above
            .and_then(|a| self.path_of(a))
            .and_then(|path| self.siblings_mut(&path))
        {
            Some((sib, idx)) => sib.insert(idx + 1, layer),
            None => self.layers.push(layer),
        }
        id
    }

    /// Insert `layer` directly below the layer `below` (or at the bottom of the root if None).
    pub fn insert_below(&mut self, below: Option<LayerId>, layer: Layer) -> LayerId {
        let id = layer.id;
        match below
            .and_then(|b| self.path_of(b))
            .and_then(|path| self.siblings_mut(&path))
        {
            Some((sib, idx)) => sib.insert(idx, layer),
            None => self.layers.insert(0, layer),
        }
        id
    }

    pub fn remove(&mut self, id: LayerId) -> Option<Layer> {
        let path = self.path_of(id)?;
        let (sib, idx) = self.siblings_mut(&path)?;
        Some(sib.remove(idx))
    }

    /// Move a layer up (+1) or down (-1) among its siblings. Returns false at the ends.
    pub fn shift(&mut self, id: LayerId, delta: i32) -> bool {
        let Some(path) = self.path_of(id) else {
            return false;
        };
        let Some((sib, idx)) = self.siblings_mut(&path) else {
            return false;
        };
        let to = idx as i64 + delta as i64;
        if to < 0 || to >= sib.len() as i64 {
            return false;
        }
        let l = sib.remove(idx);
        sib.insert(to as usize, l);
        true
    }

    /// Next unused "Layer N" name.
    pub fn next_layer_name(&self, base: &str) -> String {
        let names: std::collections::HashSet<&str> = self
            .walk()
            .into_iter()
            .map(|(_, _, l)| l.name.as_str())
            .collect();
        // At most `names.len() + 1` candidates are needed, so the search always succeeds.
        (1..=names.len() + 1)
            .map(|n| format!("{base} {n}"))
            .find(|n| !names.contains(n.as_str()))
            .unwrap_or_else(|| base.to_string())
    }

    /// Name for a copy of a layer named `name`, unique in the document: "Layer 1 copy", then
    /// "Layer 1 copy 2", "Layer 1 copy 3"… A name that already ends in "copy" (or "copy N") is
    /// numbered from its root rather than growing another "copy".
    pub fn copy_name(&self, name: &str) -> String {
        let root = match name.rsplit_once(" copy") {
            Some((root, rest))
                if rest.is_empty()
                    || rest.strip_prefix(' ').is_some_and(|n| {
                        !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())
                    }) =>
            {
                root
            }
            _ => name,
        };
        let names: std::collections::HashSet<&str> = self
            .walk()
            .into_iter()
            .map(|(_, _, l)| l.name.as_str())
            .collect();
        let first = format!("{root} copy");
        if !names.contains(first.as_str()) {
            return first;
        }
        // At most `names.len() + 1` candidates are needed, so the search always succeeds.
        (2..=names.len() + 2)
            .map(|n| format!("{root} copy {n}"))
            .find(|n| !names.contains(n.as_str()))
            .unwrap_or(first)
    }

    /// Top-most layer id, useful as the default active layer.
    pub fn top_layer(&self) -> Option<LayerId> {
        self.layers.last().map(|l| l.id)
    }
}
