//! "The Gauntlet" Extreme Stress Benchmark — 2x Video Decoders, 20 Images, 60 Particles, Sigma 30 Blur, Multilingual Text.
#![allow(clippy::manual_is_multiple_of)]
use rust_motion_rasterizer::{
    render_to_ffmpeg_pipe, BlendMode, Color, FrameConfig, ImageFit, MaskMode, PipeConfig,
    RasterizerBackend, RenderControl, Scene, SceneFilter, SceneNode, TinySkiaBackend,
};
use std::time::Instant;

fn scene(frame: u32) -> Scene {
    let mut root_scene = Scene::new();
    let time = frame as f64 / 30.0;
    let t = frame as f32 * 0.05;

    // 0. Dark tech background
    root_scene.push(SceneNode::Rect {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
        fill: Color::rgb(10, 12, 22),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    });

    // Grid lines (15 horizontal)
    let grid_offset = ((frame * 2) % 60) as f32;
    for i in 0..15 {
        let y = 60.0 + i as f32 * 70.0 + grid_offset * 0.4;
        root_scene.push(SceneNode::Path {
            d: format!("M 0,{:.1} L 1920,{:.1}", y, y),
            fill: None,
            stroke: Some(Color::rgba(25, 35, 65, 80)),
            stroke_width: 1.0,
            opacity: 0.7,
        });
    }

    // 1. Dual Video PiP Streams (Testing continuous FFmpeg frame decoding)
    // Left Video PiP
    root_scene.push(SceneNode::Rect {
        x: 56.0,
        y: 76.0,
        w: 608.0,
        h: 345.5,
        fill: Color::TRANSPARENT,
        stroke: Some(Color::rgb(0, 240, 255)),
        stroke_width: 3.0,
        corner_radius: 8.0,
    });
    root_scene.push(SceneNode::Video {
        src: "target/assets/test_video.mp4".into(),
        time,
        looped: true,
        x: 60.0,
        y: 80.0,
        w: 600.0,
        h: 337.5,
        fit: ImageFit::Cover,
        opacity: 1.0,
    });

    // Right Video PiP (offset playback rate)
    root_scene.push(SceneNode::Rect {
        x: 1256.0,
        y: 76.0,
        w: 608.0,
        h: 345.5,
        fill: Color::TRANSPARENT,
        stroke: Some(Color::rgb(255, 0, 128)),
        stroke_width: 3.0,
        corner_radius: 8.0,
    });
    root_scene.push(SceneNode::Video {
        src: "target/assets/test_video.mp4".into(),
        time: time * 1.5,
        looped: true,
        x: 1260.0,
        y: 80.0,
        w: 600.0,
        h: 337.5,
        fit: ImageFit::Cover,
        opacity: 0.9,
    });

    // 2. 20 High-Res Image Tiles with Sinusoidal Floating
    for i in 0..20 {
        let row = i / 10;
        let col = i % 10;
        let base_x = 80.0 + col as f32 * 176.0;
        let base_y = if row == 0 { 460.0 } else { 610.0 };
        let float_y = base_y + (t + i as f32 * 0.5).sin() * 12.0;

        root_scene.push(SceneNode::Image {
            src: format!("target/assets/img_{:02}.png", i),
            x: base_x,
            y: float_y,
            w: 140.0,
            h: 140.0,
            fit: ImageFit::Contain,
            opacity: 0.95,
        });
    }

    // 3. 60 Floating Light Particle Orbs
    for i in 0..60 {
        let angle = t * 0.8 + i as f32 * 0.35;
        let px = 960.0 + (angle.cos() * (300.0 + (i as f32 * 8.0).sin() * 180.0));
        let py = 540.0 + (angle.sin() * (220.0 + (i as f32 * 6.0).cos() * 140.0));
        let radius = 3.0 + (i % 5) as f32 * 1.5;

        let col = if i % 2 == 0 {
            Color::rgba(0, 240, 255, 140)
        } else {
            Color::rgba(255, 0, 128, 140)
        };

        root_scene.push(SceneNode::Circle {
            cx: px,
            cy: py,
            r: radius,
            fill: col,
            stroke: None,
            stroke_width: 0.0,
        });
    }

    // 4. Heavy Blur Layer (Sigma 30.0 Blur on a large 1720x220 container)
    let blur_panel = SceneNode::Layer {
        opacity: 0.85,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Blur { sigma: 30.0 }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 100.0,
            y: 800.0,
            w: 1720.0,
            h: 220.0,
            fill: Color::rgba(30, 20, 50, 220),
            stroke: Some(Color::rgb(255, 255, 255)),
            stroke_width: 2.0,
            corner_radius: 16.0,
        }],
    };
    root_scene.push(blur_panel);

    // 5. Multilingual Foreground Text (Rendered over the blur panel)
    root_scene.push(SceneNode::Text {
        x: 160.0,
        y: 875.0,
        content: "THE GAUNTLET // 극한 부하 스트레스 테스트".into(),
        font_size: 40.0,
        color: Color::WHITE,
        font_weight: 700,
        font_sources: vec!["/System/Library/Fonts/Supplemental/AppleGothic.ttf".into()],
    });

    root_scene.push(SceneNode::Text {
        x: 160.0,
        y: 940.0,
        content:
            "900 Frames (30s) • 2x Video Decoders • 20 Images • Sigma 30.0 Blur • 60 Particles"
                .into(),
        font_size: 24.0,
        color: Color::rgb(0, 240, 255),
        font_weight: 600,
        font_sources: vec!["/System/Library/Fonts/Supplemental/AppleGothic.ttf".into()],
    });

    root_scene
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let backend = TinySkiaBackend::new();

    if args.len() >= 4 && args[1] == "--still" {
        let frame: u32 = args[2].parse().expect("Invalid frame number");
        backend
            .render_frame(&scene(frame), &FrameConfig::new(1920, 1080, frame, 30.0))
            .expect("Render still failed")
            .save(&args[3])
            .expect("Save still failed");
        return;
    }

    let output_path = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("target/gauntlet.mp4");
    let concurrency: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    let total_frames: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(900);

    let config = PipeConfig::new(1920, 1080, 30.0, total_frames, output_path)
        .with_concurrency(concurrency)
        .with_quality(18, "fast")
        .with_control(RenderControl::new().with_stderr_progress());

    let start = Instant::now();
    render_to_ffmpeg_pipe(&backend, &config, scene).expect("Render to ffmpeg pipe failed");
    let render_ms = start.elapsed().as_secs_f64() * 1000.0;

    println!(
        "{{\"render_ms\":{:.2},\"fps\":{:.2},\"concurrency\":{},\"frames\":{}}}",
        render_ms,
        total_frames as f64 / (render_ms / 1000.0),
        concurrency,
        total_frames
    );
}
