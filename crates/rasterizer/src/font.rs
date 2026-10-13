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

/// NotoSans Regular compiled into the binary as a reproducible font fallback.
///
/// Source: Google Fonts, licensed under the SIL Open Font License 1.1.
/// The font is always available regardless of the host operating system or installed fonts.
const BUNDLED_FONT: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");

use crate::backend::RasterError;
use crate::text_atlas::{AtlasEntry, TextAtlas, TextAtlasSnapshot};
use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use unicode_linebreak::{linebreaks, BreakOpportunity};
use unicode_segmentation::UnicodeSegmentation;

const MAX_FONT_BYTES: u64 = 32 * 1024 * 1024;

/// Text horizontal alignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl From<TextAlign> for TextHorizontalAlign {
    fn from(align: TextAlign) -> Self {
        match align {
            TextAlign::Left => TextHorizontalAlign::Start,
            TextAlign::Center => TextHorizontalAlign::Center,
            TextAlign::Right => TextHorizontalAlign::End,
        }
    }
}

impl From<TextHorizontalAlign> for TextAlign {
    fn from(align: TextHorizontalAlign) -> Self {
        match align {
            TextHorizontalAlign::Start => TextAlign::Left,
            TextHorizontalAlign::Center => TextAlign::Center,
            TextHorizontalAlign::End => TextAlign::Right,
        }
    }
}

/// Options for multi-line text auto-scaling (`fit_text_on_n_lines`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitTextOnNLinesOptions {
    pub max_lines: usize,
    pub max_box_width: f32,
    pub max_box_height: Option<f32>,
    pub min_font_size: f32,
    pub max_font_size: f32,
}

impl Default for FitTextOnNLinesOptions {
    fn default() -> Self {
        Self {
            max_lines: 1,
            max_box_width: 1920.0,
            max_box_height: None,
            min_font_size: 1.0,
            max_font_size: 200.0,
        }
    }
}

/// Result of auto-fitting text across multiple lines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextFitResult {
    pub font_size: f32,
    pub lines: Vec<String>,
    pub total_height: f32,
    pub max_line_width: f32,
}

/// Options for multi-corner parametric rounded text box generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoundedTextBoxOptions {
    pub padding_x: f32,
    pub padding_y: f32,
    pub border_radius: f32,
    pub align: TextAlign,
}

impl Default for RoundedTextBoxOptions {
    fn default() -> Self {
        Self {
            padding_x: 16.0,
            padding_y: 12.0,
            border_radius: 8.0,
            align: TextAlign::Left,
        }
    }
}

/// Layout error for text fitting and layout calculation failures.
#[derive(Debug, thiserror::Error, PartialEq, Clone)]
pub enum LayoutError {
    #[error("Invalid constraints: {0}")]
    InvalidConstraints(String),
    #[error("Text could not be fit into the specified bounds: {0}")]
    CannotFit(String),
    #[error("Layout error: {0}")]
    Generic(String),
}

impl From<LayoutError> for RasterError {
    fn from(err: LayoutError) -> Self {
        RasterError::Scene(err.to_string())
    }
}

/// Measurement of a single line of text for parametric path generation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TextLineDimension {
    pub width: f32,
    pub height: f32,
}

impl TextLineDimension {
    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// Horizontal alignment inside a resolved text box.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextHorizontalAlign {
    #[default]
    Start,
    Center,
    End,
}

/// Vertical alignment inside a resolved text box.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextVerticalAlign {
    #[default]
    Start,
    Center,
    End,
}

/// Behavior when text still exceeds its box at the minimum font size.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextOverflow {
    #[default]
    Clip,
    Ellipsis,
}

/// Font-aware text box request resolved before nodes are added to a Scene.
#[derive(Debug, Clone, PartialEq)]
pub struct TextBox {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub font_size: f32,
    pub min_font_size: f32,
    /// Multiplier applied to the resolved font size.
    pub line_height: f32,
    pub max_lines: Option<usize>,
    pub horizontal_align: TextHorizontalAlign,
    pub vertical_align: TextVerticalAlign,
    pub overflow: TextOverflow,
    pub font_sources: Vec<String>,
}

impl TextBox {
    pub fn new(
        text: impl Into<String>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        font_size: f32,
    ) -> Self {
        Self {
            text: text.into(),
            x,
            y,
            width,
            height,
            font_size,
            min_font_size: font_size,
            line_height: 1.2,
            max_lines: None,
            horizontal_align: TextHorizontalAlign::Start,
            vertical_align: TextVerticalAlign::Start,
            overflow: TextOverflow::Clip,
            font_sources: Vec::new(),
        }
    }
}

/// One baseline-positioned line produced by [`layout_text_box`].
#[derive(Debug, Clone, PartialEq)]
pub struct PositionedTextLine {
    pub text: String,
    pub x: f32,
    pub y: f32,
}

/// Resolved size and lines for deterministic native and Player rendering.
#[derive(Debug, Clone, PartialEq)]
pub struct TextBoxLayout {
    pub font_size: f32,
    pub line_height: f32,
    pub lines: Vec<PositionedTextLine>,
}

/// Platform-specific font search paths, in preference order.
#[cfg(target_os = "macos")]
const FONT_SEARCH_PATHS: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/System/Library/Fonts/Supplemental/Verdana.ttf",
    "/System/Library/Fonts/Supplemental/Georgia.ttf",
    "/System/Library/Fonts/SFNS.ttf",
];

#[cfg(target_os = "linux")]
const FONT_SEARCH_PATHS: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];

#[cfg(target_os = "windows")]
const FONT_SEARCH_PATHS: &[&str] = &[
    "C:\\Windows\\Fonts\\arial.ttf",
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\calibri.ttf",
];

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
const FONT_SEARCH_PATHS: &[&str] = &[];

/// A loaded, ready-to-use font.
pub struct FontCache {
    font: Option<Arc<LoadedFont>>,
    path: Option<String>,
    assets: Mutex<HashMap<String, Arc<LoadedFont>>>,
    rasterized: Mutex<HashMap<TextRasterKey, Arc<RenderedText>>>,
    atlas: Mutex<TextAtlas>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct TextRasterKey {
    text: String,
    font_size_bits: u32,
    font_weight: u16,
    sources: Vec<String>,
}

pub(crate) struct LoadedFont {
    pub(crate) raster: FontVec,
    pub(crate) data: Arc<Vec<u8>>,
}

struct ShapedGlyph {
    font: Arc<LoadedFont>,
    glyph: ab_glyph::Glyph,
}

fn apply_synthetic_bold(pixels: &mut [u8], width: u32, height: u32, weight: u16) {
    let radius = u32::from(weight)
        .saturating_sub(400)
        .div_ceil(200)
        .clamp(1, 3);
    let original = pixels.to_vec();
    for y in 0..height {
        for x in 0..width {
            let mut coverage = 0u8;
            for dy in 0..=radius {
                for dx in 0..=radius {
                    let sx = x.saturating_sub(dx);
                    let sy = y.saturating_sub(dy);
                    coverage = coverage.max(original[(sy * width + sx) as usize]);
                }
            }
            pixels[(y * width + x) as usize] = coverage;
        }
    }
}

impl LoadedFont {
    fn from_bytes(bytes: Vec<u8>) -> Result<Self, ab_glyph::InvalidFont> {
        let raster = FontVec::try_from_vec(bytes.clone())?;
        Ok(Self {
            raster,
            data: Arc::new(bytes),
        })
    }
}

#[derive(Debug)]
pub(crate) struct FontLoadError {
    pub path: String,
    pub reason: String,
}

impl FontCache {
    /// Load a font using a three-step priority order:
    ///
    /// 1. `RUST_MOTION_FONT_PATH` env var — e.g. `RUST_MOTION_FONT_PATH=/fonts/MyFont.ttf`
    /// 2. Platform system font search paths
    /// 3. Bundled `NotoSans-Regular.ttf` — always succeeds
    ///
    /// This method never returns a cache with `is_loaded() == false`.
    pub fn load() -> Self {
        // Step 1: environment-variable override
        if let Ok(path) = std::env::var("RUST_MOTION_FONT_PATH") {
            let path = path.trim().to_string();
            if !path.is_empty() {
                match std::fs::read(&path).and_then(|b| {
                    LoadedFont::from_bytes(b).map_err(|e| std::io::Error::other(e.to_string()))
                }) {
                    Ok(font) => {
                        tracing::info!(font_path = %path, "Loaded font from RUST_MOTION_FONT_PATH");
                        return Self {
                            font: Some(Arc::new(font)),
                            path: Some(path),
                            assets: Mutex::new(HashMap::new()),
                            rasterized: Mutex::new(HashMap::new()),
                            atlas: Mutex::new(TextAtlas::new(2048, 2048)),
                        };
                    }
                    Err(err) => {
                        tracing::warn!(
                            font_path = %path,
                            error = %err,
                            "RUST_MOTION_FONT_PATH could not be loaded, falling back"
                        );
                    }
                }
            }
        }

        // Step 2: optional system font search if explicitly opted in
        if std::env::var("RUST_MOTION_USE_SYSTEM_FONTS")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false)
        {
            for path in FONT_SEARCH_PATHS {
                if let Ok(bytes) = std::fs::read(path) {
                    if let Ok(font) = LoadedFont::from_bytes(bytes) {
                        tracing::debug!(font_path = %path, "Loaded system font");
                        return Self {
                            font: Some(Arc::new(font)),
                            path: Some(path.to_string()),
                            assets: Mutex::new(HashMap::new()),
                            rasterized: Mutex::new(HashMap::new()),
                            atlas: Mutex::new(TextAtlas::new(2048, 2048)),
                        };
                    }
                }
            }
        }

