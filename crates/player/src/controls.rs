//! Playback controls UI for the `<Player>` component.

use dioxus::prelude::*;

/// Props for the `<Controls>` bar.
#[derive(Props, Clone, PartialEq)]
pub struct ControlsProps {
    /// Current frame number.
    pub frame: u32,
    /// Total frames.
    pub duration: u32,
    /// Whether the player is currently playing.
    pub playing: bool,
    /// Called when play/pause is clicked.
    pub on_play_pause: EventHandler<MouseEvent>,
    /// Called when the scrubber is moved, with the new frame number.
    pub on_seek: EventHandler<u32>,
}

/// Playback controls — play/pause button + scrubber timeline.
#[component]
pub fn Controls(props: ControlsProps) -> Element {
    let play_icon = if props.playing { "⏸" } else { "▶" };
    let progress_pct = if props.duration > 1 {
        props.frame as f64 / (props.duration - 1) as f64 * 100.0
    } else {
        0.0
    };

    let duration = props.duration;
    let on_seek = props.on_seek;

    rsx! {
        div {
            class: "rust-motion-controls",
            style: "
                display: flex;
                align-items: center;
                gap: 10px;
                width: 100%;
                padding: 8px 12px;
                background: rgba(20, 20, 30, 0.85);
                backdrop-filter: blur(8px);
                border-radius: 8px;
                box-sizing: border-box;
            ",

            // Play / Pause button
            button {
                onclick: props.on_play_pause,
                style: "
                    background: none;
                    border: none;
                    color: white;
                    font-size: 18px;
                    cursor: pointer;
                    padding: 4px 8px;
                    border-radius: 4px;
                    transition: background 0.15s;
                ",
                onmouseenter: |e: MouseEvent| {
                    // Hover handled via CSS in production
                    let _ = e;
                },
                "{play_icon}"
            }

            // Scrubber
            div {
                style: "
                    flex: 1;
                    position: relative;
                    height: 20px;
                    display: flex;
                    align-items: center;
                    cursor: pointer;
                ",
                // Track background
                div {
                    style: "
                        position: absolute;
                        left: 0; right: 0;
                        height: 4px;
                        background: rgba(255,255,255,0.2);
                        border-radius: 2px;
                    ",
                }
                // Progress fill
                div {
                    style: "
                        position: absolute;
                        left: 0;
                        width: {progress_pct:.1}%;
                        height: 4px;
                        background: #6c63ff;
                        border-radius: 2px;
                        pointer-events: none;
                    ",
                }
                // Invisible range input for interaction
                input {
                    r#type: "range",
                    min: "0",
                    max: "{duration.saturating_sub(1)}",
                    value: "{props.frame}",
                    style: "
                        position: absolute;
                        width: 100%;
                        opacity: 0;
                        cursor: pointer;
                        height: 20px;
                        margin: 0;
                    ",
                    oninput: move |e| {
                        if let Ok(v) = e.value().parse::<u32>() {
                            on_seek.call(v);
                        }
                    },
                }
            }

            // Frame counter
            span {
                style: "color: rgba(255,255,255,0.6); font-size: 12px; font-family: monospace; min-width: 60px; text-align: right;",
                "{props.frame}/{duration.saturating_sub(1)}"
            }
        }
    }
}
