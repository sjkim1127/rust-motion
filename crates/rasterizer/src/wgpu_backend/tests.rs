use super::*;
use crate::backend::{FrameConfig, RasterizerBackend};
use crate::scene::{Color, GradientStop, ImageFit, Scene, SceneNode};

#[test]
fn test_wgpu_backend_init() {
    match WgpuBackend::new() {
        Ok(backend) => {
            println!("GPU backend initialised successfully");
            // Render a minimal 64x64 scene
            let scene = crate::scene::Scene::new();
            let config = FrameConfig::new(64, 64, 0, 30.0);
            let img = backend
                .render_frame(&scene, &config)
                .expect("GPU render failed");
            assert_eq!(img.width(), 64);
            assert_eq!(img.height(), 64);
        }
        Err(e) => {
            // In CI / headless without GPU this is expected
            println!("GPU backend unavailable (expected in headless CI): {e}");
        }
    }
}

#[test]
fn offscreen_layer_pool_reuses_resolution_slots() {
    let Ok(backend) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping offscreen pool test");
        return;
    };
    let first = backend.offscreen_layer_slot(64, 32);
    let second = backend.offscreen_layer_slot(64, 32);
    let different = backend.offscreen_layer_slot(32, 32);
    assert!(Arc::ptr_eq(&first, &second));
    assert!(!Arc::ptr_eq(&first, &different));
    let slot = first.lock().expect("offscreen layer slot lock poisoned");
    let _binding = backend.offscreen_layer_binding(&slot);
    let destination = GpuFrameSlot::new(&backend.ctx.device, 64, 32, false);
    let submission = backend
        .composite_external_texture(&slot, &destination, 64, 32, 0.5)
        .expect("offscreen composite submission failed");
    backend
        .ctx
        .device
        .poll(wgpu::Maintain::wait_for(submission));
}

#[test]
fn gpu_plain_rects_match_cpu_pixels_exactly() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping exact plain-rect parity test");
        return;
    };
    let scene = Scene {
        nodes: vec![
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 128.0,
                h: 96.0,
                fill: Color::rgb(15, 23, 42),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            SceneNode::Rect {
                x: 16.0,
                y: 12.0,
                w: 48.0,
                h: 32.0,
                fill: Color::rgb(80, 160, 220),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
        ],
    };
    let config = FrameConfig::new(128, 96, 0, 30.0);
    let cpu = TinySkiaBackend::headless()
        .render_frame(&scene, &config)
        .expect("CPU render failed");
    let gpu = gpu
        .render_frame(&scene, &config)
        .expect("GPU render failed");
    assert_eq!(gpu.as_raw(), cpu.as_raw());
}

#[test]
fn gpu_disjoint_texture_layer_matches_cpu_pixels() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping disjoint texture parity test");
        return;
    };
    let red_path = std::env::temp_dir().join(format!(
        "rust-motion-disjoint-red-{}.png",
        std::process::id()
    ));
    let blue_path = std::env::temp_dir().join(format!(
        "rust-motion-disjoint-blue-{}.png",
        std::process::id()
    ));
    image::RgbaImage::from_pixel(2, 2, image::Rgba([240, 32, 24, 255]))
        .save(&red_path)
        .unwrap();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([24, 64, 240, 255]))
        .save(&blue_path)
        .unwrap();
    let scene = Scene {
        nodes: vec![
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 64.0,
                h: 32.0,
                fill: Color::rgb(12, 18, 28),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            SceneNode::Layer {
                opacity: 0.5,
                blend_mode: crate::scene::BlendMode::Normal,
                clip: None,
                mask: None,
                mask_mode: crate::scene::MaskMode::Alpha,
                filters: Vec::new(),
                shadow: None,
                children: vec![
                    SceneNode::Group {
                        transform: crate::scene::Transform2D::translate(4.0, 4.0),
                        opacity: 1.0,
                        children: vec![SceneNode::Image {
                            src: red_path.to_string_lossy().into_owned(),
                            x: 0.0,
                            y: 0.0,
                            w: 20.0,
                            h: 20.0,
                            fit: ImageFit::Fill,
                            opacity: 1.0,
                        }],
                    },
                    SceneNode::Group {
                        transform: crate::scene::Transform2D::translate(36.0, 4.0),
                        opacity: 1.0,
                        children: vec![SceneNode::Image {
                            src: blue_path.to_string_lossy().into_owned(),
                            x: 0.0,
                            y: 0.0,
                            w: 20.0,
                            h: 20.0,
                            fit: ImageFit::Fill,
                            opacity: 1.0,
                        }],
                    },
                ],
            },
        ],
    };
    let config = FrameConfig::new(64, 32, 0, 30.0);
    assert!(gpu_supports_scene(&scene));
    let cpu = TinySkiaBackend::headless()
        .render_frame(&scene, &config)
        .expect("CPU render failed");
    let gpu_image = gpu
        .render_frame(&scene, &config)
        .expect("GPU render failed");
    assert_eq!(gpu_image.as_raw(), cpu.as_raw());
    let _ = std::fs::remove_file(red_path);
    let _ = std::fs::remove_file(blue_path);
}

#[test]
fn gpu_overlapping_texture_layer_composites_on_gpu() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping overlapping texture parity test");
        return;
    };
    let red_path = std::env::temp_dir().join(format!(
        "rust-motion-overlap-red-{}.png",
        std::process::id()
    ));
    let blue_path = std::env::temp_dir().join(format!(
        "rust-motion-overlap-blue-{}.png",
        std::process::id()
    ));
    image::RgbaImage::from_pixel(2, 2, image::Rgba([240, 32, 24, 255]))
        .save(&red_path)
        .unwrap();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([24, 64, 240, 255]))
        .save(&blue_path)
        .unwrap();
    let scene = Scene {
        nodes: vec![
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 64.0,
                h: 32.0,
                fill: Color::rgb(12, 18, 28),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            SceneNode::Layer {
                opacity: 0.5,
                blend_mode: crate::scene::BlendMode::Normal,
                clip: None,
                mask: None,
                mask_mode: crate::scene::MaskMode::Alpha,
                filters: vec![crate::scene::SceneFilter::Opacity { amount: 0.8 }],
                shadow: None,
                children: vec![
                    SceneNode::Group {
                        transform: crate::scene::Transform2D::translate(4.0, 4.0),
                        opacity: 1.0,
                        children: vec![SceneNode::Image {
                            src: red_path.to_string_lossy().into_owned(),
                            x: 0.0,
                            y: 0.0,
                            w: 20.0,
                            h: 20.0,
                            fit: ImageFit::Fill,
                            opacity: 1.0,
                        }],
                    },
                    SceneNode::Group {
                        transform: crate::scene::Transform2D::translate(12.0, 4.0),
                        opacity: 1.0,
                        children: vec![SceneNode::Image {
                            src: blue_path.to_string_lossy().into_owned(),
                            x: 0.0,
                            y: 0.0,
                            w: 20.0,
                            h: 20.0,
                            fit: ImageFit::Fill,
                            opacity: 1.0,
                        }],
                    },
                ],
            },
        ],
    };
    let config = FrameConfig::new(64, 32, 0, 30.0);
    let cpu = TinySkiaBackend::headless()
        .render_frame(&scene, &config)
        .expect("CPU render failed");
    let gpu_image = gpu
        .render_frame(&scene, &config)
        .expect("GPU render failed");
    assert_eq!(gpu_image.as_raw(), cpu.as_raw());
    let stats = gpu.render_stats();
    assert_eq!(stats.gpu_frames, 1);
    assert_eq!(stats.cpu_fallback_frames, 0);
    let _ = std::fs::remove_file(red_path);
    let _ = std::fs::remove_file(blue_path);
}

#[test]
fn render_stats_distinguish_gpu_and_cpu_fallback_frames() {
    let Ok(backend) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping render stats test");
        return;
    };
    let config = FrameConfig::new(64, 64, 0, 30.0);
    let gpu_scene = Scene {
        nodes: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 64.0,
            h: 64.0,
            fill: Color::WHITE,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    };
    // Keep the asset inline so the texture upload test is hermetic.
    let fallback_scene = Scene {
            nodes: vec![SceneNode::Image {
                src: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=".into(),
                x: 0.0,
                y: 0.0,
                w: 64.0,
                h: 64.0,
                fit: crate::scene::ImageFit::Fill,
                opacity: 1.0,
            }],
        };
    backend.render_frame(&gpu_scene, &config).unwrap();
    backend.render_frame(&fallback_scene, &config).unwrap();
    assert_eq!(
        backend.render_stats(),
        WgpuRenderStats {
            gpu_frames: 2,
            cpu_fallback_frames: 0,
            texture_cache_hits: 0,
            texture_cache_misses: 1,
        }
    );
    assert!(backend.render_stats().cpu_fallback_ratio().abs() < f64::EPSILON);
}

#[test]
fn gpu_image_matches_cpu_for_fill_and_preserves_draw_order() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping image parity test");
        return;
    };
    let scene = Scene {
            nodes: vec![
                SceneNode::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 32.0,
                    h: 32.0,
                    fill: Color::rgb(0, 0, 255),
                    stroke: None,
                    stroke_width: 0.0,
                    corner_radius: 0.0,
                },
                SceneNode::Image {
                    src: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=".into(),
                    x: 8.0,
                    y: 8.0,
                    w: 16.0,
                    h: 16.0,
                    fit: ImageFit::Fill,
                    opacity: 0.5,
                },
            ],
        };
    let config = FrameConfig::new(32, 32, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let _second_gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let mut equivalent_source_scene = scene.clone();
    if let SceneNode::Image { src, .. } = &mut equivalent_source_scene.nodes[1] {
        *src = format!("  {src}  ");
    }
    let _third_gpu_image = gpu.render_frame(&equivalent_source_scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::headless()
        .render_frame(&scene, &config)
        .unwrap();
    assert_eq!(gpu.gpu_image_cache_len(), 1);
    assert_eq!(gpu.gpu_texture_uploads(), 1);
    assert_eq!(gpu.gpu_texture_upload_bytes(), 4);
    assert!(gpu_image.get_pixel(16, 16)[3] > 0);
    assert_eq!(gpu_image.get_pixel(2, 2), cpu_image.get_pixel(2, 2));
    let mean_error = gpu_image
        .pixels()
        .zip(cpu_image.pixels())
        .map(|(gpu, cpu)| {
            (0..4)
                .map(|channel| {
                    (i16::from(gpu[channel]) - i16::from(cpu[channel])).unsigned_abs() as u64
                })
                .sum::<u64>()
        })
        .sum::<u64>() as f64
        / (config.width * config.height * 4) as f64;
    assert!(
        mean_error < 12.0,
        "GPU/CPU image mean error was {mean_error}"
    );
}

#[test]
fn gpu_image_fits_match_cpu_for_all_modes() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping image fit parity test");
        return;
    };
    let path =
        std::env::temp_dir().join(format!("rust-motion-image-fit-{}.png", std::process::id()));
    let mut source = image::RgbaImage::new(2, 1);
    source.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
    source.put_pixel(1, 0, image::Rgba([0, 0, 255, 255]));
    source.save(&path).unwrap();
    let fits = [
        ImageFit::Cover,
        ImageFit::Contain,
        ImageFit::Fill,
        ImageFit::None,
        ImageFit::ScaleDown,
    ];
    let config = FrameConfig::new(64, 64, 0, 30.0);
    for fit in fits {
        let scene = Scene {
            nodes: vec![SceneNode::Image {
                src: path.to_string_lossy().into_owned(),
                x: 8.0,
                y: 8.0,
                w: 48.0,
                h: 48.0,
                fit,
                opacity: 1.0,
            }],
        };
        let gpu_image = gpu.render_frame(&scene, &config).unwrap();
        let cpu_image = TinySkiaBackend::new()
            .render_frame(&scene, &config)
            .unwrap();
        let mean_error: f64 = gpu_image
            .pixels()
            .zip(cpu_image.pixels())
            .map(|(a, b)| {
                (0..4)
                    .map(|channel| {
                        (i16::from(a[channel]) - i16::from(b[channel])).unsigned_abs() as u64
                    })
                    .sum::<u64>()
            })
            .sum::<u64>() as f64
            / (64 * 64 * 4) as f64;
        assert!(
            mean_error < 18.0,
            "GPU/CPU {fit:?} mean error was {mean_error}"
        );
    }
    // `none` must keep the natural image size even when it is larger than
    // the destination box. This exercises target clipping and catches a
    // top-left crop masquerading as a centered natural-size draw.
    let oversized_path = std::env::temp_dir().join(format!(
        "rust-motion-image-fit-oversized-{}.png",
        std::process::id()
    ));
    let mut oversized = image::RgbaImage::new(80, 40);
    for (x, y, pixel) in oversized.enumerate_pixels_mut() {
        *pixel = if x < 40 && y < 20 {
            image::Rgba([255, 0, 0, 255])
        } else if x >= 40 && y < 20 {
            image::Rgba([0, 255, 0, 255])
        } else if x < 40 {
            image::Rgba([0, 0, 255, 255])
        } else {
            image::Rgba([255, 255, 0, 255])
        };
    }
    oversized.save(&oversized_path).unwrap();
    let oversized_scene = Scene {
        nodes: vec![SceneNode::Image {
            src: oversized_path.to_string_lossy().into_owned(),
            x: 8.0,
            y: 8.0,
            w: 48.0,
            h: 24.0,
            fit: ImageFit::None,
            opacity: 1.0,
        }],
    };
    let gpu_image = gpu.render_frame(&oversized_scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&oversized_scene, &config)
        .unwrap();
    let mean_error: f64 = gpu_image
        .pixels()
        .zip(cpu_image.pixels())
        .map(|(a, b)| {
            (0..4)
                .map(|channel| {
                    (i16::from(a[channel]) - i16::from(b[channel])).unsigned_abs() as u64
                })
                .sum::<u64>()
        })
        .sum::<u64>() as f64
        / (64 * 64 * 4) as f64;
    assert!(
        mean_error < 18.0,
        "GPU/CPU oversized none error was {mean_error}"
    );
    let _ = std::fs::remove_file(oversized_path);
    let _ = std::fs::remove_file(path);
}

