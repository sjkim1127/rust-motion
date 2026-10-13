//! `<Freeze>` component — pauses all children at a fixed frame.
//!
//! Equivalent to Remotion's `<Freeze>`.
//!
//! When rendered, provides a `TimelineContext` that is frozen at `frame`
//! regardless of the actual playback position.
//!
//! # Example
//! ```rust,ignore
//! use rust_motion_core::Freeze;
//!
//! fn FrozenSection() -> Element {
//!     rsx! {
//!         // Always shows frame 10 — useful for poster frames
//!         Freeze { frame: 10,
//!             MyVideo {}
//!         }
//!     }
//! }
//! ```

use crate::timeline::context::TimelineContext;
use dioxus::prelude::*;

/// Props for `<Freeze>`.
#[derive(Props, Clone, PartialEq)]
pub struct FreezeProps {
    /// The frame number to freeze all children at.
    pub frame: u32,
    pub children: Element,
}

/// Freezes all descendant components at the given `frame`.
#[component]
pub fn Freeze(props: FreezeProps) -> Element {
    let frozen = TimelineContext::new(props.frame);
    let mut timeline = use_context_provider(|| Signal::new(frozen.clone()));
    if *timeline.peek() != frozen {
        timeline.set(frozen);
    }
    rsx! { {props.children} }
}
