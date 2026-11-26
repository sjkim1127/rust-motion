//! Magic wand and region tests against reference implementations.

use super::*;

#[test]
fn wand_contiguous_vs_global() {
    // Two separate red squares on white.
    let px = img(20, 10, |x, _| {
        if (2..6).contains(&x) || (12..16).contains(&x) {
            [1.0, 0.0, 0.0, 1.0]
        } else {
            [1.0; 4]
        }
    });
    let a = Rect::new(0, 0, 20, 10);
    let c = magic_wand(&px, a, (3, 3), 10.0, true, false);
    assert_eq!(c.iter().filter(|v| **v > 0.0).count(), 40);
    let g = magic_wand(&px, a, (3, 3), 10.0, false, false);
    assert_eq!(g.iter().filter(|v| **v > 0.0).count(), 80);
    let aa = magic_wand(&px, a, (3, 3), 10.0, true, true);
    assert!(aa[3 * 20 + 6] > 0.0 && aa[3 * 20 + 6] < 1.0);
    assert!(magic_wand(&px, a, (50, 3), 10.0, true, false)
        .iter()
        .all(|v| *v == 0.0));
}

#[test]
fn wand_tolerance() {
    let px = img(10, 1, |x, _| [x as f32 * 10.0 / 255.0, 0.0, 0.0, 1.0]);
    let m = magic_wand(&px, Rect::new(0, 0, 10, 1), (0, 0), 25.0, true, false);
    assert_eq!(m.iter().filter(|v| **v > 0.0).count(), 3);
}

#[test]
fn wand_ignores_hidden_rgb_only_when_both_pixels_are_fully_transparent() {
    let area = Rect::new(0, 0, 6, 1);
    let pixels = [
        [255, 0, 0, 0],
        [0, 255, 0, 0],
        [255, 0, 0, 128],
        [253, 0, 0, 128],
        [0, 0, 255, 0],
        [255, 0, 0, 255],
    ];
    let floats: Vec<_> = pixels
        .iter()
        .map(|p| p.map(|v| f32::from(v) / 255.0))
        .collect();
    for (seed, tolerance, contiguous, expected) in [
        ((0, 0), 0.0, true, [1.0, 1.0, 0.0, 0.0, 0.0, 0.0]),
        ((0, 0), 0.0, false, [1.0, 1.0, 0.0, 0.0, 1.0, 0.0]),
        ((4, 0), 32.0, false, [1.0, 1.0, 0.0, 0.0, 1.0, 0.0]),
        ((2, 0), 0.0, false, [0.0, 0.0, 1.0, 0.0, 0.0, 0.0]),
        ((2, 0), 2.0, true, [0.0, 0.0, 1.0, 1.0, 0.0, 0.0]),
        ((5, 0), 32.0, false, [0.0, 0.0, 0.0, 0.0, 0.0, 1.0]),
    ] {
        assert_eq!(
            magic_wand(&floats, area, seed, tolerance, contiguous, false),
            expected
        );
        let region = wand_region(&pixels, area, seed, tolerance, contiguous, false).unwrap();
        for (x, value) in expected.into_iter().enumerate() {
            assert_eq!(
                region.at(x as i32, 0),
                value,
                "seed={seed:?}, tolerance={tolerance}, x={x}"
            );
        }
    }
    // Hidden RGB cannot introduce extra edges into the anti-aliased selection either.
    let plain: Vec<_> = pixels
        .iter()
        .map(|p| if p[3] == 0 { [0; 4] } else { *p })
        .collect();
    for contiguous in [false, true] {
        assert_eq!(
            wand_region(&pixels, area, (0, 0), 0.0, contiguous, true),
            wand_region(&plain, area, (0, 0), 0.0, contiguous, true)
        );
    }
}

fn blobs(w: i32, h: i32) -> Vec<[u8; 4]> {
    // Deterministic blotchy image with gradients and hard edges.
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let v = ((x * 7 + y * 3) % 23 + if (x / 5 + y / 4) % 3 == 0 { 120 } else { 0 }) as u8;
            [v, v / 2, 255 - v, if x % 11 == 0 { 128 } else { 255 }]
        })
        .collect()
}

