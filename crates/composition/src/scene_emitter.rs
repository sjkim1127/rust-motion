//! Composable browser-free scene emitters.

use crate::{CompositionError, NativeComposition, NativeCompositionContext};
use dioxuscut_rasterizer::{
    layout_text_box, BlendMode, ClipRegion, Color, GradientStop, MaskMode, Scene, SceneFilter,
    SceneNode, SceneShadow, TextBox, TextHorizontalAlign, TextOverflow, TextVerticalAlign,
    Transform2D,
};
use serde_json::Value;

/// Timeline state passed through a native scene-emitter tree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneFrameContext {
    /// Frame local to the current sequence or freeze boundary.
    pub frame: u32,
    /// Unmodified composition frame.
    pub global_frame: u32,
    pub composition: NativeCompositionContext,
}

impl SceneFrameContext {
    pub fn new(frame: u32, composition: NativeCompositionContext) -> Self {
        Self {
            frame,
            global_frame: frame,
            composition,
        }
    }

    pub fn time_secs(self) -> f64 {
        self.frame as f64 / self.composition.fps
    }

    pub fn global_time_secs(self) -> f64 {
        self.global_frame as f64 / self.composition.fps
    }

    fn with_local_frame(self, frame: u32) -> Self {
        Self { frame, ..self }
    }
}

/// Shared primitive contract for media, shapes, captions, transitions, and apps.
///
/// Emitters append nodes to the supplied scene and can be nested through
/// [`SceneSequence`], [`SceneFreeze`], [`SceneGroup`], and [`SceneStack`].
pub trait SceneEmitter: Send + Sync {
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError>;
}

impl<F> SceneEmitter for F
where
    F: Fn(SceneFrameContext, &Value, &mut Scene) -> Result<(), CompositionError> + Send + Sync,
{
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        self(context, props, scene)
    }
}

impl SceneEmitter for SceneNode {
    fn emit(
        &self,
        _context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        scene.push(self.clone());
        Ok(())
    }
}

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

/// Native equivalent of Dioxuscut's `<Sequence>` timeline boundary.
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

/// Font-aware multiline text that resolves into ordinary baseline Text nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneTextBlock {
    pub layout: TextBox,
    pub color: Color,
    pub font_weight: u16,
}

impl SceneTextBlock {
    pub fn new(
        text: impl Into<String>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        font_size: f32,
    ) -> Self {
        Self {
            layout: TextBox::new(text, x, y, width, height, font_size),
            color: Color::WHITE,
            font_weight: 400,
        }
    }

    pub fn with_min_font_size(mut self, min_font_size: f32) -> Self {
        self.layout.min_font_size = min_font_size;
        self
    }

    pub fn with_line_height(mut self, line_height: f32) -> Self {
        self.layout.line_height = line_height;
        self
    }

    pub fn with_max_lines(mut self, max_lines: usize) -> Self {
        self.layout.max_lines = Some(max_lines);
        self
    }

    pub fn with_alignment(
        mut self,
        horizontal: TextHorizontalAlign,
        vertical: TextVerticalAlign,
    ) -> Self {
        self.layout.horizontal_align = horizontal;
        self.layout.vertical_align = vertical;
        self
    }

    pub fn with_overflow(mut self, overflow: TextOverflow) -> Self {
        self.layout.overflow = overflow;
        self
    }

    pub fn with_font_sources(mut self, sources: impl IntoIterator<Item = String>) -> Self {
        self.layout.font_sources = sources.into_iter().collect();
        self
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn with_font_weight(mut self, font_weight: u16) -> Self {
        self.font_weight = font_weight;
        self
    }
}

impl SceneEmitter for SceneTextBlock {
    fn emit(
        &self,
        context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let layout = layout_text_box(&self.layout).map_err(|error| {
            CompositionError::render(
                context.global_frame,
                format!("failed to layout text block: {error}"),
            )
        })?;
        for line in layout.lines {
            if line.text.is_empty() {
                continue;
            }
            scene.push(SceneNode::Text {
                x: line.x,
                y: line.y,
                content: line.text,
                font_size: layout.font_size,
                color: self.color,
                font_weight: self.font_weight,
                font_sources: self.layout.font_sources.clone(),
            });
        }
        Ok(())
    }
}

/// Adapts an emitter tree to the existing composition registry contract.
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

// ── Primitive emitters ────────────────────────────────────────────────────────

/// Solid-colour rectangle placed directly on the scene.
///
/// Equivalent to emitting a [`SceneNode::Rect`] without having to construct it manually.
/// Use [`SceneTextBlock`] for multi-line text layout.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub fill: Color,
    pub stroke: Option<Color>,
    pub stroke_width: f32,
    pub corner_radius: f32,
}

impl SceneRect {
    pub fn new(x: f32, y: f32, w: f32, h: f32, fill: Color) -> Self {
        Self {
            x,
            y,
            w: w.max(0.0),
            h: h.max(0.0),
            fill,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }
    }

    pub fn with_stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some(color);
        self.stroke_width = width.max(0.0);
        self
    }

    pub fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius.max(0.0);
        self
    }
}

impl SceneEmitter for SceneRect {
    fn emit(
        &self,
        _context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        scene.push(SceneNode::Rect {
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.h,
            fill: self.fill,
            stroke: self.stroke,
            stroke_width: self.stroke_width,
            corner_radius: self.corner_radius,
        });
        Ok(())
    }
}

/// Single-line text placed directly on the scene.
///
/// Use [`SceneTextBlock`] when you need word-wrapping, multi-line layout, or
/// alignment/overflow control. `SceneText` is a zero-cost shortcut for placing
/// a pre-positioned single text run.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneText {
    pub x: f32,
    pub y: f32,
    pub content: String,
    pub font_size: f32,
    pub color: Color,
    pub font_weight: u16,
    pub font_sources: Vec<String>,
}

impl SceneText {
    pub fn new(content: impl Into<String>, x: f32, y: f32, font_size: f32, color: Color) -> Self {
        Self {
            x,
            y,
            content: content.into(),
            font_size: font_size.max(0.0),
            color,
            font_weight: 400,
            font_sources: Vec::new(),
        }
    }

    pub fn with_font_weight(mut self, weight: u16) -> Self {
        self.font_weight = weight;
        self
    }

    pub fn with_font_sources(mut self, sources: impl IntoIterator<Item = String>) -> Self {
        self.font_sources = sources.into_iter().collect();
        self
    }
}

impl SceneEmitter for SceneText {
    fn emit(
        &self,
        _context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        if self.content.is_empty() {
            return Ok(());
        }
        scene.push(SceneNode::Text {
            x: self.x,
            y: self.y,
            content: self.content.clone(),
            font_size: self.font_size,
            color: self.color,
            font_weight: self.font_weight,
            font_sources: self.font_sources.clone(),
        });
        Ok(())
    }
}

/// Linear gradient rectangle placed directly on the scene.
///
/// A convenience wrapper around [`SceneNode::LinearGradient`] for use inside
/// emitter trees.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneLinearGradient {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub angle_deg: f32,
    pub stops: Vec<GradientStop>,
}

// ── SceneLoop ─────────────────────────────────────────────────────────────────

