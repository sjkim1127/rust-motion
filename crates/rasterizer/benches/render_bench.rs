//! Criterion benchmarks: CPU (`TinySkiaBackend`) vs GPU (`WgpuBackend`).
//!
//! Run all benchmarks:
//!   cargo bench -p dioxuscut-rasterizer --features gpu
//!
//! Run as unit-test smoke checks (no timing, just assert no panic):
//!   cargo bench -p dioxuscut-rasterizer --features gpu -- --test
//!
//! CPU-only (no GPU feature):
//!   cargo bench -p dioxuscut-rasterizer

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use dioxuscut_rasterizer::{
    backend::{FrameConfig, RasterizerBackend},
    scene::{Color, GradientStop, Scene, SceneNode},
    tiny_skia_backend::TinySkiaBackend,
};

// ─────────────────────────────────────────────────────────────────
// Scene factories
// ─────────────────────────────────────────────────────────────────

/// Minimal "hello world" — one gradient background + 3 rects.
fn scene_hello_world() -> Scene {
    Scene {
        nodes: vec![
            SceneNode::LinearGradient {
                x: 0.0,
                y: 0.0,
                w: 1920.0,
                h: 1080.0,
                angle_deg: 135.0,
                stops: vec![
                    GradientStop {
                        position: 0.0,
                        color: Color::rgb(20, 20, 40),
                    },
                    GradientStop {
                        position: 1.0,
                        color: Color::rgb(80, 20, 120),
                    },
                ],
            },
            SceneNode::Rect {
                x: 100.0,
                y: 200.0,
                w: 600.0,
                h: 80.0,
                fill: Color::rgb(255, 200, 50),
                stroke: Some(Color::WHITE),
                stroke_width: 3.0,
                corner_radius: 12.0,
            },
            SceneNode::Circle {
                cx: 960.0,
                cy: 540.0,
                r: 200.0,
                fill: Color::rgba(100, 180, 255, 180),
                stroke: Some(Color::WHITE),
                stroke_width: 4.0,
            },
        ],
    }
}

/// Dense grid of 25 × 14 rects — simulates a cyberpunk/tile layout.
fn scene_grid(cols: u32, rows: u32) -> Scene {
    let mut nodes = Vec::with_capacity((cols * rows + 1) as usize);
    nodes.push(SceneNode::Rect {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
        fill: Color::rgb(8, 8, 18),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    });
    let cell_w = 1920.0 / cols as f32;
    let cell_h = 1080.0 / rows as f32;
    for row in 0..rows {
        for col in 0..cols {
            let hue = (col * rows + row) as f32 / (cols * rows) as f32;
            let r = (255.0 * (1.0 - hue)) as u8;
            let g = (200.0 * hue) as u8;
            let b = 180u8;
            nodes.push(SceneNode::Rect {
                x: col as f32 * cell_w + 2.0,
                y: row as f32 * cell_h + 2.0,
                w: cell_w - 4.0,
                h: cell_h - 4.0,
                fill: Color::rgba(r, g, b, 210),
                stroke: Some(Color::rgba(255, 255, 255, 40)),
                stroke_width: 1.0,
                corner_radius: 4.0,
            });
        }
    }
    Scene { nodes }
}

