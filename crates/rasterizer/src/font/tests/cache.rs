//! Font cache loading, rasterization, bitmap caching and bundled-font behaviour.

use super::*;

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
fn glyphs_are_drawn_at_em_size_matching_shaper_advances() {
    let cache = FontCache::bundled();
    let size = 200.0_f32;
    let rendered = cache.rasterize("H", size, &[]).unwrap().unwrap();

    // Rows and columns that contain ink.
    let w = rendered.width as usize;
    let inked_rows = rendered
        .pixels
        .chunks(w)
        .filter(|row| row.iter().any(|&p| p > 127))
        .count() as f32;
    let inked_cols = (0..w)
        .filter(|&x| rendered.pixels.iter().skip(x).step_by(w).any(|&p| p > 127))
        .count() as f32;

    // Noto Sans: cap height 0.714 em, "H" ink width ~0.6 em. With the old line-height
    // scale the cap height came out near 0.52 em while advances stayed em-based.
    let cap_ratio = inked_rows / size;
    assert!(
        (0.68..=0.75).contains(&cap_ratio),
        "cap height should be ~0.714 em, got {cap_ratio}"
    );

    // "H" ink is ~0.74 of its advance (side bearings); with the old scale it was ~0.54.
    let advance = measure_text_width("H", size, &[]).unwrap();
    assert!(
        inked_cols / advance > 0.65,
        "ink width {inked_cols} too small for advance {advance}"
    );
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
