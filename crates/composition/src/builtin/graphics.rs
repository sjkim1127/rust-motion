//! Built-in graphics showcases: shapes and filters, cyberpunk grid and complex gradients.

use crate::{CompositionError, NativeComposition, NativeCompositionContext};
use rust_motion_rasterizer::{
    BlendMode, Color, GradientStop, MaskMode, Scene, SceneFilter, SceneNode, SceneShadow,
    Transform2D,
};
use serde_json::Value;

/// Built-in composition exercising geometric shapes, SVG paths, shadows, and image filters.
pub struct ShapesAndFiltersComposition;

impl NativeComposition for ShapesAndFiltersComposition {
    fn id(&self) -> &str {
        "ShapesAndFilters"
    }

    fn render(
        &self,
        frame: u32,
        _props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Scene, CompositionError> {
        let width = context.width as f32;
        let height = context.height as f32;
        let t = context.progress(frame);

        let mut scene = Scene::new();

        // 1. Dark canvas background
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
            fill: Color::rgb(11, 15, 25),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });

        // 2. Layer with drop shadow and blur filter
        let center_x = width * 0.35;
        let center_y = height * 0.5;

        scene.push(SceneNode::Layer {
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: MaskMode::Alpha,
            filters: vec![SceneFilter::Blur {
                sigma: 2.0 + t * 4.0,
            }],
            shadow: Some(SceneShadow {
                color: Color::rgba(255, 0, 128, 160),
                blur_sigma: 16.0,
                offset_x: 0.0,
                offset_y: 4.0,
            }),
            children: vec![SceneNode::Rect {
                x: center_x - 100.0,
                y: center_y - 100.0,
                w: 200.0,
                h: 200.0,
                fill: Color::rgba(255, 0, 128, 200),
                stroke: Some(Color::rgb(255, 255, 255)),
                stroke_width: 3.0,
                corner_radius: 24.0 + (t * std::f32::consts::PI).sin() * 20.0,
            }],
        });

        // 3. SVG Star path with animated transform
        let star_path =
            "M 0 -50 L 14 -15 L 50 -15 L 21 7 L 32 43 L 0 22 L -32 43 L -21 7 L -50 -15 L -14 -15 Z"
                .to_string();
        scene.push(SceneNode::Group {
            transform: Transform2D::translate(width * 0.65, height * 0.5)
                .with_rotate(t * 180.0)
                .with_scale_uniform(1.2 + (t * std::f32::consts::PI).sin() * 0.3),
            opacity: 0.9,
            children: vec![SceneNode::Path {
                d: star_path,
                fill: Some(Color::rgb(0, 240, 255)),
                stroke: Some(Color::rgb(255, 255, 255)),
                stroke_width: 2.0,
                opacity: 1.0,
            }],
        });

        // 4. Concentric circles
        scene.push(SceneNode::Circle {
            cx: width * 0.5,
            cy: height * 0.85,
            r: 40.0 + t * 20.0,
            fill: Color::rgba(0, 255, 128, 60),
            stroke: Some(Color::rgb(0, 255, 128)),
            stroke_width: 2.5,
        });

        // 5. Fullscreen vignette filter overlay
        scene.push(SceneNode::Layer {
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: MaskMode::Alpha,
            filters: vec![SceneFilter::Vignette {
                offset: 0.85,
                darkness: 0.75,
                roundness: 0.5,
            }],
            shadow: None,
            children: vec![],
        });

        Ok(scene)
    }
}

/// Built-in composition exercising procedural perspective grid lines and chromatic aberration.
pub struct CyberpunkGridComposition;

impl NativeComposition for CyberpunkGridComposition {
    fn id(&self) -> &str {
        "CyberpunkGrid"
    }

    fn render(
        &self,
        frame: u32,
        _props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Scene, CompositionError> {
        let width = context.width as f32;
        let height = context.height as f32;
        let t = context.progress(frame);

        let mut scene = Scene::new();

        // 1. Dark grid background
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
            fill: Color::rgb(5, 8, 20),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });

        // 2. Perspective grid lines
        let mut grid_nodes = Vec::new();
        let grid_y_start = height * 0.5;
        let n_lines = 12;
        for i in 0..=n_lines {
            let frac = i as f32 / n_lines as f32;
            let y = grid_y_start + (frac * frac) * (height - grid_y_start);
            grid_nodes.push(SceneNode::Rect {
                x: 0.0,
                y,
                w: width,
                h: 1.5,
                fill: Color::rgba(0, 240, 255, (frac * 180.0) as u8),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            });
        }
        for i in -6..=6 {
            let x_top = width * 0.5 + (i as f32) * (width * 0.03);
            let x_bottom = width * 0.5 + (i as f32) * (width * 0.15) + (t * 20.0);
            let d = format!("M {x_top} {grid_y_start} L {x_bottom} {height}");
            grid_nodes.push(SceneNode::Path {
                d,
                fill: None,
                stroke: Some(Color::rgba(255, 0, 128, 150)),
                stroke_width: 1.5,
                opacity: 0.8,
            });
        }

