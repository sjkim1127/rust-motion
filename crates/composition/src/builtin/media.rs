//! Built-in media templates: podcast waveform and karaoke captions.

use super::{color_prop, string_prop};
use crate::{CompositionError, NativeComposition, NativeCompositionContext};
use rust_motion_rasterizer::{Color, GradientStop, Scene, SceneNode};
use serde_json::Value;

/// Built-in podcast audio visualizer template with host avatar, episode info, and dynamic waveform.
pub struct PodcastWaveformComposition;

impl NativeComposition for PodcastWaveformComposition {
    fn id(&self) -> &str {
        "PodcastWaveform"
    }

    fn render(
        &self,
        frame: u32,
        props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Scene, CompositionError> {
        let width = context.width as f32;
        let height = context.height as f32;
        let t = context.progress(frame);

        let bg_color_top = color_prop(props, "bg_top", Color::rgb(10, 15, 29));
        let bg_color_bottom = color_prop(props, "bg_bottom", Color::rgb(22, 30, 56));
        let accent = color_prop(props, "accent", Color::rgb(0, 240, 255));
        let title = string_prop(props, "title", "THE RUST VIDEO ENGINE");
        let host = string_prop(props, "host", "Episode 42 • Hosted by Sarah & Alex");
        let duration_str = string_prop(props, "duration_label", "45:00");

        let mut scene = Scene::new();

        // 1. Background gradient
        scene.push(SceneNode::LinearGradient {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
            angle_deg: 180.0,
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: bg_color_top,
                },
                GradientStop {
                    position: 1.0,
                    color: bg_color_bottom,
                },
            ],
        });

        // 2. Ambient glow orbs
        scene.push(SceneNode::Circle {
            cx: width * 0.25,
            cy: height * 0.35,
            r: width.min(height) * 0.3,
            fill: accent.with_opacity(0.08),
            stroke: None,
            stroke_width: 0.0,
        });

        // 3. Category/Episode Badge
        let badge_w = 260.0_f32.min(width * 0.4);
        let badge_h = 36.0;
        let badge_x = (width - badge_w) * 0.5;
        let badge_y = height * 0.12;
        scene.push(SceneNode::Rect {
            x: badge_x,
            y: badge_y,
            w: badge_w,
            h: badge_h,
            fill: Color::rgba(255, 255, 255, 20),
            stroke: Some(accent.with_opacity(0.4)),
            stroke_width: 1.0,
            corner_radius: 18.0,
        });
        scene.push(SceneNode::Text {
            x: badge_x + 20.0,
            y: badge_y + 24.0,
            content: "AUDIO BROADCAST".to_string(),
            font_size: 14.0,
            color: accent,
            font_weight: 700,
            font_sources: Vec::new(),
        });

        // 4. Circular Avatar with pulsating sound ring
        let avatar_cx = width * 0.5;
        let avatar_cy = height * 0.35;
        let avatar_r = (width.min(height) * 0.14).max(40.0);
        let pulse_r = avatar_r + (t * std::f32::consts::TAU * 4.0).sin().abs() * 8.0;

        scene.push(SceneNode::Circle {
            cx: avatar_cx,
            cy: avatar_cy,
            r: pulse_r + 12.0,
            fill: Color::TRANSPARENT,
            stroke: Some(accent.with_opacity(0.25)),
            stroke_width: 2.0,
        });

        scene.push(SceneNode::Circle {
            cx: avatar_cx,
            cy: avatar_cy,
            r: avatar_r,
            fill: Color::rgb(30, 41, 59),
            stroke: Some(accent),
            stroke_width: 3.0,
        });

        scene.push(SceneNode::Text {
            x: avatar_cx - 16.0,
            y: avatar_cy + 10.0,
            content: "MIC".to_string(),
            font_size: avatar_r * 0.7,
            color: Color::WHITE,
            font_weight: 400,
            font_sources: Vec::new(),
        });

