//! Dioxuscut Rasterizer — Native browser-free frame rendering.
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
//! use dioxuscut_rasterizer::{TinySkiaBackend, Scene, SceneNode, Color};
//! use dioxuscut_rasterizer::backend::{FrameConfig, RasterizerBackend};
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
pub mod emoji;
pub mod font;
pub mod frame_cache;
pub mod gif_cache;
pub mod gltf;
mod image_cache;
pub(crate) mod lottie_cache;
pub mod mesh3d;
pub mod particles;
pub mod render;
pub mod scene;
pub mod security;
pub mod shader;
pub(crate) mod text_atlas;
pub mod text_layout;
pub mod tiny_skia_backend;
mod video_cache;
pub mod web;
pub mod web_backend;
#[cfg(feature = "gpu")]
pub mod wgpu_backend;

pub use security::MediaSecurityPolicy;

pub use audio_cache::AudioData;
pub use emoji::{is_emoji_char, is_emoji_grapheme, render_emoji, split_text_and_emojis, TextRun};
pub use gltf::{parse_glb, parse_gltf, GltfModel};
pub use mesh3d::{Mat4, Mesh3D, Quat, SkinnedVertex, Vec3};
pub use particles::{ConfettiEmitter, ConfettiShape, Particle};
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
pub use scene::{
    AudioTrack, BlendMode, ClipRegion, Color, GradientStop, ImageFit, MaskMode, Scene, SceneFilter,
    SceneNode, SceneShadow, Transform2D, VisualizerStyle,
};
pub use text_layout::{
    LayoutLine, PositionedGlyph, TextAlignment, TextDirection, TextLayout, TextLayoutEngine,
    TextStyle,
};
pub use tiny_skia_backend::TinySkiaBackend;
pub use video_cache::{probe_video_metadata, VideoMetadata};
#[cfg(feature = "gpu")]
pub use wgpu_backend::{
    CacheTelemetry, FrameProfileSample, ProfilingSummary, StageSummary, WgpuBackend,
};
