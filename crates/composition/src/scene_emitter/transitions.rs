//! Scene transition series and trail emitters.

use super::traits::{SceneEmitter, SceneFrameContext};
use crate::CompositionError;
use rust_motion_rasterizer::{BlendMode, ClipRegion, MaskMode, Scene, SceneNode, Transform2D};
use serde_json::Value;

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