/// Repeats a child emitter for a fixed number of frames, looping the local
/// frame back to zero every `duration_in_frames` frames.
///
/// Equivalent to Remotion's `<Loop durationInFrames={n} times={m}>` component.
///
/// # Fields
/// - `duration_in_frames`: length of one loop iteration in frames
/// - `times`: number of repetitions (0 = infinite)
/// - `child`: the emitter to loop
#[derive(Debug, Clone, PartialEq)]
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
        let duration = self.duration_in_frames.max(1);
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

// ── SceneTransitionSeries ─────────────────────────────────────────────────────

/// Direction for linear wipes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinearWipeDirection {
    FromLeft,
    FromRight,
    FromTop,
    FromBottom,
    FromTopLeft,
    FromTopRight,
    FromBottomLeft,
    FromBottomRight,
}

/// Direction for 3D card flips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlipDirection {
    FromRight,
    FromLeft,
    FromTop,
    FromBottom,
}

/// A transition type between consecutive clips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransitionKind {
    Fade,
    SlideLeft,
    SlideRight,
    SlideUp,
    SlideDown,
    ClockWipe,
    LinearWipe(LinearWipeDirection),
    Flip(FlipDirection),
    Zoom,
    Iris,
    Dissolve,
}

/// Timing for a transition: how many frames the overlap lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransitionTiming {
    pub duration_in_frames: u32,
}

impl TransitionTiming {
    pub fn new(duration_in_frames: u32) -> Self {
        Self { duration_in_frames }
    }
}

/// A clip in the TransitionSeries.
struct TsClip {
    duration_in_frames: u32,
    emitter: Box<dyn SceneEmitter>,
}

/// A transition between clips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TsTransition {
    kind: TransitionKind,
    timing: TransitionTiming,
}

/// Builder for `SceneTransitionSeries`.
///
/// Usage:
/// ```rust,ignore
/// let series = SceneTransitionSeries::new()
///     .clip(60, scene1)
///     .transition(TransitionKind::Fade, TransitionTiming::new(15))
///     .clip(60, scene2)
///     .transition(TransitionKind::SlideLeft, TransitionTiming::new(20))
///     .clip(60, scene3);
/// ```
pub struct SceneTransitionSeries {
    clips: Vec<TsClip>,
    transitions: Vec<(usize, TsTransition)>, // (after clip index, transition)
}

impl SceneTransitionSeries {
    pub fn new() -> Self {
        Self {
            clips: Vec::new(),
            transitions: Vec::new(),
        }
    }

    pub fn clip<E: SceneEmitter + 'static>(mut self, duration_in_frames: u32, emitter: E) -> Self {
        self.clips.push(TsClip {
            duration_in_frames,
            emitter: Box::new(emitter),
        });
        self
    }

    pub fn transition(mut self, kind: TransitionKind, timing: TransitionTiming) -> Self {
        // Associate transition with the last added clip (before the next one)
        let after_index = self.clips.len().saturating_sub(1);
        self.transitions
            .push((after_index, TsTransition { kind, timing }));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    pub fn len(&self) -> usize {
        self.clips.len()
    }

    /// Computes clip start offsets and effective transition overlap durations.
    /// Overlap durations are clamped to the duration of both adjacent clips.
    pub fn calculate_timeline(&self) -> (Vec<u32>, Vec<u32>) {
        let n = self.clips.len();
        if n == 0 {
            return (Vec::new(), Vec::new());
        }

        let mut transitions_map = std::collections::BTreeMap::new();
        for (idx, trans) in &self.transitions {
            transitions_map.insert(*idx, trans);
        }

        let mut overlaps = Vec::with_capacity(n.saturating_sub(1));
        for i in 0..n.saturating_sub(1) {
            let len_cur = self.clips[i].duration_in_frames;
            let len_next = self.clips[i + 1].duration_in_frames;
            let overlap = transitions_map
                .get(&i)
                .map(|t| t.timing.duration_in_frames.min(len_cur).min(len_next))
                .unwrap_or(0);
            overlaps.push(overlap);
        }

        let mut starts = Vec::with_capacity(n);
        let mut offset: u32 = 0;
        for (i, clip) in self.clips.iter().enumerate() {
            starts.push(offset);
            if i < overlaps.len() {
                offset = offset
                    .saturating_add(clip.duration_in_frames)
                    .saturating_sub(overlaps[i]);
            }
        }

        (starts, overlaps)
    }

    /// Total timeline duration in frames, accounting for transition overlaps.
    pub fn total_duration(&self) -> u32 {
        if self.clips.is_empty() {
            return 0;
        }
        let (starts, _) = self.calculate_timeline();
        let last_idx = self.clips.len() - 1;
        starts[last_idx].saturating_add(self.clips[last_idx].duration_in_frames)
    }

    /// Alias for `total_duration`.
    pub fn duration_in_frames(&self) -> u32 {
        self.total_duration()
    }
}

impl Default for SceneTransitionSeries {
    fn default() -> Self {
        Self::new()
    }
}

fn clock_wipe_path(width: f32, height: f32, progress: f32) -> String {
    let p = progress.clamp(0.0, 1.0);
    if p <= 0.0 {
        return "M 0 0 Z".to_string();
    }
    if p >= 1.0 {
        return format!("M 0 0 L {width} 0 L {width} {height} L 0 {height} Z");
    }
    let cx = width / 2.0;
    let cy = height / 2.0;
    let radius = (width * width + height * height).sqrt() / 2.0 * 1.05;
    let start_deg = -90.0f32;
    let end_deg = start_deg + p * 360.0;
    let start_rad = start_deg.to_radians();
    let end_rad = end_deg.to_radians();
    let x0 = cx + radius * start_rad.cos();
    let y0 = cy + radius * start_rad.sin();
    let x1 = cx + radius * end_rad.cos();
    let y1 = cy + radius * end_rad.sin();
    let large_arc_flag = if p > 0.5 { 1 } else { 0 };
    format!(
        "M {cx:.2} {cy:.2} L {x0:.2} {y0:.2} A {radius:.2} {radius:.2} 0 {large_arc_flag} 1 {x1:.2} {y1:.2} Z"
    )
}

fn iris_path(width: f32, height: f32, progress: f32) -> String {
    let p = progress.clamp(0.0, 1.0);
    if p <= 0.0 {
        return "M 0 0 Z".to_string();
    }
    if p >= 1.0 {
        return format!("M 0 0 L {width} 0 L {width} {height} L 0 {height} Z");
    }
    let cx = width / 2.0;
    let cy = height / 2.0;
    let max_r = (width * width + height * height).sqrt() / 2.0 * 1.05;
    let r = p * max_r;
    let d = 2.0 * r;
    format!(
        "M {cx:.2} {cy:.2} m -{r:.2}, 0 a {r:.2},{r:.2} 0 1,0 {d:.2},0 a {r:.2},{r:.2} 0 1,0 -{d:.2},0 Z"
    )
}

