//! Section 10 End-to-End Unified Creative Engine Milestone Test:
//! 3D geometry + procedural HDR landscape + PBR materials + deterministic particle
//! simulation + 2D compositing + audio synchronization + CPU/GPU rendering + zero-FFmpeg MP4 export.

use std::sync::Arc;

use rust_motion_media_container::{probe_isobmff_path, Mp4Demuxer};
use rust_motion_media_core::VideoCodecId;
use rust_motion_rasterizer::backend::{FrameConfig, RasterizerBackend};
use rust_motion_rasterizer::mesh3d::{Mesh3D, Vec3};
use rust_motion_rasterizer::procedural_scenes::{
    render_procedural_scene_pixmap, ProceduralSceneType,
};
use rust_motion_rasterizer::render::{
    render_to_video, EncoderBackend, HwAccel, PipeConfig, VideoCodec,
};
use rust_motion_rasterizer::scene3d::{
    Geometry3D, Material3D, Node3D, Scene3D, Scene3DBackground, Scene3DCamera, Scene3DLight,
    Transform3D,
};
use rust_motion_rasterizer::{
    AudioTrack, BlendMode, Color, ConfettiEmitter, ConfettiShape, ImageFit, MaskMode, Scene,
    SceneFilter, SceneNode, TinySkiaBackend,
};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;
const FPS: f64 = 30.0;
const FRAMES: u32 = 12;

fn write_test_wav_sine(path: &std::path::Path, sample_rate: u32, duration_secs: f32) {
    let num_samples = (sample_rate as f32 * duration_secs) as u32;
    let channels: u16 = 2;
    let bits_per_sample: u16 = 16;
    let data_size = num_samples * u32::from(channels) * u32::from(bits_per_sample / 8);
    let riff_size = 36 + data_size;
    let mut buf = Vec::with_capacity((44 + data_size) as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&riff_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample / 8);
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    let block_align = channels * (bits_per_sample / 8);
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());
    for i in 0..num_samples {
        let t = i as f32 / sample_rate as f32;
        let sample = (t * 523.25 * std::f32::consts::TAU).sin() * 0.25;
        let pcm = (sample * i16::MAX as f32) as i16;
        buf.extend_from_slice(&pcm.to_le_bytes());
        buf.extend_from_slice(&pcm.to_le_bytes());
    }
    std::fs::write(path, buf).unwrap();
}

fn custom_pyramid_mesh() -> Arc<Mesh3D> {
    let vertices = vec![
        Vec3::new(-18.0, -14.0, -18.0),
        Vec3::new(18.0, -14.0, -18.0),
        Vec3::new(18.0, -14.0, 18.0),
        Vec3::new(-18.0, -14.0, 18.0),
        Vec3::new(0.0, 22.0, 0.0),
    ];
    let faces = vec![
        vec![0, 1, 4],
        vec![1, 2, 4],
        vec![2, 3, 4],
        vec![3, 0, 4],
        vec![3, 2, 1, 0],
    ];
    Arc::new(Mesh3D::new(vertices, faces))
}

