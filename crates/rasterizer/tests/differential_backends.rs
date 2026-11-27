//! Three-way differential and frame-order determinism verification across
//! CPU (`TinySkiaBackend`), `WgpuBackend`, and Apple Silicon `MetalComputeExecutor` (#166).

use image::RgbaImage;
use rust_motion_rasterizer::backend::{FrameConfig, RasterizerBackend};
use rust_motion_rasterizer::mesh3d::Vec3;
use rust_motion_rasterizer::scene3d::{
    Geometry3D, Material3D, Node3D, Scene3D, Scene3DBackground, Scene3DCamera, Scene3DLight,
    Transform3D,
};
use rust_motion_rasterizer::{
    BlendMode, Color, MaskMode, Scene, SceneFilter, SceneNode, TinySkiaBackend,
};

const WIDTH: u32 = 160;
const HEIGHT: u32 = 120;
const FPS: f64 = 30.0;

fn analytic_2d_scene(frame: u32) -> Scene {
    let mut s = Scene::new();
    s.push(SceneNode::Rect {
        x: 0.0,
        y: 0.0,
        w: WIDTH as f32,
        h: HEIGHT as f32,
        fill: Color::rgb(18, 24, 38),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    });
    s.push(SceneNode::Rect {
        x: 16.0 + (frame as f32) * 10.0,
        y: 22.0,
        w: 52.0,
        h: 40.0,
        fill: Color::rgb(235, 95, 45),
        stroke: Some(Color::WHITE),
        stroke_width: 2.0,
        corner_radius: 6.0,
    });
    s.push(SceneNode::Circle {
        cx: 110.0 - (frame as f32) * 5.0,
        cy: 74.0,
        r: 22.0,
        fill: Color::rgba(40, 190, 220, 180),
        stroke: None,
        stroke_width: 0.0,
    });
    s
}

fn blur_and_filter_layer_scene(frame: u32) -> Scene {
    let mut s = Scene::new();
    s.push(SceneNode::Rect {
        x: 0.0,
        y: 0.0,
        w: WIDTH as f32,
        h: HEIGHT as f32,
        fill: Color::rgb(22, 42, 74),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    });
    s.push(SceneNode::Layer {
        opacity: 0.9,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Blur { sigma: 2.0 }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 24.0 + (frame as f32) * 7.0,
            y: 24.0 + (frame as f32) * 3.0,
            w: 64.0,
            h: 48.0,
            fill: Color::rgb(240, 160, 40),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 6.0,
        }],
    });
    s
}

fn mixed_3d_and_2d_scene(frame: u32) -> Scene {
    let t = frame as f32 * 0.25;
    let mut scene3d = Scene3D::new()
        .with_camera(Scene3DCamera::perspective(
            Vec3::new(0.0, 20.0, 160.0),
            Vec3::new(0.0, 0.0, 0.0),
            50.0,
        ))
        .with_background(Scene3DBackground::Solid {
            color: Color::rgb(12, 18, 28),
        });
    scene3d.add_light(Scene3DLight::ambient(Color::WHITE, 0.35));
    scene3d.add_light(Scene3DLight::directional(
        Vec3::new(0.8, 1.0, -0.7),
        Color::WHITE,
        0.9,
    ));
    scene3d.add_node(
        Node3D::new(
            Geometry3D::Cube { size: 42.0 },
            Material3D::pbr(Color::rgb(65, 195, 240), 0.3, 0.4),
        )
        .with_transform(Transform3D::from_euler(t * 0.6, t, 0.15)),
    );
    scene3d.add_node(
        Node3D::new(
            Geometry3D::Torus {
                r_major: 36.0,
                r_minor: 8.0,
                segs_major: 20,
                segs_minor: 10,
            },
            Material3D::pbr(Color::rgb(245, 110, 75), 0.7, 0.25),
        )
        .with_transform(Transform3D::from_euler(1.0 + t * 0.4, -t * 0.7, 0.0)),
    );

    let mut s = Scene::new();
    s.push(SceneNode::Scene3D {
        x: 0.0,
        y: 0.0,
        w: WIDTH as f32,
        h: HEIGHT as f32,
        scene: Box::new(scene3d),
        opacity: 1.0,
    });
    s.push(SceneNode::Layer {
        opacity: 0.85,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Blur { sigma: 1.5 }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 10.0,
            y: 88.0,
            w: 96.0,
            h: 22.0,
            fill: Color::rgba(20, 30, 50, 220),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 4.0,
        }],
    });
    s.push(SceneNode::Text {
        x: 16.0,
        y: 103.0,
        content: format!("F{frame}"),
        font_size: 12.0,
        color: Color::WHITE,
        font_weight: 700,
        font_sources: Vec::new(),
    });
    s
}

fn corpus_scene(kind: usize, frame: u32) -> (&'static str, Scene) {
    match kind {
        0 => ("analytic_2d", analytic_2d_scene(frame)),
        1 => ("blur_layer", blur_and_filter_layer_scene(frame)),
        _ => ("mixed_3d_2d", mixed_3d_and_2d_scene(frame)),
    }
}

