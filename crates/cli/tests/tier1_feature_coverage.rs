//! Tier 1: Feature Coverage E2E Tests
//!
//! Tests CLI argument parsing for `--composition`, `--props`, `--output`,
//! `--width`, `--height`, `--fps`, `--duration`, and `--backend`.

use clap::Parser;
use rust_motion_cli::{Cli, Commands, RenderBackend, RenderCodec};
use std::path::PathBuf;

#[test]
fn test_cli_flag_defaults() {
    let args = vec!["rust-motion", "render", "-c", "HelloWorld"];
    let cli = Cli::try_parse_from(args).expect("Failed to parse CLI args");

    match cli.command {
        Commands::Render {
            composition,
            script,
            props,
            output,
            audio,
            width,
            height,
            fps,
            duration,
            backend,
            codec,
            frame_start,
            frame_end,
            timeout_seconds,
            crf,
            preset,
            hw_accel,
            sandbox_roots,
            permissive,
            ..
        } => {
            assert_eq!(composition, Some("HelloWorld".into()));
            assert_eq!(script, None);
            assert_eq!(props, None);
            assert_eq!(output, PathBuf::from("out.mp4"));
            assert!(audio.is_empty());
            assert_eq!(width, 1920);
            assert_eq!(height, 1080);
            assert!((fps - 30.0).abs() < f64::EPSILON);
            assert_eq!(duration, 150);
            assert_eq!(backend, RenderBackend::Native);
            assert_eq!(codec, RenderCodec::H264);
            assert_eq!(frame_start, 0);
            assert_eq!(frame_end, None);
            assert_eq!(timeout_seconds, None);
            assert_eq!(crf, 18);
            assert_eq!(preset, "fast");
            assert_eq!(hw_accel, rust_motion_cli::HwAccelArg::Auto);
            assert!(sandbox_roots.is_empty());
            assert!(!permissive);
        }
        _ => panic!("Expected Commands::Render"),
    }
}

#[test]
fn test_cli_flag_custom_values() {
    let args = vec![
        "rust-motion",
        "render",
        "--composition",
        "CustomComposition",
        "--props",
        "input_data.json",
        "--output",
        "result_video.mp4",
        "--width",
        "1280",
        "--height",
        "720",
        "--fps",
        "60",
        "--duration",
        "300",
        "--audio",
        "music.wav",
    ];

    let cli = Cli::try_parse_from(args).expect("Failed to parse custom CLI args");

    match cli.command {
        Commands::Render {
            composition,
            script,
            props,
            output,
            audio,
            width,
            height,
            fps,
            duration,
            backend,
            codec,
            frame_start,
            frame_end,
            timeout_seconds,
            crf,
            preset,
            hw_accel,
            sandbox_roots,
            permissive,
            ..
        } => {
            assert_eq!(composition, Some("CustomComposition".into()));
            assert_eq!(script, None);
            assert_eq!(props, Some(PathBuf::from("input_data.json")));
            assert_eq!(output, PathBuf::from("result_video.mp4"));
            assert_eq!(audio, vec![PathBuf::from("music.wav")]);
            assert_eq!(width, 1280);
            assert_eq!(height, 720);
            assert!((fps - 60.0).abs() < f64::EPSILON);
            assert_eq!(duration, 300);
            assert_eq!(backend, RenderBackend::Native);
            assert_eq!(codec, RenderCodec::H264);
            assert_eq!(frame_start, 0);
            assert_eq!(frame_end, None);
            assert_eq!(timeout_seconds, None);
            assert_eq!(crf, 18);
            assert_eq!(preset, "fast");
            assert_eq!(hw_accel, rust_motion_cli::HwAccelArg::Auto);
            assert_eq!(sandbox_roots, Vec::<PathBuf>::new());
            assert!(!permissive);
        }
        _ => panic!("Expected Commands::Render"),
    }
}

#[test]
fn test_cli_parses_codec_range_quality_and_timeout() {
    let cli = Cli::try_parse_from([
        "rust-motion",
        "render",
        "-c",
        "HelloWorld",
        "-o",
        "clip.webm",
        "--codec",
        "av1",
        "--frame-start",
        "12",
        "--frame-end",
        "23",
        "--timeout-seconds",
        "90",
        "--crf",
        "28",
        "--preset",
        "medium",
        "--scale",
        "1.5",
    ])
    .expect("Failed to parse render controls");

    match cli.command {
        Commands::Render {
            codec,
            frame_start,
            frame_end,
            timeout_seconds,
            crf,
            preset,
            scale,
            ..
        } => {
            assert_eq!(codec, RenderCodec::Av1);
            assert_eq!(frame_start, 12);
            assert_eq!(frame_end, Some(23));
            assert_eq!(timeout_seconds, Some(90));
            assert_eq!(crf, 28);
            assert_eq!(preset, "medium");
            assert_eq!(scale, 1.5);
        }
        _ => panic!("Expected Commands::Render"),
    }
}

