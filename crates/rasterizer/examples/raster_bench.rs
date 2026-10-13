//! Deterministic CPU raster workload; hashes are computed outside the render timer.
use rust_motion_rasterizer::{
    BlendMode, Color, FrameConfig, MaskMode, RasterizerBackend, Scene, SceneFilter, SceneNode,
    TinySkiaBackend,
};
use std::{collections::hash_map::DefaultHasher, hash::Hasher, time::Instant};

fn scene(width: u32, height: u32, frame: u32, effects: bool) -> Scene {
    let rect = |x, y, w, h, fill| SceneNode::Rect {
        x,
        y,
        w,
        h,
        fill,
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    };
    let mut scene = Scene::new();
    scene.push(rect(
        0.0,
        0.0,
        width as f32,
        height as f32,
        Color::rgb(15, 23, 42),
    ));
    for index in 0..32 {
        scene.push(rect(
            (index % 8) as f32 * width as f32 / 8.0 + (frame % 20) as f32,
            (index / 8) as f32 * height as f32 / 4.0,
            width as f32 / 16.0,
            height as f32 / 8.0,
            Color::rgba(80 + index * 4, 160, 220, 180),
        ));
    }
    if effects {
        scene.nodes = vec![SceneNode::Layer {
            opacity: 0.9,
            blend_mode: BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: MaskMode::Alpha,
            shadow: None,
            filters: vec![SceneFilter::Vignette {
                offset: 0.25,
                darkness: 0.65,
                roundness: 0.85,
            }],
            children: scene.nodes,
        }];
    }
    scene
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let width: u32 = args[1].parse().unwrap();
    let height: u32 = args[2].parse().unwrap();
    let effects = args[3] == "effects";
    let backend = TinySkiaBackend::headless();
    let mut elapsed = 0.0;
    let mut hashes = Vec::new();
    for frame in 0..28 {
        let scene = scene(width, height, frame, effects);
        let start = Instant::now();
        let image = backend
            .render_frame(&scene, &FrameConfig::new(width, height, frame, 30.0))
            .unwrap();
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        if frame >= 4 {
            elapsed += ms;
            let mut hash = DefaultHasher::new();
            hash.write(image.as_raw());
            hashes.push(format!("{:016x}", hash.finish()));
        }
    }
    println!(
        "{}",
        serde_json::json!({"ms_per_frame":elapsed/24.0, "frames":24, "hashes":hashes})
    );
}
