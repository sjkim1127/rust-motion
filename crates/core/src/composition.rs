//! `<Composition>` component — defines a named video composition.
//!
//! Equivalent to Remotion's `<Composition>` component. Provides a
//! `TimelineContext` and `VideoConfigContext` to all descendants.
//!
//! # Example
//! ```rust,ignore
//! use rust_motion_core::{Composition, CompositionProps};
//!
//! fn App() -> Element {
//!     rsx! {
//!         Composition {
//!             id: "MyVideo",
//!             width: 1920,
//!             height: 1080,
//!             fps: 30.0,
//!             duration_in_frames: 150,
//!             MyVideoComponent {}
//!         }
//!     }
//! }
//! ```

use crate::timeline::context::{TimelineContext, VideoConfigContext};
use crate::types::VideoConfig;
use dioxus::prelude::*;

/// Props for the `<Composition>` component.
#[derive(Props, Clone, PartialEq)]
pub struct CompositionProps {
    /// Unique ID of this composition.
    pub id: String,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Frames per second.
    pub fps: f64,
    /// Total frame count.
    pub duration_in_frames: u32,
    /// Current frame to render (driven by the player or renderer).
    #[props(default = 0)]
    pub frame: u32,
    /// Child elements (the actual video content).
    pub children: Element,
}

/// A named video composition.
///
/// Sets up the [`TimelineContext`] and [`VideoConfigContext`] so that
/// all descendant components can call `use_current_frame()` and
/// `use_video_config()`.
#[component]
pub fn Composition(props: CompositionProps) -> Element {
    let config = VideoConfig {
        id: props.id.clone(),
        width: props.width,
        height: props.height,
        fps: props.fps,
        duration_in_frames: props.duration_in_frames,
        default_codec: None,
        default_pixel_format: None,
    };

    // Clamp the current frame to valid range
    let frame = props.frame.min(props.duration_in_frames.saturating_sub(1));

    let mut timeline = use_context_provider(|| Signal::new(TimelineContext::new(frame)));
    if timeline.peek().frame != frame {
        timeline.set(TimelineContext::new(frame));
    }

    let mut video_config = use_context_provider(|| Signal::new(VideoConfigContext(config.clone())));
    if video_config.peek().0 != config {
        video_config.set(VideoConfigContext(config));
    }

    rsx! {
        div {
            style: "position: relative; width: {props.width}px; height: {props.height}px; overflow: hidden;",
            {props.children}
        }
    }
}
