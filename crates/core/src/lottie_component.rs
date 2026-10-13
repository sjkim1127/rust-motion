//! `<Lottie>` component for headless and web Lottie animation playback.
//!
//! Matches Remotion's `@remotion/lottie` `<Lottie>` component.

use crate::hooks::{use_current_frame, use_video_config};
use dioxus::prelude::*;
use rust_motion_rasterizer::LoopBehavior;

/// Props for the `<Lottie>` component.
#[derive(Props, Clone, PartialEq)]
pub struct LottieProps {
    /// File path or asset URI to the Lottie JSON file.
    pub src: String,
    /// Rendered width in pixels.
    #[props(default = 200.0)]
    pub width: f64,
    /// Rendered height in pixels.
    #[props(default = 200.0)]
    pub height: f64,
    /// Playback rate multiplier (1.0 = normal speed).
    #[props(default = 1.0)]
    pub playback_rate: f64,
    /// Animation looping behavior.
    #[props(default = LoopBehavior::Loop)]
    pub loop_behavior: LoopBehavior,
    /// Additional CSS styles.
    #[props(default)]
    pub style: String,
    /// Additional CSS classes.
    #[props(default)]
    pub class: String,
}

/// Renders a Lottie vector animation on the timeline.
#[component]
pub fn Lottie(props: LottieProps) -> Element {
    let frame = use_current_frame();
    let config = use_video_config();
    let time_secs = frame as f64 / config.fps * props.playback_rate;
    let combined_style = format!(
        "width: {}px; height: {}px; display: inline-block; overflow: hidden; {}",
        props.width, props.height, props.style
    );

    rsx! {
        div {
            class: "{props.class}",
            style: "{combined_style}",
            "data-rust-motion-lottie": "{props.src}",
            "data-frame": "{frame}",
            "data-time": "{time_secs:.4}",
            "data-playback-rate": "{props.playback_rate}",
            "data-loop": "{props.loop_behavior:?}",
        }
    }
}
