//! Declarative, deterministic scene authoring API.

use super::traits::{SceneEmitter, SceneFrameContext};
use crate::CompositionError;
use rust_motion_rasterizer::Scene;
use serde_json::Value;

/// A declarative scene-authoring operation.
///
/// Operations are compiled into an immutable schedule by
/// [`SceneAuthoringBuilder::compile`]. `Add` objects remain visible until a
/// matching `Remove`; `Play` emits only during its duration and receives a
/// local frame starting at zero.
pub enum SceneAction {
    Add {
        id: String,
        emitter: Box<dyn SceneEmitter>,
    },
    Remove {
        id: String,
    },
    Play {
        duration_in_frames: u32,
        emitter: Box<dyn SceneEmitter>,
    },
    Wait {
        duration_in_frames: u32,
    },
    Parallel(Vec<SceneAction>),
}

impl SceneAction {
    pub fn add(id: impl Into<String>, emitter: impl SceneEmitter + 'static) -> Self {
        Self::Add {
            id: id.into(),
            emitter: Box::new(emitter),
        }
    }

    pub fn remove(id: impl Into<String>) -> Self {
        Self::Remove { id: id.into() }
    }

    pub fn play(duration_in_frames: u32, emitter: impl SceneEmitter + 'static) -> Self {
        Self::Play {
            duration_in_frames,
            emitter: Box::new(emitter),
        }
    }

    pub fn wait(duration_in_frames: u32) -> Self {
        Self::Wait { duration_in_frames }
    }

    pub fn parallel(actions: impl IntoIterator<Item = Self>) -> Self {
        Self::Parallel(actions.into_iter().collect())
    }
}

/// Builder for deterministic, random-access scene authoring.
///
/// The builder is intentionally separate from [`SceneEmitter`] so existing
/// frame-based compositions do not need to change. Sequential operations
/// advance the playhead; [`SceneAction::parallel`] runs its children from the
/// same playhead and advances by the longest child.
#[derive(Default)]
pub struct SceneAuthoringBuilder {
    actions: Vec<SceneAction>,
}

impl SceneAuthoringBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(mut self, id: impl Into<String>, emitter: impl SceneEmitter + 'static) -> Self {
        self.actions.push(SceneAction::add(id, emitter));
        self
    }

    pub fn remove(mut self, id: impl Into<String>) -> Self {
        self.actions.push(SceneAction::remove(id));
        self
    }

    pub fn play(mut self, duration_in_frames: u32, emitter: impl SceneEmitter + 'static) -> Self {
        self.actions
            .push(SceneAction::play(duration_in_frames, emitter));
        self
    }

    pub fn wait(mut self, duration_in_frames: u32) -> Self {
        self.actions.push(SceneAction::wait(duration_in_frames));
        self
    }

    pub fn parallel(mut self, actions: impl IntoIterator<Item = SceneAction>) -> Self {
        self.actions.push(SceneAction::parallel(actions));
        self
    }

    pub fn compile(self) -> SceneAuthoring {
        let mut schedule = Vec::new();
        let duration_in_frames = compile_actions(self.actions, 0, &mut schedule);
        SceneAuthoring {
            schedule,
            duration_in_frames,
        }
    }
}

enum ScheduledAction {
    Add {
        start: u32,
        id: String,
        emitter: Box<dyn SceneEmitter>,
    },
    Remove {
        start: u32,
        id: String,
    },
    Play {
        start: u32,
        end: u32,
        emitter: Box<dyn SceneEmitter>,
    },
}

fn compile_actions(
    actions: Vec<SceneAction>,
    start: u32,
    schedule: &mut Vec<ScheduledAction>,
) -> u32 {
    let mut elapsed = 0;
    for action in actions {
        let action_start = start.saturating_add(elapsed);
        let action_duration = match action {
            SceneAction::Add { id, emitter } => {
                schedule.push(ScheduledAction::Add {
                    start: action_start,
                    id,
                    emitter,
                });
                0
            }
            SceneAction::Remove { id } => {
                schedule.push(ScheduledAction::Remove {
                    start: action_start,
                    id,
                });
                0
            }
            SceneAction::Play {
                duration_in_frames,
                emitter,
            } => {
                schedule.push(ScheduledAction::Play {
                    start: action_start,
                    end: action_start.saturating_add(duration_in_frames),
                    emitter,
                });
                duration_in_frames
            }
            SceneAction::Wait { duration_in_frames } => duration_in_frames,
            SceneAction::Parallel(children) => {
                let mut duration = 0;
                for child in children {
                    duration = duration.max(compile_actions(vec![child], action_start, schedule));
                }
                duration
            }
        };
        elapsed = elapsed.saturating_add(action_duration);
    }
    elapsed
}

/// Immutable, frame-evaluable result of [`SceneAuthoringBuilder::compile`].
pub struct SceneAuthoring {
    schedule: Vec<ScheduledAction>,
    duration_in_frames: u32,
}

impl SceneAuthoring {
    pub fn duration_in_frames(&self) -> u32 {
        self.duration_in_frames
    }
}

impl SceneEmitter for SceneAuthoring {
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let mut active: Vec<(&str, &dyn SceneEmitter)> = Vec::new();
        for action in &self.schedule {
            match action {
                ScheduledAction::Add { start, id, emitter } if *start <= context.frame => {
                    active.retain(|(active_id, _)| *active_id != id);
                    active.push((id.as_str(), emitter.as_ref()));
                }
                ScheduledAction::Remove { start, id } if *start <= context.frame => {
                    active.retain(|(active_id, _)| *active_id != id);
                }
                _ => {}
            }
        }
        for (_, emitter) in active {
            emitter.emit(context, props, scene)?;
        }
        for action in &self.schedule {
            if let ScheduledAction::Play {
                start,
                end,
                emitter,
            } = action
            {
                if context.frame >= *start && context.frame < *end {
                    emitter.emit(
                        context.with_local_frame(context.frame - start),
                        props,
                        scene,
                    )?;
                }
            }
        }
        Ok(())
    }
}
