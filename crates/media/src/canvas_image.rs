//! `<CanvasImage>` — a browser-canvas-compatible animated image surface.
//!
//! Chromium hosts can draw the element with a canvas adapter. Native VDOM
//! rendering treats the same source as an image fallback, keeping the project
//! contract portable across backends.

use crate::img::ImageFit;
use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct CanvasImageProps {
    pub src: String,
    #[props(default)]
    pub width: Option<f32>,
    #[props(default)]
    pub height: Option<f32>,
    /// How the source fills the canvas (vendor default: `fill`).
    #[props(default = ImageFit::Fill)]
    pub fit: ImageFit,
    #[props(default)]
    pub class: Option<String>,
    #[props(default)]
    pub style: Option<String>,
    /// Number of browser-side source load retries (default: 2).
    #[props(default = 2)]
    pub max_retries: u32,
    /// Keep the canvas hidden until the source has loaded.
    #[props(default = false)]
    pub pause_when_loading: bool,
}

/// Render an image source on a canvas-capable surface.
///
/// The `data-rust-motion-canvas-image` marker is intentionally stable so a
/// Chromium/Three.js host can install a richer frame decoder without coupling
/// the Rust crate to a particular JavaScript canvas library.
#[component]
pub fn CanvasImage(props: CanvasImageProps) -> Element {
    let dimensions = match (props.width, props.height) {
        (Some(width), Some(height)) => format!("width: {width}px; height: {height}px;"),
        (Some(width), None) => format!("width: {width}px;"),
        (None, Some(height)) => format!("height: {height}px;"),
        (None, None) => String::new(),
    };
    let style = match &props.style {
        Some(extra) => format!("{dimensions} {extra}"),
        None => dimensions,
    };
    rsx! {
        canvas {
            class: props.class.unwrap_or_default(),
            style: "object-fit: {props.fit.as_css()}; {style}",
            "data-rust-motion-canvas-image": "true",
            "data-src": "{props.src}",
            "data-fit": "{props.fit.as_css()}",
            "data-max-retries": "{props.max_retries}",
            "data-pause-when-loading": "{props.pause_when_loading}",
        }
    }
}
