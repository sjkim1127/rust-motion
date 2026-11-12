//! Combinator scene emitters for hierarchy, sequences, series, loops, and layers.

use super::traits::{SceneEmitter, SceneFrameContext};
use crate::{CompositionError, NativeComposition, NativeCompositionContext};
use rust_motion_rasterizer::{
    BlendMode, ClipRegion, MaskMode, Scene, SceneFilter, SceneNode, SceneShadow, Transform2D,
};
use serde_json::Value;

/// Heterogeneous emitter collection rendered in insertion order.
#[derive(Default)]
pub struct SceneStack {
    children: Vec<Box<dyn SceneEmitter>>,
}

impl SceneStack {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, child: impl SceneEmitter + 'static) {
        self.children.push(Box::new(child));
    }

    pub fn with(mut self, child: impl SceneEmitter + 'static) -> Self {
        self.push(child);
        self
    }

    pub fn len(&self) -> usize {
        self.children.len()
    }

    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}

impl SceneEmitter for SceneStack {
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        for child in &self.children {
            child.emit(context, props, scene)?;
        }
        Ok(())
    }
}

/// Native equivalent of Rust Motion's `<Sequence>` timeline boundary.
pub struct SceneSequence<E> {
    pub from: u32,
    pub duration_in_frames: Option<u32>,
    pub hidden: bool,
    pub child: E,
}

impl<E> SceneSequence<E> {
    pub fn new(from: u32, child: E) -> Self {
        Self {
            from,
            duration_in_frames: None,
            hidden: false,
            child,
        }
    }

    pub fn with_duration(mut self, duration_in_frames: u32) -> Self {
        self.duration_in_frames = Some(duration_in_frames);
        self
    }

    pub fn hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }
}

impl<E: SceneEmitter> SceneEmitter for SceneSequence<E> {
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let end = self
            .duration_in_frames
            .map(|duration| self.from.saturating_add(duration))
            .unwrap_or(u32::MAX);
        if self.hidden || context.frame < self.from || context.frame >= end {
            return Ok(());
        }
        self.child.emit(
            context.with_local_frame(context.frame - self.from),
            props,
            scene,
        )
    }
}

/// An entry inside a [`SceneSeries`].
pub struct SceneSeriesEntry<E> {
    pub duration_in_frames: u32,
    pub offset: i32,
    pub emitter: E,
}

/// Native equivalent of Remotion's `<Series>`, chaining sub-emitters sequentially in time.
pub struct SceneSeries<E> {
    pub entries: Vec<SceneSeriesEntry<E>>,
}

impl<E> Default for SceneSeries<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E> SceneSeries<E> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn sequence(mut self, duration_in_frames: u32, emitter: E) -> Self {
        self.entries.push(SceneSeriesEntry {
            duration_in_frames,
            offset: 0,
            emitter,
        });
        self
    }

    pub fn sequence_with_offset(
        mut self,
        duration_in_frames: u32,
        offset: i32,
        emitter: E,
    ) -> Self {
        self.entries.push(SceneSeriesEntry {
            duration_in_frames,
            offset,
            emitter,
        });
        self
    }
}

impl<E: SceneEmitter> SceneEmitter for SceneSeries<E> {
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let mut current_start: i64 = 0;
        for entry in &self.entries {
            let from = current_start.saturating_add(i64::from(entry.offset)).max(0);
            let end = from.saturating_add(i64::from(entry.duration_in_frames));
            let frame = i64::from(context.frame);
            if frame >= from && frame < end {
                entry.emitter.emit(
                    context.with_local_frame((frame - from) as u32),
                    props,
                    scene,
                )?;
            }
            current_start = end;
        }
        Ok(())
    }
}

/// Native equivalent of `<Freeze>`.
pub struct SceneFreeze<E> {
    pub frame: u32,
    pub child: E,
}

impl<E> SceneFreeze<E> {
    pub fn new(frame: u32, child: E) -> Self {
        Self { frame, child }
    }
}

impl<E: SceneEmitter> SceneEmitter for SceneFreeze<E> {
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        self.child
            .emit(context.with_local_frame(self.frame), props, scene)
    }
}

/// Applies the Scene graph's transform and opacity primitive to emitted children.
pub struct SceneGroup<E> {
    pub transform: Transform2D,
    pub opacity: f32,
    pub child: E,
}

impl<E> SceneGroup<E> {
    pub fn new(child: E) -> Self {
        Self {
            transform: Transform2D::default(),
            opacity: 1.0,
            child,
        }
    }

    pub fn with_transform(mut self, transform: Transform2D) -> Self {
        self.transform = transform;
        self
    }

    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }
}

impl<E: SceneEmitter> SceneEmitter for SceneGroup<E> {
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let mut child_scene = Scene::new();
        self.child.emit(context, props, &mut child_scene)?;
        if !child_scene.nodes.is_empty() {
            scene.push(SceneNode::Group {
                transform: self.transform,
                opacity: self.opacity,
                children: child_scene.nodes,
            });
        }
        Ok(())
    }
}

/// Native counterpart to After Effects Null Object guide layer.
///
/// Drives parented or nested child emitters spatially without drawing any pixels.
pub struct SceneNullObject<E> {
    pub id: Option<String>,
    pub transform: Transform2D,
    pub opacity: f32,
    pub child: E,
}

