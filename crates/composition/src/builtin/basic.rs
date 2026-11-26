//! Basic built-in samples: HelloWorld, named time events and spring rectangles.

use super::{color_prop, string_prop};
use crate::{CompositionError, NativeComposition, NativeCompositionContext, TimeEventSchedule};
use rust_motion_animation::{spring_with_options, SpringConfig, SpringOptions};
use rust_motion_rasterizer::{Color, GradientStop, Scene, SceneNode};
use serde_json::Value;

/// Built-in native composition used by the quickstart and acceptance tests.
pub struct HelloWorldComposition;

/// Small native sample showing how named event positions drive frame output.
pub struct NamedTimeEventsDemo;

impl NativeComposition for NamedTimeEventsDemo {
    fn id(&self) -> &str {
        "NamedTimeEventsDemo"
    }

    fn render(
        &self,
        frame: u32,
        props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Scene, CompositionError> {
        self.render_with_time_events(frame, props, context, &TimeEventSchedule::default())
    }

    fn render_with_time_events(
        &self,
        frame: u32,
        _props: &Value,
        context: NativeCompositionContext,
        events: &TimeEventSchedule,
    ) -> Result<Scene, CompositionError> {
        let start = events
            .wait_until("voice_start")
            .map_err(|error| CompositionError::render(frame, error.to_string()))?;
        let duration = events
            .duration_between("voice_start", "voice_end")
            .map_err(|error| CompositionError::render(frame, error.to_string()))?;
        let progress =
            (frame.saturating_sub(start) as f32 / duration.max(1) as f32).clamp(0.0, 1.0);
        let width = context.width as f32;
        let height = context.height as f32;
        let padding = width * 0.1;
        let bar_width = width - padding * 2.0;
        let bar_height = (height * 0.08).max(12.0);
        let y = (height - bar_height) / 2.0;
        let mut scene = Scene::new();
        scene.push(SceneNode::Rect {
            x: padding,
            y,
            w: bar_width,
            h: bar_height,
            fill: Color::rgb(30, 41, 59),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: bar_height / 2.0,
        });
        scene.push(SceneNode::Rect {
            x: padding,
            y,
            w: bar_width * progress,
            h: bar_height,
            fill: Color::rgb(249, 115, 62),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: bar_height / 2.0,
        });
        Ok(scene)
    }
}

/// Deterministic 2D parity fixture shared with the browser `SpringRects`
/// composition. It deliberately uses only opaque rectangles so CPU, WGPU,
/// and Chromium can be compared without font or antialiasing differences.
pub struct SpringRectsComposition;

impl NativeComposition for SpringRectsComposition {
    fn id(&self) -> &str {
        "SpringRects"
    }

    fn render(
        &self,
        frame: u32,
        _props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Scene, CompositionError> {
        let scale_x = context.width as f32 / 1280.0;
        let scale_y = context.height as f32 / 720.0;
        let mut scene = Scene::new();
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: context.width as f32,
            h: context.height as f32,
            fill: Color::rgb(15, 23, 42),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });
        for index in 0..32_u32 {
            let progress = spring_with_options(
                f64::from(frame % 60),
                context.fps,
                SpringConfig::default(),
                SpringOptions {
                    duration_in_frames: Some(24.0),
                    delay: f64::from(index % 8) * 2.0,
                    ..Default::default()
                },
            )
            .map_err(|error| CompositionError::render(frame, error.to_string()))?;
            let x =
                (60.0 + f64::from(index % 8) * 145.0 + progress * 40.0).round() as f32 * scale_x;
            let y = (80.0 + (index / 8) as f32 * 140.0) * scale_y;
            scene.push(SceneNode::Rect {
                x,
                y,
                w: 64.0 * scale_x,
                h: 64.0 * scale_y,
                fill: Color::rgb(80 + (index * 4) as u8, 160, 220),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            });
        }
        Ok(scene)
    }
}

impl NativeComposition for HelloWorldComposition {
    fn id(&self) -> &str {
        "HelloWorld"
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

        let bg_start = color_prop(props, "background_start", Color::rgb(15, 23, 42));
        let bg_end = color_prop(props, "background_end", Color::rgb(30, 27, 75));
        let accent = color_prop(props, "accent_color", Color::rgb(108, 99, 255));
        let title = string_prop(props, "title", "Hello Rust Motion");
        let subtitle = string_prop(props, "subtitle", "Declarative programmatic video in Rust");

        let mut scene = Scene::new();
        scene.push(SceneNode::LinearGradient {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
            angle_deg: 135.0 + t * 90.0,
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: bg_start,
                },
                GradientStop {
                    position: 1.0,
                    color: bg_end,
                },
            ],
        });

        let center_x = width * 0.5;
        let center_y = height * 0.5;
        let shortest_side = width.min(height);
        let r1 = shortest_side * 0.2 + (t * std::f32::consts::TAU).sin() * 20.0;
        scene.push(SceneNode::Circle {
            cx: center_x,
            cy: center_y,
            r: r1,
            fill: accent.with_opacity(0.12),
            stroke: Some(accent),
            stroke_width: 2.0,
        });

        let r2 = shortest_side * 0.3 + (t * std::f32::consts::PI).cos() * 30.0;
        scene.push(SceneNode::Circle {
            cx: center_x,
            cy: center_y,
            r: r2,
            fill: Color::TRANSPARENT,
            stroke: Some(Color::rgba(0, 242, 254, 180)),
            stroke_width: 1.5,
        });

        let rect_size = 80.0 + (t * std::f32::consts::TAU).sin() * 15.0;
        scene.push(SceneNode::Rect {
            x: width * 0.15,
            y: height * 0.2,
            w: rect_size,
            h: rect_size,
            fill: Color::rgba(0, 242, 254, 40),
            stroke: Some(Color::rgb(0, 242, 254)),
            stroke_width: 2.0,
            corner_radius: 12.0,
        });
        scene.push(SceneNode::Rect {
            x: width * 0.78,
            y: height * 0.65,
            w: rect_size * 1.2,
            h: rect_size * 1.2,
            fill: Color::rgba(255, 230, 0, 30),
            stroke: Some(Color::rgb(255, 230, 0)),
            stroke_width: 2.0,
            corner_radius: 16.0,
        });

        scene.push(SceneNode::Rect {
            x: 0.0,
            y: height - 6.0,
            w: width * t,
            h: 6.0,
            fill: Color::rgb(0, 242, 254),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });

        let font_size = (width * 0.045).max(28.0);
        let text_x = width * 0.12;
        let text_y = height * 0.45;
        scene.push(SceneNode::Text {
            x: text_x,
            y: text_y,
            content: title,
            font_size,
            color: Color::WHITE,
            font_weight: 700,
            font_sources: Vec::new(),
        });
        scene.push(SceneNode::Text {
            x: text_x,
            y: text_y + font_size * 0.8,
            content: subtitle,
            font_size: font_size * 0.45,
            color: Color::rgb(0, 242, 254),
            font_weight: 400,
            font_sources: Vec::new(),
        });
        Ok(scene)
    }
}
