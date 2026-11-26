//! Tone, colour and gradient adjustments against Photoshop's measured results.

use super::*;

fn ramp() -> Buffer {
    let rect = Rect::new(0, 0, 8, 8);
    let px = (0..64)
        .map(|i| [((i % 8) as f32) / 7.0, ((i / 8) as f32) / 7.0, 0.3, 1.0])
        .collect();
    Buffer { rect, px }
}

/// Photoshop 27.11, measured: Brightness/Contrast with Use Legacy on, over a gray ramp
/// (x = 0, 16, …, 240, 255), one row per (brightness, contrast).
/// One pixel through `adj` in a 32-bit document.
fn hdr(adj: &Adjustment, c: [f32; 3]) -> [f32; 3] {
    let mut b = Buffer::filled(Rect::new(0, 0, 1, 1), [c[0], c[1], c[2], 1.0]);
    apply_depth(adj, &mut b, Transfer::Gamma(1.0), Some(SampleType::F32));
    [b.px[0][0], b.px[0][1], b.px[0][2]]
}

/// Photoshop's own 32-bit results (photoshop corpus rgb32): nothing clips, legacy contrast
/// pivots at 127, the mixer cuts its percentages to 1/1024 and counts 255/512 of the
/// constant, and the Black & White tint keeps values above 1.
#[test]
fn thirty_two_bit_adjustments_match_photoshop_unclipped() {
    let near = |got: [f32; 3], want: [f32; 3], tol: f32| {
        got.iter().zip(want).all(|(g, w)| (g - w).abs() <= tol)
    };
    let bc = Adjustment::BrightnessContrast {
        brightness: 25.0,
        contrast: 30.0,
        legacy: true,
    };
    // brightness-contrast-legacy.psd
    assert!(
        near(
            hdr(&bc, [0.4910, 0.0009, 0.9301]),
            [0.6280, -0.0721, 1.2553],
            2e-4
        ),
        "{:?}",
        hdr(&bc, [0.4910, 0.0009, 0.9301])
    );
    // channel-mixer.psd: pure red, green, blue and black, exactly.
    let mixer = Adjustment::ChannelMixer {
        matrix: [
            [0.8, 0.3, -0.1, 0.0],
            [0.1, 0.9, 0.0, 0.05],
            [0.0, 0.2, 0.7, 0.0],
        ],
        monochrome: false,
    };
    for (c, want) in [
        ([1.0, 0.0, 0.0], [0.7998047, 0.1245117, 0.0]),
        ([0.0, 1.0, 0.0], [0.2998047, 0.9243164, 0.1992188]),
        ([0.0, 0.0, 1.0], [-0.0996094, 0.0249023, 0.6992188]),
        ([0.0, 0.0, 0.0], [0.0, 0.0249023, 0.0]),
    ] {
        assert!(
            near(hdr(&mixer, c), want, 1e-6),
            "{c:?}: {:?}",
            hdr(&mixer, c)
        );
    }
    // black-white-tint.psd: white tints past 1.
    let bw = Adjustment::BlackWhite {
        weights: [40.0, 60.0, 40.0, 60.0, 20.0, 80.0],
        tint: Some([0.88235295, 0.827451, 0.7019608]),
    };
    assert!(
        near(hdr(&bw, [1.0; 3]), [1.0522, 0.9973, 0.8718], 2e-4),
        "{:?}",
        hdr(&bw, [1.0; 3])
    );
    // 8/16-bit documents still clip.
    let mut b = Buffer::filled(Rect::new(0, 0, 1, 1), [1.0, 1.0, 1.0, 1.0]);
    apply_depth(&bw, &mut b, Transfer::Srgb, None);
    assert!(b.px[0].iter().all(|v| *v <= 1.0));
}

