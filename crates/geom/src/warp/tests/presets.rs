//! Preset styles against Photoshop's measurements.

use super::*;

#[test]
fn styles_parse_and_round_trip_names() {
    for s in WarpStyle::all() {
        assert_eq!(WarpStyle::parse(s.id()), Some(s));
        assert_eq!(WarpStyle::parse(s.psd_name()), Some(s));
        assert_eq!(WarpStyle::parse(s.label()), Some(s), "{s:?}");
    }
    assert_eq!(WarpStyle::parse("spiral"), None);
}

#[test]
fn presets_at_bend_zero_are_identity() {
    for s in WarpStyle::PRESETS {
        let w = Warp::preset(s, 0.0, B);
        assert!(w.is_identity());
        assert_eq!(w.map(33.0, 44.0), (33.0, 44.0));
    }
}

const SQUARE: [f64; 4] = [128.0, 128.0, 384.0, 384.0];
const WIDE: [f64; 4] = [128.0, 208.0, 384.0, 304.0];
const TALL: [f64; 4] = [208.0, 128.0, 304.0, 384.0];

/// Photoshop 27.11, measured: a 256 px grid image whose colours encode each pixel's own
/// position, warped in a 512 px document over the box (128, 128)–(384, 384). Each pair is a
/// source point and where Photoshop put it.
#[test]
fn presets_match_photoshop() {
    type Case = (WarpStyle, f64, f64, f64, bool, [([f64; 2], [f64; 2]); 3]);
    let cases: [Case; 9] = [
        (
            WarpStyle::Arc,
            50.0,
            0.0,
            0.0,
            false,
            [
                ([272.5, 155.5], [296.5, 104.5]),
                ([331.5, 182.5], [424.5, 168.5]),
                ([259.5, 157.5], [264.5, 104.5]),
            ],
        ),
        (
            WarpStyle::Arc,
            -100.0,
            0.0,
            0.0,
            false,
            [
                ([277.5, 354.5], [344.5, 472.5]),
                ([140.5, 218.5], [40.5, 168.5]),
                ([172.5, 237.5], [56.5, 264.5]),
            ],
        ),
        (
            WarpStyle::Flag,
            50.0,
            0.0,
            0.0,
            false,
            [
                ([136.5, 143.5], [136.5, 120.5]),
                ([136.5, 159.5], [136.5, 136.5]),
                ([136.5, 175.5], [136.5, 152.5]),
            ],
        ),
        (
            WarpStyle::Twist,
            -50.0,
            0.0,
            0.0,
            false,
            [
                ([148.5, 205.5], [152.5, 216.5]),
                ([197.5, 212.5], [200.5, 232.5]),
                ([134.5, 212.5], [136.5, 216.5]),
            ],
        ),
        (
            WarpStyle::ShellLower,
            50.0,
            0.0,
            0.0,
            false,
            [
                ([248.5, 136.5], [248.5, 136.5]),
                ([264.5, 136.5], [264.5, 136.5]),
                ([232.5, 136.5], [232.5, 136.5]),
            ],
        ),
        (
            WarpStyle::Fish,
            -100.0,
            0.0,
            0.0,
            false,
            [
                ([296.5, 355.5], [296.5, 440.5]),
                ([344.5, 298.5], [344.5, 344.5]),
                ([328.5, 356.5], [328.5, 472.5]),
            ],
        ),
        (
            WarpStyle::Bulge,
            50.0,
            50.0,
            0.0,
            false,
            [
                ([293.5, 147.5], [296.5, 88.5]),
                ([293.5, 375.5], [296.5, 440.5]),
                ([340.5, 281.5], [344.5, 296.5]),
            ],
        ),
        (
            WarpStyle::Arc,
            50.0,
            0.0,
            -50.0,
            false,
            [
                ([181.5, 350.5], [184.5, 328.5]),
                ([291.5, 372.5], [280.5, 344.5]),
                ([232.5, 349.5], [232.5, 312.5]),
            ],
        ),
        (
            WarpStyle::Arc,
            50.0,
            0.0,
            0.0,
            true,
            [
                ([155.5, 272.5], [104.5, 296.5]),
                ([182.5, 331.5], [168.5, 424.5]),
                ([157.5, 259.5], [104.5, 264.5]),
            ],
        ),
    ];
    // Wide (256 × 96 at 128, 208) and tall (96 × 256 at 208, 128) boxes, and a vertical warp
    // with distortion.
    type Boxed = (
        WarpStyle,
        f64,
        f64,
        f64,
        bool,
        [f64; 4],
        [([f64; 2], [f64; 2]); 2],
    );
    let boxed: [Boxed; 6] = [
        (
            WarpStyle::Arc,
            50.0,
            0.0,
            0.0,
            false,
            WIDE,
            [
                ([144.5, 269.6], [120.5, 264.5]),
                ([271.5, 220.42], [280.5, 168.5]),
            ],
        ),
        (
            WarpStyle::Bulge,
            50.0,
            0.0,
            0.0,
            false,
            TALL,
            [
                ([249.11, 374.5], [248.5, 392.5]),
                ([249.11, 360.5], [248.5, 376.5]),
            ],
        ),
        (
            WarpStyle::Flag,
            50.0,
            0.0,
            0.0,
            true,
            WIDE,
            [
                ([137.5, 296.42], [184.5, 296.5]),
                ([153.5, 296.42], [200.5, 296.5]),
            ],
        ),
        (
            WarpStyle::Rise,
            50.0,
            0.0,
            0.0,
            true,
            TALL,
            [
                ([250.97, 152.5], [344.5, 152.5]),
                ([301.26, 264.5], [344.5, 264.5]),
            ],
        ),
        (
            WarpStyle::Bulge,
            60.0,
            20.0,
            -10.0,
            false,
            WIDE,
            [
                ([208.5, 224.15], [200.5, 184.5]),
                ([219.5, 277.42], [216.5, 296.5]),
            ],
        ),
        (
            WarpStyle::Arc,
            50.0,
            30.0,
            0.0,
            true,
            SQUARE,
            [
                ([331.5, 327.5], [296.5, 360.5]),
                ([318.5, 240.5], [264.5, 232.5]),
            ],
        ),
    ];
    let all = cases
        .iter()
        .map(|c| (c.0, c.1, c.2, c.3, c.4, SQUARE, c.5.to_vec()))
        .chain(
            boxed
                .iter()
                .map(|c| (c.0, c.1, c.2, c.3, c.4, c.5, c.6.to_vec())),
        );
    for (style, bend, hd, vd, vertical, bounds, pairs) in all {
        let w = Warp {
            h_distort: hd,
            v_distort: vd,
            vertical,
            ..Warp::preset(style, bend, bounds)
        };
        for (src, want) in pairs {
            let got = w.map(src[0], src[1]);
            let d = (got.0 - want[0]).hypot(got.1 - want[1]);
            assert!(d < 1.5, "{style:?} {bend} h{hd} v{vd} vertical {vertical} {bounds:?}: {src:?} -> {got:?}, Photoshop {want:?}");
        }
    }
}

#[test]
fn preset_fits_closely_as_mesh_and_affine_composes() {
    let w = Warp::preset(WarpStyle::Bulge, 40.0, B);
    let m = Warp::custom(w.to_mesh(4, 4), B);
    for (x, y) in [(20.0, 30.0), (60.0, 45.0), (100.0, 65.0)] {
        assert!(
            close(w.map(x, y), m.map(x, y), 0.5),
            "{:?} vs {:?}",
            w.map(x, y),
            m.map(x, y)
        );
    }
    let t =
        Warp::custom(BezierMesh::identity(B, 1, 1), B).then_affine([1.0, 0.0, 0.0, 1.0, 5.0, -2.0]);
    assert!(close(t.map(50.0, 50.0), (55.0, 48.0), 1e-9));
    let ob = Warp::none(B).output_bounds();
    assert!((ob[0] - 10.0).abs() < 1e-9 && (ob[3] - 70.0).abs() < 1e-9);
}