/// Stacked radial and linear gradients.
fn scene_complex_gradients() -> Scene {
    let mut nodes = Vec::new();
    // Background linear gradient
    nodes.push(SceneNode::LinearGradient {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
        angle_deg: 45.0,
        stops: vec![
            GradientStop {
                position: 0.0,
                color: Color::rgb(5, 5, 20),
            },
            GradientStop {
                position: 0.5,
                color: Color::rgb(50, 10, 80),
            },
            GradientStop {
                position: 1.0,
                color: Color::rgb(10, 60, 100),
            },
        ],
    });
    // Multiple radial gradients
    for i in 0..8u32 {
        let cx = 240.0 * (i % 4 + 1) as f32;
        let cy = 360.0 * (i / 4 + 1) as f32;
        nodes.push(SceneNode::RadialGradient {
            cx,
            cy,
            r: 180.0,
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: Color::rgba(255, 200, 50, 200),
                },
                GradientStop {
                    position: 1.0,
                    color: Color::rgba(255, 50, 100, 0),
                },
            ],
        });
    }
    // Foreground circles
    for i in 0..6u32 {
        nodes.push(SceneNode::Circle {
            cx: 300.0 + i as f32 * 280.0,
            cy: 540.0,
            r: 100.0,
            fill: Color::rgba(80, 200, 255, 120),
            stroke: Some(Color::rgba(255, 255, 255, 80)),
            stroke_width: 2.0,
        });
    }
    Scene { nodes }
}

/// A representative browser/native media scene with a single image texture.
fn scene_image_fallback() -> Scene {
    Scene {
        nodes: vec![
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 1920.0,
                h: 1080.0,
                fill: Color::rgb(12, 12, 20),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            SceneNode::Image {
                src: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=".into(),
                x: 160.0,
                y: 120.0,
                w: 1600.0,
                h: 840.0,
                fit: dioxuscut_rasterizer::scene::ImageFit::Cover,
                opacity: 0.9,
            },
        ],
    }
}

#[allow(dead_code)]
fn scene_image_source(src: String) -> Scene {
    let mut scene = scene_image_fallback();
    if let Some(SceneNode::Image { src: image_src, .. }) = scene.nodes.get_mut(1) {
        *image_src = src;
    }
    scene
}

fn scene_text_heavy() -> Scene {
    let mut nodes = Vec::with_capacity(80);
    for index in 0..80u32 {
        nodes.push(SceneNode::Text {
            x: 40.0 + (index % 8) as f32 * 230.0,
            y: 70.0 + (index / 8) as f32 * 92.0,
            content: format!("Atlas text {:02} Remotion parity", index),
            font_size: 28.0,
            color: Color::rgba(230, 240, 255, 255),
            font_weight: 400,
            font_sources: Vec::new(),
        });
    }
    Scene { nodes }
}

#[allow(dead_code)]
fn scene_text_variant(variant: u32) -> Scene {
    let mut scene = scene_text_heavy();
    for (index, node) in scene.nodes.iter_mut().enumerate() {
        if let SceneNode::Text { content, .. } = node {
            *content = format!("Atlas text {variant:04} {index:02} 한글 日本語 العربية 😀");
        }
    }
    scene
}

// ─────────────────────────────────────────────────────────────────
// Bench groups
// ─────────────────────────────────────────────────────────────────

fn bench_cpu_scenes(c: &mut Criterion) {
    let backend = TinySkiaBackend::headless();

    let mut group = c.benchmark_group("cpu_scenes_1080p");
    group.sample_size(20);

    let scenes: &[(&str, Scene)] = &[
        ("hello_world", scene_hello_world()),
        ("grid_25x14", scene_grid(25, 14)),
        ("complex_gradients", scene_complex_gradients()),
        ("image_cpu_fallback", scene_image_fallback()),
    ];

    for (name, scene) in scenes {
        let config = FrameConfig::new(1920, 1080, 0, 30.0);
        group.bench_with_input(BenchmarkId::from_parameter(name), name, |b, _| {
            b.iter(|| backend.render_frame(scene, &config).unwrap())
        });
    }
    group.finish();
}

fn bench_cpu_text_atlas(c: &mut Criterion) {
    let backend = TinySkiaBackend::new();
    let scene = scene_text_heavy();
    let config = FrameConfig::new(1920, 1080, 0, 30.0);
    let mut group = c.benchmark_group("cpu_text_atlas_1080p");
    group.sample_size(20);
    group.bench_function("80_text_nodes", |b| {
        b.iter(|| backend.render_frame(&scene, &config).unwrap())
    });
    group.finish();
}