#[test]
fn gpu_image_transform_and_opacity_match_cpu() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping transformed image parity test");
        return;
    };
    let path = std::env::temp_dir().join(format!(
        "rust-motion-image-transform-{}.png",
        std::process::id()
    ));
    let mut source = image::RgbaImage::new(2, 2);
    for pixel in source.pixels_mut() {
        *pixel = image::Rgba([255, 32, 64, 255]);
    }
    source.save(&path).unwrap();
    let scene = Scene {
        nodes: vec![
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 64.0,
                h: 64.0,
                fill: Color::rgb(10, 20, 30),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            SceneNode::Group {
                transform: crate::scene::Transform2D::rotate(17.0).with_translate(12.0, 8.0),
                opacity: 0.55,
                children: vec![SceneNode::Image {
                    src: path.to_string_lossy().into_owned(),
                    x: 12.0,
                    y: 12.0,
                    w: 32.0,
                    h: 24.0,
                    fit: ImageFit::Contain,
                    opacity: 0.8,
                }],
            },
        ],
    };
    let config = FrameConfig::new(64, 64, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    let mean_error: f64 = gpu_image
        .pixels()
        .zip(cpu_image.pixels())
        .map(|(a, b)| {
            (0..4)
                .map(|channel| {
                    (i16::from(a[channel]) - i16::from(b[channel])).unsigned_abs() as u64
                })
                .sum::<u64>()
        })
        .sum::<u64>() as f64
        / (64 * 64 * 4) as f64;
    assert!(
        mean_error < 18.0,
        "GPU/CPU transformed image mean error was {mean_error}"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn gpu_text_uses_gpu_texture_path() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping text GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Text {
            x: 4.0,
            y: 24.0,
            content: "GPU text".into(),
            font_size: 18.0,
            color: Color::WHITE,
            font_weight: 400,
            font_sources: Vec::new(),
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let image = gpu
        .render_frame(&scene, &FrameConfig::new(96, 32, 0, 30.0))
        .unwrap();
    let cpu = TinySkiaBackend::new()
        .render_frame(&scene, &FrameConfig::new(96, 32, 0, 30.0))
        .unwrap();
    let generation = gpu.text_atlas_cache_generation();
    let first_upload_bytes = gpu.text_atlas_upload_bytes();
    let same = gpu
        .render_frame(&scene, &FrameConfig::new(96, 32, 0, 30.0))
        .unwrap();
    assert_eq!(gpu.text_atlas_cache_generation(), generation);
    assert_eq!(gpu.text_atlas_upload_bytes(), first_upload_bytes);
    assert_eq!(
        same.pixels().collect::<Vec<_>>(),
        image.pixels().collect::<Vec<_>>(),
        "reusing an unchanged atlas must preserve the rendered pixels"
    );
    let updated_scene = Scene {
        nodes: vec![SceneNode::Text {
            x: 4.0,
            y: 24.0,
            content: "atlas update".into(),
            font_size: 18.0,
            color: Color::WHITE,
            font_weight: 400,
            font_sources: Vec::new(),
        }],
    };
    let second = gpu
        .render_frame(&updated_scene, &FrameConfig::new(96, 32, 1, 30.0))
        .unwrap();
    assert_eq!(gpu.render_stats().gpu_frames, 3);
    assert_eq!(gpu.text_atlas_full_uploads(), 1);
    assert!(gpu.text_atlas_dirty_uploads() >= 1);
    assert_eq!(
        gpu.text_atlas_upload_bytes(),
        gpu.text_atlas_full_upload_bytes() + gpu.text_atlas_dirty_upload_bytes()
    );
    assert!(gpu.text_atlas_dirty_upload_bytes() < gpu.text_atlas_full_upload_bytes());
    assert!(image.pixels().any(|pixel| pixel[3] > 0));
    assert!(second.pixels().any(|pixel| pixel[3] > 0));
    assert!(gpu.text_atlas_cache_generation().unwrap() > generation.unwrap());
    let second_upload_bytes = gpu.text_atlas_upload_bytes();
    assert!(second_upload_bytes > first_upload_bytes);
    assert!(second_upload_bytes - first_upload_bytes < 2048 * 2048);
    let alpha_error: u64 = image
        .pixels()
        .zip(cpu.pixels())
        .map(|(gpu, cpu)| (i16::from(gpu[3]) - i16::from(cpu[3])).unsigned_abs() as u64)
        .sum();
    let mean_alpha_error = alpha_error as f64 / (96 * 32) as f64;
    assert!(
        mean_alpha_error < 12.0,
        "GPU/CPU text alpha mean error was {mean_alpha_error}"
    );
}

#[test]
fn gpu_text_unicode_and_weight_use_distinct_atlas_entries() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping Unicode text test");
        return;
    };
    let regular = Scene {
        nodes: vec![SceneNode::Text {
            x: 4.0,
            y: 30.0,
            content: "한글 日本語 العربية 😀".into(),
            font_size: 20.0,
            color: Color::WHITE,
            font_weight: 400,
            font_sources: Vec::new(),
        }],
    };
    let bold = Scene {
        nodes: vec![SceneNode::Text {
            x: 4.0,
            y: 30.0,
            content: "한글 日本語 العربية 😀".into(),
            font_size: 20.0,
            color: Color::WHITE,
            font_weight: 700,
            font_sources: Vec::new(),
        }],
    };
    let config = FrameConfig::new(192, 48, 0, 30.0);
    let regular_gpu = gpu.render_frame(&regular, &config).unwrap();
    let regular_cpu = TinySkiaBackend::new()
        .render_frame(&regular, &config)
        .unwrap();
    let bold_gpu = gpu.render_frame(&bold, &config).unwrap();
    let bold_cpu = TinySkiaBackend::new().render_frame(&bold, &config).unwrap();
    assert!(regular_gpu.pixels().any(|pixel| pixel[3] > 0));
    assert!(bold_gpu.pixels().any(|pixel| pixel[3] > 0));
    for (gpu_image, cpu_image) in [(&regular_gpu, &regular_cpu), (&bold_gpu, &bold_cpu)] {
        let mean_alpha_error = gpu_image
            .pixels()
            .zip(cpu_image.pixels())
            .map(|(gpu, cpu)| (i16::from(gpu[3]) - i16::from(cpu[3])).unsigned_abs() as u64)
            .sum::<u64>() as f64
            / (192 * 48) as f64;
        assert!(
            mean_alpha_error < 14.0,
            "Unicode alpha error: {mean_alpha_error}"
        );
    }
    assert_ne!(
        regular_gpu.pixels().collect::<Vec<_>>(),
        bold_gpu.pixels().collect::<Vec<_>>()
    );
    assert!(gpu.text_atlas_upload_bytes() > 0);
}

#[test]
fn gpu_vignette_filter_matches_cpu_layer_falloff() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping vignette GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 1.0,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: vec![crate::scene::SceneFilter::Vignette {
                offset: 0.1,
                darkness: 0.8,
                roundness: 0.5,
            }],
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 64.0,
                h: 64.0,
                fill: Color::rgba(220, 120, 40, 255),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(64, 64, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    let center = gpu_image.get_pixel(32, 32);
    let corner = gpu_image.get_pixel(1, 1);
    assert!(center[0] > corner[0]);
    let cpu_center = cpu_image.get_pixel(32, 32);
    let cpu_corner = cpu_image.get_pixel(1, 1);
    let attenuation_error: f64 = (0..3)
        .map(|channel| {
            let gpu_ratio = f64::from(corner[channel]) / f64::from(center[channel].max(1));
            let cpu_ratio = f64::from(cpu_corner[channel]) / f64::from(cpu_center[channel].max(1));
            (gpu_ratio - cpu_ratio).abs()
        })
        .sum::<f64>()
        / 3.0;
    assert!(
        attenuation_error < 0.03,
        "GPU/CPU vignette attenuation error was {attenuation_error}"
    );
}

#[test]
fn gpu_multiply_layer_uses_blend_pipeline_and_linear_cpu_reference() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping multiply GPU test");
        return;
    };
    let mut scene = Scene {
        nodes: vec![
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 16.0,
                h: 16.0,
                fill: Color::rgb(128, 96, 64),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            SceneNode::Layer {
                opacity: 0.5,
                blend_mode: crate::scene::BlendMode::Multiply,
                clip: None,
                mask: None,
                mask_mode: crate::scene::MaskMode::Alpha,
                filters: vec![],
                shadow: None,
                children: vec![SceneNode::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 16.0,
                    h: 16.0,
                    fill: Color::rgb(64, 160, 192),
                    stroke: None,
                    stroke_width: 0.0,
                    corner_radius: 0.0,
                }],
            },
        ],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(16, 16, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    let gpu_pixel = gpu_image.get_pixel(8, 8);
    let cpu_pixel = cpu_image.get_pixel(8, 8);
    for channel in 0..3 {
        let expected = f32::from(cpu_pixel[channel]);
        assert!(
            (f32::from(gpu_pixel[channel]) - expected).abs() < 4.0,
            "channel {channel}: GPU {:?}, CPU {:?}, expected linear {expected}",
            gpu_pixel,
            cpu_pixel
        );
    }
    assert_eq!(gpu.render_stats().cpu_fallback_frames, 0);

    let set_blend_mode = |scene: &mut Scene, mode| {
        let SceneNode::Layer { blend_mode, .. } = &mut scene.nodes[1] else {
            unreachable!("blend test scene lost its layer");
        };
        *blend_mode = mode;
    };
    set_blend_mode(&mut scene, crate::scene::BlendMode::Screen);
    assert!(gpu_supports_scene(&scene));
    let screen_image = gpu.render_frame(&scene, &config).unwrap();
    assert_eq!(gpu.render_stats().cpu_fallback_frames, 0);
    assert_ne!(screen_image.get_pixel(8, 8), gpu_pixel);

    set_blend_mode(&mut scene, crate::scene::BlendMode::Darken);
    let darken_image = gpu.render_frame(&scene, &config).unwrap();
    set_blend_mode(&mut scene, crate::scene::BlendMode::Lighten);
    let lighten_image = gpu.render_frame(&scene, &config).unwrap();
    assert_eq!(gpu.render_stats().cpu_fallback_frames, 0);
    assert_ne!(darken_image.get_pixel(8, 8), lighten_image.get_pixel(8, 8));

    let SceneNode::Layer {
        blend_mode,
        mask,
        mask_mode,
        filters,
        ..
    } = &mut scene.nodes[1]
    else {
        unreachable!("blend test scene lost its layer");
    };
    *blend_mode = crate::scene::BlendMode::Multiply;
    *mask_mode = crate::scene::MaskMode::Luminance;
    *filters = vec![crate::scene::SceneFilter::Opacity { amount: 0.75 }];
    *mask = Some(vec![
        SceneNode::Rect {
            x: 2.0,
            y: 2.0,
            w: 4.0,
            h: 12.0,
            fill: Color::rgb(128, 128, 128),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        },
        SceneNode::Rect {
            x: 10.0,
            y: 2.0,
            w: 4.0,
            h: 12.0,
            fill: Color::WHITE,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        },
    ]);
    assert!(gpu_supports_scene(&scene));
    let gpu_masked = gpu.render_frame(&scene, &config).unwrap();
    let cpu_masked = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    assert_eq!(gpu.render_stats().cpu_fallback_frames, 0);
    for &(x, y) in &[(4, 8), (11, 8), (8, 8), (1, 1)] {
        for channel in 0..4 {
            assert!(
                (i16::from(gpu_masked.get_pixel(x, y)[channel])
                    - i16::from(cpu_masked.get_pixel(x, y)[channel]))
                .abs()
                    <= 5,
                "masked blend mismatch at ({x},{y}) channel {channel}: GPU {:?}, CPU {:?}",
                gpu_masked.get_pixel(x, y),
                cpu_masked.get_pixel(x, y)
            );
        }
    }
}

#[test]
fn gpu_invert_filter_uses_gpu_layer_path() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping invert GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 1.0,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: vec![crate::scene::SceneFilter::Invert { amount: 1.0 }],
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 16.0,
                h: 16.0,
                fill: Color::rgb(255, 0, 0),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let image = gpu
        .render_frame(&scene, &FrameConfig::new(16, 16, 0, 30.0))
        .unwrap();
    let pixel = image.get_pixel(8, 8);
    eprintln!("invert pixel={pixel:?}");
    assert!(pixel[0] < 8 && pixel[1] > 247 && pixel[2] > 247);
    assert_eq!(gpu.render_stats().cpu_fallback_frames, 0);
}