fn linear_wipe_path(dir: LinearWipeDirection, width: f32, height: f32, progress: f32) -> String {
    let p = progress.clamp(0.0, 1.0);
    if p <= 0.0 {
        return "M 0 0 Z".to_string();
    }
    if p >= 1.0 {
        return format!("M 0 0 L {width} 0 L {width} {height} L 0 {height} Z");
    }
    match dir {
        LinearWipeDirection::FromLeft => {
            let w = width * p;
            format!("M 0 0 L {w:.2} 0 L {w:.2} {height:.2} L 0 {height:.2} Z")
        }
        LinearWipeDirection::FromRight => {
            let x0 = width * (1.0 - p);
            format!("M {x0:.2} 0 L {width:.2} 0 L {width:.2} {height:.2} L {x0:.2} {height:.2} Z")
        }
        LinearWipeDirection::FromTop => {
            let h = height * p;
            format!("M 0 0 L {width:.2} 0 L {width:.2} {h:.2} L 0 {h:.2} Z")
        }
        LinearWipeDirection::FromBottom => {
            let y0 = height * (1.0 - p);
            format!("M 0 {y0:.2} L {width:.2} {y0:.2} L {width:.2} {height:.2} L 0 {height:.2} Z")
        }
        LinearWipeDirection::FromTopLeft => {
            let d = 2.0 * p;
            format!("M 0 0 L {:.2} 0 L 0 {:.2} Z", width * d, height * d)
        }
        LinearWipeDirection::FromTopRight => {
            let d = 2.0 * p;
            format!(
                "M {width:.2} 0 L {:.2} 0 L {width:.2} {:.2} Z",
                width * (1.0 - d),
                height * d
            )
        }
        LinearWipeDirection::FromBottomLeft => {
            let d = 2.0 * p;
            format!(
                "M 0 {height:.2} L 0 {:.2} L {:.2} {height:.2} Z",
                height * (1.0 - d),
                width * d
            )
        }
        LinearWipeDirection::FromBottomRight => {
            let d = 2.0 * p;
            format!(
                "M {width:.2} {height:.2} L {:.2} {height:.2} L {width:.2} {:.2} Z",
                width * (1.0 - d),
                height * (1.0 - d)
            )
        }
    }
}

impl SceneEmitter for SceneTransitionSeries {
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        if self.clips.is_empty() {
            return Ok(());
        }

        let (starts, overlaps) = self.calculate_timeline();
        let mut transitions_map = std::collections::BTreeMap::new();
        for (idx, trans) in &self.transitions {
            transitions_map.insert(*idx, trans);
        }

        let frame = context.frame;
        let width = context.composition.width as f32;
        let height = context.composition.height as f32;

        for (i, clip) in self.clips.iter().enumerate() {
            let start = starts[i];
            let duration = clip.duration_in_frames;
            let end = start.saturating_add(duration);

            if frame < start || frame >= end {
                continue;
            }

            let local_frame = frame - start;
            let clip_context = context.with_local_frame(local_frame);

            // Incoming transition (enter from clip i - 1)
            let mut alpha_in = 1.0f32;
            let mut tx_in = 0.0f32;
            let mut ty_in = 0.0f32;
            let mut scale_x_in = 1.0f32;
            let mut scale_y_in = 1.0f32;
            let mut clip_in: Option<ClipRegion> = None;

            if i > 0 && !overlaps.is_empty() {
                let overlap_in = overlaps[i - 1];
                if overlap_in > 0 && local_frame < overlap_in {
                    let p_in = (local_frame as f32 / overlap_in as f32).clamp(0.0, 1.0);
                    if let Some(trans) = transitions_map.get(&(i - 1)) {
                        match trans.kind {
                            TransitionKind::Fade | TransitionKind::Dissolve => {
                                alpha_in = p_in;
                            }
                            TransitionKind::SlideLeft => {
                                tx_in = (1.0 - p_in) * width;
                            }
                            TransitionKind::SlideRight => {
                                tx_in = -(1.0 - p_in) * width;
                            }
                            TransitionKind::SlideUp => {
                                ty_in = (1.0 - p_in) * height;
                            }
                            TransitionKind::SlideDown => {
                                ty_in = -(1.0 - p_in) * height;
                            }
                            TransitionKind::ClockWipe => {
                                clip_in = Some(ClipRegion::Path {
                                    d: clock_wipe_path(width, height, p_in),
                                });
                            }
                            TransitionKind::LinearWipe(dir) => {
                                clip_in = Some(ClipRegion::Path {
                                    d: linear_wipe_path(dir, width, height, p_in),
                                });
                            }
                            TransitionKind::Iris => {
                                clip_in = Some(ClipRegion::Path {
                                    d: iris_path(width, height, p_in),
                                });
                            }
                            TransitionKind::Flip(dir) => {
                                if p_in < 0.5 {
                                    alpha_in = 0.0;
                                } else {
                                    let local_p = (p_in - 0.5) * 2.0;
                                    let s = ((1.0 - local_p) * 90.0f32).to_radians().cos().max(0.0);
                                    match dir {
                                        FlipDirection::FromRight | FlipDirection::FromLeft => {
                                            scale_x_in = s;
                                            tx_in = (width / 2.0) * (1.0 - s);
                                        }
                                        FlipDirection::FromTop | FlipDirection::FromBottom => {
                                            scale_y_in = s;
                                            ty_in = (height / 2.0) * (1.0 - s);
                                        }
                                    }
                                }
                            }
                            TransitionKind::Zoom => {
                                alpha_in = p_in;
                                let s = p_in;
                                scale_x_in = s;
                                scale_y_in = s;
                                tx_in = (width / 2.0) * (1.0 - s);
                                ty_in = (height / 2.0) * (1.0 - s);
                            }
                        }
                    }
                }
            }

            // Outgoing transition (exit to clip i + 1)
            let mut alpha_out = 1.0f32;
            let mut tx_out = 0.0f32;
            let mut ty_out = 0.0f32;
            let mut scale_x_out = 1.0f32;
            let mut scale_y_out = 1.0f32;

            if i < overlaps.len() {
                let overlap_out = overlaps[i];
                let out_start = duration.saturating_sub(overlap_out);
                if overlap_out > 0 && local_frame >= out_start {
                    let p_out =
                        ((local_frame - out_start) as f32 / overlap_out as f32).clamp(0.0, 1.0);
                    if let Some(trans) = transitions_map.get(&i) {
                        match trans.kind {
                            TransitionKind::Fade | TransitionKind::Dissolve => {
                                alpha_out = 1.0 - p_out;
                            }
                            TransitionKind::SlideLeft => {
                                tx_out = -p_out * width;
                            }
                            TransitionKind::SlideRight => {
                                tx_out = p_out * width;
                            }
                            TransitionKind::SlideUp => {
                                ty_out = -p_out * height;
                            }
                            TransitionKind::SlideDown => {
                                ty_out = p_out * height;
                            }
                            TransitionKind::ClockWipe
                            | TransitionKind::LinearWipe(_)
                            | TransitionKind::Iris => {
                                // Exiting clip remains visible beneath entering clip's clip-path
                            }
                            TransitionKind::Flip(dir) => {
                                if p_out >= 0.5 {
                                    alpha_out = 0.0;
                                } else {
                                    let local_p = p_out * 2.0;
                                    let s = (local_p * 90.0f32).to_radians().cos().max(0.0);
                                    match dir {
                                        FlipDirection::FromRight | FlipDirection::FromLeft => {
                                            scale_x_out = s;
                                            tx_out = (width / 2.0) * (1.0 - s);
                                        }
                                        FlipDirection::FromTop | FlipDirection::FromBottom => {
                                            scale_y_out = s;
                                            ty_out = (height / 2.0) * (1.0 - s);
                                        }
                                    }
                                }
                            }
                            TransitionKind::Zoom => {
                                alpha_out = 1.0 - p_out;
                                let s = 1.0 + 0.5 * p_out;
                                scale_x_out = s;
                                scale_y_out = s;
                                tx_out = (width / 2.0) * (1.0 - s);
                                ty_out = (height / 2.0) * (1.0 - s);
                            }
                        }
                    }
                }
            }

            let total_alpha = (alpha_in * alpha_out).clamp(0.0, 1.0);
            let total_tx = tx_in + tx_out;
            let total_ty = ty_in + ty_out;
            let total_scale_x = scale_x_in * scale_x_out;
            let total_scale_y = scale_y_in * scale_y_out;

            if let Some(clip_region) = clip_in {
                let mut sub_scene = Scene::new();
                clip.emitter.emit(clip_context, props, &mut sub_scene)?;
                if !sub_scene.nodes.is_empty() {
                    let children = if total_tx.abs() > 1e-5
                        || total_ty.abs() > 1e-5
                        || (total_scale_x - 1.0).abs() > 1e-5
                        || (total_scale_y - 1.0).abs() > 1e-5
                    {
                        vec![SceneNode::Group {
                            transform: Transform2D {
                                tx: total_tx,
                                ty: total_ty,
                                scale_x: total_scale_x,
                                scale_y: total_scale_y,
                                rotate_deg: 0.0,
                            },
                            opacity: 1.0,
                            children: sub_scene.nodes,
                        }]
                    } else {
                        sub_scene.nodes
                    };
                    scene.push(SceneNode::Layer {
                        opacity: total_alpha,
                        blend_mode: BlendMode::Normal,
                        clip: Some(clip_region),
                        mask: None,
                        mask_mode: MaskMode::Alpha,
                        filters: Vec::new(),
                        shadow: None,
                        children,
                    });
                }
            } else {
                let needs_group = (total_alpha - 1.0).abs() > 1e-5
                    || total_tx.abs() > 1e-5
                    || total_ty.abs() > 1e-5
                    || (total_scale_x - 1.0).abs() > 1e-5
                    || (total_scale_y - 1.0).abs() > 1e-5;

                if needs_group {
                    let mut sub_scene = Scene::new();
                    clip.emitter.emit(clip_context, props, &mut sub_scene)?;
                    if !sub_scene.nodes.is_empty() {
                        scene.push(SceneNode::Group {
                            transform: Transform2D {
                                tx: total_tx,
                                ty: total_ty,
                                scale_x: total_scale_x,
                                scale_y: total_scale_y,
                                rotate_deg: 0.0,
                            },
                            opacity: total_alpha,
                            children: sub_scene.nodes,
                        });
                    }
                } else {
                    clip.emitter.emit(clip_context, props, scene)?;
                }
            }
        }