fn bench_cpu_resolutions(c: &mut Criterion) {
    let backend = TinySkiaBackend::headless();
    let scene = scene_hello_world();

    let mut group = c.benchmark_group("cpu_hello_world_resolutions");
    group.sample_size(15);

    let resolutions: &[(&str, u32, u32)] = &[
        ("480p", 854, 480),
        ("720p", 1280, 720),
        ("1080p", 1920, 1080),
        ("4k", 3840, 2160),
    ];

    for (label, w, h) in resolutions {
        let config = FrameConfig::new(*w, *h, 0, 30.0);
        group.bench_with_input(BenchmarkId::from_parameter(label), label, |b, _| {
            b.iter(|| backend.render_frame(&scene, &config).unwrap())
        });
    }
    group.finish();
}

#[cfg(feature = "gpu")]
fn bench_gpu_scenes(c: &mut Criterion) {
    use dioxuscut_rasterizer::wgpu_backend::WgpuBackend;

    let backend = match WgpuBackend::new() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("GPU backend unavailable, skipping GPU bench: {e}");
            return;
        }
    };

    let mut group = c.benchmark_group("gpu_scenes_1080p");
    group.sample_size(20);

    let scenes: &[(&str, Scene)] = &[
        ("hello_world", scene_hello_world()),
        ("grid_25x14", scene_grid(25, 14)),
        ("complex_gradients", scene_complex_gradients()),
        ("image_texture", scene_image_fallback()),
    ];

    for (name, scene) in scenes {
        let config = FrameConfig::new(1920, 1080, 0, 30.0);
        group.bench_with_input(BenchmarkId::from_parameter(name), name, |b, _| {
            b.iter(|| backend.render_frame(scene, &config).unwrap())
        });
    }
    group.finish();
}

#[cfg(feature = "gpu")]
fn bench_gpu_image_cache(c: &mut Criterion) {
    use dioxuscut_rasterizer::wgpu_backend::WgpuBackend;

    let Ok(warm_backend) = WgpuBackend::new() else {
        eprintln!("GPU backend unavailable, skipping image cache benchmark");
        return;
    };
    let Ok(cold_backend) = WgpuBackend::new() else {
        return;
    };
    let config = FrameConfig::new(1920, 1080, 0, 30.0);
    let base = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
    let warm_scene = scene_image_source(base.into());
    let cold_scenes: Vec<Scene> = (0..20)
        .map(|index| {
            scene_image_source(format!(
                "data:image/png;cache_key={index};base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="
            ))
        })
        .collect();

    let mut group = c.benchmark_group("gpu_image_cache_1080p");
    group.sample_size(10);
    group.bench_function("warm_cache_hit", |b| {
        b.iter(|| warm_backend.render_frame(&warm_scene, &config).unwrap())
    });
    let mut index = 0usize;
    group.bench_function("cold_unique_upload", |b| {
        b.iter(|| {
            let scene = &cold_scenes[index % cold_scenes.len()];
            index += 1;
            cold_backend.render_frame(scene, &config).unwrap()
        })
    });
    group.finish();
}

