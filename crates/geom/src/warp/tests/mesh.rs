//! Identity meshes, fitting and splitting.

use super::*;

#[test]
fn identity_mesh_is_exact_and_fit_reproduces_affine() {
    let m = BezierMesh::identity(B, 1, 1);
    assert_eq!((m.nx(), m.ny()), (4, 4));
    let w = Warp::custom(m, B);
    assert!(w.is_identity());
    assert!(close(w.map(37.5, 61.25), (37.5, 61.25), 1e-9));
    // Affine maps fit exactly.
    let f = |s: f64, t: f64| [3.0 + 2.0 * s - t, 1.0 + 0.5 * s + 4.0 * t];
    let m = BezierMesh::fit(&f, vec![0.0, 1.0], vec![0.0, 0.4, 1.0]);
    for (s, t) in [(0.1, 0.9), (0.5, 0.2), (0.77, 0.41)] {
        let p = m.eval(s, t);
        let e = f(s, t);
        assert!((p[0] - e[0]).abs() < 1e-9 && (p[1] - e[1]).abs() < 1e-9);
    }
}

#[test]
fn splits_are_exact_and_removable() {
    let mut m = BezierMesh::identity(B, 1, 1);
    // Bend it: move an inner control point.
    m.points[5] = [50.0, 10.0];
    let before: Vec<[f64; 2]> = (0..=10)
        .flat_map(|j| (0..=10).map(move |i| (i, j)))
        .map(|(i, j)| m.eval(i as f64 / 10.0, j as f64 / 10.0))
        .collect();
    let orig = m.clone();
    assert!(m.split_u(0.3));
    assert!(m.split_v(0.6));
    assert!(!m.split_u(0.3), "existing boundary");
    assert_eq!((m.nx(), m.ny()), (7, 7));
    assert!(m.is_valid());
    let after: Vec<[f64; 2]> = (0..=10)
        .flat_map(|j| (0..=10).map(move |i| (i, j)))
        .map(|(i, j)| m.eval(i as f64 / 10.0, j as f64 / 10.0))
        .collect();
    for (a, b) in before.iter().zip(&after) {
        assert!((a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
    }
    let (ls, lt) = m.param_at(m.eval(0.42, 0.77));
    assert!(
        (ls - 0.42).abs() < 1e-6 && (lt - 0.77).abs() < 1e-6,
        "{ls},{lt}"
    );
    assert!(m.remove_split_u(1));
    assert!(m.remove_split_v(1));
    assert_eq!(m.us, orig.us);
    for (a, b) in m.points.iter().zip(&orig.points) {
        assert!((a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
    }
}
