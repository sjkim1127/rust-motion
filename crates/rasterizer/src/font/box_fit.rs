//! N-line font-size fitting and rounded text-box SVG generation.

use super::*;

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
