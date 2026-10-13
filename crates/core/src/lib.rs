//! # rust-motion-core
//!
//! Core Dioxus components and hooks for Rust Motion.
//!
//! Provides a Rust/Dioxus equivalent of `remotion/core`:
//!
//! ## Components
//! - [`Composition`] — top-level video definition
//! - [`Sequence`]    — time-sliced sub-composition
//! - [`AbsoluteFill`] — full-size absolute overlay
//! - [`Freeze`]       — pause a subtree at a specific frame
//!
//! ## Hooks
//! - [`use_current_frame`] — returns the current render frame
//! - [`use_video_config`]  — returns [`VideoConfig`] for the composition
//!
//! ## Re-exports
//! - Animation primitives from [`rust_motion_animation`]

pub mod absolute_fill;
pub mod audio_visualizer_component;
pub mod composition;
pub mod emoji_component;
pub mod freeze;
pub mod hooks;
pub mod loop_component;
pub mod lottie_component;
pub mod scene;
pub mod sequence;
pub mod series;
pub mod timeline;
pub mod types;

// ── Public re-exports ─────────────────────────────────────────────────────────
pub use absolute_fill::AbsoluteFill;
pub use audio_visualizer_component::{AudioVisualizer, AudioVisualizerProps, VisualizerStyle};
pub use composition::{Composition, CompositionProps};
pub use emoji_component::{Emoji, EmojiProps};
pub use freeze::Freeze;
pub use hooks::{use_current_frame, use_video_config};
pub use loop_component::{Loop, LoopProps};
pub use lottie_component::{Lottie, LottieProps};
pub use scene::SceneAbsoluteFill;
pub use sequence::{Sequence, SequenceLayout, SequenceProps};
pub use series::{Series, SeriesCoordinator, SeriesProps, SeriesSequence, SeriesSequenceProps};
pub use timeline::context::{TimelineContext, VideoConfigContext};
pub use types::VideoConfig;

// Re-export sibling crates
pub use rust_motion_animation as animation;
pub use rust_motion_composition as native_composition;
pub use rust_motion_rasterizer as rasterizer;

// Core animation & TouchDesigner-style CHOP primitives
pub use rust_motion_animation::{
    bezier, interpolate, interpolate_colors, interpolate_colors_range, interpolate_styles,
    make_transform, matrix, matrix3d, measure_spring, perspective, random, rotate, rotate3d,
    rotate_x, rotate_y, rotate_z, scale, scale3d, scale_x, scale_y, scale_z, skew, skew_x, skew_y,
    spring, spring_with_options, translate, translate3d, translate_x, translate_y, translate_z,
    AudioEnvelopeChop, EasingFn, ExtrapolateType, InterpolateOptions, LagChop, LfoChop, LfoWave,
    MathChop, MathOp, RandomSeed, SpringConfig, SpringError, SpringOptions, StyleMap, StyleValue,
    TransformOp,
};

// Composition primitives & emitters
pub use rust_motion_composition::{
    CompositionError, Mesh3DPrimitive, NativeComposition, NativeCompositionContext, SceneEmitter,
    SceneFrameContext, SceneLoop, SceneMesh3D, SceneSequence, SceneSeries, SceneSeriesEntry,
    SceneStack, SceneTransitionSeries,
};

// Typography, layout & rasterizer primitives
pub use rust_motion_rasterizer::{
    create_rounded_text_box, create_rounded_text_box_from_measurements, fill_text_box, fit_text,
    fit_text_on_n_lines, get_lottie_metadata, layout_text_box, make_cancel_signal,
    measure_text_width, measure_text_width_with_font, AudioTrack, BlendMode, CancelSignal,
    ClipRegion, Color, FitTextOnNLinesOptions, FontCache, FrameConfig, GifFrame, GifFrameCache,
    GradientStop, ImageFit, LayoutError, LoopBehavior, LottieMetadata, MaskMode,
    PositionedTextLine, RasterError, RasterizerBackend, RenderCancellationToken, RenderControl,
    RenderProgress, RoundedTextBoxOptions, Scene, SceneFilter, SceneNode, SceneShadow, TextAlign,
    TextBox, TextBoxLayout, TextFitResult, TextHorizontalAlign, TextLineDimension, TextOverflow,
    TextVerticalAlign, TinySkiaBackend, Transform2D,
};
