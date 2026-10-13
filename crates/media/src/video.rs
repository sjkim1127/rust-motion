//! `<Video>` component — HTML video element synchronized with the composition frame.
//!
//! Equivalent to Remotion's `<Video>`.

use dioxus::prelude::*;
use rust_motion_core::hooks::use_current_frame;
use rust_motion_core::hooks::use_video_config;

/// Props for `<Video>`.
#[derive(Props, Clone, PartialEq)]
pub struct VideoProps {
    /// Video source URL.
    pub src: String,
    /// Volume (0.0–1.0). Default: `1.0`.
    #[props(default = 1.0)]
    pub volume: f64,
    /// Playback rate multiplier. Default: `1.0`.
    #[props(default = 1.0)]
    pub playback_rate: f64,
    /// Start time offset into the source video (seconds).
    #[props(default = 0.0)]
    pub start_from: f64,
    /// Extra inline style.
    #[props(default)]
    pub style: Option<String>,
    /// Mute the video.
    #[props(default = false)]
    pub muted: bool,
    /// Repeat the source when it reaches its end.
    #[props(default = false)]
    pub looped: bool,
    /// Composition time at which this video becomes active, in seconds.
    #[props(default = 0.0)]
    pub start_at: f64,
    /// Composition time at which this video stops, in seconds.
    #[props(default)]
    pub end_at: Option<f64>,
}

/// A video element synchronized to the composition timeline.
///
/// During rendering/export, the video is seeked to the correct time.
/// During preview (Player), it plays normally.
#[component]
pub fn Video(props: VideoProps) -> Element {
    let frame = use_current_frame();
    let config = use_video_config();

    // Compute the source video timestamp for this frame
    let timeline_time = frame as f64 / config.fps;
    if timeline_time < props.start_at || props.end_at.is_some_and(|end| timeline_time >= end) {
        return rsx! {};
    }
    let time_in_seconds =
        props.start_from + (timeline_time - props.start_at).max(0.0) * props.playback_rate.max(0.0);
    let duration_attr = props
        .end_at
        .map(|end| (end - props.start_at).max(0.0).to_string())
        .unwrap_or_default();

    let base_style = "width: 100%; height: 100%; object-fit: cover;";
    let style = match &props.style {
        Some(extra) => format!("{base_style} {extra}"),
        None => base_style.to_string(),
    };

    rsx! {
        video {
            src: "{props.src}",
            style: "{style}",
            muted: props.muted,
            r#loop: props.looped,
            // `data-time` is the native Scene contract; retain Remotion's
            // metadata for browser-side consumers as well.
            "data-time": "{time_in_seconds}",
            "data-start-from": "{props.start_from}",
            "data-timeline-start": "{props.start_at}",
            "data-duration": "{duration_attr}",
            "data-volume": "{props.muted.then_some(0.0).unwrap_or(props.volume)}",
            "data-playback-rate": "{props.playback_rate}",
            // Data attributes carry rendering metadata
            "data-remotion-seek": "{time_in_seconds}",
            "data-remotion-volume": "{props.volume}",
            "data-remotion-playback-rate": "{props.playback_rate}",
        }
    }
}

/// Remotion-compatible alias for frame-accurate, off-thread video rendering.
///
/// Native export already seeks `<Video>` explicitly for every composition
/// frame, so the compatibility component intentionally shares the exact same
/// props and timeline semantics instead of maintaining a second implementation.
#[component]
pub fn OffthreadVideo(props: VideoProps) -> Element {
    Video(props)
}

/// Remotion-compatible alias for preview-oriented HTML video playback.
///
/// The shared component keeps preview and native export on one timeline
/// contract; the host chooses whether the emitted media node is played or
/// frame-seeked.
#[component]
pub fn Html5Video(props: VideoProps) -> Element {
    Video(props)
}
