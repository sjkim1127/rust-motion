//! Tests for alignment, focal length and projections.

use super::*;
use crate::transform::Homography;

#[test]
fn recovers_translations_and_homographies() {
    let (sw, sh) = (640, 300);
    let sc = scene(sw, sh, 3);
    let (tw, th) = (260, 220);
    // Three overlapping crops at known offsets.
    let offs = [(20.0, 30.0), (190.0, 40.0), (360.0, 25.0)];
    let imgs: Vec<Vec<f32>> = offs
        .iter()
        .map(|&(ox, oy)| view(&sc, sw, sh, tw, th, |x, y| (x + ox, y + oy)))
        .collect();
    let feats: Vec<ImageFeatures> = imgs
        .iter()
        .map(|im| detect(tw, th, im, None, 700))
        .collect();
    let matches = match_pairs(&feats);
    assert!(matches.len() >= 2, "{}", matches.len());
    for layout in [Layout::Reposition, Layout::Collage, Layout::Perspective] {
        let a = align(
            &feats,
            &matches,
            &AlignOptions {
                layout,
                focal: None,
                reference: Some(1),
                geometric: false,
            },
        )
        .unwrap();
        assert_eq!(a.reference, 1);
        for (i, p) in a.placements.iter().enumerate() {
            let p = p.as_ref().unwrap();
            // A point at image px (50, 60) is scene (50+ox, 60+oy); relative to image 1.
            let (u, v) = p.forward(50.0, 60.0);
            let (eu, ev) = (50.0 + offs[i].0 - offs[1].0, 60.0 + offs[i].1 - offs[1].1);
            assert!(
                (u - eu).abs() < 1.0 && (v - ev).abs() < 1.0,
                "{layout:?} image {i}: ({u},{v}) vs ({eu},{ev})"
            );
        }
        assert!(a.rms < 1.0, "{layout:?} rms {}", a.rms);
    }
    // A projective view is recovered by the perspective layout.
    let hq = Homography([1.05, 0.03, 180.0, -0.02, 1.0, 35.0, 0.0001, -0.00005, 1.0]);
    let im2 = view(&sc, sw, sh, tw, th, |x, y| hq.apply(x, y));
    let feats2 = vec![feats[0].clone(), detect(tw, th, &im2, None, 700)];
    let m2 = match_pairs(&feats2);
    let a = align(
        &feats2,
        &m2,
        &AlignOptions {
            layout: Layout::Perspective,
            focal: None,
            reference: Some(0),
            geometric: false,
        },
    )
    .unwrap();
    let p = a.placements[1].as_ref().unwrap();
    // Points inside the overlap (extrapolation beyond it is loosely constrained).
    for (x, y) in [(20.0, 30.0), (70.0, 180.0), (40.0, 100.0)] {
        let (sx, sy) = hq.apply(x, y);
        let (u, v) = p.forward(x, y);
        assert!(
            (u - (sx - 20.0)).abs() < 1.5 && (v - (sy - 30.0)).abs() < 1.5,
            "({u},{v}) vs ({},{})",
            sx - 20.0,
            sy - 30.0
        );
    }
}

#[test]
fn rotating_camera_focal_and_cylindrical_layout() {
    // Views of a cylinder-mapped scene from a camera panning by 25° with f = 300 px.
    let f = 300.0f64;
    let (sw, sh) = (1400, 500);
    let sc = scene(sw, sh, 9);
    let (tw, th) = (320, 240);
    let pans = [-25f64, 0.0, 25.0];
    let imgs: Vec<Vec<f32>> = pans
        .iter()
        .map(|&deg| {
            view(&sc, sw, sh, tw, th, |x, y| {
                let (dx, dy) = (x - tw as f64 / 2.0, y - th as f64 / 2.0);
                let th_ = dx.atan2(f) + deg.to_radians();
                let hh = dy / dx.hypot(f);
                (sw as f64 / 2.0 + f * th_, sh as f64 / 2.0 + f * hh)
            })
        })
        .collect();
    let feats: Vec<ImageFeatures> = imgs
        .iter()
        .map(|im| detect(tw, th, im, None, 700))
        .collect();
    let matches = match_pairs(&feats);
    assert_eq!(matches.iter().filter(|m| m.b == m.a + 1).count(), 2);
    let est: Vec<f64> = matches
        .iter()
        .filter_map(|m| focal_from_homography(&m.homography, [160.0, 120.0], [160.0, 120.0]))
        .collect();
    assert!(
        est.iter().any(|e| (e - f).abs() / f < 0.15),
        "focal estimates {est:?}"
    );
    let a = align(
        &feats,
        &matches,
        &AlignOptions {
            layout: Layout::Cylindrical,
            focal: None,
            reference: Some(1),
            geometric: false,
        },
    )
    .unwrap();
    assert!((a.focal - f).abs() / f < 0.1, "focal {}", a.focal);
    let p = a.placements[2].as_ref().unwrap();
    // The centre of the right image sits 25° along the cylinder.
    let (u, _) = p.forward(160.0, 120.0);
    let expect = 160.0 + a.focal * 25f64.to_radians();
    assert!((u - expect).abs() < 4.0, "{u} vs {expect}");
    assert!(a.rms < 1.5, "rms {}", a.rms);
    // Round trip through the placement.
    let (u, v) = p.forward(40.0, 200.0);
    let (x, y) = p.inverse(u, v).unwrap();
    assert!((x - 40.0).abs() < 1e-6 && (y - 200.0).abs() < 1e-6);
}

#[test]
fn placement_round_trips_all_projections() {
    for projection in [Projection::Plane, Projection::Cylinder, Projection::Sphere] {
        let p = Placement {
            projection,
            focal: 400.0,
            center: [200.0, 150.0],
            k1: 0.05,
            h: Homography([0.98, -0.1, 30.0, 0.1, 0.98, -12.0, 0.0, 0.0, 1.0]),
        };
        for (x, y) in [(0.0, 0.0), (390.0, 20.0), (123.0, 280.0)] {
            let (u, v) = p.forward(x, y);
            let (bx, by) = p.inverse(u, v).unwrap();
            assert!(
                (bx - x).abs() < 1e-6 && (by - y).abs() < 1e-6,
                "{projection:?}"
            );
        }
        let s = p.scaled(2.0);
        let (u, v) = p.forward(100.0, 50.0);
        let (u2, v2) = s.forward(200.0, 100.0);
        assert!((u2 - 2.0 * u).abs() < 1e-6 && (v2 - 2.0 * v).abs() < 1e-6);
    }
}
