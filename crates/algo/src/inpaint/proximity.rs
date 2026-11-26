//! Proximity match: the single displacement whose ring best matches the hole's ring.

use super::proximity_integral::HoleIntegral;
use super::*;

/// Proximity match: the displacement `(dx, dy)` (within `max_radius`) whose surroundings best match the
/// ring of `ring` pixels around the hole, with the displaced hole fully inside the grid and outside the
/// hole. `None` if no displacement fits.
pub fn best_offset(
    w: usize,
    h: usize,
    ch: usize,
    img: &[f32],
    hole: &[bool],
    ring: usize,
    max_radius: i32,
) -> Option<(i32, i32)> {
    let (bx0, by0, bx1, by1) = hole_bbox(w, h, hole)?;
    // No valid translation can move farther than the source image's extent.
    // Bound arbitrary public API inputs before constructing i32 search ranges:
    // usize -> i32 truncation and signed increments otherwise overflow or hang.
    let extent = w.max(h).min((i32::MAX / 4) as usize) as i32;
    let max_radius = max_radius.max(0).min(extent);
    if max_radius == 0 {
        return None;
    }
    let ring = ring.max(1).min(extent as usize) as i32;
    // Every set cell lies in the hole bounds. Counts outside that rectangle are zero,
    // so source rejection needs a table for the hole, not for the whole photo.
    let hs = HoleIntegral::new(w, hole, (bx0, by0, bx1, by1))?;
    // Sample points: the hole's ring (dilated minus hole), subsampled for speed.
    let mut pts = Vec::new();
    let (x0, y0) = (bx0 as i32 - ring, by0 as i32 - ring);
    let (x1, y1) = (bx1 as i32 + ring, by1 as i32 + ring);
    for y in y0.max(0)..y1.min(h as i32) {
        for x in x0.max(0)..x1.min(w as i32) {
            if hole[y as usize * w + x as usize] {
                continue;
            }
            if hs.count(x - ring, y - ring, x + ring + 1, y + ring + 1) > 0 {
                pts.push((x, y));
            }
        }
    }
    let stride = (pts.len() / 2000).max(1);
    let pts: Vec<(i32, i32)> = pts.into_iter().step_by(stride).collect();
    let eval = |dx: i32, dy: i32, cutoff: f32| -> Option<f32> {
        let (hx0, hy0, hx1, hy1) = (x0 + dx, y0 + dy, x1 + dx, y1 + dy);
        let inside = hx0 >= 0 && hy0 >= 0 && hx1 <= w as i32 && hy1 <= h as i32;
        if (dx, dy) == (0, 0) || !inside || hs.count(hx0, hy0, hx1, hy1) != 0 {
            return None;
        }
        let mut e = 0.0f32;
        let n = pts.len().max(1) as f32;
        let penalty = 1.0 + 1e-3 * ((dx as f32).hypot(dy as f32) / max_radius as f32);
        for (i, &(x, y)) in pts.iter().enumerate() {
            let (a, b) = (
                (y as usize * w + x as usize) * ch,
                ((y + dy) as usize * w + (x + dx) as usize) * ch,
            );
            for c in 0..ch {
                let d = img[a + c] - img[b + c];
                e += d * d;
            }
            // SSD terms are nonnegative. The partial score is a lower bound on the
            // final score; use the same division and penalty to keep float rounding
            // and strict tie ordering unchanged. Check in batches to keep this cheap.
            if i % 32 == 31 && cutoff.is_finite() && e / n * penalty >= cutoff {
                return None;
            }
        }
        // Mild preference for nearer sources on ties.
        Some(e / n * penalty)
    };
    // Coarse grid over the search window, then a full-resolution refinement around the winner.
    let step = (max_radius / 24).max(1);
    let mut best: Option<((i32, i32), f32)> = None;
    let consider = |dx: i32, dy: i32, best: &mut Option<((i32, i32), f32)>| {
        if let Some(e) = eval(dx, dy, best.map_or(f32::INFINITY, |(_, error)| error)) {
            if best.is_none_or(|(_, be)| e < be) {
                *best = Some(((dx, dy), e));
            }
        }
    };
    let mut dy = -max_radius;
    while dy <= max_radius {
        let mut dx = -max_radius;
        while dx <= max_radius {
            consider(dx, dy, &mut best);
            dx += step;
        }
        dy += step;
    }
    if step > 1 {
        if let Some(((bx, by), _)) = best {
            for dy in (by - step)..=(by + step) {
                for dx in (bx - step)..=(bx + step) {
                    consider(dx, dy, &mut best);
                }
            }
        }
    }
    best.map(|b| b.0)
}