        // Step 3: bundled NotoSans-Regular — guarantees deterministic cross-platform typography
        tracing::debug!("Using bundled NotoSans-Regular as deterministic default font");
        Self::bundled()
    }

    /// Return a `FontCache` loaded with the bundled `NotoSans-Regular.ttf`.
    ///
    /// Unlike [`Self::load`], this skips env-var and system-font discovery and
    /// directly uses the font compiled into the binary. Useful for tests that
    /// need a deterministic font independent of the host machine.
    pub fn bundled() -> Self {
        let font = LoadedFont::from_bytes(BUNDLED_FONT.to_vec())
            .expect("bundled NotoSans-Regular.ttf is a valid font");
        Self {
            font: Some(Arc::new(font)),
            path: Some("<bundled:NotoSans-Regular>".into()),
            assets: Mutex::new(HashMap::new()),
            rasterized: Mutex::new(HashMap::new()),
            atlas: Mutex::new(TextAtlas::new(2048, 2048)),
        }
    }

    /// Create a FontCache with no font loaded (for tests that verify tofu-box rendering).
    pub fn headless() -> Self {
        Self {
            font: None,
            path: None,
            assets: Mutex::new(HashMap::new()),
            rasterized: Mutex::new(HashMap::new()),
            atlas: Mutex::new(TextAtlas::new(2048, 2048)),
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.font.is_some()
    }

    pub fn font_path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Register a font from raw bytes under a named key/family.
    pub fn register_font_bytes(&self, name: &str, bytes: Vec<u8>) -> Result<(), RasterError> {
        let loaded = LoadedFont::from_bytes(bytes)
            .map_err(|e| RasterError::Scene(format!("failed to parse font '{name}': {e:?}")))?;
        self.assets
            .lock()
            .expect("font cache lock poisoned")
            .insert(name.to_string(), Arc::new(loaded));
        self.rasterized
            .lock()
            .expect("font raster cache lock poisoned")
            .clear();
        self.atlas.lock().expect("text atlas lock poisoned").clear();
        Ok(())
    }

    /// Load and register a font from a local file path into the cache.
    pub fn register_font_from_path(
        &self,
        name: &str,
        path: &std::path::Path,
    ) -> Result<(), RasterError> {
        let bytes = std::fs::read(path).map_err(|e| {
            RasterError::Scene(format!(
                "failed to read font from '{}': {e}",
                path.display()
            ))
        })?;
        self.register_font_bytes(name, bytes)
    }

    #[allow(dead_code)]
    pub(crate) fn text_atlas_snapshot(&self) -> TextAtlasSnapshot {
        self.atlas
            .lock()
            .expect("text atlas lock poisoned")
            .snapshot()
    }

    #[allow(dead_code)]
    pub(crate) fn take_text_atlas_snapshot(&self) -> TextAtlasSnapshot {
        self.atlas
            .lock()
            .expect("text atlas lock poisoned")
            .take_snapshot()
    }

    #[allow(dead_code)]
    pub(crate) fn text_atlas_entry(&self, key: &str) -> Option<AtlasEntry> {
        self.atlas
            .lock()
            .expect("text atlas lock poisoned")
            .entry(key)
    }

    /// Rasterize text with ordered explicit local fonts followed by the system fallback.
    #[allow(dead_code)]
    pub(crate) fn rasterize(
        &self,
        text: &str,
        font_size: f32,
        sources: &[String],
    ) -> Result<Option<RenderedText>, FontLoadError> {
        self.rasterize_with_weight(text, font_size, 400, sources)
    }

    /// Rasterize text using a CSS-like numeric font weight.
    ///
    /// Font assets are not variable-font aware yet, so weights heavier than
    /// regular use deterministic synthetic coverage expansion. Keeping the
    /// weight in both caches is important: regular and bold text must never
    /// alias the same atlas entry.
    pub(crate) fn rasterize_with_weight(
        &self,
        text: &str,
        font_size: f32,
        font_weight: u16,
        sources: &[String],
    ) -> Result<Option<RenderedText>, FontLoadError> {
        let key = TextRasterKey {
            text: text.to_string(),
            font_size_bits: font_size.to_bits(),
            font_weight,
            sources: sources.to_vec(),
        };
        if let Some(cached) = self
            .rasterized
            .lock()
            .expect("font raster cache lock poisoned")
            .get(&key)
            .cloned()
        {
            let rendered = (*cached).clone();
            let atlas_key = format!(
                "{}:{}:{}:{:?}",
                key.text, key.font_size_bits, key.font_weight, key.sources
            );
            let mut atlas = self.atlas.lock().expect("text atlas lock poisoned");
            if atlas.entry(&atlas_key).is_none() {
                let _ = atlas.insert(
                    atlas_key,
                    &rendered.pixels,
                    rendered.width,
                    rendered.height,
                    rendered.baseline,
                );
            }
            return Ok(Some(rendered));
        }
        let rendered = self.rasterize_uncached(text, font_size, font_weight, sources)?;
        if let Some(rendered) = rendered.as_ref() {
            let atlas_key = format!(
                "{}:{}:{}:{:?}",
                key.text, key.font_size_bits, key.font_weight, key.sources
            );
            let _ = self.atlas.lock().expect("text atlas lock poisoned").insert(
                atlas_key,
                &rendered.pixels,
                rendered.width,
                rendered.height,
                rendered.baseline,
            );
            self.rasterized
                .lock()
                .expect("font raster cache lock poisoned")
                .insert(key, Arc::new(rendered.clone()));
        }
        Ok(rendered)
    }

    fn rasterize_uncached(
        &self,
        text: &str,
        font_size: f32,
        font_weight: u16,
        sources: &[String],
    ) -> Result<Option<RenderedText>, FontLoadError> {
        let fonts = self.font_chain(sources)?;
        if fonts.is_empty() {
            return Ok(None);
        }
        let scale = PxScale::from(font_size);
        let metric_ascent = fonts
            .iter()
            .map(|font| font.raster.as_scaled(scale).ascent())
            .fold(0.0_f32, f32::max)
            .ceil();
        let metric_descent = fonts
            .iter()
            .map(|font| -font.raster.as_scaled(scale).descent())
            .fold(0.0_f32, f32::max)
            .ceil();
        let (mut glyphs, advance) = shape_runs(text, font_size, &fonts)?;
        let mut left = 0.0_f32;
        let mut right = advance.max(0.0);
        let mut top = -metric_ascent;
        let mut bottom = metric_descent;
        for glyph in &glyphs {
            if let Some(outlined) = glyph.font.raster.outline_glyph(glyph.glyph.clone()) {
                let bounds = outlined.px_bounds();
                left = left.min(bounds.min.x.floor());
                right = right.max(bounds.max.x.ceil());
                top = top.min(bounds.min.y.floor());
                bottom = bottom.max(bounds.max.y.ceil());
            }
        }
        let horizontal_shift = -left;
        for glyph in &mut glyphs {
            glyph.glyph.position.x += horizontal_shift;
        }
        let total_width = (right - left).ceil().max(0.0) as u32;
        let total_height = (bottom - top).ceil().max(0.0) as u32;
        let baseline = (-top).ceil().max(0.0) as u32;

        if total_width == 0 || total_height == 0 {
            return Ok(Some(RenderedText {
                pixels: vec![],
                width: 0,
                height: 0,
                baseline,
            }));
        }

        let mut pixels = vec![0u8; (total_width * total_height) as usize];

        for glyph in &glyphs {
            if let Some(outlined) = glyph.font.raster.outline_glyph(glyph.glyph.clone()) {
                let bounds = outlined.px_bounds();
                let gx = bounds.min.x.floor() as i32;
                let gy = bounds.min.y.floor() as i32 + baseline as i32;

                outlined.draw(|rx, ry, coverage| {
                    let px = gx + rx as i32;
                    let py = gy + ry as i32;
                    if px >= 0 && py >= 0 {
                        let px = px as u32;
                        let py = py as u32;
                        if px < total_width && py < total_height {
                            let idx = (py * total_width + px) as usize;
                            // Accumulate coverage (clamp to 255)
                            let existing = pixels[idx] as f32 / 255.0;
                            let blended = (existing + coverage * (1.0 - existing)).min(1.0);
                            pixels[idx] = (blended * 255.0) as u8;
                        }
                    }
                });
            }
        }

        if font_weight > 400 {
            apply_synthetic_bold(&mut pixels, total_width, total_height, font_weight);
        }

        Ok(Some(RenderedText {
            pixels,
            width: total_width,
            height: total_height,
            baseline,
        }))
    }

    pub(crate) fn font_chain(
        &self,
        sources: &[String],
    ) -> Result<Vec<Arc<LoadedFont>>, FontLoadError> {
        let mut fonts = Vec::with_capacity(sources.len() + usize::from(self.font.is_some()));
        for source in sources {
            fonts.push(self.load_asset(source)?);
        }
        if let Some(font) = &self.font {
            fonts.push(font.clone());
        }
        Ok(fonts)
    }

    fn load_asset(&self, source: &str) -> Result<Arc<LoadedFont>, FontLoadError> {
        let source = source.trim();
        let path = source.strip_prefix("file://").unwrap_or(source);
        if path.is_empty() {
            return Err(FontLoadError {
                path: source.into(),
                reason: "font source path must not be empty".into(),
            });
        }
        if path == "<bundled:NotoSans-Regular>" || path.starts_with("<bundled:") {
            if let Some(font) = &self.font {
                return Ok(font.clone());
            }
            let font = Arc::new(
                LoadedFont::from_bytes(BUNDLED_FONT.to_vec())
                    .expect("bundled NotoSans-Regular.ttf is valid"),
            );
            return Ok(font);
        }
        if source.contains("://") && !source.starts_with("file://") {
            return Err(FontLoadError {
                path: source.into(),
                reason: "remote font sources are not supported by native rendering".into(),
            });
        }
        if let Some(font) = self
            .assets
            .lock()
            .expect("font cache lock poisoned")
            .get(path)
            .cloned()
        {
            return Ok(font);
        }

        let metadata = std::fs::metadata(path).map_err(|error| FontLoadError {
            path: source.into(),
            reason: error.to_string(),
        })?;
        if !metadata.is_file() {
            return Err(FontLoadError {
                path: source.into(),
                reason: "font source is not a regular file".into(),
            });
        }
        if metadata.len() > MAX_FONT_BYTES {
            return Err(FontLoadError {
                path: source.into(),
                reason: format!("font exceeds the {MAX_FONT_BYTES} byte safety limit"),
            });
        }
        let bytes = std::fs::read(path).map_err(|error| FontLoadError {
            path: source.into(),
            reason: error.to_string(),
        })?;
        let font = Arc::new(
            LoadedFont::from_bytes(bytes).map_err(|error| FontLoadError {
                path: source.into(),
                reason: format!("unsupported or invalid font data: {error:?}"),
            })?,
        );
        self.assets
            .lock()
            .expect("font cache lock poisoned")
            .insert(path.into(), font.clone());
        Ok(font)
    }

    #[cfg(test)]
    fn asset_count(&self) -> usize {
        self.assets.lock().expect("font cache lock poisoned").len()
    }
}

static TEXT_LAYOUT_FONT_CACHE: OnceLock<FontCache> = OnceLock::new();

/// Resolve Unicode line breaks, font fitting, ellipsis, and box alignment.
///
/// The returned lines retain explicit baseline positions and can therefore be
/// emitted as ordinary [`crate::scene::SceneNode::Text`] nodes for both native
/// export and Player preview.
pub fn layout_text_box(request: &TextBox) -> Result<TextBoxLayout, RasterError> {
    validate_text_box(request)?;
    let cache = TEXT_LAYOUT_FONT_CACHE.get_or_init(FontCache::load);
    let fonts = cache
        .font_chain(&request.font_sources)
        .map_err(font_asset_error)?;
    if fonts.is_empty() {
        return Err(RasterError::Scene(
            "text box layout requires an explicit or system font".into(),
        ));
    }

    let mut font_size = request.font_size;
    let mut lines;
    loop {
        lines =
            wrap_text(&request.text, request.width, font_size, &fonts).map_err(font_asset_error)?;
        let line_height = font_size * request.line_height;
        let fits_line_limit = request
            .max_lines
            .is_none_or(|maximum| lines.len() <= maximum);
        let fits_height = line_height * lines.len() as f32 <= request.height + 0.01;
        if (fits_line_limit && fits_height) || font_size <= request.min_font_size + 0.01 {
            break;
        }
        font_size = (font_size - 0.5).max(request.min_font_size);
    }

    let line_height = font_size * request.line_height;
    let height_line_limit = (request.height / line_height).floor().max(1.0) as usize;
    let allowed_lines = request
        .max_lines
        .unwrap_or(usize::MAX)
        .min(height_line_limit);
    let truncated = lines.len() > allowed_lines;
    lines.truncate(allowed_lines);
    if truncated && request.overflow == TextOverflow::Ellipsis {
        if let Some(last) = lines.last_mut() {
            add_ellipsis(last, request.width, font_size, &fonts).map_err(font_asset_error)?;
        }
    }

    let content_height = line_height * lines.len() as f32;
    let vertical_offset = match request.vertical_align {
        TextVerticalAlign::Start => 0.0,
        TextVerticalAlign::Center => (request.height - content_height).max(0.0) * 0.5,
        TextVerticalAlign::End => (request.height - content_height).max(0.0),
    };
    let ascent = fonts
        .iter()
        .map(|font| font.raster.as_scaled(PxScale::from(font_size)).ascent())
        .fold(0.0_f32, f32::max);
    let mut positioned = Vec::with_capacity(lines.len());
    for (index, line) in lines.into_iter().enumerate() {
        let width = measure_text(&line, font_size, &fonts).map_err(font_asset_error)?;
        let horizontal_offset = match request.horizontal_align {
            TextHorizontalAlign::Start => 0.0,
            TextHorizontalAlign::Center => (request.width - width).max(0.0) * 0.5,
            TextHorizontalAlign::End => (request.width - width).max(0.0),
        };
        positioned.push(PositionedTextLine {
            text: line,
            x: request.x + horizontal_offset,
            y: request.y + vertical_offset + ascent + index as f32 * line_height,
        });
    }

    Ok(TextBoxLayout {
        font_size,
        line_height,
        lines: positioned,
    })
}

/// Measure a shaped single line using the same font chain as native rendering.
pub fn measure_text_width(
    text: &str,
    font_size: f32,
    font_sources: &[String],
) -> Result<f32, RasterError> {
    if !font_size.is_finite() || font_size <= 0.0 || font_size > 4096.0 {
        return Err(RasterError::Scene(
            "text measurement font size must be between 0 and 4096".into(),
        ));
    }
    let cache = TEXT_LAYOUT_FONT_CACHE.get_or_init(FontCache::load);
    let fonts = cache.font_chain(font_sources).map_err(font_asset_error)?;
    if fonts.is_empty() {
        return Err(RasterError::Scene(
            "text measurement requires an explicit or system font".into(),
        ));
    }
    measure_text(text, font_size, &fonts).map_err(font_asset_error)
}

/// Find the largest font size at which `text` fits within `max_width`.
///
/// Binary search between `min_font_size` and `max_font_size` with sub-pixel precision.
/// Returns the font size that just fits, or `min_font_size` if text cannot fit.
///
/// # Arguments
/// * `text` - the text to measure
/// * `max_width` - maximum allowed width in pixels
/// * `font_sources` - font file paths (same as `measure_text_width`)
/// * `min_font_size` - lower bound for binary search (must be positive)
/// * `max_font_size` - upper bound for binary search (must be >= `min_font_size` and <= 4096.0)
pub fn fit_text(
    text: &str,
    max_width: f64,
    font_sources: &[String],
    min_font_size: f64,
    max_font_size: f64,
) -> Result<f64, RasterError> {
    if !max_width.is_finite() || !min_font_size.is_finite() || !max_font_size.is_finite() {
        return Err(RasterError::Scene(
            "fit_text parameters must be finite".into(),
        ));
    }
    if max_width <= 0.0 {
        return Err(RasterError::Scene(
            "fit_text max_width must be positive".into(),
        ));
    }
    if min_font_size <= 0.0 {
        return Err(RasterError::Scene(
            "fit_text min_font_size must be positive".into(),
        ));
    }
    if max_font_size < min_font_size {
        return Err(RasterError::Scene(
            "fit_text max_font_size must be greater than or equal to min_font_size".into(),
        ));
    }
    if max_font_size > 4096.0 {
        return Err(RasterError::Scene(
            "fit_text max_font_size must not exceed 4096".into(),
        ));
    }

    if text.is_empty() {
        return Ok(max_font_size);
    }

    let max_width_actual = f64::from(measure_text_width(
        text,
        max_font_size as f32,
        font_sources,
    )?);
    if max_width_actual <= max_width {
        return Ok(max_font_size);
    }

    let min_width_actual = f64::from(measure_text_width(
        text,
        min_font_size as f32,
        font_sources,
    )?);
    if min_width_actual > max_width {
        return Ok(min_font_size);
    }

    let mut lo = min_font_size;
    let mut hi = max_font_size;
    let mut best = min_font_size;

    // Up to 25 iterations of binary search → precision ~0.1px
    for _ in 0..25 {
        if hi - lo < 0.1 {
            break;
        }
        let mid = (lo + hi) / 2.0;
        let width = f64::from(measure_text_width(text, mid as f32, font_sources)?);
        if width <= max_width {
            best = mid;
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Ok(best)
}

/// Measure text width using an explicit font instance.
pub fn measure_text_width_with_font<F: Font>(text: &str, font: &F, font_size: f32) -> f32 {
    if text.is_empty() || !font_size.is_finite() || font_size <= 0.0 {
        return 0.0;
    }
    let scaled = font.as_scaled(PxScale::from(font_size));
    let mut total_width = 0.0_f32;
    let mut prev_glyph: Option<ab_glyph::GlyphId> = None;
    for ch in text.chars() {
        if ch == '\n' || ch == '\r' {
            continue;
        }
        let glyph_id = scaled.glyph_id(ch);
        if let Some(prev) = prev_glyph {
            total_width += scaled.kern(prev, glyph_id);
        }
        total_width += scaled.h_advance(glyph_id);
        prev_glyph = Some(glyph_id);
    }
    total_width
}

/// Greedy word-by-word text box line-wrapping algorithm.
///
/// Accumulates words into lines such that each line does not exceed `max_box_width`.
/// Respects explicit line breaks in the input string.
pub fn fill_text_box<F: Font>(
    text: &str,
    font: &F,
    font_size: f32,
    max_box_width: f32,
) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    if !font_size.is_finite()
        || font_size <= 0.0
        || !max_box_width.is_finite()
        || max_box_width <= 0.0
    {
        return vec![text.to_string()];
    }

    let mut result_lines = Vec::new();

    // Split by hard line breaks first
    let raw_lines: Vec<&str> = text.split('\n').collect();

    for raw_line in raw_lines {
        let clean_line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let words: Vec<&str> = clean_line.split_whitespace().collect();
        if words.is_empty() {
            if raw_line.is_empty() {
                result_lines.push(String::new());
            }
            continue;
        }

        let mut current_line = String::new();

        for word in words {
            if current_line.is_empty() {
                current_line = word.to_string();
            } else {
                let candidate = format!("{current_line} {word}");
                let candidate_width = measure_text_width_with_font(&candidate, font, font_size);
                if candidate_width <= max_box_width {
                    current_line = candidate;
                } else {
                    result_lines.push(current_line);
                    current_line = word.to_string();
                }
            }
        }

        if !current_line.is_empty() {
            result_lines.push(current_line);
        }
    }

    result_lines
}

/// Binary search font auto-scaling to fit multi-line word-wrapped text into bounding box.
///
/// Finds the maximum font size in `[options.min_font_size, options.max_font_size]` such that
/// the word-wrapped text fits within `options.max_lines` and `options.max_box_width` (and optional `max_box_height`).
pub fn fit_text_on_n_lines<F: Font>(
    text: &str,
    font: &F,
    options: &FitTextOnNLinesOptions,
) -> Result<TextFitResult, LayoutError> {
    if options.max_lines == 0 {
        return Err(LayoutError::InvalidConstraints(
            "max_lines must be at least 1".into(),
        ));
    }
    if !options.max_box_width.is_finite() || options.max_box_width <= 0.0 {
        return Err(LayoutError::InvalidConstraints(
            "max_box_width must be positive and finite".into(),
        ));
    }
    if let Some(h) = options.max_box_height {
        if !h.is_finite() || h <= 0.0 {
            return Err(LayoutError::InvalidConstraints(
                "max_box_height must be positive and finite".into(),
            ));
        }
    }
    if !options.min_font_size.is_finite() || options.min_font_size <= 0.0 {
        return Err(LayoutError::InvalidConstraints(
            "min_font_size must be positive and finite".into(),
        ));
    }
    if !options.max_font_size.is_finite() || options.max_font_size < options.min_font_size {
        return Err(LayoutError::InvalidConstraints(
            "max_font_size must be >= min_font_size and finite".into(),
        ));
    }

    if text.trim().is_empty() {
        let scaled = font.as_scaled(PxScale::from(options.max_font_size));
        let total_height = (scaled.ascent() - scaled.descent()).max(options.max_font_size);
        return Ok(TextFitResult {
            font_size: options.max_font_size,
            lines: if text.is_empty() {
                vec![]
            } else {
                vec![text.to_string()]
            },
            total_height,
            max_line_width: 0.0,
        });
    }

    let test_fit = |size: f32| -> Option<(Vec<String>, f32, f32)> {
        let lines = fill_text_box(text, font, size, options.max_box_width);
        if lines.len() > options.max_lines {
            return None;
        }
        let mut max_w = 0.0_f32;
        for line in &lines {
            let w = measure_text_width_with_font(line, font, size);
            if w > options.max_box_width + 0.01 {
                return None;
            }
            max_w = max_w.max(w);
        }
        let scaled = font.as_scaled(PxScale::from(size));
        let line_h = (scaled.ascent() - scaled.descent() + scaled.line_gap()).max(size);
        let total_h = if lines.is_empty() {
            0.0
        } else if lines.len() == 1 {
            (scaled.ascent() - scaled.descent()).max(size)
        } else {
            lines.len() as f32 * line_h
        };
        if let Some(max_h) = options.max_box_height {
            if total_h > max_h + 0.01 {
                return None;
            }
        }
        Some((lines, total_h, max_w))
    };

    // First check if max_font_size fits
    if let Some((lines, total_height, max_line_width)) = test_fit(options.max_font_size) {
        return Ok(TextFitResult {
            font_size: options.max_font_size,
            lines,
            total_height,
            max_line_width,
        });
    }

    // Binary search between min_font_size and max_font_size
    let mut low = options.min_font_size;
    let mut high = options.max_font_size;
    let mut best_result = test_fit(low);
    let mut best_size = low;

    for _ in 0..30 {
        if high - low < 0.05 {
            break;
        }
        let mid = (low + high) / 2.0;
        if let Some(fit) = test_fit(mid) {
            best_result = Some(fit);
            best_size = mid;
            low = mid;
        } else {
            high = mid;
        }
    }

    if let Some((lines, total_height, max_line_width)) = best_result {
        Ok(TextFitResult {
            font_size: best_size,
            lines,
            total_height,
            max_line_width,
        })
    } else {
        // Fallback to min_font_size with best effort wrapping
        let lines = fill_text_box(text, font, options.min_font_size, options.max_box_width);
        let scaled = font.as_scaled(PxScale::from(options.min_font_size));
        let line_h =
            (scaled.ascent() - scaled.descent() + scaled.line_gap()).max(options.min_font_size);
        let total_height = if lines.is_empty() {
            0.0
        } else {
            lines.len() as f32 * line_h
        };
        let max_line_width = lines
            .iter()
            .map(|l| measure_text_width_with_font(l, font, options.min_font_size))
            .fold(0.0_f32, f32::max);
        Ok(TextFitResult {
            font_size: options.min_font_size,
            lines,
            total_height,
            max_line_width,
        })
    }
}

/// Create a parametric rounded text box SVG path string from pre-computed line measurements.
pub fn create_rounded_text_box_from_measurements(
    measurements: &[TextLineDimension],
    options: &RoundedTextBoxOptions,
) -> String {
    if measurements.is_empty() {
        return String::new();
    }

    let mut instructions: Vec<String> = Vec::new();

    let max_width = measurements
        .iter()
        .map(|m| m.width + options.padding_x * 2.0)
        .fold(0.0_f32, f32::max);

    let mut y_offset = 0.0_f32;

    // Forward pass (Top & Right edges)
    for i in 0..measurements.len() {
        let current_line = &measurements[i];
        let prev_line = if i > 0 {
            Some(&measurements[i - 1])
        } else {
            None
        };
        let next_line = if i + 1 < measurements.len() {
            Some(&measurements[i + 1])
        } else {
            None
        };

        let line_total_width = current_line.width + options.padding_x * 2.0;
        let x_offset = match options.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (max_width - line_total_width) / 2.0,
            TextAlign::Right => max_width - line_total_width,
        };

        let max_corner_radius = (options.border_radius.max(0.0)).min(current_line.height / 2.0);

        if i == 0 {
            instructions.push(format!("M {} {}", x_offset + max_corner_radius, y_offset));
        }

        let top_right_radius_raw = match prev_line {
            Some(prev) => match options.align {
                TextAlign::Right => 0.0,
                TextAlign::Left => (prev.width - current_line.width) / 2.0,
                TextAlign::Center => (prev.width - current_line.width) / 4.0,
            },
            None => -f32::INFINITY,
        };
        let top_right_corner_radius =
            top_right_radius_raw.clamp(-max_corner_radius, max_corner_radius);

        if top_right_corner_radius != 0.0 {
            let r = top_right_corner_radius.abs();
            let lx =
                x_offset + current_line.width + options.padding_x * 2.0 + top_right_corner_radius;
            instructions.push(format!("L {lx} {y_offset}"));
            let sweep = if top_right_corner_radius < 0.0 { 1 } else { 0 };
            let end_x = x_offset + current_line.width + options.padding_x * 2.0;
            let end_y = y_offset + r;
            instructions.push(format!("A {r} {r} 0 0 {sweep} {end_x} {end_y}"));
        } else {
            let lx = x_offset + current_line.width + options.padding_x * 2.0;
            instructions.push(format!("L {lx} {y_offset}"));
        }

        let bottom_right_radius_raw = match next_line {
            Some(next) => match options.align {
                TextAlign::Right => 0.0,
                TextAlign::Left => (next.width - current_line.width) / 2.0,
                TextAlign::Center => (next.width - current_line.width) / 4.0,
            },
            None => -f32::INFINITY,
        };
        let bottom_right_corner_radius =
            bottom_right_radius_raw.clamp(-max_corner_radius, max_corner_radius);

        if bottom_right_corner_radius != 0.0 {
            let r = bottom_right_corner_radius.abs();
            let ly = y_offset + current_line.height - r;
            let lx = x_offset + current_line.width + options.padding_x * 2.0;
            instructions.push(format!("L {lx} {ly}"));
            let sweep = if bottom_right_corner_radius < 0.0 {
                1
            } else {
                0
            };
            let end_x = x_offset
                + current_line.width
                + options.padding_x * 2.0
                + bottom_right_corner_radius;
            let end_y = y_offset + current_line.height;
            instructions.push(format!("A {r} {r} 0 0 {sweep} {end_x} {end_y}"));
        } else {
            let lx = x_offset + current_line.width + options.padding_x * 2.0;
            let ly = y_offset + current_line.height;
            instructions.push(format!("L {lx} {ly}"));
        }

        y_offset += current_line.height;
    }

    // Backward pass (Bottom & Left edges)
    for i in (0..measurements.len()).rev() {
        let current_line = &measurements[i];
        let prev_line = if i + 1 < measurements.len() {
            Some(&measurements[i + 1])
        } else {
            None
        };
        let next_line = if i > 0 {
            Some(&measurements[i - 1])
        } else {
            None
        };

        let line_total_width = current_line.width + options.padding_x * 2.0;
        let x_offset = match options.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (max_width - line_total_width) / 2.0,
            TextAlign::Right => max_width - line_total_width,
        };

        let max_corner_radius = (options.border_radius.max(0.0)).min(current_line.height / 2.0);

        let bottom_left_radius_raw = match prev_line {
            Some(prev) => {
                let diff = prev.width - current_line.width;
                match options.align {
                    TextAlign::Left => 0.0,
                    TextAlign::Right => diff / 2.0,
                    TextAlign::Center => diff / 4.0,
                }
            }
            None => -f32::INFINITY,
        };
        let bottom_left_corner_radius =
            bottom_left_radius_raw.clamp(-max_corner_radius, max_corner_radius);

        if bottom_left_corner_radius != 0.0 {
            let r = bottom_left_corner_radius.abs();
            let lx = x_offset - bottom_left_corner_radius;
            instructions.push(format!("L {lx} {y_offset}"));
            let sweep = if bottom_left_corner_radius < 0.0 {
                1
            } else {
                0
            };
            let end_x = x_offset;
            let end_y = y_offset - r;
            instructions.push(format!("A {r} {r} 0 0 {sweep} {end_x} {end_y}"));
        } else {
            instructions.push(format!("L {x_offset} {y_offset}"));
        }

        let top_left_radius_raw = match next_line {
            Some(next) => {
                let diff = next.width - current_line.width;
                match options.align {
                    TextAlign::Left => 0.0,
                    TextAlign::Right => diff / 2.0,
                    TextAlign::Center => diff / 4.0,
                }
            }
            None => -f32::INFINITY,
        };
        let top_left_corner_radius =
            top_left_radius_raw.clamp(-max_corner_radius, max_corner_radius);

        if top_left_corner_radius != 0.0 {
            let r = top_left_corner_radius.abs();
            let ly = y_offset - current_line.height + r;
            instructions.push(format!("L {x_offset} {ly}"));
            let sweep = if top_left_corner_radius < 0.0 { 1 } else { 0 };
            let end_x = x_offset - top_left_corner_radius;
            let end_y = y_offset - current_line.height;
            instructions.push(format!("A {r} {r} 0 0 {sweep} {end_x} {end_y}"));
        } else {
            let ly = y_offset - current_line.height;
            instructions.push(format!("L {x_offset} {ly}"));
        }

        y_offset -= current_line.height;
    }