fn build_unified_scene(
    frame: u32,
    backdrop_path: &std::path::Path,
    emitter: &ConfettiEmitter,
) -> Scene {
    let t = frame as f32 / FPS as f32;

    // 1. 3D scene with procedural geometry, custom Mesh3D hierarchy, PBR materials & lights
    let mut scene3d = Scene3D::new()
        .with_camera(Scene3DCamera::perspective(
            Vec3::new(0.0, 35.0, 180.0),
            Vec3::new(0.0, 0.0, 0.0),
            48.0,
        ))
        .with_background(Scene3DBackground::Transparent);
    scene3d.add_light(Scene3DLight::ambient(Color::WHITE, 0.35));
    scene3d.add_light(Scene3DLight::directional(
        Vec3::new(0.6, 1.0, -0.8),
        Color::rgb(255, 245, 225),
        0.95,
    ));

    let mut hero = Node3D::new(
        Geometry3D::Torus {
            r_major: 38.0,
            r_minor: 9.0,
            segs_major: 24,
            segs_minor: 12,
        },
        Material3D::pbr(Color::rgb(70, 190, 245), 0.75, 0.25),
    )
    .with_id("hero_torus")
    .with_transform(Transform3D::from_euler(0.45 + t * 0.8, t * 1.4, 0.0));

    hero.add_child(
        Node3D::new(
            Geometry3D::Mesh(custom_pyramid_mesh()),
            Material3D::pbr(Color::rgb(245, 130, 65), 0.2, 0.45),
        )
        .with_id("pyramid_core")
        .with_transform(Transform3D::from_translation(Vec3::new(0.0, 4.0, 0.0))),
    );
    scene3d.add_node(hero);

    scene3d.add_node(
        Node3D::new(
            Geometry3D::Sphere {
                radius: 14.0,
                rings: 12,
                sectors: 16,
            },
            Material3D::pbr(Color::rgb(130, 240, 145), 0.1, 0.35),
        )
        .with_id("orbit_sphere")
        .with_transform(Transform3D::from_translation(Vec3::new(
            (t * 3.0).cos() * 68.0,
            (t * 3.0).sin() * 26.0,
            0.0,
        ))),
    );

    let mut scene = Scene::new();
    // 2. Procedural tonemapped backdrop composited with Image node
    scene.push(SceneNode::Image {
        src: backdrop_path.to_string_lossy().into_owned(),
        x: 0.0,
        y: 0.0,
        w: WIDTH as f32,
        h: HEIGHT as f32,
        fit: ImageFit::Cover,
        opacity: 1.0,
    });
    // 3. 3D layer composited over procedural background
    scene.push(SceneNode::Scene3D {
        x: 0.0,
        y: 0.0,
        w: WIDTH as f32,
        h: HEIGHT as f32,
        scene: Box::new(scene3d),
        opacity: 1.0,
    });
    // 4. Deterministic particle simulation overlay
    for node in emitter.render_frame(frame as usize, FPS as f32) {
        scene.push(node);
    }
    // 5. Filtered 2D HUD overlay
    scene.push(SceneNode::Layer {
        opacity: 0.85,
        blend_mode: BlendMode::Normal,
        clip: None,
        mask: None,
        mask_mode: MaskMode::Alpha,
        filters: vec![SceneFilter::Blur { sigma: 1.5 }],
        shadow: None,
        children: vec![SceneNode::Rect {
            x: 12.0,
            y: 196.0,
            w: 140.0,
            h: 30.0,
            fill: Color::rgba(15, 23, 42, 210),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 6.0,
        }],
    });
    scene.push(SceneNode::Text {
        x: 20.0,
        y: 215.0,
        content: format!("UNIFIED {frame:02}"),
        font_size: 13.0,
        color: Color::WHITE,
        font_weight: 700,
        font_sources: Vec::new(),
    });
    scene
}

