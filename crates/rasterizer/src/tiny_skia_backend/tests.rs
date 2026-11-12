use super::*;
use crate::backend::FrameConfig;
use crate::scene::{
    BlendMode, ClipRegion, Color, ImageFit, MaskMode, Scene, SceneFilter, SceneNode, SceneShadow,
    TrackMatteMode, Transform2D,
};
use image::Rgba;

fn render(scene: &Scene, w: u32, h: u32) -> RgbaImage {
    let backend = TinySkiaBackend::new();
    let config = FrameConfig::new(w, h, 0, 30.0);
    backend.render_frame(scene, &config).expect("render failed")
}

#[test]
fn test_solid_red_rect() {
    let mut scene = Scene::new();
    scene.push(SceneNode::Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
        fill: Color::rgb(255, 0, 0),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    });

    let img = render(&scene, 100, 100);
    let px = img.get_pixel(50, 50);
    assert_eq!(px[0], 255, "Red channel should be 255");
    assert_eq!(px[1], 0, "Green channel should be 0");
    assert_eq!(px[2], 0, "Blue channel should be 0");
}

#[test]
fn test_circle_center_pixel() {
    let mut scene = Scene::new();
    scene.push(SceneNode::Circle {
        cx: 100.0,
        cy: 100.0,
        r: 80.0,
        fill: Color::rgb(0, 0, 255),
        stroke: None,
        stroke_width: 0.0,
    });

    let img = render(&scene, 200, 200);
    let px = img.get_pixel(100, 100);
    assert_eq!(px[2], 255, "Blue channel at circle center should be 255");
}

#[test]
fn svg_elliptical_arcs_render_full_circles() {
    let scene = Scene {
        nodes: vec![SceneNode::Path {
            d: "M 50 0 A 50 50 0 1 0 50 100 A 50 50 0 1 0 50 0 Z".into(),
            fill: Some(Color::rgb(255, 0, 0)),
            stroke: None,
            stroke_width: 0.0,
            opacity: 1.0,
        }],
    };
    let image = render(&scene, 100, 100);

    assert!(image.get_pixel(50, 50)[0] > 240);
    assert_eq!(image.get_pixel(0, 0)[3], 0);
    assert_eq!(image.get_pixel(99, 99)[3], 0);
}

#[test]
fn svg_arc_flags_select_the_expected_pie_quadrant() {
    let scene = Scene {
        nodes: vec![SceneNode::Path {
            d: "M 50 50 L 50 0 A 50 50 0 0 1 100 50 Z".into(),
            fill: Some(Color::rgb(0, 255, 0)),
            stroke: None,
            stroke_width: 0.0,
            opacity: 1.0,
        }],
    };
    let image = render(&scene, 100, 100);

    assert!(image.get_pixel(75, 25)[1] > 240);
    assert_eq!(image.get_pixel(25, 25)[3], 0);
    assert_eq!(image.get_pixel(75, 75)[3], 0);
}

#[test]
fn test_transparent_background() {
    let scene = Scene::new(); // empty
    let img = render(&scene, 64, 64);
    let px = img.get_pixel(32, 32);
    assert_eq!(px[3], 0, "Empty scene should be fully transparent");
}

#[test]
fn test_linear_gradient_renders() {
    use crate::scene::GradientStop;
    let mut scene = Scene::new();
    scene.push(SceneNode::LinearGradient {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 100.0,
        angle_deg: 90.0,
        stops: vec![
            GradientStop {
                position: 0.0,
                color: Color::rgb(255, 0, 0),
            },
            GradientStop {
                position: 1.0,
                color: Color::rgb(0, 0, 255),
            },
        ],
    });

    let img = render(&scene, 200, 100);
    // Left pixel should be more red than blue
    let left = img.get_pixel(5, 50);
    let right = img.get_pixel(195, 50);
    assert!(left[0] > left[2], "Left side of gradient should be redder");
    assert!(
        right[2] > right[0],
        "Right side of gradient should be bluer"
    );
}

