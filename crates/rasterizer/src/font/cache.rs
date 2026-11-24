//! Font cache: discovery, loading, glyph-run shaping data and the rasterized-bitmap caches.

use super::*;

/// Platform-specific font search paths, in preference order.
#[cfg(target_os = "macos")]
pub(super) const FONT_SEARCH_PATHS: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/System/Library/Fonts/Supplemental/Verdana.ttf",
    "/System/Library/Fonts/Supplemental/Georgia.ttf",
    "/System/Library/Fonts/SFNS.ttf",
];

#[cfg(target_os = "linux")]
pub(super) const FONT_SEARCH_PATHS: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];

#[cfg(target_os = "windows")]
pub(super) const FONT_SEARCH_PATHS: &[&str] = &[
    "C:\\Windows\\Fonts\\arial.ttf",
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\calibri.ttf",
];

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub(super) const FONT_SEARCH_PATHS: &[&str] = &[];

/// A loaded, ready-to-use font.
pub struct FontCache {
    font: Option<Arc<LoadedFont>>,
    path: Option<String>,
    assets: Mutex<HashMap<String, Arc<LoadedFont>>>,
    pub(super) rasterized: Mutex<HashMap<TextRasterKey, Arc<RenderedText>>>,
    atlas: Mutex<TextAtlas>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub(super) struct TextRasterKey {
    text: String,
    font_size_bits: u32,
    font_weight: u16,
    sources: Vec<String>,
}

pub(crate) struct LoadedFont {
    pub(crate) raster: FontVec,
    pub(crate) data: Arc<Vec<u8>>,
}

pub(super) struct ShapedGlyph {
    pub(super) font: Arc<LoadedFont>,
    pub(super) glyph: ab_glyph::Glyph,
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
    /// Pixel scale at which one em equals `font_size` pixels.
    ///
    /// `ab_glyph::PxScale` is the *line height* (ascent - descent) in pixels, not the em
    /// size, so `PxScale::from(font_size)` draws glyphs smaller than `font_size` while the
    /// shaper advances (`font_size / units_per_em`) stay em-based, which spreads letters.
    pub(crate) fn em_scale(&self, font_size: f32) -> PxScale {
        // Not `Font::pt_to_px_scale`: that also applies a 96/72 dpi factor.
        match self.raster.units_per_em() {
            Some(units_per_em) if units_per_em > 0.0 => {
                PxScale::from(font_size * self.raster.height_unscaled() / units_per_em)
            }
            _ => PxScale::from(font_size),
        }
    }

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
        let metric_ascent = fonts
            .iter()
            .map(|font| font.raster.as_scaled(font.em_scale(font_size)).ascent())
            .fold(0.0_f32, f32::max)
            .ceil();
        let metric_descent = fonts
            .iter()
            .map(|font| -font.raster.as_scaled(font.em_scale(font_size)).descent())
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
    pub(super) fn asset_count(&self) -> usize {
        self.assets.lock().expect("font cache lock poisoned").len()
    }
}

pub(super) static TEXT_LAYOUT_FONT_CACHE: OnceLock<FontCache> = OnceLock::new();

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
