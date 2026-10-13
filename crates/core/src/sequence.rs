//! `<Sequence>` component — time-slices a sub-tree within a composition.
//!
//! Equivalent to Remotion's `<Sequence>`.
//!
//! A `Sequence` starts rendering its children at frame `from`, and optionally
//! stops after `duration_in_frames`. Children see a local frame that starts at
//! `0` when the sequence begins.
//!
//! # Example
//! ```rust,ignore
//! use rust_motion_core::Sequence;
//!
//! fn MyVideo() -> Element {
//!     rsx! {
//!         // First 60 frames: title card
//!         Sequence { from: 0, duration_in_frames: 60,
//!             TitleCard {}
//!         }
//!         // Frames 60+: main content
//!         Sequence { from: 60,
//!             MainContent {}
//!         }
//!     }
//! }
//! ```

use crate::timeline::context::TimelineContext;
use dioxus::prelude::*;

/// Layout mode for the sequence container div.
#[derive(Clone, PartialEq, Debug, Default)]
pub enum SequenceLayout {
    /// Fill the parent absolutely (default — matches Remotion's default).
    #[default]
    AbsoluteFill,
    /// No additional styling.
    None,
}

/// Props for the `<Sequence>` component.
#[derive(Props, Clone, PartialEq)]
pub struct SequenceProps {
    /// Frame offset where this sequence starts (default: `0`).
    #[props(default = 0)]
    pub from: u32,

    /// How many frames this sequence lasts.
    /// `None` = plays until the end of the parent composition.
    #[props(default)]
    pub duration_in_frames: Option<u32>,

    /// Human-readable name shown in the studio timeline.
    #[props(default)]
    pub name: Option<String>,

    /// Layout mode for the sequence wrapper.
    #[props(default)]
    pub layout: SequenceLayout,

    /// Whether to hide the content (renders nothing).
    #[props(default = false)]
    pub hidden: bool,

    /// Children.
    pub children: Element,
}

/// A time-sliced segment of a composition.
///
/// Children only render during the active window `[from, from + duration)`.
/// Inside the children, `use_current_frame()` returns a **local** frame
/// starting at `0` when the sequence begins.
#[component]
pub fn Sequence(props: SequenceProps) -> Element {
    // Read the parent timeline context
    let parent_signal = use_context::<Signal<TimelineContext>>();
    let parent_ctx = parent_signal.read().clone();
    let parent_frame = parent_ctx.frame;

    let from = props.from;
    let end_frame = props
        .duration_in_frames
        .map(|d| from.saturating_add(d))
        .unwrap_or(u32::MAX);

    // Only render children within the active window
    let is_active = parent_frame >= from && parent_frame < end_frame;

    if props.hidden || !is_active {
        return rsx! {};
    }

    // Provide a child context with the local frame offset applied
    let child_ctx = TimelineContext::offset_from(&parent_ctx, from);

    let style = match props.layout {
        SequenceLayout::AbsoluteFill => "position: absolute; top: 0; left: 0; right: 0; bottom: 0;",
        SequenceLayout::None => "",
    };

    rsx! {
        // Inject child context so descendant hooks see the local frame
        div {
            style: "{style}",
            // Temporarily override the context for children.
            // Dioxus context is provided via use_context_provider at the call site;
            // here we wrap via a helper inner component.
            SequenceInner {
                ctx: child_ctx,
                children: props.children,
            }
        }
    }
}

/// Internal helper that injects the child TimelineContext.
#[derive(Props, Clone, PartialEq)]
struct SequenceInnerProps {
    ctx: TimelineContext,
    children: Element,
}

#[component]
fn SequenceInner(props: SequenceInnerProps) -> Element {
    let mut timeline = use_context_provider(|| Signal::new(props.ctx.clone()));
    if *timeline.peek() != props.ctx {
        timeline.set(props.ctx);
    }
    rsx! { {props.children} }
}
