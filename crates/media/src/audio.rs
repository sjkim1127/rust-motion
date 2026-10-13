//! `<Audio>` component — audio element synchronized with the composition frame.
//!
//! Equivalent to Remotion's `<Audio>`.

use dioxus::prelude::*;
use rust_motion_core::hooks::use_current_frame;
use rust_motion_core::hooks::use_video_config;

/// Props for `<Audio>`.
#[derive(Props, Clone, PartialEq)]
pub struct AudioProps {
    /// Audio source URL.
    pub src: String,
    /// Volume (0.0–1.0). Default: `1.0`.
    #[props(default = 1.0)]
    pub volume: f64,
    /// Start time offset into the audio file (seconds).
    #[props(default = 0.0)]
    pub start_from: f64,
    /// Playback rate multiplier. Default: `1.0`.
    #[props(default = 1.0)]
    pub playback_rate: f64,
    /// Mute the audio (useful during preview).
    #[props(default = false)]
    pub muted: bool,
    /// Repeat the source when it reaches its end.
    #[props(default = false)]
    pub looped: bool,
    /// Composition time at which this track becomes active, in seconds.
    #[props(default = 0.0)]
    pub start_at: f64,
    /// Composition time at which this track stops, in seconds.
    #[props(default)]
    pub end_at: Option<f64>,
}

/// An audio element synchronized to the composition timeline.
#[component]
pub fn Audio(props: AudioProps) -> Element {
    let frame = use_current_frame();
    let config = use_video_config();

    let timeline_time = frame as f64 / config.fps;
    let time_in_seconds =
        props.start_from + (timeline_time - props.start_at).max(0.0) * props.playback_rate.max(0.0);
    let duration = props.end_at.map(|end| (end - props.start_at).max(0.0));
    let duration_attr = duration.map(|value| value.to_string()).unwrap_or_default();

    rsx! {
        audio {
            src: "{props.src}",
            muted: props.muted,
            r#loop: props.looped,
            // Native VDOM emission consumes these canonical timeline fields.
            "data-start-from": "{props.start_from}",
            "data-timeline-start": "{props.start_at}",
            "data-duration": "{duration_attr}",
            "data-remotion-seek": "{time_in_seconds}",
            volume: "{props.muted.then_some(0.0).unwrap_or(props.volume)}",
            "data-volume": "{props.muted.then_some(0.0).unwrap_or(props.volume)}",
            "playback-rate": "{props.playback_rate}",
            "data-remotion-volume": "{props.volume}",
            "data-remotion-playback-rate": "{props.playback_rate}",
        }
    }
}

/// Remotion-compatible alias for preview-oriented HTML audio playback.
///
/// The shared component preserves the same trim, rate, volume, and timeline
/// semantics for native export and browser preview.
#[component]
pub fn Html5Audio(props: AudioProps) -> Element {
    Audio(props)
}
