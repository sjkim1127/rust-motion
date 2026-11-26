//! Scene builder methods for rectangles, circles, gradients, paths and groups.

use super::*;

impl SceneBuilder {
    pub(super) fn rect(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        w: FLOAT,
        h: FLOAT,
        fill: &str,
    ) -> RhaiResult<()> {
        self.scene.push(SceneNode::Rect {
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            w: non_negative_f32("width", w)?,
            h: non_negative_f32("height", h)?,
            fill: parse_color(fill)?,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });
        Ok(())
    }

    pub(super) fn round_rect(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        w: FLOAT,
        h: FLOAT,
        fill: &str,
        radius: FLOAT,
    ) -> RhaiResult<()> {
        self.scene.push(SceneNode::Rect {
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            w: non_negative_f32("width", w)?,
            h: non_negative_f32("height", h)?,
            fill: parse_color(fill)?,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: non_negative_f32("corner radius", radius)?,
        });
        Ok(())
    }

    pub(super) fn circle(
        &mut self,
        cx: FLOAT,
        cy: FLOAT,
        radius: FLOAT,
        fill: &str,
    ) -> RhaiResult<()> {
        self.scene.push(SceneNode::Circle {
            cx: finite_f32("center x", cx)?,
            cy: finite_f32("center y", cy)?,
            r: non_negative_f32("radius", radius)?,
            fill: parse_color(fill)?,
            stroke: None,
            stroke_width: 0.0,
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn rect_stroke(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        w: FLOAT,
        h: FLOAT,
        fill: &str,
        stroke: &str,
        stroke_width: FLOAT,
        radius: FLOAT,
    ) -> RhaiResult<()> {
        let stroke_color = if stroke.trim().is_empty() {
            None
        } else {
            Some(parse_color(stroke)?)
        };
        self.scene.push(SceneNode::Rect {
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            w: non_negative_f32("width", w)?,
            h: non_negative_f32("height", h)?,
            fill: parse_color(fill)?,
            stroke: stroke_color,
            stroke_width: non_negative_f32("stroke width", stroke_width)?,
            corner_radius: non_negative_f32("corner radius", radius)?,
        });
        Ok(())
    }

    pub(super) fn circle_stroke(
        &mut self,
        cx: FLOAT,
        cy: FLOAT,
        radius: FLOAT,
        fill: &str,
        stroke: &str,
        stroke_width: FLOAT,
    ) -> RhaiResult<()> {
        let stroke_color = if stroke.trim().is_empty() {
            None
        } else {
            Some(parse_color(stroke)?)
        };
        self.scene.push(SceneNode::Circle {
            cx: finite_f32("center x", cx)?,
            cy: finite_f32("center y", cy)?,
            r: non_negative_f32("radius", radius)?,
            fill: parse_color(fill)?,
            stroke: stroke_color,
            stroke_width: non_negative_f32("stroke width", stroke_width)?,
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn linear_gradient(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        w: FLOAT,
        h: FLOAT,
        angle_deg: FLOAT,
        color_start: &str,
        color_end: &str,
    ) -> RhaiResult<()> {
        self.scene.push(SceneNode::LinearGradient {
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            w: non_negative_f32("width", w)?,
            h: non_negative_f32("height", h)?,
            angle_deg: finite_f32("angle", angle_deg)?,
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: parse_color(color_start)?,
                },
                GradientStop {
                    position: 1.0,
                    color: parse_color(color_end)?,
                },
            ],
        });
        Ok(())
    }

    pub(super) fn path(
        &mut self,
        d: ImmutableString,
        fill: &str,
        stroke: &str,
        stroke_width: FLOAT,
        opacity: FLOAT,
    ) -> RhaiResult<()> {
        let fill_color = if fill.trim().is_empty() {
            None
        } else {
            Some(parse_color(fill)?)
        };
        let stroke_color = if stroke.trim().is_empty() {
            None
        } else {
            Some(parse_color(stroke)?)
        };
        let opacity = unit_f32("opacity", opacity)?;
        self.scene.push(SceneNode::Path {
            d: d.into_owned(),
            fill: fill_color,
            stroke: stroke_color,
            stroke_width: non_negative_f32("stroke width", stroke_width)?,
            opacity,
        });
        Ok(())
    }

    pub(super) fn group(
        &mut self,
        children: SceneBuilder,
        tx: FLOAT,
        ty: FLOAT,
        scale: FLOAT,
        rotate_deg: FLOAT,
        opacity: FLOAT,
    ) -> RhaiResult<()> {
        let opacity = finite_f32("opacity", opacity)?;
        if !(0.0..=1.0).contains(&opacity) {
            return Err(runtime_error(
                "opacity must be between 0.0 and 1.0".to_string(),
            ));
        }
        let scale = non_negative_f32("scale", scale)?;
        self.scene.push(SceneNode::Group {
            transform: Transform2D {
                tx: finite_f32("translation x", tx)?,
                ty: finite_f32("translation y", ty)?,
                scale_x: scale,
                scale_y: scale,
                rotate_deg: finite_f32("rotation", rotate_deg)?,
            },
            opacity,
            children: children.into_scene().nodes,
        });
        Ok(())
    }
}
