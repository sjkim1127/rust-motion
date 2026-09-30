//! `<Series>` and `<SeriesSequence>` — sequential time-chained composition layout.
//!
//! Matches Remotion's `<Series>`.
//!
//! Automatically chains child sequences one after another without requiring manual calculation
//! of `from` offsets. Supports negative or positive `offset` for overlaps and transitions.

use crate::sequence::SequenceLayout;
use crate::timeline::context::TimelineContext;
use dioxus::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Coordinator passed via Dioxus context down to child `SeriesSequence` elements.
#[derive(Clone, Debug)]
pub struct SeriesCoordinator {
    pub parent_frame: u32,
    pub cursor: Arc<AtomicU32>,
}

impl PartialEq for SeriesCoordinator {
    fn eq(&self, other: &Self) -> bool {
        self.parent_frame == other.parent_frame && Arc::ptr_eq(&self.cursor, &other.cursor)
    }
}

/// Props for `<Series>`.
#[derive(Props, Clone, PartialEq)]
pub struct SeriesProps {
    /// Layout mode for the container div.
    #[props(default)]
    pub layout: SequenceLayout,
    /// Child `<SeriesSequence>` components.
    pub children: Element,
}

/// A container that chains child `<SeriesSequence>` components sequentially in time.
#[component]
pub fn Series(props: SeriesProps) -> Element {
    let parent_signal = use_context::<Signal<TimelineContext>>();
    let parent_ctx = parent_signal.read().clone();
    let parent_frame = parent_ctx.frame;

    // Reset coordinator for this frame's traversal
    let coordinator = SeriesCoordinator {
        parent_frame,
        cursor: Arc::new(AtomicU32::new(0)),
    };

    let style = match props.layout {
        SequenceLayout::AbsoluteFill => "position: absolute; top: 0; left: 0; right: 0; bottom: 0;",
        SequenceLayout::None => "",
    };

    rsx! {
        div {
            style: "{style}",
            SeriesCoordinatorProvider {
                coordinator,
                children: props.children,
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct SeriesCoordinatorProviderProps {
    coordinator: SeriesCoordinator,
    children: Element,
}

#[component]
fn SeriesCoordinatorProvider(props: SeriesCoordinatorProviderProps) -> Element {
    let mut coord_sig = use_context_provider(|| Signal::new(props.coordinator.clone()));
    coord_sig.set(props.coordinator);
    rsx! { {props.children} }
}

/// Props for `<SeriesSequence>`.
#[derive(Props, Clone, PartialEq)]
pub struct SeriesSequenceProps {
    /// How many frames this segment lasts.
    pub duration_in_frames: u32,

    /// Frame offset relative to the preceding sequence end (can be negative for cross-fading/overlapping).
    #[props(default = 0)]
    pub offset: i32,

    /// Layout mode for this segment.
    #[props(default)]
    pub layout: SequenceLayout,

    pub children: Element,
}

fn offset_frame(prev_cursor: u32, offset: i32) -> u32 {
    if offset < 0 {
        prev_cursor.saturating_sub(offset.unsigned_abs())
    } else {
        prev_cursor.saturating_add(offset as u32)
    }
}

/// A single segment inside a `<Series>`.
#[component]
pub fn SeriesSequence(props: SeriesSequenceProps) -> Element {
    let coord_sig = use_context::<Signal<SeriesCoordinator>>();
    let coord = coord_sig.read().clone();
    let parent_frame = coord.parent_frame;

    // Determine current start frame
    let prev_cursor = coord.cursor.load(Ordering::SeqCst);
    let start_frame = offset_frame(prev_cursor, props.offset);

    let duration = props.duration_in_frames;
    let next_cursor = start_frame.saturating_add(duration);
    coord.cursor.store(next_cursor, Ordering::SeqCst);

    let end_frame = start_frame.saturating_add(duration);
    let is_active = parent_frame >= start_frame && parent_frame < end_frame;

    if !is_active {
        return rsx! {};
    }

    let parent_signal = use_context::<Signal<TimelineContext>>();
    let parent_ctx = parent_signal.read().clone();
    let child_ctx = TimelineContext::offset_from(&parent_ctx, start_frame);

    let style = match props.layout {
        SequenceLayout::AbsoluteFill => "position: absolute; top: 0; left: 0; right: 0; bottom: 0;",
        SequenceLayout::None => "",
    };

    rsx! {
        div {
            style: "{style}",
            SeriesSequenceInner {
                ctx: child_ctx,
                children: props.children,
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct SeriesSequenceInnerProps {
    ctx: TimelineContext,
    children: Element,
}

#[component]
fn SeriesSequenceInner(props: SeriesSequenceInnerProps) -> Element {
    let mut timeline = use_context_provider(|| Signal::new(props.ctx.clone()));
    if *timeline.peek() != props.ctx {
        timeline.set(props.ctx);
    }
    rsx! { {props.children} }
}

#[cfg(test)]
mod tests {
    use super::offset_frame;

    #[test]
    fn offset_frame_handles_signed_boundaries_without_overflow() {
        assert_eq!(offset_frame(30, i32::MIN), 0);
        assert_eq!(offset_frame(u32::MAX, i32::MIN), u32::MAX - (1 << 31));
        assert_eq!(offset_frame(1, i32::MAX), 1 + i32::MAX as u32);
    }

    #[test]
    fn offset_frame_saturates_when_negative_offset_exceeds_cursor() {
        assert_eq!(offset_frame(5, -6), 0);
    }
}
