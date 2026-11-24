//! Text-box layout, width measuring and single-line font-size fitting.

use super::*;

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
        .map(|font| font.raster.as_scaled(font.em_scale(font_size)).ascent())
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