#[test]
fn test_group_with_opacity() {
    let mut scene = Scene::new();
    scene.push(SceneNode::Group {
        transform: Default::default(),
        opacity: 0.5,
        children: vec![SceneNode::Rect {
            x: 10.0,
            y: 10.0,
            w: 80.0,
            h: 80.0,
            fill: Color::rgb(255, 255, 255),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    });

    let img = render(&scene, 100, 100);
    let px = img.get_pixel(50, 50);
    // At 50% opacity over transparent, alpha should be ~127
    assert!(
        px[3] > 50 && px[3] < 200,
        "Group opacity should reduce alpha"
    );
}

fn composited_layer(children: Vec<SceneNode>) -> SceneNode {
    SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: Vec::new(),
        shadow: None,
        children,
    }
}

fn solid_rect(fill: Color, x: f32, y: f32, w: f32, h: f32) -> SceneNode {
    SceneNode::Rect {
        x,
        y,
        w,
        h,
        fill,
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    }
}

#[test]
fn layer_rect_clip_limits_pixels() {
    let mut layer = composited_layer(vec![solid_rect(
        Color::rgb(255, 0, 0),
        0.0,
        0.0,
        64.0,
        64.0,
    )]);
    let SceneNode::Layer { clip, .. } = &mut layer else {
        unreachable!();
    };
    *clip = Some(ClipRegion::Rect {
        x: 16.0,
        y: 16.0,
        w: 32.0,
        h: 32.0,
        corner_radius: 0.0,
    });
    let image = render(&Scene { nodes: vec![layer] }, 64, 64);

    assert_eq!(image.get_pixel(32, 32), &Rgba([255, 0, 0, 255]));
    assert_eq!(image.get_pixel(8, 8)[3], 0);
}

#[test]
fn opaque_normal_layer_matches_direct_children() {
    let children = vec![
        solid_rect(Color::rgb(255, 0, 0), 4.0, 5.0, 20.0, 12.0),
        solid_rect(Color::rgb(0, 0, 255), 12.0, 9.0, 20.0, 12.0),
    ];
    let direct = render(
        &Scene {
            nodes: children.clone(),
        },
        40,
        32,
    );
    let layered = render(
        &Scene {
            nodes: vec![composited_layer(children)],
        },
        40,
        32,
    );

    assert_eq!(direct, layered);
}

#[test]
fn transparent_layer_does_not_render_children() {
    let layer = SceneNode::Layer {
        opacity: 0.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Blur { sigma: 20.0 }],
        shadow: Some(SceneShadow {
            offset_x: 10.0,
            offset_y: 10.0,
            blur_sigma: 5.0,
            color: Color::WHITE,
        }),
        children: vec![solid_rect(Color::WHITE, 0.0, 0.0, 40.0, 32.0)],
    };
    let image = render(&Scene { nodes: vec![layer] }, 40, 32);

    assert!(image.pixels().all(|pixel| pixel[3] == 0));
}

#[test]
fn layer_alpha_mask_controls_coverage() {
    let mut layer = composited_layer(vec![solid_rect(
        Color::rgb(255, 0, 0),
        0.0,
        0.0,
        64.0,
        64.0,
    )]);
    let SceneNode::Layer { mask, .. } = &mut layer else {
        unreachable!();
    };
    *mask = Some(vec![solid_rect(Color::WHITE, 0.0, 0.0, 32.0, 64.0)]);
    let image = render(&Scene { nodes: vec![layer] }, 64, 64);

    assert_eq!(image.get_pixel(16, 32), &Rgba([255, 0, 0, 255]));
    assert_eq!(image.get_pixel(48, 32)[3], 0);
}

