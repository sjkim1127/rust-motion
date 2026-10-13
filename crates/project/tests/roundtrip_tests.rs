use dioxuscut_project::{
    AssetKind, AssetRef, BackendKind, Clip, Project, ProjectSettings, TimeEvent, Track,
};
use std::fs;
use std::path::Path;

#[test]
fn test_roundtrip_named_time_events_example() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example_path = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples/named-time-events.dioxuscut.json");

    assert!(example_path.exists(), "Example project file must exist");

    let project = Project::load(&example_path).expect("Failed to load example project");

    assert_eq!(project.version, 1);
    assert_eq!(project.composition, "NamedTimeEventsDemo");
    assert_eq!(project.settings.width, 1280);
    assert_eq!(project.settings.height, 720);
    assert_eq!(project.settings.duration, 150);
    assert_eq!(project.events.len(), 2);
    assert_eq!(project.events[0].id, "voice_start");
    assert_eq!(project.events[0].frame, 30);
    assert_eq!(project.events[1].id, "voice_end");
    assert_eq!(project.events[1].frame, 90);

    // Round-trip serialize to JSON string and back
    let serialized = serde_json::to_string_pretty(&project).expect("Failed to serialize project");
    let reloaded: Project =
        serde_json::from_str(&serialized).expect("Failed to deserialize project");

    assert_eq!(
        project, reloaded,
        "Reloaded project must match original exactly"
    );
}

#[test]
fn test_roundtrip_mutations_and_file_save() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let project_path = temp_dir.path().join("test_project.dioxuscut.json");

    let project = Project {
        version: 1,
        composition: "CustomTimeline".into(),
        settings: ProjectSettings {
            width: 1920,
            height: 1080,
            fps: 60.0,
            duration: 300,
            scale: 1.0,
            crf: Some(17),
            preset: Some("slow".into()),
            concurrency: Some(4),
            frame_step: 1,
            frame_start: Some(0),
            frame_end: Some(299),
            backend: BackendKind::Native,
            browser_image_format: None,
            browser_jpeg_quality: None,
            browser_frame_timeout_ms: None,
            browser_transport: None,
            browser_transport_retries: None,
        },
        props: serde_json::json!({
            "title": "Rust Motion 1.0",
            "theme": "dark",
            "accent": "#00ffcc"
        }),
        assets: vec![
            AssetRef {
                id: "logo".into(),
                path: "assets/logo.png".into(),
                kind: AssetKind::Image,
                sha256: None,
            },
            AssetRef {
                id: "voice".into(),
                path: "audio/voice.wav".into(),
                kind: AssetKind::Audio,
                sha256: None,
            },
        ],
        tracks: vec![
            Track {
                id: "video_track_1".into(),
                clips: vec![
                    Clip {
                        id: "clip_intro".into(),
                        composition: "SceneIntro".into(),
                        start: 0,
                        duration: 60,
                        props: serde_json::json!({}),
                    },
                    Clip {
                        id: "clip_main".into(),
                        composition: "SceneMain".into(),
                        start: 60,
                        duration: 240,
                        props: serde_json::json!({ "mode": "presentation" }),
                    },
                ],
            },
            Track {
                id: "audio_track_1".into(),
                clips: vec![Clip {
                    id: "clip_audio_1".into(),
                    composition: "SceneAudio".into(),
                    start: 0,
                    duration: 300,
                    props: serde_json::json!({ "volume": 0.85 }),
                }],
            },
        ],
        events: vec![
            TimeEvent {
                id: "logo_fade".into(),
                frame: 45,
            },
            TimeEvent {
                id: "title_pop".into(),
                frame: 120,
            },
        ],
        voice_over_asset_id: Some("voice".into()),
    };

    // Save project to disk
    project
        .save(&project_path)
        .expect("Failed to save project file");

    // Load project back from disk
    let loaded = Project::load(&project_path).expect("Failed to load saved project");
    assert_eq!(project, loaded);

    // Mutate loaded project
    let mut modified = loaded;
    modified.settings.fps = 24.0;
    modified.events.push(TimeEvent {
        id: "credits_roll".into(),
        frame: 280,
    });
    modified.tracks[0].clips.push(Clip {
        id: "clip_outro".into(),
        composition: "SceneOutro".into(),
        start: 240,
        duration: 60,
        props: serde_json::json!({}),
    });

    // Save modified project and reload
    modified
        .save(&project_path)
        .expect("Failed to save modified project");
    let reloaded_modified =
        Project::load(&project_path).expect("Failed to reload modified project");
    assert_eq!(modified, reloaded_modified);
    assert_eq!(reloaded_modified.events.len(), 3);
    assert_eq!(reloaded_modified.tracks[0].clips.len(), 3);
    assert_eq!(reloaded_modified.settings.fps, 24.0);
}

#[test]
fn test_relative_asset_path_resolution() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let assets_dir = temp_dir.path().join("media");
    fs::create_dir_all(&assets_dir).expect("Failed to create media dir");

    let image_file = assets_dir.join("bg.png");
    fs::write(&image_file, b"fake png data").expect("Failed to write mock asset");

    let mut project = Project {
        version: 1,
        composition: "AssetResolutionTest".into(),
        settings: ProjectSettings {
            width: 800,
            height: 600,
            fps: 30.0,
            duration: 90,
            scale: 1.0,
            crf: None,
            preset: None,
            concurrency: None,
            frame_step: 1,
            frame_start: None,
            frame_end: None,
            backend: BackendKind::Native,
            browser_image_format: None,
            browser_jpeg_quality: None,
            browser_frame_timeout_ms: None,
            browser_transport: None,
            browser_transport_retries: None,
        },
        props: serde_json::json!({}),
        assets: vec![AssetRef {
            id: "background".into(),
            path: "media/bg.png".into(),
            kind: AssetKind::Image,
            sha256: None,
        }],
        tracks: vec![],
        events: vec![],
        voice_over_asset_id: None,
    };

    // Asset validation should succeed against parent dir
    project
        .validate_asset_files(temp_dir.path())
        .expect("Asset validation should find media/bg.png relative to project dir");

    // Resolving local asset paths converts relative to absolute
    project.resolve_local_asset_paths(temp_dir.path());
    let resolved_path = Path::new(&project.assets[0].path);
    assert!(
        resolved_path.is_absolute(),
        "Resolved asset path must be absolute"
    );
    assert_eq!(resolved_path, image_file);
}

#[test]
fn test_invalid_project_json_rejected() {
    // Missing required fields like version and composition
    let malformed_json = r#"{ "settings": { "width": 1920 } }"#;
    let result: Result<Project, _> = serde_json::from_str(malformed_json);
    assert!(
        result.is_err(),
        "Malformed project JSON must fail to deserialize"
    );

    // Invalid schema version
    let bad_version_json = r#"{
        "version": "not-a-number",
        "composition": "Test",
        "settings": { "width": 1920, "height": 1080, "fps": 30.0, "duration": 100 }
    }"#;
    let result: Result<Project, _> = serde_json::from_str(bad_version_json);
    assert!(
        result.is_err(),
        "Invalid version type must fail to deserialize"
    );
}
