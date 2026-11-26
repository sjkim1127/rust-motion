//! Tests for the onion peel against its plain-scan reference.

use super::*;

/// The onion peel as first written: every source in order, every known pixel summed.
fn onion_peel_plain(
    w: usize,
    h: usize,
    ch: usize,
    r: i32,
    img: &mut [f32],
    px: &[Px],
    sources: &[(i32, i32)],
) {
    let mut known: Vec<bool> = px
        .iter()
        .map(|k| matches!(k, Px::Known | Px::Source))
        .collect();
    loop {
        let front: Vec<usize> = (0..w * h)
            .filter(|&i| {
                let (x, y) = ((i % w) as i32, (i / w) as i32);
                !known[i]
                    && px[i] == Px::Hole
                    && (-1..=1).any(|dy: i32| {
                        (-1..=1).any(|dx: i32| {
                            let (nx, ny) = (x + dx, y + dy);
                            nx >= 0
                                && ny >= 0
                                && (nx as usize) < w
                                && (ny as usize) < h
                                && known[ny as usize * w + nx as usize]
                        })
                    })
            })
            .collect();
        if front.is_empty() {
            break;
        }
        let vals: Vec<Vec<f32>> = front
            .iter()
            .map(|&i| {
                let (x, y) = ((i % w) as i32, (i / w) as i32);
                let mut best = (f32::INFINITY, (0i32, 0i32));
                for &(sx, sy) in sources {
                    let (mut e, mut n) = (0.0f32, 0usize);
                    for dy in -r..=r {
                        for dx in -r..=r {
                            let (tx, ty) = (x + dx, y + dy);
                            if tx < 0
                                || ty < 0
                                || tx as usize >= w
                                || ty as usize >= h
                                || !known[ty as usize * w + tx as usize]
                            {
                                continue;
                            }
                            let (ti, si) = (
                                (ty as usize * w + tx as usize) * ch,
                                ((sy + dy) as usize * w + (sx + dx) as usize) * ch,
                            );
                            for c in 0..ch {
                                let d = img[ti + c] - img[si + c];
                                e += d * d;
                            }
                            n += 1;
                        }
                    }
                    if n > 0 && e / (n as f32) < best.0 {
                        best = (e / n as f32, (sx, sy));
                    }
                }
                let si = (best.1 .1 as usize * w + best.1 .0 as usize) * ch;
                img[si..si + ch].to_vec()
            })
            .collect();
        for (&i, v) in front.iter().zip(vals) {
            img[i * ch..(i + 1) * ch].copy_from_slice(&v);
            known[i] = true;
        }
    }
}

/// #2805's case (upstream's own speed-up of the onion peel, superseded here): repeating
/// texture with many exact ties, two holes; the result must be the plain scan's to the bit.
#[test]
fn onion_peel_picks_exactly_what_the_plain_scan_picks() {
    let (w, h, ch, r) = (48, 40, 3, 3);
    let mut img = texture(w, h);
    for y in 0..h {
        for x in 6..w / 2 {
            for c in 0..ch {
                img[(y * w + x) * ch + c] = img[(y * w + x % 6) * ch + c];
            }
        }
    }
    let none = rust_motion_raster::Interrupt::NONE;
    for hole in [disc(w, h, 20.0, 18.0, 7.5), disc(w, h, 34.0, 22.0, 5.0)] {
        let px: Vec<Px> = hole
            .iter()
            .map(|h| if *h { Px::Hole } else { Px::Source })
            .collect();
        let near = integral(w, h, &hole);
        let sources: Vec<(i32, i32)> = (r..h as i32 - r)
            .flat_map(|y| (r..w as i32 - r).map(move |x| (x, y)))
            .filter(|&(x, y)| window_count(&near, w, h, x - r, y - r, x + r + 1, y + r + 1) == 0)
            .collect();
        let (mut fast, mut plain) = (img.clone(), img.clone());
        onion_peel(
            w,
            h,
            ch,
            r,
            &mut fast,
            &px,
            hole_bounds(w, h, &px).unwrap(),
            &sources,
            &none,
        )
        .unwrap();
        onion_peel_plain(w, h, ch, r, &mut plain, &px, &sources);
        assert!(fast
            .iter()
            .zip(&plain)
            .all(|(a, b)| a.to_bits() == b.to_bits()));
        assert_ne!(fast, img);
    }
}

#[test]
fn onion_peel_takes_the_plain_scans_sources() {
    let none = rust_motion_raster::Interrupt::NONE;
    for (w, h, ch, seed) in [(44, 36, 1, 1), (50, 40, 3, 2), (40, 40, 4, 3)] {
        let mut img = busy(w, h, ch, seed);
        // Also an image of a few exact values, where many sources tie.
        if seed == 3 {
            img.iter_mut().for_each(|v| *v = (*v * 3.0).round() / 3.0);
        }
        let hole = disc(w, h, w as f32 * 0.45, h as f32 * 0.5, 7.5);
        let px = roles(w, h, &hole);
        let valid = valid_sources(w, h, 3, &px, &none).unwrap();
        let sources = Sources::new(&valid, w).all(&valid, w);
        let (mut a, mut b) = (img.clone(), img.clone());
        onion_peel(
            w,
            h,
            ch,
            3,
            &mut a,
            &px,
            hole_bounds(w, h, &px).unwrap(),
            &sources,
            &none,
        )
        .unwrap();
        onion_peel_plain(w, h, ch, 3, &mut b, &px, &sources);
        assert_eq!(bits(&a), bits(&b), "{w}×{h}×{ch}");
    }
}

#[test]
fn least_sum_is_where_the_quotient_crosses() {
    let mut rng = Rng::new(5);
    for i in 0..4000 {
        let n = (1 + rng.next() % 49) as f32;
        let v = match i % 4 {
            0 => 0.0,
            1 => (rng.next() % 1000) as f32 * 1e-9,
            _ => (rng.next() % 1_000_000) as f32 / 1_000_000.0,
        };
        for strict in [true, false] {
            let holds = |e: f32| if strict { e / n > v } else { e / n >= v };
            let e = least_sum(v, n, strict);
            assert!(holds(e), "{v} {n} {strict}: {e}");
            assert!(
                e == 0.0 || !holds(e.next_down()),
                "{v} {n} {strict}: {e} is not the least"
            );
        }
    }
    assert_eq!(least_sum(f32::INFINITY, 4.0, true), f32::INFINITY);
}
