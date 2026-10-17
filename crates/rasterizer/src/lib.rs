//! Rust Motion Rasterizer — Native browser-free frame rendering.
//!
//! Replaces Headless Chrome with a pure-Rust rasterizer pipeline.
//!
//! # Backends
//!
//! - [`TinySkiaBackend`]: CPU rasterizer using `tiny-skia`. Works everywhere — no GPU or browser required.
//!
//! # Example
//!
//! ```rust,no_run
//! use rust_motion_rasterizer::{TinySkiaBackend, Scene, SceneNode, Color};
//! use rust_motion_rasterizer::backend::{FrameConfig, RasterizerBackend};
//!
//! let backend = TinySkiaBackend::new();
//! let mut scene = Scene::new();
//! scene.push(SceneNode::Rect {
//!     x: 0.0, y: 0.0, w: 1920.0, h: 1080.0,
//!     fill: Color::rgb(15, 23, 42),
//!     stroke: None, stroke_width: 0.0, corner_radius: 0.0,
//! });
//!
//! let config = FrameConfig::new(1920, 1080, 0, 30.0);
//! let img = backend.render_frame(&scene, &config).unwrap();
//! img.save("frame_000001.png").unwrap();
//! ```

#![allow(unknown_lints)]
#![allow(clippy::chunks_exact_to_as_chunks)]

pub mod audio_cache;
pub mod backend;
pub mod camera3d;
pub mod emoji;
pub mod font;
pub mod frame_cache;
pub mod gif_cache;
pub mod gltf;
mod image_cache;
pub(crate) mod lottie_cache;
pub mod mesh3d;
pub mod motion_blur;
pub mod parenting;
pub mod particles;
pub mod psd;
pub mod render;
pub mod scene;
pub mod security;
pub mod shader;
pub(crate) mod text_atlas;
pub mod text_layout;
pub mod time_remap;
pub mod tiny_skia_backend;
mod video_cache;
pub mod vfx;
pub mod web;
pub mod web_backend;
#[cfg(feature = "gpu")]
pub mod wgpu_backend;

pub extern crate rust_motion_color as photocraft_color;
pub extern crate photocraft_affinity;
pub extern crate photocraft_algo;
pub use photocraft_algo as algo;
pub extern crate photocraft_plugins;
pub use photocraft_plugins as plugins;
pub use photocraft_geom as geom;
pub use photocraft_paint as paint;
pub use photocraft_tablet as tablet;
pub use photocraft_doc as doc;
pub use photocraft_vector as vector;
pub mod affinity_loader;
pub use affinity_loader::*;

pub use security::MediaSecurityPolicy;

pub use audio_cache::AudioData;
pub use camera3d::{
    planar_shadow_matrix, shade_surface, BokehShape, Camera3D, CameraType, DepthOfField,
    LightSource, Material3D,
};
pub use emoji::{is_emoji_char, is_emoji_grapheme, render_emoji, split_text_and_emojis, TextRun};
pub use gltf::{parse_glb, parse_gltf, GltfModel};
pub use mesh3d::{Mat4, Mesh3D, Quat, SkinnedVertex, Vec3};
pub use particles::{
    ConfettiEmitter, ConfettiShape, EmitterGeometry, Particle, ParticleEmitter,
};
pub use vfx::{
    apply_displacement_map, apply_optical_glow, generate_fractal_noise_pixmap, DisplacementChannel,
    DisplacementMap, DisplacementWrap, FractalNoise, FractalNoiseType, FractalType, GlowBlendMode,
    OpticalGlow,
};
#[cfg(feature = "gpu")]
pub use shader::WgpuShaderRunner;
pub use shader::{render_shader_cpu, wrap_wgsl_shader, ShaderUniforms};

pub use backend::{
    BackendCapabilities, BackendRenderStats, FrameConfig, FrameSink, RasterError, RasterizerBackend,
};
pub use font::{
    create_rounded_text_box, create_rounded_text_box_from_measurements, fill_text_box, fit_text,
    fit_text_on_n_lines, layout_text_box, measure_text_width, measure_text_width_with_font,
    FitTextOnNLinesOptions, FontCache, LayoutError, PositionedTextLine, RoundedTextBoxOptions,
    TextAlign, TextBox, TextBoxLayout, TextFitResult, TextHorizontalAlign, TextLineDimension,
    TextOverflow, TextVerticalAlign,
};
pub use frame_cache::{
    CacheMetrics, CachedFrame, FrameCacheConfig, FrameCacheKey, FrameCacheManager,
    DEFAULT_MAX_CACHE_BYTES,
};
pub use render::{
    build_pipe_ffmpeg_args, make_cancel_signal, render_all_frames, render_frame_timed,
    render_parallel, render_still_fallible, render_still_fallible_scaled, render_to_ffmpeg_pipe,
    render_to_ffmpeg_pipe_fallible, render_web_to_ffmpeg_pipe_fallible, save_frame, CancelSignal,
    EncodingProgress, HwAccel, NativeRenderConfig, PipeConfig, RenderCancellationToken,
    RenderControl, RenderDiagnostics, RenderProgress, StillImageFormat, VideoCodec,
};
pub use web::{
    WebFrameDriftReport, WebFrameRequest, WebFrameResponse, WebFrameTiming, WebTimeEvent,
    WebTimelineClip, WebWorkerMessage, WEB_WORKER_PROTOCOL_VERSION,
};
pub use web_backend::BrowserFrameBackend;

pub use gif_cache::{GifFrame, GifFrameCache, LoopBehavior};
pub use image_cache::DEFAULT_IMAGE_CACHE_BYTES;
pub use lottie_cache::{get_lottie_metadata, LottieMetadata};
pub use motion_blur::{
    accumulate_subframes, apply_directional_motion_blur, subframe_time_samples, ShutterConfig,
    TemporalFilterKernel,
};
pub use parenting::{LayerNode, LayerTransform, Matrix4, ParentingError, ParentingGraph};
pub use psd::{load_psd_as_scene, render_psd_to_image, render_psd_to_pixmap};
pub use scene::{
    AudioTrack, BlendMode, ClipRegion, Color, GradientStop, ImageFit, MaskMode, Scene, SceneFilter,
    SceneNode, SceneShadow, Transform2D, VisualizerStyle,
};
pub use text_layout::{
    LayoutLine, PositionedGlyph, TextAlignment, TextDirection, TextLayout, TextLayoutEngine,
    TextStyle,
};
pub use time_remap::{
    FrameInterpolationMode, OpticalFlowEngine, TimeRemapCurve, TimeRemapPoint,
};
pub use tiny_skia_backend::TinySkiaBackend;
pub use video_cache::{probe_video_metadata, VideoMetadata};
#[cfg(feature = "gpu")]
pub use wgpu_backend::{
    CacheTelemetry, FrameProfileSample, ProfilingSummary, StageSummary, WgpuBackend,
};