#[test]
fn layer_luminance_mask_uses_rendered_color() {
    let mut layer = composited_layer(vec![solid_rect(
        Color::rgb(0, 0, 255),
        0.0,
        0.0,
        64.0,
        64.0,
    )]);
    let SceneNode::Layer {
        mask, mask_mode, ..
    } = &mut layer
    else {
        unreachable!();
    };
    *mask_mode = MaskMode::Luminance;
    *mask = Some(vec![
        solid_rect(Color::WHITE, 0.0, 0.0, 32.0, 64.0),
        solid_rect(Color::BLACK, 32.0, 0.0, 32.0, 64.0),
    ]);
    let image = render(&Scene { nodes: vec![layer] }, 64, 64);

    assert_eq!(image.get_pixel(16, 32), &Rgba([0, 0, 255, 255]));
    assert_eq!(image.get_pixel(48, 32)[3], 0);
}

#[test]
fn layer_alpha_inverted_mask_inverts_coverage() {
    let mut layer = composited_layer(vec![solid_rect(
        Color::rgb(255, 0, 0),
        0.0,
        0.0,
        64.0,
        64.0,
    )]);
    let SceneNode::Layer {
        mask, mask_mode, ..
    } = &mut layer
    else {
        unreachable!();
    };
    *mask_mode = MaskMode::AlphaInverted;
    *mask = Some(vec![solid_rect(Color::WHITE, 0.0, 0.0, 32.0, 64.0)]);
    let image = render(&Scene { nodes: vec![layer] }, 64, 64);

    // x: 0..32 is masked out (inverted alpha)
    assert_eq!(image.get_pixel(16, 32)[3], 0);
    // x: 32..64 remains visible
    assert_eq!(image.get_pixel(48, 32), &Rgba([255, 0, 0, 255]));
}

#[test]
fn layer_luminance_inverted_mask_inverts_luma() {
    let mut layer = composited_layer(vec![solid_rect(
        Color::rgb(0, 255, 0),
        0.0,
        0.0,
        64.0,
        64.0,
    )]);
    let SceneNode::Layer {
        mask, mask_mode, ..
    } = &mut layer
    else {
        unreachable!();
    };
    *mask_mode = MaskMode::LuminanceInverted;
    *mask = Some(vec![
        solid_rect(Color::WHITE, 0.0, 0.0, 32.0, 64.0),
        solid_rect(Color::BLACK, 32.0, 0.0, 32.0, 64.0),
    ]);
    let image = render(&Scene { nodes: vec![layer] }, 64, 64);

    // White has luma 1.0 -> inverted luma 0.0 -> masked out
    assert_eq!(image.get_pixel(16, 32)[3], 0);
    // Black has luma 0.0 -> inverted luma 1.0 -> full opacity
    assert_eq!(image.get_pixel(48, 32), &Rgba([0, 255, 0, 255]));
}

#[test]
fn track_matte_alpha_and_inverted() {
    let target = solid_rect(Color::rgb(255, 0, 0), 0.0, 0.0, 64.0, 64.0);
    let matte = solid_rect(Color::WHITE, 0.0, 0.0, 32.0, 64.0);

    // Alpha mode
    let scene_alpha = Scene {
        nodes: vec![SceneNode::TrackMatte {
            target: Box::new(target.clone()),
            matte: Box::new(matte.clone()),
            mode: TrackMatteMode::Alpha,
        }],
    };
    let img_alpha = render(&scene_alpha, 64, 64);
    assert_eq!(img_alpha.get_pixel(16, 32), &Rgba([255, 0, 0, 255]));
    assert_eq!(img_alpha.get_pixel(48, 32)[3], 0);

    // AlphaInverted mode
    let scene_inv = Scene {
        nodes: vec![SceneNode::TrackMatte {
            target: Box::new(target),
            matte: Box::new(matte),
            mode: TrackMatteMode::AlphaInverted,
        }],
    };
    let img_inv = render(&scene_inv, 64, 64);
    assert_eq!(img_inv.get_pixel(16, 32)[3], 0);
    assert_eq!(img_inv.get_pixel(48, 32), &Rgba([255, 0, 0, 255]));
}

