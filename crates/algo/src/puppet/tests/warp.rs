//! Warping: identity cases, refusals, linked masks, rigidity, ARAP modes and depth order.

use super::*;

#[test]
fn no_pins_or_unmoved_pins_are_identity() {
    for st in [SampleType::U8, SampleType::U16, SampleType::F32] {
        let s = blob(st);
        let b = s.content_bounds();
        for pins in [
            vec![],
            vec![
                PuppetPin {
                    src: [28.0, 20.0],
                    dst: [28.0, 20.0],
                    rotate: None,
                    depth: 0,
                },
                PuppetPin {
                    src: [60.0, 62.0],
                    dst: [60.0, 62.0],
                    rotate: None,
                    depth: 0,
                },
            ],
        ] {
            let w = PuppetWarp {
                pins,
                mode: PuppetMode::Normal,
                density: PuppetDensity::Normal,
                expansion: 2.0,
            };
            let out = puppet_warp(&s, b, &w, Interp::Bicubic).unwrap();
            let worst = s
                .read_region(b)
                .iter()
                .zip(out.read_region(b))
                .map(|(a, c)| (a - c).abs())
                .fold(0.0f32, f32::max);
            assert!(worst <= 1.0 / 255.0 + 1e-6, "{st:?}: {worst}");
        }
    }
}

#[test]
fn an_expansion_that_empties_the_mesh_warps_nothing() {
    // #951: with pins and no vertices, `locate` fell back to vertex 0 and the solver indexed
    // its empty union-find.
    let solver = PuppetSolver::new(PuppetMesh::default(), &[[10.0, 10.0]], &[true]);
    assert!(solver.mesh.verts.is_empty());
    let s = blob(SampleType::U8);
    let b = s.content_bounds();
    let w = PuppetWarp {
        pins: vec![PuppetPin {
            src: [28.0, 30.0],
            dst: [38.0, 25.0],
            rotate: None,
            depth: 0,
        }],
        mode: PuppetMode::Normal,
        density: PuppetDensity::Normal,
        expansion: -200.0,
    };
    assert!(build_mesh(&s, b, w.density, w.expansion).tris.is_empty());
    // Nothing to warp: the pixels stay, rather than being dropped outside an empty mesh.
    let out = puppet_warp(&s, b, &w, Interp::Bilinear).unwrap();
    assert_eq!(out.read_region(b), s.read_region(b));
}

#[test]
fn a_pin_dragged_absurdly_far_is_refused_before_rendering() {
    // #1019: the renderer sized its tile bookkeeping by the deformed mesh, so one far pin
    // allocated hundreds of MB before drawing a pixel.
    let s = blob(SampleType::U8);
    let b = s.content_bounds();
    let pin = |dst: [f64; 2]| PuppetWarp {
        pins: vec![
            PuppetPin {
                src: [28.0, 20.0],
                dst: [28.0, 20.0],
                rotate: None,
                depth: 0,
            },
            PuppetPin {
                src: [60.0, 62.0],
                dst,
                rotate: None,
                depth: 0,
            },
        ],
        mode: PuppetMode::Normal,
        density: PuppetDensity::Normal,
        expansion: 2.0,
    };
    assert!(puppet_warp(&s, b, &pin([1048576.0, 1048576.0]), Interp::Bilinear).is_none());
    assert!(puppet_warp(&s, b, &pin([-1e7, 62.0]), Interp::Bilinear).is_none());
    let near = puppet_warp(&s, b, &pin([70.0, 66.0]), Interp::Bilinear).expect("a near pin warps");
    assert!(!near.content_bounds().is_empty());
}

#[test]
fn a_linked_mask_moves_with_the_mesh_and_leaves_its_default_behind() {
    // #4910. A hide-all mask (default 0) that reveals the bar's top (y 10..30): moved down by
    // 8, the revealed patch moves along, what it left is hidden again, and the mask away
    // from the puppet keeps its values.
    let s = blob(SampleType::U8);
    let b = s.content_bounds();
    for sample in [SampleType::U8, SampleType::U16, SampleType::F32] {
        let mut mask = Surface::with_default(
            PixelFormat::new(ColorMode::Grayscale, sample, false),
            &[0.0],
        );
        mask.fill_rect(Rect::new(20, 10, 36, 30), &[1.0]);
        mask.fill_rect(Rect::new(90, 90, 94, 94), &[0.5]);
        let w = PuppetWarp {
            pins: vec![PuppetPin {
                src: [28.0, 30.0],
                dst: [28.0, 38.0],
                rotate: None,
                depth: 0,
            }],
            mode: PuppetMode::Rigid,
            density: PuppetDensity::Normal,
            expansion: 2.0,
        };
        let (_, moved) =
            puppet_warp_with_mask(&s, b, &w, Interp::Bilinear, Some(&mask)).expect("warps");
        let m = moved.expect("a mask in, a mask out");
        let at = |x, y| m.sample_channel(x, y, 0);
        assert!(
            at(28, 20) > 0.99 && at(28, 36) > 0.99,
            "{sample:?}: the revealed patch moved down"
        );
        assert!(at(28, 15) < 0.01, "{sample:?}: its old top is hidden again");
        assert!(
            (at(91, 91) - 0.5).abs() <= 1.0 / 255.0,
            "{sample:?}: away from the mesh nothing changes"
        );
        assert!(puppet_warp_with_mask(&s, b, &w, Interp::Bilinear, None)
            .expect("warps")
            .1
            .is_none());
    }
}