        // 5. Podcast Title & Subtitle
        let title_y = height * 0.56;
        scene.push(SceneNode::Text {
            x: width * 0.1,
            y: title_y,
            content: title,
            font_size: (width * 0.04).clamp(24.0, 44.0),
            color: Color::WHITE,
            font_weight: 700,
            font_sources: Vec::new(),
        });

        scene.push(SceneNode::Text {
            x: width * 0.1,
            y: title_y + 36.0,
            content: host,
            font_size: (width * 0.022).clamp(14.0, 22.0),
            color: Color::rgb(148, 163, 184),
            font_weight: 400,
            font_sources: Vec::new(),
        });

        // 6. Dynamic Audio Waveform (28 Procedural Reactive Bars)
        let wave_y = height * 0.76;
        let bar_count = 28;
        let total_wave_w = width * 0.8;
        let bar_w = (total_wave_w / bar_count as f32) * 0.65;
        let gap = (total_wave_w / bar_count as f32) * 0.35;
        let start_x = (width - total_wave_w) * 0.5;

        for i in 0..bar_count {
            let fi = i as f32;
            let harmonic1 = (fi * 0.45 + t * 18.0).sin();
            let harmonic2 = (fi * 0.85 - t * 12.0).cos();
            let norm_height = ((harmonic1 + harmonic2) * 0.5).abs().clamp(0.08, 1.0);
            let bar_h = norm_height * (height * 0.14).max(30.0);
            let x = start_x + i as f32 * (bar_w + gap);
            let y = wave_y - bar_h * 0.5;

            let bar_color = if fi < (bar_count as f32 * t) {
                accent
            } else {
                Color::rgba(148, 163, 184, 80)
            };

            scene.push(SceneNode::Rect {
                x,
                y,
                w: bar_w,
                h: bar_h,
                fill: bar_color,
                stroke: None,
                stroke_width: 0.0,
                corner_radius: bar_w * 0.4,
            });
        }

        // 7. Timecode Telemetry
        let cur_secs = (t * 2700.0) as u32;
        let cur_time_str = format!(
            "{:02}:{:02} / {}",
            cur_secs / 60,
            cur_secs % 60,
            duration_str
        );
        scene.push(SceneNode::Text {
            x: start_x,
            y: wave_y + 45.0,
            content: cur_time_str,
            font_size: 14.0,
            color: Color::rgb(148, 163, 184),
            font_weight: 400,
            font_sources: Vec::new(),
        });

        // 8. Bottom playback progress line
        let progress_w = width * t;
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: height - 8.0,
            w: progress_w,
            h: 8.0,
            fill: accent,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });

        Ok(scene)
    }
}

/// Built-in timed kinetic karaoke subtitle template for short-form video.
pub struct KaraokeCaptionsComposition;

impl NativeComposition for KaraokeCaptionsComposition {
    fn id(&self) -> &str {
        "KaraokeCaptions"
    }

    fn render(
        &self,
        frame: u32,
        props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Scene, CompositionError> {
        let width = context.width as f32;
        let height = context.height as f32;
        let t = context.progress(frame);

        let bg_top = color_prop(props, "bg_top", Color::rgb(11, 13, 25));
        let bg_bottom = color_prop(props, "bg_bottom", Color::rgb(24, 15, 38));
        let active_color = color_prop(props, "active_color", Color::rgb(255, 230, 0));
        let inactive_color = color_prop(props, "inactive_color", Color::rgb(255, 255, 255));
        let tag = string_prop(props, "tag", "#RUST #VIDEO #NATIVE");

        let mut scene = Scene::new();

        // 1. Vertical dark backdrop
        scene.push(SceneNode::LinearGradient {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
            angle_deg: 160.0,
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: bg_top,
                },
                GradientStop {
                    position: 1.0,
                    color: bg_bottom,
                },
            ],
        });

