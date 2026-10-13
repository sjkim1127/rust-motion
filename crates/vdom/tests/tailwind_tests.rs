use dioxus::prelude::*;
use rust_motion_rasterizer::{Color, SceneNode};
use rust_motion_vdom::{render_virtual_dom, Stylesheet};

#[test]
fn test_tailwind_flexbox_and_colors() {
    fn app() -> Element {
        rsx! {
            div {
                class: "flex flex-row items-center justify-between gap-4 p-6 bg-slate-900 rounded-xl",
                div {
                    class: "w-24 h-12 bg-red-500 rounded-lg text-white font-bold",
                    "Tag"
                }
                div {
                    class: "text-2xl font-bold text-sky-400",
                    "Dashboard"
                }
            }
        }
    }

    let mut vdom = VirtualDom::new(app);
    let stylesheet = Stylesheet::new();
    let scene = render_virtual_dom(&mut vdom, 800, 200, &stylesheet).expect("render vdom");

    // 1. Root container background and radius
    let root_bg = scene.nodes.iter().find(|node| match node {
        SceneNode::Rect {
            fill,
            corner_radius,
            ..
        } => *fill == Color::rgb(0x0f, 0x17, 0x2a) && (*corner_radius - 12.0).abs() < 0.1,
        _ => false,
    });
    assert!(
        root_bg.is_some(),
        "Root slate-900 container with rounded-xl expected"
    );

    // 2. Child 1: w-24 (96px), h-12 (48px), bg-red-500 (#ef4444), rounded-lg (8px)
    let red_badge = scene.nodes.iter().find(|node| match node {
        SceneNode::Rect {
            w,
            h,
            fill,
            corner_radius,
            ..
        } => {
            (*w - 96.0).abs() < 1.0
                && (*h - 48.0).abs() < 1.0
                && *fill == Color::rgb(0xef, 0x44, 0x44)
                && (*corner_radius - 8.0).abs() < 0.1
        }
        _ => false,
    });
    assert!(
        red_badge.is_some(),
        "Red badge with w-24, h-12, rounded-lg expected"
    );

    // 3. Child 2: text-2xl (24px), font-bold (700), text-sky-400 (#38bdf8)
    let sky_title = scene.nodes.iter().find(|node| match node {
        SceneNode::Text {
            content,
            font_size,
            font_weight,
            color,
            ..
        } => {
            content == "Dashboard"
                && (*font_size - 24.0).abs() < 0.1
                && *font_weight == 700
                && *color == Color::rgb(0x38, 0xbd, 0xf8)
        }
        _ => false,
    });
    assert!(
        sky_title.is_some(),
        "Sky-400 title with text-2xl and font-bold expected"
    );
}

#[test]
fn test_tailwind_grid_3_cols() {
    fn app() -> Element {
        rsx! {
            div {
                class: "grid grid-cols-3 gap-6 p-4",
                div { class: "h-20 bg-blue-500", "Col 1" }
                div { class: "h-20 bg-blue-500", "Col 2" }
                div { class: "h-20 bg-blue-500", "Col 3" }
            }
        }
    }

    let mut vdom = VirtualDom::new(app);
    let stylesheet = Stylesheet::new();
    let scene = render_virtual_dom(&mut vdom, 600, 200, &stylesheet).expect("render vdom");

    // Find the 3 blue column rectangles
    let blue_rects: Vec<_> = scene
        .nodes
        .iter()
        .filter_map(|node| match node {
            SceneNode::Rect { x, w, h, fill, .. } if *fill == Color::rgb(0x3b, 0x82, 0xf6) => {
                Some((*x, *w, *h))
            }
            _ => None,
        })
        .collect();

    assert_eq!(blue_rects.len(), 3, "Expected 3 grid items");

    // Check that items are laid out horizontally in 3 columns: x0 < x1 < x2
    assert!(
        blue_rects[0].0 < blue_rects[1].0,
        "Col 0 x should be less than Col 1 x"
    );
    assert!(
        blue_rects[1].0 < blue_rects[2].0,
        "Col 1 x should be less than Col 2 x"
    );

    // All columns should have equal width in 3-col grid
    let w0 = blue_rects[0].1;
    let w1 = blue_rects[1].1;
    let w2 = blue_rects[2].1;
    assert!(
        (w0 - w1).abs() < 2.0,
        "Column widths should be equal: w0={w0}, w1={w1}"
    );
    assert!(
        (w1 - w2).abs() < 2.0,
        "Column widths should be equal: w1={w1}, w2={w2}"
    );
}