#[test]
fn gpu_hue_rotate_filter_uses_gpu_layer_path() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping hue rotate GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 1.0,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: vec![crate::scene::SceneFilter::HueRotate { degrees: 120.0 }],
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 16.0,
                h: 16.0,
                fill: Color::rgb(255, 0, 0),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let image = gpu
        .render_frame(&scene, &FrameConfig::new(16, 16, 0, 30.0))
        .unwrap();
    let pixel = image.get_pixel(8, 8);
    assert!(
        pixel[0] < 8 && pixel[1] > 247 && pixel[2] < 8,
        "pixel={pixel:?}"
    );
    assert_eq!(gpu.render_stats().cpu_fallback_frames, 0);
}

#[test]
fn gpu_tint_filter_matches_cpu_for_opaque_rect() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping tint GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 1.0,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: vec![crate::scene::SceneFilter::Tint {
                color: [0, 0, 255, 255],
                amount: 0.5,
            }],
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 16.0,
                h: 16.0,
                fill: Color::rgb(255, 0, 0),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(16, 16, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    let gpu_pixel = gpu_image.get_pixel(8, 8);
    let cpu_pixel = cpu_image.get_pixel(8, 8);
    for channel in 0..4 {
        assert!(
            (i32::from(gpu_pixel[channel]) - i32::from(cpu_pixel[channel])).abs() <= 2,
            "channel {channel}: GPU {:?}, CPU {:?}",
            gpu_pixel,
            cpu_pixel
        );
    }
    assert_eq!(gpu.render_stats().cpu_fallback_frames, 0);
}

#[test]
fn gpu_duotone_filter_matches_cpu_for_opaque_rect() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping duotone GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 1.0,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: vec![crate::scene::SceneFilter::Duotone {
                primary: [0, 0, 0, 255],
                secondary: [255, 255, 255, 255],
            }],
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 16.0,
                h: 16.0,
                fill: Color::rgb(255, 0, 0),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(16, 16, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    let gpu_pixel = gpu_image.get_pixel(8, 8);
    let cpu_pixel = cpu_image.get_pixel(8, 8);
    for channel in 0..4 {
        assert!(
            (i32::from(gpu_pixel[channel]) - i32::from(cpu_pixel[channel])).abs() <= 2,
            "channel {channel}: GPU {:?}, CPU {:?}",
            gpu_pixel,
            cpu_pixel
        );
    }
    assert_eq!(gpu.render_stats().cpu_fallback_frames, 0);
}

#[test]
fn gpu_color_grading_filter_matches_cpu_for_opaque_rect() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping color grading GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 1.0,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: vec![crate::scene::SceneFilter::ColorGrading {
                contrast: 1.2,
                saturation: 0.7,
                gamma: 1.3,
                tint: Some([20, 40, 80, 64]),
            }],
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 16.0,
                h: 16.0,
                fill: Color::rgb(200, 80, 30),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(16, 16, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    let gpu_pixel = gpu_image.get_pixel(8, 8);
    let cpu_pixel = cpu_image.get_pixel(8, 8);
    for channel in 0..4 {
        assert!(
            (i32::from(gpu_pixel[channel]) - i32::from(cpu_pixel[channel])).abs() <= 2,
            "channel {channel}: GPU {:?}, CPU {:?}",
            gpu_pixel,
            cpu_pixel
        );
    }
    assert_eq!(gpu.render_stats().cpu_fallback_frames, 0);
}

#[test]
fn gpu_invert_filter_chain_composes_amounts_in_order() {
    let filters = [
        crate::scene::SceneFilter::Invert { amount: 0.25 },
        crate::scene::SceneFilter::Invert { amount: 0.5 },
    ];
    let (_, _, _, _, _, amount, _) = gpu_layer_effects(&filters, 1.0).unwrap();
    assert!((amount - 0.5).abs() < f32::EPSILON);
}

#[test]
fn gpu_non_normal_blends_accept_opacity_only_filters() {
    assert!(gpu_blend_layer_filters_supported(&[]));
    assert!(gpu_blend_layer_filters_supported(&[
        crate::scene::SceneFilter::Opacity { amount: 0.5 },
    ]));
    assert!(!gpu_blend_layer_filters_supported(&[
        crate::scene::SceneFilter::Brightness { amount: 1.1 },
    ]));
}

#[test]
fn gpu_color_grading_does_not_claim_mixed_color_filter_order() {
    let filters = [
        crate::scene::SceneFilter::Brightness { amount: 1.2 },
        crate::scene::SceneFilter::ColorGrading {
            contrast: 1.1,
            saturation: 0.9,
            gamma: 1.2,
            tint: None,
        },
    ];
    assert!(gpu_layer_effects(&filters, 1.0).is_none());
}

#[test]
fn gpu_image_multiply_layer_matches_cpu_for_srgb_texture_and_alpha() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping image blend test");
        return;
    };
    let image_src = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
    let scene = Scene {
        nodes: vec![
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 16.0,
                h: 16.0,
                fill: Color::rgb(180, 120, 80),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            SceneNode::Layer {
                opacity: 0.5,
                blend_mode: crate::scene::BlendMode::Multiply,
                clip: None,
                mask: None,
                mask_mode: crate::scene::MaskMode::Alpha,
                filters: vec![],
                shadow: None,
                children: vec![SceneNode::Image {
                    src: image_src.into(),
                    x: 0.0,
                    y: 0.0,
                    w: 16.0,
                    h: 16.0,
                    fit: crate::scene::ImageFit::Fill,
                    opacity: 1.0,
                }],
            },
        ],
    };
    assert!(!gpu_supports_scene(&scene));
    let config = FrameConfig::new(16, 16, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    let gpu_pixel = gpu_image.get_pixel(8, 8);
    let cpu_pixel = cpu_image.get_pixel(8, 8);
    assert_eq!(gpu_pixel, cpu_pixel);
    assert_eq!(gpu.render_stats().cpu_fallback_frames, 1);
    assert_eq!(
        gpu.last_cpu_fallback_reason().as_deref(),
        Some("scene contains GPU-unsupported nodes or effects")
    );
}

#[test]
fn gpu_native_text_stream_reuses_atlas_without_readback() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping native text stream test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Text {
            x: 8.0,
            y: 32.0,
            content: "native atlas stream".into(),
            font_size: 24.0,
            color: Color::WHITE,
            font_weight: 400,
            font_sources: Vec::new(),
        }],
    };
    let mut delivered = Vec::new();
    gpu.render_stream_gpu(
        3,
        &|_frame| Ok(scene.clone()),
        &|frame| FrameConfig::new(128, 48, frame, 30.0),
        |frame, _view, width, height| delivered.push((frame, width, height)),
    )
    .unwrap();
    assert_eq!(delivered, vec![(0, 128, 48), (1, 128, 48), (2, 128, 48)]);
    assert_eq!(gpu.render_stats().gpu_frames, 3);
    assert!(gpu.text_atlas_upload_bytes() > 0);
    let upload_bytes = gpu.text_atlas_upload_bytes();
    assert_eq!(gpu.text_atlas_cache_misses(), 1);
    assert!(gpu.text_atlas_cache_hits() >= 2);
    gpu.render_stream_gpu(
        3,
        &|_frame| Ok(scene.clone()),
        &|frame| FrameConfig::new(128, 48, frame, 30.0),
        |_frame, _view, _width, _height| {},
    )
    .unwrap();
    assert_eq!(gpu.text_atlas_upload_bytes(), upload_bytes);
    assert!(gpu.text_atlas_cache_hits() >= 5);
}

#[test]
fn gpu_lottie_frame_is_uploaded_and_composited_as_a_texture() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping Lottie GPU test");
        return;
    };
    let dir = std::env::temp_dir().join(format!("rust-motion-wgpu-lottie-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let source = dir.join("pulse.json");
    std::fs::write(
            &source,
            r#"{"v":"5.5.7","fr":30,"ip":0,"op":30,"w":32,"h":32,"ddd":0,"assets":[],"layers":[{"ddd":0,"ind":1,"ty":4,"nm":"shape","sr":1,"ks":{"o":{"a":0,"k":100},"r":{"a":0,"k":0},"p":{"a":0,"k":[16,16,0]},"a":{"a":0,"k":[0,0,0]},"s":{"a":0,"k":[100,100,100]}},"ao":0,"shapes":[{"ty":"el","p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[20,20]}},{"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100},"r":1}],"ip":0,"op":30,"st":0,"bm":0}]}"#,
        )
        .unwrap();
    let scene = Scene {
        nodes: vec![SceneNode::Lottie {
            src: source.display().to_string(),
            time: 0.0,
            x: 0.0,
            y: 0.0,
            w: 32.0,
            h: 32.0,
            playback_rate: 1.0,
            loop_behavior: crate::gif_cache::LoopBehavior::Loop,
            opacity: 1.0,
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let image = gpu
        .render_frame(&scene, &FrameConfig::new(32, 32, 0, 30.0))
        .unwrap();
    let cpu = TinySkiaBackend::new()
        .render_frame(&scene, &FrameConfig::new(32, 32, 0, 30.0))
        .unwrap();
    assert!(image.pixels().any(|pixel| pixel[3] > 0));
    assert_eq!(gpu.gpu_texture_uploads(), 1);
    let mean_error = image
        .pixels()
        .zip(cpu.pixels())
        .map(|(gpu, cpu)| {
            (0..4)
                .map(|channel| {
                    (i16::from(gpu[channel]) - i16::from(cpu[channel])).unsigned_abs() as u64
                })
                .sum::<u64>()
        })
        .sum::<u64>() as f64
        / (32 * 32 * 4) as f64;
    assert!(
        mean_error < 18.0,
        "Lottie GPU/CPU mean error was {mean_error}"
    );
    let second = gpu
        .render_frame(&scene, &FrameConfig::new(32, 32, 0, 30.0))
        .unwrap();
    assert_eq!(gpu.gpu_texture_uploads(), 1);
    assert_eq!(
        image.pixels().collect::<Vec<_>>(),
        second.pixels().collect::<Vec<_>>()
    );
    let mut next_frame = scene.clone();
    let SceneNode::Lottie { time, .. } = &mut next_frame.nodes[0] else {
        unreachable!("Lottie scene lost its root node");
    };
    *time = 1.0 / 30.0;
    let next = gpu
        .render_frame(&next_frame, &FrameConfig::new(32, 32, 1, 30.0))
        .unwrap();
    assert!(next.pixels().any(|pixel| pixel[3] > 0));
    assert_eq!(gpu.gpu_texture_uploads(), 2);
    let composited = Scene {
        nodes: vec![
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 32.0,
                h: 32.0,
                fill: Color::rgb(0, 0, 255),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            SceneNode::Layer {
                opacity: 0.5,
                blend_mode: crate::scene::BlendMode::Normal,
                clip: None,
                mask: None,
                mask_mode: crate::scene::MaskMode::Alpha,
                filters: vec![crate::scene::SceneFilter::Brightness { amount: 0.8 }],
                shadow: None,
                children: vec![SceneNode::Lottie {
                    src: source.display().to_string(),
                    time: 0.0,
                    x: 0.0,
                    y: 0.0,
                    w: 32.0,
                    h: 32.0,
                    playback_rate: 1.0,
                    loop_behavior: crate::gif_cache::LoopBehavior::Loop,
                    opacity: 1.0,
                }],
            },
        ],
    };
    let composited_gpu = gpu
        .render_frame(&composited, &FrameConfig::new(32, 32, 0, 30.0))
        .unwrap();
    let composited_cpu = TinySkiaBackend::new()
        .render_frame(&composited, &FrameConfig::new(32, 32, 0, 30.0))
        .unwrap();
    let composite_error = composited_gpu
        .pixels()
        .zip(composited_cpu.pixels())
        .map(|(gpu, cpu)| {
            (0..4)
                .map(|channel| {
                    (i16::from(gpu[channel]) - i16::from(cpu[channel])).unsigned_abs() as u64
                })
                .sum::<u64>()
        })
        .sum::<u64>() as f64
        / (32 * 32 * 4) as f64;
    assert!(
        composite_error < 18.0,
        "Lottie layer opacity/draw-order mean error was {composite_error}"
    );
    assert!(composited_gpu.get_pixel(16, 16)[3] > 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn gpu_brightness_filter_matches_cpu_for_normal_layer() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping brightness GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 32.0,
                h: 32.0,
                fill: Color::BLACK,
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            SceneNode::Layer {
                opacity: 1.0,
                blend_mode: crate::scene::BlendMode::Normal,
                clip: None,
                mask: None,
                mask_mode: crate::scene::MaskMode::Alpha,
                filters: vec![crate::scene::SceneFilter::Brightness { amount: 0.5 }],
                shadow: None,
                children: vec![SceneNode::Rect {
                    x: 8.0,
                    y: 8.0,
                    w: 16.0,
                    h: 16.0,
                    fill: Color::WHITE,
                    stroke: None,
                    stroke_width: 0.0,
                    corner_radius: 0.0,
                }],
            },
        ],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(32, 32, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    let mean_error: f64 = gpu_image
        .pixels()
        .zip(cpu_image.pixels())
        .map(|(a, b)| {
            (0..4)
                .map(|channel| {
                    (i16::from(a[channel]) - i16::from(b[channel])).unsigned_abs() as u64
                })
                .sum::<u64>()
        })
        .sum::<u64>() as f64
        / (32 * 32 * 4) as f64;
    assert!(
        mean_error < 2.0,
        "GPU/CPU brightness mean error was {mean_error}"
    );
}

#[test]
fn gpu_grayscale_and_contrast_filters_match_cpu_for_normal_layer() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping grayscale GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 1.0,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: vec![
                crate::scene::SceneFilter::Grayscale { amount: 1.0 },
                crate::scene::SceneFilter::Contrast { factor: 1.5 },
            ],
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 8.0,
                y: 8.0,
                w: 16.0,
                h: 16.0,
                fill: Color::rgb(220, 40, 80),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(32, 32, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    let gpu_center = gpu_image.get_pixel(16, 16);
    let cpu_center = cpu_image.get_pixel(16, 16);
    assert_eq!(gpu_center[0], gpu_center[1]);
    assert_eq!(gpu_center[1], gpu_center[2]);
    assert!(
        (i16::from(gpu_center[0]) - i16::from(cpu_center[0])).abs() <= 2,
        "GPU/CPU grayscale center mismatch: {gpu_center:?} vs {cpu_center:?}"
    );
}

#[test]
fn gpu_saturation_filter_matches_cpu_for_normal_layer() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping saturation GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 1.0,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: vec![crate::scene::SceneFilter::Saturation { factor: 0.5 }],
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 8.0,
                y: 8.0,
                w: 16.0,
                h: 16.0,
                fill: Color::rgb(220, 40, 80),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(32, 32, 0, 30.0);
    let gpu_center = gpu
        .render_frame(&scene, &config)
        .unwrap()
        .get_pixel(16, 16)
        .to_owned();
    let cpu_center = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap()
        .get_pixel(16, 16)
        .to_owned();
    for channel in 0..3 {
        assert!(
            (i16::from(gpu_center[channel]) - i16::from(cpu_center[channel])).abs() <= 2,
            "GPU/CPU saturation channel {channel} mismatch: {gpu_center:?} vs {cpu_center:?}"
        );
    }
}

#[test]
fn gpu_opaque_alpha_rect_mask_matches_cpu_layer_clip() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping alpha mask GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 32.0,
                h: 32.0,
                fill: Color::rgb(0, 0, 255),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            SceneNode::Layer {
                opacity: 1.0,
                blend_mode: crate::scene::BlendMode::Normal,
                clip: None,
                mask: Some(vec![
                    SceneNode::Rect {
                        x: 4.0,
                        y: 0.0,
                        w: 6.0,
                        h: 32.0,
                        fill: Color::WHITE,
                        stroke: None,
                        stroke_width: 0.0,
                        corner_radius: 0.0,
                    },
                    SceneNode::Rect {
                        x: 22.0,
                        y: 0.0,
                        w: 6.0,
                        h: 32.0,
                        fill: Color::WHITE,
                        stroke: None,
                        stroke_width: 0.0,
                        corner_radius: 0.0,
                    },
                ]),
                mask_mode: crate::scene::MaskMode::Alpha,
                filters: Vec::new(),
                shadow: None,
                children: vec![SceneNode::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 32.0,
                    h: 32.0,
                    fill: Color::rgb(255, 0, 0),
                    stroke: None,
                    stroke_width: 0.0,
                    corner_radius: 0.0,
                }],
            },
        ],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(32, 32, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    let mean_error: f64 = gpu_image
        .pixels()
        .zip(cpu_image.pixels())
        .map(|(a, b)| {
            (0..4)
                .map(|channel| {
                    (i16::from(a[channel]) - i16::from(b[channel])).unsigned_abs() as u64
                })
                .sum::<u64>()
        })
        .sum::<u64>() as f64
        / (32 * 32 * 4) as f64;
    assert!(
        mean_error < 2.0,
        "GPU/CPU alpha mask mean error was {mean_error}"
    );
    assert_eq!(gpu_image.get_pixel(7, 16), cpu_image.get_pixel(7, 16));
    assert_eq!(gpu_image.get_pixel(16, 16), cpu_image.get_pixel(16, 16));
}

#[test]
fn gpu_grayscale_luminance_rect_mask_matches_cpu() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping luminance mask GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 1.0,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: None,
            mask: Some(vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 32.0,
                h: 32.0,
                fill: Color::rgb(128, 128, 128),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }]),
            mask_mode: crate::scene::MaskMode::Luminance,
            filters: Vec::new(),
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 32.0,
                h: 32.0,
                fill: Color::rgb(255, 0, 0),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(32, 32, 0, 30.0);
    let gpu_pixel = gpu
        .render_frame(&scene, &config)
        .unwrap()
        .get_pixel(16, 16)
        .to_owned();
    let cpu_pixel = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap()
        .get_pixel(16, 16)
        .to_owned();
    for channel in 0..4 {
        assert!(
            (i16::from(gpu_pixel[channel]) - i16::from(cpu_pixel[channel])).abs() <= 2,
            "GPU/CPU luminance mask mismatch: {gpu_pixel:?} vs {cpu_pixel:?}"
        );
    }
}

