//! Dioxus VirtualDom and CSS to native Rust Motion Scene conversion.
//!
//! This crate implements a renderer for Dioxus 0.6 mutations. It preserves a
//! lightweight DOM tree, resolves a practical CSS subset, computes block,
//! Flexbox, and Grid layout with Taffy, and emits the shared native [`Scene`].

mod composition;
pub mod css;
mod dom;
mod scene;
pub mod tailwind;

pub use composition::{VdomComposition, VdomFactory};
pub use css::{CssError, ResolvedStyle, Stylesheet};
pub use dom::{NativeDom, NativeDomError};
pub use tailwind::apply_utility_class;

use dioxus_core::VirtualDom;
use rust_motion_rasterizer::Scene;

/// Rebuild a Dioxus VirtualDom and convert it to a native Scene.
pub fn render_virtual_dom(
    virtual_dom: &mut VirtualDom,
    width: u32,
    height: u32,
    stylesheet: &Stylesheet,
) -> Result<Scene, NativeDomError> {
    let mut native_dom = NativeDom::new();
    virtual_dom.rebuild(&mut native_dom);
    native_dom.to_scene(width, height, stylesheet)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dioxus::prelude::*;
    use rust_motion_composition::{NativeComposition, NativeCompositionContext};
    use rust_motion_rasterizer::{BlendMode, Color, ImageFit, SceneFilter, SceneNode};
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn card() -> Element {
        let title = "Native Dioxus";
        rsx! {
            div { class: "card", id: "hero", "{title}" }
        }
    }

    #[test]
    fn renders_dioxus_elements_css_and_dynamic_text() {
        let mut virtual_dom = VirtualDom::new(card);
        let stylesheet = Stylesheet::parse(
            ".card { display: flex; width: 320px; height: 100px; padding: 12px; background: #112233; color: white; font-size: 20px; }",
        )
        .unwrap();

        let scene = render_virtual_dom(&mut virtual_dom, 640, 360, &stylesheet).unwrap();

        assert!(scene.nodes.iter().any(|node| matches!(
            node,
            SceneNode::Rect {
                w,
                h,
                fill,
                ..
            } if (*w - 320.0).abs() < 0.01
                && (*h - 100.0).abs() < 0.01
                && *fill == Color::rgb(0x11, 0x22, 0x33)
        )));
        assert!(
            scene.nodes.iter().any(|node| matches!(
                node,
                SceneNode::Text { content, color, .. }
                    if content == "Native Dioxus" && *color == Color::WHITE
            )),
            "{scene:#?}"
        );
    }

    fn media() -> Element {
        rsx! {
            img {
                src: "assets/poster.png",
                width: "160",
                height: "90",
                style: "object-fit: contain; opacity: 0.5",
            }
            audio {
                src: "assets/voice.wav",
                "data-start-from": "1.25",
                "data-timeline-start": "2",
                "data-duration": "3.5",
                "data-volume": "0.4",
                "playback-rate": "1.5",
                r#loop: true,
            }
            video {
                src: "assets/clip.mp4",
                width: "320",
                height: "180",
                "data-time": "4.25",
                r#loop: true,
            }
            canvas {
                "data-src": "assets/animated.png",
                width: "64",
                height: "32",
                style: "object-fit: contain",
            }
        }
    }

    #[test]
    fn maps_html_media_attributes_to_scene_nodes() {
        let mut virtual_dom = VirtualDom::new(media);
        let scene = render_virtual_dom(&mut virtual_dom, 640, 360, &Stylesheet::new()).unwrap();

        assert!(matches!(
            &scene.nodes[0],
            SceneNode::Group { opacity, children, .. }
                if (*opacity - 0.5).abs() < f32::EPSILON
                    && matches!(
                        &children[0],
                        SceneNode::Image { src, w, h, fit: ImageFit::Contain, .. }
                            if src == "assets/poster.png"
                                && (*w - 160.0).abs() < 0.01
                                && (*h - 90.0).abs() < 0.01
                    )
        ));
        assert!(matches!(
            &scene.nodes[1],
            SceneNode::Audio { track }
                if track.src == "assets/voice.wav"
                    && (track.start_from - 1.25).abs() < 0.01
                    && (track.timeline_start - 2.0).abs() < 0.01
                    && (track.duration.unwrap() - 3.5).abs() < 0.01
                    && (track.volume - 0.4).abs() < 0.01
                    && (track.playback_rate - 1.5).abs() < 0.01
                    && track.looped
        ));
        assert!(matches!(
            &scene.nodes[2],
            SceneNode::Video { src, time, looped, .. }
                if src == "assets/clip.mp4"
                    && (*time - 4.25).abs() < 0.01
                    && *looped
        ));
        assert!(matches!(
            &scene.nodes[3],
            SceneNode::Image { src, w, h, fit: ImageFit::Contain, .. }
                if src == "assets/animated.png" && (*w - 64.0).abs() < 0.01 && (*h - 32.0).abs() < 0.01
        ));
    }

    fn styled_layer() -> Element {
        rsx! {
            div {
                style: "width: 120px; height: 80px; background: red; filter: grayscale(50%); box-shadow: 2px 3px 4px black; mix-blend-mode: screen;",
            }
        }
    }

    #[test]
    fn maps_css_effects_to_a_shared_native_layer() {
        let mut virtual_dom = VirtualDom::new(styled_layer);
        let scene = render_virtual_dom(&mut virtual_dom, 640, 360, &Stylesheet::new()).unwrap();
        let SceneNode::Layer {
            blend_mode,
            filters,
            shadow,
            ..
        } = &scene.nodes[0]
        else {
            panic!("expected CSS effects to emit a native layer: {scene:#?}");
        };
        assert_eq!(*blend_mode, BlendMode::Screen);
        assert!(
            matches!(filters.as_slice(), [SceneFilter::Grayscale { amount }] if (*amount - 0.5).abs() < f32::EPSILON)
        );
        assert!(shadow.is_some());
    }

    fn grid() -> Element {
        rsx! {
            div { class: "grid",
                div { class: "first" }
                div { class: "second" }
            }
        }
    }

    #[test]
    fn applies_grid_track_layout_to_scene_coordinates() {
        let mut virtual_dom = VirtualDom::new(grid);
        let stylesheet = Stylesheet::parse(
            ".grid { display: grid; width: 220px; height: 50px; grid-template-columns: 100px 100px; column-gap: 20px; } .first { background: #ff0000; } .second { background: #0000ff; }",
        )
        .unwrap();

        let scene = render_virtual_dom(&mut virtual_dom, 640, 360, &stylesheet).unwrap();
        assert!(scene.nodes.iter().any(|node| matches!(
            node,
            SceneNode::Rect { x, w, fill, .. }
                if x.abs() < 0.01 && (*w - 100.0).abs() < 0.01 && *fill == Color::rgb(255, 0, 0)
        )));
        assert!(scene.nodes.iter().any(|node| matches!(
            node,
            SceneNode::Rect { x, w, fill, .. }
                if (*x - 120.0).abs() < 0.01
                    && (*w - 100.0).abs() < 0.01
                    && *fill == Color::rgb(0, 0, 255)
        )));
    }

    fn composition_view() -> Element {
        rsx! { div { class: "frame", "Frame" } }
    }

    #[test]
    fn vdom_composition_uses_native_composition_contract() {
        let composition = VdomComposition::new(
            "DioxusFrame",
            |_frame, _props: &serde_json::Value, _context| VirtualDom::new(composition_view),
        )
        .with_css(".frame { width: 200px; height: 80px; background: #ff0000; }")
        .unwrap();
        let context = NativeCompositionContext {
            width: 640,
            height: 360,
            fps: 30.0,
            duration_in_frames: 60,
        };

        let scene = composition.render(12, &json!({}), context).unwrap();

        assert!(scene.nodes.iter().any(
            |node| matches!(node, SceneNode::Rect { fill, .. } if *fill == Color::rgb(255, 0, 0))
        ));
    }

    static SHOW_DYNAMIC_TEXT: AtomicBool = AtomicBool::new(false);

    fn dynamic_tree() -> Element {
        let content = SHOW_DYNAMIC_TEXT
            .load(Ordering::SeqCst)
            .then_some("Now visible");
        rsx! { div { width: "240", height: "60", {content} } }
    }

    #[test]
    fn applies_incremental_vdom_replacement_mutations() {
        SHOW_DYNAMIC_TEXT.store(false, Ordering::SeqCst);
        let mut virtual_dom = VirtualDom::new(dynamic_tree);
        let mut native_dom = NativeDom::new();
        virtual_dom.rebuild(&mut native_dom);
        assert_eq!(native_dom.node_count(), 3);

        SHOW_DYNAMIC_TEXT.store(true, Ordering::SeqCst);
        virtual_dom.mark_dirty(ScopeId::APP);
        virtual_dom.render_immediate(&mut native_dom);
        let scene = native_dom.to_scene(640, 360, &Stylesheet::new()).unwrap();

        assert!(scene.nodes.iter().any(
            |node| matches!(node, SceneNode::Text { content, .. } if content == "Now visible")
        ));
    }
}
