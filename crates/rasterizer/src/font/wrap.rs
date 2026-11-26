//! Line wrapping, ellipsis, text measuring and glyph-run shaping helpers.

use super::*;

pub(super) fn wrap_text(
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

pub(super) fn add_ellipsis(
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

pub(super) fn measure_text(
    text: &str,
    font_size: f32,
    fonts: &[Arc<LoadedFont>],
) -> Result<f32, FontLoadError> {
    let (_, advance) = shape_runs(text, font_size, fonts)?;
    Ok(advance.abs())
}

pub(super) fn font_asset_error(error: FontLoadError) -> RasterError {
    RasterError::FontAsset {
        path: error.path,
        reason: error.reason,
    }
}

pub(super) fn shape_runs(
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
                    .with_scale_and_position(font.em_scale(font_size), ab_glyph::point(x, y)),
            });
            cursor_x += g.x_advance;
        }
    }
    Ok((output, cursor_x))
}