    instructions.push("Z".to_string());
    instructions.join(" ")
}

/// Create a parametric rounded text box SVG path string from rendered lines of text.
pub fn create_rounded_text_box<F: Font>(
    lines: &[String],
    font: &F,
    font_size: f32,
    options: &RoundedTextBoxOptions,
) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let scaled = font.as_scaled(PxScale::from(font_size));
    let line_height = (scaled.ascent() - scaled.descent() + scaled.line_gap()).max(font_size)
        + options.padding_y * 2.0;
    let measurements: Vec<TextLineDimension> = lines
        .iter()
        .map(|line| {
            let width = measure_text_width_with_font(line, font, font_size);
            TextLineDimension {
                width,
                height: line_height,
            }
        })
        .collect();

    create_rounded_text_box_from_measurements(&measurements, options)
}

fn validate_text_box(request: &TextBox) -> Result<(), RasterError> {
    let finite = [
        ("x", request.x),
        ("y", request.y),
        ("width", request.width),
        ("height", request.height),
        ("font size", request.font_size),
        ("minimum font size", request.min_font_size),
        ("line height", request.line_height),
    ];
    if let Some((name, value)) = finite.iter().find(|(_, value)| !value.is_finite()) {
        return Err(RasterError::Scene(format!(
            "text box {name} must be finite, got {value}"
        )));
    }
    if request.width <= 0.0 || request.height <= 0.0 {
        return Err(RasterError::Scene(
            "text box width and height must be positive".into(),
        ));
    }
    if request.font_size <= 0.0 || request.font_size > 4096.0 {
        return Err(RasterError::Scene(
            "text box font size must be between 0 and 4096".into(),
        ));
    }
    if request.min_font_size <= 0.0 || request.min_font_size > request.font_size {
        return Err(RasterError::Scene(
            "minimum font size must be positive and no larger than font size".into(),
        ));
    }
    if !(0.5..=10.0).contains(&request.line_height) {
        return Err(RasterError::Scene(
            "text box line height multiplier must be between 0.5 and 10".into(),
        ));
    }
    if request.max_lines == Some(0) {
        return Err(RasterError::Scene(
            "text box max lines must be at least one".into(),
        ));
    }
    Ok(())
}

