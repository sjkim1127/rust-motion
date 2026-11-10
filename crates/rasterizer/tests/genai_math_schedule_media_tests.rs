//! Comprehensive integration tests for:
//! - #103 & #111: Generative Media Orchestration (OpenAI Sora, Runway Gen-3, WorldLabs 3D Gaussian Splats,
//!   Timeline Pre-fetch Scheduler, Distributed GPU Worker Cluster)
//! - #50: Deterministic Math Typesetting (`MathTex`) with Addressable Formula Parts
//! - #49: Deterministic Scene Play/Wait Authoring API (`SceneDirector` & `CompiledSceneSchedule`)
//! - #51: Linked Audio/Video Media Strips with Synchronized Trim/Retime and Independent Opacity/Gain

use rust_motion_rasterizer::backend::{FrameConfig, RasterizerBackend};
use rust_motion_rasterizer::genai_orchestrator::{
    apply_video_style_transfer, DistributedWorkerCluster, GaussianSplatCloud, GenAiPromptSpec,
    GenAiProviderModel, GpuWorkerNode, TimelinePrefetchScheduler,
};
use rust_motion_rasterizer::math_typeset::{
    parse_math_tex, typeset_math, MathPartStyle,
};
use rust_motion_rasterizer::play_wait_schedule::{
    LinkedMediaStrip, MediaStreamKind, ObjectAnimation, SceneDirector,
};
use rust_motion_rasterizer::scene::{Color, Scene, SceneNode};
use rust_motion_rasterizer::TinySkiaBackend;
use std::collections::HashMap;

#[test]
fn test_genai_clients_worldlabs_splats_prefetch_and_worker_cluster() {
    let sora_spec = GenAiPromptSpec {
        model: GenAiProviderModel::OpenAiSora,
        prompt: "Aerial sunset over ocean waves".into(),
        negative_prompt: Some("blurry, lowres".into()),
        width: 64,
        height: 36,
        duration_frames: 10,
        fps: 24.0,
        seed: 12345,
        timeline_in_frame: 20,
        style_transfer_strength: None,
    };
    let (url, payload) = sora_spec.build_api_payload();
    assert!(url.contains("openai.com/v1/video/generations"));
    assert_eq!(payload["model"], "sora-1.0-turbo");
    assert_eq!(sora_spec.cache_key(), sora_spec.cache_key());

    let splat_spec = GenAiPromptSpec {
        model: GenAiProviderModel::WorldLabsGaussianSplat,
        prompt: "Desert sunset canyon".into(),
        negative_prompt: None,
        width: 64,
        height: 36,
        duration_frames: 5,
        fps: 24.0,
        seed: 777,
        timeline_in_frame: 0,
        style_transfer_strength: None,
    };

    // Test 3D Gaussian Splat generation & depth-sorted software splatting
    let cloud = GaussianSplatCloud::generate_from_prompt("sunset canyon", 777, 64);
    assert_eq!(cloud.splats.len(), 64);
    let splat_pm = cloud.render_to_pixmap(64, 36, [0.0, 0.0, 0.0], 50.0);
    assert_eq!(splat_pm.width(), 64);
    assert_eq!(splat_pm.height(), 36);

    // Test Runway video-to-video style transfer
    let styled_pm = apply_video_style_transfer(&splat_pm, "cyberpunk neon", 0.7);
    assert_eq!(styled_pm.width(), 64);

    // Test TimelinePrefetchScheduler
    let mut scheduler = TimelinePrefetchScheduler::new();
    scheduler.register_prompt(splat_spec.clone());
    scheduler.register_prompt(sora_spec.clone());

    // Lookahead at frame 0..10 should only prefetch splat_spec (in_frame = 0), not sora_spec (in_frame = 20)
    let count1 = scheduler.prefetch_window(0, 5);
    assert!(count1 >= 5);
    assert!(scheduler.get_frame(&splat_spec, 2).is_some());
    assert!(scheduler.get_frame(&sora_spec, 22).is_none());

    // Lookahead at frame 15..25 should prefetch sora_spec frames 20..25
    let count2 = scheduler.prefetch_window(15, 10);
    assert!(count2 >= 5);
    assert!(scheduler.get_frame(&sora_spec, 22).is_some());

    // Test DistributedWorkerCluster routing and proportional VRAM frame partitioning
    let mut cluster = DistributedWorkerCluster::new(vec![
        GpuWorkerNode {
            id: "worker-a100".into(),
            endpoint: "http://10.0.0.1:8080".into(),
            vram_gb: 80,
            active_jobs: 1,
            healthy: true,
        },
        GpuWorkerNode {
            id: "worker-4090".into(),
            endpoint: "http://10.0.0.2:8080".into(),
            vram_gb: 24,
            active_jobs: 0,
            healthy: true,
        },
        GpuWorkerNode {
            id: "worker-offline".into(),
            endpoint: "http://10.0.0.3:8080".into(),
            vram_gb: 80,
            active_jobs: 0,
            healthy: false,
        },
    ]);

    // 48GB job can only run on worker-a100 (80GB)
    assert_eq!(cluster.select_worker(48).as_deref(), Some("worker-a100"));
    let partitions = cluster.partition_render_frames(104);
    assert_eq!(partitions.len(), 2);
    assert_eq!(partitions[0].start_frame, 0);
    assert_eq!(partitions.last().unwrap().end_frame, 104);
}

