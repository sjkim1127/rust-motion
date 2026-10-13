//! Cyberpunk Motion Graphic Benchmark — Text Auto-Fitting, Offscreen Layer Filters, Vignette & Chromatic Aberration.
#![allow(clippy::manual_is_multiple_of)]
use rust_motion_animation::{spring_with_options, SpringConfig, SpringOptions};
use rust_motion_rasterizer::{
    layout_text_box, render_to_ffmpeg_pipe, BlendMode, Color, FrameConfig, MaskMode, PipeConfig,
    RasterizerBackend, Scene, SceneFilter, SceneNode, SceneShadow, TextBox, TinySkiaBackend,
};
use std::time::Instant;

fn scene(frame: u32) -> Scene {
    let mut root_scene = Scene::new();

    // 1. Dark Cyberpunk background
    root_scene.push(SceneNode::Rect {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
        fill: Color::rgb(11, 13, 25), // #0b0d19
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    });

    // 2. Animated grid lines
    let grid_offset = ((frame * 2) % 60) as f32;
    for i in 0..12 {
        let y = 100.0 + i as f32 * 80.0 + grid_offset * 0.5;
        root_scene.push(SceneNode::Path {
            d: format!("M 100,{:.1} L 1820,{:.1}", y, y),
            fill: None,
            stroke: Some(Color::rgba(30, 45, 80, 120)),
            stroke_width: 1.0,
            opacity: 0.8,
        });
    }

    // 3. Horizontal Cyan accent laser line
    let laser_x = 200.0 + ((frame as f32 * 8.0) % 1520.0);
    root_scene.push(SceneNode::Path {
        d: "M 200,320 L 1720,320".into(),
        fill: None,
        stroke: Some(Color::rgb(0, 240, 255)),
        stroke_width: 3.0,
        opacity: 0.9,
    });
    root_scene.push(SceneNode::Rect {
        x: laser_x,
        y: 317.0,
        w: 40.0,
        h: 9.0,
        fill: Color::rgb(255, 255, 255),
        stroke: Some(Color::rgb(0, 240, 255)),
        stroke_width: 2.0,
        corner_radius: 2.0,
    });

    // 4. Spring-animated multi-line auto-fitted text inside offscreen Layer
    let progress = spring_with_options(
        (frame % 90) as f64,
        30.0,
        SpringConfig {
            damping: 12.0,
            mass: 1.0,
            stiffness: 100.0,
            overshoot_clamping: false,
        },
        SpringOptions {
            duration_in_frames: Some(45.0),
            delay: 0.0,
            ..Default::default()
        },
    )
    .unwrap_or(1.0) as f32;

    let text_x = 200.0 + (1.0 - progress) * -120.0;
    let title_text = "CYBERPUNK 2088\nNEON PROTOCOL";
    let title_req = TextBox::new(title_text, text_x, 380.0, 1400.0, 280.0, 68.0);

    let mut title_nodes = Vec::new();
    if let Ok(layout) = layout_text_box(&title_req) {
        for line in layout.lines {
            title_nodes.push(SceneNode::Text {
                x: line.x,
                y: line.y,
                content: line.text,
                font_size: layout.font_size,
                color: Color::WHITE,
                font_weight: 700,
                font_sources: Vec::new(),
            });
        }
    }

    // 5. Reproduce Remotion's three text layers: the white glyphs stay sharp,
    // while the cyan/magenta screen layers carry the chromatic shift.
    let glitch_chroma = if frame % 15 == 0 { 6.0 } else { 2.5 };
    let text_variant = |color: Color, dx: f32, dy: f32, shadow: Option<SceneShadow>| {
        let mut nodes = title_nodes.clone();
        for node in &mut nodes {
            if let SceneNode::Text {
                x,
                y,
                color: node_color,
                ..
            } = node
            {
                *x += dx;
                *y += dy;
                *node_color = color;
            }
        }
        SceneNode::Layer {
            opacity: 1.0,
            blend_mode: if shadow.is_some() {
                BlendMode::Normal
            } else {
                BlendMode::Screen
            },
            clip: None,
            mask: None,
            mask_mode: MaskMode::Alpha,
            filters: Vec::new(),
            shadow,
            children: nodes,
        }
    };
    root_scene.push(SceneNode::Layer {
        opacity: progress.clamp(0.0, 1.0),
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Brightness { amount: 1.2 }],
        shadow: None,
        children: vec![
            text_variant(Color::rgba(255, 0, 80, 179), glitch_chroma, 1.0, None),
            text_variant(Color::rgba(0, 240, 255, 179), -glitch_chroma, -1.0, None),
            text_variant(
                Color::WHITE,
                0.0,
                0.0,
                Some(SceneShadow {
                    offset_x: 0.0,
                    offset_y: 0.0,
                    blur_sigma: 14.0,
                    color: Color::rgba(255, 0, 128, 220),
                }),
            ),
        ],
    });

    // 6. Cyberpunk badge container with text
    root_scene.push(SceneNode::Rect {
        x: 200.0,
        y: 740.0,
        w: 360.0,
        h: 56.0,
        fill: Color::rgba(255, 0, 128, 45),
        stroke: Some(Color::rgb(255, 0, 128)),
        stroke_width: 2.0,
        corner_radius: 8.0,
    });
    root_scene.push(SceneNode::Text {
        x: 230.0,
        y: 778.0,
        content: "STATUS // ONLINE".into(),
        font_size: 24.0,
        color: Color::rgb(0, 240, 255),
        font_weight: 600,
        font_sources: Vec::new(),
    });

    // 7. Fullscreen Vignette Filter via Layer
    let full_layer = SceneNode::Layer {
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Vignette {
            offset: 0.25,
            darkness: 0.65,
            roundness: 0.85,
        }],
        shadow: None,
        children: root_scene.nodes,
    };

    let mut final_scene = Scene::new();
    final_scene.push(full_layer);
    final_scene
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
        .unwrap_or("target/cyberpunk.mp4");
    let concurrency: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4);

    let config = PipeConfig::new(1920, 1080, 30.0, 180, output_path)
        .with_concurrency(concurrency)
        .with_quality(18, "fast");

    let start = Instant::now();
    render_to_ffmpeg_pipe(&backend, &config, scene).expect("Render to ffmpeg pipe failed");
    let render_ms = start.elapsed().as_secs_f64() * 1000.0;

    println!(
        "{{\"render_ms\":{:.2},\"fps\":{:.2},\"concurrency\":{}}}",
        render_ms,
        180.0 / (render_ms / 1000.0),
        concurrency
    );
}
