//! Shared native composition contract and built-in composition registry.

pub mod layout;
pub mod mesh3d;
pub mod safe_area;
mod scene_emitter;
mod time_events;

pub use layout::{LayoutBox, LayoutChild, SceneFlex, SceneGrid};
pub use mesh3d::{Mesh3DPrimitive, SceneMesh3D};
pub use safe_area::{
    fit_text, get_safe_area_insets, measure_text_approx, Platform, SafeAreaInsets,
};
pub use scene_emitter::{
    FlipDirection, LinearWipeDirection, SceneEmitter, SceneEmitterComposition, SceneFrameContext,
    SceneFreeze, SceneGroup, SceneLayer, SceneLinearGradient, SceneLoop, SceneRect, SceneSequence,
    SceneSeries, SceneSeriesEntry, SceneStack, SceneText, SceneTextBlock, SceneTrail,
    SceneTrailOpacity, SceneTransitionSeries, TransitionKind, TransitionTiming,
};
pub use time_events::{TimeEventError, TimeEventSchedule};

use dioxuscut_animation::{spring_with_options, SpringConfig, SpringOptions};
use dioxuscut_rasterizer::{
    AudioTrack, BlendMode, Color, GradientStop, MaskMode, Scene, SceneFilter, SceneNode,
    SceneShadow, Transform2D,
};
use serde_json::Value;
use std::collections::BTreeMap;
use thiserror::Error;

/// Immutable render parameters supplied to every native composition frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeCompositionContext {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_in_frames: u32,
}

impl NativeCompositionContext {
    /// Normalized timeline progress in the inclusive range `0.0..=1.0`.
    pub fn progress(self, frame: u32) -> f32 {
        let last_frame = self.duration_in_frames.saturating_sub(1).max(1);
        (frame.min(last_frame) as f32 / last_frame as f32).clamp(0.0, 1.0)
    }
}

/// Errors produced while preparing or rendering a composition.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CompositionError {
    #[error("Failed to prepare composition: {0}")]
    Prepare(String),
    #[error("Failed to render frame {frame}: {reason}")]
    Render { frame: u32, reason: String },
}

impl CompositionError {
    pub fn render(frame: u32, reason: impl Into<String>) -> Self {
        Self::Render {
            frame,
            reason: reason.into(),
        }
    }
}

/// A composition instance prepared once for a complete render job.
///
/// Implementations may cache parsed input, compiled scripts, and other
/// immutable state here. `render` can be called concurrently for different
/// frames.
pub trait PreparedComposition: Send + Sync {
    fn render(&self, frame: u32) -> Result<Scene, CompositionError>;

    /// Return audio tracks for the complete render, when the composition has
    /// an explicit timeline-level audio contract.
    ///
    /// `None` asks the host to derive tracks from rendered scenes. The CLI
    /// scans the complete frame range before encoding so it can find tracks
    /// first emitted after frame zero; this requires an extra render pass.
    /// Implementations that can describe the complete audio timeline should
    /// return `Some`, including tracks whose timing changes over time, to
    /// avoid that scan.
    fn audio_tracks(&self) -> Result<Option<Vec<AudioTrack>>, CompositionError> {
        Ok(None)
    }
}

/// General composition contract used by the registry.
pub trait Composition: Send + Sync {
    fn id(&self) -> &str;

    fn prepare(
        &self,
        props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Box<dyn PreparedComposition + '_>, CompositionError>;

    /// Prepare against the project's immutable named-event schedule.
    /// Existing compositions remain compatible and can opt in by overriding
    /// this method or by implementing [`NativeComposition::render_with_time_events`].
    fn prepare_with_time_events(
        &self,
        props: &Value,
        context: NativeCompositionContext,
        _events: TimeEventSchedule,
    ) -> Result<Box<dyn PreparedComposition + '_>, CompositionError> {
        self.prepare(props, context)
    }
}

/// A browser-free Rust composition that produces one rasterizer scene per frame.
///
/// Applications can implement this trait, register implementations in a
/// [`CompositionRegistry`], and call `execute_render_command_with_registry`.
pub trait NativeComposition: Send + Sync {
    fn id(&self) -> &str;