#[test]
fn legacy_brightness_contrast_matches_photoshop() {
    const X: [u8; 17] = [
        0, 16, 32, 48, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 224, 240, 255,
    ];
    let measured: [(f32, f32, [u8; 17]); 7] = [
        (
            40.0,
            0.0,
            [
                40, 56, 72, 88, 104, 120, 136, 152, 168, 184, 200, 216, 232, 248, 255, 255, 255,
            ],
        ),
        (
            -40.0,
            0.0,
            [
                0, 0, 0, 8, 24, 40, 56, 72, 88, 104, 120, 136, 152, 168, 184, 200, 215,
            ],
        ),
        (
            0.0,
            30.0,
            [
                0, 0, 0, 14, 37, 60, 82, 105, 128, 151, 174, 196, 219, 242, 255, 255, 255,
            ],
        ),
        (
            0.0,
            -30.0,
            [
                38, 49, 61, 72, 83, 94, 106, 117, 128, 139, 150, 162, 173, 184, 195, 207, 217,
            ],
        ),
        (
            40.0,
            30.0,
            [
                3, 25, 48, 71, 94, 117, 139, 162, 185, 208, 231, 253, 255, 255, 255, 255, 255,
            ],
        ),
        (
            100.0,
            0.0,
            [
                100, 116, 132, 148, 164, 180, 196, 212, 228, 244, 255, 255, 255, 255, 255, 255, 255,
            ],
        ),
        (
            0.0,
            80.0,
            [
                0, 0, 0, 0, 0, 0, 0, 48, 128, 208, 255, 255, 255, 255, 255, 255, 255,
            ],
        ),
    ];
    for (brightness, contrast, want) in measured {
        let rect = Rect::new(0, 0, 17, 1);
        let px = X
            .iter()
            .map(|x| [f32::from(*x) / 255.0; 4])
            .map(|p| [p[0], p[1], p[2], 1.0])
            .collect();
        let mut b = Buffer { rect, px };
        apply(
            &Adjustment::BrightnessContrast {
                brightness,
                contrast,
                legacy: true,
            },
            &mut b,
        );
        for (i, (p, w)) in b.px.iter().zip(want).enumerate() {
            let got = (p[0] * 255.0).round() as i32;
            assert!(
                (got - i32::from(w)).abs() <= 1,
                "b {brightness} c {contrast} at {}: {got} vs Photoshop {w}",
                X[i]
            );
        }
    }
}

/// Pixel → Photoshop's split. 8-bit from psd-tools threshold_rgb.psd: the first three are
/// white though their Rec. 601 grey rounds below 60, and (131, 109, 99) sits exactly on 114.5
/// at level 115 (halves round down). 16-bit from threshold_16bits_rgb.psd and the photoshop
/// corpus's rgb16 threshold-128.psd, whose mid grey 16448 is level 128 in 16 bits.
#[test]
fn threshold_splits_on_the_luminosity_like_photoshop() {
    let c8 = |r: u8, g: u8, b: u8| [r, g, b].map(|v| f32::from(v) / 255.0);
    for (c, level, white) in [
        (c8(87, 51, 31), 60u8, true),
        (c8(90, 49, 33), 60, true),
        (c8(68, 57, 50), 60, true),
        (c8(48, 63, 72), 60, false),
        (c8(131, 109, 99), 115, false),
    ] {
        assert_eq!(
            threshold_white(c, f32::from(level) / 255.0, Some(SampleType::U8)),
            white,
            "8-bit {c:?} at {level}"
        );
    }
    let c16 = |r: u16, g: u16, b: u16| [r, g, b].map(|v| f32::from(v) / 32768.0);
    for (c, level, white) in [
        (c16(8995, 7196, 6425), 60u8, false),
        (c16(11180, 6554, 3984), 60, false),
        (c16(16448, 16448, 16448), 128, true),
        (c16(16447, 16447, 16447), 128, false),
    ] {
        assert_eq!(
            threshold_white(c, f32::from(level) / 255.0, Some(SampleType::U16)),
            white,
            "16-bit {c:?} at {level}"
        );
    }
    // Through the adjustment: neutral black or white.
    let mut b = Buffer::filled(
        Rect::new(0, 0, 1, 1),
        [87.0 / 255.0, 51.0 / 255.0, 31.0 / 255.0, 1.0],
    );
    apply_depth(
        &Adjustment::Threshold {
            level: 60.0 / 255.0,
        },
        &mut b,
        Transfer::Srgb,
        Some(SampleType::U8),
    );
    assert_eq!(b.px[0], [1.0, 1.0, 1.0, 1.0]);
}

/// 16-bit sample → Photoshop's 8-bit level, from psd-tools posterize_16bits_rgb.psd: bins
/// split on the 16-bit sample (10794 and 10923 straddle a third of 32768), and the middle of 3
/// levels is 128 where 8 bits give 127, while 21 levels keep 25 for 25.5.
#[test]
fn sixteen_bit_posterize_matches_photoshop() {
    let points: [(u32, u16, u8); 9] = [
        (3, 10794, 0),
        (3, 10923, 128),
        (3, 21845, 128),
        (3, 21974, 255),
        (13, 2504, 0),
        (13, 2542, 21),
        (13, 7453, 42),
        (13, 7582, 64),
        (21, 3341, 25),
    ];
    for (levels, x, want) in points {
        let got = posterize_in(f32::from(x) / 32768.0, levels, Some(SampleType::U16));
        assert!(
            (got * 255.0 - f32::from(want)).abs() < 1e-3,
            "{levels} levels at {x}: {} vs Photoshop {want}",
            got * 255.0
        );
    }
    // 8 bits keep their rule.
    assert_eq!(
        posterize_in(128.0 / 255.0, 3, Some(SampleType::U8)),
        posterize(128.0 / 255.0, 3)
    );
}

