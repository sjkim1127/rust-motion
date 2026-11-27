//! Media assets: registration, and the sandbox that bounds which files a session may read.
//!
//! Paths come from an agent, so every file is resolved against the session's media root and
//! rejected when it (or a symlink target) lies outside it.

use crate::session::McpSession;
use rust_motion_project::{AssetKind, AssetRef};
use rust_motion_rasterizer::backend::{FrameConfig, RasterizerBackend};
use rust_motion_rasterizer::scene::{ImageFit, Scene, SceneNode};
use rust_motion_rasterizer::{probe_video_metadata, MediaSecurityPolicy, TinySkiaBackend};
use std::path::{Path, PathBuf};

impl McpSession {
    /// A session that may only read media inside `root`.
    ///
    /// [`McpSession::new`] uses the current working directory, unless that is a file system root.
    /// A root that cannot be resolved permits no files at all.
    pub fn with_media_root(root: impl AsRef<Path>) -> Self {
        let mut session = Self::new();
        session.media_root = root.as_ref().canonicalize().ok();
        session
    }

    pub(crate) fn media_policy(&self) -> MediaSecurityPolicy {
        MediaSecurityPolicy::sandboxed(self.media_root.iter())
    }

    /// The canonical location of a local file inside the media root. Relative paths start there.
    pub(crate) fn resolve_media(&self, path: &str) -> Result<PathBuf, String> {
        let root = self
            .media_root
            .as_deref()
            .ok_or("no media root is available, so no media files can be read")?;
        let local = path.trim();
        let local = local.strip_prefix("file://").unwrap_or(local);
        if local.is_empty() {
            return Err("invalid 'path': must not be empty".to_string());
        }
        if local.contains("://") || local.starts_with("data:") {
            return Err(format!(
                "invalid 'path': '{path}' is not a local file; only local files are supported"
            ));
        }
        let joined = root.join(local);
        let joined = joined
            .to_str()
            .ok_or("invalid 'path': the media root is not valid UTF-8")?;
        let resolved = self
            .media_policy()
            .validate_path(joined)
            .map_err(|error| format!("invalid 'path': {error}"))?;
        if !resolved.is_file() {
            return Err(format!("invalid 'path': '{path}' is not a file"));
        }
        Ok(resolved)
    }

    /// Register a local media file so that clips can refer to it by ID.
    ///
    /// Images and videos are read to prove they decode; audio files are only checked to exist.
    /// The stored path is relative to the media root.
    pub fn import_asset(
        &mut self,
        path: &str,
        kind: AssetKind,
        asset_id: Option<&str>,
    ) -> Result<AssetRef, String> {
        let mut project = self
            .project
            .clone()
            .ok_or("no project: call project_create before importing assets")?;
        let resolved = self.resolve_media(path)?;
        match kind {
            AssetKind::Image => self.check_image(&resolved)?,
            AssetKind::Video => {
                let source = resolved
                    .to_str()
                    .ok_or("invalid 'path': the file name is not valid UTF-8")?;
                probe_video_metadata(source)
                    .map_err(|error| format!("invalid 'path': cannot read video ({error})"))?;
            }
            AssetKind::Audio => {}
            other => {
                return Err(format!(
                    "unsupported 'kind': {other:?} assets cannot be imported"
                ));
            }
        }

        let taken = |id: &str| project.assets.iter().any(|asset| asset.id == id);
        let id = match asset_id {
            Some(id) if id.trim().is_empty() => {
                return Err("invalid 'asset_id': must not be empty".to_string());
            }
            Some(id) if taken(id) => {
                return Err(format!("invalid 'asset_id': '{id}' is already registered"));
            }
            Some(id) => id.to_string(),
            None => {
                let stem = resolved
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().to_lowercase().replace(' ', "_"))
                    .unwrap_or_default();
                let base = format!("asset_{stem}");
                let mut id = base.clone();
                let mut suffix = 2usize;
                while taken(&id) {
                    id = format!("{base}_{suffix}");
                    suffix += 1;
                }
                id
            }
        };

        let relative = self
            .media_root
            .as_deref()
            .and_then(|root| resolved.strip_prefix(root).ok())
            .and_then(Path::to_str)
            .ok_or("invalid 'path': the file name is not valid UTF-8")?;
        let asset = AssetRef {
            id,
            path: relative.to_string(),
            kind,
            sha256: None,
        };
        project.assets.push(asset.clone());
        project
            .validate()
            .map_err(|error| format!("asset would produce an invalid project: {error}"))?;
        self.project = Some(project);
        Ok(asset)
    }

    fn check_image(&self, resolved: &Path) -> Result<(), String> {
        // SVG is not offered as an asset type.
        if resolved
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
        {
            return Err("unsupported 'path': SVG images are not supported".to_string());
        }
        let mut scene = Scene::new();
        scene.push(SceneNode::Image {
            src: resolved.display().to_string(),
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
            fit: ImageFit::Fill,
            opacity: 1.0,
        });
        TinySkiaBackend::headless_with_policy(self.media_policy())
            .render_frame(&scene, &FrameConfig::new(1, 1, 0, 30.0))
            .map_err(|error| format!("invalid 'path': cannot decode image ({error})"))?;
        Ok(())
    }
}