#[test]
fn test_cli_short_flags() {
    let args = vec![
        "rust-motion",
        "render",
        "-c",
        "ShortFlagComp",
        "-p",
        "props.json",
        "-o",
        "out_short.mp4",
    ];

    let cli = Cli::try_parse_from(args).expect("Failed to parse short CLI args");

    match cli.command {
        Commands::Render {
            composition,
            props,
            output,
            ..
        } => {
            assert_eq!(composition, Some("ShortFlagComp".into()));
            assert_eq!(props, Some(PathBuf::from("props.json")));
            assert_eq!(output, PathBuf::from("out_short.mp4"));
        }
        _ => panic!("Expected Commands::Render"),
    }
}

#[test]
fn test_cli_accepts_rhai_script_as_the_composition_source() {
    let cli = Cli::try_parse_from(["rust-motion", "render", "--script", "composition.rhai"])
        .expect("Failed to parse Rhai script argument");

    match cli.command {
        Commands::Render {
            composition,
            script,
            ..
        } => {
            assert_eq!(composition, None);
            assert_eq!(script, Some(PathBuf::from("composition.rhai")));
        }
        _ => panic!("Expected Commands::Render"),
    }
}

#[test]
fn test_cli_rejects_conflicting_composition_sources() {
    let result = Cli::try_parse_from([
        "rust-motion",
        "render",
        "--composition",
        "HelloWorld",
        "--script",
        "composition.rhai",
    ]);
    assert!(result.is_err());
}

#[test]
fn test_cli_missing_required_composition() {
    let args = vec!["rust-motion", "render"];
    let result = Cli::try_parse_from(args);
    assert!(
        result.is_err(),
        "Expected parsing error when --composition is missing"
    );
}

#[test]
fn test_cli_migrate_subcommand() {
    let args = vec![
        "rust-motion",
        "migrate",
        "input.tsx",
        "--target",
        "python",
        "-o",
        "output.py",
    ];
    let cli = Cli::try_parse_from(args).expect("Failed to parse migrate command");
    match cli.command {
        Commands::Migrate {
            input,
            target,
            output,
            experimental,
        } => {
            assert_eq!(input, PathBuf::from("input.tsx"));
            assert_eq!(target, "python");
            assert_eq!(output, Some(PathBuf::from("output.py")));
            assert!(!experimental);
        }
        _ => panic!("Expected Commands::Migrate"),
    }
}

#[test]
fn test_cli_probe_subcommand() {
    let args = vec!["rust-motion", "probe", "sample.mp4"];
    let cli = Cli::try_parse_from(args).expect("Failed to parse probe command");
    match cli.command {
        Commands::Probe { path, full } => {
            assert!(!full);
            assert_eq!(path, PathBuf::from("sample.mp4"));
        }
        _ => panic!("Expected Commands::Probe"),
    }
}

#[test]
fn test_cli_render_project_accepts_remote_asset_cache() {
    let cli = Cli::try_parse_from([
        "rust-motion",
        "render-project",
        "project.rust_motion.json",
        "--output",
        "out.mp4",
        "--asset-cache-dir",
        ".rust-motion-assets",
        "--max-total-asset-bytes",
        "2048",
    ])
    .expect("Failed to parse render-project asset cache option");
    match cli.command {
        Commands::RenderProject {
            input,
            output,
            asset_cache_dir,
            max_total_asset_bytes,
        } => {
            assert_eq!(input, PathBuf::from("project.rust_motion.json"));
            assert_eq!(output, PathBuf::from("out.mp4"));
            assert_eq!(asset_cache_dir, Some(PathBuf::from(".rust-motion-assets")));
            assert_eq!(max_total_asset_bytes, 2048);
        }
        _ => panic!("Expected Commands::RenderProject"),
    }
}

#[test]
fn test_cli_webcodecs_drift_accepts_validation_options() {
    let cli = Cli::try_parse_from([
        "rust-motion",
        "webcodecs-drift",
        "--worker",
        "worker.mjs",
        "--fps",
        "23.976",
        "--frame-start",
        "12",
        "--frames",
        "48",
        "--output",
        "drift.json",
        "--csv",
        "drift.csv",
    ])
    .expect("Failed to parse webcodecs-drift options");
    match cli.command {
        Commands::WebcodecsDrift {
            worker,
            fps,
            frame_start,
            frames,
            output,
            csv,
            ..
        } => {
            assert_eq!(worker, PathBuf::from("worker.mjs"));
            assert_eq!(fps, 23.976);
            assert_eq!(frame_start, 12);
            assert_eq!(frames, 48);
            assert_eq!(output, PathBuf::from("drift.json"));
            assert_eq!(csv, Some(PathBuf::from("drift.csv")));
        }
        _ => panic!("Expected Commands::WebcodecsDrift"),
    }
}

#[test]
fn test_cli_profile_flag() {
    let cli = Cli::try_parse_from(["rust-motion", "render", "-c", "ProfileComp", "--profile"])
        .expect("Failed to parse --profile CLI arg");

    match cli.command {
        Commands::Render { profile, .. } => {
            assert!(profile);
        }
        _ => panic!("Expected Commands::Render"),
    }
}
