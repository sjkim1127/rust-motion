//! Each tool's brush falloff and displacement sense.

use super::*;

#[test]
fn the_brush_fades_to_nothing_at_its_rim_at_every_density() {
    for density in [0.0, 0.25, 0.5, 0.75, 1.0, f64::NAN, -3.0, 7.0] {
        assert_eq!(
            falloff(0.0, density),
            1.0,
            "full strength at the centre ({density})"
        );
        assert_eq!(falloff(1.0, density), 0.0, "nothing at the rim ({density})");
        // Flat at the rim: the last 2 % of the radius moves at most a couple of percent.
        assert!(
            falloff(0.98f64.powi(2), density) < 0.02,
            "{density}: {}",
            falloff(0.98f64.powi(2), density)
        );
        let mut prev = 1.0;
        for k in 1..=100 {
            let v = falloff(k as f64 / 100.0, density);
            assert!(v <= prev && v >= 0.0, "{density}: not falling at t² = {k}%");
            prev = v;
        }
    }
    // Density widens the strong core: more strength at every radius, as in Photoshop.
    for k in 1..100 {
        let t2 = k as f64 / 100.0;
        assert!(
            falloff(t2, 1.0) >= falloff(t2, 0.5) && falloff(t2, 0.5) >= falloff(t2, 0.0),
            "t² = {t2}"
        );
    }
    assert!(falloff(0.5, 1.0) > 0.5 && falloff(0.5, 0.0) < 0.2);
}

/// #2606: the dab's strength used to stop at 1 - Density at its rim, so a Forward Warp moved
/// everything just inside the brush and nothing just outside, tearing the image along every
/// dab's circle (a hard-edged block at Density 0, half strength at the default 50).
#[test]
fn forward_warp_leaves_the_brush_rim_in_place() {
    // A drag of one dab spacing (a twentieth of the radius): one dab, centred where it started.
    let (r, drag) = (20.0, 1.0);
    for density in [0.0, 50.0, 100.0] {
        let mut s = LiquifyStroke::new(LiquifyTool::ForwardWarp, 2.0 * r);
        s.density = density;
        s.points = vec![vec![48.0, 32.0], vec![48.0 + drag, 32.0]];
        let f = LiquifyField::from_strokes(bounds(), 1.0, &[s]);
        let mut rim = 0;
        for j in 0..f.h {
            for i in 0..f.w {
                let p = f.node_pos(i, j);
                let t = (p[0] - 48.0).hypot(p[1] - 32.0) / r;
                if (0.97..1.0).contains(&t) {
                    let d = f.d[j * f.w + i];
                    assert!(
                        f64::from(d[0].hypot(d[1])) < 0.03 * drag,
                        "density {density}: a node at {t:.3} r moved {d:?}"
                    );
                    rim += 1;
                }
            }
        }
        assert!(rim > 20, "{rim} rim nodes");
        assert!(
            f.sample(48.0, 32.0)[0] < -0.9 * drag,
            "density {density}: the centre follows the brush"
        );
    }
}

#[test]
fn forward_warp_displaces_along_the_stroke_and_reconstruct_restores() {
    let mut s = LiquifyStroke::new(LiquifyTool::ForwardWarp, 40.0);
    s.points = vec![vec![40.0, 32.0], vec![52.0, 32.0]];
    let mut f = LiquifyField::from_strokes(bounds(), 2.0, &[s]);
    // Content moved right: the output at the end point samples from the left.
    let d = f.sample(52.0, 32.0);
    assert!(d[0] < -4.0 && d[1].abs() < 0.5, "{d:?}");
    // A pixel-level check: a marker column follows the stroke.
    let mut img = Surface::new(PixelFormat::new(
        ColorMode::Grayscale,
        SampleType::F32,
        true,
    ));
    img.fill_rect(bounds(), &[0.0, 1.0]);
    img.fill_rect(Rect::new(40, 28, 42, 36), &[1.0, 1.0]);
    let out = apply_liquify(&img, &f);
    let brightest = (30..70)
        .max_by(|a, b| out.pixel(*a, 32)[0].total_cmp(&out.pixel(*b, 32)[0]))
        .unwrap();
    assert!(brightest > 44, "marker moved right: {brightest}");
    // Reconstruct All restores the original exactly.
    let mut rec = LiquifyStroke::new(LiquifyTool::ReconstructAll, 10.0);
    rec.amount = Some(100.0);
    f.apply_stroke(&rec);
    assert!(f.is_identity());
    assert_eq!(apply_liquify(&img, &f), img);
}

