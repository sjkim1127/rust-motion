use dioxuscut_project::{JobStatus, JobStore, Project, RenderJob};
use std::sync::{Arc, Mutex};

/// Host-neutral adapter for the Tauri render-job commands.
///
/// Keeping the mutex and store lookup here makes job orchestration testable
/// without constructing a Tauri runtime or UI window.
#[derive(Clone)]
pub(crate) struct RenderJobController {
    jobs: Arc<Mutex<JobStore>>,
}

impl RenderJobController {
    pub(crate) fn new(jobs: Arc<Mutex<JobStore>>) -> Self {
        Self { jobs }
    }

    pub(crate) fn submit(&self, project: Project) -> Result<String, String> {
        self.jobs
            .lock()
            .map_err(|_| "job store lock poisoned".to_string())?
            .submit(project)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn get(&self, id: &str) -> Result<Option<RenderJob>, String> {
        Ok(self
            .jobs
            .lock()
            .map_err(|_| "job store lock poisoned".to_string())?
            .get(id)
            .cloned())
    }

    pub(crate) fn list(&self) -> Result<Vec<RenderJob>, String> {
        Ok(self
            .jobs
            .lock()
            .map_err(|_| "job store lock poisoned".to_string())?
            .list())
    }

    pub(crate) fn complete(&self, id: &str, frames: u32) -> Result<(), String> {
        self.jobs
            .lock()
            .map_err(|_| "job store lock poisoned".to_string())?
            .complete_render(id, frames)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn update(&self, id: &str, status: JobStatus, frames: u32) -> Result<(), String> {
        self.jobs
            .lock()
            .map_err(|_| "job store lock poisoned".to_string())?
            .try_update(id, status, frames)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn set_encoding_progress(&self, id: &str, frames: u32) -> Result<(), String> {
        self.jobs
            .lock()
            .map_err(|_| "job store lock poisoned".to_string())?
            .set_encoding_progress(id, frames)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dioxuscut_project::{BackendKind, ProjectSettings};
    use dioxuscut_rasterizer::{
        render_to_ffmpeg_pipe, BrowserFrameBackend, Color, PipeConfig, RenderControl, Scene,
        SceneNode, TinySkiaBackend, VideoCodec, WebFrameRequest,
    };
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn ffmpeg_available() -> bool {
        Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn smoke_output() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dioxuscut_tauri_smoke_{}_{}.gif",
            std::process::id(),
            nonce
        ))
    }

    #[test]
    fn controller_reads_empty_store_without_tauri_runtime() {
        let controller = RenderJobController::new(Arc::new(Mutex::new(JobStore::default())));
        assert_eq!(controller.get("job-missing").unwrap(), None);
        assert!(controller.list().unwrap().is_empty());
    }

    #[test]
    fn controller_replays_render_and_encoding_job_lifecycle() {
        let controller = RenderJobController::new(Arc::new(Mutex::new(JobStore::default())));
        let id = controller
            .submit(Project {
                version: 1,
                composition: "smoke".into(),
                settings: ProjectSettings {
                    width: 16,
                    height: 16,
                    fps: 30.0,
                    duration: 3,
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
                assets: vec![],
                tracks: vec![],
                events: vec![],
                voice_over_asset_id: None,
            })
            .unwrap();
        controller.update(&id, JobStatus::Preparing, 0).unwrap();
        controller.update(&id, JobStatus::Rendering, 3).unwrap();
        controller.set_encoding_progress(&id, 2).unwrap();
        controller.complete(&id, 3).unwrap();

        let job = controller.get(&id).unwrap().unwrap();
        assert_eq!(job.status, JobStatus::Completed);
        assert_eq!(job.completed_frames, 3);
        assert_eq!(job.encoded_frames, 3);
    }

    #[test]
    fn controller_tracks_real_native_pipe_render() {
        if !ffmpeg_available() {
            return;
        }

        let controller = RenderJobController::new(Arc::new(Mutex::new(JobStore::default())));
        let id = controller
            .submit(Project {
                version: 1,
                composition: "native-smoke".into(),
                settings: ProjectSettings {
                    width: 16,
                    height: 16,
                    fps: 30.0,
                    duration: 3,
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
                assets: vec![],
                tracks: vec![],
                events: vec![],
                voice_over_asset_id: None,
            })
            .unwrap();
        controller.update(&id, JobStatus::Preparing, 0).unwrap();
        controller.update(&id, JobStatus::Rendering, 0).unwrap();

        let progress_controller = controller.clone();
        let encoding_controller = controller.clone();
        let progress_id = id.clone();
        let encoding_id = id.clone();
        let control = RenderControl::new()
            .with_progress(move |progress| {
                progress_controller
                    .update(
                        &progress_id,
                        JobStatus::Rendering,
                        progress.completed_frames,
                    )
                    .unwrap();
            })
            .with_encoding_progress(move |progress| {
                encoding_controller
                    .set_encoding_progress(&encoding_id, progress.encoded_frames)
                    .unwrap();
            });
        let output = smoke_output();
        let config = PipeConfig::new(16, 16, 30.0, 3, &output)
            .with_codec(VideoCodec::Gif)
            .with_control(control);
        let backend = TinySkiaBackend::headless();
        render_to_ffmpeg_pipe(&backend, &config, |frame| {
            let mut scene = Scene::new();
            scene.push(SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 16.0,
                h: 16.0,
                fill: Color::rgb((frame * 40) as u8, 20, 200),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            });
            scene
        })
        .unwrap();
        controller.complete(&id, 3).unwrap();

        let job = controller.get(&id).unwrap().unwrap();
        assert_eq!(job.status, JobStatus::Completed);
        assert_eq!(job.completed_frames, 3);
        assert_eq!(job.encoded_frames, 3);
        assert!(output.is_file());
        std::fs::remove_file(output).unwrap();
    }

    #[test]
    fn real_chrome_worker_frames_decode_in_rust_backend() {
        let Ok(url) = std::env::var("DIOXUSCUT_BROWSER_SMOKE_URL") else {
            return;
        };
        let worker =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts/three-render-worker.mjs");
        let backend = BrowserFrameBackend::new("node", worker, url).unwrap();
        for frame in 0..3 {
            let image = backend
                .render_web_frame(&WebFrameRequest {
                    composition: Some("three_preview".into()),
                    frame,
                    fps: 30.0,
                    width: 2,
                    height: 2,
                    props: serde_json::json!({}),
                    assets: vec![],
                    timeline: vec![],
                    time_events: vec![],
                    image_format: None,
                    jpeg_quality: None,
                    transparent: false,
                    transport: None,
                })
                .unwrap();
            assert_eq!((image.width(), image.height()), (2, 2));
            assert_eq!(image.as_raw().len(), 16);
        }
    }
}
