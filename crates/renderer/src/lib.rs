//! # rust-motion-renderer
//!
//! Video encoding and web server management for Rust Motion.
//!
//! Equivalent to `@remotion/renderer`.
//!
//! ## Architecture
//!
//! 1. [`EncodeConfig`] — defines video output settings (resolution, FPS, codec, CRF)
//! 2. [`encode_frames`] — stitches frame PNGs into an MP4 video file via FFmpeg
//! 3. [`spawn_server`] — embedded web server for web assets and compositions

pub mod encode;
pub mod render_frames;
pub mod server;

pub use encode::{build_ffmpeg_args, cleanup_frames, encode_frames, encode_mp4, EncodeConfig};
pub use render_frames::{RenderConfig, RenderError};
pub use server::{
    spawn_server, spawn_server_with_config, ServeMode, ServerConfig, ServerError, ServerHandle,
};
