use rust_motion_rasterizer::affinity_loader::{
    affinity_color_to_rgba, affinity_path_to_svg_d, load_affinity_as_scene, map_affinity_blend,
    read_affinity_preview, render_affinity_to_image, render_affinity_to_pixmap,
};
use rust_motion_rasterizer::photocraft_affinity::model::{Path as AffPath, Point, SubPath};
use rust_motion_rasterizer::photocraft_affinity::paint::Color as AffColor;
use rust_motion_rasterizer::scene::BlendMode;

#[test]
fn test_affinity_color_conversions() {
    // RGB
    let rgb = AffColor::Rgb {
        r: 1.0,
        g: 0.5,
        b: 0.0,
        a: 1.0,
    };
    let c = affinity_color_to_rgba(&rgb);
    assert_eq!(c.r, 255);
    assert_eq!(c.g, 128);
    assert_eq!(c.b, 0);
    assert_eq!(c.a, 255);

    // Gray
    let gray = AffColor::Gray { v: 0.5, a: 0.8 };
    let c_gray = affinity_color_to_rgba(&gray);
    assert_eq!(c_gray.r, 128);
    assert_eq!(c_gray.g, 128);
    assert_eq!(c_gray.b, 128);
    assert_eq!(c_gray.a, 204);

    // CMYK
    let cmyk = AffColor::Cmyk {
        c: 0.0,
        m: 1.0,
        y: 0.0,
        k: 0.0,
        a: 1.0,
    };
    let c_cmyk = affinity_color_to_rgba(&cmyk);
    assert_eq!(c_cmyk.r, 255);
    assert_eq!(c_cmyk.g, 0);
    assert_eq!(c_cmyk.b, 255);
    assert_eq!(c_cmyk.a, 255);

    // Lab
    let lab = AffColor::Lab {
        l: 100.0,
        a: 0.0,
        b: 0.0,
        alpha: 1.0,
    };
    let c_lab = affinity_color_to_rgba(&lab);
    assert_eq!(c_lab.r, 255);
    assert_eq!(c_lab.g, 255);
    assert_eq!(c_lab.b, 255);
}

#[test]
fn test_affinity_blend_mode_mapping() {
    use rust_motion_rasterizer::photocraft_affinity::model::Blend;
    assert_eq!(map_affinity_blend(Blend::Normal), BlendMode::Normal);
    assert_eq!(map_affinity_blend(Blend::Multiply), BlendMode::Multiply);
    assert_eq!(map_affinity_blend(Blend::Screen), BlendMode::Screen);
    assert_eq!(map_affinity_blend(Blend::Overlay), BlendMode::Overlay);
    assert_eq!(map_affinity_blend(Blend::Darken), BlendMode::Darken);
    assert_eq!(map_affinity_blend(Blend::Lighten), BlendMode::Lighten);
    assert_eq!(map_affinity_blend(Blend::Difference), BlendMode::Difference);
}

#[test]
fn test_affinity_path_conversion() {
    let path = AffPath {
        subpaths: vec![
            SubPath {
                start: Point { x: 10.0, y: 10.0 },
                segments: vec![
                    [Point { x: 20.0, y: 10.0 }, Point { x: 30.0, y: 20.0 }, Point { x: 40.0, y: 20.0 }],
                ],
                closed: true,
            },
        ],
    };
    let svg = affinity_path_to_svg_d(&path);
    assert!(svg.starts_with("M 10 10"));
    assert!(svg.contains("C 20 10 30 20 40 20"));
    assert!(svg.ends_with("Z "));
}

#[test]
fn test_corrupt_or_empty_affinity_data_handling() {
    let empty_data = b"";
    assert!(load_affinity_as_scene(empty_data).is_err());
    assert!(read_affinity_preview(empty_data).is_err());
    assert!(render_affinity_to_image(empty_data).is_err());
    assert!(render_affinity_to_pixmap(empty_data).is_err());

    let garbage = b"THIS_IS_NOT_AN_AFFINITY_FILE_DATA_1234567890";
    assert!(load_affinity_as_scene(garbage).is_err());
    assert!(read_affinity_preview(garbage).is_err());
}
