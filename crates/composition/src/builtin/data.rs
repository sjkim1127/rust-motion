//! Built-in data-driven templates: bar chart race and code terminal.

use super::{color_prop, string_prop};
use crate::{CompositionError, NativeComposition, NativeCompositionContext};
use rust_motion_rasterizer::{Color, Scene, SceneNode};
use serde_json::Value;

/// Built-in animated data visualization bar chart race template.
pub struct BarChartRaceComposition;

impl NativeComposition for BarChartRaceComposition {
    fn id(&self) -> &str {
        "BarChartRace"
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

        let title = string_prop(props, "title", "Programming Language Popularity");
        let chart_bg = color_prop(props, "bg", Color::rgb(13, 17, 23));

        let mut scene = Scene::new();

        // 1. Dark dashboard background
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
            fill: chart_bg,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });

        // 2. Large timeline year counter in background
        let current_year = 2020 + (t * 6.0) as u32;
        scene.push(SceneNode::Text {
            x: width * 0.65,
            y: height * 0.75,
            content: format!("{current_year}"),
            font_size: (width * 0.12).clamp(60.0, 140.0),
            color: Color::rgba(255, 255, 255, 12),
            font_weight: 800,
            font_sources: Vec::new(),
        });

        // 3. Header title
        scene.push(SceneNode::Text {
            x: width * 0.08,
            y: height * 0.12,
            content: title,
            font_size: (width * 0.035).clamp(20.0, 36.0),
            color: Color::WHITE,
            font_weight: 700,
            font_sources: Vec::new(),
        });

        // 4. Bar Chart Items
        struct BarData<'a> {
            name: &'a str,
            start_val: f32,
            end_val: f32,
            color: Color,
        }

        let items = [
            BarData {
                name: "Rust",
                start_val: 20.0,
                end_val: 98.0,
                color: Color::rgb(0, 240, 255),
            },
            BarData {
                name: "Python",
                start_val: 65.0,
                end_val: 88.0,
                color: Color::rgb(255, 214, 0),
            },
            BarData {
                name: "TypeScript",
                start_val: 50.0,
                end_val: 78.0,
                color: Color::rgb(49, 120, 198),
            },
            BarData {
                name: "Go",
                start_val: 35.0,
                end_val: 68.0,
                color: Color::rgb(0, 173, 216),
            },
        ];

        let mut active_items: Vec<(&str, f32, Color)> = items
            .iter()
            .map(|item| {
                let current_val = item.start_val + (item.end_val - item.start_val) * t;
                (item.name, current_val, item.color)
            })
            .collect();

        // Sort descending by current value
        active_items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // 5. Render Grid Lines
        let chart_left = width * 0.22;
        let max_bar_w = width * 0.60;
        let chart_top = height * 0.22;
        let bar_h = (height * 0.08).clamp(32.0, 60.0);
        let bar_gap = (height * 0.04).clamp(16.0, 30.0);

        for grid_idx in 0..=4 {
            let grid_pct = grid_idx as f32 * 0.25;
            let gx = chart_left + grid_pct * max_bar_w;
            scene.push(SceneNode::Rect {
                x: gx,
                y: chart_top - 10.0,
                w: 1.0,
                h: 4.0 * (bar_h + bar_gap),
                fill: Color::rgba(255, 255, 255, 20),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            });
        }

        // 6. Render Sorted Bars
        for (rank, (name, val, color)) in active_items.iter().enumerate() {
            let y = chart_top + rank as f32 * (bar_h + bar_gap);
            let bar_w = (val / 100.0) * max_bar_w;

            scene.push(SceneNode::Text {
                x: width * 0.08,
                y: y + bar_h * 0.65,
                content: name.to_string(),
                font_size: (bar_h * 0.45).clamp(14.0, 22.0),
                color: Color::rgb(226, 232, 240),
                font_weight: 600,
                font_sources: Vec::new(),
            });

            scene.push(SceneNode::Rect {
                x: chart_left,
                y,
                w: bar_w.max(4.0),
                h: bar_h,
                fill: *color,
                stroke: None,
                stroke_width: 0.0,
                corner_radius: bar_h * 0.2,
            });

            scene.push(SceneNode::Text {
                x: chart_left + bar_w + 12.0,
                y: y + bar_h * 0.65,
                content: format!("{:.0}k", val),
                font_size: (bar_h * 0.42).clamp(13.0, 20.0),
                color: *color,
                font_weight: 700,
                font_sources: Vec::new(),
            });
        }

        // 7. Footer Progress
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: height - 6.0,
            w: width * t,
            h: 6.0,
            fill: Color::rgb(0, 240, 255),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });

        Ok(scene)
    }
}

/// Built-in macOS-style terminal code typing and execution reel template.
pub struct CodeTerminalComposition;

