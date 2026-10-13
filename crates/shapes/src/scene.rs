//! Native Scene adapter for all procedural shape generators.

use crate::{
    make_arrow, make_callout, make_circle, make_ellipse, make_heart, make_pie, make_polygon,
    make_rect, make_spark, make_star, make_triangle, CalloutDirection,
};
use rust_motion_composition::{CompositionError, SceneEmitter, SceneFrameContext};
use rust_motion_rasterizer::{Color, Scene, SceneNode, Transform2D};
use serde_json::Value;

/// A procedural shape primitive shared with native preview and export.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneShape {
    pub path: String,
    pub width: f64,
    pub height: f64,
    pub x: f32,
    pub y: f32,
    pub fill: String,
    pub stroke: String,
    pub stroke_width: f32,
    pub opacity: f32,
}

impl SceneShape {
    pub fn new(path: impl Into<String>, width: f64, height: f64) -> Self {
        Self {
            path: path.into(),
            width,
            height,
            x: 0.0,
            y: 0.0,
            fill: "#ffffff".into(),
            stroke: "none".into(),
            stroke_width: 0.0,
            opacity: 1.0,
        }
    }

    pub fn arrow(length: f64, thickness: f64) -> Self {
        let (path, width, height) = make_arrow(length, thickness);
        Self::new(path, width, height)
    }

    pub fn circle(radius: f64) -> Self {
        let (path, width, height) = make_circle(radius);
        Self::new(path, width, height)
    }

    pub fn ellipse(rx: f64, ry: f64) -> Self {
        let shape = make_ellipse(rx, ry);
        Self::new(shape.path, shape.width, shape.height)
    }

    pub fn callout(
        width: f64,
        height: f64,
        pointer_length: f64,
        pointer_direction: CalloutDirection,
    ) -> Self {
        let shape = make_callout(width, height, pointer_length, pointer_direction);
        Self::new(shape.path, shape.width, shape.height)
    }

    pub fn heart(width: f64, height: f64) -> Self {
        let shape = make_heart(width, height);
        Self::new(shape.path, shape.width, shape.height)
    }

    pub fn pie(
        radius: f64,
        progress: f64,
        close_path: bool,
        counter_clockwise: bool,
        rotation: f64,
    ) -> Self {
        let shape = make_pie(radius, progress, close_path, counter_clockwise, rotation);
        Self::new(shape.path, shape.width, shape.height)
    }

    pub fn polygon(points: usize, radius: f64) -> Self {
        let (path, width, height) = make_polygon(points, radius);
        Self::new(path, width, height)
    }

    pub fn rect(width: f64, height: f64, corner_radius: f64) -> Self {
        let (path, width, height) = make_rect(width, height, corner_radius);
        Self::new(path, width, height)
    }

    pub fn spark(width: f64, height: f64, edge_roundness: f64, corner_radius: f64) -> Self {
        let shape = make_spark(width, height, edge_roundness, corner_radius);
        Self::new(shape.path, shape.width, shape.height)
    }

    pub fn star(points: usize, inner_radius: f64, outer_radius: f64) -> Self {
        let (path, width, height) = make_star(points, inner_radius, outer_radius);
        Self::new(path, width, height)
    }

    pub fn triangle(length: f64) -> Self {
        let (path, width, height) = make_triangle(length);
        Self::new(path, width, height)
    }

    pub fn at(mut self, x: f32, y: f32) -> Self {
        self.x = x;
        self.y = y;
        self
    }

    pub fn with_fill(mut self, fill: impl Into<String>) -> Self {
        self.fill = fill.into();
        self
    }

    pub fn with_stroke(mut self, stroke: impl Into<String>, width: f32) -> Self {
        self.stroke = stroke.into();
        self.stroke_width = width.max(0.0);
        self
    }

    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }
}

impl SceneEmitter for SceneShape {
    fn emit(
        &self,
        context: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        if self.path.trim().is_empty() {
            return Ok(());
        }
        let node = SceneNode::Path {
            d: self.path.clone(),
            fill: parse_optional_color(&self.fill, context)?,
            stroke: parse_optional_color(&self.stroke, context)?,
            stroke_width: self.stroke_width,
            opacity: self.opacity,
        };
        if self.x == 0.0 && self.y == 0.0 {
            scene.push(node);
        } else {
            scene.push(SceneNode::Group {
                transform: Transform2D {
                    tx: self.x,
                    ty: self.y,
                    ..Default::default()
                },
                opacity: 1.0,
                children: vec![node],
            });
        }
        Ok(())
    }
}