#[cfg(feature = "gpu")]
fn bench_gpu_video_frames(c: &mut Criterion) {
    use dioxuscut_rasterizer::wgpu_backend::WgpuBackend;

    if std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("FFmpeg unavailable, skipping GPU video benchmark");
        return;
    }
    let Ok(backend) = WgpuBackend::new() else {
        eprintln!("GPU backend unavailable, skipping GPU video benchmark");
        return;
    };
    let Ok(warm_backend) = WgpuBackend::new() else {
        eprintln!("GPU backend unavailable, skipping warm video benchmark");
        return;
    };
    let external_source =
        std::env::var_os("DIOXUSCUT_VIDEO_BENCH_SOURCE").map(std::path::PathBuf::from);
    let (source, temporary_dir) = if let Some(source) = external_source {
        if !source.is_file() {
            eprintln!(
                "Video benchmark source does not exist: {}",
                source.display()
            );
            return;
        }
        (source, None)
    } else {
        let dir =
            std::env::temp_dir().join(format!("dioxuscut-gpu-video-bench-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("gradient.mkv");
        let generated = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=64x64:rate=30:duration=2",
                "-an",
                "-c:v",
                "ffv1",
            ])
            .arg(&source)
            .status()
            .unwrap();
        if !generated.success() {
            let _ = std::fs::remove_dir_all(&dir);
            eprintln!("Could not create video benchmark input");
            return;
        }
        (source, Some(dir))
    };

    let mut group = c.benchmark_group("gpu_video_frames_1080p");
    group.sample_size(10);
    let warm_scene = Scene {
        nodes: vec![SceneNode::Video {
            src: source.display().to_string(),
            time: 0.0,
            looped: false,
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
            fit: dioxuscut_rasterizer::scene::ImageFit::Cover,
            opacity: 1.0,
        }],
    };
    let warm_config = FrameConfig::new(1920, 1080, 0, 30.0);
    warm_backend
        .render_frame(&warm_scene, &warm_config)
        .unwrap();
    group.bench_function("warm_cached_frame", |b| {
        b.iter(|| {
            warm_backend
                .render_frame(&warm_scene, &warm_config)
                .unwrap()
        })
    });
    let mut frame = 0u32;
    group.bench_function("native_decode_upload_30_frames", |b| {
        b.iter(|| {
            for _ in 0..30 {
                let scene = Scene {
                    nodes: vec![SceneNode::Video {
                        src: source.display().to_string(),
                        time: frame as f64 / 30.0,
                        looped: false,
                        x: 0.0,
                        y: 0.0,
                        w: 1920.0,
                        h: 1080.0,
                        fit: dioxuscut_rasterizer::scene::ImageFit::Cover,
                        opacity: 1.0,
                    }],
                };
                backend
                    .render_frame(&scene, &FrameConfig::new(1920, 1080, frame, 30.0))
                    .unwrap();
                frame = (frame + 1) % 60;
            }
        })
    });
    let stream_backend = WgpuBackend::new().expect("GPU backend initialized above");
    group.bench_function("native_stream_no_readback_30_frames", |b| {
        b.iter(|| {
            stream_backend
                .render_stream_gpu(
                    30,
                    &|frame| {
                        Ok(Scene {
                            nodes: vec![SceneNode::Video {
                                src: source.display().to_string(),
                                time: frame as f64 / 30.0,
                                looped: false,
                                x: 0.0,
                                y: 0.0,
                                w: 1920.0,
                                h: 1080.0,
                                fit: dioxuscut_rasterizer::scene::ImageFit::Cover,
                                opacity: 1.0,
                            }],
                        })
                    },
                    &|frame| FrameConfig::new(1920, 1080, frame, 30.0),
                    |_frame, _view, _width, _height| {},
                )
                .unwrap();
        })
    });
    group.finish();
    let cold_stats = backend.render_stats();
    let warm_stats = warm_backend.render_stats();
    let cold_timing = backend.video_timing_stats();
    let warm_timing = warm_backend.video_timing_stats();
    let stream_timing = stream_backend.video_timing_stats();
    eprintln!(
        "video benchmark stats: cold_sequence frames={} cache_hits={} cache_misses={} uploads={} upload_bytes={} decode_ms={:.3} upload_ms={:.3} gpu_readback_ms={:.3}; warm_frame frames={} cache_hits={} cache_misses={} uploads={} upload_bytes={} decode_ms={:.3} upload_ms={:.3} gpu_readback_ms={:.3}; native_stream_no_readback_ms={:.3}",
        cold_stats.gpu_frames,
        cold_stats.texture_cache_hits,
        cold_stats.texture_cache_misses,
        backend.gpu_texture_uploads(),
        backend.gpu_texture_upload_bytes(),
        cold_timing.video_decode_ns as f64 / 1_000_000.0,
        cold_timing.texture_upload_ns as f64 / 1_000_000.0,
        cold_timing.gpu_submit_readback_ns as f64 / 1_000_000.0,
        warm_stats.gpu_frames,
        warm_stats.texture_cache_hits,
        warm_stats.texture_cache_misses,
        warm_backend.gpu_texture_uploads(),
        warm_backend.gpu_texture_upload_bytes(),
        warm_timing.video_decode_ns as f64 / 1_000_000.0,
        warm_timing.texture_upload_ns as f64 / 1_000_000.0,
        warm_timing.gpu_submit_readback_ns as f64 / 1_000_000.0,
        stream_timing.gpu_submit_no_readback_ns as f64 / 1_000_000.0,
    );
    if let Some(dir) = temporary_dir {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(feature = "gpu")]
