#![cfg(feature = "rhai")]

use rust_motion_cli::{execute_render_command, RenderBackend, RenderRequest};
use std::fs;
use std::path::PathBuf;

#[tokio::test]
async fn rhai_example_renders_a_real_mp4() {
    if std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_err()
    {
        eprintln!("Skipping Rhai render integration test: ffmpeg is not installed");
        return;
    }

    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp_dir = std::env::temp_dir().join(format!(
        "rust_motion_rhai_render_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    let output = temp_dir.join("rhai.mp4");
    let request = RenderRequest {
        composition: None,
        script: Some(workspace.join("examples/hello.rhai")),
        props: Some(workspace.join("examples/hello-props.json")),
        output: output.clone(),
        audio: Vec::new(),
        width: 64,
        height: 64,
        scale: 1.0,
        fps: 30.0,
        duration: 3,
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
    };

    execute_render_command(&request).await.unwrap();
    let bytes = fs::read(&output).unwrap();
    assert!(bytes.len() > 32);
    assert!(bytes.windows(4).any(|window| window == b"ftyp"));
    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_untrusted_script_defaults_to_script_dir_without_cwd() {
    use rust_motion_rasterizer::MediaSecurityPolicy;
    use std::env;

    let temp_parent =
        env::temp_dir().join(format!("rust_motion_sandbox_test_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_parent);
    let canon_parent = temp_parent
        .canonicalize()
        .unwrap_or_else(|_| temp_parent.clone());
    let script_file = canon_parent.join("script.rhai");

    let request = RenderRequest {
        composition: None,
        script: Some(script_file.clone()),
        props: None,
        output: PathBuf::from("out.mp4"),
        audio: Vec::new(),
        width: 100,
        height: 100,
        scale: 1.0,
        fps: 30.0,
        duration: 30,
        backend: RenderBackend::Native,
        codec: rust_motion_cli::RenderCodec::H264,
        frame_start: 0,
        frame_end: None,
        frame_step: 1,
        concurrency: None,
        timeout_seconds: None,
        crf: 18,
        preset: "fast".into(),
        hw_accel: rust_motion_rasterizer::HwAccel::Auto,
        sandbox_roots: Vec::new(),
        permissive: false,
    };

    let policy = request.effective_security_policy();
    match policy {
        MediaSecurityPolicy::Sandboxed { allowed_roots } => {
            let cwd = env::current_dir().unwrap().canonicalize().unwrap();
            // Should contain script_dir
            assert!(allowed_roots.iter().any(|r| r == &canon_parent));
            // Should NOT automatically include cwd if canon_parent != cwd
            if !canon_parent.starts_with(&cwd) && !cwd.starts_with(&canon_parent) {
                assert!(
                    !allowed_roots.contains(&cwd),
                    "CWD should NOT be leaked into sandbox default"
                );
            }
        }
        _ => panic!("Expected MediaSecurityPolicy::Sandboxed"),
    }
    let _ = fs::remove_dir_all(&temp_parent);
}
