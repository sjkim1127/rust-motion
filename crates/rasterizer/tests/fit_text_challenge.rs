use rust_motion_rasterizer::{fit_text, measure_text_width};

// ── 1. Empty and Single-Character Inputs ──────────────────────────────────────

#[test]
fn test_fit_text_empty_and_single_char() {
    let empty = fit_text("", 200.0, &[], 10.0, 50.0).unwrap();
    assert_eq!(
        empty, 50.0,
        "Empty string must immediately return max_font_size"
    );

    // Single ASCII char
    let char_a = fit_text("A", 20.0, &[], 8.0, 48.0).unwrap();
    assert!(
        (8.0..=48.0).contains(&char_a),
        "Single char must be in [8, 48]"
    );

    // Single space
    let space = fit_text(" ", 20.0, &[], 8.0, 48.0).unwrap();
    assert!((8.0..=48.0).contains(&space));

    // Single punctuation
    let dot = fit_text(".", 20.0, &[], 8.0, 48.0).unwrap();
    assert!((8.0..=48.0).contains(&dot));

    // Single CJK char
    let cjk = fit_text("한", 50.0, &[], 8.0, 60.0).unwrap();
    assert!((8.0..=60.0).contains(&cjk));

    // Single Emoji
    let emoji = fit_text("🦀", 50.0, &[], 8.0, 60.0).unwrap();
    assert!((8.0..=60.0).contains(&emoji));
}

// ── 2. Huge Strings (10,000+ characters) ──────────────────────────────────────

#[test]
fn test_fit_text_huge_strings_stress() {
    // 10,000 character string
    let huge_text = "The quick brown fox jumps over the lazy dog. ".repeat(250);
    assert!(huge_text.len() >= 10000);

    // For a tight max_width like 500px, 10,000 chars will never fit -> returns min_font_size
    let size = fit_text(&huge_text, 500.0, &[], 12.0, 72.0).unwrap();
    assert_eq!(
        size, 12.0,
        "Massive text overflow must return min_font_size without panic"
    );

    // 50,000 character string
    let colossal_text = "A".repeat(50_000);
    let col_size = fit_text(&colossal_text, 200.0, &[], 10.0, 50.0).unwrap();
    assert_eq!(col_size, 10.0);
}

// ── 3. Unicode Scripts: CJK, RTL Arabic, Emojis, Mixed ─────────────────────────

#[test]
fn test_fit_text_unicode_scripts() {
    let test_cases = [
        (
            "Korean Hangul",
            "안녕하세요 Rust Motion 비디오 렌더러 테스트",
        ),
        (
            "Japanese Kanji/Kana",
            "こんにちは世界、Rustで高速な動画生成",
        ),
        ("Simplified Chinese", "你好，世界！基于 Rust 的视频渲染引擎"),
        (
            "Arabic RTL",
            "مرحبا بالعالم - تجربة ملاءمة النص في ديوكس سكوت",
        ),
        ("Emoji sequence", "🎬 🦀 🚀 ✨ 🔥 💡 🎨 📦 🌟 ⚡"),
        (
            "Mixed multilingual",
            "Hello 世界 🦀 مرحبا 123 !@# ABC 대한민국",
        ),
    ];

    for (name, text) in test_cases {
        let size = fit_text(text, 300.0, &[], 8.0, 64.0);
        assert!(
            size.is_ok(),
            "Script '{name}' failed with error: {:?}",
            size.err()
        );
        let font_size = size.unwrap();
        assert!(
            (8.0..=64.0).contains(&font_size),
            "Script '{name}' size {font_size} out of bounds"
        );

        // Verification: measured width at the returned font size must be <= max_width (or at min)
        let measured = measure_text_width(text, font_size as f32, &[]).unwrap();
        if font_size > 8.0 + 0.1 {
            assert!(
                measured as f64 <= 300.0 + 1.0, // slight sub-pixel rounding allowance
                "Script '{name}' font_size {font_size} gave width {measured} > 300.0"
            );
        }
    }
}