fn wrap_text(
    text: &str,
    max_width: f32,
    font_size: f32,
    fonts: &[Arc<LoadedFont>],
) -> Result<Vec<String>, FontLoadError> {
    if text.is_empty() {
        return Ok(vec![String::new()]);
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut segment_start = 0;
    for (break_index, opportunity) in linebreaks(text) {
        let raw_segment = &text[segment_start..break_index];
        let segment = if opportunity == BreakOpportunity::Mandatory {
            raw_segment.trim_end_matches(['\r', '\n'])
        } else {
            raw_segment
        };
        append_wrapped_segment(
            &mut lines,
            &mut current,
            segment,
            max_width,
            font_size,
            fonts,
        )?;
        if opportunity == BreakOpportunity::Mandatory {
            lines.push(current.trim_end().to_string());
            current.clear();
        }
        segment_start = break_index;
    }
    if segment_start < text.len() {
        append_wrapped_segment(
            &mut lines,
            &mut current,
            &text[segment_start..],
            max_width,
            font_size,
            fonts,
        )?;
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current.trim_end().to_string());
    }
    Ok(lines)
}

fn append_wrapped_segment(
    lines: &mut Vec<String>,
    current: &mut String,
    segment: &str,
    max_width: f32,
    font_size: f32,
    fonts: &[Arc<LoadedFont>],
) -> Result<(), FontLoadError> {
    let candidate = format!("{current}{segment}");
    if measure_text(&candidate, font_size, fonts)? <= max_width || current.is_empty() {
        *current = candidate;
    } else {
        lines.push(current.trim_end().to_string());
        *current = segment.trim_start().to_string();
    }
    if measure_text(current, font_size, fonts)? <= max_width {
        return Ok(());
    }

    let oversized = std::mem::take(current);
    let mut part = String::new();
    for grapheme in oversized.graphemes(true) {
        let candidate = format!("{part}{grapheme}");
        if !part.is_empty() && measure_text(&candidate, font_size, fonts)? > max_width {
            lines.push(part);
            part = grapheme.to_string();
        } else {
            part = candidate;
        }
    }
    *current = part;
    Ok(())
}