#[test]
fn test_tailwind_arbitrary_brackets() {
    fn app() -> Element {
        rsx! {
            div {
                class: "w-[350px] h-[150px] bg-[#ff3366] rounded-[22px] p-[15px]",
                div {
                    class: "text-[19px] text-[#00ffcc] font-bold",
                    "Custom Styled Text"
                }
            }
        }
    }

    let mut vdom = VirtualDom::new(app);
    let stylesheet = Stylesheet::new();
    let scene = render_virtual_dom(&mut vdom, 800, 400, &stylesheet).expect("render vdom");

    let custom_box = scene.nodes.iter().find(|node| match node {
        SceneNode::Rect {
            w,
            h,
            fill,
            corner_radius,
            ..
        } => {
            (*w - 350.0).abs() < 0.1
                && (*h - 150.0).abs() < 0.1
                && *fill == Color::rgb(0xff, 0x33, 0x66)
                && (*corner_radius - 22.0).abs() < 0.1
        }
        _ => false,
    });
    assert!(
        custom_box.is_some(),
        "Custom box with [350px], [150px], [#ff3366], rounded-[22px] expected"
    );

    let custom_text = scene.nodes.iter().find(|node| match node {
        SceneNode::Text {
            font_size,
            color,
            content,
            ..
        } => {
            content == "Custom Styled Text"
                && (*font_size - 19.0).abs() < 0.1
                && *color == Color::rgb(0x00, 0xff, 0xcc)
        }
        _ => false,
    });
    assert!(
        custom_text.is_some(),
        "Custom text with text-[19px], text-[#00ffcc] expected"
    );
}

#[test]
fn test_tailwind_utility_overrides_stylesheet_class() {
    fn app() -> Element {
        rsx! {
            div {
                class: "base-card p-10 bg-emerald-500",
                "Overridden"
            }
        }
    }

    let mut vdom = VirtualDom::new(app);
    let stylesheet =
        Stylesheet::parse(".base-card { padding: 4px; background: #000000; }").unwrap();
    let scene = render_virtual_dom(&mut vdom, 400, 200, &stylesheet).expect("render vdom");

    // Background should be emerald-500 (#10b981), overriding #000000
    let emerald_box = scene.nodes.iter().find(|node| match node {
        SceneNode::Rect { fill, .. } => *fill == Color::rgb(0x10, 0xb9, 0x81),
        _ => false,
    });
    assert!(
        emerald_box.is_some(),
        "Tailwind bg-emerald-500 should override base-card background"
    );
}

#[test]
fn test_direct_apply_utility_class() {
    use rust_motion_vdom::{apply_utility_class, ResolvedStyle};
    use taffy::prelude::Display;

    let mut style = ResolvedStyle::default();
    assert!(apply_utility_class(&mut style, "flex"));
    assert_eq!(style.layout.display, Display::Flex);

    assert!(apply_utility_class(&mut style, "bg-rose-500"));
    assert_eq!(style.background, Some(Color::rgb(0xf4, 0x3f, 0x5e)));

    assert!(apply_utility_class(&mut style, "rounded-2xl"));
    assert_eq!(style.border_radius, 16.0);

    assert!(apply_utility_class(&mut style, "font-black"));
    assert_eq!(style.font_weight, 900);

    assert!(apply_utility_class(&mut style, "opacity-75"));
    assert!((style.opacity - 0.75).abs() < 0.01);

    // Unrecognized token returns false
    assert!(!apply_utility_class(&mut style, "not-a-tailwind-class-xyz"));
}