// ── 4. Non-Finite Inputs (NaN, Infinity) ───────────────────────────────────────

#[test]
fn test_fit_text_non_finite_inputs_rejected() {
    // NaN in max_width
    assert!(fit_text("test", f64::NAN, &[], 8.0, 48.0).is_err());
    // NaN in min_font_size
    assert!(fit_text("test", 200.0, &[], f64::NAN, 48.0).is_err());
    // NaN in max_font_size
    assert!(fit_text("test", 200.0, &[], 8.0, f64::NAN).is_err());

    // Infinity in max_width
    assert!(fit_text("test", f64::INFINITY, &[], 8.0, 48.0).is_err());
    assert!(fit_text("test", f64::NEG_INFINITY, &[], 8.0, 48.0).is_err());

    // Infinity in min_font_size
    assert!(fit_text("test", 200.0, &[], f64::INFINITY, 48.0).is_err());
    assert!(fit_text("test", 200.0, &[], f64::NEG_INFINITY, 48.0).is_err());

    // Infinity in max_font_size
    assert!(fit_text("test", 200.0, &[], 8.0, f64::INFINITY).is_err());
    assert!(fit_text("test", 200.0, &[], 8.0, f64::NEG_INFINITY).is_err());
}

// ── 5. Negative and Invalid Bounds ───────────────────────────────────────────

#[test]
fn test_fit_text_negative_and_invalid_bounds_rejected() {
    // max_width <= 0
    assert!(fit_text("test", 0.0, &[], 8.0, 48.0).is_err());
    assert!(fit_text("test", -10.0, &[], 8.0, 48.0).is_err());

    // min_font_size <= 0
    assert!(fit_text("test", 200.0, &[], 0.0, 48.0).is_err());
    assert!(fit_text("test", 200.0, &[], -5.0, 48.0).is_err());

    // max_font_size < min_font_size
    assert!(fit_text("test", 200.0, &[], 48.0, 8.0).is_err());

    // max_font_size > 4096
    assert!(fit_text("test", 200.0, &[], 8.0, 4097.0).is_err());
    assert!(fit_text("test", 200.0, &[], 8.0, 10000.0).is_err());
}

// ── 6. Equal Min and Max Font Sizes ───────────────────────────────────────────

#[test]
fn test_fit_text_equal_min_and_max_sizes() {
    // Case 1: text fits at 24.0
    let fit = fit_text("Hi", 500.0, &[], 24.0, 24.0).unwrap();
    assert_eq!(fit, 24.0);

    // Case 2: text does not fit at 24.0 -> returns min_font_size (24.0)
    let overflow = fit_text("Super Long Text That Exceeds Width", 10.0, &[], 24.0, 24.0).unwrap();
    assert_eq!(overflow, 24.0);
}

// ── 7. Tight Constraints and Binary Search Monotonicity ───────────────────────

#[test]
fn test_fit_text_tight_constraints_and_monotonicity() {
    // Very tight max_width
    let tight = fit_text("Hello World", 0.001, &[], 10.0, 50.0).unwrap();
    assert_eq!(tight, 10.0, "Tight width must fall back to min_font_size");

    // Very large max_width
    let huge_width = fit_text("Hello World", 1_000_000.0, &[], 10.0, 50.0).unwrap();
    assert_eq!(
        huge_width, 50.0,
        "Huge width must immediately return max_font_size"
    );

    // Monotonicity test: As width increases, fitted font size must never decrease
    let text = "Rust Motion Declarative Video Engine";
    let widths = [50.0, 100.0, 200.0, 300.0, 500.0, 800.0, 1200.0, 2000.0];
    let mut prev_size = 0.0f64;

    for &w in &widths {
        let size = fit_text(text, w, &[], 8.0, 120.0).unwrap();
        assert!(
            size >= prev_size - 1e-4,
            "Monotonicity violation: for width {w}, size {size} < prev_size {prev_size}"
        );
        prev_size = size;
    }
}