#[test]
fn a_single_pin_translates_rigidly() {
    let s = blob(SampleType::U8);
    let b = s.content_bounds();
    let w = PuppetWarp {
        pins: vec![PuppetPin {
            src: [28.0, 30.0],
            dst: [38.0, 25.0],
            rotate: None,
            depth: 0,
        }],
        mode: PuppetMode::Rigid,
        density: PuppetDensity::Normal,
        expansion: 2.0,
    };
    let (solver, v, _) = deform(&s, b, &w, ITERATIONS);
    for (d, x) in v.iter().zip(&solver.mesh.verts) {
        assert!(
            (d[0] - x[0] - 10.0).abs() < 1e-3 && (d[1] - x[1] + 5.0).abs() < 1e-3,
            "{d:?} {x:?}"
        );
    }
    let out = puppet_warp(&s, b, &w, Interp::Bilinear).unwrap();
    assert_eq!(out.content_bounds(), b.translate(10, -5));
    assert_eq!(out.pixel(30, 20), s.pixel(20, 25));
    // A fixed rotation turns the whole mesh about the pin.
    let w = PuppetWarp {
        pins: vec![PuppetPin {
            src: [28.0, 30.0],
            dst: [28.0, 30.0],
            rotate: Some(90.0),
            depth: 0,
        }],
        ..w
    };
    let (solver, v, _) = deform(&s, b, &w, ITERATIONS);
    let i = (0..v.len())
        .max_by(|&a, &c| solver.mesh.verts[a][0].total_cmp(&solver.mesh.verts[c][0]))
        .unwrap();
    let (x, d) = (solver.mesh.verts[i], v[i]);
    // (dx, dy) → (−dy, dx) for +90° with y down.
    let (dx, dy) = (x[0] - 28.0, x[1] - 30.0);
    assert!(
        (d[0] - (28.0 - dy)).abs() < 1.0 && (d[1] - (30.0 + dx)).abs() < 1.0,
        "{x:?} → {d:?}"
    );
}

#[test]
fn arap_energy_decreases_and_modes_differ() {
    let s = blob(SampleType::U8);
    let b = s.content_bounds();
    let mesh = build_mesh(&s, b, PuppetDensity::Normal, 2.0);
    let src = [[28.0, 14.0], [64.0, 62.0], [28.0, 62.0]];
    let dst = [[10.0, 30.0], [70.0, 50.0], [28.0, 62.0]];
    let solver = PuppetSolver::new(mesh, &src, &[false; 3]);
    let rot = [None; 3];
    let (v, e) = solver.solve(&dst, &rot, PuppetMode::Rigid, 30, None);
    assert!(e.len() == 30);
    let start = solver.energy(
        &solver.solve(&dst, &rot, PuppetMode::Rigid, 0, None).0,
        &dst,
        &rot,
    );
    assert!(e[0] <= start * (1.0 + 1e-9), "{} vs {start}", e[0]);
    for w in e.windows(2) {
        assert!(w[1] <= w[0] * (1.0 + 1e-6) + 1e-9, "{w:?}");
    }
    assert!(e[29] < start);
    // Pins are honoured closely.
    for (b, d) in solver.bound.iter().zip(&dst) {
        let p = (0..3).fold([0.0, 0.0], |a, k| {
            [
                a[0] + b.bary[k] * v[b.verts[k]][0],
                a[1] + b.bary[k] * v[b.verts[k]][1],
            ]
        });
        assert!(dist2(p, *d) < 0.25, "{p:?} vs {d:?}");
    }
    let (vd, _) = solver.solve(&dst, &rot, PuppetMode::Distort, 30, None);
    assert_ne!(v, vd);
}

#[test]
fn depth_orders_triangles() {
    let s = blob(SampleType::U8);
    let b = s.content_bounds();
    let mesh = build_mesh(&s, b, PuppetDensity::Fewer, 2.0);
    let pins = [[28.0, 14.0], [64.0, 62.0]];
    let solver = PuppetSolver::new(mesh, &pins, &[false; 2]);
    let order = solver.draw_order(&[1, 0], &pins);
    let near0 = |t: usize| {
        let c = solver.mesh.tris[t].iter().fold([0.0, 0.0], |a, &i| {
            [
                a[0] + solver.mesh.verts[i][0] / 3.0,
                a[1] + solver.mesh.verts[i][1] / 3.0,
            ]
        });
        dist2(c, pins[0]) < dist2(c, pins[1])
    };
    // Triangles nearest the raised pin come last, all others first.
    let k = order.iter().position(|&t| near0(t)).unwrap();
    assert!(order[..k].iter().all(|&t| !near0(t)) && order[k..].iter().all(|&t| near0(t)));
}