    fn render(
        &self,
        frame: u32,
        props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Scene, CompositionError>;

    /// Render with a prepared named-event schedule. The default preserves
    /// existing frame-based compositions unchanged.
    fn render_with_time_events(
        &self,
        frame: u32,
        props: &Value,
        context: NativeCompositionContext,
        _events: &TimeEventSchedule,
    ) -> Result<Scene, CompositionError> {
        self.render(frame, props, context)
    }
}

struct PreparedNativeComposition<'a, C> {
    composition: &'a C,
    props: Value,
    context: NativeCompositionContext,
    events: TimeEventSchedule,
}

impl<C> PreparedComposition for PreparedNativeComposition<'_, C>
where
    C: NativeComposition,
{
    fn render(&self, frame: u32) -> Result<Scene, CompositionError> {
        self.composition
            .render_with_time_events(frame, &self.props, self.context, &self.events)
    }
}

impl<C> Composition for C
where
    C: NativeComposition,
{
    fn id(&self) -> &str {
        NativeComposition::id(self)
    }

    fn prepare(
        &self,
        props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Box<dyn PreparedComposition + '_>, CompositionError> {
        Ok(Box::new(PreparedNativeComposition {
            composition: self,
            props: props.clone(),
            context,
            events: TimeEventSchedule::default(),
        }))
    }

    fn prepare_with_time_events(
        &self,
        props: &Value,
        context: NativeCompositionContext,
        events: TimeEventSchedule,
    ) -> Result<Box<dyn PreparedComposition + '_>, CompositionError> {
        Ok(Box::new(PreparedNativeComposition {
            composition: self,
            props: props.clone(),
            context,
            events,
        }))
    }
}

/// Errors produced while building or querying a composition registry.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CompositionRegistryError {
    #[error("Composition '{0}' is already registered")]
    Duplicate(String),
    #[error("Unknown composition '{requested}'. Available compositions: {available}")]
    Unknown {
        requested: String,
        available: String,
    },
}

/// Deterministic registry used by preview and export clients to resolve composition IDs.
#[derive(Default)]
pub struct CompositionRegistry {
    compositions: BTreeMap<String, Box<dyn Composition>>,
}

impl CompositionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<C>(&mut self, composition: C) -> Result<(), CompositionRegistryError>
    where
        C: Composition + 'static,
    {
        let id = composition.id().to_string();
        if self.compositions.contains_key(&id) {
            return Err(CompositionRegistryError::Duplicate(id));
        }
        self.compositions.insert(id, Box::new(composition));
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<&dyn Composition, CompositionRegistryError> {
        self.compositions.get(id).map(Box::as_ref).ok_or_else(|| {
            CompositionRegistryError::Unknown {
                requested: id.to_string(),
                available: self.ids().join(", "),
            }
        })
    }

    pub fn ids(&self) -> Vec<&str> {
        self.compositions.keys().map(String::as_str).collect()
    }
}

/// Registry shipped by the standalone `dioxuscut` binary.
pub fn built_in_registry() -> CompositionRegistry {
    let mut registry = CompositionRegistry::new();
    registry
        .register(HelloWorldComposition)
        .expect("built-in composition IDs must be unique");
    registry
        .register(NamedTimeEventsDemo)
        .expect("built-in composition IDs must be unique");
    registry
        .register(SpringRectsComposition)
        .expect("built-in composition IDs must be unique");
    registry
        .register(ShapesAndFiltersComposition)
        .expect("built-in composition IDs must be unique");
    registry
        .register(CyberpunkGridComposition)
        .expect("built-in composition IDs must be unique");
    registry
        .register(ComplexGradientsComposition)
        .expect("built-in composition IDs must be unique");
    registry
        .register(PodcastWaveformComposition)
        .expect("built-in composition IDs must be unique");
    registry
        .register(KaraokeCaptionsComposition)
        .expect("built-in composition IDs must be unique");
    registry
        .register(BarChartRaceComposition)
        .expect("built-in composition IDs must be unique");
    registry
        .register(CodeTerminalComposition)
        .expect("built-in composition IDs must be unique");
    registry
}

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
        let title = string_prop(props, "title", "Hello Dioxuscut");
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
            content: "🎙".to_string(),
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
            ("Dioxuscut", 0.00, 0.20),
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
        let term_title = string_prop(props, "terminal_title", "dioxuscut — bash — 80x24");

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
            content: "➜  dioxuscut git:(main) ".to_string(),
            font_size,
            color: Color::rgb(166, 227, 161),
            font_weight: 700,
            font_sources: Vec::new(),
        });

        // 4. Command typing animation
        let full_command = "cargo run --release --bin dioxuscut";
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

