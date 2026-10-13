//! # Rust Motion Hello World Example
//!
//! Demonstrates the core Rust Motion API:
//! - `<Composition>` — defines the video canvas
//! - `<Sequence>` — time-sliced segments
//! - `<AbsoluteFill>` — full-canvas overlay
//! - `use_current_frame()` — reads the frame number
//! - `interpolate()` — maps frames to animation values
//! - `spring()` — physics-based animation
//! - `<Fade>` / `<Slide>` — transitions

use dioxus::prelude::*;
use rust_motion_animation::{
    interpolate::{interpolate, ExtrapolateType, InterpolateOptions},
    spring::{spring, spring_with_options, SpringConfig, SpringOptions},
};
use rust_motion_core::{
    hooks::{use_current_frame, use_input_props, use_video_config},
    AbsoluteFill, Sequence,
};
use rust_motion_player::Player;
use rust_motion_transitions::{Fade, Slide, SlideDirection};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct ExampleProps {
    pub title: String,
    pub subtitle: String,
    pub background_start: String,
    pub background_end: String,
}

impl Default for ExampleProps {
    fn default() -> Self {
        Self {
            title: "🦀 Rust Motion".to_string(),
            subtitle: "Remotion · powered by Rust & Dioxus".to_string(),
            background_start: "#1a0533".to_string(),
            background_end: "#001a33".to_string(),
        }
    }
}

// ─── Composition dimensions ───────────────────────────────────────────────────
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
const FPS: f64 = 30.0;
const DURATION: u32 = 180; // 6 seconds

// ─── Entry point ──────────────────────────────────────────────────────────────

fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();

    dioxus::launch(App);
}

// ─── Root App ─────────────────────────────────────────────────────────────────

#[component]
fn App() -> Element {
    rsx! {
        div {
            style: "
                min-height: 100vh;
                background: #0d0d14;
                display: flex;
                flex-direction: column;
                align-items: center;
                justify-content: center;
                font-family: 'Inter', system-ui, sans-serif;
                padding: 32px;
                gap: 24px;
            ",

            h1 {
                style: "color: #e8e8f0; font-size: 1.5rem; font-weight: 600; margin: 0; letter-spacing: -0.02em;",
                "🦀 Rust Motion — Remotion in Rust"
            }

            Player {
                width: WIDTH,
                height: HEIGHT,
                fps: FPS,
                duration_in_frames: DURATION,
                controls: true,
                HelloWorldComposition {}
            }

            p {
                style: "color: rgba(255,255,255,0.35); font-size: 0.8rem; margin: 0;",
                "Built with Dioxus 0.6 · rust-motion 0.1.0"
            }
        }
    }
}

// ─── Main composition ─────────────────────────────────────────────────────────

/// The actual video content.
#[component]
fn HelloWorldComposition() -> Element {
    rsx! {
        // Scene 1: Title (frames 0–59)
        Sequence { from: 0, duration_in_frames: 60,
            Fade { enter_duration: 20, exit_duration: 15, duration_in_frames: 60,
                TitleScene {}
            }
        }

        // Scene 2: Animated logo (frames 50–119) — overlaps for crossfade
        Sequence { from: 50, duration_in_frames: 70,
            Slide { enter_duration: 25, direction: SlideDirection::FromRight,
                LogoScene {}
            }
        }

        // Scene 3: Stats (frames 110–179)
        Sequence { from: 110, duration_in_frames: 70,
            Fade { enter_duration: 20,
                StatsScene {}
            }
        }
    }
}

// ─── Scene 1: Title ───────────────────────────────────────────────────────────

#[component]
fn TitleScene() -> Element {
    let frame = use_current_frame();
    let config = use_video_config();

    // Settle the title over 24 frames, independent of its natural spring duration.
    let scale = spring_with_options(
        frame as f64,
        config.fps,
        SpringConfig {
            overshoot_clamping: true,
            ..Default::default()
        },
        SpringOptions {
            from: 0.8,
            to: 1.0,
            duration_in_frames: Some(24.0),
            ..Default::default()
        },
    )
    .expect("valid title spring");

    // Delay the subtitle by 8 frames, then spring from 30px to its resting position.
    let subtitle_y = spring_with_options(
        frame as f64,
        config.fps,
        SpringConfig::default(),
        SpringOptions {
            from: 30.0,
            to: 0.0,
            delay: 8.0,
            duration_in_frames: Some(20.0),
            ..Default::default()
        },
    )
    .expect("valid subtitle spring");

    let props = use_input_props::<ExampleProps>(ExampleProps::default);

    rsx! {
        AbsoluteFill {
            style: "background: linear-gradient(135deg, {props.background_start} 0%, #0d1b4b 50%, {props.background_end} 100%); display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 16px;",

            // Title
            div {
                style: "
                    color: #ffffff;
                    font-size: 72px;
                    font-weight: 800;
                    letter-spacing: -0.04em;
                    transform: scale({scale:.4});
                    text-align: center;
                    line-height: 1;
                ",
                "{props.title}"
            }

            // Subtitle
            div {
                style: "
                    color: rgba(200, 180, 255, 0.85);
                    font-size: 28px;
                    font-weight: 400;
                    transform: translateY({subtitle_y:.2}px);
                    text-align: center;
                ",
                "{props.subtitle}"
            }

            // Frame counter (for demo)
            div {
                style: "
                    position: absolute; bottom: 32px; right: 32px;
                    font-size: 13px; font-family: monospace;
                    color: rgba(255,255,255,0.3);
                ",
                "frame {frame}"
            }
        }
    }
}

