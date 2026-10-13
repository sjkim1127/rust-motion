use rust_motion_rasterizer::{
    render_to_ffmpeg_pipe, AudioTrack, ImageFit, PipeConfig, Scene, SceneNode, TinySkiaBackend,
};
use std::fs;
use std::process::Command;

#[cfg(feature = "rhai")]
use rust_motion_cli::{execute_render_command, RenderBackend, RenderRequest};

fn command_available(name: &str) -> bool {
    Command::new(name)
        .arg("-version")
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn native_video_frames_and_audio_are_encoded_together() {
    if !command_available("ffmpeg") || !command_available("ffprobe") {
        eprintln!("Skipping native media integration test: FFmpeg tools are unavailable");
        return;
    }

    let temp_dir = std::env::temp_dir().join(format!(
        "rust_motion_native_media_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    let source = temp_dir.join("source.mp4");
    let output = temp_dir.join("output.mp4");

    let generated = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=64x64:rate=5:duration=1",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=1",
            "-shortest",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
        ])
        .arg(&source)
        .status()
        .unwrap();
    assert!(generated.success(), "failed to generate source media");

    let mut audio = AudioTrack::new(source.display().to_string());
    audio.duration = Some(0.2);
    let config = PipeConfig::new(64, 64, 5.0, 5, &output)
        .with_concurrency(2)
        .with_audio_tracks([audio]);
    let source_for_frames = source.display().to_string();
    render_to_ffmpeg_pipe(&TinySkiaBackend::headless(), &config, move |frame| Scene {
        nodes: vec![SceneNode::Video {
            src: source_for_frames.clone(),
            time: frame as f64 / 5.0,
            looped: false,
            x: 0.0,
            y: 0.0,
            w: 64.0,
            h: 64.0,
            fit: ImageFit::Cover,
            opacity: 1.0,
        }],
    })
    .unwrap();

    let streams = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "csv=p=0",
        ])
        .arg(&output)
        .output()
        .unwrap();
    assert!(streams.status.success());
    let streams = String::from_utf8_lossy(&streams.stdout);
    assert!(streams.lines().any(|line| line == "video"));
    assert!(streams.lines().any(|line| line == "audio"));
    assert!(fs::metadata(&output).unwrap().len() > 1_000);

    let duration = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(&output)
        .output()
        .unwrap();
    let duration: f64 = String::from_utf8_lossy(&duration.stdout)
        .trim()
        .parse()
        .unwrap();
    assert!(
        duration >= 0.9,
        "short audio truncated video to {duration}s"
    );

    fs::remove_dir_all(temp_dir).unwrap();
}

#[cfg(feature = "rhai")]
#[tokio::test]
async fn rhai_media_nodes_flow_through_the_cli_renderer() {
    if !command_available("ffmpeg") || !command_available("ffprobe") {
        eprintln!("Skipping Rhai media integration test: FFmpeg tools are unavailable");
        return;
    }

    let temp_dir = std::env::temp_dir().join(format!(
        "rust_motion_rhai_media_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    let source = temp_dir.join("source.mp4");
    let script = temp_dir.join("media.rhai");
    let props = temp_dir.join("props.json");
    let output = temp_dir.join("rhai-output.mp4");

    let generated = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:size=64x64:rate=5:duration=1",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:sample_rate=48000:duration=1",
            "-shortest",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
        ])
        .arg(&source)
        .status()
        .unwrap();
    assert!(generated.success());

    fs::write(
        &script,
        r#"
            fn render(ctx, props) {
                let output = scene();
                output.video(0.0, 0.0, ctx.width.to_float(), ctx.height.to_float(), props.src, ctx.frame.to_float() / ctx.fps, "cover", 1.0);
                output.audio(props.src, 0.0, 0.0, 0.0, 1.0, 1.0, false);
                output
            }
        "#,
    )
    .unwrap();
    fs::write(
        &props,
        serde_json::json!({"src": source.display().to_string()}).to_string(),
    )
    .unwrap();

    execute_render_command(&RenderRequest {
        composition: None,
        script: Some(script),
        props: Some(props),
        output: output.clone(),
        audio: Vec::new(),
        width: 64,
        height: 64,
        scale: 1.0,
        fps: 5.0,
        duration: 5,
        backend: RenderBackend::Native,
        codec: rust_motion_cli::RenderCodec::H264,
        frame_start: 0,
        frame_end: None,
        frame_step: 1,
        concurrency: None,
        timeout_seconds: None,
        crf: 18,
        preset: "fast".into(),
        hw_accel: rust_motion_rasterizer::HwAccel::Disabled,
        sandbox_roots: Vec::new(),
        permissive: false,
    })
    .await
    .unwrap();

    let streams = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "csv=p=0",
        ])
        .arg(&output)
        .output()
        .unwrap();
    let streams = String::from_utf8_lossy(&streams.stdout);
    assert!(streams.lines().any(|line| line == "video"));
    assert!(streams.lines().any(|line| line == "audio"));

    fs::remove_dir_all(temp_dir).unwrap();
}
