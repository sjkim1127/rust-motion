//! Project JSON, schema and settings validation tests.

use super::*;

#[test]
fn project_json_round_trip() {
    let p = project();
    assert_eq!(
        serde_json::from_str::<Project>(&serde_json::to_string(&p).unwrap()).unwrap(),
        p
    );
}

#[test]
fn project_rejects_ambiguous_asset_manifest() {
    let mut p = project();
    p.assets = vec![
        AssetRef {
            id: "logo".into(),
            path: "logo.png".into(),
            kind: AssetKind::Image,
            sha256: None,
        },
        AssetRef {
            id: "logo".into(),
            path: "other.png".into(),
            kind: AssetKind::Image,
            sha256: None,
        },
    ];
    assert_eq!(
        p.validate(),
        Err(ProjectError::DuplicateAssetId("logo".into()))
    );
    p.assets[1].id.clear();
    assert_eq!(p.validate(), Err(ProjectError::EmptyAssetId));
    p.assets[1].id = "other".into();
    p.assets[1].path.clear();
    assert_eq!(
        p.validate(),
        Err(ProjectError::EmptyAssetPath("other".into()))
    );
    p.assets[1].path = "other.png".into();
    p.assets[1].sha256 = Some("not-a-digest".into());
    assert_eq!(
        p.validate(),
        Err(ProjectError::InvalidAssetHash("other".into()))
    );
    p.assets[1].sha256 = None;
    p.props = serde_json::json!({"src": "asset://missing"});
    assert_eq!(
        p.validate(),
        Err(ProjectError::UnknownAssetReference("missing".into()))
    );
}

#[test]
fn project_json_loader_validates_schema() {
    let mut value = serde_json::to_value(project()).unwrap();
    value["version"] = serde_json::json!(2);
    assert_eq!(
        Project::from_json_str(&value.to_string()),
        Err(ProjectError::UnsupportedVersion(2))
    );
}

#[test]
fn project_json_rejects_unknown_fields() {
    let mut value = serde_json::to_value(project()).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(matches!(
        Project::from_json_str(&value.to_string()),
        Err(ProjectError::Json(_))
    ));
}

#[test]
fn project_rejects_zero_concurrency() {
    let mut value = serde_json::to_value(project()).unwrap();
    value["settings"]["concurrency"] = serde_json::json!(0);

    assert_eq!(
        Project::from_json_str(&serde_json::to_string(&value).unwrap()),
        Err(ProjectError::InvalidConcurrency)
    );
}

#[test]
fn project_rejects_invalid_browser_capture_settings() {
    let mut p = project();
    p.settings.browser_image_format = Some("webp".into());
    assert_eq!(p.validate(), Err(ProjectError::InvalidBrowserImageFormat));

    let mut p = project();
    p.settings.browser_jpeg_quality = Some(0);
    assert_eq!(p.validate(), Err(ProjectError::InvalidBrowserJpegQuality));

    let mut p = project();
    p.settings.browser_frame_timeout_ms = Some(0);
    assert_eq!(p.validate(), Err(ProjectError::InvalidBrowserFrameTimeout));

    let mut p = project();
    p.settings.browser_transport = Some("shared-memory".into());
    assert_eq!(p.validate(), Err(ProjectError::InvalidBrowserTransport));
}

#[test]
fn project_accepts_valid_browser_capture_settings() {
    let mut p = project();
    p.settings.browser_image_format = Some(" JPEG ".into());
    p.settings.browser_jpeg_quality = Some(90);
    p.settings.browser_frame_timeout_ms = Some(5_000);
    p.settings.browser_transport = Some(" FILE ".into());
    assert!(p.validate().is_ok());
    p.settings.browser_transport_retries = Some(2);
    assert!(p.validate().is_ok());
}

#[test]
fn invalid_project_is_rejected() {
    let mut p = project();
    p.settings.fps = 0.0;
    assert_eq!(p.validate(), Err(ProjectError::InvalidFps));
}

#[test]
fn invalid_project_frame_ranges_are_rejected() {
    let mut p = project();
    p.settings.frame_start = Some(20);
    p.settings.frame_end = Some(10);
    assert!(matches!(
        p.validate(),
        Err(ProjectError::InvalidFrameRange { .. })
    ));

    p.settings.frame_end = None;
    p.settings.frame_start = Some(p.settings.duration);
    assert!(matches!(
        p.validate(),
        Err(ProjectError::InvalidFrameRange { .. })
    ));

    p.settings.frame_start = Some(0);
    p.settings.frame_end = Some(p.settings.duration);
    assert!(matches!(
        p.validate(),
        Err(ProjectError::InvalidFrameRange { .. })
    ));
}

#[test]
fn project_rejects_clips_outside_duration() {
    let mut p = project();
    p.tracks = vec![Track {
        id: "main".into(),
        clips: vec![Clip {
            id: "late".into(),
            composition: "caption".into(),
            start: 59,
            duration: 2,
            props: serde_json::json!({}),
        }],
    }];
    assert!(matches!(
        p.validate(),
        Err(ProjectError::InvalidClip { reason, .. })
            if reason == "clip range must fit within the project duration"
    ));

    p.tracks[0].clips[0].start = u32::MAX;
    assert!(matches!(
        p.validate(),
        Err(ProjectError::InvalidClip { reason, .. })
            if reason == "clip range must fit within the project duration"
    ));
}

#[test]
fn project_frame_start_without_end_defaults_to_duration_end() {
    let mut p = project();
    p.settings.frame_start = Some(12);
    assert!(p.validate().is_ok());
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["settings"]["frame_start"], 12);
}