impl<E> SceneNullObject<E> {
    pub fn new(child: E) -> Self {
        Self {
            id: None,
            transform: Transform2D::default(),
            opacity: 1.0,
            child,
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn with_transform(mut self, transform: Transform2D) -> Self {
        self.transform = transform;
        self
    }

    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }
}

impl<E: SceneEmitter> SceneEmitter for SceneNullObject<E> {
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let mut child_scene = Scene::new();
        self.child.emit(context, props, &mut child_scene)?;
        if !child_scene.nodes.is_empty() {
            scene.push(SceneNode::NullObject {
                id: self.id.clone(),
                transform: self.transform,
                opacity: self.opacity,
                children: child_scene.nodes,
            });
        }
        Ok(())
    }
}

/// Creates an offscreen compositing boundary around emitted children.
pub struct SceneLayer<E> {
    pub opacity: f32,
    pub blend_mode: BlendMode,
    pub clip: Option<ClipRegion>,
    pub mask: Option<Vec<SceneNode>>,
    pub mask_mode: MaskMode,
    pub filters: Vec<SceneFilter>,
    pub shadow: Option<SceneShadow>,
    pub child: E,
}

impl<E> SceneLayer<E> {
    pub fn new(child: E) -> Self {
        Self {
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: MaskMode::Alpha,
            filters: Vec::new(),
            shadow: None,
            child,
        }
    }

    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    pub fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    pub fn with_clip(mut self, clip: ClipRegion) -> Self {
        self.clip = Some(clip);
        self
    }

    pub fn with_mask(mut self, nodes: impl IntoIterator<Item = SceneNode>, mode: MaskMode) -> Self {
        self.mask = Some(nodes.into_iter().collect());
        self.mask_mode = mode;
        self
    }

    pub fn with_filter(mut self, filter: SceneFilter) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn with_shadow(mut self, shadow: SceneShadow) -> Self {
        self.shadow = Some(shadow);
        self
    }
}

impl<E: SceneEmitter> SceneEmitter for SceneLayer<E> {
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let mut child_scene = Scene::new();
        self.child.emit(context, props, &mut child_scene)?;
        if !child_scene.nodes.is_empty() {
            scene.push(SceneNode::Layer {
                opacity: self.opacity,
                blend_mode: self.blend_mode,
                clip: self.clip.clone(),
                mask: self.mask.clone(),
                mask_mode: self.mask_mode,
                filters: self.filters.clone(),
                shadow: self.shadow.clone(),
                children: child_scene.nodes,
            });
        }
        Ok(())
    }
}

/// Native wrapper turning any [`SceneEmitter`] into a [`NativeComposition`].
pub struct SceneEmitterComposition<E> {
    id: String,
    root: E,
}

impl<E> SceneEmitterComposition<E> {
    pub fn new(id: impl Into<String>, root: E) -> Self {
        Self {
            id: id.into(),
            root,
        }
    }

    pub fn root(&self) -> &E {
        &self.root
    }
}

impl<E: SceneEmitter> NativeComposition for SceneEmitterComposition<E> {
    fn id(&self) -> &str {
        &self.id
    }

    fn render(
        &self,
        frame: u32,
        props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Scene, CompositionError> {
        let mut scene = Scene::new();
        self.root
            .emit(SceneFrameContext::new(frame, context), props, &mut scene)?;
        Ok(scene)
    }
}

/// Loops a child emitter over a duration with an optional repetition count.
pub struct SceneLoop<E> {
    pub duration_in_frames: u32,
    pub times: u32, // 0 means infinite
    pub child: E,
}

impl<E> SceneLoop<E> {
    /// Create a new infinite loop repeating every `duration_in_frames`.
    pub fn new(duration_in_frames: u32, child: E) -> Self {
        Self {
            duration_in_frames,
            times: 0,
            child,
        }
    }

    /// Create a loop that repeats exactly `times` times.
    pub fn with_times(duration_in_frames: u32, times: u32, child: E) -> Self {
        Self {
            duration_in_frames,
            times,
            child,
        }
    }

    /// Fluent builder to set the repetition count (0 = infinite).
    pub fn times(mut self, times: u32) -> Self {
        self.times = times;
        self
    }

    /// Returns the total duration in frames for bounded loops, or `None` for infinite loops.
    pub fn total_duration(&self) -> Option<u32> {
        if self.times > 0 {
            Some(self.duration_in_frames.saturating_mul(self.times))
        } else {
            None
        }
    }
}

impl<E: SceneEmitter> SceneEmitter for SceneLoop<E> {
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        if self.duration_in_frames == 0 {
            return Ok(());
        }

        let duration = self.duration_in_frames;
        let frame = context.frame;

        // If times > 0, check if we've exceeded the total duration
        if self.times > 0 {
            let total_frames = duration.saturating_mul(self.times);
            if frame >= total_frames {
                return Ok(()); // Past the end — render nothing
            }
        }

        // Loop the frame: local_frame = frame % duration
        let local_frame = frame % duration;

        self.child
            .emit(context.with_local_frame(local_frame), props, scene)
    }
}
