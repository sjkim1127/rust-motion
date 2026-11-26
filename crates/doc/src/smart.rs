//! Smart objects, their sources, filters and the fill cache.

use crate::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct SmartObject {
    /// Ordinary layer duplicates share this identity; independent contents receive a fresh one.
    pub contents_id: SmartContentsId,
    /// Embedded source file bytes (PSD, PNG, …) or linked path.
    pub source: SmartSource,
    pub transform: Affine,
    pub smart_filters: Vec<SmartFilter>,
    /// Rendered appearance.
    pub cache: Option<Surface>,
    /// Data of the PSD placed-layer block (`SoLd`, `PlLd` or `SoLE`). On export
    /// it replaces that entry in [`Layer::psd_blocks`].
    pub psd_raw: Option<Arc<Vec<u8>>>,
    /// Layer › Smart Filter › Disable Smart Filters turns the whole stack off
    /// without touching each filter's own visibility.
    pub filters_enabled: bool,
    /// The smart filter mask (one per smart object, as in Photoshop): where the
    /// filter stack's result shows over the unfiltered content.
    pub filter_mask: Option<LayerMask>,
    /// Edit › Transform › Warp on the smart object, in source-image coordinates; applied before
    /// `transform` when re-rendering from the source, so warping stays lossless.
    pub warp: Option<rust_motion_geom::warp::Warp>,
    /// Layer › Smart Objects › Stack Mode: when set, the source's top-level layers are combined
    /// per pixel with this statistic instead of composited.
    pub stack_mode: Option<StackMode>,
    /// Distort / Perspective (or an imported placement whose corners aren't a parallelogram): the
    /// full projective map from source pixels to document pixels, row-major 3×3. It overrides
    /// `transform`, which then holds its affine approximation at the source origin.
    pub perspective: Option<[f64; 9]>,
}

impl SmartObject {
    /// A smart object with no filters and no PSD data.
    pub fn new(source: SmartSource, transform: Affine, cache: Option<Surface>) -> Self {
        Self {
            contents_id: SmartContentsId::fresh(),
            source,
            transform,
            smart_filters: Vec::new(),
            cache,
            psd_raw: None,
            filters_enabled: true,
            filter_mask: None,
            warp: None,
            stack_mode: None,
            perspective: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SmartSource {
    Embedded {
        file_name: String,
        bytes: Arc<Vec<u8>>,
    },
    Linked {
        path: String,
    },
}

/// A filter applied non-destructively: a command id plus its parameters (data, not code).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmartFilter {
    pub command: String,
    pub params: serde_json::Value,
    pub blend: BlendMode,
    pub opacity: f32,
    pub visible: bool,
}

/// Pixels rendered by another application (Photoshop) for a fill layer.
///
/// Used by the compositor instead of rendering [`Fill`] itself, but only while
/// `fill` still equals the layer's current fill (editing the fill invalidates
/// the cache automatically).
#[derive(Clone, Debug, PartialEq)]
pub struct FillCache {
    /// The fill these pixels were rendered for.
    pub fill: Fill,
    /// Rendered pixels in document coordinates.
    pub surface: Surface,
}