impl NativeComposition for CodeTerminalComposition {
    fn id(&self) -> &str {
        "CodeTerminal"
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

        let bg_color = color_prop(props, "bg", Color::rgb(15, 17, 26));
        let term_title = string_prop(props, "terminal_title", "rust-motion — bash — 80x24");

        let mut scene = Scene::new();

        // 1. Cyber dark backdrop
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
            fill: bg_color,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });

        // 2. Terminal Window Container
        let win_w = width * 0.88;
        let win_h = height * 0.80;
        let win_x = (width - win_w) * 0.5;
        let win_y = (height - win_h) * 0.5;

        scene.push(SceneNode::Rect {
            x: win_x,
            y: win_y,
            w: win_w,
            h: win_h,
            fill: Color::rgb(30, 30, 46),
            stroke: Some(Color::rgb(69, 71, 90)),
            stroke_width: 1.5,
            corner_radius: 16.0,
        });

        // Window Title Bar
        let bar_h = 44.0;
        scene.push(SceneNode::Rect {
            x: win_x,
            y: win_y,
            w: win_w,
            h: bar_h,
            fill: Color::rgb(24, 24, 37),
            stroke: Some(Color::rgb(69, 71, 90)),
            stroke_width: 1.0,
            corner_radius: 16.0,
        });

        // 3 Traffic lights
        let dot_y = win_y + bar_h * 0.5;
        let dot_r = 6.0;
        scene.push(SceneNode::Circle {
            cx: win_x + 22.0,
            cy: dot_y,
            r: dot_r,
            fill: Color::rgb(255, 95, 86),
            stroke: None,
            stroke_width: 0.0,
        });
        scene.push(SceneNode::Circle {
            cx: win_x + 42.0,
            cy: dot_y,
            r: dot_r,
            fill: Color::rgb(255, 189, 46),
            stroke: None,
            stroke_width: 0.0,
        });
        scene.push(SceneNode::Circle {
            cx: win_x + 62.0,
            cy: dot_y,
            r: dot_r,
            fill: Color::rgb(39, 201, 63),
            stroke: None,
            stroke_width: 0.0,
        });

        // Terminal title in center of titlebar
        scene.push(SceneNode::Text {
            x: win_x + win_w * 0.35,
            y: win_y + 27.0,
            content: term_title,
            font_size: 13.0,
            color: Color::rgb(166, 173, 200),
            font_weight: 500,
            font_sources: Vec::new(),
        });

        // 3. Shell Prompt line
        let content_x = win_x + 30.0;
        let mut line_y = win_y + bar_h + 36.0;
        let font_size = (win_w * 0.024).clamp(14.0, 22.0);

        scene.push(SceneNode::Text {
            x: content_x,
            y: line_y,
            content: "➜  rust-motion git:(main) ".to_string(),
            font_size,
            color: Color::rgb(166, 227, 161),
            font_weight: 700,
            font_sources: Vec::new(),
        });

        // 4. Command typing animation
        let full_command = "cargo run --release --bin rust-motion";
        let typing_progress = (t * 2.2).min(1.0);
        let char_count = (typing_progress * full_command.len() as f32).round() as usize;
        let typed_str = &full_command[..char_count.min(full_command.len())];

        let cursor_char = if (frame / 15).is_multiple_of(2) {
            "▋"
        } else {
            " "
        };
        let cmd_display = format!("{typed_str}{cursor_char}");

        scene.push(SceneNode::Text {
            x: content_x + 230.0,
            y: line_y,
            content: cmd_display,
            font_size,
            color: Color::rgb(205, 214, 244),
            font_weight: 600,
            font_sources: Vec::new(),
        });

        // 5. Code lines
        line_y += 40.0;
        let code_lines = [
            (
                "fn render_composition() -> Scene {",
                Color::rgb(203, 166, 247),
            ),
            (
                "    let mut scene = Scene::new();",
                Color::rgb(147, 154, 183),
            ),
            (
                "    scene.push(SceneNode::Text);",
                Color::rgb(137, 180, 250),
            ),
            ("    scene.compile_pipeline()", Color::rgb(249, 226, 175)),
            ("}", Color::rgb(203, 166, 247)),
        ];

        for (code, col) in code_lines {
            scene.push(SceneNode::Text {
                x: content_x + 20.0,
                y: line_y,
                content: code.to_string(),
                font_size: font_size * 0.95,
                color: col,
                font_weight: 500,
                font_sources: Vec::new(),
            });
            line_y += 30.0;
        }

        // 6. Build Success Badge appearing in second half (t > 0.5)
        if t > 0.5 {
            let badge_w = 260.0;
            let badge_h = 36.0;
            let badge_y = win_y + win_h - 60.0;

            scene.push(SceneNode::Rect {
                x: content_x,
                y: badge_y,
                w: badge_w,
                h: badge_h,
                fill: Color::rgba(39, 201, 63, 35),
                stroke: Some(Color::rgb(39, 201, 63)),
                stroke_width: 1.0,
                corner_radius: 8.0,
            });

            scene.push(SceneNode::Text {
                x: content_x + 16.0,
                y: badge_y + 24.0,
                content: "✓ Render complete in 18ms".to_string(),
                font_size: 14.0,
                color: Color::rgb(166, 227, 161),
                font_weight: 700,
                font_sources: Vec::new(),
            });
        }

        // 7. Bottom timeline bar
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: height - 6.0,
            w: width * t,
            h: 6.0,
            fill: Color::rgb(203, 166, 247),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });

        Ok(scene)
    }
}
