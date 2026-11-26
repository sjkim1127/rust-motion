//! The script-facing `SceneBuilder` type and its constructor.

use super::*;

/// A restricted, script-facing builder for the native scene graph.
#[derive(Debug, Clone, Default)]
pub struct SceneBuilder {
    pub(super) scene: Scene,
}

impl SceneBuilder {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn into_scene(self) -> Scene {
        self.scene
    }
}
