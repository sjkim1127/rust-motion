use super::*;
use crate::mesh3d::Vec3;
use crate::scene::Color;
use tiny_skia::{Pixmap, Transform};

#[test]
fn test_scene3d_construction_and_hierarchy() {
    let mut scene = Scene3D::new()
        .with_camera(Scene3DCamera::perspective(
            Vec3::new(0.0, 100.0, 300.0),
            Vec3::new(0.0, 0.0, 0.0),
            45.0,
        ))
        .with_background(Scene3DBackground::Solid {
            color: Color::rgb(20, 24, 33),
        });

    scene.add_light(Scene3DLight::directional(
        Vec3::new(1.0, -2.0, -1.0),
        Color::WHITE,
        1.2,
    ));
    scene.add_light(Scene3DLight::point(
        Vec3::new(0.0, 50.0, 50.0),
        Color::rgb(255, 180, 100),
        2.0,
        500.0,
    ));

    let mut parent_node = Node3D::new(
        Geometry3D::Cube { size: 50.0 },
        Material3D::pbr(Color::rgb(255, 100, 50), 0.8, 0.2),
    )
    .with_id("cube_root")
    .with_transform(Transform3D::from_translation(Vec3::new(0.0, 10.0, 0.0)));

    let child_node = Node3D::new(
        Geometry3D::Sphere {
            radius: 15.0,
            rings: 12,
            sectors: 16,
        },
        Material3D::wireframe(Color::WHITE, 1.5),
    )
    .with_id("sphere_child")
    .with_transform(Transform3D::from_translation(Vec3::new(30.0, 0.0, 0.0)));

    parent_node.add_child(child_node);
    scene.add_node(parent_node);

    assert_eq!(scene.nodes.len(), 1);
    assert!(scene.find_node("cube_root").is_some());
    assert!(scene.find_node("sphere_child").is_some());
    assert!(scene.find_node("non_existent").is_none());

    let render_items = scene.collect_render_items();
    assert_eq!(render_items.len(), 2);
}

#[test]
fn test_geometry_to_mesh() {
    let plane = Geometry3D::Plane {
        width: 100.0,
        height: 100.0,
        subdivisions: 4,
    }
    .to_mesh();
    assert_eq!(plane.vertices.len(), 25); // (4+1) * (4+1)
    assert_eq!(plane.faces.len(), 16); // 4 * 4

    let cylinder = Geometry3D::Cylinder {
        radius: 20.0,
        height: 60.0,
        segments: 8,
    }
    .to_mesh();
    assert_eq!(cylinder.vertices.len(), 2 + 8 * 2); // 2 caps + 16 ring verts
    assert_eq!(cylinder.faces.len(), 8 + 8 + 8); // top, bottom, side quads
}

#[test]
fn test_instancing_generators() {
    let grid = InstancingBuffer::grid_3d(3, 3, 3, 20.0);
    assert_eq!(grid.instances.len(), 27);

    let ring = InstancingBuffer::ring(12, 100.0);
    assert_eq!(ring.instances.len(), 12);

    let sphere = InstancingBuffer::fibonacci_sphere(50, 150.0);
    assert_eq!(sphere.instances.len(), 50);
}

#[test]
fn test_scene3d_serialization() {
    let mut scene = Scene3D::new();
    scene.add_node(Node3D::new(
        Geometry3D::Torus {
            r_major: 40.0,
            r_minor: 12.0,
            segs_major: 16,
            segs_minor: 12,
        },
        Material3D::pbr(Color::rgb(0, 200, 255), 0.5, 0.5),
    ));

    let json = serde_json::to_string(&scene).expect("serialization failed");
    let deserialized: Scene3D = serde_json::from_str(&json).expect("deserialization failed");
    assert_eq!(deserialized.nodes.len(), 1);
}

#[test]
fn test_software_rasterizer_draws_pixels() {
    let mut scene = Scene3D::new()
        .with_camera(Scene3DCamera::perspective(
            Vec3::new(0.0, 0.0, 150.0),
            Vec3::new(0.0, 0.0, 0.0),
            60.0,
        ))
        .with_background(Scene3DBackground::Solid {
            color: Color::rgb(0, 0, 0),
        });

    scene.add_light(Scene3DLight::ambient(Color::WHITE, 0.5));
    scene.add_light(Scene3DLight::directional(
        Vec3::new(0.0, 0.0, -1.0),
        Color::WHITE,
        0.8,
    ));

    scene.add_node(Node3D::new(
        Geometry3D::Cube { size: 40.0 },
        Material3D::pbr(Color::rgb(255, 0, 0), 0.0, 1.0),
    ));

    let mut pixmap = Pixmap::new(200, 200).expect("pixmap creation failed");
    render_scene3d_software(
        &scene,
        0.0,
        0.0,
        200.0,
        200.0,
        1.0,
        &mut pixmap,
        Transform::identity(),
    );

    // Verify non-zero pixels were drawn in the center
    let center_px = pixmap.pixel(100, 100).expect("pixel lookup");
    assert!(
        center_px.red() > 0,
        "Center pixel should have red rendered cube"
    );
}
