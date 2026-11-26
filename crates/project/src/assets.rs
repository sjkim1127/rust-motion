//! Local asset manifest validation and path rewriting.

use super::*;

impl Project {
    /// Validate local manifest assets relative to the project file directory.
    /// Remote URLs are intentionally left to the browser backend.
    pub fn validate_asset_files(
        &self,
        base_dir: impl AsRef<std::path::Path>,
    ) -> Result<(), ProjectError> {
        let base_dir =
            base_dir
                .as_ref()
                .canonicalize()
                .map_err(|error| ProjectError::AssetRead {
                    asset: "<project>".into(),
                    reason: error.to_string(),
                })?;
        for asset in &self.assets {
            if asset.path.contains("://") || asset.path.starts_with("data:") {
                continue;
            }
            let path = base_dir.join(&asset.path);
            let canonical = path
                .canonicalize()
                .map_err(|error| ProjectError::AssetRead {
                    asset: asset.id.clone(),
                    reason: format!("{} ({})", error, path.display()),
                })?;
            if !canonical.starts_with(&base_dir) {
                return Err(ProjectError::AssetOutsideProject(asset.id.clone()));
            }
            let bytes = std::fs::read(&canonical).map_err(|error| ProjectError::AssetRead {
                asset: asset.id.clone(),
                reason: format!("{} ({})", error, canonical.display()),
            })?;
            if let Some(expected) = &asset.sha256 {
                let actual = format!("{:x}", Sha256::digest(bytes));
                if !expected.eq_ignore_ascii_case(&actual) {
                    return Err(ProjectError::AssetHashMismatch {
                        asset: asset.id.clone(),
                        expected: expected.clone(),
                        actual,
                    });
                }
            }
        }
        Ok(())
    }

    /// Resolve local manifest paths and matching prop values from one project directory.
    pub fn resolve_local_asset_paths(&mut self, base_dir: impl AsRef<std::path::Path>) {
        let base_dir = base_dir.as_ref();
        let replacements: Vec<(String, String)> = self
            .assets
            .iter_mut()
            .flat_map(|asset| {
                let reference = format!("asset://{}", asset.id);
                let original = asset.path.clone();
                let resolved = if original.contains("://") || original.starts_with("data:") {
                    original.clone()
                } else {
                    base_dir.join(&original).to_string_lossy().into_owned()
                };
                asset.path = resolved.clone();
                vec![(original, resolved.clone()), (reference, resolved)]
            })
            .collect();

        fn rewrite(value: &mut serde_json::Value, replacements: &[(String, String)]) {
            match value {
                serde_json::Value::String(text) => {
                    if let Some((_, resolved)) =
                        replacements.iter().find(|(original, _)| original == text)
                    {
                        *text = resolved.clone();
                    }
                }
                serde_json::Value::Array(values) => values
                    .iter_mut()
                    .for_each(|value| rewrite(value, replacements)),
                serde_json::Value::Object(values) => values
                    .values_mut()
                    .for_each(|value| rewrite(value, replacements)),
                serde_json::Value::Null
                | serde_json::Value::Bool(_)
                | serde_json::Value::Number(_) => {}
            }
        }
        rewrite(&mut self.props, &replacements);
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                rewrite(&mut clip.props, &replacements);
            }
        }
    }

    /// Convert local asset paths back to project-relative paths before saving.
    pub fn relativize_local_asset_paths(&mut self, base_dir: impl AsRef<std::path::Path>) {
        let base_dir = base_dir.as_ref();
        let replacements: Vec<(String, String)> = self
            .assets
            .iter_mut()
            .flat_map(|asset| {
                if asset.path.contains("://") || asset.path.starts_with("data:") {
                    return vec![(asset.path.clone(), format!("asset://{}", asset.id))];
                }
                let path = std::path::Path::new(&asset.path);
                let Some(relative) = path.strip_prefix(base_dir).ok() else {
                    return Vec::new();
                };
                let relative = relative.to_string_lossy().into_owned();
                let resolved = asset.path.clone();
                asset.path = relative.clone();
                vec![(resolved, relative)]
            })
            .collect();

        fn rewrite(value: &mut serde_json::Value, replacements: &[(String, String)]) {
            match value {
                serde_json::Value::String(text) => {
                    if let Some((_resolved, relative)) =
                        replacements.iter().find(|(resolved, _)| resolved == text)
                    {
                        *text = relative.clone();
                    }
                }
                serde_json::Value::Array(values) => values
                    .iter_mut()
                    .for_each(|value| rewrite(value, replacements)),
                serde_json::Value::Object(values) => values
                    .values_mut()
                    .for_each(|value| rewrite(value, replacements)),
                serde_json::Value::Null
                | serde_json::Value::Bool(_)
                | serde_json::Value::Number(_) => {}
            }
        }
        rewrite(&mut self.props, &replacements);
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                rewrite(&mut clip.props, &replacements);
            }
        }
    }
}