#[test]
fn gpu_translucent_alpha_rect_mask_matches_cpu() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping translucent mask GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 32.0,
                h: 32.0,
                fill: Color::rgb(0, 0, 255),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            SceneNode::Layer {
                opacity: 1.0,
                blend_mode: crate::scene::BlendMode::Normal,
                clip: None,
                mask: Some(vec![SceneNode::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 32.0,
                    h: 32.0,
                    fill: Color::rgba(255, 255, 255, 128),
                    stroke: None,
                    stroke_width: 0.0,
                    corner_radius: 0.0,
                }]),
                mask_mode: crate::scene::MaskMode::Alpha,
                filters: Vec::new(),
                shadow: None,
                children: vec![SceneNode::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 32.0,
                    h: 32.0,
                    fill: Color::rgb(255, 0, 0),
                    stroke: None,
                    stroke_width: 0.0,
                    corner_radius: 0.0,
                }],
            },
        ],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(32, 32, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    for channel in 0..4 {
        assert!(
            (i16::from(gpu_image.get_pixel(16, 16)[channel])
                - i16::from(cpu_image.get_pixel(16, 16)[channel]))
            .abs()
                <= 2,
            "GPU/CPU translucent mask mismatch: {:?} vs {:?}",
            gpu_image.get_pixel(16, 16),
            cpu_image.get_pixel(16, 16)
        );
    }
}

#[test]
fn gpu_circle_alpha_mask_matches_cpu_at_stable_pixels() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping circle mask GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 1.0,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: None,
            mask: Some(vec![SceneNode::Circle {
                cx: 16.0,
                cy: 16.0,
                r: 10.0,
                fill: Color::WHITE,
                stroke: None,
                stroke_width: 0.0,
            }]),
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: Vec::new(),
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 32.0,
                h: 32.0,
                fill: Color::rgb(255, 0, 0),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(32, 32, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    for (x, y) in [(16, 16), (1, 1)] {
        assert_eq!(gpu_image.get_pixel(x, y), cpu_image.get_pixel(x, y));
    }
}

#[test]
fn gpu_rounded_rect_alpha_mask_matches_cpu_at_stable_pixels() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping rounded mask GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 1.0,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: None,
            mask: Some(vec![SceneNode::Rect {
                x: 4.0,
                y: 4.0,
                w: 24.0,
                h: 24.0,
                fill: Color::WHITE,
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 6.0,
            }]),
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: Vec::new(),
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 32.0,
                h: 32.0,
                fill: Color::rgb(255, 0, 0),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(32, 32, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    for (x, y) in [(16, 16), (1, 1)] {
        assert_eq!(gpu_image.get_pixel(x, y), cpu_image.get_pixel(x, y));
    }
}

#[test]
fn gpu_layer_clip_rect_matches_cpu_at_stable_pixels() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping layer clip GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 1.0,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: Some(crate::scene::ClipRegion::Rect {
                x: 4.0,
                y: 4.0,
                w: 24.0,
                h: 24.0,
                corner_radius: 0.0,
            }),
            mask: None,
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: Vec::new(),
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 32.0,
                h: 32.0,
                fill: Color::rgb(255, 0, 0),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(32, 32, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    assert_eq!(gpu_image.get_pixel(16, 16), cpu_image.get_pixel(16, 16));
    assert_eq!(gpu_image.get_pixel(1, 1), cpu_image.get_pixel(1, 1));
}

#[test]
fn gpu_path_clip_uses_intermediate_mask_texture() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping path clip GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 1.0,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: Some(crate::scene::ClipRegion::Path {
                d: "M 4 4 L 28 4 L 16 28 Z".into(),
            }),
            mask: None,
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: Vec::new(),
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 32.0,
                h: 32.0,
                fill: Color::rgb(255, 0, 0),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(32, 32, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    for (x, y) in [(16, 10), (16, 20), (2, 2)] {
        assert_eq!(gpu_image.get_pixel(x, y), cpu_image.get_pixel(x, y));
    }
}

#[test]
fn gpu_path_alpha_mask_uses_intermediate_mask_texture() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping path alpha mask GPU test");
        return;
    };
    let mut scene = Scene {
        nodes: vec![
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 32.0,
                h: 32.0,
                fill: Color::rgb(40, 80, 160),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            SceneNode::Layer {
                opacity: 1.0,
                blend_mode: crate::scene::BlendMode::Multiply,
                clip: None,
                mask: Some(vec![SceneNode::Path {
                    d: "M 4 4 L 28 4 L 16 28 Z".into(),
                    fill: Some(Color::rgb(128, 128, 128)),
                    stroke: None,
                    stroke_width: 0.0,
                    opacity: 1.0,
                }]),
                mask_mode: crate::scene::MaskMode::Luminance,
                filters: Vec::new(),
                shadow: None,
                children: vec![SceneNode::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 32.0,
                    h: 32.0,
                    fill: Color::rgb(255, 0, 0),
                    stroke: None,
                    stroke_width: 0.0,
                    corner_radius: 0.0,
                }],
            },
        ],
    };
    let config = FrameConfig::new(32, 32, 0, 30.0);
    for blend_mode in [crate::scene::BlendMode::Multiply] {
        let SceneNode::Layer {
            blend_mode: mode, ..
        } = &mut scene.nodes[1]
        else {
            unreachable!("path mask test lost its layer");
        };
        *mode = blend_mode;
        assert!(gpu_supports_scene(&scene));
        let gpu_image = gpu.render_frame(&scene, &config).unwrap();
        let cpu_image = TinySkiaBackend::new()
            .render_frame(&scene, &config)
            .unwrap();
        for (x, y) in [(16, 10), (16, 20), (2, 2)] {
            for channel in 0..4 {
                assert!(
                        (i16::from(gpu_image.get_pixel(x, y)[channel])
                            - i16::from(cpu_image.get_pixel(x, y)[channel]))
                        .abs()
                            <= 5,
                        "GPU/CPU path luminance mask mismatch for {blend_mode:?} at ({x},{y}) channel {channel}: {:?} vs {:?}",
                        gpu_image.get_pixel(x, y),
                        cpu_image.get_pixel(x, y)
                    );
            }
        }
        assert_eq!(gpu.render_stats().cpu_fallback_frames, 0);
    }
}

#[test]
fn gpu_path_stroke_mask_uses_intermediate_mask_texture() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping path stroke mask GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 32.0,
                h: 32.0,
                fill: Color::rgb(40, 80, 160),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            SceneNode::Layer {
                opacity: 1.0,
                blend_mode: crate::scene::BlendMode::Multiply,
                clip: None,
                mask: Some(vec![SceneNode::Path {
                    d: "M 4 16 L 28 16".into(),
                    fill: None,
                    stroke: Some(Color::rgb(128, 128, 128)),
                    stroke_width: 4.0,
                    opacity: 1.0,
                }]),
                mask_mode: crate::scene::MaskMode::Luminance,
                filters: Vec::new(),
                shadow: None,
                children: vec![SceneNode::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 32.0,
                    h: 32.0,
                    fill: Color::rgb(255, 0, 0),
                    stroke: None,
                    stroke_width: 0.0,
                    corner_radius: 0.0,
                }],
            },
        ],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(32, 32, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    for (x, y) in [(16, 16), (16, 10), (2, 2)] {
        for channel in 0..4 {
            assert!(
                    (i16::from(gpu_image.get_pixel(x, y)[channel])
                        - i16::from(cpu_image.get_pixel(x, y)[channel]))
                    .abs()
                        <= 5,
                    "GPU/CPU path stroke luminance mismatch at ({x},{y}) channel {channel}: {:?} vs {:?}",
                    gpu_image.get_pixel(x, y),
                    cpu_image.get_pixel(x, y)
                );
        }
    }
    assert_eq!(gpu.render_stats().cpu_fallback_frames, 0);
}

#[test]
fn gpu_layer_clip_and_mask_use_intersection() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping combined clip/mask GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 1.0,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: Some(crate::scene::ClipRegion::Rect {
                x: 4.0,
                y: 4.0,
                w: 24.0,
                h: 24.0,
                corner_radius: 0.0,
            }),
            mask: Some(vec![SceneNode::Rect {
                x: 8.0,
                y: 8.0,
                w: 16.0,
                h: 16.0,
                fill: Color::WHITE,
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }]),
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: Vec::new(),
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 32.0,
                h: 32.0,
                fill: Color::rgb(255, 0, 0),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(32, 32, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    for (x, y) in [(16, 16), (6, 6), (1, 1)] {
        assert_eq!(gpu_image.get_pixel(x, y), cpu_image.get_pixel(x, y));
    }
}