fn add_ellipsis(
    line: &mut String,
    max_width: f32,
    font_size: f32,
    fonts: &[Arc<LoadedFont>],
) -> Result<(), FontLoadError> {
    *line = line.trim_end().to_string();
    loop {
        let candidate = format!("{line}…");
        if measure_text(&candidate, font_size, fonts)? <= max_width || line.is_empty() {
            *line = candidate;
            return Ok(());
        }
        let Some((index, _)) = line.grapheme_indices(true).next_back() else {
            line.push('…');
            return Ok(());
        };
        line.truncate(index);
    }
}

fn measure_text(
    text: &str,
    font_size: f32,
    fonts: &[Arc<LoadedFont>],
) -> Result<f32, FontLoadError> {
    let (_, advance) = shape_runs(text, font_size, fonts)?;
    Ok(advance.abs())
}

fn font_asset_error(error: FontLoadError) -> RasterError {
    RasterError::FontAsset {
        path: error.path,
        reason: error.reason,
    }
}

fn shape_runs(
    text: &str,
    font_size: f32,
    fonts: &[Arc<LoadedFont>],
) -> Result<(Vec<ShapedGlyph>, f32), FontLoadError> {
    let bidi_runs = crate::text_layout::bidi::segment_bidi_runs(
        text,
        crate::text_layout::style::TextDirection::Auto,
    );
    let resolver = crate::text_layout::font_resolver::FontResolver::from_fonts(fonts.to_vec());
    let style = crate::text_layout::style::TextStyle::new(font_size);
    let shaped = crate::text_layout::shaper::shape_bidi_runs(text, &bidi_runs, &resolver, &style)
        .map_err(|e| FontLoadError {
        path: "<loaded font>".into(),
        reason: format!("{e}"),
    })?;

    let mut output = Vec::new();
    let mut cursor_x = 0.0_f32;
    for run in shaped {
        let font = fonts[run.font_index].clone();
        for g in run.glyphs {
            let x = cursor_x + g.x_offset;
            let y = g.y_offset;
            output.push(ShapedGlyph {
                font: font.clone(),
                glyph: ab_glyph::GlyphId(g.glyph_id)
                    .with_scale_and_position(PxScale::from(font_size), ab_glyph::point(x, y)),
            });
            cursor_x += g.x_advance;
        }
    }
    Ok((output, cursor_x))
}

