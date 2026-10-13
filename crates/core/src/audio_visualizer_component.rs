use dioxus::prelude::*;
pub use rust_motion_rasterizer::scene::VisualizerStyle;

/// Props for the `<AudioVisualizer>` component.
#[derive(Props, Clone, PartialEq)]
pub struct AudioVisualizerProps {
    /// Source path or `file://` URI to audio file (MP3, WAV, AAC, M4A).
    pub src: String,
    /// X coordinate on canvas in pixels (default 0.0).
    #[props(default = 0.0)]
    pub x: f32,
    /// Y coordinate on canvas in pixels (default 0.0).
    #[props(default = 0.0)]
    pub y: f32,
    /// Width of the visualizer bounding box in pixels (default 400.0).
    #[props(default = 400.0)]
    pub width: f32,
    /// Height of the visualizer bounding box in pixels (default 150.0).
    #[props(default = 150.0)]
    pub height: f32,
    /// Hex or rgba color string (default `#00E5FF`).
    #[props(default = "#00E5FF".to_string())]
    pub color: String,
    /// Visual style: `Bars`, `Wave`, or `Radial`.
    #[props(default)]
    pub style: VisualizerStyle,
    /// Layer opacity in `0.0..=1.0` (default 1.0).
    #[props(default = 1.0)]
    pub opacity: f32,
}

/// An audio frequency or waveform visualizer synchronized with the composition timeline.
///
/// Supports 3 styles:
/// - `VisualizerStyle::Bars`: Spectrum bar graph (with gap, radius, mirror options)
/// - `VisualizerStyle::Wave`: Continuous smooth oscilloscope wave (with stroke and filled options)
/// - `VisualizerStyle::Radial`: Circular radiating bars (ideal for podcast speaker avatars)
#[component]
pub fn AudioVisualizer(props: AudioVisualizerProps) -> Element {
    let style_str = format!(
        "position: absolute; left: {}px; top: {}px; width: {}px; height: {}px; opacity: {}; pointer-events: none;",
        props.x, props.y, props.width, props.height, props.opacity
    );

    rsx! {
        div {
            style: "{style_str}",
            "data-audio-viz": "{props.src}",
            "data-color": "{props.color}",
        }
    }
}