#[test]
fn gpu_four_stop_linear_alpha_mask_matches_cpu() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping gradient mask GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 32.0,
                h: 32.0,
                fill: Color::rgb(40, 80, 160),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            SceneNode::Layer {
                opacity: 1.0,
                blend_mode: crate::scene::BlendMode::Multiply,
                clip: None,
                mask: Some(vec![SceneNode::LinearGradient {
                    x: 0.0,
                    y: 0.0,
                    w: 32.0,
                    h: 32.0,
                    angle_deg: 45.0,
                    stops: vec![
                        crate::scene::GradientStop {
                            position: 0.0,
                            color: Color::rgba(255, 255, 255, 0),
                        },
                        crate::scene::GradientStop {
                            position: 0.5,
                            color: Color::WHITE,
                        },
                        crate::scene::GradientStop {
                            position: 0.75,
                            color: Color::rgba(255, 255, 255, 128),
                        },
                        crate::scene::GradientStop {
                            position: 1.0,
                            color: Color::rgba(255, 255, 255, 0),
                        },
                    ],
                }]),
                mask_mode: crate::scene::MaskMode::Luminance,
                filters: Vec::new(),
                shadow: None,
                children: vec![SceneNode::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 32.0,
                    h: 32.0,
                    fill: Color::rgb(255, 0, 0),
                    stroke: None,
                    stroke_width: 0.0,
                    corner_radius: 0.0,
                }],
            },
        ],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(32, 32, 0, 30.0);
    let gpu_pixel = gpu
        .render_frame(&scene, &config)
        .unwrap()
        .get_pixel(24, 16)
        .to_owned();
    let cpu_pixel = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap()
        .get_pixel(24, 16)
        .to_owned();
    for channel in 0..4 {
        assert!(
            (i16::from(gpu_pixel[channel]) - i16::from(cpu_pixel[channel])).abs() <= 3,
            "GPU/CPU gradient mask mismatch: {gpu_pixel:?} vs {cpu_pixel:?}"
        );
    }
}

#[test]
fn gpu_two_stop_radial_alpha_mask_matches_cpu() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping radial mask GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 1.0,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: None,
            mask: Some(vec![SceneNode::RadialGradient {
                cx: 16.0,
                cy: 16.0,
                r: 12.0,
                stops: vec![
                    crate::scene::GradientStop {
                        position: 0.0,
                        color: Color::WHITE,
                    },
                    crate::scene::GradientStop {
                        position: 1.0,
                        color: Color::rgba(255, 255, 255, 0),
                    },
                ],
            }]),
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: Vec::new(),
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 32.0,
                h: 32.0,
                fill: Color::rgb(255, 0, 0),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let config = FrameConfig::new(32, 32, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    for (x, y) in [(16, 16), (16, 20), (1, 1)] {
        for channel in 0..4 {
            assert!(
                (i16::from(gpu_image.get_pixel(x, y)[channel])
                    - i16::from(cpu_image.get_pixel(x, y)[channel]))
                .abs()
                    <= 3,
                "GPU/CPU radial mask mismatch at ({x},{y}): {:?} vs {:?}",
                gpu_image.get_pixel(x, y),
                cpu_image.get_pixel(x, y)
            );
        }
    }
}

#[test]
fn gpu_fullscreen_shader_uses_wgpu_shader_runner() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping shader GPU test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Shader {
            x: 0.0,
            y: 0.0,
            w: 32.0,
            h: 16.0,
            source: "return vec4<f32>(uv.x, uv.y, 0.25, 1.0);".into(),
            time: 0.0,
            params: [1.0, 1.0, 1.0, 1.0],
            opacity: 0.5,
        }],
    };
    let image = gpu
        .render_frame(&scene, &FrameConfig::new(32, 16, 0, 30.0))
        .unwrap();
    assert_eq!(gpu.render_stats().gpu_frames, 1);
    assert!(image.get_pixel(1, 1)[2] > 0);
    assert!((120..=136).contains(&image.get_pixel(1, 1)[3]));
    assert_ne!(image.get_pixel(1, 1), image.get_pixel(30, 14));
}

#[test]
fn gpu_shader_layers_preserve_rects_opacity_and_draw_order() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping shader layer test");
        return;
    };
    let scene = Scene {
        nodes: vec![
            SceneNode::Shader {
                x: 2.0,
                y: 1.0,
                w: 8.0,
                h: 6.0,
                source: "return vec4<f32>(1.0, 0.0, 0.0, 1.0);".into(),
                time: 0.0,
                params: [0.0; 4],
                opacity: 1.0,
            },
            SceneNode::Shader {
                x: 5.0,
                y: 3.0,
                w: 4.0,
                h: 2.0,
                source: "return vec4<f32>(0.0, 0.0, 1.0, 1.0);".into(),
                time: 0.0,
                params: [0.0; 4],
                opacity: 0.5,
            },
        ],
    };
    let image = gpu
        .render_frame(&scene, &FrameConfig::new(16, 10, 0, 30.0))
        .unwrap();
    assert_eq!(gpu.render_stats().gpu_frames, 1);
    assert_eq!(image.get_pixel(0, 0)[3], 0);
    assert_eq!(image.get_pixel(3, 2)[0], 255);
    let overlap = image.get_pixel(6, 4);
    assert!(
        overlap[0] > 80 && overlap[2] > 80,
        "overlap was {overlap:?}"
    );
    assert_eq!(image.get_pixel(10, 4)[3], 0);
}

#[test]
fn gpu_shader_region_outside_target_uses_compatibility_path() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping shader bounds test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Shader {
            x: 14.0,
            y: 2.0,
            w: 8.0,
            h: 4.0,
            source: "return vec4<f32>(1.0, 0.0, 0.0, 1.0);".into(),
            time: 0.0,
            params: [0.0; 4],
            opacity: 1.0,
        }],
    };
    let image = gpu
        .render_frame(&scene, &FrameConfig::new(16, 10, 0, 30.0))
        .unwrap();
    assert_eq!(image.width(), 16);
    assert_eq!(image.height(), 10);
    assert!(image.get_pixel(15, 3)[0] > 0);
}

#[test]
fn gpu_shader_suffix_composites_after_regular_gpu_scene() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping mixed shader test");
        return;
    };
    let scene = Scene {
        nodes: vec![
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 16.0,
                h: 10.0,
                fill: Color::rgb(255, 0, 0),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            SceneNode::Shader {
                x: 4.0,
                y: 2.0,
                w: 6.0,
                h: 4.0,
                source: "return vec4<f32>(0.0, 0.0, 1.0, 1.0);".into(),
                time: 0.0,
                params: [0.0; 4],
                opacity: 0.5,
            },
        ],
    };
    let image = gpu
        .render_frame(&scene, &FrameConfig::new(16, 10, 0, 30.0))
        .unwrap();
    assert_eq!(gpu.render_stats().gpu_frames, 1);
    assert_eq!(image.get_pixel(1, 1)[0], 255);
    let overlap = image.get_pixel(6, 4);
    assert!(
        overlap[0] > 80 && overlap[2] > 80,
        "overlap was {overlap:?}"
    );
}

#[test]
fn gpu_interleaved_shader_preserves_top_level_draw_order() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping interleaved shader test");
        return;
    };
    let rect = |color| SceneNode::Rect {
        x: 0.0,
        y: 0.0,
        w: 16.0,
        h: 10.0,
        fill: color,
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    };
    let scene = Scene {
        nodes: vec![
            rect(Color::rgb(255, 0, 0)),
            SceneNode::Shader {
                x: 4.0,
                y: 2.0,
                w: 6.0,
                h: 4.0,
                source: "return vec4<f32>(0.0, 0.0, 1.0, 1.0);".into(),
                time: 0.0,
                params: [0.0; 4],
                opacity: 1.0,
            },
            rect(Color::rgb(0, 255, 0)),
        ],
    };
    let image = gpu
        .render_frame(&scene, &FrameConfig::new(16, 10, 0, 30.0))
        .unwrap();
    assert_eq!(image.get_pixel(1, 1)[1], 255);
    assert_eq!(image.get_pixel(6, 4)[1], 255);
    assert_eq!(image.get_pixel(6, 4)[2], 0);
    assert_eq!(gpu.render_stats().gpu_frames, 2);
}

#[test]
fn gpu_image_cache_obeys_byte_budget_and_lru_eviction() {
    let Ok(gpu) = WgpuBackend::new().map(|backend| backend.with_image_cache_bytes(4)) else {
        println!("GPU backend unavailable; skipping image cache eviction test");
        return;
    };
    let red = "data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%221%22%20height%3D%221%22%3E%3Crect%20width%3D%221%22%20height%3D%221%22%20fill%3D%22%23f00%22%2F%3E%3C%2Fsvg%3E";
    let blue = "data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%221%22%20height%3D%221%22%3E%3Crect%20width%3D%221%22%20height%3D%221%22%20fill%3D%22%2300f%22%2F%3E%3C%2Fsvg%3E";
    let scene = Scene {
        nodes: vec![
            SceneNode::Image {
                src: red.into(),
                x: 0.0,
                y: 0.0,
                w: 8.0,
                h: 8.0,
                fit: ImageFit::Fill,
                opacity: 1.0,
            },
            SceneNode::Image {
                src: blue.into(),
                x: 8.0,
                y: 0.0,
                w: 8.0,
                h: 8.0,
                fit: ImageFit::Fill,
                opacity: 1.0,
            },
        ],
    };
    gpu.render_frame(&scene, &FrameConfig::new(16, 8, 0, 30.0))
        .unwrap();
    assert_eq!(gpu.gpu_image_cache_len(), 1);
}

#[test]
fn gpu_image_cache_reconfiguration_trims_existing_textures() {
    let Ok(gpu) = WgpuBackend::new().map(|backend| backend.with_image_cache_bytes(8)) else {
        println!("GPU backend unavailable; skipping image cache reconfiguration test");
        return;
    };
    let red = "data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%221%22%20height%3D%221%22%3E%3Crect%20width%3D%221%22%20height%3D%221%22%20fill%3D%22%23f00%22%2F%3E%3C%2Fsvg%3E";
    let blue = "data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%221%22%20height%3D%221%22%3E%3Crect%20width%3D%221%22%20height%3D%221%22%20fill%3D%22%2300f%22%2F%3E%3C%2Fsvg%3E";
    for src in [red, blue] {
        let scene = Scene {
            nodes: vec![SceneNode::Image {
                src: src.into(),
                x: 0.0,
                y: 0.0,
                w: 8.0,
                h: 8.0,
                fit: ImageFit::Fill,
                opacity: 1.0,
            }],
        };
        gpu.render_frame(&scene, &FrameConfig::new(8, 8, 0, 30.0))
            .unwrap();
    }
    assert_eq!(gpu.gpu_image_cache_len(), 2);
    let gpu = gpu.with_image_cache_bytes(4);
    assert_eq!(gpu.gpu_image_cache_len(), 1);
}

#[test]
fn gpu_gif_frame_uses_texture_path_and_matches_cpu() {
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame, RgbaImage};
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping GIF texture test");
        return;
    };
    let path =
        std::env::temp_dir().join(format!("rust-motion-wgpu-gif-{}.gif", std::process::id()));
    let file = std::fs::File::create(&path).unwrap();
    let mut encoder = GifEncoder::new(file);
    encoder.set_repeat(Repeat::Infinite).unwrap();
    for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
        let image = RgbaImage::from_pixel(2, 2, image::Rgba(color));
        encoder
            .encode_frame(Frame::from_parts(
                image,
                0,
                0,
                Delay::from_numer_denom_ms(100, 1),
            ))
            .unwrap();
    }
    drop(encoder);
    let scene = Scene {
        nodes: vec![SceneNode::Gif {
            src: path.to_string_lossy().into_owned(),
            time: 0.0,
            x: 0.0,
            y: 0.0,
            w: 16.0,
            h: 16.0,
            playback_rate: 1.0,
            loop_behavior: crate::gif_cache::LoopBehavior::Loop,
            fit: ImageFit::Fill,
            opacity: 1.0,
        }],
    };
    let config = FrameConfig::new(16, 16, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    let mean_error: f64 = gpu_image
        .pixels()
        .zip(cpu_image.pixels())
        .map(|(gpu, cpu)| {
            (0..4)
                .map(|channel| {
                    (i16::from(gpu[channel]) - i16::from(cpu[channel])).unsigned_abs() as u64
                })
                .sum::<u64>()
        })
        .sum::<u64>() as f64
        / (48 * 48 * 4) as f64;
    assert!(mean_error < 18.0, "GPU/CPU GIF mean error was {mean_error}");
    assert!(gpu_image.pixels().any(|pixel| pixel[3] > 0));
    assert_eq!(gpu.render_stats().gpu_frames, 1);
    assert_eq!(gpu.render_stats().cpu_fallback_frames, 0);
    let _ = std::fs::remove_file(path);
}

