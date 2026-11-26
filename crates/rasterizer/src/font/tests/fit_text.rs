//! Font-size fitting and text-width measuring.

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
