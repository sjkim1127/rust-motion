//! Adjustments in the Lab, CMYK and CMY colour modes.

use super::*;

/// `adj` applied in a Lab document to one encoded sRGB colour, returned as (L, a, b).
fn in_lab(adj: &Adjustment, c: [f32; 3]) -> [f32; 3] {
    let mut b = Buffer::filled(Rect::new(0, 0, 1, 1), [c[0], c[1], c[2], 1.0]);
    apply_in(
        adj,
        &mut b,
        Transfer::Srgb,
        Some(SampleType::U8),
        ColorMode::Lab,
    );
    srgb_to_lab([b.px[0][0], b.px[0][1], b.px[0][2]])
}

/// A muted, in-gamut colour from (L, a, b).
fn lab(l: f32, a: f32, b: f32) -> [f32; 3] {
    lab_to_srgb([l, a, b])
}

#[test]
fn lab_invert_flips_l_a_and_b() {
    let got = in_lab(&Adjustment::Invert, lab(30.0, 12.0, -18.0));
    // n -> 1 - n: L -> 100 - L, a -> -a - 1, b -> -b - 1 (mirrored on the 0..255 grid).
    assert!(near(got, [70.0, -13.0, 17.0], 0.6), "{got:?}");
    // On the screen colour it would not be a mirror in Lab.
    let mut rgb = Buffer::filled(Rect::new(0, 0, 1, 1), [0.0, 0.0, 0.0, 1.0]);
    let c = lab(30.0, 12.0, -18.0);
    rgb.px[0] = [c[0], c[1], c[2], 1.0];
    apply(&Adjustment::Invert, &mut rgb);
    assert!(!near(
        srgb_to_lab([rgb.px[0][0], rgb.px[0][1], rgb.px[0][2]]),
        got,
        1.0
    ));
}

#[test]
fn sixteen_bit_lab_invert_keeps_greys_neutral() {
    // 16-bit Lab Invert mirrors a and b around neutral (photoshop corpus lab16 invert.psd);
    // 8-bit bytes land one step off it.
    let mut b = Buffer::filled(Rect::new(0, 0, 1, 1), [0.0, 0.0, 0.0, 1.0]);
    let c = lab(40.0, 0.0, 0.0);
    b.px[0] = [c[0], c[1], c[2], 1.0];
    apply_in(
        &Adjustment::Invert,
        &mut b,
        Transfer::Srgb,
        Some(SampleType::U16),
        ColorMode::Lab,
    );
    let got = srgb_to_lab([b.px[0][0], b.px[0][1], b.px[0][2]]);
    assert!(near(got, [60.0, 0.0, 0.0], 0.05), "{got:?}");
    assert!(near(
        in_lab(&Adjustment::Invert, c),
        [60.0, -1.0, -1.0],
        0.1
    ));
}

/// `adj` applied in a CMYK document to one encoded sRGB colour.
fn in_cmyk(adj: &Adjustment, c: [f32; 3]) -> [f32; 3] {
    let mut b = Buffer::filled(Rect::new(0, 0, 1, 1), [c[0], c[1], c[2], 1.0]);
    apply_in(
        adj,
        &mut b,
        Transfer::Srgb,
        Some(SampleType::U8),
        ColorMode::Cmyk,
    );
    [b.px[0][0], b.px[0][1], b.px[0][2]]
}

#[test]
fn cmyk_invert_posterize_and_threshold_work_on_the_inks() {
    use crate::convert::{cmyk_to_rgb, rgb_to_cmyk};
    // Paper inverts to all four inks at 100%, not to RGB black (photoshop corpus cmyk8 invert.psd).
    assert!(near(
        in_cmyk(&Adjustment::Invert, [1.0; 3]),
        cmyk_to_rgb([1.0; 4]),
        2e-3
    ));
    let red = [0.9, 0.2, 0.1];
    assert!(near(
        in_cmyk(&Adjustment::Invert, red),
        cmyk_to_rgb(rgb_to_cmyk(red).map(|v| 1.0 - v)),
        2e-3
    ));
    let two = cmyk_to_rgb(rgb_to_cmyk(red).map(|v| if v >= 0.5 { 1.0 } else { 0.0 }));
    assert!(near(
        in_cmyk(&Adjustment::Posterize { levels: 2 }, red),
        two,
        2e-3
    ));
    // Threshold's black is the black ink alone (threshold-128.psd), its white the paper.
    let t = Adjustment::Threshold { level: 0.5 };
    assert!(near(
        in_cmyk(&t, [0.2; 3]),
        cmyk_to_rgb([0.0, 0.0, 0.0, 1.0]),
        2e-3
    ));
    assert!(near(in_cmyk(&t, [0.8; 3]), cmyk_to_rgb([0.0; 4]), 2e-3));
}

