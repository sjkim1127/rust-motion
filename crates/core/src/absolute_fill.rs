//! `<AbsoluteFill>` component — full-size absolute-positioned container.
//!
//! Equivalent to Remotion's `<AbsoluteFill>`.
//!
//! Renders a `div` with `position: absolute; top: 0; left: 0; right: 0; bottom: 0`
//! so that it covers its nearest positioned ancestor entirely.
//!
//! # Example
//! ```rust,ignore
//! use rust_motion_core::AbsoluteFill;
//!
//! fn Background() -> Element {
//!     rsx! {
//!         AbsoluteFill {
//!             style: "background-color: #0a0a23;",
//!         }
//!     }
//! }
//! ```

use dioxus::prelude::*;

/// Props for the `<AbsoluteFill>` component.
#[derive(Props, Clone, PartialEq)]
pub struct AbsoluteFillProps {
    /// Additional CSS to merge with the absolute fill base styles.
    #[props(default)]
    pub style: Option<String>,

    /// Additional CSS classes.
    #[props(default)]
    pub class: Option<String>,

    /// Child elements.
    pub children: Element,
}

/// A full-size absolute-positioned container.
///
/// Equivalent to Remotion's `<AbsoluteFill>`.
#[component]
pub fn AbsoluteFill(props: AbsoluteFillProps) -> Element {
    let base = "position: absolute; top: 0; left: 0; right: 0; bottom: 0;";
    let style = if let Some(extra) = &props.style {
        format!("{base} {extra}")
    } else {
        base.to_string()
    };

    rsx! {
        div {
            style: "{style}",
            class: props.class.unwrap_or_default(),
            {props.children}
        }
    }
}