#[test]
fn unified_3d_procedural_sim_compositing_audio_mp4_e2e() {
    let temp_dir = tempfile::tempdir().unwrap();
    let backdrop_path = temp_dir.path().join("aurora_backdrop.png");
    let wav_path = temp_dir.path().join("cue.wav");
    let cpu_mp4_path = temp_dir.path().join("unified_cpu_h264.mp4");

    // Generate deterministic procedural HDR Aurora backdrop (opaque RGBA -> safe direct write).
    let pm = render_procedural_scene_pixmap(ProceduralSceneType::Aurora, 160, 120, 7, 0.5, 0.0);
    let img = image::RgbaImage::from_raw(pm.width(), pm.height(), pm.data().to_vec()).unwrap();
    img.save(&backdrop_path).unwrap();
    write_test_wav_sine(&wav_path, 48_000, 1.0);

    let mut emitter = ConfettiEmitter::new(WIDTH as f32 * 0.5, HEIGHT as f32 * 0.4)
        .with_count(24)
        .with_velocity(180.0)
        .with_colors(vec![Color::rgb(255, 200, 80), Color::rgb(90, 220, 255)]);
    emitter.shapes = vec![ConfettiShape::Circle, ConfettiShape::Rectangle];
    emitter.seed = 42;
    emitter.gravity = 240.0;

    let cpu = TinySkiaBackend::headless();
    let f0 = cpu
        .render_frame(
            &build_unified_scene(0, &backdrop_path, &emitter),
            &FrameConfig::new(WIDTH, HEIGHT, 0, FPS),
        )
        .unwrap();
    let f4_a = cpu
        .render_frame(
            &build_unified_scene(4, &backdrop_path, &emitter),
            &FrameConfig::new(WIDTH, HEIGHT, 4, FPS),
        )
        .unwrap();
    let f4_b = cpu
        .render_frame(
            &build_unified_scene(4, &backdrop_path, &emitter),
            &FrameConfig::new(WIDTH, HEIGHT, 4, FPS),
        )
        .unwrap();
    assert_eq!(
        f4_a.as_raw(),
        f4_b.as_raw(),
        "CPU rendering must be bit-exact deterministic"
    );
    assert_ne!(
        f0.as_raw(),
        f4_a.as_raw(),
        "Animation + simulation must advance across frames"
    );

    let audio_track = AudioTrack {
        src: wav_path.to_string_lossy().into_owned(),
        start_from: 0.0,
        timeline_start: 0.0,
        duration: Some(1.0),
        volume: 0.85,
        playback_rate: 1.0,
        looped: false,
        volume_keyframes: Vec::new(),
    };

    // Pure-Rust CPU + H.264 native MP4 export (zero FFmpeg)
    let mut cpu_pipe = PipeConfig::new(WIDTH, HEIGHT, FPS, FRAMES, &cpu_mp4_path)
        .with_codec(VideoCodec::H264)
        .with_hw_accel(HwAccel::Disabled)
        .with_encoder_backend(EncoderBackend::Native);
    if rust_motion_media_hwaccel::is_audiotoolbox_available() {
        cpu_pipe = cpu_pipe.with_audio_tracks(vec![audio_track.clone()]);
    }
    render_to_video(&cpu, &cpu_pipe, |frame| {
        build_unified_scene(frame, &backdrop_path, &emitter)
    })
    .unwrap();

    let cpu_probe = probe_isobmff_path(&cpu_mp4_path).unwrap();
    assert_eq!(cpu_probe.video_streams[0].width, WIDTH);
    assert_eq!(cpu_probe.video_streams[0].height, HEIGHT);
    assert_eq!(cpu_probe.video_streams[0].codec, VideoCodecId::H264);
    let cpu_demux = Mp4Demuxer::open(std::fs::File::open(&cpu_mp4_path).unwrap()).unwrap();
    assert_eq!(cpu_demux.video_tracks()[0].samples.len(), FRAMES as usize);

    // Zero-copy Apple Silicon Metal + VideoToolbox HEVC + AudioToolbox AAC MP4 export
    #[cfg(all(target_os = "macos", feature = "metal"))]
    {
        use rust_motion_media_core::AudioCodecId;
        let metal_mp4_path = temp_dir.path().join("unified_metal_hevc_av.mp4");
        let metal = rust_motion_rasterizer::MetalComputeExecutor::new().unwrap();
        let metal_pipe = PipeConfig::new(WIDTH, HEIGHT, FPS, FRAMES, &metal_mp4_path)
            .with_codec(VideoCodec::H265)
            .with_hw_accel(HwAccel::VideoToolbox)
            .with_encoder_backend(EncoderBackend::Native)
            .with_audio_tracks(vec![audio_track]);
        render_to_video(&metal, &metal_pipe, |frame| {
            build_unified_scene(frame, &backdrop_path, &emitter)
        })
        .unwrap();

        assert_eq!(metal.zero_copy_frames(), u64::from(FRAMES));
        assert_eq!(metal.render_stats().gpu_frames, u64::from(FRAMES));

        let metal_probe = probe_isobmff_path(&metal_mp4_path).unwrap();
        assert_eq!(metal_probe.video_streams[0].width, WIDTH);
        assert_eq!(metal_probe.video_streams[0].height, HEIGHT);
        assert_eq!(metal_probe.video_streams[0].codec, VideoCodecId::Hevc);
        assert_eq!(metal_probe.audio_streams[0].codec, AudioCodecId::Aac);
        assert_eq!(metal_probe.audio_streams[0].sample_rate, 48_000);
        assert_eq!(metal_probe.audio_streams[0].channels, 2);

        let metal_demux = Mp4Demuxer::open(std::fs::File::open(&metal_mp4_path).unwrap()).unwrap();
        assert_eq!(metal_demux.video_tracks()[0].samples.len(), FRAMES as usize);

        if std::process::Command::new("ffprobe")
            .arg("-version")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            let out = std::process::Command::new("ffprobe")
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "stream=codec_type",
                    "-of",
                    "csv=p=0",
                ])
                .arg(&metal_mp4_path)
                .output()
                .unwrap();
            let s = String::from_utf8_lossy(&out.stdout);
            assert!(s.lines().any(|l| l == "video"));
            assert!(s.lines().any(|l| l == "audio"));
        }
    }
}
