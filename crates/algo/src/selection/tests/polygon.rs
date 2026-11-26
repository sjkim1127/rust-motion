//! Polygon coverage against the every-edge-fill reference.

use super::*;

#[test]
fn polygon_area_and_aa() {
    let a = Rect::new(0, 0, 20, 20);
    let sq = polygon(
        &[(2.0, 2.0), (12.0, 2.0), (12.0, 12.0), (2.0, 12.0)],
        a,
        true,
    );
    let area: f32 = sq.iter().sum();
    assert!((area - 100.0).abs() < 1e-3, "{area}");
    let tri = polygon(&[(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)], a, true);
    let area: f32 = tri.iter().sum();
    assert!((area - 50.0).abs() < 1.0, "{area}");
    assert!(tri.iter().any(|v| *v > 0.0 && *v < 1.0));
    let hard = polygon(&[(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)], a, false);
    assert!(hard.iter().all(|v| *v == 0.0 || *v == 1.0));
    assert!(polygon(&[(0.0, 0.0), (1.0, 1.0)], a, true)
        .iter()
        .all(|v| *v == 0.0));
}

/// The previous polygon fill (every edge tested on every sub-row), kept as the oracle.
fn old_polygon(points: &[(f32, f32)], area: Rect, anti_alias: bool) -> Vec<f32> {
    let (w, h) = (area.width() as usize, area.height() as usize);
    let mut m = vec![0.0f32; w * h];
    if points.len() < 3 {
        return m;
    }
    let sub = if anti_alias { 4 } else { 1 };
    let mut xs: Vec<f32> = Vec::new();
    for py in 0..h {
        for s in 0..sub {
            let y = area.y0 as f32 + py as f32 + (s as f32 + 0.5) / sub as f32;
            xs.clear();
            for i in 0..points.len() {
                let (a, b) = (points[i], points[(i + 1) % points.len()]);
                if (a.1 <= y && b.1 > y) || (b.1 <= y && a.1 > y) {
                    xs.push(a.0 + (y - a.1) / (b.1 - a.1) * (b.0 - a.0));
                }
            }
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
                for cx in c0..c1 {
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
                    m[py * w + cx] += cov.max(0.0) / sub as f32;
                }
            }
        }
    }
    for v in &mut m {
        *v = v.min(1.0);
    }
    m
}

#[test]
fn polygon_matches_the_every_edge_fill_bit_for_bit() {
    let mut seed = 0x2545_f491_u32;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    // Taller than one 64-row band, offset, and a 1-pixel-wide area.
    let areas = [
        Rect::new(0, 0, 90, 150),
        Rect::new(-13, 7, 60, 211),
        Rect::new(5, -40, 6, 30),
    ];
    let mut cases: Vec<Vec<(f32, f32)>> = vec![
        // Self-intersecting star, horizontal edges, repeated points, collinear runs.
        (0..11)
            .map(|i| {
                let t = i as f32 * 4.0 * std::f32::consts::PI / 11.0;
                (40.0 + 35.0 * t.cos(), 80.0 + 70.0 * t.sin())
            })
            .collect(),
        vec![
            (10.0, 10.0),
            (50.0, 10.0),
            (50.0, 10.0),
            (50.0, 60.0),
            (30.0, 60.0),
            (30.0, 30.0),
            (10.0, 30.0),
        ],
        vec![
            (0.0, 0.0),
            (20.0, 0.0),
            (40.0, 0.0),
            (40.0, 0.0),
            (0.0, 0.0),
        ],
        // Vertices on the sub-row centres and the pixel edges.
        vec![
            (3.125, 2.375),
            (70.5, 2.375),
            (70.875, 140.625),
            (3.0, 140.0),
            (35.0, 64.0),
            (35.0, 64.125),
        ],
        // Entirely outside, and straddling every side.
        vec![(-500.0, -500.0), (-400.0, -500.0), (-450.0, -300.0)],
        vec![(-80.0, -60.0), (300.0, 20.0), (60.0, 400.0)],
        // Non-finite coordinates and a negative zero.
        vec![
            (10.0, 10.0),
            (f32::NAN, 40.0),
            (60.0, 70.0),
            (20.0, f32::INFINITY),
            (-0.0, -0.0),
            (0.0, 0.0),
        ],
    ];
    for n in [3, 7, 40, 400] {
        for spread in [1.0, 2.0] {
            cases.push(
                (0..n)
                    .map(|_| {
                        (
                            rnd() * 100.0 * spread - 50.0 * (spread - 1.0),
                            rnd() * 220.0 * spread - 60.0 * spread,
                        )
                    })
                    .collect(),
            );
            // Lasso-like: a random walk snapped to quarter pixels.
            let (mut x, mut y) = (45.0f32, 75.0f32);
            cases.push(
                (0..n)
                    .map(|_| {
                        x = ((x + (rnd() - 0.5) * 12.0 * spread) * 4.0).round() / 4.0;
                        y = ((y + (rnd() - 0.5) * 12.0 * spread) * 4.0).round() / 4.0;
                        (x, y)
                    })
                    .collect(),
            );
        }
    }
    for pts in &cases {
        for &area in &areas {
            for aa in [true, false] {
                let (new, old) = (polygon(pts, area, aa), old_polygon(pts, area, aa));
                assert_eq!(new.len(), old.len());
                let diff = new
                    .iter()
                    .zip(&old)
                    .position(|(a, b)| a.to_bits() != b.to_bits());
                assert!(
                    diff.is_none(),
                    "{pts:?} {area:?} aa={aa}: first difference at {diff:?}"
                );
            }
        }
    }
}
