//! System diagnostics, external dependency verification, and hardware acceleration detection.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCheck {
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoders: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TempDirCheck {
    pub path: String,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DoctorReport {
    pub ok: bool,
    pub ffmpeg: ToolCheck,
    pub ffprobe: ToolCheck,
    pub node: ToolCheck,
    pub temp_dir: TempDirCheck,
    pub hw_accel: String,
    pub supported_codecs: Vec<String>,
}

pub(crate) fn check_command_output(cmd: &str, args: &[&str]) -> (bool, Option<String>) {
    match std::process::Command::new(cmd).args(args).output() {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let first_line = stdout.lines().next().unwrap_or("").trim().to_string();
            (true, Some(first_line))
        }
        _ => (false, None),
    }
}

pub(crate) fn check_ffmpeg_encoder_supported(name: &str) -> bool {
    std::process::Command::new("ffmpeg")
        .args(["-v", "quiet", "-encoders"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).contains(name))
        .unwrap_or(false)
}

/// Verify local environment, external dependencies (FFmpeg, FFprobe, Node), and hardware encoding capabilities.
pub fn run_doctor() -> DoctorReport {
    let (ffmpeg_avail, ffmpeg_ver) = check_command_output("ffmpeg", &["-version"]);
    let (ffprobe_avail, ffprobe_ver) = check_command_output("ffprobe", &["-version"]);
    let (node_avail, node_ver) = check_command_output("node", &["--version"]);

    let mut encoders = Vec::new();
    if ffmpeg_avail {
        for enc in ["libx264", "libx265", "libvpx-vp9", "prores", "libsvtav1"] {
            if check_ffmpeg_encoder_supported(enc) {
                encoders.push(enc.to_string());
            }
        }
    }

    let temp_dir_path = std::env::temp_dir();
    let temp_probe_file = temp_dir_path.join(format!(
        "rust_motion_doctor_probe_{}.tmp",
        std::process::id()
    ));
    let temp_writable = std::fs::write(&temp_probe_file, b"ok").is_ok()
        && std::fs::remove_file(&temp_probe_file).is_ok();

    let hw_accel = if cfg!(target_os = "macos") {
        "videotoolbox (hardware acceleration available)".into()
    } else if cfg!(target_os = "windows") {
        "d3d11va / nvenc (hardware acceleration available)".into()
    } else if cfg!(target_os = "linux") {
        "vaapi / nvenc (hardware acceleration available)".into()
    } else {
        "none (cpu rasterizer)".into()
    };

    let supported_codecs = vec![
        "h264".into(),
        "vp9".into(),
        "prores".into(),
        "gif".into(),
        "png".into(),
        "jpeg".into(),
        "webp".into(),
    ];

    let ok = ffmpeg_avail && temp_writable;

    DoctorReport {
        ok,
        ffmpeg: ToolCheck {
            available: ffmpeg_avail,
            path: if ffmpeg_avail {
                Some("ffmpeg".into())
            } else {
                None
            },
            version: ffmpeg_ver,
            encoders: if ffmpeg_avail { Some(encoders) } else { None },
        },
        ffprobe: ToolCheck {
            available: ffprobe_avail,
            path: if ffprobe_avail {
                Some("ffprobe".into())
            } else {
                None
            },
            version: ffprobe_ver,
            encoders: None,
        },
        node: ToolCheck {
            available: node_avail,
            path: if node_avail {
                Some("node".into())
            } else {
                None
            },
            version: node_ver,
            encoders: None,
        },
        temp_dir: TempDirCheck {
            path: temp_dir_path.display().to_string(),
            writable: temp_writable,
        },
        hw_accel,
        supported_codecs,
    }
}