#[test]
fn track_matte_luma_and_inverted() {
    let target = solid_rect(Color::rgb(0, 0, 255), 0.0, 0.0, 64.0, 64.0);
    let matte = SceneNode::Group {
        transform: Default::default(),
        opacity: 1.0,
        children: vec![
            solid_rect(Color::WHITE, 0.0, 0.0, 32.0, 64.0),
            solid_rect(Color::BLACK, 32.0, 0.0, 32.0, 64.0),
        ],
    };

    // Luma mode: White passes target, Black blocks
    let scene_luma = Scene {
        nodes: vec![SceneNode::TrackMatte {
            target: Box::new(target.clone()),
            matte: Box::new(matte.clone()),
            mode: TrackMatteMode::Luma,
        }],
    };
    let img_luma = render(&scene_luma, 64, 64);
    assert_eq!(img_luma.get_pixel(16, 32), &Rgba([0, 0, 255, 255]));
    assert_eq!(img_luma.get_pixel(48, 32)[3], 0);

    // LumaInverted mode: White blocks, Black passes target
    let scene_luma_inv = Scene {
        nodes: vec![SceneNode::TrackMatte {
            target: Box::new(target),
            matte: Box::new(matte),
            mode: TrackMatteMode::LumaInverted,
        }],
    };
    let img_luma_inv = render(&scene_luma_inv, 64, 64);
    assert_eq!(img_luma_inv.get_pixel(16, 32)[3], 0);
    assert_eq!(img_luma_inv.get_pixel(48, 32), &Rgba([0, 0, 255, 255]));
}

#[test]
fn null_object_transforms_children_without_pixel_overhead() {
    let scene = Scene {
        nodes: vec![SceneNode::NullObject {
            id: Some("controller".into()),
            transform: Transform2D::translate(20.0, 10.0),
            opacity: 0.5,
            children: vec![solid_rect(Color::rgb(255, 0, 0), 0.0, 0.0, 20.0, 20.0)],
        }],
    };
    let img = render(&scene, 64, 64);

    // Outside child: NullObject draws NO pixels itself
    assert_eq!(img.get_pixel(5, 5)[3], 0);
    assert_eq!(img.get_pixel(50, 50)[3], 0);

    // Inside transformed child (translated by (20, 10)):
    let child_px = img.get_pixel(25, 15);
    assert!(
        (child_px[0] as i32 - 128).abs() <= 2,
        "premultiplied red should be ~128"
    );
    assert!(
        (child_px[3] as i32 - 128).abs() <= 2,
        "alpha should be ~128 due to opacity 0.5"
    );
}

#[test]
fn layer_multiply_blends_with_destination() {
    let background = solid_rect(Color::rgb(200, 100, 50), 0.0, 0.0, 32.0, 32.0);
    let mut layer = composited_layer(vec![solid_rect(
        Color::rgb(128, 255, 255),
        0.0,
        0.0,
        32.0,
        32.0,
    )]);
    let SceneNode::Layer { blend_mode, .. } = &mut layer else {
        unreachable!();
    };
    *blend_mode = BlendMode::Multiply;
    let image = render(
        &Scene {
            nodes: vec![background, layer],
        },
        32,
        32,
    );
    let pixel = image.get_pixel(16, 16);

    assert!((98..=102).contains(&pixel[0]));
    assert!((98..=102).contains(&pixel[1]));
    assert!((48..=52).contains(&pixel[2]));
}

#[test]
fn layer_filters_apply_in_order() {
    let mut layer = composited_layer(vec![solid_rect(
        Color::rgb(255, 0, 0),
        0.0,
        0.0,
        16.0,
        16.0,
    )]);
    let SceneNode::Layer { filters, .. } = &mut layer else {
        unreachable!();
    };
    *filters = vec![
        SceneFilter::Grayscale { amount: 1.0 },
        SceneFilter::Opacity { amount: 0.5 },
    ];
    let image = render(&Scene { nodes: vec![layer] }, 16, 16);
    let pixel = image.get_pixel(8, 8);

    assert!((26..=28).contains(&pixel[0]));
    assert!((26..=28).contains(&pixel[1]));
    assert!((26..=28).contains(&pixel[2]));
    assert!((127..=128).contains(&pixel[3]));
}

