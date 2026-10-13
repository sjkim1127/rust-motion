//! `use_video_config` hook — returns the `VideoConfig` for the current composition.
//!
//! Equivalent to Remotion's `useVideoConfig()`.
//!
//! # Usage
//! ```rust,ignore
//! use rust_motion_core::hooks::use_video_config;
//!
//! fn MyComponent() -> Element {
//!     let config = use_video_config();
//!     rsx! { div { "FPS: {config.fps}" } }
//! }
//! ```

use crate::timeline::context::VideoConfigContext;
use crate::types::VideoConfig;
use dioxus::prelude::*;

/// Returns the [`VideoConfig`] of the enclosing `<Composition>`.
///
/// # Panics
/// Panics in debug mode if called outside a `<Composition>`.
pub fn use_video_config() -> VideoConfig {
    let config = use_context::<Signal<VideoConfigContext>>();
    let snapshot = config.read();
    snapshot.0.clone()
}
