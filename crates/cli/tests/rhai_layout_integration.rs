use rust_motion_cli::rhai_runtime::RhaiComposition;
use rust_motion_composition::{Composition, NativeCompositionContext};
use rust_motion_rasterizer::{Color, SceneNode};
use serde_json::json;

#[test]
fn test_rhai_script_flex_row_layout() {
    let script = r##"
        fn render(context, props) {
            let scene = scene();
            let row = scene.flex_row(#{
                gap: 24,
                justify: "center",
                align: "center",
                width: 800,
                height: 200,
                padding: 10
            });
            row.rect(100, 50, "#ff0000");
            row.round_rect(100, 50, "#00ff00", 8);
            row.text("Badge", 20, "#ffffff");
            scene.add_layout(row);
            return scene;
        }
    "##;

    let comp = RhaiComposition::from_source("test_rhai_flex", script).expect("compile script");
    let ctx = NativeCompositionContext {
        width: 800,
        height: 200,
        fps: 30.0,
        duration_in_frames: 30,
    };

    let prepared = comp.prepare(&json!({}), ctx).expect("prepare composition");
    let scene = prepared.render(0).expect("render frame 0");

    assert_eq!(scene.nodes.len(), 3);

    // First node: Rect
    match scene.nodes[0] {
        SceneNode::Rect { w, h, fill, .. } => {
            assert_eq!(w, 100.0);
            assert_eq!(h, 50.0);
            assert_eq!(
                fill,
                Color {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255
                }
            );
        }
        _ => panic!("expected Rect node at index 0"),
    }

    // Second node: RoundRect
    match scene.nodes[1] {
        SceneNode::Rect {
            corner_radius,
            fill,
            ..
        } => {
            assert_eq!(corner_radius, 8.0);
            assert_eq!(
                fill,
                Color {
                    r: 0,
                    g: 255,
                    b: 0,
                    a: 255
                }
            );
        }
        _ => panic!("expected RoundRect node at index 1"),
    }

    // Third node: Text
    match &scene.nodes[2] {
        SceneNode::Text {
            content, font_size, ..
        } => {
            assert_eq!(content, "Badge");
            assert_eq!(*font_size, 20.0);
        }
        _ => panic!("expected Text node at index 2"),
    }

    // Check that items are positioned strictly left-to-right: x0 < x1 < x2
    let x0 = match scene.nodes[0] {
        SceneNode::Rect { x, .. } => x,
        _ => 0.0,
    };
    let x1 = match scene.nodes[1] {
        SceneNode::Rect { x, .. } => x,
        _ => 0.0,
    };
    let x2 = match &scene.nodes[2] {
        SceneNode::Text { x, .. } => *x,
        _ => 0.0,
    };

    assert!(
        x0 < x1,
        "item 0 (x={x0}) should be to the left of item 1 (x={x1})"
    );
    assert!(
        x1 < x2,
        "item 1 (x={x1}) should be to the left of item 2 (x={x2})"
    );
}

#[test]
fn test_rhai_script_grid_layout() {
    let script = r##"
        fn render(context, props) {
            let scene = scene();
            let grid = scene.grid(#{
                cols: 3,
                gap: 16,
                width: 600,
                height: 400
            });
            for i in 0..6 {
                grid.rect(100, 80, "#3b82f6");
            }
            scene.add_layout(grid);
            return scene;
        }
    "##;

    let comp = RhaiComposition::from_source("test_rhai_grid", script).expect("compile script");
    let ctx = NativeCompositionContext {
        width: 600,
        height: 400,
        fps: 30.0,
        duration_in_frames: 30,
    };

    let prepared = comp.prepare(&json!({}), ctx).expect("prepare composition");
    let scene = prepared.render(0).expect("render frame 0");

    assert_eq!(scene.nodes.len(), 6);

    // Row 0 has 3 items with same y, Row 1 has 3 items with same y
    let y_row0 = match scene.nodes[0] {
        SceneNode::Rect { y, .. } => y,
        _ => 0.0,
    };
    let y_row1 = match scene.nodes[3] {
        SceneNode::Rect { y, .. } => y,
        _ => 0.0,
    };

    assert!(
        y_row0 < y_row1,
        "Row 0 y ({y_row0}) should be above Row 1 y ({y_row1})"
    );
}