        // 2. Ambient neon orb behind text
        scene.push(SceneNode::Circle {
            cx: width * 0.5,
            cy: height * 0.5,
            r: width * 0.35,
            fill: Color::rgba(255, 0, 128, 25),
            stroke: None,
            stroke_width: 0.0,
        });

        // 3. Category Tag Badge
        let badge_w = 280.0_f32.min(width * 0.6);
        let badge_h = 42.0;
        let badge_x = (width - badge_w) * 0.5;
        let badge_y = height * 0.15;
        scene.push(SceneNode::Rect {
            x: badge_x,
            y: badge_y,
            w: badge_w,
            h: badge_h,
            fill: Color::rgba(255, 255, 255, 25),
            stroke: Some(Color::rgba(255, 255, 255, 60)),
            stroke_width: 1.0,
            corner_radius: 21.0,
        });
        scene.push(SceneNode::Text {
            x: badge_x + 25.0,
            y: badge_y + 28.0,
            content: tag,
            font_size: 16.0,
            color: Color::rgb(0, 240, 255),
            font_weight: 700,
            font_sources: Vec::new(),
        });

        // 4. Karaoke timed sentence
        let words = [
            ("Rust Motion", 0.00, 0.20),
            ("builds", 0.20, 0.40),
            ("native", 0.40, 0.60),
            ("video", 0.60, 0.80),
            ("fast", 0.80, 1.00),
        ];

        let base_font_size = (width * 0.07).clamp(28.0, 56.0);
        let center_y = height * 0.5;

        // Draw background pill card for captions
        let card_w = width * 0.88;
        let card_h = 160.0;
        let card_x = (width - card_w) * 0.5;
        let card_y = center_y - card_h * 0.5;
        scene.push(SceneNode::Rect {
            x: card_x,
            y: card_y,
            w: card_w,
            h: card_h,
            fill: Color::rgba(0, 0, 0, 160),
            stroke: Some(Color::rgba(255, 255, 255, 30)),
            stroke_width: 1.0,
            corner_radius: 24.0,
        });

        // Lay out words horizontally
        let word_spacing = width * 0.02;
        let approx_word_w =
            (card_w - word_spacing * (words.len() as f32 + 1.0)) / words.len() as f32;

        for (idx, (text, w_start, w_end)) in words.iter().enumerate() {
            let is_active = t >= *w_start && (t < *w_end || (*w_end == 1.0 && t <= 1.0));
            let is_past = t >= *w_end;

            let font_size = if is_active {
                base_font_size * 1.15
            } else {
                base_font_size
            };

            let color = if is_active {
                active_color
            } else if is_past {
                Color::rgb(200, 200, 200)
            } else {
                inactive_color.with_opacity(0.4)
            };

            let x = card_x + word_spacing + idx as f32 * (approx_word_w + word_spacing);
            let y = center_y + (if is_active { 2.0 } else { 0.0 });

            if is_active {
                scene.push(SceneNode::Rect {
                    x: x - 8.0,
                    y: y - font_size * 0.9,
                    w: approx_word_w + 16.0,
                    h: font_size * 1.25,
                    fill: active_color.with_opacity(0.2),
                    stroke: Some(active_color),
                    stroke_width: 1.5,
                    corner_radius: 8.0,
                });
            }

            scene.push(SceneNode::Text {
                x,
                y,
                content: (*text).to_string(),
                font_size,
                color,
                font_weight: if is_active { 800 } else { 600 },
                font_sources: Vec::new(),
            });
        }

        // 5. Speaker attribution
        scene.push(SceneNode::Text {
            x: card_x + 30.0,
            y: card_y + card_h - 20.0,
            content: "SPEECH SYNCHRONIZED".to_string(),
            font_size: 12.0,
            color: Color::rgb(148, 163, 184),
            font_weight: 400,
            font_sources: Vec::new(),
        });

        // 6. Bottom animated progress bar
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: height - 12.0,
            w: width * t,
            h: 12.0,
            fill: active_color,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });

        Ok(scene)
    }
}
