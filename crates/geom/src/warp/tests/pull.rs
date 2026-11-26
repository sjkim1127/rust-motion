//! Pulling and dragging a mesh: smoothness, locality and the border.

use super::*;

/// The largest mismatch between the tangents on the two sides of any interior grid line.
fn tangent_defect(m: &BezierMesh) -> f64 {
    let nx = m.nx();
    let mut worst: f64 = 0.0;
    for a in 1..m.us.len() - 1 {
        let (hl, hr) = (m.us[a] - m.us[a - 1], m.us[a + 1] - m.us[a]);
        for j in 0..m.ny() {
            let (l, c, r) = (
                m.points[j * nx + 3 * a - 1],
                m.points[j * nx + 3 * a],
                m.points[j * nx + 3 * a + 1],
            );
            for k in 0..2 {
                worst = worst.max(((c[k] - l[k]) / hl - (r[k] - c[k]) / hr).abs());
            }
        }
    }
    for b in 1..m.vs.len() - 1 {
        let (hl, hr) = (m.vs[b] - m.vs[b - 1], m.vs[b + 1] - m.vs[b]);
        for i in 0..nx {
            let (l, c, r) = (
                m.points[(3 * b - 1) * nx + i],
                m.points[3 * b * nx + i],
                m.points[(3 * b + 1) * nx + i],
            );
            for k in 0..2 {
                worst = worst.max(((c[k] - l[k]) / hl - (r[k] - c[k]) / hr).abs());
            }
        }
    }
    worst
}

#[test]
fn dragging_an_anchor_or_handle_keeps_the_surface_smooth_across_the_lines() {
    let mut base = BezierMesh::identity([0.0, 0.0, 300.0, 300.0], 1, 1);
    for s in [0.3, 0.55, 0.9] {
        assert!(base.split_u(s));
        assert!(base.split_v(s));
    }
    let nx = base.nx();
    // Every kind of point on a grid line, interior and border: anchors, handles each way.
    for (i, j) in [
        (6, 6),
        (7, 6),
        (5, 6),
        (6, 7),
        (6, 5),
        (3, 9),
        (9, 3),
        (0, 6),
        (6, 0),
        (4, 3),
        (3, 4),
        (12, 12),
    ] {
        let mut m = base.clone();
        m.drag_point(&base.points, j * nx + i, [23.0, -31.0]);
        assert_eq!(
            m.points[j * nx + i],
            [
                base.points[j * nx + i][0] + 23.0,
                base.points[j * nx + i][1] - 31.0
            ],
            "({i}, {j}) lands on the pointer"
        );
        assert!(
            tangent_defect(&m) < 1e-6,
            "({i}, {j}): {}",
            tangent_defect(&m)
        );
    }
}

/// Fraction of the surface where the map folds over itself (its Jacobian is not positive).
fn fold_fraction(m: &BezierMesh) -> f64 {
    let (n, h) = (80, 1e-4);
    let mut bad = 0;
    for j in 0..n {
        for i in 0..n {
            let (s, t) = ((i as f64 + 0.5) / n as f64, (j as f64 + 0.5) / n as f64);
            let q = m.eval(s, t);
            let (qs, qt) = (m.eval(s + h, t), m.eval(s, t + h));
            if (qs[0] - q[0]) * (qt[1] - q[1]) - (qt[0] - q[0]) * (qs[1] - q[1]) <= 0.0 {
                bad += 1;
            }
        }
    }
    bad as f64 / (n * n) as f64
}

#[test]
fn a_pull_within_a_quarter_cell_never_folds_the_surface() {
    // 75 px cells: dragging a quarter of a cell, any direction, anywhere in the grid must not tear the picture.
    let base = BezierMesh::identity([0.0, 0.0, 300.0, 300.0], 4, 4);
    for (s, t) in [
        (0.42, 0.42),
        (0.75, 0.2),
        (0.2, 0.2),
        (0.5, 0.5),
        (0.85, 0.7),
        (0.3, 0.7),
    ] {
        for (dx, dy) in [
            (-1.0, 0.0),
            (1.0, 0.0),
            (0.0, -1.0),
            (0.0, 1.0),
            (0.7, 0.7),
            (-0.7, 0.7),
        ] {
            let mut m = base.clone();
            m.pull(&base.points, s, t, [dx * 19.0, dy * 19.0]);
            assert_eq!(
                fold_fraction(&m),
                0.0,
                "({s}, {t}) by ({dx}, {dy}) quarter cells"
            );
        }
    }
}