#[test]
fn color_lookup_with_an_overflowing_stored_size_is_the_identity() {
    // A stored document's size is untrusted: (2²²)³ × 3 overflows, which once wrapped past
    // the table-length guard and indexed out of bounds.
    for size in [1 << 22, u32::MAX] {
        let a = Adjustment::ColorLookup {
            name: "x".into(),
            lut: Some(std::sync::Arc::new(vec![0.5; 24])),
            size,
            tetrahedral: false,
            dither: false,
        };
        let mut b = ramp();
        apply(&a, &mut b);
        assert_eq!(b.px, ramp().px, "size {size}");
    }
}

/// Photoshop 27.11, measured: Color Balance with one cyan–red slider moved, over a gray ramp
/// (x = 0, 16, …, 240, 255); red and green out (blue follows green on gray).
#[test]
fn color_balance_matches_photoshop() {
    const X: [u8; 17] = [
        0, 16, 32, 48, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 224, 240, 255,
    ];
    type Row = (&'static str, f32, bool, [u8; 17], [u8; 17]);
    let measured: [Row; 6] = [
        (
            "midtones",
            50.0,
            false,
            [
                0, 36, 59, 78, 96, 112, 128, 142, 156, 170, 183, 196, 209, 221, 233, 244, 255,
            ],
            X,
        ),
        (
            "shadows",
            -100.0,
            false,
            [
                0, 0, 0, 0, 0, 0, 0, 7, 23, 43, 67, 93, 122, 154, 186, 221, 255,
            ],
            X,
        ),
        (
            "highlights",
            100.0,
            false,
            [
                0, 51, 84, 111, 136, 160, 182, 202, 223, 242, 255, 255, 255, 255, 255, 255, 255,
            ],
            X,
        ),
        (
            "shadows",
            50.0,
            true,
            X,
            [
                0, 0, 0, 0, 17, 37, 57, 77, 97, 117, 137, 157, 177, 197, 216, 236, 255,
            ],
        ),
        (
            "midtones",
            100.0,
            true,
            [
                0, 36, 59, 78, 96, 112, 128, 142, 156, 170, 183, 196, 209, 221, 233, 244, 255,
            ],
            [
                0, 5, 14, 24, 36, 50, 64, 80, 97, 114, 132, 151, 171, 191, 212, 234, 255,
            ],
        ),
        (
            "highlights",
            -100.0,
            true,
            X,
            [
                0, 26, 53, 79, 105, 132, 158, 184, 211, 237, 255, 255, 255, 255, 255, 255, 255,
            ],
        ),
    ];
    // Two sliders in one tone, Preserve Luminosity on (measured): shadows red and blue +50 darken
    // only green; highlights red −50 with green +50 lift green by 100 and blue by 50.
    let two = |tones: [[f32; 3]; 3], x: f32| -> [i32; 3] {
        std::array::from_fn(|ch| {
            (color_balance_channel(tones, true, ch, x / 255.0) * 255.0).round() as i32
        })
    };
    assert_eq!(
        two([[50.0, 0.0, 50.0], [0.0; 3], [0.0; 3]], 128.0),
        [128, 97, 128]
    );
    let hi = two([[0.0; 3], [0.0; 3], [-50.0, 50.0, 0.0]], 128.0);
    assert!(
        (hi[0] - 128).abs() <= 1 && (hi[1] - 211).abs() <= 1 && (hi[2] - 159).abs() <= 1,
        "{hi:?}"
    );
    for (tone, amount, preserve, red, green) in measured {
        let mut tones = [[0.0f32; 3]; 3];
        tones[["shadows", "midtones", "highlights"]
            .iter()
            .position(|t| *t == tone)
            .unwrap()][0] = amount;
        for (i, x) in X.iter().enumerate() {
            let v = f32::from(*x) / 255.0;
            let got = |ch| (color_balance_channel(tones, preserve, ch, v) * 255.0).round() as i32;
            let (r, g) = (got(0), got(1));
            assert!(
                (r - i32::from(red[i])).abs() <= 1,
                "{tone} {amount} pl {preserve} at {x}: red {r} vs Photoshop {}",
                red[i]
            );
            assert!(
                (g - i32::from(green[i])).abs() <= 1,
                "{tone} {amount} pl {preserve} at {x}: green {g} vs Photoshop {}",
                green[i]
            );
        }
    }
}

/// Photoshop, measured (8-bit gray ramp, every channel): whole settings, where the tones fold
/// into one Levels per channel (#3423). The issue's midtones red +100 / magenta −100 (the
/// shadow toe), its sequoia14 layer with Preserve Luminosity off and on, all nine sliders with
/// Preserve Luminosity on, a black point past the white point, and lowered shadows with raised
/// highlights (their gammas cancel: a straight line between the two points).
#[test]
fn color_balance_whole_settings_match_photoshop() {
    const X: [u8; 12] = [0, 4, 8, 16, 32, 64, 96, 128, 160, 192, 224, 255];
    type Case = ([[f32; 3]; 3], bool, [[u8; 12]; 3]);
    let cases: [Case; 6] = [
        (
            [[0.0; 3], [100.0, -100.0, 0.0], [0.0; 3]],
            false,
            [
                [0, 16, 30, 55, 90, 128, 156, 181, 202, 221, 239, 255],
                [0, 0, 0, 1, 4, 16, 36, 64, 100, 145, 197, 255],
                X,
            ],
        ),
        (
            [[-6.0, 22.0, 5.0], [-1.0, -3.0, -8.0], [-1.0, -4.0, -6.0]],
            false,
            [
                [0, 0, 2, 9, 25, 56, 89, 122, 156, 188, 222, 255],
                [0, 5, 9, 18, 35, 67, 100, 131, 163, 194, 225, 255],
                [0, 3, 7, 14, 29, 60, 91, 123, 156, 189, 222, 255],
            ],
        ),
        (
            [[-6.0, 22.0, 5.0], [-1.0, -3.0, -8.0], [-1.0, -4.0, -6.0]],
            true,
            [
                [0, 0, 0, 0, 5, 42, 80, 117, 154, 189, 226, 255],
                [0, 4, 8, 16, 33, 66, 98, 130, 162, 195, 226, 255],
                [0, 0, 0, 0, 15, 48, 82, 116, 151, 186, 221, 255],
            ],
        ),
        (
            [
                [40.0, -30.0, 20.0],
                [-50.0, 25.0, 60.0],
                [30.0, -70.0, 10.0],
            ],
            true,
            [
                [0, 1, 3, 9, 25, 69, 126, 193, 255, 255, 255, 255],
                [0, 0, 0, 0, 0, 0, 46, 93, 136, 177, 217, 255],
                [0, 0, 0, 0, 44, 107, 156, 199, 238, 255, 255, 255],
            ],
        ),
        (
            [[100.0, -100.0, 0.0], [0.0; 3], [-100.0, 100.0, 0.0]],
            true,
            [
                X,
                [0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255],
                [0, 0, 0, 0, 0, 0, 0, 130, 255, 255, 255, 255],
            ],
        ),
        (
            [[-76.0, -2.0, 60.0], [0.0; 3], [99.0, 0.0, -30.0]],
            false,
            [
                [0, 0, 0, 0, 0, 0, 71, 171, 255, 255, 255, 255],
                [0, 2, 6, 14, 29, 61, 94, 126, 158, 191, 224, 255],
                [0, 6, 11, 21, 39, 73, 106, 137, 168, 197, 227, 255],
            ],
        ),
    ];
    for (tones, preserve, want) in cases {
        for (ch, row) in want.iter().enumerate() {
            for (x, w) in X.iter().zip(row) {
                let got = (color_balance_channel(tones, preserve, ch, f32::from(*x) / 255.0)
                    * 255.0)
                    .round() as i32;
                assert!(
                    (got - i32::from(*w)).abs() <= 1,
                    "{tones:?} pl {preserve} ch {ch} at {x}: {got} vs Photoshop {w}"
                );
            }
        }
    }
    // Hostile input stays in range.
    for v in [f32::NAN, -1.0, 2.0, f32::INFINITY] {
        let out = color_balance_channel([[f32::NAN, 1e9, -1e9]; 3], true, 7, v);
        assert!((0.0..=1.0).contains(&out), "{v}: {out}");
    }
}

#[test]
fn color_balance_midtones_with_multiple_sliders_preserve_luminosity() {
    // A shared offset depends on the two extremes, not the sum of moved sliders.
    // The old per-slider product leaves red/blue unchanged for [30, 0, 30].
    let tones = [[0.0; 3], [30.0, 0.0, 30.0], [0.0; 3]];
    for input in [0.25_f32, 0.5, 0.75] {
        for (ch, offset) in [-0.15_f32, 0.15, -0.15].into_iter().enumerate() {
            let expected = input.powf(2f32.powf(offset));
            let actual = color_balance_channel(tones, true, ch, input);
            assert!(
                (actual - expected).abs() < 1e-6,
                "channel {ch}, input {input}: {actual} vs {expected}"
            );
        }
        // Preserve Luminosity off must still affect only the moved channels.
        assert_eq!(color_balance_channel(tones, false, 1, input), input);
        // One moved slider retains its previous half-strength compensation.
        let single = [[0.0; 3], [30.0, 0.0, 0.0], [0.0; 3]];
        assert!(
            (color_balance_channel(single, true, 0, input) - input.powf(2f32.powf(-0.15))).abs()
                < 1e-6
        );
        assert!(
            (color_balance_channel(single, true, 1, input) - input.powf(2f32.powf(0.15))).abs()
                < 1e-6
        );
    }
}

#[test]
fn levels_and_curves_apply_each_channel_before_the_master() {
    // These two affine tone adjustments describe the same mapping:
    // channel(x) = 1 / 2 + x / 2, then master(x) = x / 2, so 0 maps to 0.25.
    // Levels used to apply the master first (0 mapped to 0.5).
    let line = |a: f32, b: f32| {
        vec![
            CurvePoint {
                input: 0.0,
                output: a,
            },
            CurvePoint {
                input: 1.0,
                output: b,
            },
        ]
    };
    let levels = Adjustment::Levels {
        master: LevelsChannel {
            out_white: 0.5,
            ..Default::default()
        },
        per_channel: std::array::from_fn(|_| LevelsChannel {
            out_black: 0.5,
            ..Default::default()
        }),
        space: ToneSpace::Rgb,
        black: Default::default(),
    };
    let curves = Adjustment::Curves {
        master: line(0.0, 0.5),
        per_channel: std::array::from_fn(|_| line(0.5, 1.0)),
        space: ToneSpace::Rgb,
        black: Vec::new(),
        pencil: false,
    };
    for depth in [
        None,
        Some(SampleType::U8),
        Some(SampleType::U16),
        Some(SampleType::F32),
    ] {
        let ll = tone_luts_depth(&levels, depth);
        let cl = tone_luts_depth(&curves, depth);
        for channel in 0..3 {
            for index in [
                0,
                LUT_SIZE / 4,
                LUT_SIZE / 2,
                3 * LUT_SIZE / 4,
                LUT_SIZE - 1,
            ] {
                assert!(
                    (ll[channel][index] - cl[channel][index]).abs() < 0.01,
                    "{depth:?} channel {channel} sample {index}: Levels {} vs Curves {}",
                    ll[channel][index],
                    cl[channel][index]
                );
            }
        }
        assert!((cl[0][0] - 0.25).abs() < 0.01, "{depth:?}: {}", cl[0][0]);

        // Exercise the compositor's real adjustment path, not only its LUT builder.
        for adj in [&levels, &curves] {
            let mut pixel = Buffer {
                rect: Rect::new(0, 0, 1, 1),
                px: vec![[0.0, 0.0, 0.0, 1.0]],
            };
            apply_depth(adj, &mut pixel, Transfer::Srgb, depth);
            for channel in 0..3 {
                assert!(
                    (pixel.px[0][channel] - 0.25).abs() < 0.01,
                    "{depth:?}: {adj:?} rendered {:?}",
                    pixel.px[0]
                );
            }
        }
    }

    // Lab has no composite/master tone record.
    let mut lab = curves.clone();
    let mut lab_without_master = curves;
    if let Adjustment::Curves { space, .. } = &mut lab {
        *space = ToneSpace::Lab;
    }
    if let Adjustment::Curves { master, space, .. } = &mut lab_without_master {
        *space = ToneSpace::Lab;
        *master = line(0.0, 1.0);
    }
    assert_eq!(tone_luts(&lab), tone_luts(&lab_without_master));
}

#[test]
fn identity_tone_in_every_space_is_identity() {
    for space in [ToneSpace::Rgb, ToneSpace::Cmyk, ToneSpace::Lab] {
        let mut a = Adjustment::identity_curves();
        if let Adjustment::Curves { space: s, .. } = &mut a {
            *s = space;
        }
        let mut b = ramp();
        apply(&a, &mut b);
        for (x, y) in b.px.iter().zip(ramp().px.iter()) {
            for k in 0..3 {
                assert!((x[k] - y[k]).abs() < 0.02, "{space:?} {x:?} {y:?}");
            }
        }
    }
}

#[test]
fn cmyk_black_curve_darkens_and_lab_lightness_lifts() {
    let line = |a: f32, b: f32| {
        vec![
            CurvePoint {
                input: 0.0,
                output: a,
            },
            CurvePoint {
                input: 1.0,
                output: b,
            },
        ]
    };
    // Black brightness 1 -> 0.5: more black ink everywhere.
    let a = Adjustment::Curves {
        master: line(0.0, 1.0),
        per_channel: [line(0.0, 1.0), line(0.0, 1.0), line(0.0, 1.0)],
        space: ToneSpace::Cmyk,
        black: line(0.0, 0.5),
        pencil: false,
    };
    let mut b = Buffer {
        rect: Rect::new(0, 0, 1, 1),
        px: vec![[0.8, 0.8, 0.8, 1.0]],
    };
    apply(&a, &mut b);
    assert!(b.px[0][0] < 0.7, "{:?}", b.px[0]);
    // Lab lightness 0..1 -> 0.2..1 lifts a dark grey; a/b untouched keeps it grey.
    let a = Adjustment::Curves {
        master: line(1.0, 0.0), // ignored in Lab
        per_channel: [line(0.2, 1.0), line(0.0, 1.0), line(0.0, 1.0)],
        space: ToneSpace::Lab,
        black: Vec::new(),
        pencil: false,
    };
    let mut b = Buffer {
        rect: Rect::new(0, 0, 1, 1),
        px: vec![[0.2, 0.2, 0.2, 1.0]],
    };
    apply(&a, &mut b);
    let p = b.px[0];
    assert!(p[0] > 0.3 && (p[0] - p[2]).abs() < 0.02, "{p:?}");
}

#[test]
fn hue_ranges_only_touch_their_colours() {
    let mut ranges = HueRange::defaults();
    ranges[0].saturation = -100.0; // desaturate reds
    let a = Adjustment::HueSaturation {
        hue: 0.0,
        saturation: 0.0,
        lightness: 0.0,
        colorize: false,
        ranges,
    };
    let mut b = Buffer {
        rect: Rect::new(0, 0, 3, 1),
        px: vec![
            [0.9, 0.1, 0.1, 1.0],
            [0.1, 0.1, 0.9, 1.0],
            [0.5, 0.5, 0.5, 1.0],
        ],
    };
    apply(&a, &mut b);
    let red = b.px[0];
    assert!((red[0] - red[1]).abs() < 0.05, "red desaturated {red:?}");
    assert!(
        (b.px[1][2] - 0.9).abs() < 1e-4 && (b.px[1][0] - 0.1).abs() < 1e-4,
        "blue untouched {:?}",
        b.px[1]
    );
    assert_eq!(b.px[2], [0.5, 0.5, 0.5, 1.0]);
}

#[test]
fn colorize_at_zero_saturation_is_grey() {
    // Photoshop: Colorize at 0% saturation leaves no colour, only the pixel's lightness (#2326).
    let px = vec![
        [0.9, 0.2, 0.1, 1.0],
        [0.1, 0.6, 0.8, 1.0],
        [0.5, 0.5, 0.5, 1.0],
    ];
    for depth in [
        None,
        Some(SampleType::U8),
        Some(SampleType::U16),
        Some(SampleType::F32),
    ] {
        for lightness in [0.0, 30.0, -30.0] {
            let a = Adjustment::HueSaturation {
                hue: 200.0,
                saturation: 0.0,
                lightness,
                colorize: true,
                ranges: HueRange::defaults(),
            };
            let mut b = Buffer {
                rect: Rect::new(0, 0, 3, 1),
                px: px.clone(),
            };
            apply_depth(&a, &mut b, Transfer::Srgb, depth);
            for p in &b.px {
                assert!(
                    (p[0] - p[1]).abs() < 1e-4 && (p[1] - p[2]).abs() < 1e-4,
                    "{depth:?} lightness {lightness}: {p:?}"
                );
            }
        }
        // Low saturations still tint: the old 25% floor no longer hides them.
        let tint = |saturation: f32| {
            let a = Adjustment::HueSaturation {
                hue: 0.0,
                saturation,
                lightness: 0.0,
                colorize: true,
                ranges: HueRange::defaults(),
            };
            let mut b = Buffer {
                rect: Rect::new(0, 0, 1, 1),
                px: vec![[0.5, 0.5, 0.5, 1.0]],
            };
            apply_depth(&a, &mut b, Transfer::Srgb, depth);
            b.px[0][0] - b.px[0][2]
        };
        assert!(
            tint(10.0) > 0.05 && tint(10.0) < tint(25.0),
            "{depth:?}: 10% {} vs 25% {}",
            tint(10.0),
            tint(25.0)
        );
    }
}

/// Photoshop 27.11, measured: Hue/Saturation raising saturation, and Colorize with Lightness.
#[test]
fn hue_saturation_matches_photoshop() {
    // (input, hue, saturation, lightness, colorize, Photoshop's output)
    type Case = ([u8; 3], f32, f32, f32, bool, [u8; 3]);
    let measured: [Case; 8] = [
        ([204, 102, 102], 0.0, 50.0, 0.0, false, [254, 51, 51]),
        ([204, 159, 102], 0.0, 50.0, 0.0, false, [254, 164, 51]),
        ([204, 102, 102], 0.0, -50.0, 0.0, false, [178, 127, 127]),
        ([0, 0, 0], 120.0, 100.0, 30.0, true, [0, 152, 0]),
        ([28, 28, 28], 120.0, 100.0, 30.0, true, [0, 192, 0]),
        ([160, 160, 160], 30.0, 25.0, -40.0, true, [120, 96, 73]),
        ([252, 252, 252], 30.0, 25.0, -40.0, true, [177, 151, 126]),
        ([255, 143, 0], 30.0, 25.0, -40.0, true, [95, 76, 58]),
    ];
    for (input, hue, saturation, lightness, colorize, want) in measured {
        let a = Adjustment::HueSaturation {
            hue,
            saturation,
            lightness,
            colorize,
            ranges: HueRange::defaults(),
        };
        let c = input.map(|v| v as f32 / 255.0);
        let mut b = Buffer {
            rect: Rect::new(0, 0, 1, 1),
            px: vec![[c[0], c[1], c[2], 1.0]],
        };
        apply(&a, &mut b);
        for (i, w) in want.iter().enumerate() {
            let got = b.px[0][i] * 255.0;
            assert!((got - *w as f32).abs() <= 2.0, "{input:?} h{hue} s{saturation} l{lightness} colorize {colorize}, channel {i}: {got} vs {w}");
        }
    }
}

/// Photoshop 27.11, measured: a range's Lightness (Blues ±50) moves towards the brightest or darkest channel.
#[test]
fn range_lightness_matches_photoshop() {
    let measured = [
        ([0u8, 29, 115], 0.5, [57u8, 71, 115]),
        ([0, 64, 255], 0.5, [125, 157, 255]),
        ([102, 128, 204], 0.5, [152, 165, 204]),
        ([102, 128, 204], -0.5, [102, 115, 154]),
        ([0, 18, 115], -0.5, [0, 9, 58]),
    ];
    for (input, l, want) in measured {
        let got = range_lightness(input.map(|v| v as f32 / 255.0), l);
        for i in 0..3 {
            assert!(
                (got[i] * 255.0 - want[i] as f32).abs() <= 2.5,
                "{input:?} channel {i}: {} vs {}",
                got[i] * 255.0,
                want[i]
            );
        }
    }
}

/// Photoshop's own render (merged image of psd-tools `adjustments/huesaturation_lightness_rgb.psd`,
/// layer "Hue/Saturation 1"): a dull magenta gets the whole range edit, as a vivid one does (#4089).
#[test]
fn hue_ranges_apply_in_full_to_dull_colours() {
    let range = |lightness: f32, bounds: [f32; 4]| HueRange {
        hue: 0.0,
        saturation: 0.0,
        lightness,
        bounds,
    };
    let ranges = [
        range(-11.0, [-57.0, 4.0, 82.0, 175.0]),
        range(100.0, [12.0, 45.0, 75.0, 105.0]),
        HueRange::neutral(2),
        HueRange::neutral(3),
        HueRange::neutral(4),
        range(100.0, [-117.0, -50.0, 39.0, 104.0]),
    ];
    let a = Adjustment::HueSaturation {
        hue: 0.0,
        saturation: 0.0,
        lightness: 17.0,
        colorize: false,
        ranges,
    };
    let measured = [
        ([126u8, 97, 120], [148u8, 148, 148]),
        ([131, 103, 125], [152, 152, 152]),
        ([95, 69, 90], [122, 122, 122]),
    ];
    for (input, want) in measured {
        let c = input.map(|v| v as f32 / 255.0);
        let mut b = Buffer {
            rect: Rect::new(0, 0, 1, 1),
            px: vec![[c[0], c[1], c[2], 1.0]],
        };
        apply(&a, &mut b);
        for (i, w) in want.iter().enumerate() {
            let got = b.px[0][i] * 255.0;
            assert!(
                (got - *w as f32).abs() <= 2.0,
                "{input:?} channel {i}: {got} vs {w}"
            );
        }
    }
    // Greys have no hue, so the ranges leave them alone: only the master Lightness moves them.
    let mut b = Buffer {
        rect: Rect::new(0, 0, 1, 1),
        px: vec![[0.4, 0.4, 0.4, 1.0]],
    };
    apply(&a, &mut b);
    let p = b.px[0];
    assert!(
        (p[0] - p[1]).abs() < 1e-6
            && (p[1] - p[2]).abs() < 1e-6
            && (p[0] - (0.4 + 0.6 * 0.17)).abs() < 1e-4,
        "{p:?}"
    );
}

#[test]
fn black_white_weights() {
    let d = [40.0, 60.0, 40.0, 60.0, 20.0, 80.0];
    assert!(
        (black_white_gray([0.5; 3], &d) - 0.5).abs() < 1e-6,
        "greys keep their value"
    );
    assert!((black_white_gray([1.0, 0.0, 0.0], &d) - 0.4).abs() < 1e-6);
    assert!((black_white_gray([1.0, 1.0, 0.0], &d) - 0.6).abs() < 1e-6);
    let mut w = d;
    w[0] = 100.0;
    assert!(black_white_gray([1.0, 0.0, 0.0], &w) > black_white_gray([1.0, 0.0, 0.0], &d));
}

/// Photoshop 27.11, measured: Black & White (default weights) with Tint over greys. The tint
/// is blended like the Color blend mode: its hue and saturation at the grey's luminosity.
#[test]
fn black_white_tint_matches_photoshop() {
    let weights = [40.0, 60.0, 40.0, 60.0, 20.0, 80.0];
    let measured: [([u8; 3], u8, [u8; 3]); 6] = [
        ([255, 0, 0], 24, [81, 1, 1]),
        ([255, 0, 0], 120, [255, 64, 64]),
        ([255, 0, 0], 240, [255, 235, 235]),
        ([0xe1, 0xd3, 0xb3], 120, [133, 119, 87]),
        ([0xe1, 0xd3, 0xb3], 24, [33, 24, 1]),
        ([0x40, 0x20, 0x10], 120, [144, 112, 96]),
    ];
    for (tint, grey, want) in measured {
        let tint = Some(tint.map(|v| v as f32 / 255.0));
        let mut b = Buffer {
            rect: Rect::new(0, 0, 1, 1),
            px: vec![[grey as f32 / 255.0; 4]],
        };
        b.px[0][3] = 1.0;
        apply(&Adjustment::BlackWhite { weights, tint }, &mut b);
        for (i, w) in want.iter().enumerate() {
            let got = b.px[0][i] * 255.0;
            assert!(
                (got - *w as f32).abs() <= 2.5,
                "tint {tint:?} grey {grey} channel {i}: {got} vs {w}"
            );
        }
    }
}

#[test]
fn gradient_map_follows_the_w3c_luminosity() {
    // Photoshop's Gradient Map is driven by 0.3 R + 0.59 G + 0.11 B (photoshop corpus rgb8 and
    // rgb16 gradient-map.psd), not the Rec. 601 grey.
    let stops = vec![(0.0, [0.0; 3]), (1.0, [1.0; 3])];
    for (c, t) in [
        ([0.0, 1.0, 0.0], 0.59),
        ([1.0, 0.0, 0.0], 0.3),
        ([0.0, 0.0, 1.0], 0.11),
        ([0.2, 0.6, 0.9], 0.513),
    ] {
        let mut b = Buffer::filled(Rect::new(0, 0, 1, 1), [c[0], c[1], c[2], 1.0]);
        apply(
            &Adjustment::GradientMap {
                stops: stops.clone(),
                reverse: false,
                dither: false,
            },
            &mut b,
        );
        assert!((b.px[0][0] - t).abs() < 1e-5, "{c:?}: {}", b.px[0][0]);
    }
}

#[test]
fn gradient_map_dither_stays_close() {
    let stops = vec![(0.0, [0.0; 3]), (1.0, [1.0; 3])];
    let mut a = ramp();
    let mut b = ramp();
    apply(
        &Adjustment::GradientMap {
            stops: stops.clone(),
            reverse: false,
            dither: false,
        },
        &mut a,
    );
    apply(
        &Adjustment::GradientMap {
            stops,
            reverse: false,
            dither: true,
        },
        &mut b,
    );
    assert!(a
        .px
        .iter()
        .zip(&b.px)
        .all(|(x, y)| (x[0] - y[0]).abs() <= 0.5 / 255.0 + 1e-6));
    assert!(a.px.iter().zip(&b.px).any(|(x, y)| x[0] != y[0]));
}