        Ok(())
    }
}

impl SceneLinearGradient {
    /// Horizontal left-to-right gradient (0°) spanning the given rectangle.
    pub fn new(x: f32, y: f32, w: f32, h: f32, stops: Vec<GradientStop>) -> Self {
        Self {
            x,
            y,
            w: w.max(0.0),
            h: h.max(0.0),
            angle_deg: 0.0,
            stops,
        }
    }

    pub fn with_angle(mut self, angle_deg: f32) -> Self {
        self.angle_deg = angle_deg;
        self
    }
}

impl SceneEmitter for SceneLinearGradient {
    fn emit(
        &self,
        _context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        scene.push(SceneNode::LinearGradient {
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.h,
            angle_deg: self.angle_deg,
            stops: self.stops.clone(),
        });
        Ok(())
    }
}

// ── SceneTrail ────────────────────────────────────────────────────────────────

/// Strategy for calculating opacity across ghost layers in a trail.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SceneTrailOpacity {
    /// Linear decay: newest layer is base_opacity, oldest approaches 0.
    #[default]
    Linear,
    /// Exponential decay: opacity = base_opacity * decay_factor^layer_index.
    Exponential(f32),
    /// Uniform opacity across all trail layers.
    Uniform(f32),
}

/// Renders ghost trailing copies of child elements from preceding frames.
///
/// Ported from `@remotion/motion-blur`'s `<Trail />` component.
///
/// # Fields
/// - `layers`: Number of trailing ghost layers (e.g. 5)
/// - `lag_in_frames`: Frame delay between consecutive ghost layers (e.g. 1 or 2)
/// - `trail_opacity`: Opacity decay strategy across trailing layers
/// - `child`: Inner emitter to produce ghost trails for
pub struct SceneTrail<E> {
    pub layers: u32,
    pub lag_in_frames: u32,
    pub trail_opacity: SceneTrailOpacity,
    pub child: E,
}

impl<E> SceneTrail<E> {
    /// Creates a new trail with the specified number of layers and frame lag.
    pub fn new(layers: u32, lag_in_frames: u32, child: E) -> Self {
        Self {
            layers: layers.max(1),
            lag_in_frames: lag_in_frames.max(1),
            trail_opacity: SceneTrailOpacity::Linear,
            child,
        }
    }

    /// Customizes the opacity decay across trail layers.
    pub fn with_opacity_strategy(mut self, strategy: SceneTrailOpacity) -> Self {
        self.trail_opacity = strategy;
        self
    }
}