#[test]
fn layer_shadow_uses_masked_alpha_and_blur() {
    let mut layer = composited_layer(vec![solid_rect(Color::WHITE, 8.0, 8.0, 12.0, 12.0)]);
    let SceneNode::Layer { shadow, .. } = &mut layer else {
        unreachable!();
    };
    *shadow = Some(SceneShadow {
        offset_x: 20.0,
        offset_y: 0.0,
        blur_sigma: 2.0,
        color: Color::rgb(255, 0, 0),
    });
    let image = render(&Scene { nodes: vec![layer] }, 48, 32);
    let shadow_pixel = image.get_pixel(34, 14);

    assert!(shadow_pixel[0] > 0);
    assert_eq!(shadow_pixel[1], 0);
    assert_eq!(shadow_pixel[2], 0);
    assert!(image.get_pixel(26, 14)[3] > 0, "blur should expand alpha");
}

#[test]
fn invalid_layer_filter_returns_scene_error() {
    let mut layer = composited_layer(Vec::new());
    let SceneNode::Layer { filters, .. } = &mut layer else {
        unreachable!();
    };
    filters.push(SceneFilter::Blur { sigma: f32::NAN });
    let error = TinySkiaBackend::headless()
        .render_frame(
            &Scene { nodes: vec![layer] },
            &FrameConfig::new(16, 16, 0, 30.0),
        )
        .unwrap_err();

    assert!(error.to_string().contains("Scene compositing error"));
    assert!(error.to_string().contains("blur sigma"));
}

#[test]
fn missing_explicit_font_returns_asset_error() {
    let scene = Scene {
        nodes: vec![SceneNode::Text {
            x: 0.0,
            y: 16.0,
            content: "missing".into(),
            font_size: 16.0,
            color: Color::WHITE,
            font_weight: 400,
            font_sources: vec!["/rust-motion/does-not-exist.ttf".into()],
        }],
    };
    let error = TinySkiaBackend::headless()
        .render_frame(&scene, &FrameConfig::new(64, 32, 0, 30.0))
        .unwrap_err();

    assert!(error.to_string().contains("Font asset error"));
    assert!(error.to_string().contains("does-not-exist.ttf"));
}