// ─── Scene 2: Animated logo ───────────────────────────────────────────────────

#[component]
fn LogoScene() -> Element {
    let frame = use_current_frame();
    let config = use_video_config();

    // Rotate the crab emoji using interpolate
    let rotate = interpolate(
        frame as f64,
        &[0.0, config.duration_in_frames as f64],
        &[0.0, 360.0],
        Default::default(),
    );

    // Pulse scale with spring
    let pulse = spring(
        frame % 30,
        config.fps,
        SpringConfig {
            stiffness: 200.0,
            damping: 12.0,
            ..Default::default()
        },
    );
    let scale = interpolate(pulse, &[0.0, 1.0], &[0.95, 1.05], Default::default());

    rsx! {
        AbsoluteFill {
            style: "
                background: linear-gradient(135deg, #0a2a1a 0%, #001a33 100%);
                display: flex; flex-direction: column;
                align-items: center; justify-content: center;
                gap: 24px;
            ",

            div {
                style: "
                    font-size: 120px;
                    transform: rotate({rotate:.1}deg) scale({scale:.4});
                    filter: drop-shadow(0 0 40px rgba(108,99,255,0.6));
                ",
                "🦀"
            }

            div {
                style: "color: rgba(255,255,255,0.7); font-size: 24px; font-weight: 500;",
                "Physics-based animations"
            }
            div {
                style: "color: rgba(108,99,255,0.9); font-size: 16px; font-family: monospace;",
                "spring(frame, fps, SpringConfig {{ damping: 10, stiffness: 100 }})"
            }
        }
    }
}

// ─── Scene 3: Stats ───────────────────────────────────────────────────────────

#[component]
fn StatsScene() -> Element {
    let frame = use_current_frame();

    let stats = [
        ("interpolate()", "Range mapping with easing"),
        ("spring()", "Physics-based animation"),
        ("Sequence", "Timeline-sliced segments"),
        ("AbsoluteFill", "Full-canvas overlays"),
        ("Fade / Slide", "Built-in transitions"),
    ];

    rsx! {
        AbsoluteFill {
            style: "
                background: linear-gradient(135deg, #0d0d14 0%, #1a1030 100%);
                display: flex; flex-direction: column;
                align-items: center; justify-content: center;
                gap: 20px; padding: 60px;
            ",

            div {
                style: "color: #e8e8f0; font-size: 36px; font-weight: 700; margin-bottom: 12px;",
                "API at a glance"
            }

            div {
                style: "display: flex; flex-direction: column; gap: 12px; width: 100%; max-width: 640px;",

                for (i, (name, desc)) in stats.iter().enumerate() {
                    // Stagger entry using interpolate
                    {
                        let stagger = (i as f64) * 5.0;
                        let t = interpolate(
                            frame as f64,
                            &[stagger, stagger + 20.0],
                            &[0.0, 1.0],
                            InterpolateOptions {
                                extrapolate_left: ExtrapolateType::Clamp,
                                extrapolate_right: ExtrapolateType::Clamp,
                                ..Default::default()
                            },
                        );
                        rsx! {
                            div {
                                key: "{name}",
                                style: "
                                    display: flex; align-items: center; gap: 16px;
                                    background: rgba(255,255,255,0.05);
                                    border: 1px solid rgba(108,99,255,0.3);
                                    border-radius: 10px; padding: 14px 20px;
                                    opacity: {t:.4};
                                    transform: translateX({(1.0 - t) * 40.0:.2}px);
                                ",
                                code {
                                    style: "color: #6c63ff; font-size: 16px; font-weight: 600; min-width: 180px;",
                                    "{name}"
                                }
                                span {
                                    style: "color: rgba(255,255,255,0.6); font-size: 14px;",
                                    "{desc}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