/// Rasterized text as a greyscale coverage map.
#[derive(Debug, Clone)]
pub struct RenderedText {
    /// Single-channel (alpha coverage) pixel data, row-major.
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Row index of the baseline within the pixel buffer.
    pub baseline: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_text_finds_font_size_within_width() {
        let cache = FontCache::bundled();
        let _font_path = cache.font_path().unwrap().to_string();
        let sources: Vec<String> = vec![];
        let size = fit_text("Hello World", 200.0, &sources, 8.0, 48.0);
        assert!(size.is_ok(), "fit_text should succeed");
        let size = size.unwrap();
        assert!(
            (8.0..=48.0).contains(&size),
            "font size {size} out of range"
        );
        let measured = measure_text_width("Hello World", size as f32, &sources).unwrap();
        assert!(
            f64::from(measured) <= 200.0,
            "measured width {measured} should be <= max_width 200.0"
        );
    }

    #[test]
    fn fit_text_exact_max_font_size_for_short_text() {
        let sources: Vec<String> = vec![];
        let size = fit_text("Hi", 1000.0, &sources, 10.0, 50.0).unwrap();
        assert_eq!(
            size, 50.0,
            "should return max_font_size when text easily fits"
        );
    }

    #[test]
    fn fit_text_empty_string_returns_max_font_size() {
        let sources: Vec<String> = vec![];
        let size = fit_text("", 200.0, &sources, 8.0, 48.0).unwrap();
        assert_eq!(size, 48.0, "empty text should return max_font_size");
    }

    #[test]
    fn fit_text_returns_min_font_size_when_overflows() {
        let sources: Vec<String> = vec![];
        let size = fit_text(
            "This is a very long sentence that cannot possibly fit in a tiny width",
            5.0,
            &sources,
            12.0,
            64.0,
        )
        .unwrap();
        assert_eq!(
            size, 12.0,
            "should return min_font_size when text cannot fit"
        );
    }

    #[test]
    fn fit_text_equal_min_and_max_font_size() {
        let sources: Vec<String> = vec![];
        // Fits:
        let size_fit = fit_text("Hello", 500.0, &sources, 24.0, 24.0).unwrap();
        assert_eq!(size_fit, 24.0);

        // Does not fit:
        let size_overflow = fit_text("Hello World Very Long", 5.0, &sources, 24.0, 24.0).unwrap();
        assert_eq!(size_overflow, 24.0);
    }

    #[test]
    fn fit_text_rejects_non_finite_inputs() {
        let sources: Vec<String> = vec![];
        assert!(fit_text("test", f64::NAN, &sources, 8.0, 48.0).is_err());
        assert!(fit_text("test", 200.0, &sources, f64::NAN, 48.0).is_err());
        assert!(fit_text("test", 200.0, &sources, 8.0, f64::NAN).is_err());
        assert!(fit_text("test", f64::INFINITY, &sources, 8.0, 48.0).is_err());
        assert!(fit_text("test", 200.0, &sources, f64::INFINITY, 48.0).is_err());
        assert!(fit_text("test", 200.0, &sources, 8.0, f64::INFINITY).is_err());
    }

