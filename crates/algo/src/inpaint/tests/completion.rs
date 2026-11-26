//! Tests for content-aware completion and the colour-adaptation fit.

use super::*;

/// The mean is matched first: on a nearly flat bright patch with a noisy deviation ratio the
/// gain stays where the bias can still reach the target's mean.
#[test]
fn fit_matches_the_mean_before_the_contrast() {
    let a = Adapt {
        gain: (0.9, 1.1),
        bias: 0.04,
    };
    // Target mean 0.70, source mean 0.70; deviations 0.01 vs 0.004 (ratio 2.5).
    let n = 25.0;
    let (mt, ms, sdt, sds) = (0.70f32, 0.70f32, 0.01f32, 0.004f32);
    let (g, b) = fit(
        a,
        n,
        mt * n,
        (sdt * sdt + mt * mt) * n,
        ms * n,
        (sds * sds + ms * ms) * n,
    );
    assert!(
        (g * ms + b - mt).abs() < 1e-5,
        "mean {} vs {mt} (g {g}, b {b})",
        g * ms + b
    );
    // Where the mean can't be reached anyway, the limits hold as before.
    let (g, b) = fit(a, n, 0.2 * n, 0.04 * n, 0.9 * n, 0.81 * n);
    assert!((a.gain.0..=a.gain.1).contains(&g) && b.abs() <= a.bias + 1e-6);
}

#[test]
fn completion_fills_blemish_from_texture() {
    let (w, h, ch) = (96, 96, 3);
    let clean = texture(w, h);
    let hole = disc(w, h, 48.0, 48.0, 9.0);
    let mut dirty = clean.clone();
    for (i, hh) in hole.iter().enumerate() {
        if *hh {
            dirty[i * ch..i * ch + ch].copy_from_slice(&[0.02, 0.02, 0.02]);
        }
    }
    let out = complete(w, h, ch, &dirty, &hole, &CompleteParams::default()).unwrap();
    let before = hole_rmse(&dirty, &clean, &hole, ch);
    let after = hole_rmse(&out, &clean, &hole, ch);
    assert!(
        after < 0.06 && after < before * 0.15,
        "rmse before {before} after {after}"
    );
    // Outside the hole untouched.
    assert!(out
        .iter()
        .zip(&dirty)
        .zip(hole.iter().flat_map(|h| [*h; 3]))
        .all(|((a, b), h)| h || a == b));
    // Deterministic.
    assert_eq!(
        out,
        complete(w, h, ch, &dirty, &hole, &CompleteParams::default()).unwrap()
    );
}

#[test]
fn completion_continues_structure() {
    // Vertical stripes: the filled hole should keep the stripes (not a blur).
    let (w, h, ch) = (80, 80, 1);
    let clean: Vec<f32> = (0..w * h)
        .map(|i| if (i % w / 4) % 2 == 0 { 0.2 } else { 0.8 })
        .collect();
    let hole = disc(w, h, 40.0, 40.0, 7.0);
    let dirty: Vec<f32> = clean
        .iter()
        .zip(&hole)
        .map(|(c, h)| if *h { 0.5 } else { *c })
        .collect();
    let out = complete(w, h, ch, &dirty, &hole, &CompleteParams::default()).unwrap();
    let err = hole_rmse(&out, &clean, &hole, ch);
    assert!(err < 0.15, "{err}");
}

#[test]
fn owned_completion_fills_in_place_or_gives_the_pixels_back() {
    let (w, h, ch) = (64, 52, 3);
    let img = busy(w, h, ch, 6);
    let hole = disc(w, h, 30.0, 26.0, 8.0);
    let px = roles(w, h, &hole);
    let (p, none) = (
        CompleteParams::default(),
        rust_motion_raster::Interrupt::NONE,
    );
    let borrowed = complete_masked(w, h, ch, &img, &px, &p, &none)
        .unwrap()
        .unwrap();
    let owned = complete_masked_owned(w, h, ch, img.clone(), &px, &p, &none)
        .unwrap()
        .unwrap();
    assert_eq!(bits(&owned), bits(&borrowed));
    // Nothing may be copied: no completion, and the pixels come back as they were.
    let known: Vec<Px> = px
        .iter()
        .map(|k| if *k == Px::Source { Px::Known } else { *k })
        .collect();
    assert!(complete_masked(w, h, ch, &img, &known, &p, &none)
        .unwrap()
        .is_none());
    let back = complete_masked_owned(w, h, ch, img.clone(), &known, &p, &none)
        .unwrap()
        .unwrap_err();
    assert_eq!(bits(&back), bits(&img));
    // Cancelled, it stops.
    let yes = || true;
    assert!(complete_masked_owned(
        w,
        h,
        ch,
        img,
        &px,
        &p,
        &rust_motion_raster::Interrupt::cancel_only(&yes)
    )
    .is_err());
}
