//! Native counterpart of benchmarks/remotion/scene.tsx.
use rust_motion_animation::{spring_with_options, SpringConfig, SpringOptions};
use rust_motion_rasterizer::{
    render_to_ffmpeg_pipe, Color, FrameConfig, PipeConfig, RasterizerBackend, Scene, SceneNode,
    TinySkiaBackend,
};
use std::time::Instant;

fn rect(x: f32, y: f32, w: f32, h: f32, fill: Color) -> SceneNode {
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

fn scene(frame: u32) -> Scene {
    let mut scene = Scene::new();
    scene.push(rect(0.0, 0.0, 1280.0, 720.0, Color::rgb(15, 23, 42)));
    for index in 0..32 {
        let progress = spring_with_options(
            (frame % 60) as f64,
            30.0,
            SpringConfig::default(),
            SpringOptions {
                duration_in_frames: Some(24.0),
                delay: (index % 8) as f64 * 2.0,
                ..Default::default()
            },
        )
        .unwrap();
        let x = (60.0 + (index % 8) as f64 * 145.0 + progress * 40.0).round() as f32;
        let y = 80.0 + (index / 8) as f32 * 140.0;
        scene.push(rect(
            x,
            y,
            64.0,
            64.0,
            Color::rgb(80 + index as u8 * 4, 160, 220),
        ));
    }
    scene
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let backend = TinySkiaBackend::new();
    if args[1] == "--still" {
        let frame: u32 = args[2].parse().unwrap();
        backend
            .render_frame(&scene(frame), &FrameConfig::new(1280, 720, frame, 30.0))
            .unwrap()
            .save(&args[3])
            .unwrap();
        return;
    }
    let concurrency: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    let config = PipeConfig::new(1280, 720, 30.0, 180, &args[1])
        .with_concurrency(concurrency)
        .with_quality(18, "fast")
        .with_control(rust_motion_rasterizer::RenderControl::new().with_stderr_render_stats());
    let start = Instant::now();
    render_to_ffmpeg_pipe(&backend, &config, scene).unwrap();
    println!(
        "{{\"render_ms\":{}}}",
        start.elapsed().as_secs_f64() * 1000.0
    );
}
