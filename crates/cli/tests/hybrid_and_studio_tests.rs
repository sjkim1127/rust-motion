use clap::Parser;
use rust_motion_cli::{
    built_in_registry, resolve_browser_worker_path, resolve_hybrid_backend, Cli, Commands,
    RenderBackend,
};
use std::path::Path;

#[test]
fn test_resolve_hybrid_backend_routing() {
    let registry = built_in_registry();

    // 1. Web/Three.js compositions automatically route to Chromium Browser worker
    assert_eq!(
        resolve_hybrid_backend(
            RenderBackend::Auto,
            Some("three_lifecycle_preview"),
            None,
            &registry
        ),
        RenderBackend::Browser
    );
    assert_eq!(
        resolve_hybrid_backend(
            RenderBackend::Auto,
            Some("three_audio_reactive"),
            None,
            &registry
        ),
        RenderBackend::Browser
    );
    assert_eq!(
        resolve_hybrid_backend(
            RenderBackend::Auto,
            Some("web_component_scene"),
            None,
            &registry
        ),
        RenderBackend::Browser
    );
    assert_eq!(
        resolve_hybrid_backend(RenderBackend::Auto, Some("dom_overlay"), None, &registry),
        RenderBackend::Browser
    );
    assert_eq!(
        resolve_hybrid_backend(RenderBackend::Auto, Some("Scene.tsx"), None, &registry),
        RenderBackend::Browser
    );

    // 2. Motion graphics / text / subtitles / built-in compositions route 10x faster to Native
    assert_eq!(
        resolve_hybrid_backend(RenderBackend::Auto, Some("HelloWorld"), None, &registry),
        RenderBackend::Native
    );
    assert_eq!(
        resolve_hybrid_backend(
            RenderBackend::Auto,
            Some("KaraokeCaptions"),
            None,
            &registry
        ),
        RenderBackend::Native
    );
    assert_eq!(
        resolve_hybrid_backend(RenderBackend::Auto, Some("CyberpunkGrid"), None, &registry),
        RenderBackend::Native
    );

    // 3. Rhai scripts always route to Native for pure-Rust speed
    let rhai_path = Path::new("examples/script.rhai");
    assert_eq!(
        resolve_hybrid_backend(RenderBackend::Auto, None, Some(rhai_path), &registry),
        RenderBackend::Native
    );

    // 4. Explicit backend overrides are always respected
    assert_eq!(
        resolve_hybrid_backend(
            RenderBackend::Native,
            Some("three_lifecycle_preview"),
            None,
            &registry
        ),
        RenderBackend::Native
    );
    assert_eq!(
        resolve_hybrid_backend(RenderBackend::Browser, Some("text_anim"), None, &registry),
        RenderBackend::Browser
    );
}

#[test]
fn test_resolve_browser_worker_path() {
    let worker_path = resolve_browser_worker_path();
    assert!(
        worker_path.is_ok(),
        "Browser worker script should be discovered automatically"
    );
    let path = worker_path.unwrap();
    assert!(path.exists(), "Resolved worker path must exist: {:?}", path);
}

#[test]
fn test_cli_studio_command_parsing() {
    let cli = Cli::parse_from(["rust-motion", "studio", "--port", "1425", "--open"]);
    match cli.command {
        Commands::Studio {
            port,
            open,
            project,
        } => {
            assert_eq!(port, 1425);
            assert!(open);
            assert!(project.is_none());
        }
        _ => panic!("Expected Commands::Studio"),
    }
}
