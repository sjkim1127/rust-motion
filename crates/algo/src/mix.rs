//! Mixing filtered samples back toward the original pixels by the selection's coverage.

use super::*;

/// Mixes filtered `data` for `t` back toward the original pixels in `orig` by the selection's
/// coverage (channel 0 of `sel`). Changing alpha requires premultiplied colour interpolation;
/// otherwise hidden colours leak into feathered edges and alpha-removing filters darken them.
pub(crate) fn mix_selection(data: &mut [f32], t: Rect, sel: &Surface, orig: &Image, alpha: bool) {
    let w = t.width() as usize;
    // The selection over `t` read once, not a lookup and an allocation per pixel (#4533).
    let mask = sel.read_region(t);
    let sc = sel.channels().max(1);
    for (i, px) in data.chunks_exact_mut(orig.ch.max(1)).enumerate() {
        let (x, y) = (t.x0 + (i % w) as i32, t.y0 + (i / w) as i32);
        let k = mask.get(i * sc).copied().unwrap_or(0.0).clamp(0.0, 1.0);
        if k >= 1.0 {
            continue;
        }
        if k <= 0.0 {
            for (c, v) in px.iter_mut().enumerate() {
                *v = orig.get(x, y, c);
            }
            continue;
        }
        if alpha {
            let a = px.len() - 1;
            let original_alpha = orig.get(x, y, a);
            let filtered_alpha = px[a];
            if original_alpha != filtered_alpha {
                let mixed_alpha = original_alpha + (filtered_alpha - original_alpha) * k;
                let original_weight = original_alpha * (1.0 - k);
                let filtered_weight = filtered_alpha * k;
                for (c, v) in px[..a].iter_mut().enumerate() {
                    *v = if mixed_alpha > 0.0 {
                        (orig.get(x, y, c) * original_weight + *v * filtered_weight) / mixed_alpha
                    } else {
                        0.0
                    };
                }
                px[a] = mixed_alpha;
                continue;
            }
        }
        for (c, v) in px.iter_mut().enumerate() {
            let o = orig.get(x, y, c);
            *v = o + (*v - o) * k;
        }
    }
}