#[test]
fn reconstruct_brush_reduces_the_field_and_freeze_protects() {
    let mut s = LiquifyStroke::new(LiquifyTool::ForwardWarp, 40.0);
    s.points = vec![vec![30.0, 32.0], vec![45.0, 32.0]];
    let mut f = LiquifyField::from_strokes(bounds(), 2.0, std::slice::from_ref(&s));
    let before = f.max_displacement();
    let mut r = LiquifyStroke::new(LiquifyTool::Reconstruct, 80.0);
    r.points = vec![vec![40.0, 32.0]; 30];
    f.apply_stroke(&r);
    assert!(
        f.max_displacement() < before * 0.2,
        "{} vs {before}",
        f.max_displacement()
    );
    // Frozen area doesn't move.
    let mut fr = LiquifyStroke::new(LiquifyTool::Freeze, 60.0);
    fr.density = 100.0;
    fr.points = vec![vec![40.0, 32.0]];
    let mut g = LiquifyField::new(bounds(), 2.0);
    g.apply_stroke(&fr);
    assert!(g.freeze_at(40.0, 32.0) > 0.99);
    g.apply_stroke(&s);
    assert!(g.sample(40.0, 32.0)[0].abs() < 1e-3);
    // Thaw unfreezes.
    let mut th = fr.clone();
    th.tool = LiquifyTool::Thaw;
    g.apply_stroke(&th);
    assert!(g.freeze_at(40.0, 32.0) < 0.01);
    // Mask All / Invert / None.
    g.apply_stroke(&LiquifyStroke::new(LiquifyTool::FreezeAll, 1.0));
    assert!(g.freeze.iter().all(|f| *f == 1.0));
    g.apply_stroke(&LiquifyStroke::new(LiquifyTool::InvertFreeze, 1.0));
    assert!(g.freeze.iter().all(|f| *f == 0.0));
    g.apply_stroke(&fr);
    g.apply_stroke(&LiquifyStroke::new(LiquifyTool::InvertFreeze, 1.0));
    assert!(g.freeze_at(40.0, 32.0) < 0.01 && g.freeze_at(5.0, 5.0) > 0.99);
    g.apply_stroke(&LiquifyStroke::new(LiquifyTool::ThawAll, 1.0));
    assert!(g.freeze.iter().all(|f| *f == 0.0));
}

#[test]
fn twirl_pucker_bloat_and_push_left_have_the_right_sense() {
    let at = |tool: LiquifyTool, pts: Vec<Vec<f64>>| {
        let mut s = LiquifyStroke::new(tool, 40.0);
        s.points = pts;
        LiquifyField::from_strokes(bounds(), 2.0, &[s])
    };
    let c = vec![vec![48.0, 32.0]; 10];
    // Pucker pulls content in: the output samples outward (d points away from the centre).
    let f = at(LiquifyTool::Pucker, c.clone());
    assert!(f.sample(58.0, 32.0)[0] > 0.1);
    let f = at(LiquifyTool::Bloat, c.clone());
    assert!(f.sample(58.0, 32.0)[0] < -0.1);
    // Twirl clockwise (y down): content right of centre moves down, so it samples from above.
    let f = at(LiquifyTool::TwirlCw, c.clone());
    assert!(f.sample(58.0, 32.0)[1] < -0.1, "{:?}", f.sample(58.0, 32.0));
    let f = at(LiquifyTool::TwirlCcw, c);
    assert!(f.sample(58.0, 32.0)[1] > 0.1);
    // Push Left while dragging up moves content left (samples from the right).
    let f = at(
        LiquifyTool::PushLeft,
        vec![vec![48.0, 44.0], vec![48.0, 20.0]],
    );
    assert!(f.sample(48.0, 32.0)[0] > 0.5, "{:?}", f.sample(48.0, 32.0));
    // Smooth reduces sharp field differences.
    let mut s = LiquifyStroke::new(LiquifyTool::ForwardWarp, 12.0);
    s.points = vec![vec![40.0, 32.0], vec![50.0, 32.0]];
    let mut g = LiquifyField::from_strokes(bounds(), 2.0, &[s]);
    let m0 = g.max_displacement();
    let mut sm = LiquifyStroke::new(LiquifyTool::Smooth, 60.0);
    sm.points = vec![vec![45.0, 32.0]; 20];
    g.apply_stroke(&sm);
    assert!(g.max_displacement() < m0);
}
