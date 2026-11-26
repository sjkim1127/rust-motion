//! Loading and atomically saving project JSON files.

use super::*;

impl Project {
    pub fn from_json_str(source: &str) -> Result<Self, ProjectError> {
        let project: Self =
            serde_json::from_str(source).map_err(|error| ProjectError::Json(error.to_string()))?;
        project.validate()?;
        Ok(project)
    }

    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, ProjectError> {
        let source =
            std::fs::read_to_string(path).map_err(|error| ProjectError::File(error.to_string()))?;
        Self::from_json_str(&source)
    }

    pub fn save(&self, path: impl AsRef<std::path::Path>) -> Result<(), ProjectError> {
        self.validate()?;
        use std::io::Write;

        let path = path.as_ref();
        let source = serde_json::to_vec_pretty(self)
            .map_err(|error| ProjectError::Json(error.to_string()))?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .map_err(|error| ProjectError::File(error.to_string()))?;
        temporary
            .write_all(&source)
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|error| ProjectError::File(error.to_string()))?;
        if let Ok(metadata) = std::fs::metadata(path) {
            temporary
                .as_file()
                .set_permissions(metadata.permissions())
                .map_err(|error| ProjectError::File(error.to_string()))?;
        }
        temporary
            .persist(path)
            .map(|_| ())
            .map_err(|error| ProjectError::File(error.to_string()))
    }
}
