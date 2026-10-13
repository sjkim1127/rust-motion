//! `<Player>` component — interactive video player with playback controls.
//!
//! Equivalent to `@remotion/player`'s `<Player>`.
//!
//! # Example
//! ```rust,ignore
//! use rust_motion_player::Player;
//!
//! fn App() -> Element {
//!     rsx! {
//!         Player {
//!             component: MyComposition,
//!             width: 1920,
//!             height: 1080,
//!             fps: 30.0,
//!             duration_in_frames: 150,
//!             controls: true,
//!         }
//!     }
//! }
//! ```

use dioxus::prelude::*;
use rust_motion_core::Composition;
use std::time::{Duration, Instant};

use crate::controls::Controls;

/// Props for the `<Player>` component.
#[derive(Props, Clone, PartialEq)]
pub struct PlayerProps {
    /// Composition width.
    pub width: u32,
    /// Composition height.
    pub height: u32,
    /// Frames per second.
    pub fps: f64,
    /// Total frame count.
    pub duration_in_frames: u32,
    /// Show playback controls below the canvas.
    #[props(default = true)]
    pub controls: bool,
    /// Initial frame to start at.
    #[props(default = 0)]
    pub initial_frame: u32,
    /// Whether the player should loop.
    #[props(default = true)]
    pub loop_playback: bool,
    /// The composition content to render.
    pub children: Element,
}