fn parse_optional_color(
    value: &str,
    context: SceneFrameContext,
) -> Result<Option<Color>, CompositionError> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") || value.eq_ignore_ascii_case("transparent") {
        return Ok(None);
    }
    Color::from_css(value).map(Some).ok_or_else(|| {
        CompositionError::render(
            context.global_frame,
            format!("unsupported native shape color '{value}'"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_motion_composition::{
        NativeComposition, NativeCompositionContext, SceneEmitterComposition,
    };
    use rust_motion_rasterizer::{FrameConfig, RasterizerBackend, TinySkiaBackend};

    fn context() -> NativeCompositionContext {
        NativeCompositionContext {
            width: 120,
            height: 120,
            fps: 30.0,
            duration_in_frames: 1,
        }
    }

    #[test]
    fn every_shape_generator_emits_a_native_path() {
        let shapes = [
            SceneShape::arrow(80.0, 10.0),
            SceneShape::callout(60.0, 40.0, 15.0, CalloutDirection::Down),
            SceneShape::circle(30.0),
            SceneShape::heart(60.0, 60.0),
            SceneShape::pie(30.0, 0.25, true, false, 0.0),
            SceneShape::polygon(6, 30.0),
            SceneShape::rect(60.0, 40.0, 8.0),
            SceneShape::spark(50.0, 50.0, 0.5, 2.0),
            SceneShape::star(5, 12.0, 30.0),
            SceneShape::triangle(60.0),
        ];
        for shape in shapes {
            let composition = SceneEmitterComposition::new("shape", shape);
            let scene = composition.render(0, &Value::Null, context()).unwrap();
            assert!(matches!(scene.nodes[0], SceneNode::Path { .. }));
        }
    }

    #[test]
    fn rounded_shape_arc_renders_after_translation() {
        let shape = SceneShape::rect(60.0, 40.0, 10.0)
            .at(20.0, 30.0)
            .with_fill("rgb(255, 0, 0)");
        let composition = SceneEmitterComposition::new("shape", shape);
        let scene = composition.render(0, &Value::Null, context()).unwrap();
        let image = TinySkiaBackend::headless()
            .render_frame(&scene, &FrameConfig::new(120, 120, 0, 30.0))
            .unwrap();

        assert!(image.get_pixel(50, 50)[0] > 240);
        assert_eq!(image.get_pixel(20, 30)[3], 0);
    }

    #[test]
    fn heart_shape_renders_native_pixels() {
        let shape = SceneShape::heart(80.0, 80.0)
            .at(20.0, 20.0)
            .with_fill("#ff0000");
        let composition = SceneEmitterComposition::new("heart_shape", shape);
        let scene = composition.render(0, &Value::Null, context()).unwrap();
        let image = TinySkiaBackend::headless()
            .render_frame(&scene, &FrameConfig::new(120, 120, 0, 30.0))
            .unwrap();

        // Center of heart at (60, 60) should be filled with red
        assert!(image.get_pixel(60, 60)[0] > 200);
    }

    #[test]
    fn callout_shape_renders_native_pixels() {
        let shape = SceneShape::callout(80.0, 50.0, 20.0, CalloutDirection::Down)
            .at(10.0, 10.0)
            .with_fill("#00ff00");
        let composition = SceneEmitterComposition::new("callout_shape", shape);
        let scene = composition.render(0, &Value::Null, context()).unwrap();
        let image = TinySkiaBackend::headless()
            .render_frame(&scene, &FrameConfig::new(120, 120, 0, 30.0))
            .unwrap();

        // Inside callout body
        assert!(image.get_pixel(50, 35)[1] > 200);
    }

    #[test]
    fn spark_shape_renders_native_pixels() {
        let shape = SceneShape::spark(80.0, 80.0, 0.5, 0.0)
            .at(20.0, 20.0)
            .with_fill("#0000ff");
        let composition = SceneEmitterComposition::new("spark_shape", shape);
        let scene = composition.render(0, &Value::Null, context()).unwrap();
        let image = TinySkiaBackend::headless()
            .render_frame(&scene, &FrameConfig::new(120, 120, 0, 30.0))
            .unwrap();

        // Center of spark at (60, 60) should be blue
        assert!(image.get_pixel(60, 60)[2] > 200);
    }

    #[test]
    fn pie_shape_renders_native_pixels() {
        let shape = SceneShape::pie(40.0, 0.5, true, false, 0.0)
            .at(20.0, 20.0)
            .with_fill("#ffff00");
        let composition = SceneEmitterComposition::new("pie_shape", shape);
        let scene = composition.render(0, &Value::Null, context()).unwrap();
        let image = TinySkiaBackend::headless()
            .render_frame(&scene, &FrameConfig::new(120, 120, 0, 30.0))
            .unwrap();

        // Right half of pie slice at (80, 60) should be yellow (R > 200, G > 200)
        assert!(image.get_pixel(80, 60)[0] > 200);
        assert!(image.get_pixel(80, 60)[1] > 200);
    }

    #[test]
    fn invalid_css_colors_fail_with_the_global_frame() {
        let composition = SceneEmitterComposition::new(
            "shape",
            SceneShape::circle(20.0).with_fill("hsl(10 20% 30%)"),
        );
        let error = composition.render(0, &Value::Null, context()).unwrap_err();
        assert!(error.to_string().contains("unsupported native shape color"));
    }
}