#[test]
fn test_math_typesetting_and_addressable_formula_parts() {
    let tex = r"\part{lhs}{E_0} = \part{rhs}{\frac{\sqrt{m^2 c^4 + p^2 c^2}}{\alpha}}";
    let ast = parse_math_tex(tex);
    let formula = typeset_math(&ast, 32.0);
    assert!(formula.width > 40.0);
    assert!(formula.height > 32.0);

    let lhs_bounds = formula.part_bounds("lhs").expect("lhs bounds exist");
    let rhs_bounds = formula.part_bounds("rhs").expect("rhs bounds exist");
    assert!(lhs_bounds[0] < rhs_bounds[0], "LHS is to the left of RHS");

    // Also test matrix and aligned environments
    let mat_ast = parse_math_tex(r"\begin{pmatrix} 1 & 0 \\ 0 & 1 \end{pmatrix}");
    let mat_layout = typeset_math(&mat_ast, 28.0);
    assert!(mat_layout.width > 20.0 && mat_layout.height > 40.0);

    let align_ast = parse_math_tex(r"\begin{aligned} f(x) &= x^2 + 1 \\ g(x) &= 2x \end{aligned}");
    let align_layout = typeset_math(&align_ast, 28.0);
    assert!(align_layout.height > 45.0);

    // Render formula with distinct styling on lhs vs rhs onto a CPU Pixmap
    let mut styles = HashMap::new();
    styles.insert(
        "lhs".to_string(),
        MathPartStyle {
            color: Some(Color::rgb(255, 80, 80)),
            opacity: 1.0,
            scale: 1.2,
            offset_x: 0.0,
            offset_y: 0.0,
        },
    );
    styles.insert(
        "rhs".to_string(),
        MathPartStyle {
            color: Some(Color::rgb(80, 200, 255)),
            opacity: 0.9,
            scale: 1.0,
            offset_x: 4.0,
            offset_y: 0.0,
        },
    );

    let mut scene = Scene::new();
    scene.push(SceneNode::Rect {
        x: 0.0,
        y: 0.0,
        w: 320.0,
        h: 160.0,
        fill: Color::rgb(15, 20, 30),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    });
    formula.emit_to_scene(&mut scene, 24.0, 32.0, Color::rgb(255, 255, 255), &styles);

    let backend = TinySkiaBackend::new();
    let img = backend
        .render_frame(&scene, &FrameConfig::new(320, 160, 0, 30.0))
        .expect("render math formula");
    assert_eq!(img.width(), 320);
    assert_eq!(img.height(), 160);
}