fn bench_gpu_resolutions(c: &mut Criterion) {
    use dioxuscut_rasterizer::wgpu_backend::WgpuBackend;

    let backend = match WgpuBackend::new() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("GPU backend unavailable, skipping GPU resolution bench: {e}");
            return;
        }
    };
    let scene = scene_hello_world();

    let mut group = c.benchmark_group("gpu_hello_world_resolutions");
    group.sample_size(15);

    let resolutions: &[(&str, u32, u32)] = &[
        ("480p", 854, 480),
        ("720p", 1280, 720),
        ("1080p", 1920, 1080),
        ("4k", 3840, 2160),
    ];

    for (label, w, h) in resolutions {
        let config = FrameConfig::new(*w, *h, 0, 30.0);
        group.bench_with_input(BenchmarkId::from_parameter(label), label, |b, _| {
            b.iter(|| backend.render_frame(&scene, &config).unwrap())
        });
    }
    group.finish();
}

#[cfg(feature = "gpu")]
fn bench_gpu_streaming(c: &mut Criterion) {
    use dioxuscut_rasterizer::wgpu_backend::WgpuBackend;

    let backend = match WgpuBackend::new() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("GPU backend unavailable, skipping GPU streaming bench: {e}");
            return;
        }
    };

    let mut group = c.benchmark_group("gpu_streaming_throughput_1080p");
    group.sample_size(15);

    let frame_count = 10u32;
    let scene = scene_grid(25, 14);

    group.bench_function("sequential_render_frame_10_frames", |b| {
        b.iter(|| {
            for f in 0..frame_count {
                let config = FrameConfig::new(1920, 1080, f, 30.0);
                let _ = backend.render_frame(&scene, &config).unwrap();
            }
        })
    });

    group.bench_function("pipelined_render_stream_10_frames", |b| {
        b.iter(|| {
            backend
                .render_stream(
                    frame_count,
                    &|_f| Ok(scene.clone()),
                    &|f| FrameConfig::new(1920, 1080, f, 30.0),
                    &mut |_f, _rgba: &[u8]| Ok(()),
                )
                .unwrap();
        })
    });

    group.bench_function("native_render_stream_gpu_10_frames_no_readback", |b| {
        b.iter(|| {
            backend
                .render_stream_gpu(
                    frame_count,
                    &|_f| Ok(scene.clone()),
                    &|f| FrameConfig::new(1920, 1080, f, 30.0),
                    |_f, _view, _width, _height| {},
                )
                .unwrap();
        })
    });

    group.finish();
}

