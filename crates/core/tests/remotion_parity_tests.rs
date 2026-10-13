use dioxus::prelude::*;
use dioxuscut_core::{
    random, use_current_frame, Loop, SequenceLayout, Series, SeriesSequence, TimelineContext,
};

#[test]
fn test_random_parity_with_remotion() {
    let r1 = random("hello");
    let r2 = random("hello");
    assert_eq!(r1, r2);
    assert!((0.0..1.0).contains(&r1));

    let n1 = random(100);
    let n2 = random(100);
    assert_eq!(n1, n2);
    assert!((0.0..1.0).contains(&n1));
    assert_ne!(r1, n1);
}

#[component]
fn DummyChild() -> Element {
    let frame = use_current_frame();
    rsx! {
        div { "Frame: {frame}" }
    }
}

#[test]
fn test_loop_component_renders() {
    let mut dom = VirtualDom::new_with_props(
        |props: (u32, u32)| {
            let (parent_frame, duration) = props;
            let timeline = TimelineContext::new(parent_frame);
            use_context_provider(|| Signal::new(timeline));

            rsx! {
                Loop {
                    duration_in_frames: duration,
                    times: Some(3),
                    layout: SequenceLayout::None,
                    DummyChild {}
                }
            }
        },
        (15, 10), // frame 15 with duration 10 -> local frame should be 5
    );

    dom.rebuild_in_place();
    let html = dioxus_ssr::render(&dom);
    assert!(html.contains("Frame: 5"));
}

#[test]
fn test_series_component_renders() {
    let mut dom = VirtualDom::new_with_props(
        |props: u32| {
            let parent_frame = props;
            let timeline = TimelineContext::new(parent_frame);
            use_context_provider(|| Signal::new(timeline));

            rsx! {
                Series {
                    layout: SequenceLayout::None,
                    SeriesSequence {
                        duration_in_frames: 30,
                        DummyChild {}
                    }
                    SeriesSequence {
                        duration_in_frames: 30,
                        DummyChild {}
                    }
                }
            }
        },
        45, // frame 45 is inside the second sequence (30..60), local frame should be 15
    );

    dom.rebuild_in_place();
    let html = dioxus_ssr::render(&dom);
    assert!(html.contains("Frame: 15"));
}

#[test]
fn test_series_sequence_min_offset_saturates_without_panicking() {
    let mut dom = VirtualDom::new_with_props(
        |parent_frame: u32| {
            let timeline = TimelineContext::new(parent_frame);
            use_context_provider(|| Signal::new(timeline));

            rsx! {
                Series {
                    layout: SequenceLayout::None,
                    SeriesSequence {
                        duration_in_frames: 30,
                        DummyChild {}
                    }
                    SeriesSequence {
                        duration_in_frames: 10,
                        offset: i32::MIN,
                        DummyChild {}
                    }
                }
            }
        },
        0,
    );

    dom.rebuild_in_place();
    let html = dioxus_ssr::render(&dom);
    assert_eq!(html.matches("Frame: 0").count(), 2);
}

#[test]
fn test_interpolate_colors_parity() {
    use dioxuscut_core::interpolate_colors_range;

    let frames = vec![0.0, 15.0, 30.0];
    let colors = vec!["#ff0000", "#00ff00", "#0000ff"];

    let c0 = interpolate_colors_range(0.0, &frames, &colors);
    assert_eq!(c0, "rgba(255, 0, 0, 1.0000)");

    let c15 = interpolate_colors_range(15.0, &frames, &colors);
    assert_eq!(c15, "rgba(0, 255, 0, 1.0000)");

    let c30 = interpolate_colors_range(30.0, &frames, &colors);
    assert_eq!(c30, "rgba(0, 0, 255, 1.0000)");
}
