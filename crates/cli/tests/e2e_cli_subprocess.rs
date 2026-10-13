//! E2E Subprocess Tests
//!
//! These tests build and invoke the real `rust-motion` CLI binary as a child process,
//! verifying end-to-end output correctness without calling any internal library functions.
//!
//! Each test:
//!   1. Spawns `rust-motion render …` as a subprocess via `std::process::Command`
//!   2. Asserts the process exit code
//!   3. Inspects the produced file's byte headers and/or runs `ffprobe` for codec metadata

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Returns the path to the compiled `rust-motion` CLI binary.
///
/// Cargo injects `CARGO_BIN_EXE_rust_motion` automatically when the `[[bin]]` target
/// is declared in the same package's `Cargo.toml`. Falls back to `target/debug/rust-motion`
/// for manual runs outside the test harness.
fn cli_bin() -> PathBuf {
    std::env::var("CARGO_BIN_EXE_rust_motion")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            manifest.join("../..").join("target/debug/rust-motion")
        })
}

/// Returns a unique temporary directory for the test.
fn tmp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rust_motion_e2e_{}_{}", label, nanos));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Returns `true` if `ffprobe` is available on `$PATH`.
fn ffprobe_available() -> bool {
    Command::new("ffprobe")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ── Positive path tests ───────────────────────────────────────────────────────

/// Full pipeline: HelloWorld composition → MP4 via CPU rasterizer.
/// Asserts exit code 0 and valid `ftyp` MP4 container header.
#[test]
fn e2e_hello_world_mp4() {
    let dir = tmp_dir("hello_world_mp4");
    let output = dir.join("out.mp4");

    let status = Command::new(cli_bin())
        .args([
            "render",
            "--composition",
            "HelloWorld",
            "--output",
            output.to_str().unwrap(),
            "--width",
            "640",
            "--height",
            "360",
            "--fps",
            "30",
            "--duration",
            "30",
            "--codec",
            "h264",
            "--preset",
            "ultrafast",
            "--crf",
            "28",
        ])
        .status()
        .expect("Failed to spawn rust-motion CLI");

    assert!(
        status.success(),
        "rust-motion render exited with non-zero status: {status}"
    );
    assert!(output.exists(), "Output MP4 was not created at {output:?}");

    let bytes = fs::read(&output).unwrap();
    assert!(
        bytes.len() > 12,
        "Output MP4 is too small to contain headers"
    );
    assert_eq!(
        &bytes[4..8],
        b"ftyp",
        "MP4 file missing 'ftyp' atom — not a valid MPEG-4 container"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Full pipeline: HelloWorld composition → GIF output.
/// Asserts exit code 0 and valid `GIF89a` magic header.
#[test]
fn e2e_gif_output() {
    let dir = tmp_dir("gif_output");
    let output = dir.join("out.gif");

    let status = Command::new(cli_bin())
        .args([
            "render",
            "--composition",
            "HelloWorld",
            "--output",
            output.to_str().unwrap(),
            "--width",
            "320",
            "--height",
            "180",
            "--fps",
            "15",
            "--duration",
            "15",
            "--codec",
            "gif",
        ])
        .status()
        .expect("Failed to spawn rust-motion CLI");

    assert!(
        status.success(),
        "rust-motion gif render exited with non-zero status: {status}"
    );
    assert!(output.exists(), "Output GIF was not created at {output:?}");

    let bytes = fs::read(&output).unwrap();
    assert!(bytes.len() > 6, "Output GIF is too small");
    assert_eq!(
        &bytes[0..6],
        b"GIF89a",
        "GIF file missing 'GIF89a' magic header"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Single-frame still: HelloWorld composition → PNG.
/// Asserts exit code 0 and valid `\x89PNG` magic header.
#[test]
fn e2e_still_png_output() {
    let dir = tmp_dir("still_png");
    let output = dir.join("frame.png");

    let status = Command::new(cli_bin())
        .args([
            "render",
            "--composition",
            "HelloWorld",
            "--output",
            output.to_str().unwrap(),
            "--width",
            "640",
            "--height",
            "360",
            "--fps",
            "30",
            "--duration",
            "30",
            "--codec",
            "png",
            "--frame-start",
            "0",
        ])
        .status()
        .expect("Failed to spawn rust-motion CLI");

    assert!(
        status.success(),
        "rust-motion png render exited with non-zero status: {status}"
    );
    assert!(output.exists(), "Output PNG was not created at {output:?}");

    let bytes = fs::read(&output).unwrap();
    assert!(bytes.len() > 8, "Output PNG is too small");
    assert_eq!(
        &bytes[0..8],
        &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
        "PNG file missing PNG magic header"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// ffprobe metadata validation: Rendered MP4 must report:
///   - `codec_name` = "h264"
///   - `width` = 640, `height` = 360
///   - at least 1 video stream
///
/// Skipped automatically if `ffprobe` is not on PATH.
#[test]
fn e2e_ffprobe_video_stream_metadata() {
    if !ffprobe_available() {
        eprintln!("Skipping e2e_ffprobe_video_stream_metadata: ffprobe not found on PATH");
        return;
    }

    let dir = tmp_dir("ffprobe_meta");
    let output = dir.join("probe.mp4");

    let render_status = Command::new(cli_bin())
        .args([
            "render",
            "--composition",
            "HelloWorld",
            "--output",
            output.to_str().unwrap(),
            "--width",
            "640",
            "--height",
            "360",
            "--fps",
            "30",
            "--duration",
            "30",
            "--codec",
            "h264",
            "--preset",
            "ultrafast",
            "--crf",
            "28",
        ])
        .status()
        .expect("Failed to spawn rust-motion CLI for ffprobe test");

    assert!(
        render_status.success(),
        "rust-motion render failed before ffprobe could run: {render_status}"
    );

    let ffprobe_out = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_streams",
            output.to_str().unwrap(),
        ])
        .output()
        .expect("ffprobe command failed to execute");

    assert!(
        ffprobe_out.status.success(),
        "ffprobe exited with error: {}",
        String::from_utf8_lossy(&ffprobe_out.stderr)
    );

    let json_str = String::from_utf8_lossy(&ffprobe_out.stdout);
    let probe: serde_json::Value =
        serde_json::from_str(&json_str).expect("ffprobe output is not valid JSON");

    let streams = probe["streams"]
        .as_array()
        .expect("ffprobe JSON missing 'streams' array");

    assert!(
        !streams.is_empty(),
        "ffprobe found 0 streams in rendered MP4"
    );

    let video_stream = streams
        .iter()
        .find(|s| s["codec_type"].as_str() == Some("video"))
        .expect("No video stream found in rendered MP4");

    assert_eq!(
        video_stream["codec_name"].as_str(),
        Some("h264"),
        "Expected h264 codec, got: {}",
        video_stream["codec_name"]
    );
    assert_eq!(
        video_stream["width"].as_u64(),
        Some(640),
        "Expected width=640, got: {}",
        video_stream["width"]
    );
    assert_eq!(
        video_stream["height"].as_u64(),
        Some(360),
        "Expected height=360, got: {}",
        video_stream["height"]
    );

    let _ = fs::remove_dir_all(&dir);
}

// ── Negative path tests (must exit non-zero) ──────────────────────────────────

/// Mismatched codec/extension: `--codec h264` with `.gif` output must fail validation.
#[test]
fn e2e_invalid_codec_extension_fails() {
    let dir = tmp_dir("invalid_ext");
    let output = dir.join("out.gif"); // wrong extension for h264

    let status = Command::new(cli_bin())
        .args([
            "render",
            "--composition",
            "HelloWorld",
            "--output",
            output.to_str().unwrap(),
            "--width",
            "640",
            "--height",
            "360",
            "--fps",
            "30",
            "--duration",
            "30",
            "--codec",
            "h264",
        ])
        .status()
        .expect("Failed to spawn rust-motion CLI");

    assert!(
        !status.success(),
        "Expected non-zero exit for codec/extension mismatch, but got success"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Zero duration must be rejected before FFmpeg is spawned.
#[test]
fn e2e_zero_duration_fails() {
    let dir = tmp_dir("zero_duration");
    let output = dir.join("out.mp4");

    let status = Command::new(cli_bin())
        .args([
            "render",
            "--composition",
            "HelloWorld",
            "--output",
            output.to_str().unwrap(),
            "--width",
            "640",
            "--height",
            "360",
            "--fps",
            "30",
            "--duration",
            "0",
        ])
        .status()
        .expect("Failed to spawn rust-motion CLI");

    assert!(
        !status.success(),
        "Expected non-zero exit for --duration 0, but got success"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Unknown composition ID must produce a descriptive error and non-zero exit code.
#[test]
fn e2e_unknown_composition_fails() {
    let dir = tmp_dir("unknown_comp");
    let output = dir.join("out.mp4");

    let status = Command::new(cli_bin())
        .args([
            "render",
            "--composition",
            "ThisCompositionDoesNotExist",
            "--output",
            output.to_str().unwrap(),
            "--width",
            "640",
            "--height",
            "360",
            "--fps",
            "30",
            "--duration",
            "30",
        ])
        .status()
        .expect("Failed to spawn rust-motion CLI");

    assert!(
        !status.success(),
        "Expected non-zero exit for unknown composition, but got success"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Odd-dimension resolution must be rejected for H.264 encoding.
#[test]
fn e2e_odd_resolution_fails() {
    let dir = tmp_dir("odd_res");
    let output = dir.join("out.mp4");

    let status = Command::new(cli_bin())
        .args([
            "render",
            "--composition",
            "HelloWorld",
            "--output",
            output.to_str().unwrap(),
            "--width",
            "641",
            "--height",
            "360",
            "--fps",
            "30",
            "--duration",
            "30",
            "--codec",
            "h264",
        ])
        .status()
        .expect("Failed to spawn rust-motion CLI");

    assert!(
        !status.success(),
        "Expected non-zero exit for odd resolution with h264, but got success"
    );

    let _ = fs::remove_dir_all(&dir);
}