/// State tracked internally by the player.
#[derive(Clone, Debug, PartialEq)]
struct PlayerState {
    frame: u32,
    playing: bool,
    duration: u32,
    playback_started_at: Option<Instant>,
    playback_started_frame: u32,
    playback_cycle: u64,
    seek_revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlayerPlaybackState {
    pub playing: bool,
    pub seek_revision: u64,
}

/// Interactive video player component.
///
/// Wraps a `<Composition>` and adds play/pause/seek controls driven by
/// Dioxus reactive state.
#[component]
pub fn Player(props: PlayerProps) -> Element {
    let duration = props.duration_in_frames;
    let fps = props.fps;
    let loop_playback = props.loop_playback;
    let tick_duration = frame_duration(fps);

    let initial_frame = props.initial_frame.min(duration.saturating_sub(1));
    let mut state = use_signal(|| PlayerState {
        frame: initial_frame,
        playing: false,
        duration,
        playback_started_at: None,
        playback_started_frame: initial_frame,
        playback_cycle: 0,
        seek_revision: 0,
    });

    // Advance frame on each animation tick when playing
    use_future(move || async move {
        loop {
            tokio::time::sleep(tick_duration).await;
            let s = state.read();
            let Some(started_at) = s.playback_started_at.filter(|_| s.playing) else {
                continue;
            };
            let (next_frame, keep_playing, cycle) = frame_at_elapsed(
                s.playback_started_frame,
                started_at.elapsed(),
                fps,
                s.duration,
                loop_playback,
            );
            let cycle_changed = cycle != s.playback_cycle;
            let changed = next_frame != s.frame || keep_playing != s.playing || cycle_changed;
            drop(s);
            if changed {
                let mut next_state = state.write();
                next_state.frame = next_frame;
                next_state.playing = keep_playing;
                if cycle_changed {
                    next_state.playback_cycle = cycle;
                    next_state.seek_revision = next_state.seek_revision.wrapping_add(1);
                }
                if !keep_playing {
                    next_state.playback_started_at = None;
                }
            }
        }
    });

    let current_frame = state.read().frame;
    let is_playing = state.read().playing;
    let seek_revision = state.read().seek_revision;

    let mut playback_context = use_context_provider(|| {
        Signal::new(PlayerPlaybackState {
            playing: is_playing,
            seek_revision,
        })
    });
    let playback_snapshot = PlayerPlaybackState {
        playing: is_playing,
        seek_revision,
    };
    if *playback_context.peek() != playback_snapshot {
        playback_context.set(playback_snapshot);
    }

    let on_play_pause = move |_| {
        let mut next_state = state.write();
        next_state.playing = !next_state.playing;
        next_state.seek_revision = next_state.seek_revision.wrapping_add(1);
        next_state.playback_cycle = 0;
        if next_state.playing {
            next_state.playback_started_frame = next_state.frame;
            next_state.playback_started_at = Some(Instant::now());
        } else {
            next_state.playback_started_at = None;
        }
    };

    let on_seek = move |f: u32| {
        let mut next_state = state.write();
        next_state.frame = f.min(duration.saturating_sub(1));
        next_state.playback_started_frame = next_state.frame;
        next_state.playback_started_at = next_state.playing.then(Instant::now);
        next_state.playback_cycle = 0;
        next_state.seek_revision = next_state.seek_revision.wrapping_add(1);
    };

    rsx! {
        div {
            class: "rust-motion-player",
            style: "display: flex; flex-direction: column; align-items: center; gap: 8px; font-family: sans-serif;",

            // Composition viewport
            div {
                style: "position: relative; width: {props.width}px; height: {props.height}px; overflow: hidden; border-radius: 8px; box-shadow: 0 4px 24px rgba(0,0,0,0.4);",

                Composition {
                    id: "player-composition",
                    width: props.width,
                    height: props.height,
                    fps,
                    duration_in_frames: duration,
                    frame: current_frame,
                    {props.children}
                }

                // Frame counter overlay
                div {
                    style: "position: absolute; top: 8px; right: 10px; color: rgba(255,255,255,0.7); font-size: 11px; font-family: monospace; background: rgba(0,0,0,0.4); padding: 2px 6px; border-radius: 4px; pointer-events: none;",
                    "{current_frame} / {duration.saturating_sub(1)}"
                }
            }

            // Controls
            if props.controls {
                Controls {
                    frame: current_frame,
                    duration,
                    playing: is_playing,
                    on_play_pause,
                    on_seek,
                }
            }
        }
    }
}

fn frame_duration(fps: f64) -> tokio::time::Duration {
    let safe_fps = if fps.is_finite() && fps > 0.0 {
        fps
    } else {
        30.0
    };
    tokio::time::Duration::from_secs_f64(1.0 / safe_fps).max(tokio::time::Duration::from_millis(1))
}

fn frame_at_elapsed(
    start_frame: u32,
    elapsed: Duration,
    fps: f64,
    duration: u32,
    loop_playback: bool,
) -> (u32, bool, u64) {
    if duration == 0 {
        return (0, false, 0);
    }
    let safe_fps = if fps.is_finite() && fps > 0.0 {
        fps
    } else {
        30.0
    };
    let elapsed_frames = (elapsed.as_secs_f64() * safe_fps)
        .floor()
        .clamp(0.0, u64::MAX as f64) as u64;
    let absolute_frame = u64::from(start_frame).saturating_add(elapsed_frames);
    if absolute_frame < u64::from(duration) {
        return (absolute_frame as u32, true, 0);
    }
    if loop_playback {
        (
            (absolute_frame % u64::from(duration)) as u32,
            true,
            absolute_frame / u64::from(duration),
        )
    } else {
        (duration - 1, false, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_duration_respects_fps() {
        assert_eq!(frame_duration(25.0), tokio::time::Duration::from_millis(40));
        assert_eq!(
            frame_duration(0.0),
            tokio::time::Duration::from_secs_f64(1.0 / 30.0)
        );
        assert_eq!(frame_duration(1.0e12), Duration::from_millis(1));
    }

    #[test]
    fn playback_stops_on_the_last_frame_when_looping_is_disabled() {
        assert_eq!(
            frame_at_elapsed(8, Duration::from_millis(100), 10.0, 10, false),
            (9, true, 0)
        );
        assert_eq!(
            frame_at_elapsed(8, Duration::from_millis(200), 10.0, 10, false),
            (9, false, 0)
        );
    }

    #[test]
    fn playback_wraps_when_looping_is_enabled() {
        assert_eq!(
            frame_at_elapsed(9, Duration::from_millis(100), 10.0, 10, true),
            (0, true, 1)
        );
    }

    #[test]
    fn wall_clock_frame_selection_skips_late_ticks() {
        assert_eq!(
            frame_at_elapsed(5, Duration::from_millis(220), 30.0, 100, false),
            (11, true, 0)
        );
    }

    #[test]
    fn wall_clock_frame_selection_reports_loop_cycles() {
        assert_eq!(
            frame_at_elapsed(8, Duration::from_millis(300), 10.0, 10, true),
            (1, true, 1)
        );
    }
}