fn color_prop(props: &Value, key: &str, fallback: Color) -> Color {
    props
        .get(key)
        .and_then(Value::as_str)
        .and_then(Color::from_hex)
        .unwrap_or(fallback)
}

fn string_prop(props: &Value, key: &str, fallback: &str) -> String {
    props
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_rejects_duplicate_ids_and_reports_known_ids() {
        let mut registry = CompositionRegistry::new();
        registry.register(HelloWorldComposition).unwrap();
        assert_eq!(
            registry.register(HelloWorldComposition),
            Err(CompositionRegistryError::Duplicate("HelloWorld".into()))
        );

        let error = match registry.get("Missing") {
            Ok(_) => panic!("missing composition unexpectedly resolved"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            CompositionRegistryError::Unknown {
                requested: "Missing".into(),
                available: "HelloWorld".into(),
            }
        );
    }

    #[test]
    fn hello_world_uses_props_and_reaches_full_progress() {
        let context = NativeCompositionContext {
            width: 100,
            height: 100,
            fps: 30.0,
            duration_in_frames: 3,
        };
        let props = serde_json::json!({"title": "Custom title"});
        let scene = HelloWorldComposition.render(2, &props, context).unwrap();

        assert!(scene.nodes.iter().any(|node| matches!(
            node,
            SceneNode::Text { content, .. } if content == "Custom title"
        )));
        assert!(scene.nodes.iter().any(|node| matches!(
            node,
            SceneNode::Rect { w, h, .. } if *w == 100.0 && *h == 6.0
        )));
    }

    #[test]
    fn killer_templates_render_successfully_in_registry() {
        let registry = built_in_registry();
        let expected_ids = [
            "HelloWorld",
            "SpringRects",
            "ShapesAndFilters",
            "CyberpunkGrid",
            "ComplexGradients",
            "PodcastWaveform",
            "KaraokeCaptions",
            "BarChartRace",
            "CodeTerminal",
        ];
        for id in expected_ids {
            assert!(
                registry.get(id).is_ok(),
                "Composition '{}' must be registered in built_in_registry()",
                id
            );
        }

        let context = NativeCompositionContext {
            width: 960,
            height: 540,
            fps: 30.0,
            duration_in_frames: 60,
        };
        let props = serde_json::json!({});

        let spring_rects = registry.get("SpringRects").unwrap();
        let spring_scene = spring_rects
            .prepare(&props, context)
            .unwrap()
            .render(15)
            .unwrap();
        assert_eq!(spring_scene.nodes.len(), 33);

        // Test PodcastWaveform
        let podcast = registry.get("PodcastWaveform").unwrap();
        let scene = podcast
            .prepare(&props, context)
            .unwrap()
            .render(15)
            .unwrap();
        assert!(!scene.nodes.is_empty(), "PodcastWaveform must emit nodes");
        assert!(scene
            .nodes
            .iter()
            .any(|n| matches!(n, SceneNode::LinearGradient { .. })));

        // Test KaraokeCaptions
        let karaoke = registry.get("KaraokeCaptions").unwrap();
        let scene = karaoke
            .prepare(&props, context)
            .unwrap()
            .render(15)
            .unwrap();
        assert!(!scene.nodes.is_empty(), "KaraokeCaptions must emit nodes");
        assert!(scene
            .nodes
            .iter()
            .any(|n| matches!(n, SceneNode::Text { .. })));

        // Test BarChartRace
        let race = registry.get("BarChartRace").unwrap();
        let scene = race.prepare(&props, context).unwrap().render(30).unwrap();
        assert!(!scene.nodes.is_empty(), "BarChartRace must emit nodes");
        assert!(scene
            .nodes
            .iter()
            .any(|n| matches!(n, SceneNode::Rect { .. })));

        // Test CodeTerminal
        let terminal = registry.get("CodeTerminal").unwrap();
        let scene = terminal
            .prepare(&props, context)
            .unwrap()
            .render(45)
            .unwrap();
        assert!(!scene.nodes.is_empty(), "CodeTerminal must emit nodes");
        assert!(scene
            .nodes
            .iter()
            .any(|n| matches!(n, SceneNode::Circle { .. })));
    }
}