#[test]
fn local_image_is_fitted_and_decoded_once() {
    let path = std::env::temp_dir().join(format!(
        "rust-motion-rasterizer-image-{}.png",
        std::process::id()
    ));
    RgbaImage::from_pixel(4, 2, Rgba([255, 0, 0, 255]))
        .save(&path)
        .unwrap();

    let scene = Scene {
        nodes: vec![SceneNode::Image {
            src: path.display().to_string(),
            x: 0.0,
            y: 0.0,
            w: 4.0,
            h: 4.0,
            fit: ImageFit::Contain,
            opacity: 1.0,
        }],
    };
    let backend = TinySkiaBackend::headless();
    let config = FrameConfig::new(4, 4, 0, 30.0);
    let first = backend.render_frame(&scene, &config).unwrap();
    let second = backend.render_frame(&scene, &config).unwrap();

    assert_eq!(first.get_pixel(2, 0)[3], 0, "letterbox should be clear");
    assert_eq!(first.get_pixel(2, 2), &Rgba([255, 0, 0, 255]));
    assert_eq!(first, second);
    assert_eq!(backend.images.len(), 1);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn missing_image_returns_an_asset_error() {
    let path = std::env::temp_dir().join(format!(
        "rust-motion-missing-image-{}.png",
        std::process::id()
    ));
    let scene = Scene {
        nodes: vec![SceneNode::Image {
            src: path.display().to_string(),
            x: 0.0,
            y: 0.0,
            w: 4.0,
            h: 4.0,
            fit: ImageFit::Cover,
            opacity: 1.0,
        }],
    };
    let error = TinySkiaBackend::headless()
        .render_frame(&scene, &FrameConfig::new(4, 4, 0, 30.0))
        .unwrap_err();

    assert!(error.to_string().contains("Image asset error"));
    assert!(error.to_string().contains("rust-motion-missing-image"));
}

#[test]
fn video_frame_is_decoded_and_cached() {
    if std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("skipping video decode test: FFmpeg is unavailable");
        return;
    }

    let dir = std::env::temp_dir().join(format!(
        "rust-motion-video-frame-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("red.mkv");
    let generated = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=16x16:r=2:d=7",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=7",
            "-shortest",
            "-c:v",
            "ffv1",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(&source)
        .status()
        .unwrap();
    assert!(generated.success());

    let scene = Scene {
        nodes: vec![SceneNode::Video {
            src: source.display().to_string(),
            time: 0.0,
            looped: false,
            x: 0.0,
            y: 0.0,
            w: 16.0,
            h: 16.0,
            fit: ImageFit::Cover,
            opacity: 1.0,
            interpolation: Default::default(),
            time_remap: None,
        }],
    };
    let backend = TinySkiaBackend::headless();
    let config = FrameConfig::new(16, 16, 0, 2.0);
    let first = backend.render_frame(&scene, &config).unwrap();
    let mut next_scene = scene.clone();
    let SceneNode::Video { time, .. } = &mut next_scene.nodes[0] else {
        unreachable!();
    };
    *time = 0.5;
    let second = backend.render_frame(&next_scene, &config).unwrap();

    let pixel = first.get_pixel(8, 8);
    assert!(pixel[0] > 240 && pixel[1] < 20 && pixel[2] < 20);
    assert_eq!(first, second);
    assert!(backend.videos.bytes() > 0);
    assert_eq!(
        backend.videos.spawn_count(),
        1,
        "sequential frames should reuse one persistent decoder"
    );

    let mut jump_scene = scene.clone();
    let SceneNode::Video { time, .. } = &mut jump_scene.nodes[0] else {
        unreachable!();
    };
    *time = 6.5;
    backend.render_frame(&jump_scene, &config).unwrap();
    assert_eq!(
        backend.videos.spawn_count(),
        2,
        "large seeks should restart"
    );

    let mut reverse_scene = scene.clone();
    let SceneNode::Video { time, .. } = &mut reverse_scene.nodes[0] else {
        unreachable!();
    };
    *time = 1.0;
    backend.render_frame(&reverse_scene, &config).unwrap();
    assert_eq!(
        backend.videos.spawn_count(),
        3,
        "uncached reverse seeks should restart"
    );

    let mut loop_scene = scene.clone();
    let SceneNode::Video { time, looped, .. } = &mut loop_scene.nodes[0] else {
        unreachable!();
    };
    *time = 7.25;
    *looped = true;
    backend.render_frame(&loop_scene, &config).unwrap();

    let mut eof_scene = scene.clone();
    let SceneNode::Video { time, .. } = &mut eof_scene.nodes[0] else {
        unreachable!();
    };
    *time = 20.0;
    backend.render_frame(&eof_scene, &config).unwrap();
    assert_eq!(backend.videos.decoder_count(), 1);

    let metadata = crate::probe_video_metadata(source.to_str().unwrap()).unwrap();
    assert_eq!((metadata.width, metadata.height), (16, 16));
    assert_eq!((metadata.display_width, metadata.display_height), (16, 16));
    assert_eq!(metadata.fps, Some(2.0));
    assert_eq!(metadata.audio_stream_indices.len(), 1);
    assert!(metadata.duration.is_some_and(|duration| duration >= 7.0));

    backend.shutdown_media();
    assert_eq!(backend.videos.decoder_count(), 0);
    std::fs::remove_dir_all(dir).unwrap();
}
