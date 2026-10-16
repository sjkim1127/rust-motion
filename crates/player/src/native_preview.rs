//! Dioxus preview adapter for the same native scenes used during export.

use crate::player::PlayerPlaybackState;
use dioxus::prelude::*;
use rust_motion_composition::{Composition, CompositionError, NativeCompositionContext};
use rust_motion_core::use_current_frame;
use rust_motion_rasterizer::{
    AudioTrack, BlendMode, ClipRegion, Color, GradientStop, ImageFit, MaskMode, Scene, SceneFilter,
    SceneNode, SceneShadow, Transform2D,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static NEXT_SCENE_VIEW_ID: AtomicU64 = AtomicU64::new(1);

/// Cloneable handle for passing a native composition through Dioxus props.
#[derive(Clone)]
pub struct CompositionHandle(Arc<dyn Composition>);

impl CompositionHandle {
    pub fn new<C>(composition: C) -> Self
    where
        C: Composition + 'static,
    {
        Self(Arc::new(composition))
    }

    pub fn id(&self) -> &str {
        self.0.id()
    }

    /// Prepare and render one frame through the shared export contract.
    pub fn render_frame(
        &self,
        frame: u32,
        input_props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Scene, CompositionError> {
        self.0.prepare(input_props, context)?.render(frame)
    }
}

impl PartialEq for CompositionHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct NativeCompositionPreviewProps {
    pub composition: CompositionHandle,
    #[props(default = Value::Object(Default::default()))]
    pub input_props: Value,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_in_frames: u32,
}

/// Renders the current Player frame using the same `Composition -> Scene` path as export.
#[component]
pub fn NativeCompositionPreview(props: NativeCompositionPreviewProps) -> Element {
    let frame = use_current_frame();
    let playback = try_consume_context::<Signal<PlayerPlaybackState>>()
        .map(|state| *state.read())
        .unwrap_or(PlayerPlaybackState {
            playing: false,
            seek_revision: 0,
        });
    let context = NativeCompositionContext {
        width: props.width,
        height: props.height,
        fps: props.fps,
        duration_in_frames: props.duration_in_frames,
    };

    match props
        .composition
        .render_frame(frame, &props.input_props, context)
    {
        Ok(scene) => rsx! {
            SceneView {
                scene,
                width: props.width,
                height: props.height,
                frame,
                fps: props.fps,
                playing: playback.playing,
                seek_revision: playback.seek_revision,
            }
        },
        Err(error) => rsx! {
            div {
                class: "rust-motion-native-preview-error",
                style: "box-sizing: border-box; width: 100%; height: 100%; padding: 24px; background: #220d16; color: #fecdd3; font-family: monospace; white-space: pre-wrap;",
                "Failed to preview composition '{props.composition.id()}' at frame {frame}: {error}"
            }
        },
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct SceneViewProps {
    pub scene: Scene,
    pub width: u32,
    pub height: u32,
    #[props(default = 0)]
    pub frame: u32,
    #[props(default = 30.0)]
    pub fps: f64,
    #[props(default = false)]
    pub playing: bool,
    #[props(default = 0)]
    pub seek_revision: u64,
}

/// Converts the shared native scene graph to SVG for Dioxus web/desktop preview.
#[component]
pub fn SceneView(props: SceneViewProps) -> Element {
    let instance_id = use_hook(|| NEXT_SCENE_VIEW_ID.fetch_add(1, Ordering::Relaxed));
    let media_status = use_context_provider(|| Signal::new(MediaPreviewStatus::default()));
    let view_box = format!("0 0 {} {}", props.width, props.height);
    let root_id = format!("rust-motion-scene-{instance_id}");
    let safe_fps = safe_fps(props.fps);
    let timeline_time = f64::from(props.frame) / safe_fps;
    let scene_signature = media_signature(&props.scene);
    let status = media_status.read();
    let errors = status.errors.values().cloned().collect::<Vec<_>>();
    let buffering_count = status.buffering.len();
    let media_event_revision = status.revision;
    drop(status);
    let sync_script = build_media_sync_script(
        &root_id,
        props.playing,
        props.seek_revision,
        scene_signature,
        safe_fps,
        media_event_revision,
    );
    use_effect(use_reactive!(|(sync_script,)| {
        let _ = dioxus::document::eval(&sync_script);
    }));

    rsx! {
        div {
            class: "rust-motion-scene-view-container",
            width: "100%",
            height: "100%",
            style: "position: relative; display: block; overflow: hidden;",
            svg {
                id: root_id,
                class: "rust-motion-scene-view",
                view_box,
                width: "100%",
                height: "100%",
                style: "display: block; overflow: hidden;",
                xmlns: "http://www.w3.org/2000/svg",
                for (index, node) in props.scene.nodes.into_iter().enumerate() {
                    SceneNodeView {
                        key: "root-{index}",
                        node,
                        node_path: format!("scene-{instance_id}-root-{index}"),
                        timeline_time,
                        inherited_volume: 1.0,
                    }
                }
            }
            if buffering_count > 0 && props.playing {
                div {
                    class: "rust-motion-media-buffering",
                    style: "position: absolute; left: 12px; bottom: 12px; padding: 5px 8px; border-radius: 5px; background: rgba(0,0,0,0.72); color: white; font: 12px sans-serif; pointer-events: none;",
                    "Buffering {buffering_count} media source(s)…"
                }
            }
            if !errors.is_empty() {
                div {
                    class: "rust-motion-media-error",
                    style: "position: absolute; inset: auto 12px 12px 12px; padding: 8px 10px; border-radius: 5px; background: rgba(69,10,10,0.92); color: #fecaca; font: 12px monospace; white-space: pre-wrap; pointer-events: none;",
                    for error in errors {
                        div { "{error}" }
                    }
                }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct SceneNodeViewProps {
    node: SceneNode,
    node_path: String,
    timeline_time: f64,
    inherited_volume: f64,
}

#[component]
fn SceneNodeView(props: SceneNodeViewProps) -> Element {
    let media_status = use_context::<Signal<MediaPreviewStatus>>();
    match props.node {
        SceneNode::Rect {
            x,
            y,
            w,
            h,
            fill,
            stroke,
            stroke_width,
            corner_radius,
        } => {
            let fill = color_css(fill);
            let stroke = optional_color_css(stroke);
            rsx! {
                rect {
                    x,
                    y,
                    width: w,
                    height: h,
                    rx: corner_radius,
                    ry: corner_radius,
                    fill,
                    stroke,
                    stroke_width,
                }
            }
        }
        SceneNode::Circle {
            cx,
            cy,
            r,
            fill,
            stroke,
            stroke_width,
        } => {
            let fill = color_css(fill);
            let stroke = optional_color_css(stroke);
            rsx! {
                circle { cx, cy, r, fill, stroke, stroke_width }
            }
        }
        SceneNode::Path {
            d,
            fill,
            stroke,
            stroke_width,
            opacity,
        } => {
            let fill = optional_color_css(fill);
            let stroke = optional_color_css(stroke);
            rsx! {
                path { d, fill, stroke, stroke_width, opacity }
            }
        }
        SceneNode::Text {
            x,
            y,
            content,
            font_size,
            color,
            font_weight,
            font_sources,
        } => {
            let fill = color_css(color);
            let (font_face_css, font_family) =
                text_font_css(&props.node_path, &font_sources, font_weight);
            rsx! {
                if !font_face_css.is_empty() {
                    defs { style { "{font_face_css}" } }
                }
                text {
                    x,
                    y,
                    fill,
                    font_size,
                    font_weight,
                    font_family,
                    dominant_baseline: "alphabetic",
                    "{content}"
                }
            }
        }
        SceneNode::Image {
            src,
            x,
            y,
            w,
            h,
            fit,
            opacity,
        } => {
            let preserve_aspect_ratio = image_preserve_aspect_ratio(fit);
            rsx! {
                image {
                    x,
                    y,
                    width: w,
                    height: h,
                    href: src,
                    opacity,
                    preserve_aspect_ratio,
                }
            }
        }
        SceneNode::Video {
            src,
            time,
            looped,
            x,
            y,
            w,
            h,
            fit,
            opacity,
        } => {
            let media_id = format!("rust-motion-media-{}", props.node_path);
            let style = format!(
                "display:block;width:100%;height:100%;object-fit:{};opacity:{};",
                media_object_fit(fit),
                opacity.clamp(0.0, 1.0)
            );
            let loading_id = media_id.clone();
            let waiting_id = media_id.clone();
            let ready_id = media_id.clone();
            let error_id = media_id.clone();
            let error_src = src.clone();
            let loading_status = media_status;
            let waiting_status = media_status;
            let ready_status = media_status;
            let error_status = media_status;
            rsx! {
                foreignObject { x, y, width: w, height: h,
                    video {
                        id: media_id,
                        src,
                        style,
                        muted: true,
                        r#loop: looped,
                        preload: "auto",
                        "data-rust-motion-media": "video",
                        "data-rust-motion-time": time,
                        "data-rust-motion-active": "true",
                        "data-rust-motion-volume": "0",
                        "data-rust-motion-rate": "1",
                        "data-rust-motion-loop": looped,
                        onloadstart: move |_| mark_media_loading(loading_status, &loading_id),
                        onwaiting: move |_| mark_media_loading(waiting_status, &waiting_id),
                        oncanplay: move |_| mark_media_ready(ready_status, &ready_id),
                        onerror: move |_| mark_media_error(error_status, &error_id, &error_src),
                    }
                }
            }
        }
        SceneNode::Audio { track } => {
            let media_id = format!("rust-motion-media-{}", props.node_path);
            let (time, active) = audio_preview_timing(&track, props.timeline_time);
            let volume = (track.volume * props.inherited_volume).clamp(0.0, 1.0);
            let loading_id = media_id.clone();
            let waiting_id = media_id.clone();
            let ready_id = media_id.clone();
            let error_id = media_id.clone();
            let error_src = track.src.clone();
            let loading_status = media_status;
            let waiting_status = media_status;
            let ready_status = media_status;
            let error_status = media_status;
            rsx! {
                foreignObject { x: 0, y: 0, width: 1, height: 1,
                    audio {
                        id: media_id,
                        src: track.src,
                        preload: "auto",
                        style: "display:none;",
                        "data-rust-motion-media": "audio",
                        "data-rust-motion-time": time,
                        "data-rust-motion-active": active,
                        "data-rust-motion-volume": volume,
                        "data-rust-motion-rate": track.playback_rate,
                        "data-rust-motion-loop": track.looped,
                        onloadstart: move |_| mark_media_loading(loading_status, &loading_id),
                        onwaiting: move |_| mark_media_loading(waiting_status, &waiting_id),
                        oncanplay: move |_| mark_media_ready(ready_status, &ready_id),
                        onerror: move |_| mark_media_error(error_status, &error_id, &error_src),
                    }
                }
            }
        }
        SceneNode::LinearGradient {
            x,
            y,
            w,
            h,
            angle_deg,
            stops,
        } => {
            let gradient_id = format!("rust-motion-linear-{}", props.node_path);
            let center_x = x + w / 2.0;
            let center_y = y + h / 2.0;
            let gradient_transform = format!("rotate({angle_deg} {center_x} {center_y})");
            let fill = format!("url(#{gradient_id})");
            rsx! {
                defs {
                    linearGradient {
                        id: gradient_id,
                        gradient_units: "userSpaceOnUse",
                        x1: x,
                        y1: center_y,
                        x2: x + w,
                        y2: center_y,
                        gradient_transform,
                        for (index, stop) in stops.into_iter().enumerate() {
                            GradientStopView { key: "stop-{index}", stop }
                        }
                    }
                }
                rect { x, y, width: w, height: h, fill }
            }
        }
        SceneNode::RadialGradient { cx, cy, r, stops } => {
            let gradient_id = format!("rust-motion-radial-{}", props.node_path);
            let fill = format!("url(#{gradient_id})");
            rsx! {
                defs {
                    radialGradient {
                        id: gradient_id,
                        gradient_units: "userSpaceOnUse",
                        cx,
                        cy,
                        r,
                        for (index, stop) in stops.into_iter().enumerate() {
                            GradientStopView { key: "stop-{index}", stop }
                        }
                    }
                }
                circle { cx, cy, r, fill }
            }
        }
        SceneNode::Group {
            transform,
            opacity,
            children,
        } => {
            let transform = transform_css(transform);
            let inherited_volume = props.inherited_volume * f64::from(opacity);
            rsx! {
                g {
                    transform,
                    opacity,
                    for (index, node) in children.into_iter().enumerate() {
                        SceneNodeView {
                            key: "child-{index}",
                            node,
                            node_path: format!("{}-{index}", props.node_path),
                            timeline_time: props.timeline_time,
                            inherited_volume,
                        }
                    }
                }
            }
        }
        SceneNode::Layer {
            opacity,
            blend_mode,
            clip,
            mask,
            mask_mode,
            filters,
            shadow,
            children,
        } => {
            let clip_id = format!("rust-motion-clip-{}", props.node_path);
            let mask_id = format!("rust-motion-mask-{}", props.node_path);
            let mut style = format!("mix-blend-mode:{};", blend_mode_css(blend_mode));
            if clip.is_some() {
                style.push_str(&format!("clip-path:url(#{clip_id});"));
            }
            if mask.is_some() {
                style.push_str(&format!("mask:url(#{mask_id});"));
            }
            let filter = layer_filter_css(&filters, shadow.as_ref());
            if !filter.is_empty() {
                style.push_str(&format!("filter:{filter};"));
            }
            let inherited_volume = props.inherited_volume * f64::from(opacity);
            rsx! {
                if let Some(clip) = clip {
                    LayerClipView { id: clip_id, clip }
                }
                if let Some(nodes) = mask {
                    LayerMaskView {
                        id: mask_id,
                        mode: mask_mode,
                        nodes,
                        node_path: format!("{}-mask", props.node_path),
                        timeline_time: props.timeline_time,
                        inherited_volume,
                    }
                }
                g {
                    opacity,
                    style,
                    for (index, node) in children.into_iter().enumerate() {
                        SceneNodeView {
                            key: "layer-child-{index}",
                            node,
                            node_path: format!("{}-{index}", props.node_path),
                            timeline_time: props.timeline_time,
                            inherited_volume,
                        }
                    }
                }
            }
        }
        SceneNode::Gif {
            src,
            x,
            y,
            w,
            h,
            fit,
            opacity,
            ..
        } => {
            let preserve_aspect_ratio = image_preserve_aspect_ratio(fit);
            rsx! {
                image {
                    x,
                    y,
                    width: w,
                    height: h,
                    href: src,
                    opacity,
                    preserve_aspect_ratio,
                }
            }
        }
        SceneNode::Emoji {
            emoji,
            x,
            y,
            size,
            opacity,
        } => {
            let font_size = size;
            let style = format!("font-size: {}px; user-select: none;", font_size);
            rsx! {
                text {
                    x,
                    y: y + font_size * 0.85,
                    opacity,
                    style: "{style}",
                    "{emoji}"
                }
            }
        }
        SceneNode::Lottie {
            x,
            y,
            w,
            h,
            opacity,
            ..
        } => {
            rsx! {
                rect {
                    x,
                    y,
                    width: w,
                    height: h,
                    fill: "none",
                    stroke: "rgba(0, 240, 255, 0.4)",
                    stroke_width: 1.0,
                    opacity,
                }
            }
        }
        SceneNode::AudioVisualizer {
            x,
            y,
            width,
            height,
            color,
            opacity,
            ..
        } => {
            let fill_str = format!(
                "rgba({}, {}, {}, {:.2})",
                color.r,
                color.g,
                color.b,
                color.a as f32 / 255.0
            );
            rsx! {
                rect {
                    x,
                    y,
                    width,
                    height,
                    fill: "none",
                    stroke: "{fill_str}",
                    stroke_width: 1.5,
                    stroke_dasharray: "4 2",
                    opacity,
                }
            }
        }
        rust_motion_rasterizer::SceneNode::Shader {
            x,
            y,
            w,
            h,
            opacity,
            ..
        } => {
            rsx! {
                rect {
                    x,
                    y,
                    width: w,
                    height: h,
                    fill: "#6366f1",
                    opacity,
                }
            }
        }
        SceneNode::NullObject {
            transform,
            opacity,
            children,
            ..
        } => {
            let transform_css = transform_css(transform);
            rsx! {
                g {
                    transform: "{transform_css}",
                    opacity,
                    for (child_index, child) in children.iter().enumerate() {
                        SceneNodeView {
                            node: child.clone(),
                            node_path: format!("{}.{}", props.node_path, child_index),
                            timeline_time: props.timeline_time,
                            inherited_volume: props.inherited_volume,
                        }
                    }
                }
            }
        }
        SceneNode::TrackMatte { target, .. } => {
            rsx! {
                g {
                    SceneNodeView {
                        node: (*target).clone(),
                        node_path: format!("{}.target", props.node_path),
                        timeline_time: props.timeline_time,
                        inherited_volume: props.inherited_volume,
                    }
                }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct LayerClipViewProps {
    id: String,
    clip: ClipRegion,
}

#[component]
fn LayerClipView(props: LayerClipViewProps) -> Element {
    rsx! {
        defs {
            clipPath {
                id: props.id,
                clip_path_units: "userSpaceOnUse",
                match props.clip {
                    ClipRegion::Rect { x, y, w, h, corner_radius } => rsx! {
                        rect { x, y, width: w, height: h, rx: corner_radius, ry: corner_radius }
                    },
                    ClipRegion::Path { d } => rsx! { path { d } },
                }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct LayerMaskViewProps {
    id: String,
    mode: MaskMode,
    nodes: Vec<SceneNode>,
    node_path: String,
    timeline_time: f64,
    inherited_volume: f64,
}

#[component]
fn LayerMaskView(props: LayerMaskViewProps) -> Element {
    let style = match props.mode {
        MaskMode::Alpha | MaskMode::AlphaInverted => "mask-type:alpha;",
        MaskMode::Luminance | MaskMode::LuminanceInverted => "mask-type:luminance;",
    };
    rsx! {
        defs {
            mask {
                id: props.id,
                mask_units: "userSpaceOnUse",
                x: "0%",
                y: "0%",
                width: "100%",
                height: "100%",
                style,
                for (index, node) in props.nodes.into_iter().enumerate() {
                    SceneNodeView {
                        key: "mask-child-{index}",
                        node,
                        node_path: format!("{}-{index}", props.node_path),
                        timeline_time: props.timeline_time,
                        inherited_volume: props.inherited_volume,
                    }
                }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct GradientStopViewProps {
    stop: GradientStop,
}

#[component]
fn GradientStopView(props: GradientStopViewProps) -> Element {
    let offset = format!("{}%", props.stop.position.clamp(0.0, 1.0) * 100.0);
    let stop_color = color_css(props.stop.color);
    rsx! { stop { offset, stop_color } }
}

fn color_css(color: Color) -> String {
    format!(
        "rgba({}, {}, {}, {:.6})",
        color.r,
        color.g,
        color.b,
        color.a as f64 / 255.0
    )
}

fn text_font_css(node_path: &str, sources: &[String], font_weight: u16) -> (String, String) {
    if sources.is_empty() {
        return (String::new(), "sans-serif".into());
    }
    let mut declarations = String::new();
    let mut families = Vec::with_capacity(sources.len() + 1);
    for (index, source) in sources.iter().enumerate() {
        let family = format!("rust-motion-font-{node_path}-{index}");
        let url = browser_font_url(source)
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace(['\r', '\n'], "");
        declarations.push_str(&format!(
            "@font-face{{font-family:'{family}';src:url('{url}');font-weight:{font_weight};}}"
        ));
        families.push(format!("'{family}'"));
    }
    families.push("sans-serif".into());
    (declarations, families.join(", "))
}

fn browser_font_url(source: &str) -> String {
    if source.contains("://") {
        source.into()
    } else if source.starts_with('/') {
        format!("file://{source}")
    } else if source.as_bytes().get(1) == Some(&b':') {
        format!("file:///{}", source.replace('\\', "/"))
    } else {
        source.into()
    }
}

fn blend_mode_css(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Normal => "normal",
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
        BlendMode::Overlay => "overlay",
        BlendMode::Darken => "darken",
        BlendMode::Lighten => "lighten",
        BlendMode::ColorDodge => "color-dodge",
        BlendMode::ColorBurn => "color-burn",
        BlendMode::HardLight => "hard-light",
        BlendMode::SoftLight => "soft-light",
        BlendMode::Difference => "difference",
        BlendMode::Exclusion => "exclusion",
    }
}

fn layer_filter_css(filters: &[SceneFilter], shadow: Option<&SceneShadow>) -> String {
    let mut values = filters
        .iter()
        .map(|filter| match filter {
            SceneFilter::Blur { sigma } => format!("blur({}px)", sigma.max(0.0)),
            SceneFilter::Brightness { amount } => format!("brightness({})", amount.max(0.0)),
            SceneFilter::Grayscale { amount } => {
                format!("grayscale({})", amount.clamp(0.0, 1.0))
            }
            SceneFilter::Opacity { amount } => format!("opacity({})", amount.clamp(0.0, 1.0)),
            SceneFilter::Contrast { factor } => format!("contrast({})", factor.max(0.0)),
            SceneFilter::Saturation { factor } => format!("saturate({})", factor.max(0.0)),
            SceneFilter::HueRotate { degrees } => format!("hue-rotate({}deg)", degrees),
            SceneFilter::Invert { amount } => format!("invert({})", amount.clamp(0.0, 1.0)),
            SceneFilter::ChromaticAberration { offset_x, offset_y, .. } => {
                format!("drop-shadow({}px {}px 0 rgba(255,0,0,0.5)) drop-shadow({}px {}px 0 rgba(0,0,255,0.5))", -offset_x, -offset_y, offset_x, offset_y)
            }
            SceneFilter::Vignette { .. } => String::new(),
            SceneFilter::Tint { .. } => String::new(),
            SceneFilter::Duotone { .. } => String::new(),
            SceneFilter::ColorGrading { contrast, saturation, .. } => {
                format!("contrast({}) saturate({})", contrast.max(0.0), saturation.max(0.0))
            }
            SceneFilter::ColorKey { .. } => String::new(),
            SceneFilter::CameraMotionBlur { shutter_angle, samples: _, shutter_phase: _ } => {
                // CSS doesn't have a native shutter blur; approximate with motion blur
                // using a small horizontal blur proportional to shutter angle.
                let blur_px = (shutter_angle.clamp(0.0, 360.0) / 360.0 * 4.0).max(0.0);
                if blur_px < 0.1 { String::new() } else { format!("blur({blur_px:.1}px)") }
            }
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    if let Some(shadow) = shadow {
        values.push(format!(
            "drop-shadow({}px {}px {}px {})",
            shadow.offset_x,
            shadow.offset_y,
            shadow.blur_sigma.max(0.0),
            color_css(shadow.color)
        ));
    }
    values.join(" ")
}

fn optional_color_css(color: Option<Color>) -> String {
    color.map(color_css).unwrap_or_else(|| "none".to_string())
}

fn transform_css(transform: Transform2D) -> String {
    format!(
        "translate({} {}) scale({} {}) rotate({})",
        transform.tx, transform.ty, transform.scale_x, transform.scale_y, transform.rotate_deg
    )
}

fn image_preserve_aspect_ratio(fit: ImageFit) -> &'static str {
    match fit {
        ImageFit::Cover => "xMidYMid slice",
        ImageFit::Contain | ImageFit::ScaleDown => "xMidYMid meet",
        ImageFit::Fill => "none",
        ImageFit::None => "xMidYMid meet",
    }
}

fn media_object_fit(fit: ImageFit) -> &'static str {
    match fit {
        ImageFit::Cover => "cover",
        ImageFit::Contain => "contain",
        ImageFit::Fill => "fill",
        ImageFit::None => "none",
        ImageFit::ScaleDown => "scale-down",
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct MediaPreviewStatus {
    buffering: HashSet<String>,
    errors: HashMap<String, String>,
    revision: u64,
}

fn mark_media_loading(mut status: Signal<MediaPreviewStatus>, id: &str) {
    let mut status = status.write();
    status.buffering.insert(id.to_string());
    status.errors.remove(id);
    status.revision = status.revision.wrapping_add(1);
}

fn mark_media_ready(mut status: Signal<MediaPreviewStatus>, id: &str) {
    let mut status = status.write();
    status.buffering.remove(id);
    status.errors.remove(id);
    status.revision = status.revision.wrapping_add(1);
}

fn mark_media_error(mut status: Signal<MediaPreviewStatus>, id: &str, src: &str) {
    let mut status = status.write();
    status.buffering.remove(id);
    status
        .errors
        .insert(id.to_string(), format!("Failed to load media: {src}"));
    status.revision = status.revision.wrapping_add(1);
}

fn safe_fps(fps: f64) -> f64 {
    if fps.is_finite() && fps > 0.0 {
        fps
    } else {
        30.0
    }
}

fn audio_preview_timing(track: &AudioTrack, timeline_time: f64) -> (f64, bool) {
    let elapsed = timeline_time - track.timeline_start;
    let active = elapsed >= 0.0
        && track
            .duration
            .is_none_or(|duration| elapsed < duration.max(0.0));
    let playback_rate = if track.playback_rate.is_finite() && track.playback_rate > 0.0 {
        track.playback_rate
    } else {
        1.0
    };
    let source_time = track.start_from.max(0.0) + elapsed.max(0.0) * playback_rate;
    (source_time, active)
}

fn media_signature(scene: &Scene) -> u64 {
    fn hash_nodes(nodes: &[SceneNode], inherited_volume: f64, hasher: &mut DefaultHasher) {
        for node in nodes {
            match node {
                SceneNode::Video { src, looped, .. } => {
                    "video".hash(hasher);
                    src.hash(hasher);
                    looped.hash(hasher);
                }
                SceneNode::Audio { track } => {
                    "audio".hash(hasher);
                    track.src.hash(hasher);
                    track.start_from.to_bits().hash(hasher);
                    track.timeline_start.to_bits().hash(hasher);
                    track.duration.map(f64::to_bits).hash(hasher);
                    (track.volume * inherited_volume).to_bits().hash(hasher);
                    track.playback_rate.to_bits().hash(hasher);
                    track.looped.hash(hasher);
                }
                SceneNode::Group {
                    opacity, children, ..
                } => {
                    "group".hash(hasher);
                    opacity.to_bits().hash(hasher);
                    hash_nodes(children, inherited_volume * f64::from(*opacity), hasher);
                }
                SceneNode::Layer {
                    opacity, children, ..
                } => {
                    "layer".hash(hasher);
                    opacity.to_bits().hash(hasher);
                    hash_nodes(children, inherited_volume * f64::from(*opacity), hasher);
                }
                _ => {}
            }
        }
    }

    let mut hasher = DefaultHasher::new();
    hash_nodes(&scene.nodes, 1.0, &mut hasher);
    hasher.finish()
}

fn build_media_sync_script(
    root_id: &str,
    playing: bool,
    seek_revision: u64,
    scene_signature: u64,
    fps: f64,
    media_event_revision: u64,
) -> String {
    let root_id = serde_json::to_string(root_id).expect("element ID is JSON serializable");
    let revision = serde_json::to_string(&format!("{seek_revision}-{scene_signature}"))
        .expect("revision is JSON serializable");
    let drift_threshold = (1.5 / safe_fps(fps)).max(0.04);
    format!(
        r#"(() => {{
  const root = document.getElementById({root_id});
  if (!root) return;
  const playing = {playing};
  const revision = {revision};
  const mediaEventRevision = {media_event_revision};
  void mediaEventRevision;
  const driftThreshold = {drift_threshold:.9};
  for (const media of root.querySelectorAll('[data-rust-motion-media]')) {{
    let expected = Number(media.dataset.rust-motionTime);
    if (!Number.isFinite(expected)) continue;
    const active = media.dataset.rust-motionActive === 'true';
    const looped = media.dataset.rust-motionLoop === 'true';
    const rate = Number(media.dataset.rust-motionRate);
    const volume = Number(media.dataset.rust-motionVolume);
    media.loop = looped;
    media.playbackRate = Number.isFinite(rate) && rate > 0 ? rate : 1;
    media.volume = Number.isFinite(volume) ? Math.min(1, Math.max(0, volume)) : 1;
    if (Number.isFinite(media.duration) && media.duration > 0) {{
      expected = looped
        ? ((expected % media.duration) + media.duration) % media.duration
        : Math.min(expected, Math.max(0, media.duration - 0.001));
    }}
    const hardSeek = !playing || media.dataset.rust-motionRevision !== revision;
    const drifted = !Number.isFinite(media.currentTime)
      || Math.abs(media.currentTime - expected) > driftThreshold;
    if ((hardSeek || drifted) && Number.isFinite(expected)) {{
      try {{ media.currentTime = Math.max(0, expected); }} catch (_) {{}}
    }}
    media.dataset.rust-motionRevision = revision;
    if (playing && active && media.readyState >= 2) {{
      const promise = media.play();
      if (promise) promise.catch((error) => {{
        media.dataset.rust-motionPlayError = String(error);
      }});
    }} else {{
      media.pause();
    }}
  }}
}})();"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_motion_composition::HelloWorldComposition;

    #[test]
    fn composition_handle_renders_the_shared_scene() {
        let composition = CompositionHandle::new(HelloWorldComposition);
        let context = NativeCompositionContext {
            width: 320,
            height: 180,
            fps: 30.0,
            duration_in_frames: 30,
        };
        let scene = composition
            .render_frame(10, &serde_json::json!({"title": "Shared preview"}), context)
            .unwrap();

        assert!(scene.nodes.iter().any(|node| matches!(
            node,
            SceneNode::Text { content, .. } if content == "Shared preview"
        )));
    }

    #[test]
    fn composition_handles_compare_by_identity() {
        let first = CompositionHandle::new(HelloWorldComposition);
        let clone = first.clone();
        let second = CompositionHandle::new(HelloWorldComposition);

        assert!(first == clone);
        assert!(first != second);
    }

    #[test]
    fn image_fit_maps_to_svg_aspect_ratio() {
        assert_eq!(
            image_preserve_aspect_ratio(ImageFit::Cover),
            "xMidYMid slice"
        );
        assert_eq!(
            image_preserve_aspect_ratio(ImageFit::Contain),
            "xMidYMid meet"
        );
        assert_eq!(image_preserve_aspect_ratio(ImageFit::Fill), "none");
        assert_eq!(media_object_fit(ImageFit::ScaleDown), "scale-down");
    }

    #[test]
    fn layer_effects_map_to_svg_css() {
        assert_eq!(blend_mode_css(BlendMode::ColorDodge), "color-dodge");
        let filter = layer_filter_css(
            &[
                SceneFilter::Blur { sigma: 2.5 },
                SceneFilter::Grayscale { amount: 0.75 },
            ],
            Some(&SceneShadow {
                offset_x: 4.0,
                offset_y: 5.0,
                blur_sigma: 6.0,
                color: Color::rgba(10, 20, 30, 128),
            }),
        );

        assert!(filter.contains("blur(2.5px)"));
        assert!(filter.contains("grayscale(0.75)"));
        assert!(filter.contains("drop-shadow(4px 5px 6px rgba(10, 20, 30,"));
    }

    #[test]
    fn test_all_10_filter_variants_in_layer_filter_css() {
        let filters = vec![
            SceneFilter::Blur { sigma: 4.0 },
            SceneFilter::Brightness { amount: 1.2 },
            SceneFilter::Grayscale { amount: 0.5 },
            SceneFilter::Opacity { amount: 0.8 },
            SceneFilter::ChromaticAberration {
                offset_x: 5.0,
                offset_y: 0.0,
                angle_rad: 0.0,
            },
            SceneFilter::Vignette {
                offset: 0.5,
                darkness: 0.8,
                roundness: 0.5,
            },
            SceneFilter::Contrast { factor: 1.5 },
            SceneFilter::Saturation { factor: 2.0 },
            SceneFilter::HueRotate { degrees: 90.0 },
            SceneFilter::Invert { amount: 1.0 },
            SceneFilter::Tint {
                color: [255, 0, 0, 128],
                amount: 0.5,
            },
            SceneFilter::Duotone {
                primary: [0, 0, 0, 255],
                secondary: [255, 255, 255, 255],
            },
            SceneFilter::ColorGrading {
                contrast: 1.2,
                saturation: 1.4,
                gamma: 1.0,
                tint: None,
            },
            SceneFilter::ColorKey {
                key_color: [0, 255, 0, 255],
                similarity: 0.4,
                smoothness: 0.1,
                spill_suppression: 0.2,
            },
        ];

        let css = layer_filter_css(&filters, None);
        assert!(css.contains("blur(4px)"));
        assert!(css.contains("brightness(1.2)"));
        assert!(css.contains("grayscale(0.5)"));
        assert!(css.contains("opacity(0.8)"));
        assert!(css.contains("drop-shadow"));
        assert!(css.contains("contrast(1.5)"));
        assert!(css.contains("saturate(2)"));
        assert!(css.contains("hue-rotate(90deg)"));
        assert!(css.contains("invert(1)"));
    }

    #[test]
    fn explicit_fonts_map_to_ordered_font_faces() {
        let (declarations, family) = text_font_css(
            "node-2",
            &["/assets/Primary.ttf".into(), "fonts/Fallback.otf".into()],
            700,
        );

        assert!(declarations.contains("file:///assets/Primary.ttf"));
        assert!(declarations.contains("url('fonts/Fallback.otf')"));
        assert!(declarations.contains("font-weight:700"));
        assert_eq!(
            family,
            "'rust-motion-font-node-2-0', 'rust-motion-font-node-2-1', sans-serif"
        );
    }

    #[test]
    fn audio_preview_respects_timeline_trim_and_rate() {
        let track = AudioTrack {
            src: "voice.wav".into(),
            start_from: 1.5,
            timeline_start: 2.0,
            duration: Some(3.0),
            volume: 0.75,
            playback_rate: 1.25,
            looped: false,
            volume_keyframes: Vec::new(),
        };

        assert_eq!(audio_preview_timing(&track, 1.0), (1.5, false));
        assert_eq!(audio_preview_timing(&track, 3.0), (2.75, true));
        assert_eq!(audio_preview_timing(&track, 5.0), (5.25, false));
    }

    #[test]
    fn media_signature_ignores_video_time_but_tracks_source_changes() {
        let make_scene = |src: &str, time: f64| Scene {
            nodes: vec![SceneNode::Video {
                src: src.into(),
                time,
                looped: false,
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
                fit: ImageFit::Cover,
                opacity: 1.0,
            }],
        };

        assert_eq!(
            media_signature(&make_scene("clip.mp4", 0.0)),
            media_signature(&make_scene("clip.mp4", 1.0))
        );
        assert_ne!(
            media_signature(&make_scene("clip.mp4", 0.0)),
            media_signature(&make_scene("other.mp4", 0.0))
        );
    }

    #[test]
    fn sync_script_contains_drift_and_transport_controls() {
        let script = build_media_sync_script("scene-1", true, 7, 11, 30.0, 2);
        assert!(script.contains("document.getElementById(\"scene-1\")"));
        assert!(script.contains("Math.abs(media.currentTime - expected)"));
        assert!(script.contains("media.playbackRate"));
        assert!(script.contains("media.volume"));
        assert!(script.contains("media.play()"));
    }
}
