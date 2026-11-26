//! Anti-aliased polygon (lasso) coverage.

use super::*;

/// Anti-aliased polygon (even-odd) coverage over `area`: exact horizontal
/// coverage, 4 sub-rows per pixel row (or 1 when not anti-aliased). Bands of
/// rows are filled in parallel, and each sub-row tests only the edges that
/// cross it (an active edge list over the edges sorted by their top).
pub fn polygon(points: &[(f32, f32)], area: Rect, anti_alias: bool) -> Vec<f32> {
    let (w, h) = (area.width() as usize, area.height() as usize);
    let mut m = vec![0.0f32; w * h];
    if points.len() < 3 || w == 0 {
        return m;
    }
    let sub = if anti_alias { 4 } else { 1 };
    let sample_y = |py: usize, s: usize| area.y0 as f32 + py as f32 + (s as f32 + 0.5) / sub as f32;
    // A sub-row at y crosses the edge a→b iff top <= y < bottom; horizontal and NaN edges
    // never cross one.
    struct Edge {
        top: f32,
        bottom: f32,
        a: (f32, f32),
        b: (f32, f32),
    }
    let mut edges: Vec<Edge> = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .filter_map(|(&a, &b)| {
            if a.1 < b.1 {
                Some(Edge {
                    top: a.1,
                    bottom: b.1,
                    a,
                    b,
                })
            } else if b.1 < a.1 {
                Some(Edge {
                    top: b.1,
                    bottom: a.1,
                    a,
                    b,
                })
            } else {
                None
            }
        })
        .collect();
    edges.sort_by(|p, q| p.top.total_cmp(&q.top));
    const BAND: usize = 64;
    par_rows(&mut m, w, BAND, |bi, rows| {
        let py0 = bi * BAND;
        let (first, last) = (
            sample_y(py0, 0),
            sample_y(py0 + rows.len() / w - 1, sub - 1),
        );
        let reach = edges.partition_point(|e| e.top <= last);
        let band_edges = edges.get(..reach).unwrap_or_default();
        let mut pending = band_edges.iter().filter(|e| e.bottom > first).peekable();
        let mut active: Vec<&Edge> = Vec::new();
        let mut xs: Vec<f32> = Vec::new();
        for (r, row) in rows.chunks_exact_mut(w).enumerate() {
            for s in 0..sub {
                // Sub-row ys never decrease, so an edge whose bottom is passed stays out.
                let y = sample_y(py0 + r, s);
                while let Some(e) = pending.next_if(|e| e.top <= y) {
                    active.push(e);
                }
                active.retain(|e| e.bottom > y);
                if active.is_empty() {
                    continue;
                }
                xs.clear();
                xs.extend(
                    active
                        .iter()
                        .map(|&&Edge { a, b, .. }| a.0 + (y - a.1) / (b.1 - a.1) * (b.0 - a.0)),
                );
                xs.sort_by(|a, b| a.total_cmp(b));
                for span in xs.as_chunks::<2>().0 {
                    let (x0, x1) = (
                        (span[0] - area.x0 as f32).max(0.0),
                        (span[1] - area.x0 as f32).min(w as f32),
                    );
                    if x1 <= x0 {
                        continue;
                    }
                    let (c0, c1) = (x0.floor() as usize, (x1.ceil() as usize).min(w));
                    for (cx, v) in row.iter_mut().enumerate().take(c1).skip(c0) {
                        let cov = if anti_alias {
                            ((cx + 1) as f32).min(x1) - (cx as f32).max(x0)
                        } else {
                            let c = cx as f32 + 0.5;
                            if c >= x0 && c < x1 {
                                1.0
                            } else {
                                0.0
                            }
                        };
                        *v += cov.max(0.0) / sub as f32;
                    }
                }
            }
        }
        for v in rows {
            *v = v.min(1.0);
        }
    });
    m
}