#[test]
fn test_deterministic_play_wait_scene_director_random_frame_order() {
    let mut director = SceneDirector::new();
    director
        .add(
            "box",
            SceneNode::Rect {
                x: 20.0,
                y: 20.0,
                w: 60.0,
                h: 60.0,
                fill: Color::rgb(240, 80, 80),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 4.0,
            },
        )
        .add(
            "orb",
            SceneNode::Circle {
                cx: 50.0,
                cy: 100.0,
                r: 20.0,
                fill: Color::rgb(80, 180, 250),
                stroke: None,
                stroke_width: 0.0,
            },
        )
        .play_parallel(
            vec![
                ObjectAnimation::MoveTo {
                    object_id: "box".into(),
                    target_x: 120.0,
                    target_y: 40.0,
                },
                ObjectAnimation::ScaleTo {
                    object_id: "orb".into(),
                    scale_factor: 1.5,
                },
            ],
            20,
        )
        .wait(10)
        .play_sequential(vec![
            (
                ObjectAnimation::ColorTo {
                    object_id: "box".into(),
                    target_color: Color::rgb(80, 240, 120),
                },
                10,
            ),
            (
                ObjectAnimation::Fade {
                    object_id: "orb".into(),
                    from: 1.0,
                    to: 0.2,
                },
                10,
            ),
        ])
        .remove("orb");

    let schedule = director.compile();
    assert_eq!(schedule.total_frames, 50);

    let backend = TinySkiaBackend::new();
    let sample_frames = [0u32, 10, 20, 25, 35, 45, 50];

    // Render in ascending order
    let asc_renders: Vec<Vec<u8>> = sample_frames
        .iter()
        .map(|&f| {
            let sc = schedule.evaluate_frame(f);
            backend
                .render_frame(&sc, &FrameConfig::new(240, 160, f, 30.0))
                .unwrap()
                .into_raw()
        })
        .collect();

    // Render in reverse (descending) order and verify bit-identical pixel buffers
    for (idx, &f) in sample_frames.iter().enumerate().rev() {
        let sc = schedule.evaluate_frame(f);
        let raw = backend
            .render_frame(&sc, &FrameConfig::new(240, 160, f, 30.0))
            .unwrap()
            .into_raw();
        assert_eq!(
            raw, asc_renders[idx],
            "Frame {f} pixels differed between ascending and descending evaluation"
        );
    }

    // At frame 50, "orb" was removed so only "box" remains
    assert_eq!(schedule.evaluate_frame(50).nodes.len(), 1);
}

#[test]
fn test_linked_audio_video_media_strip_sync_and_independence() {
    let mut strip = LinkedMediaStrip::new(
        "clip-1",
        "interview.mp4",
        MediaStreamKind::VideoWithAudio,
        10,
        90,
        1920.0,
        1080.0,
    )
    .expect("create linked media strip");

    // 1. Trim in by +15 frames: source_in = 15, timeline_start = 25, duration = 75
    strip.trim_in(15).expect("trim in");
    assert_eq!(strip.source_in_frame, 15);
    assert_eq!(strip.timeline_start_frame, 25);
    assert_eq!(strip.duration_frames, 75);

    // 2. Move to timeline frame 40: source_in stays 15
    strip.move_to(40);
    assert_eq!(strip.timeline_start_frame, 40);
    assert!((strip.source_time_sec_at(40, 30.0).unwrap() - 0.5).abs() < 1e-6);

    // 3. Retime to 1.5x speed: 75 source frames -> 50 timeline frames
    strip.set_playback_rate(1.5).expect("retime 1.5x");
    assert_eq!(strip.duration_frames, 50);
    // At timeline frame 50 (10 frames into clip at 1.5x = +15 source frames -> source frame 30 = 1.0s)
    assert!((strip.source_time_sec_at(50, 30.0).unwrap() - 1.0).abs() < 1e-6);

    // 4. Muting audio does not hide video; changing visual opacity does not change audio gain
    strip.visual_opacity = 0.5;
    strip.audio_gain_db = -6.0;
    let vol_unmuted = strip.effective_audio_volume();
    assert!((vol_unmuted - 0.501187).abs() < 1e-3);

    strip.audio_muted = true;
    assert_eq!(strip.effective_audio_volume(), 0.0);

    let mut scene = Scene::new();
    strip.emit_to_scene(&mut scene, 50, 30.0);
    assert_eq!(
        scene.nodes.len(),
        2,
        "Both Video node and Audio node emitted into scene"
    );
    let tracks = scene.audio_tracks();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].volume, 0.0);

    // 5. Waveform peak generation
    strip.audio_muted = false;
    let pcm = vec![0.2f32, -0.8, 0.4, -0.4, 1.0, -0.5, 0.1, 0.2];
    let peaks = strip.compute_waveform_peaks(&pcm, 4);
    assert_eq!(peaks.len(), 4);
    assert!(peaks[2] > peaks[0]);
}
