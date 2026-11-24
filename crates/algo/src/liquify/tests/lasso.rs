//! The freeze/thaw lasso against a point-in-polygon reference.

use super::*;

pub(super) fn lasso(points: &[[f64; 2]], amount: Option<f64>) -> LiquifyStroke {
    let mut s = LiquifyStroke::new(LiquifyTool::LassoMask, 1.0);
    s.points = points.iter().map(|p| p.to_vec()).collect();
    s.amount = amount;
    s
}

/// Reference even-odd test (ray casting), to check the scanline fill against.
pub(super) fn inside(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for (i, a) in poly.iter().enumerate() {
        let b = poly[j];
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[test]
fn lasso_freezes_exactly_the_nodes_inside_a_concave_polygon() {
    // A concave "C" with a notch, at a non-integer cell size.
    let poly = [
        [10.0, 8.0],
        [70.0, 8.0],
        [70.0, 20.0],
        [30.0, 22.5],
        [33.0, 40.0],
        [80.0, 41.0],
        [60.0, 58.0],
        [9.0, 55.0],
    ];
    for cell in [1.0, 2.0, 3.5] {
        let mut f = LiquifyField::new(bounds(), cell);
        let dirty = f.apply_stroke(&lasso(&poly, None));
        assert_eq!(dirty, Rect::new(9, 8, 80, 58));
        for j in 0..f.h {
            for i in 0..f.w {
                let want = if inside(f.node_pos(i, j), &poly) {
                    1.0
                } else {
                    0.0
                };
                assert_eq!(
                    f.freeze[j * f.w + i],
                    want,
                    "cell {cell}: node ({i}, {j}) at {:?}",
                    f.node_pos(i, j)
                );
            }
        }
        assert!(f.freeze.contains(&1.0));
        // Strokes replay to the same field (undo rebuilds from the list).
        assert_eq!(
            LiquifyField::from_strokes(bounds(), cell, &[lasso(&poly, None)]),
            f
        );
    }
}

#[test]
fn lasso_thaws_with_amount_zero() {
    let mut f = LiquifyField::new(bounds(), 2.0);
    f.apply_stroke(&LiquifyStroke::new(LiquifyTool::FreezeAll, 1.0));
    assert!(f.freeze.iter().all(|&v| v == 1.0));
    let square = [[20.0, 20.0], [40.0, 20.0], [40.0, 40.0], [20.0, 40.0]];
    f.apply_stroke(&lasso(&square, Some(0.0)));
    assert_eq!(f.freeze_at(30.0, 30.0), 0.0, "thawed inside");
    assert_eq!(f.freeze_at(5.0, 5.0), 1.0, "still frozen outside");
}

#[test]
fn lasso_ignores_degenerate_and_hostile_polygons() {
    let empty = LiquifyField::new(bounds(), 2.0);
    let cases: [&[[f64; 2]]; 6] = [
        &[],
        &[[10.0, 10.0], [50.0, 50.0]],
        &[[f64::NAN, 10.0], [50.0, f64::INFINITY], [20.0, 30.0]],
        // Entirely outside the canvas, on every side.
        &[[-50.0, -50.0], [-10.0, -50.0], [-10.0, -10.0]],
        &[[200.0, 10.0], [300.0, 10.0], [250.0, 50.0]],
        &[[10.0, 1e12], [50.0, 1e12], [30.0, 2e12]],
    ];
    for poly in cases {
        let mut f = empty.clone();
        assert_eq!(f.apply_stroke(&lasso(poly, None)), Rect::EMPTY, "{poly:?}");
        assert_eq!(f, empty, "{poly:?}");
    }
    // Huge coordinates around the canvas clamp to it instead of overflowing.
    let mut f = empty.clone();
    f.apply_stroke(&lasso(
        &[[-1e15, -1e15], [1e15, -1e15], [1e15, 1e15], [-1e15, 1e15]],
        None,
    ));
    assert!(f.freeze.iter().all(|&v| v == 1.0));
    // A NaN point among good ones is skipped.
    let mut f = empty.clone();
    f.apply_stroke(&lasso(
        &[
            [10.0, 10.0],
            [f64::NAN, 0.0],
            [50.0, 10.0],
            [50.0, 50.0],
            [10.0, 50.0],
        ],
        None,
    ));
    assert_eq!(f.freeze_at(30.0, 30.0), 1.0);
}