#[test]
fn a_pull_in_the_middle_leaves_the_border_where_it_is() {
    let mut base = BezierMesh::identity([0.0, 0.0, 600.0, 800.0], 1, 1);
    for s in [0.33, 0.66] {
        assert!(base.split_u(s));
    }
    assert!(base.split_v(0.5));
    let (nx, ny) = (base.nx(), base.ny());
    let on_border = |i: usize, j: usize| i == 0 || j == 0 || i == nx - 1 || j == ny - 1;
    let mut m = base.clone();
    m.pull(&base.points, 0.5, 0.5, [-180.0, 40.0]);
    for j in 0..ny {
        for i in 0..nx {
            if on_border(i, j) {
                let (a, b) = (m.points[j * nx + i], base.points[j * nx + i]);
                assert!(
                    (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6,
                    "border point ({i}, {j}) moved"
                );
            }
        }
    }
    // The pull itself still lands exactly.
    let (before, after) = (base.eval(0.5, 0.5), m.eval(0.5, 0.5));
    assert!(
        (after[0] - before[0] + 180.0).abs() < 1e-4 && (after[1] - before[1] - 40.0).abs() < 1e-4
    );
}

#[test]
fn a_pull_anywhere_away_from_the_border_never_moves_the_border() {
    // The default one-patch grid and a cut one; grabs all over, none within 12% of an edge.
    let one = BezierMesh::identity([0.0, 0.0, 600.0, 800.0], 1, 1);
    let mut cut = one.clone();
    assert!(cut.split_u(0.4) && cut.split_u(0.7) && cut.split_v(0.35) && cut.split_v(0.8));
    for base in [one, cut] {
        let (nx, ny) = (base.nx(), base.ny());
        for (s, t) in [
            (0.5, 0.5),
            (0.2, 0.5),
            (0.8, 0.3),
            (0.3, 0.85),
            (0.14, 0.14),
            (0.86, 0.5),
        ] {
            for d in [[-400.0, 0.0], [250.0, -90.0], [0.0, 300.0]] {
                let mut m = base.clone();
                m.pull(&base.points, s, t, d);
                for j in 0..ny {
                    for i in 0..nx {
                        if i == 0 || j == 0 || i == nx - 1 || j == ny - 1 {
                            let (a, b) = (m.points[j * nx + i], base.points[j * nx + i]);
                            assert!(
                                (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6,
                                "({s}, {t}) by {d:?} moved border point ({i}, {j})"
                            );
                        }
                    }
                }
                let (before, after) = (base.eval(s, t), m.eval(s, t));
                assert!(
                    (after[0] - before[0] - d[0]).abs() < 1e-3
                        && (after[1] - before[1] - d[1]).abs() < 1e-3,
                    "({s}, {t}) by {d:?}: {after:?}"
                );
            }
        }
    }
}

#[test]
fn pulling_on_the_border_bows_it_and_leaves_the_corners_put() {
    // A one-patch grid, grabbed on the middle of its right edge and pulled 170 px out.
    let base = BezierMesh::identity([0.0, 0.0, 800.0, 1200.0], 1, 1);
    let mut m = base.clone();
    m.pull(&base.points, 1.0, 0.5, [170.0, 0.0]);
    let at = m.eval(1.0, 0.5);
    assert!(
        (at[0] - 970.0).abs() < 1e-3 && (at[1] - 600.0).abs() < 1e-3,
        "{at:?}"
    );
    for (s, t) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        let (a, b) = (m.eval(s, t), base.eval(s, t));
        assert!(
            (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6,
            "corner ({s}, {t}) moved"
        );
    }
    // The other three edges did not move.
    for k in 0..=10 {
        let u = k as f64 / 10.0;
        for (s, t) in [(0.0, u), (u, 0.0), (u, 1.0)] {
            let (a, b) = (m.eval(s, t), base.eval(s, t));
            assert!(
                (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6,
                "edge point ({s}, {t}) moved"
            );
        }
    }
    // Only the right edge's two handles carry the pull.
    let nx = base.nx();
    let moved: Vec<usize> = (0..m.points.len())
        .filter(|k| m.points[*k] != base.points[*k])
        .collect();
    assert_eq!(moved, vec![nx + 3, 2 * nx + 3], "{moved:?}");
    assert_eq!(fold_fraction(&m), 0.0);
}

#[test]
fn pull_keeps_the_surface_smooth_across_the_grid_lines() {
    // A grid cut unevenly both ways, then pulled hard inside one cell and then another.
    let mut base = BezierMesh::identity([0.0, 0.0, 300.0, 300.0], 1, 1);
    for s in [0.3, 0.55, 0.9] {
        assert!(base.split_u(s));
        assert!(base.split_v(s));
    }
    assert!(tangent_defect(&base) < 1e-9, "cuts are exact");
    for (s, t, d) in [
        (0.42, 0.42, [60.0, -35.0]),
        (0.75, 0.2, [-40.0, 50.0]),
        (0.1, 0.95, [30.0, 30.0]),
    ] {
        let mut m = base.clone();
        m.pull(&base.points, s, t, d);
        assert!(
            tangent_defect(&m) < 1e-6,
            "({s}, {t}): {}",
            tangent_defect(&m)
        );
        let before = base.eval(s, t);
        let after = m.eval(s, t);
        assert!(
            (after[0] - before[0] - d[0]).abs() < 1e-4
                && (after[1] - before[1] - d[1]).abs() < 1e-4,
            "{after:?}"
        );
    }
}

#[test]
fn pull_moves_the_grabbed_surface_point_exactly_and_stays_local() {
    let base = BezierMesh::identity(B, 3, 2);
    for (s, t) in [(0.5, 0.5), (0.1, 0.9), (0.4, 0.25), (0.0, 0.5), (1.0, 0.3)] {
        let mut m = base.clone();
        let before = base.eval(s, t);
        m.pull(&base.points, s, t, [7.0, -4.5]);
        let after = m.eval(s, t);
        assert!(
            (after[0] - before[0] - 7.0).abs() < 1e-4 && (after[1] - before[1] + 4.5).abs() < 1e-4,
            "({s}, {t}): {after:?}"
        );
    }
    // The pull stays near the grabbed patch: the far end of the grid does not move.
    let mut m = base.clone();
    m.pull(&base.points, 0.1, 0.25, [5.0, 5.0]);
    let nx = base.nx();
    let moved = |k: usize| {
        (m.points[k][0] - base.points[k][0]).abs() > 1e-6
            || (m.points[k][1] - base.points[k][1]).abs() > 1e-6
    };
    assert!(
        (0..base.ny()).all(|j| !moved(j * nx + nx - 1)),
        "last column stays put"
    );
    assert!((0..nx).any(moved), "something moved");
    // Pulling again from the same base replaces the pull (a drag is not cumulative).
    m.pull(&base.points, 0.5, 0.25, [0.0, 0.0]);
    assert!(m
        .points
        .iter()
        .zip(&base.points)
        .all(|(a, b)| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9));
}
