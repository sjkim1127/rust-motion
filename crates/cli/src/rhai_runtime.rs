//! Sandboxed Rhai composition runtime.
//!
//! JSON remains the external props format. A Rhai script receives those props
//! plus an immutable frame context and returns a restricted [`SceneBuilder`].

use crate::composition::{
    layout::prelude::*, Composition, CompositionError, LayoutBox, NativeCompositionContext,
    PreparedComposition,
};
use base64::Engine as _;
use dioxuscut_rasterizer::{
    layout_text_box, parse_glb, parse_gltf, AudioTrack, Color, ConfettiEmitter, ConfettiShape,
    GradientStop, ImageFit, Mesh3D, Scene, SceneNode, TextBox, TextHorizontalAlign, TextOverflow,
    Transform2D, Vec3, VisualizerStyle,
};
use rhai::module_resolvers::DummyModuleResolver;
use rhai::{
    Dynamic, Engine, EvalAltResult, ImmutableString, Map, Position, Scope, AST, FLOAT, INT,
};
use serde_json::Value;
use std::fs;
use std::path::Path;

const MAX_OPERATIONS_PER_FRAME: u64 = 100_000;
const MAX_STRING_SIZE: usize = 1_048_576;
const MAX_ARRAY_SIZE: usize = 4_096;
const MAX_MAP_SIZE: usize = 1_024;

type RhaiResult<T> = Result<T, Box<EvalAltResult>>;

/// A restricted, script-facing builder for the native scene graph.
#[derive(Debug, Clone, Default)]
pub struct SceneBuilder {
    scene: Scene,
}

impl SceneBuilder {
    fn new() -> Self {
        Self::default()
    }

    fn into_scene(self) -> Scene {
        self.scene
    }