#[cfg(feature = "gpu")]
fn bench_gpu_concurrent_resolutions(c: &mut Criterion) {
    use dioxuscut_rasterizer::wgpu_backend::WgpuBackend;
    use std::sync::Arc;

    let backend = match WgpuBackend::new() {
        Ok(b) => Arc::new(b),
        Err(e) => {
            eprintln!("GPU backend unavailable, skipping concurrent resolution bench: {e}");
            return;
        }
    };
    let scene = Arc::new(scene_hello_world());
    let mut group = c.benchmark_group("gpu_concurrent_resolutions");
    group.sample_size(15);
    group.bench_function("720p_plus_1080p", |b| {
        b.iter(|| {
            let left_backend = Arc::clone(&backend);
            let right_backend = Arc::clone(&backend);
            let left_scene = Arc::clone(&scene);
            let right_scene = Arc::clone(&scene);
            let left = std::thread::spawn(move || {
                left_backend
                    .render_frame(&left_scene, &FrameConfig::new(1280, 720, 0, 30.0))
                    .unwrap()
            });
            let right = std::thread::spawn(move || {
                right_backend
                    .render_frame(&right_scene, &FrameConfig::new(1920, 1080, 0, 30.0))
                    .unwrap()
            });
            left.join().unwrap();
            right.join().unwrap();
        })
    });
    group.finish();
}

#[cfg(feature = "gpu")]
fn bench_gpu_text_atlas(c: &mut Criterion) {
    use dioxuscut_rasterizer::wgpu_backend::WgpuBackend;
    use std::time::Instant;
    let backend = match WgpuBackend::new() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("GPU backend unavailable, skipping text atlas bench: {e}");
            return;
        }
    };
    let scene = scene_text_heavy();
    let config = FrameConfig::new(1920, 1080, 0, 30.0);
    let cold_backend = WgpuBackend::new().expect("GPU backend initialized above");
    let cold_started = Instant::now();
    cold_backend.render_frame(&scene, &config).unwrap();
    eprintln!(
        "text atlas cold frame: elapsed_ms={:.3} full_uploads={} full_bytes={} dirty_uploads={} dirty_bytes={} hits={} misses={}",
        cold_started.elapsed().as_secs_f64() * 1000.0,
        cold_backend.text_atlas_full_uploads(),
        cold_backend.text_atlas_full_upload_bytes(),
        cold_backend.text_atlas_dirty_uploads(),
        cold_backend.text_atlas_dirty_upload_bytes(),
        cold_backend.text_atlas_cache_hits(),
        cold_backend.text_atlas_cache_misses(),
    );
    let mut group = c.benchmark_group("gpu_text_atlas_1080p");
    group.sample_size(15);
    group.bench_function("80_text_nodes_atlas_reuse", |b| {
        b.iter(|| backend.render_frame(&scene, &config).unwrap())
    });
    group.bench_function("native_stream_no_readback_80_text_nodes", |b| {
        b.iter(|| {
            backend
                .render_stream_gpu(
                    10,
                    &|_frame| Ok(scene.clone()),
                    &|frame| FrameConfig::new(1920, 1080, frame, 30.0),
                    |_frame, _view, _width, _height| {},
                )
                .unwrap();
        })
    });
    let mut variant = 0u32;
    group.bench_function("80_text_nodes_dirty_rect_churn", |b| {
        b.iter(|| {
            variant = variant.wrapping_add(1);
            let scene = scene_text_variant(variant);
            backend.render_frame(&scene, &config).unwrap()
        })
    });
    eprintln!(
        "text atlas warm stats: upload_bytes={} full_uploads={} full_bytes={} dirty_uploads={} dirty_bytes={} hits={} misses={}",
        backend.text_atlas_upload_bytes(),
        backend.text_atlas_full_uploads(),
        backend.text_atlas_full_upload_bytes(),
        backend.text_atlas_dirty_uploads(),
        backend.text_atlas_dirty_upload_bytes(),
        backend.text_atlas_cache_hits(),
        backend.text_atlas_cache_misses(),
    );
    group.finish();
}

