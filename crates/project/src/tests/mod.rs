//! Unit tests for the project crate, grouped by concern.

use super::*;

mod assets;
mod jobs;
mod remote;
mod validation;

pub(super) fn project() -> Project {
    Project {
        version: 1,
        composition: "shorts".into(),
        settings: ProjectSettings {
            width: 1080,
            height: 1920,
            fps: 30.0,
            duration: 60,
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
        props: serde_json::json!({"title":"hello"}),
        assets: vec![],
        tracks: vec![],
        events: vec![],
        voice_over_asset_id: None,
    }
}
