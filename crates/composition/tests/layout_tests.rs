use rust_motion_composition::layout::prelude::*;
use rust_motion_rasterizer::{Color, Scene, SceneNode};

const RED: Color = Color {
    r: 255,
    g: 0,
    b: 0,
    a: 255,
};
const GREEN: Color = Color {
    r: 0,
    g: 255,
    b: 0,
    a: 255,
};
const BLUE: Color = Color {
    r: 0,
    g: 0,
    b: 255,
    a: 255,
};

#[test]
fn test_flexbox_row_equal_distribution_and_gap() {
    let mut scene = Scene::new();

    // 3 boxes in a row with 20px gap inside a 640px wide container
    let mut row = SceneFlex::row(20.0, JustifyContent::FLEX_START, AlignItems::CENTER)
        .size(640.0, 200.0)
        .padding(10.0);

    row.push_node(
        SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
            fill: RED,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        },
        None, // auto-infer width: 100, height: 50
    );

    row.push_node(
        SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
            fill: GREEN,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        },
        None,
    );

    row.push_node(
        SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
            fill: BLUE,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        },
        None,
    );

    row.compute_and_emit(640.0, 200.0, &mut scene).unwrap();

    assert_eq!(scene.nodes.len(), 3);

    // Box 1: x = padding(10) = 10
    if let SceneNode::Rect {
        x, y, w, h, fill, ..
    } = &scene.nodes[0]
    {
        assert_eq!(*x, 10.0);
        assert_eq!(*y, 75.0); // centered vertically: (200 - 50) / 2 = 75
        assert_eq!(*w, 100.0);
        assert_eq!(*h, 50.0);
        assert_eq!(*fill, RED);
    } else {
        panic!("expected Rect node");
    }

    // Box 2: x = 10 + 100 + gap(20) = 130
    if let SceneNode::Rect {
        x, y, w, h, fill, ..
    } = &scene.nodes[1]
    {
        assert_eq!(*x, 130.0);
        assert_eq!(*y, 75.0);
        assert_eq!(*w, 100.0);
        assert_eq!(*h, 50.0);
        assert_eq!(*fill, GREEN);
    } else {
        panic!("expected Rect node");
    }

    // Box 3: x = 130 + 100 + gap(20) = 250
    if let SceneNode::Rect {
        x, y, w, h, fill, ..
    } = &scene.nodes[2]
    {
        assert_eq!(*x, 250.0);
        assert_eq!(*y, 75.0);
        assert_eq!(*w, 100.0);
        assert_eq!(*h, 50.0);
        assert_eq!(*fill, BLUE);
    } else {
        panic!("expected Rect node");
    }
}

#[test]
fn test_flexbox_column_vertical_stacking() {
    let mut scene = Scene::new();

    let mut col = SceneFlex::column(15.0, JustifyContent::FLEX_START, AlignItems::FLEX_START)
        .size(300.0, 500.0)
        .padding(20.0);

    for i in 0..3 {
        col.push_node(
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 40.0,
                fill: Color::rgba(i as u8 * 50, 100, 200, 255),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 4.0,
            },
            None,
        );
    }

    col.compute_and_emit(300.0, 500.0, &mut scene).unwrap();

    assert_eq!(scene.nodes.len(), 3);

    // Item 0: y = 20
    if let SceneNode::Rect { x, y, h, .. } = scene.nodes[0] {
        assert_eq!(x, 20.0);
        assert_eq!(y, 20.0);
        assert_eq!(h, 40.0);
    }
    // Item 1: y = 20 + 40 + 15 = 75
    if let SceneNode::Rect { x, y, h, .. } = scene.nodes[1] {
        assert_eq!(x, 20.0);
        assert_eq!(y, 75.0);
        assert_eq!(h, 40.0);
    }
    // Item 2: y = 75 + 40 + 15 = 130
    if let SceneNode::Rect { x, y, h, .. } = scene.nodes[2] {
        assert_eq!(x, 20.0);
        assert_eq!(y, 130.0);
        assert_eq!(h, 40.0);
    }
}

#[test]
fn test_css_grid_2x2_placement() {
    let mut scene = Scene::new();

    // 2 columns, 10px gap inside 210px container -> columns are 100px each
    let mut grid = SceneGrid::columns(2, 10.0).size(210.0, 210.0);

    for _ in 0..4 {
        grid.push_node(
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
                fill: Color::WHITE,
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            None,
        );
    }

    grid.compute_and_emit(210.0, 210.0, &mut scene).unwrap();

    assert_eq!(scene.nodes.len(), 4);

    // Cell (0, 0): (0, 0)
    if let SceneNode::Rect { x, y, .. } = scene.nodes[0] {
        assert_eq!(x, 0.0);
        assert_eq!(y, 0.0);
    }
    // Cell (0, 1): (110, 0)
    if let SceneNode::Rect { x, y, .. } = scene.nodes[1] {
        assert_eq!(x, 110.0);
        assert_eq!(y, 0.0);
    }
    // Cell (1, 0): (0, 110)
    if let SceneNode::Rect { x, y, .. } = scene.nodes[2] {
        assert_eq!(x, 0.0);
        assert_eq!(y, 110.0);
    }
    // Cell (1, 1): (110, 110)
    if let SceneNode::Rect { x, y, .. } = scene.nodes[3] {
        assert_eq!(x, 110.0);
        assert_eq!(y, 110.0);
    }
}

#[test]
fn test_nested_flex_in_flex() {
    let mut scene = Scene::new();

    let mut outer_row = SceneFlex::row(30.0, JustifyContent::FLEX_START, AlignItems::FLEX_START)
        .size(800.0, 400.0)
        .padding(20.0);

    let mut inner_col = SceneFlex::column(10.0, JustifyContent::FLEX_START, AlignItems::FLEX_START)
        .size(200.0, 300.0);

    inner_col.push_node(
        SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 180.0,
            h: 50.0,
            fill: Color::BLACK,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        },
        None,
    );

    inner_col.push_node(
        SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 180.0,
            h: 50.0,
            fill: Color::WHITE,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        },
        None,
    );

    outer_row.push_nested(inner_col);

    // Second column directly in outer row
    outer_row.push_node(
        SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 200.0,
            fill: BLUE,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        },
        None,
    );

    outer_row
        .compute_and_emit(800.0, 400.0, &mut scene)
        .unwrap();

    assert_eq!(scene.nodes.len(), 3);

    // Nested col item 1
    if let SceneNode::Rect { x, y, .. } = scene.nodes[0] {
        assert_eq!(x, 20.0);
        assert_eq!(y, 20.0);
    }
    // Nested col item 2
    if let SceneNode::Rect { x, y, .. } = scene.nodes[1] {
        assert_eq!(x, 20.0);
        assert_eq!(y, 80.0); // 20 + 50 + 10
    }
    // Outer second item: x = padding(20) + inner_col_w(200) + gap(30) = 250
    if let SceneNode::Rect { x, y, .. } = scene.nodes[2] {
        assert_eq!(x, 250.0);
        assert_eq!(y, 20.0);
    }
}
