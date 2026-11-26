//! Frame rendering coordinator — sequential, parallel, and streaming pipeline modes.
//!
//! # Render Modes
//!
//! | Mode                     | I/O               | Parallelism | Best for                         |
//! |--------------------------|-------------------|-------------|----------------------------------|
//! | [`render_all_frames`]    | PNG files on disk | Sequential  | Debugging, inspection            |
//! | [`render_parallel`]      | PNG files on disk | Rayon (N cores) | Large renders with disk I/O  |
//! | [`render_to_ffmpeg_pipe`]| FFmpeg stdin      | Rayon + pipe| **Fastest** — zero PNG overhead  |
//!
//! ## Pipeline comparison
//!
//! ```text
//! Sequential PNG:   [frame 0] → PNG → [frame 1] → PNG → … → FFmpeg
//! Parallel PNG:     [frame 0]
//!                   [frame 1]  (all at once, Rayon)
//!                   [frame 2] → disk → FFmpeg
//!
//! Pipe (fastest):   bounded render window → ordered RGBA frames → FFmpeg → MP4
//!                   Zero disk I/O, zero PNG compression overhead
//! ```
//!
//! Split by concern: [`types`] holds the configuration, progress and control types, [`frames`]
//! and [`still`] the PNG/still modes, [`pipe`] and [`stream`] the FFmpeg pipe mode, [`ffmpeg_args`]
//! the encoder command line and [`audio`] the audio filter graph. Everything public is re-exported
//! here, so existing `render::Item` paths keep working.

use crate::backend::{FrameConfig, FrameSink, RasterError, RasterizerBackend};
use crate::scene::{AudioTrack, Scene};
use crate::security::MediaSecurityPolicy;
use image::RgbaImage;
use rayon::prelude::*;
use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

mod audio;
mod ffmpeg_args;
mod frames;
mod pipe;
mod still;
mod stream;
#[cfg(test)]
mod tests;
mod types;

use audio::*;
pub use ffmpeg_args::*;
pub use frames::*;
pub use pipe::*;
pub use still::*;
use stream::*;
pub use types::*;