/// CMYK posterizes the stored value 1 - ink: with 3 levels an ink of 85 (stored 170) lands on
/// the middle step, 128 (stored 127), where posterizing the ink would drop it to none (psd-tools
/// posterize_cmyk.psd, levels 3 to 21, exact so).
#[test]
fn cmyk_posterize_works_on_the_stored_values() {
    let mut ink = [85.0 / 255.0; 4];
    stored_channels(
        &Adjustment::Posterize { levels: 3 },
        ColorMode::Cmyk,
        Some(SampleType::U8),
        &mut ink,
    );
    assert!(
        ink.iter().all(|v| (v * 255.0 - 128.0).abs() < 1e-3),
        "{ink:?}"
    );
    assert_eq!(
        posterize(85.0 / 255.0, 3),
        0.0,
        "posterizing the ink itself"
    );
}

#[test]
fn cmyk_threshold_compares_the_inks_plain_luma() {
    assert_eq!(ink_luma([0.0; 4]), 1.0);
    assert_eq!(ink_luma([0.0, 0.0, 0.0, 1.0]), 0.0);
    // Two greys either side of level 128 in photoshop corpus cmyk8 threshold-128.psd: black,
    // then white (8-bit inks).
    let at = |i: [f32; 4]| {
        let mut ch = i.map(|v| v / 255.0);
        stored_channels(
            &Adjustment::Threshold {
                level: 128.0 / 255.0,
            },
            ColorMode::Cmyk,
            Some(SampleType::U8),
            &mut ch,
        );
        ch
    };
    assert_eq!(at([144.0, 122.0, 121.0, 35.0]), [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(at([131.0, 110.0, 110.0, 19.0]), [0.0; 4]);
}

#[test]
fn cmyk_brightness_contrast_curves_each_ink_from_paper() {
    use crate::convert::{cmyk_to_rgb, rgb_to_cmyk};
    // The tone curve runs on each ink's stored value, 1 - ink (photoshop corpus cmyk8
    // brightness-contrast.psd and -legacy.psd): raising brightness takes ink away, and paper
    // and solid ink stay put.
    let red = [0.9, 0.2, 0.1];
    for legacy in [false, true] {
        let adj = Adjustment::BrightnessContrast {
            brightness: 25.0,
            contrast: 30.0,
            legacy,
        };
        let table = ink_brightness_contrast_lut(25.0, 30.0, legacy);
        let want = cmyk_to_rgb(rgb_to_cmyk(red).map(|v| 1.0 - lut(&table, 1.0 - v)));
        assert!(near(in_cmyk(&adj, red), want, 2e-3), "legacy {legacy}");
        assert!(
            near(in_cmyk(&adj, [1.0; 3]), [1.0; 3], 2e-3),
            "legacy {legacy}: paper"
        );
        let solid = cmyk_to_rgb([1.0; 4]);
        assert!(near(
            in_cmyk(&adj, solid),
            cmyk_to_rgb(rgb_to_cmyk(solid).map(|v| ink_through(&table, v))),
            2e-3
        ));
    }
    // Legacy +25 / +30 on Photoshop's own 16-bit inks (photoshop corpus cmyk16
    // brightness-contrast-legacy.psd): contrast pivots at 128 in ink, 127 stored.
    let legacy = ink_brightness_contrast_lut(25.0, 30.0, true);
    for (ink, ps) in [
        (139.0, 108.0),
        (160.0, 137.99),
        (172.0, 155.13),
        (183.0, 170.85),
        (196.0, 189.42),
        (98.0, 49.42),
        (128.0, 92.28),
        (168.0, 149.42),
    ] {
        let got = ink_through(&legacy, ink / 255.0) * 255.0;
        assert!((got - ps).abs() < 0.1, "ink {ink}: {got} vs Photoshop {ps}");
    }
    let brighter = Adjustment::BrightnessContrast {
        brightness: 60.0,
        contrast: 0.0,
        legacy: false,
    };
    let ink = ink_brightness_contrast_lut(60.0, 0.0, false);
    assert!(
        ink_through(&ink, 0.5) < 0.5
            && ink_through(&ink, 0.0) == 0.0
            && (ink_through(&ink, 1.0) - 1.0).abs() < 1e-6
    );
    // On the screen colour it would land elsewhere.
    let mut rgb = Buffer::filled(Rect::new(0, 0, 1, 1), [red[0], red[1], red[2], 1.0]);
    apply(&brighter, &mut rgb);
    assert!(!near(
        in_cmyk(&brighter, red),
        [rgb.px[0][0], rgb.px[0][1], rgb.px[0][2]],
        0.01
    ));
}

#[test]
fn lab_posterize_steps_l_a_and_b() {
    let got = in_lab(&Adjustment::Posterize { levels: 2 }, lab(70.0, 20.0, -30.0));
    // Two levels: each normalised channel snaps to 0 or 1 (L 100, a and b at +127 / -128).
    assert!(
        near(got, srgb_to_lab(lab_denorm([1.0, 1.0, 0.0])), 1e-3),
        "{got:?}"
    );
    let grey = in_lab(&Adjustment::Posterize { levels: 255 }, lab(50.0, 0.0, 0.0));
    assert!(
        near(grey, [50.0, 0.0, 0.0], 1.1),
        "255 levels is within a step of the identity: {grey:?}"
    );
}

#[test]
fn lab_threshold_splits_on_l_with_a_neutral_result() {
    let t = Adjustment::Threshold { level: 0.5 };
    // A saturated blue (L ≈ 32) and yellow (L ≈ 97): black and white, nothing in between.
    for (c, white) in [
        ([0.0, 0.0, 1.0], false),
        ([1.0, 1.0, 0.0], true),
        (lab(52.0, 40.0, 30.0), true),
        (lab(48.0, -40.0, 30.0), false),
    ] {
        let mut b = Buffer::filled(Rect::new(0, 0, 1, 1), [c[0], c[1], c[2], 1.0]);
        apply_in(&t, &mut b, Transfer::Srgb, None, ColorMode::Lab);
        let v = if white { 1.0 } else { 0.0 };
        assert_eq!(b.px[0], [v, v, v, 1.0], "{c:?}");
    }
}

#[test]
fn lab_brightness_contrast_moves_l_only() {
    for legacy in [false, true] {
        let adj = Adjustment::BrightnessContrast {
            brightness: 40.0,
            contrast: 30.0,
            legacy,
        };
        let before = [45.0, 15.0, -20.0];
        let got = in_lab(&adj, lab(before[0], before[1], before[2]));
        assert!(got[0] > before[0] + 5.0, "{legacy}: L rises: {got:?}");
        assert!(
            (got[1] - before[1]).abs() < 0.5 && (got[2] - before[2]).abs() < 0.5,
            "{legacy}: a, b kept: {got:?}"
        );
    }
}

#[test]
fn lab_gradient_map_follows_l() {
    let stops = vec![(0.0, [1.0, 0.0, 0.0]), (1.0, [0.0, 0.0, 1.0])];
    for reverse in [false, true] {
        let map = Adjustment::GradientMap {
            stops: stops.clone(),
            reverse,
            dither: false,
        };
        // A grey and a saturated colour with the same L map to the same colour.
        let grey = in_lab(&map, lab(60.0, 0.0, 0.0));
        let green = in_lab(&map, lab(60.0, -50.0, 40.0));
        assert!(near(grey, green, 1e-3), "{reverse}: {grey:?} vs {green:?}");
        let t = if reverse { 0.4 } else { 0.6 };
        let want = srgb_to_lab([1.0 - t, 0.0, t]);
        assert!(near(grey, want, 0.05), "{reverse}: {grey:?} vs {want:?}");
    }
}

#[test]
fn other_modes_and_adjustments_keep_the_screen_colour_path() {
    let c = [0.2, 0.5, 0.7, 1.0];
    let lab_only = [
        Adjustment::Invert,
        Adjustment::Threshold { level: 0.47 },
        Adjustment::Posterize { levels: 4 },
    ];
    for adj in lab_only.iter().chain([&Adjustment::HueSaturation {
        hue: 30.0,
        saturation: 0.0,
        lightness: 0.0,
        colorize: false,
        ranges: HueRange::defaults(),
    }]) {
        for mode in [
            ColorMode::Rgb,
            ColorMode::Cmyk,
            ColorMode::Grayscale,
            ColorMode::Lab,
        ] {
            let (mut a, mut b) = (
                Buffer::filled(Rect::new(0, 0, 1, 1), c),
                Buffer::filled(Rect::new(0, 0, 1, 1), c),
            );
            apply_in(adj, &mut a, Transfer::Srgb, None, mode);
            apply_depth(adj, &mut b, Transfer::Srgb, None);
            let own = (mode == ColorMode::Lab && lab_channel_adjustment(adj))
                || (mode == ColorMode::Cmyk && cmyk_channel_adjustment(adj));
            assert_eq!(a.px == b.px, !own, "{adj:?} in {mode:?}");
        }
    }
    // Transparent pixels are left alone and odd values never panic.
    let mut t = Buffer::filled(Rect::new(0, 0, 2, 1), [f32::NAN, 2.0, -1.0, 0.0]);
    for adj in lab_only.iter().chain([&Adjustment::GradientMap {
        stops: vec![],
        reverse: true,
        dither: true,
    }]) {
        for mode in [ColorMode::Lab, ColorMode::Cmyk] {
            apply_in(adj, &mut t, Transfer::Srgb, None, mode);
            let mut odd =
                Buffer::filled(Rect::new(0, 0, 1, 1), [f32::NAN, f32::INFINITY, -3.0, 1.0]);
            apply_in(adj, &mut odd, Transfer::Srgb, Some(SampleType::U16), mode);
        }
    }
    assert!(t.px[0][1] == 2.0);
}