    fn rect(&mut self, x: FLOAT, y: FLOAT, w: FLOAT, h: FLOAT, fill: &str) -> RhaiResult<()> {
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

    fn round_rect(
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

    fn circle(&mut self, cx: FLOAT, cy: FLOAT, radius: FLOAT, fill: &str) -> RhaiResult<()> {
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
    fn rect_stroke(
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

    fn circle_stroke(
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
    fn linear_gradient(
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

    fn path(
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

    fn text(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        content: ImmutableString,
        font_size: FLOAT,
        color: &str,
    ) -> RhaiResult<()> {
        self.push_text(x, y, content, font_size, color, 400)
    }

    fn text_bold(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        content: ImmutableString,
        font_size: FLOAT,
        color: &str,
    ) -> RhaiResult<()> {
        self.push_text(x, y, content, font_size, color, 700)
    }

    #[allow(clippy::too_many_arguments)]
    fn text_font(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        content: ImmutableString,
        font_size: FLOAT,
        color: &str,
        font_source: ImmutableString,
    ) -> RhaiResult<()> {
        let font_source = font_source.trim();
        if font_source.is_empty() {
            return Err(runtime_error("font source path must not be empty".into()));
        }
        self.scene.push(SceneNode::Text {
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            content: content.into_owned(),
            font_size: non_negative_f32("font size", font_size)?,
            color: parse_color(color)?,
            font_weight: 400,
            font_sources: vec![font_source.to_string()],
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn text_box(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        width: FLOAT,
        height: FLOAT,
        content: ImmutableString,
        font_size: FLOAT,
        min_font_size: FLOAT,
        max_lines: INT,
        color: &str,
        font_source: ImmutableString,
        align: &str,
    ) -> RhaiResult<()> {
        if max_lines <= 0 {
            return Err(runtime_error("text box max lines must be positive".into()));
        }
        let font_sources = if font_source.trim().is_empty() {
            Vec::new()
        } else {
            vec![font_source.trim().to_string()]
        };
        let mut request = TextBox::new(
            content.into_owned(),
            finite_f32("x", x)?,
            finite_f32("y", y)?,
            finite_f32("width", width)?,
            finite_f32("height", height)?,
            finite_f32("font size", font_size)?,
        );
        request.min_font_size = finite_f32("minimum font size", min_font_size)?;
        request.max_lines = Some(max_lines as usize);
        request.horizontal_align = match align.trim().to_ascii_lowercase().as_str() {
            "left" | "start" => TextHorizontalAlign::Start,
            "center" => TextHorizontalAlign::Center,
            "right" | "end" => TextHorizontalAlign::End,
            _ => {
                return Err(runtime_error(format!(
                    "text box alignment must be start, center, or end, got '{align}'"
                )))
            }
        };
        request.overflow = TextOverflow::Ellipsis;
        request.font_sources = font_sources.clone();
        let layout = layout_text_box(&request)
            .map_err(|error| runtime_error(format!("failed to layout text box: {error}")))?;
        let color = parse_color(color)?;
        for line in layout.lines {
            if line.text.is_empty() {
                continue;
            }
            self.scene.push(SceneNode::Text {
                x: line.x,
                y: line.y,
                content: line.text,
                font_size: layout.font_size,
                color,
                font_weight: 400,
                font_sources: font_sources.clone(),
            });
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn image(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        w: FLOAT,
        h: FLOAT,
        src: ImmutableString,
        fit: &str,
        opacity: FLOAT,
    ) -> RhaiResult<()> {
        if src.trim().is_empty() {
            return Err(runtime_error("image source path must not be empty".into()));
        }
        let opacity = finite_f32("opacity", opacity)?;
        if !(0.0..=1.0).contains(&opacity) {
            return Err(runtime_error(
                "opacity must be between 0.0 and 1.0".to_string(),
            ));
        }

        self.scene.push(SceneNode::Image {
            src: src.into_owned(),
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            w: non_negative_f32("width", w)?,
            h: non_negative_f32("height", h)?,
            fit: parse_image_fit(fit)?,
            opacity,
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn video(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        w: FLOAT,
        h: FLOAT,
        src: ImmutableString,
        time: FLOAT,
        fit: &str,
        opacity: FLOAT,
    ) -> RhaiResult<()> {
        self.video_inner(x, y, w, h, src, time, fit, opacity, false)
    }

    #[allow(clippy::too_many_arguments)]
    fn video_looped(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        w: FLOAT,
        h: FLOAT,
        src: ImmutableString,
        time: FLOAT,
        fit: &str,
        opacity: FLOAT,
        looped: bool,
    ) -> RhaiResult<()> {
        self.video_inner(x, y, w, h, src, time, fit, opacity, looped)
    }

    fn emoji(&mut self, x: FLOAT, y: FLOAT, size: FLOAT, emoji: ImmutableString) -> RhaiResult<()> {
        self.scene.push(SceneNode::Emoji {
            emoji: emoji.into_owned(),
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            size: non_negative_f32("size", size)?,
            opacity: 1.0,
        });
        Ok(())
    }

    fn lottie(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        w: FLOAT,
        h: FLOAT,
        src: ImmutableString,
        time: FLOAT,
    ) -> RhaiResult<()> {
        self.scene.push(SceneNode::Lottie {
            src: src.into_owned(),
            time: time.max(0.0),
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            w: non_negative_f32("width", w)?,
            h: non_negative_f32("height", h)?,
            playback_rate: 1.0,
            loop_behavior: dioxuscut_rasterizer::LoopBehavior::Loop,
            opacity: 1.0,
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn audio_visualizer(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        w: FLOAT,
        h: FLOAT,
        src: ImmutableString,
        time: FLOAT,
        color: ImmutableString,
        style_type: ImmutableString,
    ) -> RhaiResult<()> {
        let style = match style_type.to_lowercase().as_str() {
            "wave" => VisualizerStyle::Wave {
                stroke_width: 3.0,
                filled: true,
            },
            "radial" | "circle" => VisualizerStyle::Radial {
                radius: (h.min(w) * 0.35) as f32,
                bar_count: 48,
                bar_length: (h.min(w) * 0.25) as f32,
            },
            _ => VisualizerStyle::Bars {
                count: 32,
                gap: 4.0,
                radius: 2.0,
                mirror: false,
            },
        };

        self.scene.push(SceneNode::AudioVisualizer {
            src: src.into_owned(),
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            width: non_negative_f32("width", w)?,
            height: non_negative_f32("height", h)?,
            color: Color::from_hex(&color).unwrap_or(Color::WHITE),
            style,
            time: time.max(0.0),
            opacity: 1.0,
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn video_inner(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        w: FLOAT,
        h: FLOAT,
        src: ImmutableString,
        time: FLOAT,
        fit: &str,
        opacity: FLOAT,
        looped: bool,
    ) -> RhaiResult<()> {
        validate_media_source(&src)?;
        let time = non_negative_f64("video time", time)?;
        let opacity = unit_f32("opacity", opacity)?;
        self.scene.push(SceneNode::Video {
            src: src.into_owned(),
            time,
            looped,
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            w: non_negative_f32("width", w)?,
            h: non_negative_f32("height", h)?,
            fit: parse_image_fit(fit)?,
            opacity,
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn audio(
        &mut self,
        src: ImmutableString,
        start_from: FLOAT,
        timeline_start: FLOAT,
        duration: FLOAT,
        volume: FLOAT,
        playback_rate: FLOAT,
        looped: bool,
    ) -> RhaiResult<()> {
        validate_media_source(&src)?;
        let duration = non_negative_f64("audio duration", duration)?;
        let playback_rate = finite_f64("audio playback rate", playback_rate)?;
        if !(0.5..=2.0).contains(&playback_rate) {
            return Err(runtime_error(
                "audio playback rate must be between 0.5 and 2.0".into(),
            ));
        }
        self.scene.push(SceneNode::Audio {
            track: AudioTrack {
                src: src.into_owned(),
                start_from: non_negative_f64("audio source offset", start_from)?,
                timeline_start: non_negative_f64("audio timeline offset", timeline_start)?,
                duration: (duration > 0.0).then_some(duration),
                volume: f64::from(unit_f32("audio volume", volume)?),
                playback_rate,
                looped,
                volume_keyframes: Vec::new(),
            },
        });
        Ok(())
    }

    fn audio_simple(&mut self, src: ImmutableString, volume: FLOAT) -> RhaiResult<()> {
        self.audio(src, 0.0, 0.0, 0.0, volume, 1.0, false)
    }

    fn audio_ducked(
        &mut self,
        src: ImmutableString,
        volume: FLOAT,
        keyframes: rhai::Array,
    ) -> RhaiResult<()> {
        validate_media_source(&src)?;
        let mut kfs = Vec::new();
        for item in keyframes {
            let arr = item.into_typed_array::<FLOAT>().map_err(|_| {
                runtime_error("audio keyframes must be [time, volume] arrays".into())
            })?;
            if arr.len() != 2 {
                return Err(runtime_error(
                    "audio keyframes must contain exactly [time, volume]".into(),
                ));
            }

            let time = non_negative_f64("audio keyframe time", arr[0])?;
            let volume = finite_f64("audio keyframe volume", arr[1])?;
            if !(0.0..=1.0).contains(&volume) {
                return Err(runtime_error(
                    "audio keyframe volume must be between 0.0 and 1.0".into(),
                ));
            }
            kfs.push((time, volume));
        }

        let mut track = AudioTrack::new(src.into_owned());
        track.volume = f64::from(unit_f32("audio volume", volume)?);
        track.volume_keyframes = kfs;
        self.scene.push(SceneNode::Audio { track });
        Ok(())
    }

    fn group(
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

    #[allow(clippy::too_many_arguments)]
    fn cube_3d(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        size: FLOAT,
        pitch: FLOAT,
        yaw: FLOAT,
        roll: FLOAT,
        color: &str,
    ) -> RhaiResult<()> {
        let mut mesh = dioxuscut_rasterizer::Mesh3D::cube(non_negative_f32("size", size)?);
        mesh.rotate(pitch as f32, yaw as f32, roll as f32);
        mesh.render_to_scene(
            &mut self.scene,
            finite_f32("x", x)?,
            finite_f32("y", y)?,
            500.0,
            parse_color(color)?,
            dioxuscut_rasterizer::Vec3::new(0.6, 1.0, 0.8),
            false,
        );
        Ok(())
    }

    fn sphere_3d(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        radius: FLOAT,
        pitch: FLOAT,
        yaw: FLOAT,
        color: &str,
    ) -> RhaiResult<()> {
        let mut mesh =
            dioxuscut_rasterizer::Mesh3D::sphere(non_negative_f32("radius", radius)?, 12, 16);
        mesh.rotate(pitch as f32, yaw as f32, 0.0);
        mesh.render_to_scene(
            &mut self.scene,
            finite_f32("x", x)?,
            finite_f32("y", y)?,
            500.0,
            parse_color(color)?,
            dioxuscut_rasterizer::Vec3::new(0.6, 1.0, 0.8),
            false,
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn torus_3d(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        r_major: FLOAT,
        r_minor: FLOAT,
        pitch: FLOAT,
        yaw: FLOAT,
        color: &str,
    ) -> RhaiResult<()> {
        let mut mesh = dioxuscut_rasterizer::Mesh3D::torus(
            non_negative_f32("major radius", r_major)?,
            non_negative_f32("minor radius", r_minor)?,
            16,
            12,
        );
        mesh.rotate(pitch as f32, yaw as f32, 0.0);
        mesh.render_to_scene(
            &mut self.scene,
            finite_f32("x", x)?,
            finite_f32("y", y)?,
            500.0,
            parse_color(color)?,
            dioxuscut_rasterizer::Vec3::new(0.6, 1.0, 0.8),
            false,
        );
        Ok(())
    }

    fn push_text(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        content: ImmutableString,
        font_size: FLOAT,
        color: &str,
        font_weight: u16,
    ) -> RhaiResult<()> {
        self.scene.push(SceneNode::Text {
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            content: content.into_owned(),
            font_size: non_negative_f32("font size", font_size)?,
            color: parse_color(color)?,
            font_weight,
            font_sources: Vec::new(),
        });
        Ok(())
    }

    fn add(&mut self, layout: RhaiLayout) -> RhaiResult<()> {
        layout
            .inner
            .compute_and_emit(layout.target_width, layout.target_height, &mut self.scene)
            .map_err(|e| runtime_error(format!("Taffy layout calculation failed: {e}")))?;
        Ok(())
    }

    fn add_layout(&mut self, layout: RhaiLayout) -> RhaiResult<()> {
        self.add(layout)
    }

    fn flex_row(&mut self, opts: Map) -> RhaiResult<RhaiLayout> {
        self.make_layout(FlexDirection::Row, opts)
    }

    fn flex_col(&mut self, opts: Map) -> RhaiResult<RhaiLayout> {
        self.make_layout(FlexDirection::Column, opts)
    }

    fn grid(&mut self, opts: Map) -> RhaiResult<RhaiLayout> {
        let cols = get_opt_f32(&opts, "cols", 2.0).max(1.0) as u16;
        let gap = get_opt_f32(&opts, "gap", 0.0);
        let mut box_layout = SceneGrid::columns(cols, gap);
        apply_layout_options(&mut box_layout, &opts)?;
        let width = get_opt_f32(&opts, "width", 1920.0);
        let height = get_opt_f32(&opts, "height", 1080.0);
        Ok(RhaiLayout {
            inner: box_layout,
            target_width: width,
            target_height: height,
        })
    }

    fn make_layout(&self, direction: FlexDirection, opts: Map) -> RhaiResult<RhaiLayout> {
        let gap = get_opt_f32(&opts, "gap", 0.0);
        let justify_str = get_opt_string(&opts, "justify");
        let justify = match justify_str.as_deref() {
            Some("center") => JustifyContent::CENTER,
            Some("end") | Some("flex-end") => JustifyContent::FLEX_END,
            Some("between") | Some("space-between") => JustifyContent::SPACE_BETWEEN,
            Some("around") | Some("space-around") => JustifyContent::SPACE_AROUND,
            Some("evenly") | Some("space-evenly") => JustifyContent::SPACE_EVENLY,
            _ => JustifyContent::FLEX_START,
        };
        let align_str = get_opt_string(&opts, "align");
        let align = match align_str.as_deref() {
            Some("center") => AlignItems::CENTER,
            Some("end") | Some("flex-end") => AlignItems::FLEX_END,
            Some("stretch") => AlignItems::STRETCH,
            Some("baseline") => AlignItems::BASELINE,
            _ => AlignItems::FLEX_START,
        };

        let mut box_layout = match direction {
            FlexDirection::Row => SceneFlex::row(gap, justify, align),
            FlexDirection::Column => SceneFlex::column(gap, justify, align),
            _ => LayoutBox::flex(direction).gap(gap),
        };

        apply_layout_options(&mut box_layout, &opts)?;

        let width = get_opt_f32(&opts, "width", 1920.0);
        let height = get_opt_f32(&opts, "height", 1080.0);

        Ok(RhaiLayout {
            inner: box_layout,
            target_width: width,
            target_height: height,
        })
    }

    fn svg(&mut self, markup: &str) -> RhaiResult<()> {
        let doc = dioxuscut_charts::parse_svg(markup)
            .map_err(|e| runtime_error(format!("SVG parse error: {e}")))?;
        self.scene.nodes.extend(doc.scene.nodes);
        Ok(())
    }

    fn svg_at(&mut self, x: FLOAT, y: FLOAT, markup: &str) -> RhaiResult<()> {
        let doc = dioxuscut_charts::parse_svg(markup)
            .map_err(|e| runtime_error(format!("SVG parse error: {e}")))?;
        let group = SceneNode::Group {
            transform: Transform2D::translate(finite_f32("x", x)?, finite_f32("y", y)?),
            opacity: 1.0,
            children: doc.scene.nodes,
        };
        self.scene.push(group);
        Ok(())
    }

    fn mermaid(&mut self, code: &str) -> RhaiResult<()> {
        let scene = dioxuscut_charts::render_mermaid(code, None)
            .map_err(|e| runtime_error(format!("Mermaid error: {e}")))?;
        self.scene.nodes.extend(scene.nodes);
        Ok(())
    }

    fn mermaid_opts(&mut self, code: &str, opts: Map) -> RhaiResult<()> {
        let theme_str = get_opt_string(&opts, "theme");
        let theme = match theme_str.as_deref() {
            Some("light") => dioxuscut_charts::DiagramTheme::light(),
            _ => dioxuscut_charts::DiagramTheme::dark(),
        };
        let scene = dioxuscut_charts::render_mermaid(code, Some(&theme))
            .map_err(|e| runtime_error(format!("Mermaid error: {e}")))?;

        let x = get_opt_f32(&opts, "x", 0.0);
        let y = get_opt_f32(&opts, "y", 0.0);
        if x != 0.0 || y != 0.0 {
            self.scene.push(SceneNode::Group {
                transform: Transform2D::translate(x, y),
                opacity: 1.0,
                children: scene.nodes,
            });
        } else {
            self.scene.nodes.extend(scene.nodes);
        }
        Ok(())
    }

    fn d3_line(&mut self, data: Dynamic, opts: Map) -> RhaiResult<()> {
        let width = get_opt_f32(&opts, "width", 800.0);
        let height = get_opt_f32(&opts, "height", 450.0);
        let stroke_width = get_opt_f32(&opts, "stroke_width", 3.0);
        let mut chart = dioxuscut_charts::D3LineChart {
            width,
            height,
            stroke_width,
            ..Default::default()
        };
        if let Some(s) = get_opt_string(&opts, "stroke") {
            chart.stroke_color = parse_color(&s)?;
        }
        if let Some(c) = get_opt_string(&opts, "curve") {
            chart.curve = match c.as_str() {
                "linear" => dioxuscut_charts::CurveType::Linear,
                "step" => dioxuscut_charts::CurveType::Step,
                _ => dioxuscut_charts::CurveType::MonotoneX,
            };
        }
        if let Some(f) = opts.get("fill") {
            if let Ok(b) = f.as_bool() {
                chart.fill_area = b;
            }
        }
        if let Some(fc) = get_opt_string(&opts, "fill_color") {
            chart.fill_color = Some(parse_color(&fc)?);
        }
        if let Some(t) = get_opt_string(&opts, "title") {
            chart.title = Some(t);
        }

        let mut points = Vec::new();
        if let Ok(arr) = data.clone().into_array() {
            for (idx, item) in arr.into_iter().enumerate() {
                if let Ok(pair) = item.clone().into_array() {
                    if pair.len() >= 2 {
                        let px = pair[0]
                            .as_float()
                            .map(|f| f as f32)
                            .or_else(|_| pair[0].as_int().map(|i| i as f32))
                            .unwrap_or(idx as f32);
                        let py = pair[1]
                            .as_float()
                            .map(|f| f as f32)
                            .or_else(|_| pair[1].as_int().map(|i| i as f32))
                            .unwrap_or(0.0);
                        points.push((px, py));
                    }
                } else if let Ok(f) = item.as_float() {
                    points.push((idx as f32, f as f32));
                } else if let Ok(i) = item.as_int() {
                    points.push((idx as f32, i as f32));
                }
            }
        }
        chart.points = points;

        let scene = chart.to_scene();
        let x = get_opt_f32(&opts, "x", 0.0);
        let y = get_opt_f32(&opts, "y", 0.0);
        if x != 0.0 || y != 0.0 {
            self.scene.push(SceneNode::Group {
                transform: Transform2D::translate(x, y),
                opacity: 1.0,
                children: scene.nodes,
            });
        } else {
            self.scene.nodes.extend(scene.nodes);
        }
        Ok(())
    }

    fn d3_bar(&mut self, data: Dynamic, opts: Map) -> RhaiResult<()> {
        let width = get_opt_f32(&opts, "width", 800.0);
        let height = get_opt_f32(&opts, "height", 450.0);
        let corner_radius = get_opt_f32(&opts, "corner_radius", 6.0);
        let mut chart = dioxuscut_charts::D3BarChart {
            width,
            height,
            corner_radius,
            ..Default::default()
        };
        if let Some(t) = get_opt_string(&opts, "title") {
            chart.title = Some(t);
        }

        if let Ok(arr) = data.clone().into_array() {
            for (idx, item) in arr.into_iter().enumerate() {
                if let Ok(pair) = item.clone().into_array() {
                    if pair.len() >= 2 {
                        let label = pair[0]
                            .clone()
                            .into_string()
                            .unwrap_or_else(|_| format!("{idx}"));
                        let val = pair[1]
                            .as_float()
                            .map(|f| f as f32)
                            .or_else(|_| pair[1].as_int().map(|i| i as f32))
                            .unwrap_or(0.0);
                        chart.categories.push(label);
                        chart.values.push(val);
                    }
                } else if let Ok(f) = item.as_float() {
                    chart.categories.push(format!("{idx}"));
                    chart.values.push(f as f32);
                } else if let Ok(i) = item.as_int() {
                    chart.categories.push(format!("{idx}"));
                    chart.values.push(i as f32);
                }
            }
        }

        let scene = chart.to_scene();
        let x = get_opt_f32(&opts, "x", 0.0);
        let y = get_opt_f32(&opts, "y", 0.0);
        if x != 0.0 || y != 0.0 {
            self.scene.push(SceneNode::Group {
                transform: Transform2D::translate(x, y),
                opacity: 1.0,
                children: scene.nodes,
            });
        } else {
            self.scene.nodes.extend(scene.nodes);
        }
        Ok(())
    }

    fn d3_pie(&mut self, data: Dynamic, opts: Map) -> RhaiResult<()> {
        let width = get_opt_f32(&opts, "width", 600.0);
        let height = get_opt_f32(&opts, "height", 600.0);
        let outer_radius = width.min(height) * 0.38;
        let mut chart = dioxuscut_charts::D3PieChart {
            width,
            height,
            outer_radius,
            ..Default::default()
        };
        let donut_fraction = get_opt_f32(&opts, "donut", 0.0);
        if donut_fraction > 0.0 {
            chart = chart.with_donut(donut_fraction);
        }
        if let Some(t) = get_opt_string(&opts, "title") {
            chart.title = Some(t);
        }

        if let Ok(arr) = data.clone().into_array() {
            for (idx, item) in arr.into_iter().enumerate() {
                if let Ok(pair) = item.clone().into_array() {
                    if pair.len() >= 2 {
                        let label = pair[0]
                            .clone()
                            .into_string()
                            .unwrap_or_else(|_| format!("{idx}"));
                        let val = pair[1]
                            .as_float()
                            .map(|f| f as f32)
                            .or_else(|_| pair[1].as_int().map(|i| i as f32))
                            .unwrap_or(0.0);
                        chart.slices.push((label, val));
                    }
                } else if let Ok(f) = item.as_float() {
                    chart.slices.push((format!("{idx}"), f as f32));
                } else if let Ok(i) = item.as_int() {
                    chart.slices.push((format!("{idx}"), i as f32));
                }
            }
        }

        let scene = chart.to_scene();
        let x = get_opt_f32(&opts, "x", 0.0);
        let y = get_opt_f32(&opts, "y", 0.0);
        if x != 0.0 || y != 0.0 {
            self.scene.push(SceneNode::Group {
                transform: Transform2D::translate(x, y),
                opacity: 1.0,
                children: scene.nodes,
            });
        } else {
            self.scene.nodes.extend(scene.nodes);
        }
        Ok(())
    }

    fn d3_eval(&mut self, script: &str, data: Dynamic) -> RhaiResult<()> {
        let json_val = rhai::serde::from_dynamic::<Value>(&data)
            .map_err(|e| runtime_error(format!("Failed to serialize data for QuickJS D3: {e}")))?;

        thread_local! {
            static JS_ENGINE: std::cell::RefCell<Option<dioxuscut_charts::QuickJsEngine>> = const { std::cell::RefCell::new(None) };
        }

        let doc = JS_ENGINE
            .with(
                |cell| -> Result<dioxuscut_charts::SvgDocument, dioxuscut_charts::ChartError> {
                    let mut borrow = cell.borrow_mut();
                    if borrow.is_none() {
                        *borrow = Some(dioxuscut_charts::QuickJsEngine::new()?);
                    }
                    borrow.as_ref().unwrap().eval_d3(script, &json_val)
                },
            )
            .map_err(|e| runtime_error(format!("QuickJS D3 execution error: {e}")))?;

        self.scene.nodes.extend(doc.scene.nodes);
        Ok(())
    }

    fn confetti_at(&mut self, x: Dynamic, y: Dynamic, opts: Map) -> RhaiResult<()> {
        let x = dynamic_to_finite_f32(&x, "x")?;
        let y = dynamic_to_finite_f32(&y, "y")?;
        let count = get_opt_usize(&opts, "count", 150);
        let spread = get_opt_f32(&opts, "spread", 70.0);
        let angle = get_opt_f32(&opts, "angle", 270.0);
        let velocity = get_opt_f32(&opts, "velocity", 600.0);
        let gravity = get_opt_f32(&opts, "gravity", 750.0);
        let frame = get_opt_usize(&opts, "frame", 0);
        let fps = get_opt_f32(&opts, "fps", 30.0);
        let seed = get_opt_u64(&opts, "seed", 42);

        let colors =
            if let Some(c_arr) = opts.get("colors").and_then(|v| v.clone().into_array().ok()) {
                let mut list = Vec::new();
                for c in c_arr {
                    if let Ok(s) = c.into_string() {
                        if let Ok(parsed) = parse_color(&s) {
                            list.push(parsed);
                        }
                    }
                }
                if list.is_empty() {
                    None
                } else {
                    Some(list)
                }
            } else {
                None
            };

        let shapes =
            if let Some(s_arr) = opts.get("shapes").and_then(|v| v.clone().into_array().ok()) {
                let mut list = Vec::new();
                for s in s_arr {
                    if let Ok(name) = s.into_string() {
                        match name.to_lowercase().as_str() {
                            "circle" => list.push(ConfettiShape::Circle),
                            "star" => list.push(ConfettiShape::Star),
                            "rect" | "rectangle" => list.push(ConfettiShape::Rectangle),
                            _ => {}
                        }
                    }
                }
                if list.is_empty() {
                    None
                } else {
                    Some(list)
                }
            } else {
                None
            };

        let mut emitter = ConfettiEmitter::new(x, y)
            .with_count(count)
            .with_spread(spread)
            .with_angle(angle)
            .with_velocity(velocity);
        emitter.gravity = gravity;
        emitter.seed = seed;
        if let Some(c) = colors {
            emitter = emitter.with_colors(c);
        }
        if let Some(s) = shapes {
            emitter.shapes = s;
        }

        let nodes = emitter.render_frame(frame, fps);
        self.scene.nodes.extend(nodes);
        Ok(())
    }

    fn confetti(&mut self, opts: Map) -> RhaiResult<()> {
        let x = Dynamic::from_float(get_opt_f32(&opts, "x", 960.0) as FLOAT);
        let y = Dynamic::from_float(get_opt_f32(&opts, "y", 540.0) as FLOAT);
        self.confetti_at(x, y, opts)
    }

    fn confetti_cannon(&mut self, origin: &str, mut opts: Map) -> RhaiResult<()> {
        let (x, y, angle) = match origin.to_lowercase().as_str() {
            "bottom_left" | "left" => (0.0, 1080.0, 315.0),
            "bottom_right" | "right" => (1920.0, 1080.0, 225.0),
            "top" => (960.0, 0.0, 90.0),
            _ => (960.0, 540.0, 270.0),
        };
        if !opts.contains_key("angle") {
            opts.insert("angle".into(), Dynamic::from_float(angle as FLOAT));
        }
        self.confetti_at(
            Dynamic::from_float(x as FLOAT),
            Dynamic::from_float(y as FLOAT),
            opts,
        )
    }

    fn mesh_3d(&mut self, mesh_type: &str, opts: Map) -> RhaiResult<()> {
        let center_x = get_opt_f32(&opts, "x", get_opt_f32(&opts, "center_x", 960.0));
        let center_y = get_opt_f32(&opts, "y", get_opt_f32(&opts, "center_y", 540.0));
        let size = get_opt_f32(&opts, "size", 200.0);
        let scale = get_opt_f32(&opts, "scale", 1.0);
        let pitch = get_opt_f32(&opts, "pitch", get_opt_f32(&opts, "rotate_x", 0.0)).to_radians();
        let yaw = get_opt_f32(&opts, "yaw", get_opt_f32(&opts, "rotate_y", 0.0)).to_radians();
        let roll = get_opt_f32(&opts, "roll", get_opt_f32(&opts, "rotate_z", 0.0)).to_radians();
        let cam_dist = get_opt_f32(&opts, "camera_dist", 600.0);
        let wireframe = opts
            .get("wireframe")
            .and_then(|v| v.as_bool().ok())
            .unwrap_or(false);

        let color_str = get_opt_string(&opts, "color").unwrap_or_else(|| "#3b82f6".to_string());
        let base_color = parse_color(&color_str)?;

        let light_x = get_opt_f32(&opts, "light_x", 0.5);
        let light_y = get_opt_f32(&opts, "light_y", 1.0);
        let light_z = get_opt_f32(&opts, "light_z", 0.8);
        let light_dir = Vec3::new(light_x, light_y, light_z);

        let mut mesh = match mesh_type.to_lowercase().as_str() {
            "cube" | "box" => Mesh3D::cube(size),
            "sphere" => {
                let rings = get_opt_usize(&opts, "rings", 16);
                let sectors = get_opt_usize(&opts, "sectors", 24);
                Mesh3D::sphere(size * 0.5, rings, sectors)
            }
            "torus" | "donut" => {
                let r_major = size * 0.5;
                let r_minor = get_opt_f32(&opts, "tube_radius", r_major * 0.35);
                let segs_major = get_opt_usize(&opts, "segs_major", 24);
                let segs_minor = get_opt_usize(&opts, "segs_minor", 16);
                Mesh3D::torus(r_major, r_minor, segs_major, segs_minor)
            }
            unknown => {
                return Err(runtime_error(format!(
                    "Unknown 3D mesh type '{unknown}'. Valid types: 'cube', 'sphere', 'torus'"
                )))
            }
        };

        if (scale - 1.0).abs() > 1e-4 {
            mesh.scale(scale);
        }
        mesh.rotate(pitch, yaw, roll);

        mesh.render_to_scene(
            &mut self.scene,
            center_x,
            center_y,
            cam_dist,
            base_color,
            light_dir,
            wireframe,
        );

        Ok(())
    }

    fn gltf(&mut self, gltf_source: &str, opts: Map) -> RhaiResult<()> {
        let center_x = get_opt_f32(&opts, "x", get_opt_f32(&opts, "center_x", 960.0));
        let center_y = get_opt_f32(&opts, "y", get_opt_f32(&opts, "center_y", 540.0));
        let scale = get_opt_f32(&opts, "scale", 100.0);
        let pitch = get_opt_f32(&opts, "pitch", get_opt_f32(&opts, "rotate_x", 0.0)).to_radians();
        let yaw = get_opt_f32(&opts, "yaw", get_opt_f32(&opts, "rotate_y", 0.0)).to_radians();
        let roll = get_opt_f32(&opts, "roll", get_opt_f32(&opts, "rotate_z", 0.0)).to_radians();
        let cam_dist = get_opt_f32(&opts, "camera_dist", 600.0);
        let wireframe = opts
            .get("wireframe")
            .and_then(|v| v.as_bool().ok())
            .unwrap_or(false);

        let light_x = get_opt_f32(&opts, "light_x", 0.5);
        let light_y = get_opt_f32(&opts, "light_y", 1.0);
        let light_z = get_opt_f32(&opts, "light_z", 0.8);
        let light_dir = Vec3::new(light_x, light_y, light_z);

        let trimmed = gltf_source.trim();
        let model = if trimmed.starts_with('{') {
            parse_gltf(trimmed, None)
                .map_err(|e| runtime_error(format!("Failed to parse glTF: {e}")))?
        } else if let Some(b64) = trimmed.strip_prefix("data:model/gltf-binary;base64,") {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| runtime_error(format!("Failed to decode GLB base64: {e}")))?;
            parse_glb(&bytes).map_err(|e| runtime_error(format!("Failed to parse GLB: {e}")))?
        } else if let Some(b64) = trimmed.strip_prefix("data:model/gltf+json;base64,") {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| runtime_error(format!("Failed to decode glTF JSON base64: {e}")))?;
            let json = std::str::from_utf8(&bytes)
                .map_err(|e| runtime_error(format!("Invalid UTF-8 in glTF JSON base64: {e}")))?;
            parse_gltf(json, None)
                .map_err(|e| runtime_error(format!("Failed to parse glTF: {e}")))?
        } else {
            // Attempt JSON parse first, then GLB
            parse_gltf(trimmed, None)
                .or_else(|_| parse_glb(trimmed.as_bytes()))
                .map_err(|e| runtime_error(format!("Failed to parse 3D glTF/GLB: {e}")))?
        };

        let base_color = if let Some(color_str) = get_opt_string(&opts, "color") {
            parse_color(&color_str)?
        } else {
            model.base_color
        };

        let time_sec = if let Some(t) = opts
            .get("time")
            .and_then(|v| v.as_float().ok().map(|f| f as f32))
        {
            t
        } else if let Some(f) = opts.get("frame").and_then(|v| v.as_int().ok()) {
            let fps = opts
                .get("fps")
                .and_then(|v| v.as_float().ok())
                .unwrap_or(30.0) as f32;
            f as f32 / fps
        } else {
            0.0
        };

        let mut mesh = if opts.contains_key("time") || opts.contains_key("frame") {
            model.sample_pose(time_sec)
        } else {
            model.mesh
        };

        if (scale - 1.0).abs() > 1e-4 {
            mesh.scale(scale);
        }
        mesh.rotate(pitch, yaw, roll);

        mesh.render_to_scene(
            &mut self.scene,
            center_x,
            center_y,
            cam_dist,
            base_color,
            light_dir,
            wireframe,
        );

        Ok(())
    }

    fn shader(&mut self, source: &str, opts: Map) -> RhaiResult<()> {
        let x = get_opt_f32(&opts, "x", 0.0);
        let y = get_opt_f32(&opts, "y", 0.0);
        let w = get_opt_f32(&opts, "w", get_opt_f32(&opts, "width", 1920.0));
        let h = get_opt_f32(&opts, "h", get_opt_f32(&opts, "height", 1080.0));
        let time = get_opt_f32(&opts, "time", 0.0);
        let opacity = get_opt_f32(&opts, "opacity", 1.0);

        let p0 = get_opt_f32(&opts, "p0", get_opt_f32(&opts, "param0", 1.0));
        let p1 = get_opt_f32(&opts, "p1", get_opt_f32(&opts, "param1", 1.0));
        let p2 = get_opt_f32(&opts, "p2", get_opt_f32(&opts, "param2", 1.0));
        let p3 = get_opt_f32(&opts, "p3", get_opt_f32(&opts, "param3", 1.0));

        let params = if let Some(arr) = opts.get("params").and_then(|v| v.clone().into_array().ok())
        {
            [
                arr.first()
                    .and_then(|v| v.as_float().ok())
                    .map(|f| f as f32)
                    .unwrap_or(p0),
                arr.get(1)
                    .and_then(|v| v.as_float().ok())
                    .map(|f| f as f32)
                    .unwrap_or(p1),
                arr.get(2)
                    .and_then(|v| v.as_float().ok())
                    .map(|f| f as f32)
                    .unwrap_or(p2),
                arr.get(3)
                    .and_then(|v| v.as_float().ok())
                    .map(|f| f as f32)
                    .unwrap_or(p3),
            ]
        } else {
            [p0, p1, p2, p3]
        };

        self.scene.push(SceneNode::Shader {
            x,
            y,
            w,
            h,
            source: source.to_string(),
            time,
            params,
            opacity,
        });

        Ok(())
    }
}

fn get_opt_f32(map: &Map, key: &str, default: f32) -> f32 {
    if let Some(v) = map.get(key) {
        if let Ok(f) = v.as_float() {
            return f as f32;
        }
        if let Ok(i) = v.as_int() {
            return i as f32;
        }
    }
    default
}

fn get_opt_f32_opt(map: &Map, key: &str) -> Option<f32> {
    if let Some(v) = map.get(key) {
        if let Ok(f) = v.as_float() {
            return Some(f as f32);
        }
        if let Ok(i) = v.as_int() {
            return Some(i as f32);
        }
    }
    None
}

fn get_opt_string(map: &Map, key: &str) -> Option<String> {
    map.get(key).and_then(|v| v.clone().into_string().ok())
}

fn get_opt_usize(map: &Map, key: &str, default: usize) -> usize {
    if let Some(v) = map.get(key) {
        if let Ok(i) = v.as_int() {
            if i >= 0 {
                return i as usize;
            }
        }
    }
    default
}

fn get_opt_u64(map: &Map, key: &str, default: u64) -> u64 {
    if let Some(v) = map.get(key) {
        if let Ok(i) = v.as_int() {
            if i >= 0 {
                return i as u64;
            }
        }
    }
    default
}

fn apply_layout_options(box_layout: &mut LayoutBox, opts: &Map) -> RhaiResult<()> {
    if let Some(p) = get_opt_f32_opt(opts, "padding") {
        box_layout.style.padding = Rect {
            left: length(p),
            right: length(p),
            top: length(p),
            bottom: length(p),
        };
    }
    if let Some(w) = get_opt_f32_opt(opts, "width") {
        box_layout.style.size.width = length(w);
    }
    if let Some(h) = get_opt_f32_opt(opts, "height") {
        box_layout.style.size.height = length(h);
    }
    if let Some(bg) = get_opt_string(opts, "bg") {
        box_layout.background = Some(parse_color(&bg)?);
    }
    if let Some(bc) = get_opt_string(opts, "border_color") {
        let bw = get_opt_f32(opts, "border_width", 1.0);
        let br = get_opt_f32(opts, "border_radius", 0.0);
        box_layout.border = Some((parse_color(&bc)?, bw, br));
    }
    Ok(())
}

fn dynamic_to_non_negative_f32(val: &Dynamic, name: &str) -> RhaiResult<f32> {
    if let Ok(f) = val.as_float() {
        return non_negative_f32(name, f);
    }
    if let Ok(i) = val.as_int() {
        return non_negative_f32(name, i as FLOAT);
    }
    Err(runtime_error(format!("{name} must be a valid number")))
}

fn dynamic_to_finite_f32(val: &Dynamic, name: &str) -> RhaiResult<f32> {
    if let Ok(f) = val.as_float() {
        return finite_f32(name, f);
    }
    if let Ok(i) = val.as_int() {
        return finite_f32(name, i as FLOAT);
    }
    Err(runtime_error(format!("{name} must be a valid number")))
}

/// A script-accessible declarative layout container.
#[derive(Debug, Clone)]
pub struct RhaiLayout {
    inner: LayoutBox,
    target_width: f32,
    target_height: f32,
}

impl RhaiLayout {
    fn rect(&mut self, w: Dynamic, h: Dynamic, fill: &str) -> RhaiResult<()> {
        let node = SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: dynamic_to_non_negative_f32(&w, "width")?,
            h: dynamic_to_non_negative_f32(&h, "height")?,
            fill: parse_color(fill)?,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        };
        self.inner.push_node(node, None);
        Ok(())
    }

    fn round_rect(
        &mut self,
        w: Dynamic,
        h: Dynamic,
        fill: &str,
        radius: Dynamic,
    ) -> RhaiResult<()> {
        let node = SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: dynamic_to_non_negative_f32(&w, "width")?,
            h: dynamic_to_non_negative_f32(&h, "height")?,
            fill: parse_color(fill)?,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: dynamic_to_non_negative_f32(&radius, "corner radius")?,
        };
        self.inner.push_node(node, None);
        Ok(())
    }

    fn circle(&mut self, radius: Dynamic, fill: &str) -> RhaiResult<()> {
        let node = SceneNode::Circle {
            cx: 0.0,
            cy: 0.0,
            r: dynamic_to_non_negative_f32(&radius, "radius")?,
            fill: parse_color(fill)?,
            stroke: None,
            stroke_width: 0.0,
        };
        self.inner.push_node(node, None);
        Ok(())
    }

    fn text(
        &mut self,
        content: ImmutableString,
        font_size: Dynamic,
        color: &str,
    ) -> RhaiResult<()> {
        let size = dynamic_to_non_negative_f32(&font_size, "font size")?;
        let col = parse_color(color)?;
        let approx_w = content.chars().count() as f32 * size * 0.6;
        let approx_h = size * 1.2;
        let node = SceneNode::Text {
            x: 0.0,
            y: 0.0,
            content: content.into_owned(),
            font_size: size,
            color: col,
            font_weight: 400,
            font_sources: Vec::new(),
        };
        let style = Style {
            size: Size {
                width: length(approx_w),
                height: length(approx_h),
            },
            ..Default::default()
        };
        self.inner.push_node(node, Some(style));
        Ok(())
    }

    fn text_bold(
        &mut self,
        content: ImmutableString,
        font_size: Dynamic,
        color: &str,
    ) -> RhaiResult<()> {
        let size = dynamic_to_non_negative_f32(&font_size, "font size")?;
        let col = parse_color(color)?;
        let approx_w = content.chars().count() as f32 * size * 0.6;
        let approx_h = size * 1.2;
        let node = SceneNode::Text {
            x: 0.0,
            y: 0.0,
            content: content.into_owned(),
            font_size: size,
            color: col,
            font_weight: 700,
            font_sources: Vec::new(),
        };
        let style = Style {
            size: Size {
                width: length(approx_w),
                height: length(approx_h),
            },
            ..Default::default()
        };
        self.inner.push_node(node, Some(style));
        Ok(())
    }
}

/// A compiled Rhai composition. Constructing this type compiles the script once.
pub struct RhaiComposition {
    id: String,
    engine: Engine,
    ast: AST,
}

impl RhaiComposition {
    pub fn from_file(path: &Path) -> Result<Self, CompositionError> {
        let source = fs::read_to_string(path).map_err(|error| {
            CompositionError::Prepare(format!(
                "failed to read Rhai script {}: {error}",
                path.display()
            ))
        })?;
        let id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("RhaiComposition");
        Self::from_source(id, &source)
    }

    pub fn from_source(id: impl Into<String>, source: &str) -> Result<Self, CompositionError> {
        let mut engine = hardened_engine();
        register_scene_api(&mut engine);
        let ast = engine
            .compile(source)
            .map_err(|error| CompositionError::Prepare(format!("Rhai compile error: {error}")))?;

        Ok(Self {
            id: id.into(),
            engine,
            ast,
        })
    }
}

impl Composition for RhaiComposition {
    fn id(&self) -> &str {
        &self.id
    }

    fn prepare(
        &self,
        props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Box<dyn PreparedComposition + '_>, CompositionError> {
        let props = rhai::serde::to_dynamic(props).map_err(|error| {
            CompositionError::Prepare(format!("failed to convert JSON props to Rhai: {error}"))
        })?;

        Ok(Box::new(PreparedRhaiComposition {
            engine: &self.engine,
            ast: &self.ast,
            props,
            context,
        }))
    }
}

struct PreparedRhaiComposition<'a> {
    engine: &'a Engine,
    ast: &'a AST,
    props: Dynamic,
    context: NativeCompositionContext,
}

impl PreparedComposition for PreparedRhaiComposition<'_> {
    fn render(&self, frame: u32) -> Result<Scene, CompositionError> {
        let mut scope = Scope::new();
        let context = context_map(frame, self.context);
        let builder = self
            .engine
            .call_fn::<SceneBuilder>(
                &mut scope,
                self.ast,
                "render",
                (context, self.props.clone()),
            )
            .map_err(|error| CompositionError::render(frame, format!("Rhai error: {error}")))?;
        Ok(builder.into_scene())
    }
}

fn hardened_engine() -> Engine {
    let mut engine = Engine::new();
    engine.set_module_resolver(DummyModuleResolver::new());
    engine.set_max_operations(MAX_OPERATIONS_PER_FRAME);
    engine.set_max_call_levels(32);
    engine.set_max_expr_depths(64, 32);
    engine.set_max_variables(256);
    engine.set_max_functions(128);
    engine.set_max_string_size(MAX_STRING_SIZE);
    engine.set_max_array_size(MAX_ARRAY_SIZE);
    engine.set_max_map_size(MAX_MAP_SIZE);
    engine
}

fn register_scene_api(engine: &mut Engine) {
    engine.register_type_with_name::<SceneBuilder>("Scene");
    engine.register_fn("scene", SceneBuilder::new);
    engine.register_fn("rect", SceneBuilder::rect);
    engine.register_fn("round_rect", SceneBuilder::round_rect);
    engine.register_fn("rect_stroke", SceneBuilder::rect_stroke);
    engine.register_fn("circle", SceneBuilder::circle);
    engine.register_fn("circle_stroke", SceneBuilder::circle_stroke);
    engine.register_fn("linear_gradient", SceneBuilder::linear_gradient);
    engine.register_fn("path", SceneBuilder::path);
    engine.register_fn("text", SceneBuilder::text);
    engine.register_fn("text_bold", SceneBuilder::text_bold);
    engine.register_fn("text_font", SceneBuilder::text_font);
    engine.register_fn("text_box", SceneBuilder::text_box);
    engine.register_fn("image", SceneBuilder::image);
    engine.register_fn("video", SceneBuilder::video);
    engine.register_fn("video", SceneBuilder::video_looped);
    engine.register_fn("audio", SceneBuilder::audio);
    engine.register_fn("audio", SceneBuilder::audio_simple);
    engine.register_fn("audio_ducked", SceneBuilder::audio_ducked);
    engine.register_fn("emoji", SceneBuilder::emoji);
    engine.register_fn("lottie", SceneBuilder::lottie);
    engine.register_fn("audio_visualizer", SceneBuilder::audio_visualizer);
    engine.register_fn("group", SceneBuilder::group);
    engine.register_fn("flex_row", SceneBuilder::flex_row);
    engine.register_fn("flex_col", SceneBuilder::flex_col);
    engine.register_fn("grid", SceneBuilder::grid);
    engine.register_fn("svg", SceneBuilder::svg);
    engine.register_fn("svg_at", SceneBuilder::svg_at);
    engine.register_fn("mermaid", SceneBuilder::mermaid);
    engine.register_fn("mermaid", SceneBuilder::mermaid_opts);
    engine.register_fn("d3_line", SceneBuilder::d3_line);
    engine.register_fn("d3_bar", SceneBuilder::d3_bar);
    engine.register_fn("d3_pie", SceneBuilder::d3_pie);
    engine.register_fn("d3_eval", SceneBuilder::d3_eval);
    engine.register_fn("confetti", SceneBuilder::confetti);
    engine.register_fn("confetti", SceneBuilder::confetti_at);
    engine.register_fn("confetti_cannon", SceneBuilder::confetti_cannon);
    engine.register_fn("mesh_3d", SceneBuilder::mesh_3d);
    engine.register_fn("gltf", SceneBuilder::gltf);
    engine.register_fn("gltf", |scene: &mut SceneBuilder, source: &str| {
        scene.gltf(source, Map::new())
    });
    engine.register_fn("shader", SceneBuilder::shader);
    engine.register_fn("shader", |scene: &mut SceneBuilder, source: &str| {
        scene.shader(source, Map::new())
    });

    engine.register_type_with_name::<RhaiLayout>("RhaiLayout");
    engine.register_fn("rect", RhaiLayout::rect);
    engine.register_fn("round_rect", RhaiLayout::round_rect);
    engine.register_fn("circle", RhaiLayout::circle);
    engine.register_fn("text", RhaiLayout::text);
    engine.register_fn("text_bold", RhaiLayout::text_bold);
    engine.register_fn("add", SceneBuilder::add);
    engine.register_fn("add_layout", SceneBuilder::add_layout);
    engine.register_fn(
        "interpolate",
        |value: FLOAT,
         input_start: FLOAT,
         input_end: FLOAT,
         output_start: FLOAT,
         output_end: FLOAT| {
            if input_start == input_end {
                return output_end;
            }
            let t = ((value - input_start) / (input_end - input_start)).clamp(0.0, 1.0);
            output_start + (output_end - output_start) * t
        },
    );
    engine.register_fn(
        "interpolate_colors",
        |from: ImmutableString, to: ImmutableString, t: FLOAT| -> ImmutableString {
            dioxuscut_animation::interpolate_colors(from.as_str(), to.as_str(), t).into()
        },
    );
    engine.register_fn("random", |seed: ImmutableString| -> FLOAT {
        dioxuscut_animation::random(seed.as_str())
    });
    engine.register_fn("random", |seed: INT| -> FLOAT {
        dioxuscut_animation::random(seed as f64)
    });
    engine.register_fn("random", |seed: FLOAT| -> FLOAT {
        dioxuscut_animation::random(seed)
    });
    engine.register_fn("spring", |frame: FLOAT, fps: FLOAT| -> FLOAT {
        let config = dioxuscut_animation::SpringConfig::default();
        dioxuscut_animation::spring_with_options(
            frame,
            fps,
            config,
            dioxuscut_animation::SpringOptions::default(),
        )
        .unwrap_or(1.0)
    });
    engine.register_fn(
        "spring",
        |frame: FLOAT, fps: FLOAT, damping: FLOAT, mass: FLOAT, stiffness: FLOAT| -> FLOAT {
            let config = dioxuscut_animation::SpringConfig {
                damping,
                mass,
                stiffness,
                overshoot_clamping: false,
            };
            dioxuscut_animation::spring_with_options(
                frame,
                fps,
                config,
                dioxuscut_animation::SpringOptions::default(),
            )
            .unwrap_or(1.0)
        },
    );
    engine.register_fn("static_file", |path: ImmutableString| -> ImmutableString {
        dioxuscut_media::static_file(path.as_str())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string())
            .into()
    });
    engine.register_fn("cube_3d", SceneBuilder::cube_3d);
    engine.register_fn("sphere_3d", SceneBuilder::sphere_3d);
    engine.register_fn("torus_3d", SceneBuilder::torus_3d);
    engine.register_fn(
        "lfo",
        |frame: FLOAT, fps: FLOAT, freq_hz: FLOAT, amp: FLOAT| -> FLOAT {
            let lfo =
                dioxuscut_animation::LfoChop::new(dioxuscut_animation::LfoWave::Sine, freq_hz)
                    .with_amplitude(amp);
            lfo.sample(frame, fps)
        },
    );
    engine.register_fn(
        "lag",
        |current: FLOAT, target: FLOAT, dt: FLOAT, lag_sec: FLOAT| -> FLOAT {
            let filter = dioxuscut_animation::LagChop::new(lag_sec, lag_sec);
            filter.filter(current, target, dt)
        },
    );
    engine.register_fn("clamp", |val: FLOAT, min: FLOAT, max: FLOAT| -> FLOAT {
        if val.is_nan() {
            min
        } else {
            val.clamp(min, max)
        }
    });
    engine.register_fn(
        "typewriter_text",
        |text: ImmutableString, progress: FLOAT| -> ImmutableString {
            dioxuscut_animation::typewriter_text(&text, progress, true, '|').into()
        },
    );
    engine.register_fn(
        "scramble_text",
        |text: ImmutableString, progress: FLOAT, seed: INT| -> ImmutableString {
            dioxuscut_animation::scramble_text(&text, progress, seed as u64).into()
        },
    );
    engine.register_fn("make_ellipse", |rx: FLOAT, ry: FLOAT| -> ImmutableString {
        dioxuscut_shapes::make_ellipse(rx, ry).path.into()
    });
    engine.register_fn(
        "safe_area_insets",
        |platform: ImmutableString, w: FLOAT, h: FLOAT| -> Map {
            let plat = match platform.to_lowercase().as_str() {
                "reels" | "instagram" => dioxuscut_composition::Platform::InstagramReels,
                "shorts" | "youtube" => dioxuscut_composition::Platform::YouTubeShorts,
                "action" => dioxuscut_composition::Platform::ActionSafe,
                "title" => dioxuscut_composition::Platform::TitleSafe,
                _ => dioxuscut_composition::Platform::TikTok,
            };
            let insets = dioxuscut_composition::get_safe_area_insets(plat, w as f32, h as f32);
            let mut map = Map::new();
            map.insert("top".into(), Dynamic::from(insets.top as FLOAT));
            map.insert("bottom".into(), Dynamic::from(insets.bottom as FLOAT));
            map.insert("left".into(), Dynamic::from(insets.left as FLOAT));
            map.insert("right".into(), Dynamic::from(insets.right as FLOAT));
            map
        },
    );
    engine.register_fn(
        "fit_text",
        |text: ImmutableString, max_w: FLOAT, max_h: FLOAT| -> FLOAT {
            dioxuscut_composition::fit_text(&text, max_w as f32, max_h as f32, 10.0, 120.0) as FLOAT
        },
    );
    engine.register_fn("path_bounding_box", |path: ImmutableString| -> Map {
        let mut map = Map::new();
        if let Some(bbox) = dioxuscut_paths::get_bounding_box(&path) {
            map.insert("x".into(), Dynamic::from(bbox.x));
            map.insert("y".into(), Dynamic::from(bbox.y));
            map.insert("width".into(), Dynamic::from(bbox.width));
            map.insert("height".into(), Dynamic::from(bbox.height));
        }
        map
    });
    engine.register_fn(
        "rotate_path",
        |path: ImmutableString, angle_rad: FLOAT, cx: FLOAT, cy: FLOAT| -> ImmutableString {
            dioxuscut_paths::rotate_path(&path, angle_rad, cx, cy).into()
        },
    );
    engine.register_fn("reverse_path", |path: ImmutableString| -> ImmutableString {
        dioxuscut_paths::reverse_path(&path).into()
    });
}

fn context_map(frame: u32, context: NativeCompositionContext) -> Map {
    let mut map = Map::new();
    map.insert("frame".into(), Dynamic::from(frame as INT));
    map.insert(
        "time".into(),
        Dynamic::from((frame as f64) / context.fps.max(0.001)),
    );
    map.insert("width".into(), Dynamic::from(context.width as INT));
    map.insert("height".into(), Dynamic::from(context.height as INT));
    map.insert("fps".into(), Dynamic::from(context.fps as FLOAT));
    map.insert(
        "duration".into(),
        Dynamic::from(context.duration_in_frames as INT),
    );
    map.insert(
        "progress".into(),
        Dynamic::from(context.progress(frame) as FLOAT),
    );
    map
}

fn finite_f32(name: &str, value: FLOAT) -> RhaiResult<f32> {
    if !value.is_finite() || value < f32::MIN as FLOAT || value > f32::MAX as FLOAT {
        return Err(runtime_error(format!(
            "{name} must be a finite 32-bit number"
        )));
    }
    Ok(value as f32)
}

fn non_negative_f32(name: &str, value: FLOAT) -> RhaiResult<f32> {
    let value = finite_f32(name, value)?;
    if value < 0.0 {
        return Err(runtime_error(format!("{name} must not be negative")));
    }
    Ok(value)
}

fn finite_f64(name: &str, value: FLOAT) -> RhaiResult<f64> {
    if !value.is_finite() {
        return Err(runtime_error(format!("{name} must be finite")));
    }
    Ok(value)
}

fn non_negative_f64(name: &str, value: FLOAT) -> RhaiResult<f64> {
    let value = finite_f64(name, value)?;
    if value < 0.0 {
        return Err(runtime_error(format!("{name} must not be negative")));
    }
    Ok(value)
}

fn unit_f32(name: &str, value: FLOAT) -> RhaiResult<f32> {
    let value = finite_f32(name, value)?;
    if !(0.0..=1.0).contains(&value) {
        return Err(runtime_error(format!("{name} must be between 0.0 and 1.0")));
    }
    Ok(value)
}

fn validate_media_source(value: &str) -> RhaiResult<()> {
    if value.trim().is_empty() {
        Err(runtime_error("media source path must not be empty".into()))
    } else {
        Ok(())
    }
}

fn parse_color(value: &str) -> RhaiResult<Color> {
    Color::from_css(value).ok_or_else(|| {
        runtime_error(format!(
            "invalid color '{value}'; expected CSS color (#rrggbb, #rrggbbaa, rgba(...), or name)"
        ))
    })
}

fn parse_image_fit(value: &str) -> RhaiResult<ImageFit> {
    match value {
        "cover" => Ok(ImageFit::Cover),
        "contain" => Ok(ImageFit::Contain),
        "fill" => Ok(ImageFit::Fill),
        "none" => Ok(ImageFit::None),
        "scale-down" => Ok(ImageFit::ScaleDown),
        _ => Err(runtime_error(format!(
            "invalid image fit '{value}'; expected cover, contain, fill, none, or scale-down"
        ))),
    }
}

fn runtime_error(message: String) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> NativeCompositionContext {
        NativeCompositionContext {
            width: 320,
            height: 180,
            fps: 30.0,
            duration_in_frames: 10,
        }
    }

    #[test]
    fn script_builds_a_deterministic_scene_from_context_and_props() {
        let script = r##"
            fn render(ctx, props) {
                let output = scene();
                output.rect(0.0, 0.0, ctx.width.to_float(), ctx.height.to_float(), props.background);
                let x = interpolate(ctx.frame.to_float(), 0.0, 9.0, 0.0, 90.0);
                output.text_bold(x, 80.0, props.title, 24.0, "#ffffff");
                output
            }
        "##;
        let composition = RhaiComposition::from_source("test", script).unwrap();
        let props = serde_json::json!({
            "background": "#102030",
            "title": "Hello Rhai"
        });
        let prepared = composition.prepare(&props, context()).unwrap();

        let first = prepared.render(3).unwrap();
        let second = prepared.render(3).unwrap();
        assert_eq!(first.nodes, second.nodes);
        assert!(matches!(
            &first.nodes[1],
            SceneNode::Text { x, content, .. }
                if (*x - 30.0).abs() < f32::EPSILON && content == "Hello Rhai"
        ));
    }

    #[test]
    fn script_declares_an_explicit_font_source() {
        let script = r##"
            fn render(ctx, props) {
                let output = scene();
                output.text_font(10.0, 30.0, "Pinned", 20.0, "#ffffff", props.font);
                output
            }
        "##;
        let composition = RhaiComposition::from_source("font", script).unwrap();
        let prepared = composition
            .prepare(&serde_json::json!({"font": "assets/Inter.ttf"}), context())
            .unwrap();
        let scene = prepared.render(0).unwrap();

        assert!(matches!(
            &scene.nodes[0],
            SceneNode::Text { font_sources, .. } if font_sources == &["assets/Inter.ttf"]
        ));
    }

    #[test]
    fn script_resolves_a_fitted_multiline_text_box() {
        let script = r##"
            fn render(ctx, props) {
                let output = scene();
                output.text_box(
                    10.0, 20.0, 120.0, 52.0,
                    "one two three four five six", 32.0, 14.0, 2,
                    "#ffffff", props.font, "center"
                );
                output
            }
        "##;
        let font_cache = dioxuscut_rasterizer::FontCache::load();
        let Some(font) = font_cache.font_path() else {
            return;
        };
        let composition = RhaiComposition::from_source("text-box", script).unwrap();
        let prepared = composition
            .prepare(&serde_json::json!({"font": font}), context())
            .unwrap();
        let scene = prepared.render(0).unwrap();

        assert!(!scene.nodes.is_empty());
        assert!(scene.nodes.len() <= 2);
        assert!(scene.nodes.iter().all(|node| matches!(
            node,
            SceneNode::Text { x, font_size, .. } if *x >= 10.0 && *font_size <= 32.0
        )));
    }

    #[test]
    fn operation_limit_stops_an_infinite_loop() {
        let composition = RhaiComposition::from_source(
            "infinite",
            "fn render(ctx, props) { while true {} scene() }",
        )
        .unwrap();
        let prepared = composition
            .prepare(&serde_json::json!({}), context())
            .unwrap();

        let error = prepared.render(0).unwrap_err();
        assert!(error.to_string().contains("Too many operations"));
    }

    #[test]
    fn script_builds_a_local_image_node() {
        let script = r#"
            fn render(ctx, props) {
                let output = scene();
                output.image(10.0, 20.0, 100.0, 60.0, props.src, "contain", 0.75);
                output
            }
        "#;
        let composition = RhaiComposition::from_source("image", script).unwrap();
        let prepared = composition
            .prepare(&serde_json::json!({"src": "assets/card.png"}), context())
            .unwrap();
        let scene = prepared.render(0).unwrap();

        assert!(matches!(
            &scene.nodes[0],
            SceneNode::Image { src, fit: ImageFit::Contain, opacity, .. }
                if src == "assets/card.png" && (*opacity - 0.75).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn script_rejects_invalid_image_fit() {
        let script = r#"
            fn render(ctx, props) {
                let output = scene();
                output.image(0.0, 0.0, 10.0, 10.0, "asset.png", "stretchy", 1.0);
                output
            }
        "#;
        let composition = RhaiComposition::from_source("bad-image", script).unwrap();
        let prepared = composition
            .prepare(&serde_json::json!({}), context())
            .unwrap();
        let error = prepared.render(0).unwrap_err();

        assert!(error.to_string().contains("invalid image fit"));
    }

    #[test]
    fn script_builds_video_and_audio_nodes() {
        let script = r#"
            fn render(ctx, props) {
                let output = scene();
                output.video(0.0, 0.0, 320.0, 180.0, props.video, ctx.frame.to_float() / ctx.fps, "cover", 1.0);
                output.video(0.0, 0.0, 320.0, 180.0, props.video, ctx.frame.to_float() / ctx.fps, "contain", 0.5, true);
                output.audio(props.video, 0.25, 0.5, 2.0, 0.75, 1.25, true);
                output
            }
        "#;
        let composition = RhaiComposition::from_source("media", script).unwrap();
        let prepared = composition
            .prepare(&serde_json::json!({"video": "assets/clip.mp4"}), context())
            .unwrap();
        let scene = prepared.render(3).unwrap();

        assert!(matches!(
            &scene.nodes[0],
            SceneNode::Video { src, time, fit: ImageFit::Cover, looped: false, .. }
                if src == "assets/clip.mp4" && (*time - 0.1).abs() < f64::EPSILON
        ));
        assert!(matches!(
            &scene.nodes[1],
            SceneNode::Video { fit: ImageFit::Contain, opacity, looped: true, .. }
                if (*opacity - 0.5).abs() < f32::EPSILON
        ));
        let tracks = scene.audio_tracks();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].src, "assets/clip.mp4");
        assert_eq!(tracks[0].duration, Some(2.0));
        assert!(tracks[0].looped);
    }

    #[test]
    fn script_builds_a_valid_ducked_audio_track() {
        let script = r#"
            fn render(ctx, props) {
                let output = scene();
                output.audio_ducked("voice.wav", 0.8, [[0.0, 1.0], [0.5, 0.25]]);
                output
            }
        "#;
        let composition = RhaiComposition::from_source("valid-ducking", script).unwrap();
        let prepared = composition
            .prepare(&serde_json::json!({}), context())
            .unwrap();
        let scene = prepared.render(0).unwrap();
        let tracks = scene.audio_tracks();

        assert_eq!(tracks.len(), 1);
        assert!((tracks[0].volume - 0.8).abs() < 1e-6);
        assert_eq!(tracks[0].volume_keyframes, vec![(0.0, 1.0), (0.5, 0.25)]);
    }

    #[test]
    fn script_rejects_invalid_ducked_audio_values_at_evaluation_time() {
        let invalid_calls = [
            (r#"audio_ducked("voice.wav", 1.5, [])"#, "audio volume"),
            (
                r#"audio_ducked("voice.wav", 0.5, [[-1.0, 0.5]])"#,
                "audio keyframe time",
            ),
            (
                r#"audio_ducked("voice.wav", 0.5, [[parse_float("NaN"), 0.5]])"#,
                "audio keyframe time",
            ),
            (
                r#"audio_ducked("voice.wav", 0.5, [[0.0, -0.1]])"#,
                "audio keyframe volume",
            ),
            (
                r#"audio_ducked("voice.wav", 0.5, [[0.0, 1.1]])"#,
                "audio keyframe volume",
            ),
            (
                r#"audio_ducked("voice.wav", 0.5, [[0.0, parse_float("NaN")]])"#,
                "audio keyframe volume",
            ),
        ];

        for (call, expected_error) in invalid_calls {
            let script =
                format!("fn render(ctx, props) {{ let output = scene(); output.{call}; output }}");
            let composition = RhaiComposition::from_source("invalid-ducking", &script).unwrap();
            let prepared = composition
                .prepare(&serde_json::json!({}), context())
                .unwrap();
            let error = prepared.render(0).unwrap_err();
            assert!(
                error.to_string().contains(expected_error),
                "expected '{expected_error}' for `{call}`, got: {error}"
            );
        }
    }

    #[test]
    fn unregistered_scene_api_is_rejected() {
        let composition = RhaiComposition::from_source(
            "unknown-api",
            "fn render(ctx, props) { let output = scene(); output.read_file(\"secret\"); output }",
        )
        .unwrap();
        let prepared = composition
            .prepare(&serde_json::json!({}), context())
            .unwrap();

        let error = prepared.render(0).unwrap_err();
        assert!(error.to_string().contains("Function not found"));
    }

    #[test]
    fn module_imports_are_disabled() {
        let composition = RhaiComposition::from_source(
            "import",
            "import \"untrusted\" as imported; fn render(ctx, props) { scene() }",
        )
        .unwrap();
        let prepared = composition
            .prepare(&serde_json::json!({}), context())
            .unwrap();
        let error = prepared.render(0).unwrap_err();
        assert!(
            error.to_string().contains("Module not found"),
            "unexpected import error: {error}"
        );
    }

    #[test]
    fn script_uses_animation_primitives() {
        let script = r##"
            fn render(ctx, props) {
                let rnd = random("seed-42");
                let col = interpolate_colors("#000000", "#ffffff", 0.5);
                let sp = spring(ctx.frame.to_float(), ctx.fps);
                let output = scene();
                output.rect(rnd * 100.0, sp * 50.0, 200.0, 100.0, col);
                output
            }
        "##;
        let composition = RhaiComposition::from_source("anim", script).unwrap();
        let prepared = composition
            .prepare(&serde_json::json!({}), context())
            .unwrap();
        let scene = prepared.render(15).unwrap();
        assert_eq!(scene.nodes.len(), 1);
        if let SceneNode::Rect { fill, .. } = &scene.nodes[0] {
            assert_eq!(fill.r, 128);
            assert_eq!(fill.g, 128);
            assert_eq!(fill.b, 128);
        } else {
            panic!("Expected Rect node");
        }
    }

    #[test]
    fn script_renders_3d_mesh_and_chop() {
        let script = r##"
            fn render(ctx, props) {
                let frame = ctx.frame.to_float();
                let osc = lfo(frame, ctx.fps, 1.0, 50.0);
                let smoothed = lag(0.0, osc, 0.033, 0.1);

                let output = scene();
                output.cube_3d(320.0, 180.0, 80.0, frame * 0.05, frame * 0.05, 0.0, "#ff8800");
                output.sphere_3d(500.0, 180.0, 40.0, frame * 0.02, frame * 0.03, "#00f0ff");
                output.torus_3d(150.0, 180.0, 50.0, 15.0, frame * 0.03, frame * 0.04, "#ff0088");
                output
            }
        "##;
        let composition = RhaiComposition::from_source("mesh3d", script).unwrap();
        let prepared = composition
            .prepare(&serde_json::json!({}), context())
            .unwrap();
        let scene = prepared.render(10).unwrap();
        assert!(
            !scene.nodes.is_empty(),
            "3D meshes should emit projected Path nodes into the scene"
        );
    }

    #[test]
    fn script_renders_extended_drawing_primitives() {
        let script = r##"
            fn render(ctx, props) {
                let output = scene();
                output.linear_gradient(0.0, 0.0, 320.0, 180.0, 45.0, "#ff0000", "#0000ff");
                output.rect_stroke(10.0, 10.0, 100.0, 50.0, "#112233", "#ffffff", 2.0, 8.0);
                output.circle_stroke(160.0, 90.0, 30.0, "#445566", "#00ffcc", 3.0);
                output.path("M 0 0 L 100 100 Z", "#aabbcc", "#ffffff", 1.5, 0.9);
                output
            }
        "##;
        let composition = RhaiComposition::from_source("extended_draw", script).unwrap();
        let prepared = composition
            .prepare(&serde_json::json!({}), context())
            .unwrap();
        let scene = prepared.render(0).unwrap();
        assert_eq!(scene.nodes.len(), 4);
        assert!(matches!(&scene.nodes[0], SceneNode::LinearGradient { .. }));
        assert!(matches!(
            &scene.nodes[1],
            SceneNode::Rect {
                stroke: Some(_),
                corner_radius,
                ..
            } if *corner_radius == 8.0
        ));
        assert!(matches!(
            &scene.nodes[2],
            SceneNode::Circle {
                stroke: Some(_),
                ..
            }
        ));
        assert!(matches!(&scene.nodes[3], SceneNode::Path { .. }));
    }
}
