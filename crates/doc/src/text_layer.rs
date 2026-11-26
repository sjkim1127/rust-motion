//! Text layers and run normalisation.

use crate::*;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct TextLayer {
    pub text: String,
    pub font_family: String,
    pub size_pt: f32,
    pub color: Color,
    pub transform: Affine,
    /// Rasterized appearance (from PSD or our text engine).
    pub cache: Option<Surface>,
    /// Data of the PSD `TySh` block. On export it replaces the `TySh` entry in
    /// [`Layer::psd_blocks`] (the text engine updates this field when it edits text).
    pub psd_raw: Option<Arc<Vec<u8>>>,
    /// Character style runs (see [`text`]). Empty = one run built from `font_family`, `size_pt`
    /// and `color`, which otherwise mirror the first run (summary for simple UIs).
    pub runs: Vec<text::TextRun>,
    /// Paragraph style runs. Empty = default paragraph style.
    pub paragraphs: Vec<text::ParagraphRun>,
    /// Point or paragraph (box) text.
    pub shape: text::TextShape,
    pub orientation: text::Orientation,
    pub antialias: text::AntiAlias,
    /// Warp Text settings (applied to the glyph outlines when rendering).
    pub warp: Option<text::TextWarp>,
}

impl Default for TextLayer {
    fn default() -> Self {
        Self {
            text: String::new(),
            font_family: String::new(),
            size_pt: 12.0,
            color: Color::BLACK,
            transform: Affine::IDENTITY,
            cache: None,
            psd_raw: None,
            runs: Vec::new(),
            paragraphs: Vec::new(),
            shape: text::TextShape::Point,
            orientation: text::Orientation::Horizontal,
            antialias: text::AntiAlias::Smooth,
            warp: None,
        }
    }
}

impl TextLayer {
    /// Character runs covering exactly `text.len()` bytes, each on a char boundary.
    pub fn char_runs(&self) -> Vec<text::TextRun> {
        let base = text::CharStyle {
            font_family: self.font_family.clone(),
            size_pt: self.size_pt,
            color: self.color,
            ..Default::default()
        };
        normalize_runs(
            &self.text,
            self.runs.iter().map(|r| (r.len, r.style.clone())).collect(),
            base,
        )
        .into_iter()
        .map(|(len, style)| text::TextRun { len, style })
        .collect()
    }
    /// Paragraph runs covering exactly `text.len()` bytes, with an optional zero-length
    /// final run for an empty paragraph after a trailing line break.
    pub fn paragraph_runs(&self) -> Vec<text::ParagraphRun> {
        let mut runs = normalize_runs(
            &self.text,
            self.paragraphs
                .iter()
                .map(|r| (r.len, r.style.clone()))
                .collect(),
            text::ParagraphStyle::default(),
        );
        // The final empty paragraph owns the synthetic terminal paragraph marker in PSD
        // text. Its formatting has no source bytes; do not lose that last run when normalizing.
        if self.text.ends_with(['\r', '\n']) {
            if let Some((last, preceding)) = self.paragraphs.split_last() {
                if last.len <= 1
                    && preceding
                        .iter()
                        .try_fold(0usize, |n, r| n.checked_add(r.len))
                        == Some(self.text.len())
                {
                    runs.push((0, last.style.clone()));
                }
            }
        }
        runs.into_iter()
            .map(|(len, style)| text::ParagraphRun { len, style })
            .collect()
    }
    /// Copies the first run's family, size and colour into the summary fields.
    pub fn sync_summary(&mut self) {
        if let Some(r) = self.runs.first() {
            self.font_family = r.style.font_family.clone();
            self.size_pt = r.style.size_pt;
            self.color = r.style.color;
        }
    }
}

/// Stretches/truncates `(len, style)` runs to cover `text` exactly, snapping to char boundaries
/// and dropping empty runs (keeps one run for empty text).
fn normalize_runs<S: Clone>(text: &str, runs: Vec<(usize, S)>, base: S) -> Vec<(usize, S)> {
    let total = text.len();
    if total == 0 {
        return vec![(0, runs.into_iter().next().map_or(base, |r| r.1))];
    }
    let mut out: Vec<(usize, S)> = Vec::new();
    let mut at = 0usize;
    for (len, style) in runs {
        if at >= total {
            break;
        }
        let mut end = at.saturating_add(len).min(total);
        while !text.is_char_boundary(end) {
            end += 1;
        }
        if end > at {
            out.push((end - at, style));
            at = end;
        }
    }
    if at < total {
        match out.last_mut() {
            Some(last) => last.0 += total - at,
            None => out.push((total, base.clone())),
        }
    }
    if out.is_empty() {
        out.push((0, base));
    }
    out
}
