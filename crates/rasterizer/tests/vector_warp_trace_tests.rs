use rust_motion_rasterizer::{
    doc::{Path, PathOp, Subpath},
    geom::{Point, Rect},
    vector::{map::map_path, path_coverage, trace::trace_mask},
    Color, FrameConfig, RasterizerBackend, Scene, SceneNode, TinySkiaBackend,
};

#[test]
fn test_vector_boolean_operations_and_rendering() {
    // Subpath 1: Outer rectangle 10..90 x 10..90
    let sub1 = Subpath::polygon(&[(10.0, 10.0), (90.0, 10.0), (90.0, 90.0), (10.0, 90.0)])
        .with_op(PathOp::Combine);
    // Subpath 2: Inner rectangle 30..70 x 30..70 subtracted (cutting a hole)
    let sub2 = Subpath::polygon(&[(30.0, 30.0), (70.0, 30.0), (70.0, 70.0), (30.0, 70.0)])
        .with_op(PathOp::Subtract);

    let path = Path::new(vec![sub1, sub2]);

    // Check analytic coverage via rasterizer
    let rect = Rect::new(0, 0, 100, 100);
    let cov = path_coverage(&path, rect);

    // Centre of hole (50, 50) should have 0 coverage
    assert_eq!(cov[50 * 100 + 50], 0.0, "Subtracted interior must be 0 coverage");
    // Outer wall (20, 20) should have 1.0 coverage
    assert_eq!(cov[20 * 100 + 20], 1.0, "Outer body must have 1.0 coverage");

    // Test SceneNode::VectorShape rendering via TinySkiaBackend
    let backend = TinySkiaBackend::new();
    let config = FrameConfig::new(100, 100, 0, 30.0);
    let scene = Scene {
        nodes: vec![SceneNode::VectorShape {
            path,
            fill: Some(Color::rgb(255, 0, 0)),
            stroke_color: None,
            stroke_width: 0.0,
            opacity: 1.0,
        }],
    };

    let frame = backend.render_frame(&scene, &config).expect("render failed");
    // Pixel (20, 20): Red > 200, Alpha > 200
    let px_wall = frame.get_pixel(20, 20);
    assert!(px_wall[0] > 200 && px_wall[3] > 200);

    // Pixel (50, 50): Transparent hole Alpha == 0
    let px_hole = frame.get_pixel(50, 50);
    assert_eq!(px_hole[3], 0, "Hole must be fully transparent in rendered frame");
}

#[test]
fn test_non_affine_mesh_warp_of_vector_path() {
    // Create a square path 0..100 x 0..100
    let original = Path::new(vec![Subpath::polygon(&[
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 100.0),
        (0.0, 100.0),
    ])]);

    // Apply a non-affine perspective & parabolic bend warp
    // Map function f: non-linear warp expanding bottom corners and bending top
    let warped = map_path(
        &original,
        |p: Point| {
            let nx = p.x / 100.0;
            let ny = p.y / 100.0;
            // Perspective trapezoid + parabolic arch
            let wx = p.x * (0.8 + 0.4 * ny);
            let wy = p.y - 15.0 * (4.0 * nx * (1.0 - nx));
            Point::new(wx, wy)
        },
        0.05,
    );

    // Warped path must have subdivided segments along the curved edge
    let total_knots: usize = warped.subpaths.iter().map(|s| s.knots.len()).sum();
    assert!(
        total_knots > 4,
        "Curved non-affine warp should subdivide segments with smooth Béziers (got {} knots)",
        total_knots
    );

    let (_x0, y0, x1, _y1) = warped.control_bounds().expect("bounds");
    assert!(y0 < 0.0, "Parabolic arch must push top edge upward (y0 = {})", y0);
    assert!(x1 > 100.0, "Perspective flare must widen bottom edge (x1 = {})", x1);
}

#[test]
fn test_contour_auto_tracing_make_work_path() {
    // Generate a 60x60 raster mask with a 30x30 solid block in the center
    let w = 60;
    let h = 60;
    let mut mask = vec![0.0f32; w * h];
    for y in 15..45 {
        for x in 15..45 {
            mask[y * w + x] = 1.0;
        }
    }

    // Auto-trace to convert raster silhouette into cubic Bézier vector path ('Make Work Path')
    let rect = Rect::new(0, 0, w as i32, h as i32);
    let traced_path = trace_mask(&mask, rect, 0.0, 0.5);

    assert!(!traced_path.subpaths.is_empty(), "Tracing must produce at least one subpath");
    assert!(traced_path.subpaths[0].closed, "Traced contour must be closed");

    // Check bounds of traced path closely match the 15..45 rectangle (±1px tolerance)
    let (tx0, ty0, tx1, ty1) = traced_path.control_bounds().expect("traced bounds");
    assert!((tx0 - 15.0).abs() <= 1.5, "Left edge should match 15.0");
    assert!((ty0 - 15.0).abs() <= 1.5, "Top edge should match 15.0");
    assert!((tx1 - 45.0).abs() <= 1.5, "Right edge should match 45.0");
    assert!((ty1 - 45.0).abs() <= 1.5, "Bottom edge should match 45.0");
}