    #[test]
    fn fit_text_rejects_invalid_bounds() {
        let sources: Vec<String> = vec![];
        // Non-positive max_width
        assert!(fit_text("test", 0.0, &sources, 8.0, 48.0).is_err());
        assert!(fit_text("test", -100.0, &sources, 8.0, 48.0).is_err());

        // Non-positive min_font_size
        assert!(fit_text("test", 200.0, &sources, 0.0, 48.0).is_err());
        assert!(fit_text("test", 200.0, &sources, -10.0, 48.0).is_err());

        // max_font_size < min_font_size
        assert!(fit_text("test", 200.0, &sources, 50.0, 20.0).is_err());

        // max_font_size > 4096.0
        assert!(fit_text("test", 200.0, &sources, 10.0, 5000.0).is_err());
    }

    #[test]
    fn fit_text_propagates_font_load_error_for_invalid_source() {
        let sources = vec!["/nonexistent/path/custom_font.ttf".to_string()];
        let err = fit_text("test", 200.0, &sources, 8.0, 48.0);
        assert!(err.is_err());
        match err.unwrap_err() {
            RasterError::FontAsset { path, .. } => {
                assert!(path.contains("custom_font.ttf"));
            }
            other => panic!("expected RasterError::FontAsset, got {other:?}"),
        }
    }

    #[test]
    fn measure_text_width_empty_string() {
        let width = measure_text_width("", 24.0, &[]).unwrap();
        assert_eq!(width, 0.0);
    }

    #[test]
    fn measure_text_width_scales_with_font_size() {
        let w1 = measure_text_width("Scaling Test", 16.0, &[]).unwrap();
        let w2 = measure_text_width("Scaling Test", 32.0, &[]).unwrap();
        assert!(w1 > 0.0);
        assert!(
            w2 > w1 * 1.8,
            "doubling font size should roughly double width"
        );
    }

    #[test]
    fn measure_text_width_scales_with_text_length() {
        let w_short = measure_text_width("Short", 20.0, &[]).unwrap();
        let w_long = measure_text_width("Short and much longer text", 20.0, &[]).unwrap();
        assert!(w_long > w_short);
    }

    #[test]
    fn measure_text_width_rejects_invalid_font_size() {
        assert!(measure_text_width("test", f32::NAN, &[]).is_err());
        assert!(measure_text_width("test", 0.0, &[]).is_err());
        assert!(measure_text_width("test", -5.0, &[]).is_err());
        assert!(measure_text_width("test", 5000.0, &[]).is_err());
    }

    #[test]
    fn test_font_cache_loads() {
        let cache = FontCache::load();
        // On macOS this should always succeed; on other platforms it may not
        if cache.is_loaded() {
            println!("Loaded font from: {:?}", cache.font_path());
        } else {
            println!("No system font found — placeholder mode active");
        }
    }

    #[test]
    fn test_rasterize_hello() {
        let cache = FontCache::load();
        if !cache.is_loaded() {
            return; // skip if no font available
        }
        let rendered = cache
            .rasterize("Hello", 32.0, &[])
            .expect("font load failed")
            .expect("rasterize failed");
        assert!(rendered.width > 0, "Width should be > 0");
        assert!(rendered.height > 0, "Height should be > 0");
        // At least some pixels should have coverage
        let has_coverage = rendered.pixels.iter().any(|&p| p > 0);
        assert!(has_coverage, "At least one pixel should have coverage");
    }

    #[test]
    fn repeated_text_rasterization_reuses_bitmap_cache() {
        let cache = FontCache::bundled();
        let first = cache.rasterize("Atlas cache", 24.0, &[]).unwrap().unwrap();
        let second = cache.rasterize("Atlas cache", 24.0, &[]).unwrap().unwrap();
        assert_eq!(first.pixels, second.pixels);
        assert_eq!(first.width, second.width);
        assert_eq!(
            cache
                .rasterized
                .lock()
                .expect("font raster cache lock poisoned")
                .len(),
            1
        );
        let atlas = cache.text_atlas_snapshot();
        assert_eq!(atlas.width, 2048);
        assert_eq!(atlas.height, 2048);
        assert!(atlas.pixels.iter().any(|&coverage| coverage > 0));
    }

    #[test]
    fn synthetic_bold_is_cached_separately_and_adds_coverage() {
        let cache = FontCache::bundled();
        let regular = cache
            .rasterize_with_weight("Weight", 28.0, 400, &[])
            .unwrap()
            .unwrap();
        let bold = cache
            .rasterize_with_weight("Weight", 28.0, 700, &[])
            .unwrap()
            .unwrap();
        assert_ne!(regular.pixels, bold.pixels);
        let regular_coverage: u32 = regular.pixels.iter().map(|pixel| u32::from(*pixel)).sum();
        let bold_coverage: u32 = bold.pixels.iter().map(|pixel| u32::from(*pixel)).sum();
        assert!(bold_coverage >= regular_coverage);
        assert_eq!(
            cache
                .rasterized
                .lock()
                .expect("font raster cache lock poisoned")
                .len(),
            2
        );
    }

    #[test]
    fn test_rasterize_empty() {
        let cache = FontCache::load();
        if !cache.is_loaded() {
            return;
        }
        let rendered = cache
            .rasterize("", 24.0, &[])
            .expect("font load failed")
            .expect("rasterize failed");
        assert_eq!(rendered.width, 0, "Empty string should have 0 width");
    }

    #[test]
    fn explicit_font_sources_are_cached() {
        let Some(path) = FONT_SEARCH_PATHS
            .iter()
            .find(|path| std::path::Path::new(path).is_file())
        else {
            return;
        };
        let cache = FontCache::headless();
        let sources = vec![path.to_string()];
        let first = cache.rasterize("Explicit", 24.0, &sources).unwrap();
        let second = cache.rasterize("Explicit", 24.0, &sources).unwrap();

        assert!(first.is_some());
        assert!(second.is_some());
        assert_eq!(cache.asset_count(), 1);
    }

    #[test]
    fn missing_explicit_font_is_an_error() {
        let cache = FontCache::headless();
        let sources = vec!["/rust-motion/does-not-exist.ttf".to_string()];
        let error = cache.rasterize("Missing", 24.0, &sources).unwrap_err();

        assert!(error.path.ends_with("does-not-exist.ttf"));
    }

    #[test]
    fn shaping_produces_positioned_glyphs() {
        let Some(path) = font_fixture() else {
            return;
        };
        let cache = FontCache::headless();
        let fonts = cache
            .font_chain(&[path])
            .expect("system fixture font should load");
        let (glyphs, advance) = shape_runs("office", 32.0, &fonts).unwrap();

        assert!(!glyphs.is_empty());
        assert!(glyphs.len() <= "office".chars().count());
        assert!(advance > 0.0);
    }

    fn font_fixture() -> Option<String> {
        FontCache::load().font_path().map(str::to_owned)
    }

    #[test]
    fn text_box_wraps_fits_and_adds_ellipsis() {
        let Some(font) = font_fixture() else {
            return;
        };
        let mut request = TextBox::new(
            "one two three four five six seven eight",
            10.0,
            20.0,
            120.0,
            48.0,
            30.0,
        );
        request.min_font_size = 14.0;
        request.max_lines = Some(2);
        request.horizontal_align = TextHorizontalAlign::Center;
        request.vertical_align = TextVerticalAlign::Center;
        request.overflow = TextOverflow::Ellipsis;
        request.font_sources = vec![font];

        let layout = layout_text_box(&request).unwrap();

        assert!(layout.font_size < 30.0);
        assert!(layout.lines.len() <= 2);
        assert!(layout.lines.iter().all(|line| line.x >= request.x));
    }

