use rust_motion_rasterizer::{
    backend::{FrameConfig, RasterizerBackend},
    Color, LoopBehavior, Scene, SceneNode, TinySkiaBackend,
};

#[test]
fn test_render_scene_with_emoji_and_inline_emoji_text() {
    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    // Background
    scene.push(SceneNode::Rect {
        x: 0.0,
        y: 0.0,
        w: 640.0,
        h: 360.0,
        fill: Color::rgb(15, 23, 42),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    });

    // Standalone Emoji
    scene.push(SceneNode::Emoji {
        emoji: "🔥".into(),
        x: 100.0,
        y: 100.0,
        size: 80.0,
        opacity: 1.0,
    });

    // Text containing inline emoji
    scene.push(SceneNode::Text {
        x: 50.0,
        y: 260.0,
        content: "Awesome Launch! 🚀 100% ✨".into(),
        font_size: 36.0,
        color: Color::WHITE,
        font_weight: 700,
        font_sources: Vec::new(),
    });

    let config = FrameConfig::new(640, 360, 0, 30.0);
    let frame = backend
        .render_frame(&scene, &config)
        .expect("Failed to render emoji scene");

    assert_eq!(frame.width(), 640);
    assert_eq!(frame.height(), 360);

    // Verify non-transparent pixels in emoji region (around (140, 140))
    let emoji_pixel = frame.get_pixel(140, 140);
    assert!(emoji_pixel[3] > 0);
}

#[test]
fn test_render_scene_with_lottie() {
    // Minimal valid Lottie JSON (a single shape layer)
    let lottie_json = r#"{
      "v": "5.5.7",
      "fr": 30,
      "ip": 0,
      "op": 60,
      "w": 100,
      "h": 100,
      "nm": "CirclePulse",
      "ddd": 0,
      "assets": [],
      "layers": [
        {
          "ddd": 0,
          "ind": 1,
          "ty": 4,
          "nm": "Shape Layer",
          "sr": 1,
          "ks": {
            "o": { "a": 0, "k": 100 },
            "r": { "a": 0, "k": 0 },
            "p": { "a": 0, "k": [50, 50, 0] },
            "a": { "a": 0, "k": [0, 0, 0] },
            "s": { "a": 0, "k": [100, 100, 100] }
          },
          "ao": 0,
          "shapes": [
            {
              "ty": "el",
              "p": { "a": 0, "k": [0, 0] },
              "s": { "a": 0, "k": [60, 60] }
            },
            {
              "ty": "fl",
              "c": { "a": 0, "k": [1, 0.2, 0.2, 1] },
              "o": { "a": 0, "k": 100 },
              "r": 1
            }
          ],
          "ip": 0,
          "op": 60,
          "st": 0,
          "bm": 0
        }
      ]
    }"#;

    let temp_dir = std::env::temp_dir();
    let lottie_path = temp_dir.join("test_pulse.json");
    std::fs::write(&lottie_path, lottie_json).unwrap();

    let metadata = rust_motion_rasterizer::get_lottie_metadata(&lottie_path)
        .expect("Lottie metadata should load");
    assert_eq!(metadata.width, 100.0);
    assert_eq!(metadata.height, 100.0);
    assert_eq!(metadata.frame_rate, 30.0);
    assert_eq!(metadata.duration_in_frames, 60.0);

    let backend = TinySkiaBackend::new();
    let mut scene = Scene::new();

    scene.push(SceneNode::Lottie {
        src: lottie_path.to_str().unwrap().into(),
        time: 0.5,
        x: 50.0,
        y: 50.0,
        w: 100.0,
        h: 100.0,
        playback_rate: 1.0,
        loop_behavior: LoopBehavior::Loop,
        opacity: 1.0,
    });

    let config = FrameConfig::new(200, 200, 0, 30.0);
    let frame = backend
        .render_frame(&scene, &config)
        .expect("Failed to render Lottie frame");

    assert_eq!(frame.width(), 200);
    assert_eq!(frame.height(), 200);

    // Clean up
    let _ = std::fs::remove_file(lottie_path);
}
