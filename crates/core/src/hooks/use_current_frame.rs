//! `use_current_frame` hook — returns the current render frame.
//!
//! Equivalent to Remotion's `useCurrentFrame()`.
//!
//! # Usage
//! ```rust,ignore
//! use rust_motion_core::hooks::use_current_frame;
//!
//! fn MyComponent() -> Element {
//!     let frame = use_current_frame();
//!     rsx! { div { "Frame: {frame}" } }
//! }
//! ```

use crate::timeline::context::TimelineContext;
use dioxus::prelude::*;

/// Returns the current frame number for this component.
///
/// The frame is relative to the component's position in the timeline
/// (i.e., already offset by any enclosing `<Sequence>`).
///
/// # Panics
///
/// Panics if called outside a `<Composition>` context.
pub fn use_current_frame() -> u32 {
    let timeline = use_context::<Signal<TimelineContext>>();
    let snapshot = timeline.read();
    snapshot.frame
}
