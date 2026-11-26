//! Project validation, including asset-reference checks.

use super::*;

impl Project {
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.version != 1 {
            return Err(ProjectError::UnsupportedVersion(self.version));
        }
        if self.composition.trim().is_empty() {
            return Err(ProjectError::EmptyComposition);
        }
        if self.settings.width == 0 || self.settings.height == 0 || self.settings.duration == 0 {
            return Err(ProjectError::InvalidDimensions);
        }
        if !self.settings.fps.is_finite() || self.settings.fps <= 0.0 {
            return Err(ProjectError::InvalidFps);
        }
        if !self.settings.scale.is_finite() || self.settings.scale <= 0.0 {
            return Err(ProjectError::InvalidScale);
        }
        if self.settings.frame_step == 0 {
            return Err(ProjectError::InvalidFrameStep);
        }
        if self.settings.concurrency == Some(0) {
            return Err(ProjectError::InvalidConcurrency);
        }
        if let Some(format) = &self.settings.browser_image_format {
            let format = format.trim().to_ascii_lowercase();
            if format != "png" && format != "jpeg" {
                return Err(ProjectError::InvalidBrowserImageFormat);
            }
        }
        if self
            .settings
            .browser_jpeg_quality
            .is_some_and(|quality| !(1..=100).contains(&quality))
        {
            return Err(ProjectError::InvalidBrowserJpegQuality);
        }
        if self
            .settings
            .browser_frame_timeout_ms
            .is_some_and(|timeout| timeout == 0)
        {
            return Err(ProjectError::InvalidBrowserFrameTimeout);
        }
        if self
            .settings
            .browser_transport
            .as_deref()
            .is_some_and(|transport| {
                !matches!(
                    transport.trim().to_ascii_lowercase().as_str(),
                    "base64" | "file"
                )
            })
        {
            return Err(ProjectError::InvalidBrowserTransport);
        }
        let start = self.settings.frame_start.unwrap_or(0);
        let end = self
            .settings
            .frame_end
            .unwrap_or_else(|| self.settings.duration.saturating_sub(1));
        if start > end || end >= self.settings.duration {
            return Err(ProjectError::InvalidFrameRange {
                start,
                end,
                duration: self.settings.duration,
            });
        }
        let mut asset_ids = BTreeSet::new();
        for asset in &self.assets {
            if asset.id.trim().is_empty() {
                return Err(ProjectError::EmptyAssetId);
            }
            if asset.path.trim().is_empty() {
                return Err(ProjectError::EmptyAssetPath(asset.id.clone()));
            }
            if let Some(hash) = &asset.sha256 {
                if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(ProjectError::InvalidAssetHash(asset.id.clone()));
                }
            }
            if !asset_ids.insert(asset.id.as_str()) {
                return Err(ProjectError::DuplicateAssetId(asset.id.clone()));
            }
        }
        let mut event_ids = BTreeSet::new();
        for event in &self.events {
            if event.id.trim().is_empty() {
                return Err(ProjectError::EmptyTimeEventId);
            }
            if !event_ids.insert(event.id.as_str()) {
                return Err(ProjectError::DuplicateTimeEvent(event.id.clone()));
            }
            if event.frame >= self.settings.duration {
                return Err(ProjectError::InvalidTimeEventFrame {
                    event: event.id.clone(),
                    frame: event.frame,
                    duration: self.settings.duration,
                });
            }
        }
        if let Some(asset_id) = &self.voice_over_asset_id {
            if !self
                .assets
                .iter()
                .any(|asset| asset.id == *asset_id && asset.kind == AssetKind::Audio)
            {
                return Err(ProjectError::InvalidVoiceOverAsset(asset_id.clone()));
            }
        }
        for track in &self.tracks {
            for clip in &track.clips {
                let reason = if clip.composition.trim().is_empty() {
                    Some("composition cannot be empty")
                } else if clip.duration == 0 {
                    Some("duration must be greater than zero")
                } else if clip
                    .start
                    .checked_add(clip.duration)
                    .is_none_or(|end| end > self.settings.duration)
                {
                    Some("clip range must fit within the project duration")
                } else {
                    None
                };
                if let Some(reason) = reason {
                    return Err(ProjectError::InvalidClip {
                        track: track.id.clone(),
                        clip: clip.id.clone(),
                        reason: reason.into(),
                    });
                }
                validate_asset_references(&clip.props, &asset_ids)?;
            }
        }
        validate_asset_references(&self.props, &asset_ids)?;
        Ok(())
    }
}

fn validate_asset_references(
    value: &serde_json::Value,
    asset_ids: &BTreeSet<&str>,
) -> Result<(), ProjectError> {
    match value {
        serde_json::Value::String(text) if text.starts_with("asset://") => {
            let id = text.trim_start_matches("asset://");
            if id.is_empty() || !asset_ids.contains(id) {
                return Err(ProjectError::UnknownAssetReference(id.to_string()));
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                validate_asset_references(value, asset_ids)?;
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                validate_asset_references(value, asset_ids)?;
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
    Ok(())
}
