//! Combine modes and the tiled surface conversions against per-sample references.

use super::*;

#[test]
fn combine_modes() {
    let a = Rect::new(0, 0, 4, 1);
    let old = mask_to_surface(&[1.0, 1.0, 0.0, 0.0], a);
    let new = [0.0, 1.0, 1.0, 0.0];
    let v = |s: Option<Surface>| mask_from_surface(s.as_ref(), a);
    assert_eq!(
        v(combine(Some(&old), &new, a, SelectionMode::Replace)),
        vec![0.0, 1.0, 1.0, 0.0]
    );
    assert_eq!(
        v(combine(Some(&old), &new, a, SelectionMode::Add)),
        vec![1.0, 1.0, 1.0, 0.0]
    );
    assert_eq!(
        v(combine(Some(&old), &new, a, SelectionMode::Subtract)),
        vec![1.0, 0.0, 0.0, 0.0]
    );
    assert_eq!(
        v(combine(Some(&old), &new, a, SelectionMode::Intersect)),
        vec![0.0, 1.0, 0.0, 0.0]
    );
    assert!(combine(Some(&old), &[1.0; 4], a, SelectionMode::Subtract).is_none());
    assert_eq!(SelectionMode::parse("add"), SelectionMode::Add);
}

/// The previous (per-sample) conversions, kept as the oracle for the tiled fast paths.
fn old_mask_to_surface(m: &[f32], area: Rect) -> Surface {
    let mut s = Surface::new(PixelFormat::GRAY8);
    if !area.is_empty() {
        s.write_region(area, m);
    }
    s.prune();
    s
}

pub(super) fn old_combine(
    old: Option<&Surface>,
    new: &[f32],
    area: Rect,
    mode: SelectionMode,
) -> Option<Surface> {
    let mut prev = Vec::new();
    match old {
        Some(s) => s.read_region_into(area, &mut prev),
        None => prev = vec![0.0; new.len()],
    }
    let q: Vec<f32> = new
        .iter()
        .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() / 255.0)
        .collect();
    let out: Vec<f32> = match mode {
        SelectionMode::Replace => q,
        SelectionMode::Add => prev.iter().zip(&q).map(|(a, b)| a.max(*b)).collect(),
        SelectionMode::Subtract => prev.iter().zip(&q).map(|(a, b)| (a - b).max(0.0)).collect(),
        SelectionMode::Intersect => prev.iter().zip(&q).map(|(a, b)| a.min(*b)).collect(),
    };
    let out: Vec<f32> = out
        .into_iter()
        .map(|v| if v < 0.5 / 255.0 { 0.0 } else { v })
        .collect();
    if out.iter().all(|v| *v <= 0.0) {
        return None;
    }
    Some(old_mask_to_surface(&out, area))
}

#[test]
fn tiled_conversions_match_per_sample_ones() {
    let mut rng = crate::photo_util::Rng(7);
    let mut u = move || (rng.next() >> 11) as f32 / (1u64 << 53) as f32;
    // Off-grid areas spanning several tiles, including negative origins.
    // The last one holds whole tiles, which fully selected masks fill (shared tiles).
    for area in [
        Rect::new(0, 0, 300, 20),
        Rect::new(-37, 250, 530, 263),
        Rect::new(255, -3, 258, 600),
        Rect::new(5, 5, 6, 6),
        Rect::new(-37, -300, 530, 263),
    ] {
        let n = area.width() as usize * area.height() as usize;
        let old_m: Vec<f32> = (0..n)
            .map(|i| if i % 97 < 40 { u() } else { 0.0 })
            .collect();
        let old = old_mask_to_surface(&old_m, area);
        let fast = mask_to_surface(&old_m, area);
        assert_eq!(fast, old, "{area:?}");
        let mut slow_read = Vec::new();
        old.read_region_into(area, &mut slow_read);
        assert_eq!(mask_from_surface(Some(&old), area), slow_read);
        for kind in 0..4 {
            let new: Vec<f32> = (0..n)
                .map(|i| match kind {
                    0 => u() * 1.2 - 0.1,
                    1 => {
                        if i % 131 < 50 {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    2 => 0.001,
                    _ => f32::from(u8::from(i >= 1000)),
                })
                .collect();
            for mode in [
                SelectionMode::Replace,
                SelectionMode::Add,
                SelectionMode::Subtract,
                SelectionMode::Intersect,
            ] {
                for prev in [None, Some(&old)] {
                    assert_eq!(
                        combine(prev, &new, area, mode),
                        old_combine(prev, &new, area, mode),
                        "{area:?} {kind} {mode:?}"
                    );
                }
            }
        }
    }
    // A short mask buffer reads as zeros instead of panicking.
    let a = Rect::new(0, 0, 10, 10);
    assert!(combine(None, &[1.0; 5], a, SelectionMode::Replace).is_some());
    assert_eq!(mask_to_surface(&[], a).tile_count(), 0);
    assert!(combine(None, &[], Rect::EMPTY, SelectionMode::Replace).is_none());
}
