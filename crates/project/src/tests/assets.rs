//! Local asset manifest, hashing and path-rewriting tests.

use super::*;

#[test]
fn save_rewrites_local_asset_paths_in_timeline_clip_props() {
    let directory = tempfile::tempdir().unwrap();
    let asset_path = directory.path().join("logo.png");
    std::fs::write(&asset_path, b"logo").unwrap();
    let resolved_asset = asset_path.to_string_lossy().into_owned();
    let mut p = project();
    p.assets.push(AssetRef {
        id: "logo".into(),
        path: resolved_asset.clone(),
        kind: AssetKind::Image,
        sha256: None,
    });
    p.tracks.push(Track {
        id: "main".into(),
        clips: vec![Clip {
            id: "logo-clip".into(),
            composition: "shorts".into(),
            start: 0,
            duration: 10,
            props: serde_json::json!({"overlay": {"src": resolved_asset}}),
        }],
    });

    p.relativize_local_asset_paths(directory.path());
    let project_path = directory.path().join("project.json");
    p.save(&project_path).unwrap();
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(project_path).unwrap()).unwrap();

    assert_eq!(saved["assets"][0]["path"], "logo.png");
    assert_eq!(
        saved["tracks"][0]["clips"][0]["props"]["overlay"]["src"],
        "logo.png"
    );
}

#[test]
fn project_asset_files_verify_sha256_relative_to_base_dir() {
    let base =
        std::env::temp_dir().join(format!("rust-motion-project-assets-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let asset_path = base.join("logo.bin");
    std::fs::write(&asset_path, b"rust-motion asset").unwrap();
    let digest = format!("{:x}", Sha256::digest(b"rust-motion asset"));
    let mut p = project();
    p.assets = vec![AssetRef {
        id: "logo".into(),
        path: "logo.bin".into(),
        kind: AssetKind::Image,
        sha256: Some(digest.clone()),
    }];
    assert!(p.validate_asset_files(&base).is_ok());
    p.assets[0].sha256 = Some("00".repeat(32));
    assert!(matches!(
        p.validate_asset_files(&base),
        Err(ProjectError::AssetHashMismatch { .. })
    ));
    p.assets[0].path = "../outside.bin".into();
    std::fs::write(base.join("../outside.bin"), b"outside").unwrap();
    assert_eq!(
        p.validate_asset_files(&base),
        Err(ProjectError::AssetOutsideProject("logo".into()))
    );
    std::fs::remove_file(base.join("../outside.bin")).unwrap();
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn project_save_atomically_replaces_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.dcp");
    std::fs::write(&path, b"old incomplete project").unwrap();

    project().save(&path).unwrap();

    let saved = Project::load(&path).unwrap();
    assert_eq!(saved, project());
    assert!(std::fs::read_dir(dir.path())
        .unwrap()
        .all(|entry| { entry.unwrap().file_name() == "project.dcp" }));
}

#[cfg(unix)]
#[test]
fn project_asset_files_reject_symlink_escape() {
    use std::os::unix::fs::symlink;
    let base = std::env::temp_dir().join(format!(
        "rust-motion-project-symlink-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&base).unwrap();
    let outside = base.with_file_name(format!("rust-motion-outside-{}", std::process::id()));
    std::fs::write(&outside, b"outside").unwrap();
    symlink(&outside, base.join("linked.bin")).unwrap();
    let mut p = project();
    p.assets = vec![AssetRef {
        id: "linked".into(),
        path: "linked.bin".into(),
        kind: AssetKind::Other,
        sha256: None,
    }];
    assert_eq!(
        p.validate_asset_files(&base),
        Err(ProjectError::AssetOutsideProject("linked".into()))
    );
    std::fs::remove_file(base.join("linked.bin")).unwrap();
    std::fs::remove_dir_all(base).unwrap();
    std::fs::remove_file(outside).unwrap();
}

#[test]
fn project_resolves_manifest_paths_inside_nested_props() {
    let mut p = project();
    p.assets = vec![
        AssetRef {
            id: "poster".into(),
            path: "assets/poster.png".into(),
            kind: AssetKind::Image,
            sha256: None,
        },
        AssetRef {
            id: "remote".into(),
            path: "https://cdn.example/poster.png".into(),
            kind: AssetKind::Image,
            sha256: None,
        },
    ];
    p.props = serde_json::json!({"layers": [{"src": "asset://poster"}, {"src": "asset://remote"}]});
    p.resolve_local_asset_paths("/tmp/project");
    let expected_poster = std::path::Path::new("/tmp/project").join("assets/poster.png");
    assert_eq!(std::path::Path::new(&p.assets[0].path), expected_poster);
    assert_eq!(p.assets[1].path, "https://cdn.example/poster.png");
    assert_eq!(
        std::path::Path::new(p.props["layers"][0]["src"].as_str().unwrap()),
        expected_poster
    );
    assert_eq!(
        p.props["layers"][1]["src"],
        "https://cdn.example/poster.png"
    );
}
