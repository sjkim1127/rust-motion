use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn cli_bin() -> PathBuf {
    std::env::var("CARGO_BIN_EXE_rust_motion")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            manifest.join("../../target/debug/rust-motion")
        })
}

fn tmp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rust_motion_contract_{}_{}", label, nanos));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_cli_doctor_json_output() {
    let output = Command::new(cli_bin())
        .arg("doctor")
        .arg("--json")
        .output()
        .expect("doctor command executes");

    assert!(output.status.success(), "doctor command should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value =
        serde_json::from_str(&stdout).expect("doctor output must be valid JSON");

    assert!(val.get("ok").is_some());
    assert!(val.get("ffmpeg").is_some());
    assert!(val["ffmpeg"]["available"].is_boolean());
    assert!(val.get("temp_dir").is_some());
    assert!(val["temp_dir"]["writable"].as_bool().unwrap_or(false));
    assert!(val.get("supported_codecs").is_some());
}

#[test]
fn test_cli_doctor_env_json() {
    let output = Command::new(cli_bin())
        .arg("doctor")
        .env("RUST_MOTION_JSON", "1")
        .output()
        .expect("doctor command executes");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value =
        serde_json::from_str(&stdout).expect("RUST_MOTION_JSON=1 must output valid JSON");
    assert!(val.get("ok").is_some());
}

#[test]
fn test_cli_inspect_backends_json() {
    let output = Command::new(cli_bin())
        .args(["inspect", "--backends", "--json"])
        .output()
        .expect("inspect backends executes");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let backends: Vec<serde_json::Value> =
        serde_json::from_str(&stdout).expect("backends output must be JSON array");

    let names: Vec<&str> = backends
        .iter()
        .filter_map(|b| b.get("name").and_then(|n| n.as_str()))
        .collect();
    assert!(names.contains(&"native"), "native backend must be present");
    assert!(names.contains(&"gpu"), "gpu backend must be present");
    assert!(
        names.contains(&"browser"),
        "browser backend must be present"
    );

    let native = backends.iter().find(|b| b["name"] == "native").unwrap();
    assert_eq!(native["tier"], "reference");
    assert_eq!(native["status"], "supported");
}

#[test]
fn test_cli_inspect_codecs_json() {
    let output = Command::new(cli_bin())
        .args(["inspect", "--codecs", "--json"])
        .output()
        .expect("inspect codecs executes");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let codecs: Vec<serde_json::Value> =
        serde_json::from_str(&stdout).expect("codecs output must be JSON array");

    let codec_names: Vec<&str> = codecs
        .iter()
        .filter_map(|c| c.get("codec").and_then(|n| n.as_str()))
        .collect();
    assert!(codec_names.contains(&"h264"));
    assert!(codec_names.contains(&"png"));
    assert!(codec_names.contains(&"webp"));
}

#[test]
fn test_cli_inspect_compositions_json() {
    let output = Command::new(cli_bin())
        .args(["inspect", "--compositions", "--json"])
        .output()
        .expect("inspect compositions executes");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value =
        serde_json::from_str(&stdout).expect("compositions output must be JSON");
    assert_eq!(val["ok"], true);
    let comps = val["compositions"]
        .as_array()
        .expect("compositions is an array");
    let names: Vec<&str> = comps.iter().filter_map(|s| s.as_str()).collect();
    assert!(names.contains(&"HelloWorld"));
}

#[test]
fn test_cli_validate_composition_success() {
    let output = Command::new(cli_bin())
        .args(["validate", "--composition", "HelloWorld", "--json"])
        .output()
        .expect("validate command executes");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(val["ok"], true);
    assert_eq!(val["target_kind"], "composition");
    assert_eq!(val["target"], "HelloWorld");
}

#[test]
fn test_cli_validate_composition_failure_exit_code_4() {
    let output = Command::new(cli_bin())
        .args(["validate", "--composition", "DoesNotExist", "--json"])
        .output()
        .expect("validate command executes");

    assert_eq!(
        output.status.code(),
        Some(4),
        "validation error must exit with code 4 (EXIT_VALIDATION_ERROR)"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(val["ok"], false);
    assert!(!val["errors"].as_array().unwrap().is_empty());
}

#[test]
fn test_cli_validate_project_file_roundtrip() {
    let dir = tmp_dir("validate_proj");
    let proj_path = dir.join("test.rust_motion.json");
    let proj_json = serde_json::json!({
        "version": 1,
        "composition": "HelloWorld",
        "settings": {
            "width": 1280,
            "height": 720,
            "fps": 30.0,
            "duration": 60
        },
        "tracks": [],
        "assets": [],
        "events": []
    });
    fs::write(
        &proj_path,
        serde_json::to_string_pretty(&proj_json).unwrap(),
    )
    .unwrap();

    let output = Command::new(cli_bin())
        .args([
            "validate",
            "--project",
            proj_path.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("validate project executes");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(val["ok"], true);
    assert_eq!(val["target_kind"], "project");
    assert_eq!(val["details"]["width"], 1280);
    assert_eq!(val["details"]["height"], 720);
}

#[test]
fn test_cli_validate_invalid_project_fails_with_exit_code_4() {
    let dir = tmp_dir("validate_invalid_proj");
    let proj_path = dir.join("invalid.rust_motion.json");
    fs::write(&proj_path, "not a json").unwrap();

    let output = Command::new(cli_bin())
        .args([
            "validate",
            "--project",
            proj_path.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("validate invalid project executes");

    assert_eq!(
        output.status.code(),
        Some(4),
        "invalid project must exit with code 4"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(val["ok"], false);
}

#[test]
fn test_cli_inspect_project_file() {
    let dir = tmp_dir("inspect_proj");
    let proj_path = dir.join("project.rust_motion.json");
    let proj_json = serde_json::json!({
        "version": 1,
        "composition": "HelloWorld",
        "settings": {
            "width": 1920,
            "height": 1080,
            "fps": 60.0,
            "duration": 120
        },
        "tracks": [],
        "assets": [],
        "events": []
    });
    fs::write(
        &proj_path,
        serde_json::to_string_pretty(&proj_json).unwrap(),
    )
    .unwrap();

    let output = Command::new(cli_bin())
        .args(["inspect", proj_path.to_str().unwrap(), "--json"])
        .output()
        .expect("inspect project executes");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(val["version"], 1);
    assert_eq!(val["settings"]["width"], 1920);
    assert_eq!(val["settings"]["fps"], 60.0);
}
