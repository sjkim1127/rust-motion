//! Shared native composition contract and built-in composition registry.

pub mod layout;
pub mod mesh3d;
pub mod safe_area;
pub mod scene3d;
mod scene_emitter;
pub mod time;
mod time_events;

pub use layout::{LayoutBox, LayoutChild, SceneFlex, SceneGrid};
pub use mesh3d::{Mesh3DPrimitive, SceneMesh3D};
pub use safe_area::{
    fit_text, get_safe_area_insets, measure_text_approx, Platform, SafeAreaInsets,
};
pub use scene3d::Scene3DEmitter;
pub use scene_emitter::{
    FlipDirection, LinearWipeDirection, SceneAction, SceneAuthoring, SceneAuthoringBuilder,
    SceneEmitter, SceneEmitterComposition, SceneFrameContext, SceneFreeze, SceneGroup, SceneLayer,
    SceneLinearGradient, SceneLoop, SceneNullObject, SceneRect, SceneSequence, SceneSeries,
    SceneSeriesEntry, SceneStack, SceneText, SceneTextBlock, SceneTrail, SceneTrailOpacity,
    SceneTransitionSeries, TransitionKind, TransitionTiming,
};
pub use time::{
    fields_to_frames, format_time, format_timecode_frames, frames_to_fields, parse_timecode,
    FrameRate, ParseError, Tick, TimeDisplay, TimeRange, SAMPLE_RATES, TICKS_PER_SECOND,
};
pub use time_events::{TimeEventError, TimeEventSchedule};

mod builtin;
mod contract;
mod registry;
#[cfg(test)]
mod tests;

pub use builtin::*;
pub use contract::*;
pub use registry::*;