#[test]
fn magic_wand_rejects_incomplete_and_overflowing_pixel_inputs() {
    let area = Rect::new(0, 0, 4, 4);
    let short = vec![[255u8, 0, 0, 255]; 4];
    let floats: Vec<[f32; 4]> = short
        .iter()
        .map(|p| p.map(|v| f32::from(v) / 255.0))
        .collect();
    assert_eq!(
        magic_wand(&floats, area, (1, 1), 0.0, true, false),
        Vec::<f32>::new()
    );
    assert_eq!(wand_region(&short, area, (1, 1), 0.0, true, false), None);

    let huge = Rect::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX);
    assert!(magic_wand(&floats, huge, (0, 0), 0.0, false, false).is_empty());
    assert_eq!(wand_region(&short, huge, (0, 0), 0.0, false, false), None);

    let valid = Rect::new(0, 0, 2, 2);
    assert_eq!(
        magic_wand(&floats, valid, (0, 0), 0.0, false, false),
        vec![1.0; 4]
    );
    assert!(wand_region(&short, valid, (0, 0), 0.0, false, false).is_some());
}

#[test]
fn region_coverage_outside_a_short_mask_is_zero() {
    let region = Region {
        bbox: Rect::new(-2, -3, 1, -1),
        mask: vec![255, 128],
    };
    assert_eq!(region.at(-2, -3), 1.0);
    assert!((region.at(-1, -3) - 128.0 / 255.0).abs() < 1e-6);
    assert_eq!(region.at(0, -3), 0.0);
    assert_eq!(region.at(-2, -2), 0.0);
    assert_eq!(region.at(-3, -3), 0.0);

    let extreme = Region {
        bbox: Rect::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX),
        mask: vec![],
    };
    assert_eq!(extreme.at(i32::MAX - 1, i32::MAX - 1), 0.0);
    assert_eq!(extreme.at(i32::MIN, i32::MIN), 0.0);
}

#[test]
fn wand_region_matches_reference_magic_wand() {
    let (w, h) = (37, 29);
    let area = Rect::new(3, -2, 3 + w, -2 + h);
    let img = blobs(w, h);
    let f: Vec<[f32; 4]> = img.iter().map(|p| p.map(|v| v as f32 / 255.0)).collect();
    for (seed, tol, contiguous, aa) in [
        ((10, 5), 10.0, true, true),
        ((10, 5), 10.0, true, false),
        ((20, 20), 40.0, false, true),
        ((4, -2), 0.0, true, true),
        ((30, 10), 255.0, true, true),
    ] {
        let reference = magic_wand(&f, area, seed, tol, contiguous, aa);
        let r = wand_region(&img, area, seed, tol, contiguous, aa).unwrap();
        for y in area.y0..area.y1 {
            for x in area.x0..area.x1 {
                let want = (reference[((y - area.y0) * w + x - area.x0) as usize] * 255.0).round();
                assert!(
                    (r.at(x, y) * 255.0 - want).abs() <= 1.0,
                    "{seed:?} {tol} {contiguous} {aa} at {x},{y}: {} vs {want}",
                    r.at(x, y) * 255.0
                );
            }
        }
    }
    assert!(wand_region(&img, area, (0, 0), 10.0, true, true).is_none());
}

#[test]
fn combine_region_matches_combine() {
    let area = Rect::new(0, 0, 40, 30);
    let mut old = Surface::new(PixelFormat::GRAY8);
    old.fill_rect(Rect::new(5, 5, 25, 20), &[1.0]);
    old.fill_rect(Rect::new(8, 8, 10, 10), &[0.4]);
    let img = blobs(40, 30);
    let region = wand_region(&img, area, (12, 12), 30.0, true, true).unwrap();
    let full: Vec<f32> = (0..area.height())
        .flat_map(|y| (0..area.width()).map(move |x| (x, y)))
        .map(|(x, y)| region.at(x as i32, y as i32))
        .collect();
    for mode in [
        SelectionMode::Replace,
        SelectionMode::Add,
        SelectionMode::Subtract,
        SelectionMode::Intersect,
    ] {
        let a = combine(Some(&old), &full, area, mode);
        let b = combine_region(Some(&old), Some(&region), mode);
        assert_eq!(a.is_some(), b.is_some(), "{mode:?}");
        if let (Some(a), Some(b)) = (a, b) {
            for y in 0..30 {
                for x in 0..40 {
                    assert!(
                        (a.pixel(x, y)[0] - b.pixel(x, y)[0]).abs() < 1.5 / 255.0,
                        "{mode:?} {x},{y}"
                    );
                }
            }
        }
    }
    assert!(combine_region(None, None, SelectionMode::Subtract).is_none());
}