        // 3. Chromatic aberration layer over grid
        scene.push(SceneNode::Layer {
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: MaskMode::Alpha,
            filters: vec![SceneFilter::ChromaticAberration {
                offset_x: 4.0 * (1.0 - t),
                offset_y: 2.0 * (1.0 - t),
                angle_rad: 0.0,
            }],
            shadow: None,
            children: grid_nodes,
        });

        // 4. Glowing center neon sign
        scene.push(SceneNode::Layer {
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: MaskMode::Alpha,
            filters: vec![],
            shadow: Some(SceneShadow {
                color: Color::rgb(0, 240, 255),
                blur_sigma: 24.0,
                offset_x: 0.0,
                offset_y: 0.0,
            }),
            children: vec![
                SceneNode::Rect {
                    x: width * 0.5 - 180.0,
                    y: height * 0.3 - 40.0,
                    w: 360.0,
                    h: 80.0,
                    fill: Color::rgba(10, 15, 35, 220),
                    stroke: Some(Color::rgb(0, 240, 255)),
                    stroke_width: 2.5,
                    corner_radius: 8.0,
                },
                SceneNode::Text {
                    x: width * 0.5 - 120.0,
                    y: height * 0.3 + 10.0,
                    content: "CYBERPUNK".to_string(),
                    font_size: 36.0,
                    color: Color::rgb(255, 255, 255),
                    font_weight: 700,
                    font_sources: vec![],
                },
            ],
        });

        Ok(scene)
    }
}

/// Built-in composition exercising multi-stop linear and radial gradients with opacity blending.
pub struct ComplexGradientsComposition;

impl NativeComposition for ComplexGradientsComposition {
    fn id(&self) -> &str {
        "ComplexGradients"
    }

    fn render(
        &self,
        frame: u32,
        _props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Scene, CompositionError> {
        let width = context.width as f32;
        let height = context.height as f32;
        let t = context.progress(frame);

        let mut scene = Scene::new();

        // 1. Multi-stop linear gradient background
        scene.push(SceneNode::LinearGradient {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
            angle_deg: 45.0 + t * 180.0,
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: Color::rgb(76, 29, 149),
                }, // Purple
                GradientStop {
                    position: 0.35,
                    color: Color::rgb(13, 148, 136),
                }, // Teal
                GradientStop {
                    position: 0.70,
                    color: Color::rgb(234, 88, 12),
                }, // Orange
                GradientStop {
                    position: 1.0,
                    color: Color::rgb(225, 29, 72),
                }, // Rose
            ],
        });

        // 2. Overlapping Radial Gradient 1
        scene.push(SceneNode::RadialGradient {
            cx: width * 0.3 + (t * std::f32::consts::TAU).cos() * 50.0,
            cy: height * 0.4 + (t * std::f32::consts::TAU).sin() * 40.0,
            r: width * 0.35,
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: Color::rgba(255, 255, 255, 180),
                },
                GradientStop {
                    position: 0.5,
                    color: Color::rgba(0, 240, 255, 100),
                },
                GradientStop {
                    position: 1.0,
                    color: Color::rgba(0, 0, 0, 0),
                },
            ],
        });

        // 3. Overlapping Radial Gradient 2
        scene.push(SceneNode::RadialGradient {
            cx: width * 0.7 - (t * std::f32::consts::TAU).cos() * 40.0,
            cy: height * 0.6 - (t * std::f32::consts::TAU).sin() * 50.0,
            r: width * 0.4,
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: Color::rgba(255, 230, 0, 160),
                },
                GradientStop {
                    position: 0.6,
                    color: Color::rgba(255, 0, 128, 80),
                },
                GradientStop {
                    position: 1.0,
                    color: Color::rgba(0, 0, 0, 0),
                },
            ],
        });

        // 4. Semi-transparent geometric accents
        scene.push(SceneNode::Rect {
            x: width * 0.1,
            y: height * 0.1,
            w: width * 0.8,
            h: height * 0.8,
            fill: Color::rgba(255, 255, 255, 15),
            stroke: Some(Color::rgba(255, 255, 255, 120)),
            stroke_width: 1.5,
            corner_radius: 20.0,
        });

        Ok(scene)
    }
}