#[test]
fn gpu_gif_playback_rate_and_loop_behavior_parity_with_cpu() {
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame, RgbaImage};
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping GIF playback rate parity test");
        return;
    };
    let path = std::env::temp_dir().join(format!(
        "rust-motion-test-rate-{}.gif",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let file = std::fs::File::create(&path).unwrap();
    let mut encoder = GifEncoder::new(file);
    encoder.set_repeat(Repeat::Infinite).unwrap();
    // Frame 0: Red (0 - 100ms), Frame 1: Blue (100 - 200ms)
    for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
        let image = RgbaImage::from_pixel(2, 2, image::Rgba(color));
        encoder
            .encode_frame(Frame::from_parts(
                image,
                0,
                0,
                Delay::from_numer_denom_ms(100, 1),
            ))
            .unwrap();
    }
    drop(encoder);

    let cpu = TinySkiaBackend::new();
    let test_cases = [
        // (time_sec, playback_rate, loop_behavior, description)
        (
            0.15,
            0.5,
            crate::gif_cache::LoopBehavior::Loop,
            "0.5x rate should select frame 0 (75ms)",
        ),
        (
            0.06,
            2.0,
            crate::gif_cache::LoopBehavior::Loop,
            "2.0x rate should select frame 1 (120ms)",
        ),
        (
            0.08,
            1.5,
            crate::gif_cache::LoopBehavior::Loop,
            "1.5x rate should select frame 1 (120ms)",
        ),
        (
            0.30,
            1.0,
            crate::gif_cache::LoopBehavior::Loop,
            "Loop past duration wraps to frame 1 (100ms)",
        ),
        (
            0.30,
            1.0,
            crate::gif_cache::LoopBehavior::Pause,
            "Pause past duration clamps to last frame",
        ),
        (
            0.30,
            1.0,
            crate::gif_cache::LoopBehavior::Unmount,
            "Unmount past duration produces nothing",
        ),
    ];

    for (time, playback_rate, loop_behavior, desc) in test_cases {
        let scene = Scene {
            nodes: vec![SceneNode::Gif {
                src: path.to_string_lossy().into_owned(),
                time,
                x: 0.0,
                y: 0.0,
                w: 16.0,
                h: 16.0,
                playback_rate,
                loop_behavior,
                fit: ImageFit::Fill,
                opacity: 1.0,
            }],
        };
        let config = FrameConfig::new(16, 16, 0, 30.0);
        let gpu_image = gpu.render_frame(&scene, &config).unwrap();
        let cpu_image = cpu.render_frame(&scene, &config).unwrap();

        let mean_error: f64 = gpu_image
            .pixels()
            .zip(cpu_image.pixels())
            .map(|(gpu_px, cpu_px)| {
                (0..4)
                    .map(|c| (i16::from(gpu_px[c]) - i16::from(cpu_px[c])).unsigned_abs() as u64)
                    .sum::<u64>()
            })
            .sum::<u64>() as f64
            / (16 * 16 * 4) as f64;

        assert!(
            mean_error < 1.0,
            "GPU/CPU GIF parity mismatch for case '{desc}': mean error {mean_error}"
        );
    }

    let _ = std::fs::remove_file(path);
}

#[test]
fn gpu_gif_frame_index_cache_hit_across_timestamps() {
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame, RgbaImage};
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping GIF cache hit test");
        return;
    };
    let path = std::env::temp_dir().join(format!(
        "rust-motion-test-gif-cache-{}.gif",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let file = std::fs::File::create(&path).unwrap();
    let mut encoder = GifEncoder::new(file);
    encoder.set_repeat(Repeat::Infinite).unwrap();
    for color in [[255, 0, 0, 255], [0, 255, 0, 255]] {
        let image = RgbaImage::from_pixel(2, 2, image::Rgba(color));
        encoder
            .encode_frame(Frame::from_parts(
                image,
                0,
                0,
                Delay::from_numer_denom_ms(100, 1),
            ))
            .unwrap();
    }
    drop(encoder);

    let make_scene = |time: f64| Scene {
        nodes: vec![SceneNode::Gif {
            src: path.to_string_lossy().into_owned(),
            time,
            x: 0.0,
            y: 0.0,
            w: 16.0,
            h: 16.0,
            playback_rate: 1.0,
            loop_behavior: crate::gif_cache::LoopBehavior::Loop,
            fit: ImageFit::Fill,
            opacity: 1.0,
        }],
    };
    let config = FrameConfig::new(16, 16, 0, 30.0);

    let initial_uploads = gpu.gpu_texture_uploads();
    let initial_hits = gpu.gpu_texture_cache_hits();

    // 1. First render at 10ms -> frame index 0 (upload)
    gpu.render_frame(&make_scene(0.01), &config).unwrap();
    assert_eq!(gpu.gpu_texture_uploads(), initial_uploads + 1);

    // 2. Second render at 50ms -> still frame index 0 (must reuse texture cache, no new upload)
    gpu.render_frame(&make_scene(0.05), &config).unwrap();
    assert_eq!(gpu.gpu_texture_uploads(), initial_uploads + 1);
    assert_eq!(gpu.gpu_texture_cache_hits(), initial_hits + 1);

    // 3. Third render at 90ms -> still frame index 0 (must reuse texture cache)
    gpu.render_frame(&make_scene(0.09), &config).unwrap();
    assert_eq!(gpu.gpu_texture_uploads(), initial_uploads + 1);
    assert_eq!(gpu.gpu_texture_cache_hits(), initial_hits + 2);

    // 4. Render at 130ms -> frame index 1 (second upload)
    gpu.render_frame(&make_scene(0.13), &config).unwrap();
    assert_eq!(gpu.gpu_texture_uploads(), initial_uploads + 2);

    // 5. Render at 210ms -> wraps back to frame index 0 in Loop (must reuse frame 0 texture)
    gpu.render_frame(&make_scene(0.21), &config).unwrap();
    assert_eq!(gpu.gpu_texture_uploads(), initial_uploads + 2);
    assert_eq!(gpu.gpu_texture_cache_hits(), initial_hits + 3);

    let _ = std::fs::remove_file(path);
}

#[test]
fn gpu_emoji_uses_texture_path_and_matches_cpu() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping emoji texture test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Emoji {
            emoji: "🔥".into(),
            x: 4.0,
            y: 4.0,
            size: 32.0,
            opacity: 0.75,
        }],
    };
    let config = FrameConfig::new(48, 48, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    let mean_error: f64 = gpu_image
        .pixels()
        .zip(cpu_image.pixels())
        .map(|(gpu, cpu)| {
            (0..4)
                .map(|channel| {
                    (i16::from(gpu[channel]) - i16::from(cpu[channel])).unsigned_abs() as u64
                })
                .sum::<u64>()
        })
        .sum::<u64>() as f64
        / (48 * 48 * 4) as f64;
    assert!(
        mean_error < 18.0,
        "GPU/CPU emoji mean error was {mean_error}"
    );
    assert!(gpu_image.pixels().any(|pixel| pixel[3] > 0));
    assert_eq!(gpu.render_stats().cpu_fallback_frames, 0);
}

#[test]
fn gpu_audio_visualizer_uses_texture_path() {
    let audio = "/System/Library/Sounds/Glass.aiff";
    if !std::path::Path::new(audio).exists() {
        println!("System audio fixture unavailable; skipping audio visualizer test");
        return;
    }
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping audio visualizer test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::AudioVisualizer {
            src: audio.into(),
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 32.0,
            color: Color::rgb(0, 220, 255),
            style: crate::scene::VisualizerStyle::default(),
            time: 0.0,
            opacity: 0.8,
        }],
    };
    let config = FrameConfig::new(64, 32, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    let mean_error: f64 = gpu_image
        .pixels()
        .zip(cpu_image.pixels())
        .map(|(gpu, cpu)| {
            (0..4)
                .map(|channel| {
                    (i16::from(gpu[channel]) - i16::from(cpu[channel])).unsigned_abs() as u64
                })
                .sum::<u64>()
        })
        .sum::<u64>() as f64
        / (64 * 32 * 4) as f64;
    assert!(
        mean_error < 18.0,
        "GPU/CPU audio visualizer mean error was {mean_error}"
    );
    assert!(gpu_image.pixels().any(|pixel| pixel[3] > 0));
    assert_eq!(gpu.render_stats().cpu_fallback_frames, 0);
}

#[test]
fn gpu_native_video_stream_delivers_ordered_frames_without_readback() {
    if std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        println!("FFmpeg unavailable; skipping GPU native video stream test");
        return;
    }
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping GPU native video stream test");
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "rust-motion-wgpu-video-stream-{}",
        std::process::id()
    ));
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
            "testsrc2=size=16x16:rate=2:duration=1",
            "-an",
            "-c:v",
            "ffv1",
        ])
        .arg(&source)
        .status()
        .unwrap();
    assert!(generated.success());
    let frames = 2;
    let mut delivered = Vec::new();
    gpu.render_stream_gpu(
        frames,
        &|frame| {
            Ok(Scene {
                nodes: vec![SceneNode::Video {
                    src: source.display().to_string(),
                    time: frame as f64 / 2.0,
                    looped: false,
                    x: 0.0,
                    y: 0.0,
                    w: 16.0,
                    h: 16.0,
                    fit: ImageFit::Fill,
                    opacity: 1.0,
                }],
            })
        },
        &|frame| FrameConfig::new(16, 16, frame, 2.0),
        |frame, _view, width, height| delivered.push((frame, width, height)),
    )
    .unwrap();
    assert_eq!(delivered, vec![(0, 16, 16), (1, 16, 16)]);
    assert_eq!(gpu.render_stats().gpu_frames, frames as u64);
    assert_eq!(gpu.gpu_texture_cache_misses(), frames as u64);
    assert_eq!(gpu.gpu_texture_uploads(), frames as u64);
}

