//! Mesh coverage and outline following.

use super::*;

#[test]
fn mesh_covers_the_shape_only() {
    let s = blob(SampleType::U8);
    let b = s.content_bounds();
    let m = build_mesh(&s, b, PuppetDensity::Normal, 2.0);
    assert!(m.tris.len() > 20);
    let inside = |p: [f64; 2]| {
        m.tris.iter().any(|t| {
            let xs = t.map(|i| m.verts[i]);
            let (x0, x1) = (
                xs.iter().map(|v| v[0]).fold(f64::MAX, f64::min),
                xs.iter().map(|v| v[0]).fold(f64::MIN, f64::max),
            );
            let (y0, y1) = (
                xs.iter().map(|v| v[1]).fold(f64::MAX, f64::min),
                xs.iter().map(|v| v[1]).fold(f64::MIN, f64::max),
            );
            p[0] >= x0 && p[0] <= x1 && p[1] >= y0 && p[1] <= y1
        })
    };
    assert!(inside([28.0, 30.0]) && inside([60.0, 62.0]));
    assert!(
        !inside([60.0, 20.0]),
        "the empty corner of the L has no mesh"
    );
    let more = build_mesh(&s, b, PuppetDensity::More, 2.0);
    let fewer = build_mesh(&s, b, PuppetDensity::Fewer, 2.0);
    assert!(more.tris.len() > m.tris.len() && m.tris.len() > fewer.tris.len());
}

fn in_mesh(m: &PuppetMesh, p: [f64; 2]) -> bool {
    m.tris.iter().any(|t| {
        let [a, b, c] = t.map(|i| m.verts[i]);
        let (d0, d1, d2) = (orient(a, b, p), orient(b, c, p), orient(c, a, p));
        (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
    })
}

#[test]
fn mesh_follows_the_outline() {
    // #3105: the mesh was every grid cell touching the shape, a staircase up to a cell
    // outside it. It now follows the outline grown by the expansion.
    let mut s = Surface::new(PixelFormat::new(ColorMode::Rgb, SampleType::U8, true));
    for y in 40..160 {
        for x in 40..160 {
            if (f64::from(x) + 0.5 - 100.0).hypot(f64::from(y) + 0.5 - 100.0) <= 60.0 {
                s.write_pixel(x, y, &[0.3, 0.6, 0.9, 1.0]);
            }
        }
    }
    let m = build_mesh(&s, s.content_bounds(), PuppetDensity::Normal, 2.0);
    let r = |v: &[f64; 2]| (v[0] - 100.0).hypot(v[1] - 100.0);
    assert!(
        m.verts.iter().all(|v| r(v) <= 63.0),
        "a vertex outside the grown disc"
    );
    assert!(
        m.verts.iter().filter(|v| r(v) > 61.0).count() >= 40,
        "the outline carries vertices"
    );
    let area: f64 = m
        .tris
        .iter()
        .map(|t| orient(m.verts[t[0]], m.verts[t[1]], m.verts[t[2]]).abs() / 2.0)
        .sum();
    let disc = std::f64::consts::PI * 62.0 * 62.0;
    assert!((area / disc - 1.0).abs() < 0.03, "{area} vs {disc}");
    // Sharp corners and a concave corner: every opaque pixel is still inside the mesh, even
    // without an expansion, and the empty corner of the L is not.
    let l = blob(SampleType::U8);
    let m = build_mesh(&l, l.content_bounds(), PuppetDensity::Fewer, 0.0);
    for (x, y) in (20..36)
        .flat_map(|x| (10..70).map(move |y| (x, y)))
        .chain((36..70).flat_map(|x| (56..70).map(move |y| (x, y))))
    {
        assert!(
            in_mesh(&m, [f64::from(x) + 0.5, f64::from(y) + 0.5]),
            "({x}, {y}) is outside the mesh"
        );
    }
    assert!(!in_mesh(&m, [40.0, 50.0]) && !in_mesh(&m, [60.0, 20.0]));
}
