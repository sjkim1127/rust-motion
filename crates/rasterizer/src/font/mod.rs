//! Font loading and text rasterization via `ab_glyph`.
//!
//! Font discovery follows a three-step priority order:
//!
//! 1. `RUST_MOTION_FONT_PATH` environment variable — highest priority, useful for CI overrides.
//! 2. Platform-specific system font search paths — picks up the user's installed fonts.
//! 3. Bundled fallback (`NotoSans-Regular.ttf` compiled in with `include_bytes!`) — guarantees
//!    reproducible rendering in any environment, including minimal Docker images and CI runners
//!    that have no fonts installed.
//!
//! The bundled font ensures that `FontCache::load()` always returns a loaded cache, eliminating
//! the "No system font found" warning and making output deterministic across platforms.
//!
//! The module is split by concern: [`types`] holds the layout option/result data, [`cache`] the
//! font loading and bitmap caches, [`layout`] the text-box layout and fitting entry points,
//! [`box_fit`] the N-line and rounded-box fitting, and [`wrap`] the wrapping and shaping helpers.
//! Everything public is re-exported here, so existing `font::Item` paths keep working.

/// NotoSans Regular compiled into the binary as a reproducible font fallback.
///
/// Source: Google Fonts, licensed under the SIL Open Font License 1.1.
/// The font is always available regardless of the host operating system or installed fonts.
const BUNDLED_FONT: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Regular.ttf");

use crate::backend::RasterError;
use crate::text_atlas::{AtlasEntry, TextAtlas, TextAtlasSnapshot};
use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use unicode_linebreak::{linebreaks, BreakOpportunity};
use unicode_segmentation::UnicodeSegmentation;

const MAX_FONT_BYTES: u64 = 32 * 1024 * 1024;

mod box_fit;
mod cache;
mod layout;
#[cfg(test)]
mod tests;
mod types;
mod wrap;

pub use box_fit::*;
pub use cache::*;
pub use layout::*;
pub use types::*;
use wrap::*;
