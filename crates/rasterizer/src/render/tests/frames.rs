//! PNG-sequence render modes and timed single-frame rendering.

use super::*;

#[test]
fn test_sequential_renders_correct_count() {
    let backend = TinySkiaBackend::headless();
    let tmp = unique_temp_dir("sequential");
    let _ = std::fs::remove_dir_all(&tmp);

    let config = NativeRenderConfig::new(64, 64, 30.0, 5, &tmp);
    let paths = render_all_frames(&backend, &config, |frame| {
        let mut s = Scene::new();
        s.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 64.0,
            h: 64.0,
            fill: Color::rgb(frame as u8 * 40, 0, 0),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });
        s
    })
    .expect("sequential render failed");

    assert_eq!(paths.len(), 5, "Should have 5 frame files");
    for p in &paths {
        assert!(p.exists(), "PNG file should exist: {p:?}");
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_parallel_renders_same_count_as_sequential() {
    let backend = TinySkiaBackend::headless();
    let tmp = unique_temp_dir("parallel");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create test tmp dir");

    let config = NativeRenderConfig::new(64, 64, 30.0, 10, &tmp).with_concurrency(4);

    let paths = render_parallel(&backend, &config, solid_scene(Color::rgb(0, 0, 255)))
        .expect("parallel render failed");

    assert_eq!(paths.len(), 10, "Should have 10 frame files");
    // Verify they are in order
    for (i, p) in paths.iter().enumerate() {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(name, format!("frame_{:06}.png", i + 1));
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_parallel_pixel_values_correct() {
    let backend = TinySkiaBackend::headless();
    let tmp = unique_temp_dir("parallel_px");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create test tmp dir");

    let config = NativeRenderConfig::new(64, 64, 30.0, 4, &tmp);
    let paths = render_parallel(&backend, &config, |frame| {
        let mut s = Scene::new();
        // Different colour per frame so we can verify correctness
        s.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 64.0,
            h: 64.0,
            fill: Color::rgb(frame as u8 * 60, 0, 0),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });
        s
    })
    .expect("parallel pixel render failed");

    for (i, path) in paths.iter().enumerate() {
        let img = image::open(path).expect("open frame").into_rgba8();
        let px = img.get_pixel(32, 32);
        let expected_r = (i as u8) * 60;
        assert_eq!(px[0], expected_r, "Frame {i}: red channel mismatch");
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_frame_timed() {
    let backend = TinySkiaBackend::headless();
    let mut scene = Scene::new();
    scene.push(SceneNode::Circle {
        cx: 32.0,
        cy: 32.0,
        r: 20.0,
        fill: Color::rgb(255, 128, 0),
        stroke: None,
        stroke_width: 0.0,
    });
    let config = FrameConfig::new(64, 64, 0, 30.0);

    let (img, elapsed) =
        render_frame_timed(&backend, &scene, &config).expect("timed render failed");
    assert_eq!(img.width(), 64);
    println!("Single 64x64 frame: {:?}", elapsed);
}
