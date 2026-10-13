//! Integration & boundary tests for Milestone 3 Advanced Layout & Text Fitting Utilities.

use ab_glyph::FontVec;
use rust_motion_rasterizer::font::{
    create_rounded_text_box, create_rounded_text_box_from_measurements, fill_text_box,
    fit_text_on_n_lines, measure_text_width_with_font, FitTextOnNLinesOptions,
    RoundedTextBoxOptions, TextAlign, TextLineDimension,
};

const BUNDLED_FONT: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf");

fn get_test_font() -> FontVec {
    FontVec::try_from_vec(BUNDLED_FONT.to_vec()).expect("bundled font is valid")
}

#[test]
fn test_fill_text_box_greedy_wrapping() {
    let font = get_test_font();
    let text = "First second third fourth fifth sixth";

    // Width that fits ~2 words per line
    let lines = fill_text_box(text, &font, 20.0, 100.0);
    assert!(
        lines.len() >= 3,
        "Expected at least 3 lines, got {:?}",
        lines
    );
    for line in &lines {
        let width = measure_text_width_with_font(line, &font, 20.0);
        assert!(
            width <= 100.0 + 1.0,
            "Line '{}' width {} > 100.0",
            line,
            width
        );
    }
}

#[test]
fn test_fit_text_on_n_lines_exact_and_multiline() {
    let font = get_test_font();

    // 1. Single line fit
    let opt_1 = FitTextOnNLinesOptions {
        max_lines: 1,
        max_box_width: 250.0,
        max_box_height: None,
        min_font_size: 12.0,
        max_font_size: 100.0,
    };
    let res_1 = fit_text_on_n_lines("Hello Rust Motion", &font, &opt_1).unwrap();
    assert_eq!(res_1.lines.len(), 1);
    assert!(res_1.max_line_width <= 250.0);

    // 2. Multi-line fit allows larger font size
    let opt_3 = FitTextOnNLinesOptions {
        max_lines: 3,
        max_box_width: 250.0,
        max_box_height: None,
        min_font_size: 12.0,
        max_font_size: 100.0,
    };
    let res_3 =
        fit_text_on_n_lines("Hello Rust Motion Video Engine Layout Test", &font, &opt_3).unwrap();
    assert!(res_3.lines.len() <= 3);
    assert!(res_3.max_line_width <= 250.0);
}

#[test]
fn test_fit_text_on_n_lines_height_bound() {
    let font = get_test_font();

    let opt_unconstrained = FitTextOnNLinesOptions {
        max_lines: 5,
        max_box_width: 200.0,
        max_box_height: None,
        min_font_size: 8.0,
        max_font_size: 60.0,
    };
    let res_unconstrained =
        fit_text_on_n_lines("One Two Three Four Five", &font, &opt_unconstrained).unwrap();

    let opt_tight_height = FitTextOnNLinesOptions {
        max_lines: 5,
        max_box_width: 200.0,
        max_box_height: Some(30.0),
        min_font_size: 8.0,
        max_font_size: 60.0,
    };
    let res_tight =
        fit_text_on_n_lines("One Two Three Four Five", &font, &opt_tight_height).unwrap();
    assert!(res_tight.font_size <= res_unconstrained.font_size);
    assert!(res_tight.total_height <= 30.0 + 0.5);
}

#[test]
fn test_fit_text_on_n_lines_edge_cases() {
    let font = get_test_font();

    // Empty text
    let opt = FitTextOnNLinesOptions::default();
    let res_empty = fit_text_on_n_lines("", &font, &opt).unwrap();
    assert_eq!(res_empty.font_size, opt.max_font_size);

    // Whitespace only
    let res_space = fit_text_on_n_lines("   ", &font, &opt).unwrap();
    assert_eq!(res_space.font_size, opt.max_font_size);

    // Invalid parameters
    let invalid_lines = FitTextOnNLinesOptions {
        max_lines: 0,
        ..opt.clone()
    };
    assert!(fit_text_on_n_lines("test", &font, &invalid_lines).is_err());

    let invalid_width = FitTextOnNLinesOptions {
        max_box_width: 0.0,
        ..opt.clone()
    };
    assert!(fit_text_on_n_lines("test", &font, &invalid_width).is_err());

    let invalid_nan = FitTextOnNLinesOptions {
        max_box_width: f32::NAN,
        ..opt.clone()
    };
    assert!(fit_text_on_n_lines("test", &font, &invalid_nan).is_err());
}

#[test]
fn test_create_rounded_text_box_alignments_and_radii() {
    let font = get_test_font();
    let lines = vec![
        "Short".to_string(),
        "A Much Longer Line Of Text In Badge".to_string(),
        "Medium Line".to_string(),
    ];

    for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right] {
        let options = RoundedTextBoxOptions {
            padding_x: 20.0,
            padding_y: 10.0,
            border_radius: 16.0,
            align,
        };
        let d = create_rounded_text_box(&lines, &font, 24.0, &options);
        assert!(d.starts_with('M'));
        assert!(d.contains('A'));
        assert!(d.ends_with('Z'));
    }
}

#[test]
fn test_create_rounded_text_box_from_measurements_exact_geometry() {
    let measurements = vec![
        TextLineDimension::new(100.0, 30.0),
        TextLineDimension::new(200.0, 30.0),
    ];
    let options = RoundedTextBoxOptions {
        padding_x: 10.0,
        padding_y: 5.0,
        border_radius: 8.0,
        align: TextAlign::Left,
    };
    let d = create_rounded_text_box_from_measurements(&measurements, &options);
    assert!(!d.is_empty());
    assert!(d.starts_with('M'));
    assert!(d.ends_with('Z'));
}