impl<E: SceneEmitter> SceneEmitter for SceneTrail<E> {
    fn emit(
        &self,
        context: SceneFrameContext,
        props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let total_layers = self.layers.max(1);

        // Render from oldest ghost layer (layer = total_layers - 1) to newest (layer = 0)
        // so that newer frames render on top.
        for layer_idx in (0..total_layers).rev() {
            let frame_lag = layer_idx * self.lag_in_frames;
            let ghost_frame = context.frame.saturating_sub(frame_lag);

            let opacity = match self.trail_opacity {
                SceneTrailOpacity::Linear => {
                    // layer 0 (current) -> 1.0; oldest layer -> 1.0 / (total_layers + 1)
                    1.0 - (layer_idx as f32 / total_layers as f32) * 0.8
                }
                SceneTrailOpacity::Exponential(decay) => {
                    decay.clamp(0.0, 1.0).powi(layer_idx as i32)
                }
                SceneTrailOpacity::Uniform(val) => val.clamp(0.0, 1.0),
            };

            let ghost_context = context.with_local_frame(ghost_frame);

            if (opacity - 1.0).abs() < 1e-4 {
                self.child.emit(ghost_context, props, scene)?;
            } else if opacity > 1e-4 {
                let mut ghost_scene = Scene::new();
                self.child.emit(ghost_context, props, &mut ghost_scene)?;
                if !ghost_scene.nodes.is_empty() {
                    scene.push(SceneNode::Group {
                        transform: Transform2D::identity(),
                        opacity,
                        children: ghost_scene.nodes,
                    });
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dioxuscut_rasterizer::{Color, ImageFit};

    fn context() -> NativeCompositionContext {
        NativeCompositionContext {
            width: 320,
            height: 180,
            fps: 30.0,
            duration_in_frames: 90,
        }
    }

    fn frame_text() -> impl SceneEmitter {
        |context: SceneFrameContext, _props: &Value, scene: &mut Scene| {
            scene.push(SceneNode::Text {
                x: 0.0,
                y: 20.0,
                content: context.frame.to_string(),
                font_size: 20.0,
                color: Color::WHITE,
                font_weight: 400,
                font_sources: Vec::new(),
            });
            Ok(())
        }
    }

    #[test]
    fn sequence_uses_local_frames_and_duration() {
        let composition = SceneEmitterComposition::new(
            "sequence",
            SceneSequence::new(10, frame_text()).with_duration(5),
        );
        let active = composition.render(12, &Value::Null, context()).unwrap();
        let inactive = composition.render(15, &Value::Null, context()).unwrap();

        assert!(matches!(
            &active.nodes[0],
            SceneNode::Text { content, .. } if content == "2"
        ));
        assert!(inactive.nodes.is_empty());
    }

    #[test]
    fn series_keeps_later_entries_outside_the_u32_frame_domain() {
        let series = SceneSeries::new()
            .sequence(u32::MAX, frame_text())
            .sequence_with_offset(1, 1, frame_text())
            .sequence_with_offset(1, -1, frame_text());
        let mut scene = Scene::new();

        series
            .emit(
                SceneFrameContext::new(0, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();

        assert_eq!(scene.nodes.len(), 1);
        assert!(matches!(
            &scene.nodes[0],
            SceneNode::Text { content, .. } if content == "0"
        ));
    }

    #[test]
    fn series_does_not_wrap_after_large_positive_offset() {
        let series = SceneSeries::new()
            .sequence(i32::MAX as u32 + 2, frame_text())
            .sequence_with_offset(1, i32::MAX, frame_text());
        let mut scene = Scene::new();

        series
            .emit(
                SceneFrameContext::new(0, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();

        assert_eq!(scene.nodes.len(), 1);
    }

    #[test]
    fn series_can_render_a_one_frame_entry_at_u32_max() {
        let series = SceneSeries::new()
            .sequence(u32::MAX, frame_text())
            .sequence_with_offset(1, 0, frame_text());
        let mut scene = Scene::new();

        series
            .emit(
                SceneFrameContext::new(u32::MAX, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();

        assert_eq!(scene.nodes.len(), 1);
        assert!(matches!(
            &scene.nodes[0],
            SceneNode::Text { content, .. } if content == "0"
        ));
    }

    #[test]
    fn freeze_replaces_only_the_local_frame() {
        let composition = SceneEmitterComposition::new("freeze", SceneFreeze::new(7, frame_text()));
        let scene = composition.render(42, &Value::Null, context()).unwrap();
        assert!(matches!(
            &scene.nodes[0],
            SceneNode::Text { content, .. } if content == "7"
        ));
    }

    #[test]
    fn stack_and_group_preserve_primitive_order() {
        let image = SceneNode::Image {
            src: "card.png".into(),
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
            fit: ImageFit::Contain,
            opacity: 1.0,
        };
        let stack = SceneStack::new().with(image).with(frame_text());
        let composition =
            SceneEmitterComposition::new("group", SceneGroup::new(stack).with_opacity(0.5));
        let scene = composition.render(3, &Value::Null, context()).unwrap();

        assert!(matches!(
            &scene.nodes[0],
            SceneNode::Group { opacity, children, .. }
                if (*opacity - 0.5).abs() < f32::EPSILON && children.len() == 2
        ));
    }

    #[test]
    fn layer_collects_compositing_options_and_children() {
        let layer = SceneLayer::new(frame_text())
            .with_opacity(0.75)
            .with_blend_mode(BlendMode::Multiply)
            .with_clip(ClipRegion::Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
                corner_radius: 8.0,
            })
            .with_filter(SceneFilter::Grayscale { amount: 1.0 })
            .with_shadow(SceneShadow {
                offset_x: 4.0,
                offset_y: 6.0,
                blur_sigma: 3.0,
                color: Color::rgba(0, 0, 0, 128),
            });
        let composition = SceneEmitterComposition::new("layer", layer);
        let scene = composition.render(3, &Value::Null, context()).unwrap();

        assert!(matches!(
            &scene.nodes[0],
            SceneNode::Layer {
                opacity,
                blend_mode: BlendMode::Multiply,
                clip: Some(ClipRegion::Rect { .. }),
                filters,
                shadow: Some(_),
                children,
                ..
            } if (*opacity - 0.75).abs() < f32::EPSILON
                && filters.len() == 1
                && children.len() == 1
        ));
    }

    #[test]
    fn text_block_resolves_wrapping_fitting_and_alignment() {
        let font_cache = dioxuscut_rasterizer::FontCache::load();
        let Some(font) = font_cache.font_path() else {
            return;
        };
        let block =
            SceneTextBlock::new("one two three four five six", 10.0, 20.0, 120.0, 52.0, 32.0)
                .with_min_font_size(14.0)
                .with_max_lines(2)
                .with_alignment(TextHorizontalAlign::Center, TextVerticalAlign::Center)
                .with_overflow(TextOverflow::Ellipsis)
                .with_font_sources([font.to_string()]);
        let composition = SceneEmitterComposition::new("text-block", block);
        let scene = composition.render(0, &Value::Null, context()).unwrap();

        assert!(!scene.nodes.is_empty());
        assert!(scene.nodes.len() <= 2);
        assert!(scene.nodes.iter().all(|node| matches!(
            node,
            SceneNode::Text { x, font_size, .. } if *x >= 10.0 && *font_size <= 32.0
        )));
    }

    #[test]
    fn scene_rect_emits_correct_rect_node() {
        let rect = SceneRect::new(10.0, 20.0, 100.0, 50.0, Color::rgb(255, 0, 0))
            .with_stroke(Color::WHITE, 2.0)
            .with_corner_radius(8.0);
        let composition = SceneEmitterComposition::new("rect", rect);
        let scene = composition.render(0, &Value::Null, context()).unwrap();

        assert_eq!(scene.nodes.len(), 1);
        assert!(matches!(
            &scene.nodes[0],
            SceneNode::Rect { x, y, w, h, fill, stroke: Some(_), corner_radius, .. }
                if (*x - 10.0).abs() < f32::EPSILON
                    && (*y - 20.0).abs() < f32::EPSILON
                    && (*w - 100.0).abs() < f32::EPSILON
                    && (*h - 50.0).abs() < f32::EPSILON
                    && *fill == Color::rgb(255, 0, 0)
                    && (*corner_radius - 8.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn scene_rect_clamps_negative_dimensions_to_zero() {
        let rect = SceneRect::new(0.0, 0.0, -10.0, -5.0, Color::BLACK);
        let composition = SceneEmitterComposition::new("rect-clamp", rect);
        let scene = composition.render(0, &Value::Null, context()).unwrap();

        assert!(matches!(
            &scene.nodes[0],
            SceneNode::Rect { w, h, .. } if *w == 0.0 && *h == 0.0
        ));
    }

    #[test]
    fn scene_text_emits_correct_text_node() {
        let text = SceneText::new("Hello", 5.0, 30.0, 24.0, Color::WHITE).with_font_weight(700);
        let composition = SceneEmitterComposition::new("text", text);
        let scene = composition.render(0, &Value::Null, context()).unwrap();

        assert_eq!(scene.nodes.len(), 1);
        assert!(matches!(
            &scene.nodes[0],
            SceneNode::Text { x, y, content, font_size, font_weight, .. }
                if (*x - 5.0).abs() < f32::EPSILON
                    && (*y - 30.0).abs() < f32::EPSILON
                    && content == "Hello"
                    && (*font_size - 24.0).abs() < f32::EPSILON
                    && *font_weight == 700
        ));
    }

    #[test]
    fn scene_text_emits_nothing_for_empty_content() {
        let text = SceneText::new("", 0.0, 0.0, 16.0, Color::WHITE);
        let composition = SceneEmitterComposition::new("text-empty", text);
        let scene = composition.render(0, &Value::Null, context()).unwrap();
        assert!(scene.nodes.is_empty());
    }

    #[test]
    fn scene_linear_gradient_emits_correct_gradient_node() {
        use dioxuscut_rasterizer::GradientStop;
        let gradient = SceneLinearGradient::new(
            0.0,
            0.0,
            320.0,
            180.0,
            vec![
                GradientStop {
                    position: 0.0,
                    color: Color::BLACK,
                },
                GradientStop {
                    position: 1.0,
                    color: Color::WHITE,
                },
            ],
        )
        .with_angle(135.0);
        let composition = SceneEmitterComposition::new("grad", gradient);
        let scene = composition.render(0, &Value::Null, context()).unwrap();

        assert_eq!(scene.nodes.len(), 1);
        assert!(matches!(
            &scene.nodes[0],
            SceneNode::LinearGradient { w, h, angle_deg, stops, .. }
                if (*w - 320.0).abs() < f32::EPSILON
                    && (*h - 180.0).abs() < f32::EPSILON
                    && (*angle_deg - 135.0).abs() < f32::EPSILON
                    && stops.len() == 2
        ));
    }

    #[test]
    fn scene_loop_wraps_frame_at_duration() {
        let captured_frames = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let frames_clone = captured_frames.clone();
        let child = move |ctx: SceneFrameContext, _props: &Value, _scene: &mut Scene| {
            frames_clone.lock().unwrap().push(ctx.frame);
            Ok(())
        };
        let looper = SceneLoop::new(10, child);
        let ctx15 = SceneFrameContext {
            frame: 15,
            global_frame: 15,
            composition: context(),
        };
        let mut scene = Scene::new();
        looper.emit(ctx15, &Value::Null, &mut scene).unwrap();
        assert_eq!(*captured_frames.lock().unwrap(), vec![5]); // 15 % 10 = 5
    }

    #[test]
    fn scene_loop_preserves_global_frame() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let captured_clone = captured.clone();
        let child = move |ctx: SceneFrameContext, _props: &Value, _scene: &mut Scene| {
            captured_clone
                .lock()
                .unwrap()
                .push((ctx.frame, ctx.global_frame));
            Ok(())
        };
        let looper = SceneLoop::new(10, child);
        for frame in [0, 5, 9, 10, 15, 29, 99] {
            let ctx = SceneFrameContext {
                frame,
                global_frame: frame,
                composition: context(),
            };
            let mut scene = Scene::new();
            looper.emit(ctx, &Value::Null, &mut scene).unwrap();
        }
        assert_eq!(
            *captured.lock().unwrap(),
            vec![(0, 0), (5, 5), (9, 9), (0, 10), (5, 15), (9, 29), (9, 99),]
        );
    }

    #[test]
    fn scene_loop_bounded_repetitions_builder_and_total_duration() {
        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let count_clone = call_count.clone();
        let child = move |_: SceneFrameContext, _: &Value, _: &mut Scene| {
            count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        };

        let infinite_loop = SceneLoop::new(10, child.clone());
        assert_eq!(infinite_loop.total_duration(), None);

        let bounded_loop = SceneLoop::new(10, child).times(3);
        assert_eq!(bounded_loop.total_duration(), Some(30));

        // Frames 0..30 should emit
        for frame in 0..30 {
            let ctx = SceneFrameContext {
                frame,
                global_frame: frame,
                composition: context(),
            };
            let mut scene = Scene::new();
            bounded_loop.emit(ctx, &Value::Null, &mut scene).unwrap();
        }
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 30);

        // Frames >= 30 should not emit
        for frame in [30, 31, 50, 100] {
            let ctx = SceneFrameContext {
                frame,
                global_frame: frame,
                composition: context(),
            };
            let mut scene = Scene::new();
            bounded_loop.emit(ctx, &Value::Null, &mut scene).unwrap();
        }
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 30);
    }

    #[test]
    fn scene_loop_zero_duration_guard() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let captured_clone = captured.clone();
        let child = move |ctx: SceneFrameContext, _props: &Value, _scene: &mut Scene| {
            captured_clone.lock().unwrap().push(ctx.frame);
            Ok(())
        };
        let looper = SceneLoop::new(0, child);
        for frame in [0, 5, 10] {
            let ctx = SceneFrameContext {
                frame,
                global_frame: frame,
                composition: context(),
            };
            let mut scene = Scene::new();
            looper.emit(ctx, &Value::Null, &mut scene).unwrap();
        }
        // When duration is 0, duration is clamped to 1 so frame % 1 = 0
        assert_eq!(*captured.lock().unwrap(), vec![0, 0, 0]);
    }

    #[test]
    fn scene_transition_series_empty_and_single_clip() {
        let empty = SceneTransitionSeries::new();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.total_duration(), 0);
        assert_eq!(empty.duration_in_frames(), 0);
        let mut scene = Scene::new();
        empty
            .emit(
                SceneFrameContext::new(0, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        assert!(scene.nodes.is_empty());

        let rect = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
        let single = SceneTransitionSeries::new().clip(45, rect);
        assert!(!single.is_empty());
        assert_eq!(single.len(), 1);
        assert_eq!(single.total_duration(), 45);

        let mut scene = Scene::new();
        single
            .emit(
                SceneFrameContext::new(20, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        assert_eq!(scene.nodes.len(), 1);
        // Single clip without transition is emitted directly, not in a Group
        assert!(matches!(&scene.nodes[0], SceneNode::Rect { .. }));
    }

    #[test]
    fn scene_transition_series_timeline_calculation_and_clamping() {
        let rect = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
        let series = SceneTransitionSeries::new()
            .clip(60, rect.clone())
            .transition(TransitionKind::Fade, TransitionTiming::new(20))
            .clip(40, rect.clone())
            .transition(TransitionKind::SlideLeft, TransitionTiming::new(100)) // 100 > min(40, 50) -> clamped to 40
            .clip(50, rect);

        let (starts, overlaps) = series.calculate_timeline();
        assert_eq!(overlaps, vec![20, 40]);
        assert_eq!(starts, vec![0, 40, 40]);
        assert_eq!(series.total_duration(), 40 + 50); // 90
    }

    #[test]
    fn scene_transition_series_fade_crossfade() {
        let rect1 = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
        let rect2 = SceneRect::new(0.0, 0.0, 20.0, 20.0, Color::BLACK);
        let series = SceneTransitionSeries::new()
            .clip(60, rect1)
            .transition(TransitionKind::Fade, TransitionTiming::new(20))
            .clip(60, rect2);

        // Timeline:
        // Clip 1: starts at 0, duration 60 -> active [0, 60)
        // Clip 2: starts at 40 (60 - 20), duration 60 -> active [40, 100)
        // Total duration: 100

        // Frame 20: Only clip 1 active (no transition yet, local frame 20 < 40)
        let mut scene = Scene::new();
        series
            .emit(
                SceneFrameContext::new(20, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        assert_eq!(scene.nodes.len(), 1);
        assert!(
            matches!(&scene.nodes[0], SceneNode::Rect { w, .. } if (*w - 10.0).abs() < f32::EPSILON)
        );

        // Frame 50 (midpoint of overlap [40, 60)): p = (50 - 40) / 20 = 0.5
        // Clip 1: alpha = 1.0 - 0.5 = 0.5
        // Clip 2: alpha = 0.5
        let mut scene = Scene::new();
        series
            .emit(
                SceneFrameContext::new(50, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        assert_eq!(scene.nodes.len(), 2);

        if let SceneNode::Group {
            opacity, children, ..
        } = &scene.nodes[0]
        {
            assert!((*opacity - 0.5).abs() < 1e-4);
            assert!(
                matches!(&children[0], SceneNode::Rect { w, .. } if (*w - 10.0).abs() < f32::EPSILON)
            );
        } else {
            panic!("Expected SceneNode::Group for clip 1");
        }

        if let SceneNode::Group {
            opacity, children, ..
        } = &scene.nodes[1]
        {
            assert!((*opacity - 0.5).abs() < 1e-4);
            assert!(
                matches!(&children[0], SceneNode::Rect { w, .. } if (*w - 20.0).abs() < f32::EPSILON)
            );
        } else {
            panic!("Expected SceneNode::Group for clip 2");
        }

        // Frame 70: Only clip 2 active (local frame 30 >= 20, no group needed)
        let mut scene = Scene::new();
        series
            .emit(
                SceneFrameContext::new(70, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        assert_eq!(scene.nodes.len(), 1);
        assert!(
            matches!(&scene.nodes[0], SceneNode::Rect { w, .. } if (*w - 20.0).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn scene_transition_series_slide_left() {
        let rect1 = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
        let rect2 = SceneRect::new(0.0, 0.0, 20.0, 20.0, Color::BLACK);
        let series = SceneTransitionSeries::new()
            .clip(60, rect1)
            .transition(TransitionKind::SlideLeft, TransitionTiming::new(20))
            .clip(60, rect2);

        // Frame 50 (midpoint of overlap [40, 60)): p = 0.5
        // Composition width = 320
        // Outgoing (clip 1): tx = -p * width = -0.5 * 320 = -160.0
        // Incoming (clip 2): tx = (1 - p) * width = 0.5 * 320 = +160.0
        let mut scene = Scene::new();
        series
            .emit(
                SceneFrameContext::new(50, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        assert_eq!(scene.nodes.len(), 2);

        if let SceneNode::Group {
            transform, opacity, ..
        } = &scene.nodes[0]
        {
            assert!((*opacity - 1.0).abs() < 1e-4);
            assert!((transform.tx - -160.0).abs() < 1e-4);
            assert!(transform.ty.abs() < 1e-4);
        } else {
            panic!("Expected SceneNode::Group for clip 1");
        }

        if let SceneNode::Group {
            transform, opacity, ..
        } = &scene.nodes[1]
        {
            assert!((*opacity - 1.0).abs() < 1e-4);
            assert!((transform.tx - 160.0).abs() < 1e-4);
            assert!(transform.ty.abs() < 1e-4);
        } else {
            panic!("Expected SceneNode::Group for clip 2");
        }
    }

    #[test]
    fn scene_transition_series_slide_right() {
        let rect1 = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
        let rect2 = SceneRect::new(0.0, 0.0, 20.0, 20.0, Color::BLACK);
        let series = SceneTransitionSeries::new()
            .clip(60, rect1)
            .transition(TransitionKind::SlideRight, TransitionTiming::new(20))
            .clip(60, rect2);

        // Frame 50 (midpoint of overlap [40, 60)): p = 0.5
        // Composition width = 320
        // Outgoing (clip 1): tx = p * width = +160.0
        // Incoming (clip 2): tx = -(1 - p) * width = -160.0
        let mut scene = Scene::new();
        series
            .emit(
                SceneFrameContext::new(50, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        assert_eq!(scene.nodes.len(), 2);

        if let SceneNode::Group { transform, .. } = &scene.nodes[0] {
            assert!((transform.tx - 160.0).abs() < 1e-4);
            assert!(transform.ty.abs() < 1e-4);
        } else {
            panic!("Expected SceneNode::Group for clip 1");
        }

        if let SceneNode::Group { transform, .. } = &scene.nodes[1] {
            assert!((transform.tx - -160.0).abs() < 1e-4);
            assert!(transform.ty.abs() < 1e-4);
        } else {
            panic!("Expected SceneNode::Group for clip 2");
        }
    }

    #[test]
    fn scene_transition_series_slide_up() {
        let rect1 = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
        let rect2 = SceneRect::new(0.0, 0.0, 20.0, 20.0, Color::BLACK);
        let series = SceneTransitionSeries::new()
            .clip(60, rect1)
            .transition(TransitionKind::SlideUp, TransitionTiming::new(20))
            .clip(60, rect2);

        // Frame 50 (midpoint of overlap [40, 60)): p = 0.5
        // Composition height = 180
        // Outgoing (clip 1): ty = -p * height = -90.0
        // Incoming (clip 2): ty = (1 - p) * height = +90.0
        let mut scene = Scene::new();
        series
            .emit(
                SceneFrameContext::new(50, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        assert_eq!(scene.nodes.len(), 2);

        if let SceneNode::Group { transform, .. } = &scene.nodes[0] {
            assert!(transform.tx.abs() < 1e-4);
            assert!((transform.ty - -90.0).abs() < 1e-4);
        } else {
            panic!("Expected SceneNode::Group for clip 1");
        }

        if let SceneNode::Group { transform, .. } = &scene.nodes[1] {
            assert!(transform.tx.abs() < 1e-4);
            assert!((transform.ty - 90.0).abs() < 1e-4);
        } else {
            panic!("Expected SceneNode::Group for clip 2");
        }
    }

    #[test]
    fn scene_transition_series_slide_down() {
        let rect1 = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
        let rect2 = SceneRect::new(0.0, 0.0, 20.0, 20.0, Color::BLACK);
        let series = SceneTransitionSeries::new()
            .clip(60, rect1)
            .transition(TransitionKind::SlideDown, TransitionTiming::new(20))
            .clip(60, rect2);

        // Frame 50 (midpoint of overlap [40, 60)): p = 0.5
        // Composition height = 180
        // Outgoing (clip 1): ty = p * height = +90.0
        // Incoming (clip 2): ty = -(1 - p) * height = -90.0
        let mut scene = Scene::new();
        series
            .emit(
                SceneFrameContext::new(50, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        assert_eq!(scene.nodes.len(), 2);

        if let SceneNode::Group { transform, .. } = &scene.nodes[0] {
            assert!(transform.tx.abs() < 1e-4);
            assert!((transform.ty - 90.0).abs() < 1e-4);
        } else {
            panic!("Expected SceneNode::Group for clip 1");
        }

        if let SceneNode::Group { transform, .. } = &scene.nodes[1] {
            assert!(transform.tx.abs() < 1e-4);
            assert!((transform.ty - -90.0).abs() < 1e-4);
        } else {
            panic!("Expected SceneNode::Group for clip 2");
        }
    }

    #[test]
    fn scene_transition_series_chained_multi_transitions() {
        let r1 = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
        let r2 = SceneRect::new(0.0, 0.0, 20.0, 20.0, Color::BLACK);
        let r3 = SceneRect::new(0.0, 0.0, 30.0, 30.0, Color::rgb(255, 0, 0));

        let series = SceneTransitionSeries::new()
            .clip(40, r1)
            .transition(TransitionKind::SlideLeft, TransitionTiming::new(10))
            .clip(40, r2)
            .transition(TransitionKind::Fade, TransitionTiming::new(10))
            .clip(40, r3);

        // Clip 1: start = 0, duration = 40 (active [0, 40))
        // Clip 2: start = 30, duration = 40 (active [30, 70))
        // Clip 3: start = 60, duration = 40 (active [60, 100))
        // Total duration: 100
        assert_eq!(series.total_duration(), 100);

        // Transition 1 midpoint at frame 35 (overlap [30, 40), p = 0.5)
        let mut scene = Scene::new();
        series
            .emit(
                SceneFrameContext::new(35, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        assert_eq!(scene.nodes.len(), 2);
        if let SceneNode::Group { transform, .. } = &scene.nodes[0] {
            assert!((transform.tx - -160.0).abs() < 1e-4);
        } else {
            panic!("Clip 1 should be a group");
        }
        if let SceneNode::Group { transform, .. } = &scene.nodes[1] {
            assert!((transform.tx - 160.0).abs() < 1e-4);
        } else {
            panic!("Clip 2 should be a group");
        }

        // Transition 2 midpoint at frame 65 (overlap [60, 70), p = 0.5)
        let mut scene = Scene::new();
        series
            .emit(
                SceneFrameContext::new(65, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        assert_eq!(scene.nodes.len(), 2);
        if let SceneNode::Group { opacity, .. } = &scene.nodes[0] {
            assert!((*opacity - 0.5).abs() < 1e-4);
        } else {
            panic!("Clip 2 should fade out");
        }
        if let SceneNode::Group { opacity, .. } = &scene.nodes[1] {
            assert!((*opacity - 0.5).abs() < 1e-4);
        } else {
            panic!("Clip 3 should fade in");
        }
    }

    #[test]
    fn scene_transition_series_combined_in_out_for_short_clip() {
        let r1 = SceneRect::new(0.0, 0.0, 10.0, 10.0, Color::WHITE);
        let r2 = SceneRect::new(0.0, 0.0, 20.0, 20.0, Color::BLACK);
        let r3 = SceneRect::new(0.0, 0.0, 30.0, 30.0, Color::rgb(255, 0, 0));

        let series = SceneTransitionSeries::new()
            .clip(30, r1)
            .transition(TransitionKind::Fade, TransitionTiming::new(10))
            .clip(10, r2) // 10 frames: entering on [0, 10) AND exiting on [0, 10)
            .transition(TransitionKind::Fade, TransitionTiming::new(10))
            .clip(30, r3);

        // Timeline starts:
        // Clip 1: start 0, duration 30
        // Clip 2: start 20 (30 - 10), duration 10
        // Clip 3: start 20 (20 + 10 - 10), duration 30
        // Total duration: 50
        assert_eq!(series.total_duration(), 50);

        // At frame 25 (local frame 5 in Clip 2):
        // Clip 2: p_in = 5/10 = 0.5 (alpha_in = 0.5), p_out = 5/10 = 0.5 (alpha_out = 0.5)
        // Combined alpha = 0.5 * 0.5 = 0.25
        let mut scene = Scene::new();
        series
            .emit(
                SceneFrameContext::new(25, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();
        // At frame 25, all 3 clips overlap:
        // Clip 1: local frame 25 -> out_start = 20 -> p_out = 5/10 = 0.5 -> alpha = 0.5
        // Clip 2: local frame 5 -> alpha = 0.25
        // Clip 3: local frame 5 -> in_overlap = 10 -> p_in = 5/10 = 0.5 -> alpha = 0.5
        assert_eq!(scene.nodes.len(), 3);

        if let SceneNode::Group { opacity, .. } = &scene.nodes[0] {
            assert!((*opacity - 0.5).abs() < 1e-4);
        } else {
            panic!("Clip 1 expected group");
        }

        if let SceneNode::Group { opacity, .. } = &scene.nodes[1] {
            assert!((*opacity - 0.25).abs() < 1e-4);
        } else {
            panic!("Clip 2 expected group with combined opacity");
        }

        if let SceneNode::Group { opacity, .. } = &scene.nodes[2] {
            assert!((*opacity - 0.5).abs() < 1e-4);
        } else {
            panic!("Clip 3 expected group");
        }
    }

    #[test]
    fn scene_trail_emits_multiple_lagged_layers() {
        let captured_frames = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let frames_clone = captured_frames.clone();
        let child = move |ctx: SceneFrameContext, _props: &Value, scene: &mut Scene| {
            frames_clone.lock().unwrap().push(ctx.frame);
            scene.push(SceneNode::Rect {
                x: ctx.frame as f32 * 10.0,
                y: 0.0,
                w: 20.0,
                h: 20.0,
                fill: Color::WHITE,
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            });
            Ok(())
        };

        // 3 layers with lag 2 frames:
        // at frame 10:
        // layer 2 (oldest): ghost_frame = 10 - 2*2 = 6
        // layer 1: ghost_frame = 10 - 1*2 = 8
        // layer 0 (newest): ghost_frame = 10 - 0*2 = 10
        let trail = SceneTrail::new(3, 2, child);
        let mut scene = Scene::new();
        trail
            .emit(
                SceneFrameContext::new(10, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();

        assert_eq!(*captured_frames.lock().unwrap(), vec![6, 8, 10]);
        assert_eq!(scene.nodes.len(), 3);

        // First 2 ghost layers should be wrapped in SceneNode::Group with opacity < 1.0
        // The newest layer (frame 10) is rendered directly (opacity = 1.0)
        assert!(matches!(scene.nodes[0], SceneNode::Group { opacity, .. } if opacity < 1.0));
        assert!(matches!(scene.nodes[1], SceneNode::Group { opacity, .. } if opacity < 1.0));
        assert!(matches!(scene.nodes[2], SceneNode::Rect { .. }));
    }

    #[test]
    fn scene_trail_exponential_opacity() {
        let child = |ctx: SceneFrameContext, _props: &Value, scene: &mut Scene| {
            scene.push(SceneNode::Rect {
                x: ctx.frame as f32,
                y: 0.0,
                w: 10.0,
                h: 10.0,
                fill: Color::WHITE,
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            });
            Ok(())
        };

        let trail =
            SceneTrail::new(3, 1, child).with_opacity_strategy(SceneTrailOpacity::Exponential(0.5));
        let mut scene = Scene::new();
        trail
            .emit(
                SceneFrameContext::new(5, context()),
                &Value::Null,
                &mut scene,
            )
            .unwrap();

        // 3 layers: layer 2 (opacity 0.25), layer 1 (opacity 0.5), layer 0 (opacity 1.0)
        assert_eq!(scene.nodes.len(), 3);
        if let SceneNode::Group { opacity, .. } = scene.nodes[0] {
            assert!((opacity - 0.25).abs() < 1e-4);
        } else {
            panic!("Expected group for layer 2");
        }
        if let SceneNode::Group { opacity, .. } = scene.nodes[1] {
            assert!((opacity - 0.5).abs() < 1e-4);
        } else {
            panic!("Expected group for layer 1");
        }
    }
}