#[test]
fn gpu_video_frame_uses_decoded_texture_path() {
    if std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        println!("FFmpeg unavailable; skipping GPU video test");
        return;
    }
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping GPU video test");
        return;
    };
    let dir = std::env::temp_dir().join(format!("rust-motion-wgpu-video-{}", std::process::id()));
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
            "color=c=red:s=16x16:r=2:d=1",
            "-c:v",
            "ffv1",
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
            fit: ImageFit::Fill,
            opacity: 1.0,
        }],
    };
    let image = gpu
        .render_frame(&scene, &FrameConfig::new(16, 16, 0, 2.0))
        .unwrap();
    let pixel = image.get_pixel(8, 8);
    assert!(pixel[0] > 200 && pixel[1] < 40 && pixel[2] < 40);
    let mut same_frame = scene.clone();
    let SceneNode::Video { src, time, .. } = &mut same_frame.nodes[0] else {
        unreachable!();
    };
    *src = format!("file://{}", source.display());
    *time = 0.1;
    gpu.render_frame(&same_frame, &FrameConfig::new(16, 16, 0, 2.0))
        .unwrap();
    assert_eq!(gpu.gpu_image_cache_len(), 1);
    assert_eq!(gpu.gpu_texture_uploads(), 1);
    assert_eq!(gpu.gpu_texture_upload_bytes(), 16 * 16 * 4);
    assert_eq!(gpu.gpu_texture_cache_misses(), 1);
    assert_eq!(gpu.gpu_texture_cache_hits(), 1);
    assert_eq!(gpu.gpu_frame_count.load(Ordering::Relaxed), 2);

    let composited = Scene {
        nodes: vec![
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 16.0,
                h: 16.0,
                fill: Color::rgb(0, 0, 255),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            SceneNode::Layer {
                opacity: 0.5,
                blend_mode: crate::scene::BlendMode::Normal,
                clip: None,
                mask: None,
                mask_mode: crate::scene::MaskMode::Alpha,
                filters: Vec::new(),
                shadow: None,
                children: vec![SceneNode::Video {
                    src: source.display().to_string(),
                    time: 0.0,
                    looped: false,
                    x: 0.0,
                    y: 0.0,
                    w: 16.0,
                    h: 16.0,
                    fit: ImageFit::Fill,
                    opacity: 1.0,
                }],
            },
        ],
    };
    let composited_gpu = gpu
        .render_frame(&composited, &FrameConfig::new(16, 16, 0, 2.0))
        .unwrap();
    let composited_cpu = TinySkiaBackend::new()
        .render_frame(&composited, &FrameConfig::new(16, 16, 0, 2.0))
        .unwrap();
    assert_eq!(
        composited_gpu.get_pixel(8, 8),
        composited_cpu.get_pixel(8, 8)
    );
    let composite_pixel = composited_gpu.get_pixel(8, 8);
    assert!(composite_pixel[0] >= 127 && composite_pixel[0] <= 128);
    assert_eq!(composite_pixel[1], 0);
    assert_eq!(composite_pixel[2], 128);
    assert_eq!(composite_pixel[3], 255);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn gpu_video_fits_match_cpu_for_all_modes() {
    if std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        println!("FFmpeg unavailable; skipping GPU video fit test");
        return;
    }
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping GPU video fit test");
        return;
    };
    let dir =
        std::env::temp_dir().join(format!("rust-motion-wgpu-video-fit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("rectangular.mkv");
    let generated = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=16x8:r=1:d=1",
            "-c:v",
            "ffv1",
        ])
        .arg(&source)
        .status()
        .unwrap();
    assert!(generated.success());
    let config = FrameConfig::new(32, 32, 0, 1.0);
    for fit in [
        ImageFit::Cover,
        ImageFit::Contain,
        ImageFit::Fill,
        ImageFit::None,
        ImageFit::ScaleDown,
    ] {
        let scene = Scene {
            nodes: vec![SceneNode::Video {
                src: source.display().to_string(),
                time: 0.0,
                looped: false,
                x: 4.0,
                y: 4.0,
                w: 24.0,
                h: 24.0,
                fit,
                opacity: 1.0,
            }],
        };
        let gpu_image = gpu.render_frame(&scene, &config).unwrap();
        let cpu_image = TinySkiaBackend::new()
            .render_frame(&scene, &config)
            .unwrap();
        let mean_error = gpu_image
            .pixels()
            .zip(cpu_image.pixels())
            .map(|(gpu, cpu)| {
                (0..4)
                    .map(|channel| {
                        (i16::from(gpu[channel]) - i16::from(cpu[channel])).unsigned_abs() as u64
                    })
                    .sum::<u64>()
            })
            .sum::<u64>() as f64
            / (32 * 32 * 4) as f64;
        assert!(
            mean_error < 20.0,
            "GPU/CPU video {fit:?} mean error was {mean_error}"
        );
    }
    assert_eq!(gpu.gpu_image_cache_len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn gpu_video_rotation_matches_cpu_display_orientation() {
    if std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        println!("FFmpeg unavailable; skipping GPU video rotation test");
        return;
    }
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping GPU video rotation test");
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "rust-motion-wgpu-rotated-video-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("rotated.mp4");
    let generated = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:size=8x4:rate=1:duration=1",
                "-vf",
                "drawbox=x=0:y=0:w=4:h=2:color=red:t=fill,drawbox=x=4:y=0:w=4:h=2:color=green:t=fill,drawbox=x=0:y=2:w=4:h=2:color=blue:t=fill,drawbox=x=4:y=2:w=4:h=2:color=white:t=fill",
                "-metadata:s:v:0",
                "rotate=90",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
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
            w: 4.0,
            h: 8.0,
            fit: ImageFit::Fill,
            opacity: 1.0,
        }],
    };
    let config = FrameConfig::new(4, 8, 0, 1.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::new()
        .render_frame(&scene, &config)
        .unwrap();
    for &(x, y) in &[(0, 0), (3, 0), (0, 7), (3, 7)] {
        let gpu_pixel = gpu_image.get_pixel(x, y);
        let cpu_pixel = cpu_image.get_pixel(x, y);
        for channel in 0..4 {
            assert!(
                    (i16::from(gpu_pixel[channel]) - i16::from(cpu_pixel[channel])).abs() <= 3,
                    "rotation pixel mismatch at ({x},{y}) channel {channel}: GPU={gpu_pixel:?} CPU={cpu_pixel:?}"
                );
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn gpu_vfr_video_rotation_matches_cpu_across_timeline() {
    if std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        println!("FFmpeg unavailable; skipping GPU VFR rotation test");
        return;
    }
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping GPU VFR rotation test");
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "rust-motion-wgpu-vfr-rotation-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("vfr-rotated.mp4");
    let generated = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-display_rotation:v:0",
            "90",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=16x8:rate=10:duration=1",
            "-vf",
            "select=eq(n\\,0)+eq(n\\,1)+eq(n\\,4)+eq(n\\,9)",
            "-fps_mode",
            "vfr",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&source)
        .status()
        .unwrap();
    assert!(generated.success());

    let config = FrameConfig::new(8, 16, 0, 5.0);
    for time in [0.0, 0.2, 0.4, 0.6, 0.8] {
        let scene = Scene {
            nodes: vec![SceneNode::Video {
                src: source.display().to_string(),
                time,
                looped: false,
                x: 0.0,
                y: 0.0,
                w: 8.0,
                h: 16.0,
                fit: ImageFit::Fill,
                opacity: 1.0,
            }],
        };
        let gpu_image = gpu.render_frame(&scene, &config).unwrap();
        let cpu_image = TinySkiaBackend::new()
            .render_frame(&scene, &config)
            .unwrap();
        assert_eq!(gpu_image.dimensions(), (8, 16));
        let mean_error = gpu_image
            .pixels()
            .zip(cpu_image.pixels())
            .map(|(gpu, cpu)| {
                (0..4)
                    .map(|channel| {
                        (i16::from(gpu[channel]) - i16::from(cpu[channel])).unsigned_abs() as u64
                    })
                    .sum::<u64>()
            })
            .sum::<u64>() as f64
            / (8 * 16 * 4) as f64;
        assert!(
            mean_error < 20.0,
            "GPU/CPU VFR rotation mean error at {time}s was {mean_error}"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn gpu_renders_transformed_path_stroke_and_three_stop_gradient() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping render comparison");
        return;
    };
    let scene = Scene {
        nodes: vec![
            SceneNode::Group {
                transform: crate::scene::Transform2D {
                    tx: 10.0,
                    ty: 5.0,
                    ..Default::default()
                },
                opacity: 1.0,
                children: vec![SceneNode::Path {
                    d: "M 5 5 L 30 5 L 30 25 L 5 25 Z".into(),
                    fill: Some(Color::rgb(255, 0, 0)),
                    stroke: Some(Color::WHITE),
                    stroke_width: 2.0,
                    opacity: 1.0,
                }],
            },
            SceneNode::LinearGradient {
                x: 0.0,
                y: 36.0,
                w: 96.0,
                h: 24.0,
                angle_deg: 90.0,
                stops: vec![
                    GradientStop {
                        position: 0.0,
                        color: Color::rgb(255, 0, 0),
                    },
                    GradientStop {
                        position: 0.5,
                        color: Color::rgb(0, 255, 0),
                    },
                    GradientStop {
                        position: 1.0,
                        color: Color::rgb(0, 0, 255),
                    },
                ],
            },
            SceneNode::Rect {
                x: 60.0,
                y: 8.0,
                w: 24.0,
                h: 18.0,
                fill: Color::rgb(0, 0, 255),
                stroke: Some(Color::WHITE),
                stroke_width: 4.0,
                corner_radius: 4.0,
            },
        ],
    };
    let config = FrameConfig::new(96, 64, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::headless()
        .render_frame(&scene, &config)
        .unwrap();

    let path_pixel = gpu_image.get_pixel(20, 16);
    assert!(path_pixel[0] > 200 && path_pixel[3] > 200);
    let middle_gradient = gpu_image.get_pixel(48, 48);
    assert!(middle_gradient[1] > middle_gradient[0]);
    assert!(middle_gradient[1] > middle_gradient[2]);
    let rect_stroke = gpu_image.get_pixel(60, 16);
    assert!(rect_stroke[0] > 180 && rect_stroke[1] > 180 && rect_stroke[2] > 180);

    let alpha_error = gpu_image
        .pixels()
        .zip(cpu_image.pixels())
        .map(|(gpu, cpu)| (i16::from(gpu[3]) - i16::from(cpu[3])).unsigned_abs() as u64)
        .sum::<u64>();
    let mean_alpha_error = alpha_error as f64 / (config.width * config.height) as f64;
    assert!(
        mean_alpha_error < 8.0,
        "CPU/GPU mean alpha error was {mean_alpha_error}"
    );
}

#[test]
fn gpu_opacity_layer_preserves_alpha_within_tolerance() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping layer parity test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 0.5,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: vec![crate::scene::SceneFilter::Opacity { amount: 0.5 }],
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 8.0,
                y: 8.0,
                w: 32.0,
                h: 24.0,
                fill: Color::rgb(240, 120, 40),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    let config = FrameConfig::new(64, 64, 0, 30.0);
    let gpu_image = gpu.render_frame(&scene, &config).unwrap();
    let cpu_image = TinySkiaBackend::headless()
        .render_frame(&scene, &config)
        .unwrap();
    let alpha_error = gpu_image
        .pixels()
        .zip(cpu_image.pixels())
        .map(|(gpu, cpu)| (i16::from(gpu[3]) - i16::from(cpu[3])).unsigned_abs() as u64)
        .sum::<u64>();
    let mean_alpha_error = alpha_error as f64 / (config.width * config.height) as f64;
    assert!(
        mean_alpha_error < 8.0,
        "CPU/GPU layer mean alpha error was {mean_alpha_error}"
    );
    let rgb_error = gpu_image
        .pixels()
        .zip(cpu_image.pixels())
        .map(|(gpu, cpu)| {
            (0..3)
                .map(|channel| {
                    (i16::from(gpu[channel]) - i16::from(cpu[channel])).unsigned_abs() as u64
                })
                .sum::<u64>()
        })
        .sum::<u64>();
    let mean_rgb_error = rgb_error as f64 / (config.width * config.height * 3) as f64;
    assert!(
        mean_rgb_error < 8.0,
        "CPU/GPU layer mean RGB error was {mean_rgb_error}"
    );
}

#[test]
fn gpu_render_stream_pipelined_matches_render_frame() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping render_stream test");
        return;
    };

    let width = 160;
    let height = 90;
    let total_frames = 5;

    let make_scene = |frame: u32| {
        let offset = frame as f32 * 10.0;
        Scene {
            nodes: vec![
                SceneNode::Rect {
                    x: offset,
                    y: 10.0,
                    w: 40.0,
                    h: 30.0,
                    fill: Color::rgb(200, 50, 80),
                    stroke: Some(Color::WHITE),
                    stroke_width: 2.0,
                    corner_radius: 4.0,
                },
                SceneNode::Circle {
                    cx: 80.0,
                    cy: 45.0 + offset * 0.5,
                    r: 20.0,
                    fill: Color::rgba(50, 150, 250, 180),
                    stroke: None,
                    stroke_width: 0.0,
                },
            ],
        }
    };

    // 1. Render sequentially using render_frame
    let mut sequential_frames = Vec::new();
    for f in 0..total_frames {
        let scene = make_scene(f);
        let cfg = FrameConfig::new(width, height, f, 30.0);
        let img = gpu.render_frame(&scene, &cfg).expect("render_frame failed");
        sequential_frames.push(img.into_raw());
    }

    // 2. Render pipelined stream using render_stream
    let mut streamed_frames = Vec::new();
    gpu.render_stream(
        total_frames,
        &|f| Ok(make_scene(f)),
        &|f| FrameConfig::new(width, height, f, 30.0),
        &mut |_f, rgba: &[u8]| {
            streamed_frames.push(rgba.to_vec());
            Ok(())
        },
    )
    .expect("render_stream failed");

    assert_eq!(streamed_frames.len(), total_frames as usize);
    for f in 0..total_frames as usize {
        assert_eq!(
            streamed_frames[f], sequential_frames[f],
            "Frame {f} rendered via render_stream does not match render_frame"
        );
    }
}

#[test]
fn gpu_native_frame_sink_skips_cpu_readback() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping GPU-native sink test");
        return;
    };
    let scene = Scene {
        nodes: vec![SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 16.0,
            h: 16.0,
            fill: Color::WHITE,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }],
    };
    let mut callback_dimensions = None;
    gpu.render_frame_gpu(
        &scene,
        &FrameConfig::new(16, 16, 0, 30.0),
        |_view, width, height| callback_dimensions = Some((width, height)),
    )
    .unwrap();
    assert_eq!(callback_dimensions, Some((16, 16)));
    let timing = gpu.video_timing_stats();
    assert_eq!(timing.gpu_submit_readback_ns, 0);
    assert!(timing.gpu_submit_no_readback_ns > 0);
}

#[test]
fn gpu_native_stream_delivers_ordered_frames_without_readback() {
    let Ok(gpu) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping GPU-native stream test");
        return;
    };
    let mut delivered = Vec::new();
    gpu.render_stream_gpu(
        3,
        &|frame| {
            Ok(Scene {
                nodes: vec![SceneNode::Rect {
                    x: frame as f32,
                    y: 0.0,
                    w: 8.0,
                    h: 8.0,
                    fill: Color::WHITE,
                    stroke: None,
                    stroke_width: 0.0,
                    corner_radius: 0.0,
                }],
            })
        },
        &|frame| FrameConfig::new(16, 16, frame, 30.0),
        |frame, _view, width, height| delivered.push((frame, width, height)),
    )
    .unwrap();
    assert_eq!(delivered, vec![(0, 16, 16), (1, 16, 16), (2, 16, 16)]);
    let timing = gpu.video_timing_stats();
    assert_eq!(timing.gpu_submit_readback_ns, 0);
    assert!(timing.gpu_submit_no_readback_ns > 0);
}

#[test]
fn image_fit_geometry_matches_expected_crop_and_letterbox() {
    let contain =
        image_placement(ImageFit::Contain, 200.0, 100.0, 10.0, 20.0, 100.0, 100.0).unwrap();
    assert_eq!(contain.destination, [10.0, 45.0, 100.0, 50.0]);
    assert_eq!(contain.source_uv, [0.0, 0.0, 1.0, 1.0]);

    let cover = image_placement(ImageFit::Cover, 200.0, 100.0, 0.0, 0.0, 100.0, 100.0).unwrap();
    assert_eq!(cover.destination, [0.0, 0.0, 100.0, 100.0]);
    assert_eq!(cover.source_uv, [0.25, 0.0, 0.75, 1.0]);

    let fill = image_placement(ImageFit::Fill, 200.0, 100.0, 1.0, 2.0, 30.0, 40.0).unwrap();
    assert_eq!(fill.destination, [1.0, 2.0, 30.0, 40.0]);
    assert_eq!(fill.source_uv, [0.0, 0.0, 1.0, 1.0]);
}

#[test]
fn image_fit_none_and_scale_down_never_upscale() {
    let none = image_placement(ImageFit::None, 200.0, 100.0, 0.0, 0.0, 80.0, 80.0).unwrap();
    assert_eq!(none.destination, [0.0, 0.0, 80.0, 80.0]);
    for (actual, expected) in none.source_uv.into_iter().zip([0.3, 0.1, 0.7, 0.9]) {
        assert!((actual - expected).abs() < 1e-6);
    }

    let scale_down =
        image_placement(ImageFit::ScaleDown, 20.0, 10.0, 0.0, 0.0, 100.0, 100.0).unwrap();
    assert_eq!(scale_down.destination, [40.0, 45.0, 20.0, 10.0]);
}

