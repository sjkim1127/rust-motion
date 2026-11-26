//! Glyph shaping, text-box layout, N-line fitting and rounded text boxes.

use super::*;

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
    let result_height = fit_text_on_n_lines("Short title", &font, &options_height_limited).unwrap();
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