fn diff_stats(a: &RgbaImage, b: &RgbaImage) -> (u8, f64) {
    assert_eq!(a.dimensions(), b.dimensions());
    let mut max_diff = 0u8;
    let mut sum = 0u64;
    let ra = a.as_raw();
    let rb = b.as_raw();
    for (pa, pb) in ra.iter().zip(rb.iter()) {
        let d = pa.abs_diff(*pb);
        if d > max_diff {
            max_diff = d;
        }
        sum += u64::from(d);
    }
    (max_diff, sum as f64 / ra.len().max(1) as f64)
}

fn render_order_corpus<B: RasterizerBackend>(backend: &B) -> Vec<Vec<RgbaImage>> {
    let seq = [0u32, 1, 2, 3, 4];
    let rev = [4u32, 3, 2, 1, 0];
    let rand = [2u32, 0, 4, 1, 3];

    let mut seq_outputs = Vec::with_capacity(3);
    for kind in 0..3 {
        let mut by_frame: Vec<Option<RgbaImage>> = vec![None; 5];
        for &f in &seq {
            let (_label, scene) = corpus_scene(kind, f);
            let cfg = FrameConfig::new(WIDTH, HEIGHT, f, FPS);
            by_frame[f as usize] = Some(backend.render_frame(&scene, &cfg).unwrap());
        }
        let seq_imgs: Vec<RgbaImage> = by_frame.into_iter().map(Option::unwrap).collect();

        for (order_name, order) in [("reverse", rev), ("random", rand)] {
            for f in order {
                let (label, scene) = corpus_scene(kind, f);
                let cfg = FrameConfig::new(WIDTH, HEIGHT, f, FPS);
                let img = backend.render_frame(&scene, &cfg).unwrap();
                let (max_diff, mean_diff) = diff_stats(&seq_imgs[f as usize], &img);
                assert_eq!(
                    max_diff, 0,
                    "{order_name} order diverged for '{label}' frame {f} (max={max_diff}, mean={mean_diff:.3})"
                );
            }
        }
        seq_outputs.push(seq_imgs);
    }
    seq_outputs
}

#[test]
fn cpu_sequential_reverse_and_random_frame_order_are_bit_exact() {
    let cpu = TinySkiaBackend::headless();
    let outputs = render_order_corpus(&cpu);
    // Verify animation progression across frames (frame 0 vs frame 4 differs).
    for (kind, imgs) in outputs.iter().enumerate() {
        let (max_diff, _) = diff_stats(&imgs[0], &imgs[4]);
        assert!(
            max_diff > 10,
            "corpus scene {kind} frame 0 vs 4 should animate"
        );
    }
}

#[cfg(feature = "gpu")]
#[test]
fn wgpu_matches_cpu_and_is_order_deterministic() {
    let cpu = TinySkiaBackend::headless();
    let wgpu = rust_motion_rasterizer::WgpuBackend::new().expect("WGPU adapter required");
    let cpu_outputs = render_order_corpus(&cpu);
    let wgpu_outputs = render_order_corpus(&wgpu);

    for kind in 0..3 {
        let (label, _) = corpus_scene(kind, 0);
        let threshold = if kind == 0 { 1.5 } else { 2.0 };
        for f in 0..5 {
            let (max_diff, mean_diff) = diff_stats(&cpu_outputs[kind][f], &wgpu_outputs[kind][f]);
            assert!(
                mean_diff <= threshold,
                "WGPU vs CPU diverged on '{label}' frame {f}: max_diff={max_diff}, mean_abs_diff={mean_diff:.3} (limit {threshold})"
            );
        }
    }
}

#[cfg(all(target_os = "macos", feature = "metal"))]
#[test]
fn metal_matches_cpu_and_wgpu_and_is_order_deterministic() {
    let cpu = TinySkiaBackend::headless();
    let metal = rust_motion_rasterizer::MetalComputeExecutor::new()
        .expect("Metal device required on macOS");
    let cpu_outputs = render_order_corpus(&cpu);
    let metal_outputs = render_order_corpus(&metal);

    for kind in 0..3 {
        let (label, _) = corpus_scene(kind, 0);
        for f in 0..5 {
            let (max_diff, mean_diff) = diff_stats(&cpu_outputs[kind][f], &metal_outputs[kind][f]);
            assert!(
                max_diff <= 1,
                "Metal vs CPU diverged on '{label}' frame {f}: max_diff={max_diff}, mean_abs_diff={mean_diff:.3}"
            );
        }
    }

    #[cfg(feature = "gpu")]
    {
        let wgpu = rust_motion_rasterizer::WgpuBackend::new().expect("WGPU adapter required");
        let wgpu_outputs = render_order_corpus(&wgpu);
        for kind in 0..3 {
            let (label, _) = corpus_scene(kind, 0);
            for f in 0..5 {
                let (max_diff, mean_diff) =
                    diff_stats(&metal_outputs[kind][f], &wgpu_outputs[kind][f]);
                assert!(
                    mean_diff <= 2.0,
                    "Metal vs WGPU diverged on '{label}' frame {f}: max_diff={max_diff}, mean_abs_diff={mean_diff:.3}"
                );
            }
        }
    }
}
