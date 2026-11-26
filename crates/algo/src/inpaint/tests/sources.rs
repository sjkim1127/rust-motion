//! Tests for source-patch validity and clipped patch distances.

use super::*;

#[test]
fn valid_sources_are_the_wholly_copyable_patches() {
    let (w, h, r) = (37, 29, 2);
    let mut rng = Rng::new(9);
    let px: Vec<Px> = (0..w * h)
        .map(|_| match rng.next() % 23 {
            0 => Px::Hole,
            1 => Px::Void,
            2 => Px::Known,
            _ => Px::Source,
        })
        .collect();
    let valid = valid_sources(w, h, r, &px, &rust_motion_raster::Interrupt::NONE).unwrap();
    let source: Vec<bool> = px.iter().map(|k| *k == Px::Source).collect();
    let ss = integral(w, h, &source);
    let (ri, full) = (r as i32, ((2 * r + 1) * (2 * r + 1)) as u32);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let inside = x >= ri && y >= ri && x + ri < w as i32 && y + ri < h as i32;
            let want =
                inside && window_count(&ss, w, h, x - ri, y - ri, x + ri + 1, y + ri + 1) == full;
            assert_eq!(valid[y as usize * w + x as usize], want, "({x}, {y})");
        }
    }
    // Counting per row and block finds the centres in order.
    let sources = Sources::new(&valid, w);
    let list = sources.all(&valid, w);
    assert!(list.len() > 10 && list.len() == sources.len());
    for (k, p) in list.iter().enumerate() {
        assert_eq!(sources.nth(k, &valid, w), *p);
    }
    // And on a grid wider than a block, with empty blocks and rows.
    let (w, h) = (300, 9);
    let wide: Vec<bool> = (0..w * h)
        .map(|i| (i % w) % 7 == 3 && (i % w) / 64 != 2 && i / w != 4)
        .collect();
    let sources = Sources::new(&wide, w);
    let list = sources.all(&wide, w);
    for (k, p) in list.iter().enumerate() {
        assert_eq!(sources.nth(k, &wide, w), *p);
    }
}

#[test]
fn full_patches_measure_as_clipped_ones() {
    let (w, h, ch, r) = (46, 38, 4, 3);
    let mut img = busy(w, h, ch, 4);
    let hole = disc(w, h, 20.0, 18.0, 6.0);
    let px = roles(w, h, &hole);
    let none = rust_motion_raster::Interrupt::NONE;
    let hb = hole_bounds(w, h, &px).unwrap();
    let tb = hb.grow(r, w, h);
    let valid = valid_sources(w, h, r, &px, &none).unwrap();
    let picks: Vec<(i32, i32)> = Sources::new(&valid, w)
        .all(&valid, w)
        .iter()
        .step_by(37)
        .copied()
        .collect();
    assert!(picks.len() > 5);
    for adapt in [
        Adapt::NONE,
        Adapt {
            gain: (0.8, 1.25),
            bias: 0.08,
        },
    ] {
        for keep in [false, true] {
            let targets = targets_around(w, r, &px, hb, tb);
            let mut s = Solver {
                w,
                h,
                ch,
                r: r as i32,
                img: &mut img,
                px: &px,
                valid: valid.clone(),
                targets,
                hole_box: hb,
                tbox: tb,
                full: Vec::new(),
                tmom: Vec::new(),
                smom: Vec::new(),
                adapt,
                ctl: none,
            };
            s.prepare(keep);
            let nnf = vec![picks[0]; tb.area()];
            let mut cost = vec![f32::INFINITY; tb.area()];
            s.costs(&nnf, &mut cost).unwrap();
            let mut checked = 0;
            for k in 0..tb.area() {
                if !s.targets[k] || !s.full[k] {
                    continue;
                }
                let t = (
                    (tb.x0 + k % tb.width()) as i32,
                    (tb.y0 + k / tb.width()) as i32,
                );
                for &src in &picks {
                    if adapt.is_none() {
                        let exact = s.dist_clipped(t, src, f32::INFINITY);
                        assert_eq!(
                            s.dist_full(t, src, f32::INFINITY).to_bits(),
                            exact.to_bits()
                        );
                        for cutoff in [exact * 0.5, exact, exact * 2.0] {
                            let (a, b) =
                                (s.dist_full(t, src, cutoff), s.dist_clipped(t, src, cutoff));
                            assert_eq!(a < cutoff, b < cutoff);
                        }
                    } else {
                        let (mut m1, mut m2) = ([[0.0f32; 5]; 4], [[0.0f32; 5]; 4]);
                        s.moments_full(k, t, src, &mut m1);
                        assert_eq!(s.moments(t, src, &mut m2), (2 * r + 1) * (2 * r + 1));
                        assert_eq!(
                            m1.map(|c| c.map(f32::to_bits)),
                            m2.map(|c| c.map(f32::to_bits))
                        );
                        assert_eq!(
                            s.dist_adapted_full(k, t, src).to_bits(),
                            s.dist_adapted(t, src).to_bits()
                        );
                    }
                    checked += 1;
                }
            }
            assert!(checked > 100, "{checked}");
        }
    }
}