    #[test]
    fn text_box_preserves_mandatory_line_breaks() {
        let Some(font) = font_fixture() else {
            return;
        };
        let mut request = TextBox::new("first\nsecond", 0.0, 0.0, 300.0, 100.0, 24.0);
        request.font_sources = vec![font];
        let layout = layout_text_box(&request).unwrap();

        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.lines[0].text, "first");
        assert_eq!(layout.lines[1].text, "second");
        assert!(layout.lines[1].y > layout.lines[0].y);
    }

    #[test]
    fn text_box_ellipsizes_when_minimum_size_still_overflows() {
        let Some(font) = font_fixture() else {
            return;
        };
        let mut request = TextBox::new("one two three four five six", 0.0, 0.0, 90.0, 30.0, 24.0);
        request.max_lines = Some(1);
        request.overflow = TextOverflow::Ellipsis;
        request.font_sources = vec![font];
        let layout = layout_text_box(&request).unwrap();

        assert_eq!(layout.lines.len(), 1);
        assert!(layout.lines[0].text.ends_with('…'));
        assert!(
            measure_text_width(
                &layout.lines[0].text,
                layout.font_size,
                &request.font_sources
            )
            .unwrap()
                <= request.width
        );
    }

    #[test]
    fn text_box_rejects_invalid_bounds() {
        let request = TextBox::new("invalid", 0.0, 0.0, 0.0, 100.0, 24.0);
        assert!(layout_text_box(&request)
            .unwrap_err()
            .to_string()
            .contains("width and height"));
    }

    // ── Bundled font tests ────────────────────────────────────────────────────

    #[test]
    fn font_cache_load_always_returns_a_loaded_cache() {
        // FontCache::load() must never return a headless (unloaded) cache,
        // because the bundled NotoSans fallback is always compiled in.
        let cache = FontCache::load();
        assert!(
            cache.is_loaded(),
            "FontCache::load() must always be loaded (bundled font is the last resort fallback)"
        );
        assert!(
            cache.font_path().is_some(),
            "font_path() must be Some after a successful load"
        );
    }

    #[test]
    fn font_cache_bundled_is_always_loaded() {
        let cache = FontCache::bundled();
        assert!(
            cache.is_loaded(),
            "FontCache::bundled() must always be loaded"
        );
        assert_eq!(
            cache.font_path(),
            Some("<bundled:NotoSans-Regular>"),
            "font_path() should identify the bundled font"
        );
    }

    #[test]
    fn bundled_font_can_rasterize_ascii_text() {
        let cache = FontCache::bundled();
        // Rasterize a short ASCII string — must succeed and produce non-empty pixels.
        let result = cache.rasterize("Hello", 24.0, &[]).unwrap();
        let rendered = result.expect("bundled font must produce rendered text for ASCII");
        assert!(rendered.width > 0, "rendered text must have non-zero width");
        assert!(
            rendered.height > 0,
            "rendered text must have non-zero height"
        );
        assert!(
            !rendered.pixels.is_empty(),
            "rendered text must have pixel data"
        );
    }

    #[test]
    fn env_override_takes_precedence_over_bundled() {
        // When RUST_MOTION_FONT_PATH points to a non-existent file,
        // FontCache::load() must still succeed via the bundled fallback.
        std::env::set_var("RUST_MOTION_FONT_PATH", "/nonexistent/path/font.ttf");
        let cache = FontCache::load();
        std::env::remove_var("RUST_MOTION_FONT_PATH");
        assert!(
            cache.is_loaded(),
            "FontCache::load() must fall back to bundled when env path is invalid"
        );
    }

    // ── Milestone 3: Advanced Layout & Text Fitting Tests ─────────────────────

    #[test]
    fn test_fill_text_box_single_and_multi_line() {
        let font = FontVec::try_from_vec(BUNDLED_FONT.to_vec()).unwrap();

        // 1. Single line fits
        let lines = fill_text_box("Hello world", &font, 16.0, 500.0);
        assert_eq!(lines, vec!["Hello world"]);

        // 2. Wraps into multiple lines when box width is narrow
        let lines = fill_text_box(
            "The quick brown fox jumps over the lazy dog",
            &font,
            16.0,
            100.0,
        );
        assert!(lines.len() > 1, "Should wrap into multiple lines");
        for line in &lines {
            let width = measure_text_width_with_font(line, &font, 16.0);
            assert!(
                width <= 100.0 + 1.0,
                "Line '{line}' width {width} exceeds 100.0"
            );
        }

        // 3. Preserves explicit newlines
        let lines = fill_text_box("Line 1\nLine 2\n\nLine 3", &font, 16.0, 500.0);
        assert_eq!(lines, vec!["Line 1", "Line 2", "", "Line 3"]);

        // 4. Empty string returns empty
        let lines = fill_text_box("", &font, 16.0, 500.0);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_fit_text_on_n_lines_optimal_scaling() {
        let font = FontVec::try_from_vec(BUNDLED_FONT.to_vec()).unwrap();

        // Fits on 1 line
        let options = FitTextOnNLinesOptions {
            max_lines: 1,
            max_box_width: 300.0,
            max_box_height: None,
            min_font_size: 10.0,
            max_font_size: 60.0,
        };
        let result = fit_text_on_n_lines("Rust Motion Engine", &font, &options).unwrap();
        assert_eq!(result.lines.len(), 1);
        assert!(result.font_size >= 10.0 && result.font_size <= 60.0);
        assert!(result.max_line_width <= 300.0);

        // Fits on 2 lines with higher font size than 1 line
        let options_2lines = FitTextOnNLinesOptions {
            max_lines: 2,
            max_box_width: 200.0,
            max_box_height: None,
            min_font_size: 10.0,
            max_font_size: 80.0,
        };
        let result_2lines = fit_text_on_n_lines(
            "High Performance Video Editing in Pure Rust",
            &font,
            &options_2lines,
        )
        .unwrap();
        assert!(result_2lines.lines.len() <= 2);
        assert!(result_2lines.max_line_width <= 200.0);

        // Respects max_box_height constraint
        let options_height_limited = FitTextOnNLinesOptions {
            max_lines: 3,
            max_box_width: 300.0,
            max_box_height: Some(40.0),
            min_font_size: 5.0,
            max_font_size: 100.0,
        };
        let result_height =
            fit_text_on_n_lines("Short title", &font, &options_height_limited).unwrap();
        assert!(result_height.total_height <= 40.0 + 0.5);
    }

    #[test]
    fn test_fit_text_on_n_lines_invalid_constraints_error() {
        let font = FontVec::try_from_vec(BUNDLED_FONT.to_vec()).unwrap();

        // max_lines == 0
        let opt_zero_lines = FitTextOnNLinesOptions {
            max_lines: 0,
            ..Default::default()
        };
        assert!(matches!(
            fit_text_on_n_lines("test", &font, &opt_zero_lines),
            Err(LayoutError::InvalidConstraints(_))
        ));

        // Negative width
        let opt_neg_width = FitTextOnNLinesOptions {
            max_box_width: -100.0,
            ..Default::default()
        };
        assert!(matches!(
            fit_text_on_n_lines("test", &font, &opt_neg_width),
            Err(LayoutError::InvalidConstraints(_))
        ));

        // max_font_size < min_font_size
        let opt_inverted_size = FitTextOnNLinesOptions {
            min_font_size: 50.0,
            max_font_size: 20.0,
            ..Default::default()
        };
        assert!(matches!(
            fit_text_on_n_lines("test", &font, &opt_inverted_size),
            Err(LayoutError::InvalidConstraints(_))
        ));
    }

    #[test]
    fn test_create_rounded_text_box_svg_generation() {
        let font = FontVec::try_from_vec(BUNDLED_FONT.to_vec()).unwrap();
        let lines = vec!["Title Line".to_string(), "Subtitle Line Long".to_string()];
        let options = RoundedTextBoxOptions {
            padding_x: 16.0,
            padding_y: 8.0,
            border_radius: 12.0,
            align: TextAlign::Left,
        };

        let path = create_rounded_text_box(&lines, &font, 20.0, &options);
        assert!(path.starts_with('M'), "Path must begin with Move command");
        assert!(path.contains('L'), "Path must contain Line commands");
        assert!(
            path.contains('A'),
            "Path must contain Arc commands for rounded corners"
        );
        assert!(path.ends_with('Z'), "Path must close with Z");

        // Center alignment
        let mut center_opts = options.clone();
        center_opts.align = TextAlign::Center;
        let center_path = create_rounded_text_box(&lines, &font, 20.0, &center_opts);
        assert!(center_path.starts_with('M'));
        assert!(center_path.ends_with('Z'));

        // Right alignment
        let mut right_opts = options.clone();
        right_opts.align = TextAlign::Right;
        let right_path = create_rounded_text_box(&lines, &font, 20.0, &right_opts);
        assert!(right_path.starts_with('M'));
        assert!(right_path.ends_with('Z'));

        // Empty lines produces empty path
        let empty_path = create_rounded_text_box(&[], &font, 20.0, &options);
        assert_eq!(empty_path, "");
    }

    #[test]
    fn test_font_cache_register_font_bytes() {
        let cache = FontCache::bundled();
        let res = cache.register_font_bytes("custom_noto", BUNDLED_FONT.to_vec());
        assert!(res.is_ok());
        assert_eq!(cache.asset_count(), 1);

        // Invalid font bytes should fail gracefully
        let err_res = cache.register_font_bytes("bad_font", vec![0, 1, 2, 3]);
        assert!(err_res.is_err());
    }
}