#[cfg(feature = "gpu")]
fn bench_gpu_lottie(c: &mut Criterion) {
    use dioxuscut_rasterizer::gif_cache::LoopBehavior;
    use dioxuscut_rasterizer::wgpu_backend::WgpuBackend;
    use std::time::Instant;

    let source = std::env::temp_dir().join(format!(
        "dioxuscut-bench-lottie-{}.json",
        std::process::id()
    ));
    std::fs::write(
        &source,
        r#"{"v":"5.5.7","fr":30,"ip":0,"op":30,"w":64,"h":64,"ddd":0,"assets":[],"layers":[{"ddd":0,"ind":1,"ty":4,"nm":"shape","sr":1,"ks":{"o":{"a":0,"k":100},"r":{"a":0,"k":0},"p":{"a":0,"k":[32,32,0]},"a":{"a":0,"k":[0,0,0]},"s":{"a":0,"k":[100,100,100]}},"ao":0,"shapes":[{"ty":"el","p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[40,40]}},{"ty":"fl","c":{"a":0,"k":[1,0.2,0.05,1]},"o":{"a":0,"k":100},"r":1}],"ip":0,"op":30,"st":0,"bm":0}]}"#,
    )
    .expect("write Lottie benchmark fixture");
    let backend = match WgpuBackend::new() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("GPU backend unavailable, skipping Lottie bench: {e}");
            let _ = std::fs::remove_file(&source);
            return;
        }
    };
    let make_scene = |time: f64| Scene {
        nodes: vec![SceneNode::Lottie {
            src: source.display().to_string(),
            time,
            x: 0.0,
            y: 0.0,
            w: 64.0,
            h: 64.0,
            playback_rate: 1.0,
            loop_behavior: LoopBehavior::Loop,
            opacity: 1.0,
        }],
    };
    let config = FrameConfig::new(1920, 1080, 0, 30.0);
    let cold_scene = make_scene(0.0);
    let cold_started = Instant::now();
    backend.render_frame(&cold_scene, &config).unwrap();
    eprintln!(
        "lottie cold frame: elapsed_ms={:.3} uploads={} upload_bytes={} cache_hits={} cache_misses={}",
        cold_started.elapsed().as_secs_f64() * 1000.0,
        backend.gpu_texture_uploads(),
        backend.gpu_texture_upload_bytes(),
        backend.gpu_texture_cache_hits(),
        backend.gpu_texture_cache_misses(),
    );
    let mut group = c.benchmark_group("gpu_lottie_1080p");
    group.sample_size(15);
    group.bench_function("single_frame_warm_reuse", |b| {
        b.iter(|| backend.render_frame(&cold_scene, &config).unwrap())
    });
    let mut frame = 0u32;
    group.bench_function("30_frame_sequence", |b| {
        b.iter(|| {
            frame = (frame + 1) % 30;
            backend
                .render_frame(
                    &make_scene(f64::from(frame) / 30.0),
                    &FrameConfig::new(1920, 1080, frame, 30.0),
                )
                .unwrap()
        })
    });
    eprintln!(
        "lottie warm stats: uploads={} upload_bytes={} cache_hits={} cache_misses={}",
        backend.gpu_texture_uploads(),
        backend.gpu_texture_upload_bytes(),
        backend.gpu_texture_cache_hits(),
        backend.gpu_texture_cache_misses(),
    );
    group.finish();
    let _ = std::fs::remove_file(source);
}

// ─────────────────────────────────────────────────────────────────
// Entry points
// ─────────────────────────────────────────────────────────────────

#[cfg(not(feature = "gpu"))]
criterion_group!(
    benches,
    bench_cpu_scenes,
    bench_cpu_resolutions,
    bench_cpu_text_atlas
);

#[cfg(feature = "gpu")]
criterion_group!(
    benches,
    bench_cpu_scenes,
    bench_cpu_resolutions,
    bench_cpu_text_atlas,
    bench_gpu_scenes,
    bench_gpu_image_cache,
    bench_gpu_video_frames,
    bench_gpu_resolutions,
    bench_gpu_streaming,
    bench_gpu_concurrent_resolutions,
    bench_gpu_text_atlas,
    bench_gpu_lottie
);

criterion_main!(benches);