#[test]
fn unsupported_nodes_trigger_cpu_fallback() {
    let mut scene = Scene::new();
    scene.push(SceneNode::Shader {
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
        source: "return vec4<f32>(1.0);".into(),
        time: 0.0,
        params: [0.0; 4],
        opacity: 1.0,
    });
    scene.push(SceneNode::Text {
        x: 0.0,
        y: 20.0,
        content: "text".into(),
        font_size: 20.0,
        color: Color::WHITE,
        font_weight: 400,
        font_sources: Vec::new(),
    });
    assert!(!gpu_supports_scene(&scene));

    let image_scene = Scene {
        nodes: vec![SceneNode::Image {
            src: "asset.png".into(),
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
            fit: crate::scene::ImageFit::Cover,
            opacity: 1.0,
        }],
    };
    assert!(gpu_supports_scene(&image_scene));

    let invalid_path_scene = Scene {
        nodes: vec![SceneNode::Path {
            d: "M 0 0 L nope".into(),
            fill: Some(Color::WHITE),
            stroke: None,
            stroke_width: 0.0,
            opacity: 1.0,
        }],
    };
    assert!(!gpu_supports_scene(&invalid_path_scene));
}

#[test]
fn path_strokes_groups_and_multistop_gradients_compile_for_gpu() {
    let scene = Scene {
        nodes: vec![SceneNode::Group {
            transform: crate::scene::Transform2D {
                tx: 12.0,
                ty: 8.0,
                scale_x: 1.5,
                scale_y: 0.75,
                rotate_deg: 15.0,
            },
            opacity: 0.6,
            children: vec![
                SceneNode::Path {
                    d: "M 0 0 C 20 0 20 20 40 20 L 40 40 Z".into(),
                    fill: Some(Color::rgb(255, 0, 0)),
                    stroke: Some(Color::WHITE),
                    stroke_width: 3.0,
                    opacity: 0.8,
                },
                SceneNode::Rect {
                    x: 45.0,
                    y: 0.0,
                    w: 20.0,
                    h: 20.0,
                    fill: Color::BLACK,
                    stroke: Some(Color::WHITE),
                    stroke_width: 2.0,
                    corner_radius: 4.0,
                },
                SceneNode::Circle {
                    cx: 75.0,
                    cy: 10.0,
                    r: 8.0,
                    fill: Color::BLACK,
                    stroke: Some(Color::WHITE),
                    stroke_width: 2.0,
                },
                SceneNode::LinearGradient {
                    x: 0.0,
                    y: 45.0,
                    w: 80.0,
                    h: 20.0,
                    angle_deg: 90.0,
                    stops: vec![
                        GradientStop {
                            position: 0.0,
                            color: Color::rgb(255, 0, 0),
                        },
                        GradientStop {
                            position: 0.5,
                            color: Color::rgb(0, 255, 0),
                        },
                        GradientStop {
                            position: 1.0,
                            color: Color::rgb(0, 0, 255),
                        },
                    ],
                },
            ],
        }],
    };
    assert!(gpu_supports_scene(&scene));
    let commands = compile_scene(&scene, &TinySkiaBackend::headless()).unwrap();
    assert_eq!(
        commands.len(),
        5,
        "path fill/stroke, rect, circle, gradient"
    );
    assert_eq!(
        commands
            .iter()
            .filter(|command| matches!(command, DrawCommand::Mesh { .. }))
            .count(),
        2
    );
    let gradient = commands.last().unwrap().instance();
    assert_eq!(gradient.kind_data[1], 3);
    assert!((gradient.params[3] - 0.6).abs() < f32::EPSILON);

    let expected = crate::scene::Transform2D {
        tx: 12.0,
        ty: 8.0,
        scale_x: 1.5,
        scale_y: 0.75,
        rotate_deg: 15.0,
    }
    .to_tiny_skia();
    assert_eq!(gradient.transform_x, transform_rows(expected).0);
    assert_eq!(gradient.transform_y, transform_rows(expected).1);
}

#[test]
fn gradients_beyond_the_uniform_limit_use_cpu_fallback() {
    let scene = Scene {
        nodes: vec![SceneNode::LinearGradient {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            angle_deg: 0.0,
            stops: (0..=MAX_GRADIENT_STOPS)
                .map(|index| GradientStop {
                    position: index as f32 / MAX_GRADIENT_STOPS as f32,
                    color: Color::WHITE,
                })
                .collect(),
        }],
    };

    assert!(!gpu_supports_scene(&scene));
}

#[test]
fn plain_normal_layers_compile_as_gpu_opacity_groups() {
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 0.5,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: vec![crate::scene::SceneFilter::Opacity { amount: 0.5 }],
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
                fill: Color::WHITE,
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    assert!(gpu_supports_scene(&scene));
    assert!(
        (compile_scene(&scene, &TinySkiaBackend::headless()).unwrap()[0]
            .instance()
            .params[3]
            - 0.25)
            .abs()
            < f32::EPSILON
    );
}

#[test]
fn disjoint_texture_layers_compile_as_gpu_opacity_groups() {
    let source = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
    let layer = |second_x| SceneNode::Layer {
        opacity: 0.5,
        blend_mode: crate::scene::BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: crate::scene::MaskMode::Alpha,
        filters: Vec::new(),
        shadow: None,
        children: vec![
            SceneNode::Image {
                src: source.into(),
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
                fit: ImageFit::Fill,
                opacity: 1.0,
            },
            SceneNode::Image {
                src: source.into(),
                x: second_x,
                y: 0.0,
                w: 10.0,
                h: 10.0,
                fit: ImageFit::Fill,
                opacity: 1.0,
            },
        ],
    };
    assert!(gpu_supports_scene(&Scene {
        nodes: vec![layer(12.0)],
    }));
    assert!(!gpu_supports_scene(&Scene {
        nodes: vec![layer(8.0)],
    }));
    let transformed = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 0.5,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: Vec::new(),
            shadow: None,
            children: vec![
                SceneNode::Group {
                    transform: crate::scene::Transform2D::translate(0.0, 0.0),
                    opacity: 1.0,
                    children: vec![SceneNode::Image {
                        src: source.into(),
                        x: 0.0,
                        y: 0.0,
                        w: 10.0,
                        h: 10.0,
                        fit: ImageFit::Fill,
                        opacity: 1.0,
                    }],
                },
                SceneNode::Group {
                    transform: crate::scene::Transform2D::translate(12.0, 0.0).with_rotate(5.0),
                    opacity: 1.0,
                    children: vec![SceneNode::Image {
                        src: source.into(),
                        x: 0.0,
                        y: 0.0,
                        w: 10.0,
                        h: 10.0,
                        fit: ImageFit::Fill,
                        opacity: 1.0,
                    }],
                },
            ],
        }],
    };
    assert!(gpu_supports_scene(&transformed));
}

#[test]
fn overlapping_texture_layer_reports_offscreen_fallback_reason() {
    let scene = Scene {
            nodes: vec![SceneNode::Layer {
                opacity: 0.5,
                blend_mode: crate::scene::BlendMode::Normal,
                clip: None,
                mask: None,
                mask_mode: crate::scene::MaskMode::Alpha,
                filters: Vec::new(),
                shadow: None,
                children: vec![
                    SceneNode::Image {
                        src: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=".into(),
                        x: 0.0,
                        y: 0.0,
                        w: 10.0,
                        h: 10.0,
                        fit: ImageFit::Fill,
                        opacity: 1.0,
                    },
                    SceneNode::Image {
                        src: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=".into(),
                        x: 8.0,
                        y: 0.0,
                        w: 10.0,
                        h: 10.0,
                        fit: ImageFit::Fill,
                        opacity: 1.0,
                    },
                ],
            }],
        };
    assert_eq!(
        gpu_fallback_reason(&scene),
        "overlapping texture layer requires offscreen compositing"
    );
    let mut filtered = scene.clone();
    if let Some(SceneNode::Layer { filters, .. }) = filtered.nodes.first_mut() {
        filters.push(crate::scene::SceneFilter::Blur { sigma: 2.0 });
    }
    assert!(trailing_overlap_texture_layer(&filtered).is_none());

    let mut opacity_chain = scene.clone();
    if let Some(SceneNode::Layer { filters, .. }) = opacity_chain.nodes.first_mut() {
        filters.extend([
            crate::scene::SceneFilter::Opacity { amount: 0.8 },
            crate::scene::SceneFilter::Opacity { amount: 0.5 },
        ]);
    }
    let (_, _, effective_opacity) = trailing_overlap_texture_layer(&opacity_chain)
        .expect("opacity-only overlap layer should use GPU compositing");
    assert!((effective_opacity - 0.2).abs() < f32::EPSILON);
}

#[test]
fn invalid_opacity_filter_does_not_enter_gpu_path() {
    let scene = Scene {
        nodes: vec![SceneNode::Layer {
            opacity: 1.0,
            blend_mode: crate::scene::BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: crate::scene::MaskMode::Alpha,
            filters: vec![crate::scene::SceneFilter::Opacity { amount: 1.1 }],
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
                fill: Color::WHITE,
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        }],
    };
    assert!(!gpu_supports_scene(&scene));
}

#[test]
fn gpu_e2e_profiling_stream_and_summary() {
    let Ok(backend) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping profiling test");
        return;
    };

    let backend = backend.with_profiling(true);
    assert!(backend.is_profiling_enabled());

    let width = 160;
    let height = 120;
    let total_frames = 5;

    let scene_fn = |frame: u32| -> Result<Scene, RasterError> {
        let mut scene = Scene::new();
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: width as f32,
            h: height as f32,
            fill: Color::rgb(20, 30, 40),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });
        scene.push(SceneNode::Rect {
            x: (frame * 15) as f32,
            y: 30.0,
            w: 40.0,
            h: 40.0,
            fill: Color::rgba(255, 100, 50, 200),
            stroke: Some(Color::WHITE),
            stroke_width: 2.0,
            corner_radius: 4.0,
        });
        Ok(scene)
    };

    let config_fn = |frame: u32| -> FrameConfig { FrameConfig::new(width, height, frame, 30.0) };

    let mut consumed_frames = 0;
    let mut sink = |_frame: u32, pixels: &[u8]| -> Result<(), RasterError> {
        assert_eq!(pixels.len(), (width * height * 4) as usize);
        consumed_frames += 1;
        Ok(())
    };

    backend
        .render_stream(total_frames, &scene_fn, &config_fn, &mut sink)
        .expect("render_stream failed");

    assert_eq!(consumed_frames, total_frames);

    let samples = backend.profile_samples();
    assert_eq!(samples.len() as u32, total_frames);

    for (i, sample) in samples.iter().enumerate() {
        assert_eq!(sample.frame_idx, i as u32);
        assert!(!sample.cpu_fallback);
        assert_eq!(sample.readback_bytes, (width * height * 4) as u64);
        assert!(sample.total_frame_ns > 0);
    }

    let summary = backend.profile_summary();
    assert_eq!(summary.total_frames, total_frames);
    assert_eq!(summary.gpu_frames, total_frames);
    assert_eq!(summary.cpu_fallback_frames, 0);
    assert!(summary.session_wall_ms > 0.0);
    assert!(summary.throughput_fps > 0.0);
    assert!(summary.total_frame.mean_ms > 0.0);

    assert!(summary.scene_eval.mean_ms >= 0.0);
    assert!(summary.compile_encode.mean_ms >= 0.0);
    assert!(summary.submission_wait.mean_ms >= 0.0);
    assert!(summary.readback.mean_ms > 0.0);
    assert_eq!(
        summary.readback.bytes_per_frame,
        (width * height * 4) as f64
    );
    assert_eq!(
        summary.readback.total_bytes,
        (width * height * 4 * total_frames) as u64
    );

    assert!(summary.total_frame.p50_ms <= summary.total_frame.p95_ms);
    assert!(summary.total_frame.p95_ms <= summary.total_frame.p99_ms);
    assert!(summary.total_frame.min_ms <= summary.total_frame.max_ms);

    let table = summary.render_table();
    assert!(table.contains("Pipeline Stage"));
    assert!(table.contains("p50"));
    assert!(table.contains("Compile & Command Encode"));
    assert!(table.contains("GPU Fence Wait (CPU)"));
    assert!(table.contains("Readback"));
    assert!(table.contains("Total Frame Latency"));
    assert!(table.contains("Image Cache:"));
    assert!(table.contains("Text Atlas:"));
    assert!(table.contains("Throughput:"));

    backend.reset_profiling();
    assert_eq!(backend.profile_samples().len(), 0);
}

#[test]
fn gpu_e2e_profiling_single_frame() {
    let Ok(backend) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping profiling test");
        return;
    };

    let backend = backend.with_profiling(true);
    let mut scene = Scene::new();
    scene.push(SceneNode::Rect {
        x: 0.0,
        y: 0.0,
        w: 64.0,
        h: 64.0,
        fill: Color::rgb(255, 0, 0),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    });

    let config = FrameConfig::new(64, 64, 7, 30.0);
    let img = backend
        .render_frame(&scene, &config)
        .expect("render_frame failed");
    assert_eq!(img.width(), 64);
    assert_eq!(img.height(), 64);

    let samples = backend.profile_samples();
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].frame_idx, 7);
    assert_eq!(samples[0].readback_bytes, 64 * 64 * 4);
    assert!(!samples[0].cpu_fallback);

    let summary = backend.profile_summary();
    assert_eq!(summary.total_frames, 1);
    assert_eq!(summary.gpu_frames, 1);
}

#[test]
fn gpu_e2e_profiling_disabled_zero_samples() {
    let Ok(backend) = WgpuBackend::new() else {
        println!("GPU backend unavailable; skipping profiling test");
        return;
    };

    assert!(!backend.is_profiling_enabled());
    let scene = Scene::new();
    let config = FrameConfig::new(64, 64, 0, 30.0);
    let _ = backend.render_frame(&scene, &config);

    assert_eq!(backend.profile_samples().len(), 0);
    assert_eq!(backend.profile_summary().total_frames, 0);
}
