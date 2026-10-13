//! `<Emoji>` component for color emojis.

use dioxus::prelude::*;

/// Props for the `<Emoji>` component.
#[derive(Props, Clone, PartialEq)]
pub struct EmojiProps {
    /// Emoji string (e.g. "🔥", "🚀", "💡").
    pub name: String,
    /// Bounding size in pixels.
    #[props(default = 48.0)]
    pub size: f64,
    /// Additional CSS styles.
    #[props(default)]
    pub style: String,
    /// Additional CSS classes.
    #[props(default)]
    pub class: String,
}

/// Renders a full-color emoji.
#[component]
pub fn Emoji(props: EmojiProps) -> Element {
    let combined_style = format!(
        "font-size: {}px; width: {}px; height: {}px; display: inline-flex; align-items: center; justify-content: center; line-height: 1; {}",
        props.size, props.size, props.size, props.style
    );

    rsx! {
        span {
            class: "{props.class}",
            style: "{combined_style}",
            "data-rust-motion-emoji": "{props.name}",
            "{props.name}"
        }
    }
}
