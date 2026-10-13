//! `<Gif>` Dioxus component — Remotion `@remotion/gif` parity.
//!
//! Renders an animated GIF synced to the composition timeline.

use crate::img::ImageFit;
use dioxus::prelude::*;
use rust_motion_core::hooks::{use_current_frame, use_video_config};
use rust_motion_rasterizer::gif_cache::LoopBehavior;

/// Props for the [`Gif`] component.
#[derive(Props, Clone, PartialEq)]
pub struct GifProps {
    /// Path or URL of the GIF file.
    pub src: String,
    /// Rendered width in pixels.
    pub width: f32,
    /// Rendered height in pixels.
    pub height: f32,
    /// Playback speed multiplier (default `1.0`).
    #[props(default = 1.0)]
    pub playback_rate: f32,
    /// What to do when the animation ends (default: loop).
    #[props(default = LoopBehavior::Loop)]
    pub loop_behavior: LoopBehavior,
    /// How the animated image fits within its box (default: cover).
    #[props(default)]
    pub fit: ImageFit,
    /// Horizontal position in pixels (default `0.0`).
    #[props(default = 0.0)]
    pub x: f32,
    /// Vertical position in pixels (default `0.0`).
    #[props(default = 0.0)]
    pub y: f32,
}

/// Animated GIF component synced to the Rust Motion composition timeline.
///
/// In web/preview mode the browser's native GIF rendering is used; in native
/// headless rendering the rasterizer selects the correct frame via
/// [`SceneNode::Gif`].
///
/// # Example
/// ```rust,ignore
/// rsx! {
///     Gif {
///         src: "assets/confetti.gif".into(),
///         width: 400.0,
///         height: 300.0,
///         playback_rate: 1.0,
///         loop_behavior: LoopBehavior::Loop,
///     }
/// }
/// ```
#[component]
pub fn Gif(props: GifProps) -> Element {
    let frame = use_current_frame();
    let config = use_video_config();
    let time_secs = frame as f64 / config.fps * props.playback_rate as f64;

    rsx! {
        img {
            src: "{props.src}",
            style: "position: absolute; left: {props.x}px; top: {props.y}px; width: {props.width}px; height: {props.height}px; object-fit: {props.fit.as_css()};",
            "data-frame": "{frame}",
            "data-time": "{time_secs:.4}",
            "data-loop": "{props.loop_behavior:?}",
        }
    }
}
