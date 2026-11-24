//! Colour range, hue families and tonal range tests.

use super::*;

#[test]
fn skin_tones_selects_skin_and_not_cream_or_white() {
    let px = |rgb: [u8; 3]| [rgb[0], rgb[1], rgb[2]].map(|v| f32::from(v) / 255.0);
    let rgba = |rgb: [u8; 3]| {
        let [r, g, b] = px(rgb);
        [r, g, b, 1.0]
    };
    // Skin in light and shadow, then a cream shirt, a near-white wall, blue denim and black.
    let skin = [
        [183, 119, 81],
        [177, 132, 93],
        [180, 121, 84],
        [120, 76, 52],
        [173, 107, 75],
        [255, 219, 172],
        [194, 140, 104],
        [141, 85, 36],
        [92, 51, 23],
    ]
    .map(rgba);
    let m = skin_tones(&skin, 40.0);
    assert!(m.iter().all(|&v| v > 0.95), "skin {m:?}");
    let other = [
        [231, 225, 202],
        [252, 253, 247],
        [40, 60, 110],
        [5, 5, 5],
        [128, 128, 128],
        [220, 40, 40],
        [200, 160, 40],
    ]
    .map(rgba);
    let m = skin_tones(&other, 40.0);
    assert!(m.iter().all(|&v| v < 0.05), "not skin {m:?}");
    // Fuzziness widens the region; transparent pixels are never selected.
    let pale = [rgba([230, 215, 200])];
    assert!(skin_tones(&pale, 200.0)[0] > skin_tones(&pale, 0.0)[0]);
    assert_eq!(skin_tones(&[[0.7, 0.45, 0.3, 0.0]], 40.0), vec![0.0]);
}

#[test]
fn color_range_falloff() {
    let px = vec![
        [1.0, 0.0, 0.0, 1.0],
        [0.9, 0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
        [1.0, 0.0, 0.0, 0.0],
    ];
    let m = color_range(&px, [1.0, 0.0, 0.0], 40.0);
    assert_eq!(m[0], 1.0);
    assert!(m[1] > 0.3 && m[1] < 1.0);
    assert_eq!(m[2], 0.0);
    assert_eq!(m[3], 0.0);
}

#[test]
fn color_range_several_samples_and_localized() {
    let red = [1.0, 0.0, 0.0, 1.0];
    let blue = [0.0, 0.0, 1.0, 1.0];
    let green = [0.0, 1.0, 0.0, 1.0];
    // 4 × 1: red, blue, green, red.
    let px = vec![red, blue, green, red];
    let s = |c: [f32; 4], at: Option<(f32, f32)>| RangeSample {
        color: [c[0], c[1], c[2]],
        at,
    };
    let m = color_range_samples(&px, 4, &[s(red, None), s(blue, None)], 10.0, None);
    assert_eq!(m, vec![1.0, 1.0, 0.0, 1.0]);
    // One sample is exactly the single-colour Color Range.
    assert_eq!(
        color_range_samples(&px, 4, &[s(red, None)], 40.0, None),
        color_range(&px, [1.0, 0.0, 0.0], 40.0)
    );
    // Localized at x = 0 with a 2 px range: the far red pixel (x = 3) drops out, x = 1 would be half.
    let m = color_range_samples(&px, 4, &[s(red, Some((0.0, 0.0)))], 10.0, Some(2.0));
    assert_eq!(m, vec![1.0, 0.0, 0.0, 0.0]);
    let reds = vec![red; 4];
    let m = color_range_samples(&reds, 4, &[s(red, Some((0.0, 0.0)))], 10.0, Some(2.0));
    assert_eq!(m, vec![1.0, 0.5, 0.0, 0.0]);
    // No samples: nothing selected.
    assert!(color_range_samples(&px, 4, &[], 10.0, None)
        .iter()
        .all(|v| *v == 0.0));
}

#[test]
fn hue_families() {
    let px = vec![
        [1.0, 0.0, 0.0, 1.0], // red
        [1.0, 0.5, 0.0, 1.0], // orange: between red and yellow
        [0.5, 0.5, 0.5, 1.0], // grey
        [1.0, 0.5, 0.5, 1.0], // pink: a muted red
        [0.0, 0.0, 1.0, 1.0], // blue
        [1.0, 0.0, 0.0, 0.0], // transparent
    ];
    let reds = hue_range(&px, 0.0);
    assert_eq!(reds[0], 1.0);
    assert!((reds[1] - 0.5).abs() < 1e-5);
    assert_eq!(reds[2], 0.0);
    assert!((reds[3] - 0.5).abs() < 1e-5);
    assert_eq!(reds[4], 0.0);
    assert_eq!(reds[5], 0.0);
    let yellows = hue_range(&px, 60.0);
    assert!((yellows[1] - 0.5).abs() < 1e-5);
    assert_eq!(hue_range(&px, 240.0)[4], 1.0);
    // Magentas wrap around 360°.
    assert_eq!(hue_range(&[[1.0, 0.0, 1.0, 1.0]], 300.0)[0], 1.0);
    assert!((hue_range(&[[1.0, 0.0, 0.5, 1.0]], 300.0)[0] - 0.5).abs() < 1e-5);
}

#[test]
fn hue_families_weigh_dark_colours_like_photoshop() {
    // Photoshop 2025's Blues on an 8-bit sRGB document (selection levels 0..255, #5243): the
    // muted navy #3d3f54, a dark blue (0, 0, 84) and a mid blue (61, 61, 153).
    let c = |r: u8, g: u8, b: u8| {
        [
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
            1.0,
        ]
    };
    let blues = hue_range(&[c(0x3d, 0x3f, 0x54), c(0, 0, 84), c(61, 61, 153)], 240.0);
    for (got, want) in blues.iter().zip([35.0, 140.0, 129.0]) {
        assert!(
            (got * 255.0 - want).abs() <= 1.0,
            "{} vs Photoshop {want}",
            got * 255.0
        );
    }
}

#[test]
fn tonal_ranges() {
    let g = |v: f32| [v / 255.0, v / 255.0, v / 255.0, 1.0];
    let px = vec![g(0.0), g(60.0), g(75.0), g(128.0), g(200.0), g(255.0)];
    let near = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4);
    // Shadows up to 65, fading over 20 levels.
    let sh = tone_range(&px, 0.0, 65.0, 20.0);
    assert!(near(&sh, &[1.0, 1.0, 0.5, 0.0, 0.0, 0.0]), "{sh:?}");
    // Highlights from 190.
    let h = tone_range(&px, 190.0, 255.0, 20.0);
    assert!(near(&h, &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0]), "{h:?}");
    // Midtones 105..150.
    let m = tone_range(&px, 105.0, 150.0, 20.0);
    assert!(near(&m, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0]), "{m:?}");
}
