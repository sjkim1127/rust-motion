#![cfg(feature = "rhai")]

use base64::Engine;
use rust_motion_cli::rhai_runtime::RhaiComposition;
use rust_motion_composition::{Composition, NativeCompositionContext};
use rust_motion_rasterizer::SceneNode;
use serde_json::json;

#[test]
fn test_rhai_confetti_particles() {
    let script = r##"
        fn render(context, props) {
            let scene = scene();
            scene.confetti(#{
                count: 40,
                frame: context.frame,
                fps: context.fps,
                velocity: 500.0,
                colors: ["#ff0055", "#00ffcc"],
                shapes: ["rect", "circle", "star"]
            });
            scene.confetti_cannon("bottom_left", #{
                count: 20,
                frame: context.frame,
                fps: context.fps
            });
            return scene;
        }
    "##;

    let comp = RhaiComposition::from_source("test_confetti", script).expect("compile rhai");
    let ctx = NativeCompositionContext {
        width: 1920,
        height: 1080,
        fps: 30.0,
        duration_in_frames: 60,
    };
    let prepared = comp.prepare(&json!({}), ctx).expect("prepare");

    // Frame 0
    let scene_f0 = prepared.render(0).expect("render f0");
    assert_eq!(
        scene_f0.nodes.len(),
        60,
        "40 + 20 confetti particles emitted"
    );

    // Frame 15
    let scene_f15 = prepared.render(15).expect("render f15");
    assert_eq!(scene_f15.nodes.len(), 60);

    // Check that particles are wrapped in Group nodes with transforms
    for node in &scene_f15.nodes {
        match node {
            SceneNode::Group {
                transform,
                children,
                ..
            } => {
                assert!(transform.tx.is_finite());
                assert!(transform.ty.is_finite());
                assert_eq!(children.len(), 1);
            }
            other => panic!("Expected Group node for confetti particle, got: {other:?}"),
        }
    }
}

#[test]
fn test_rhai_mesh_3d_procedural() {
    let script = r##"
        fn render(context, props) {
            let scene = scene();
            // Render a rotating 3D Cube
            scene.mesh_3d("cube", #{
                x: 400.0,
                y: 300.0,
                size: 150.0,
                rotate_x: 25.0,
                rotate_y: 45.0,
                color: "#f59e0b"
            });

            // Render a 3D Sphere
            scene.mesh_3d("sphere", #{
                x: 800.0,
                y: 300.0,
                size: 100.0,
                rings: 8,
                sectors: 12,
                color: "#3b82f6"
            });

            // Render a 3D Torus
            scene.mesh_3d("torus", #{
                x: 1200.0,
                y: 300.0,
                size: 120.0,
                color: "#10b981"
            });
            return scene;
        }
    "##;

    let comp = RhaiComposition::from_source("test_mesh3d", script).expect("compile rhai");
    let ctx = NativeCompositionContext {
        width: 1920,
        height: 1080,
        fps: 30.0,
        duration_in_frames: 30,
    };
    let prepared = comp.prepare(&json!({}), ctx).expect("prepare");

    let scene = prepared.render(0).expect("render frame with 3d meshes");
    assert!(
        !scene.nodes.is_empty(),
        "3D meshes must produce scene nodes"
    );

    // All emitted nodes from mesh_3d should be Path nodes (polygons with Lambertian shading)
    let mut path_count = 0;
    for node in &scene.nodes {
        if let SceneNode::Path { d, fill, .. } = node {
            assert!(d.starts_with('M'), "Path must start with M");
            assert!(d.ends_with('Z'), "Path must close with Z");
            assert!(
                fill.is_some(),
                "Path must have Lambertian diffuse fill color"
            );
            path_count += 1;
        }
    }

    assert!(
        path_count >= 10,
        "Expected at least 10 projected faces across cube, sphere, and torus, got {path_count}"
    );
}

#[test]
fn test_rhai_gltf_loading_and_rendering() {
    // Generate minimal valid embedded glTF JSON with a 3D triangle
    let mut bin = Vec::new();
    for &(x, y, z) in &[
        (-1.0f32, 0.0f32, 0.0f32),
        (1.0f32, 0.0f32, 0.0f32),
        (0.0f32, 1.0f32, 0.0f32),
    ] {
        bin.extend_from_slice(&x.to_le_bytes());
        bin.extend_from_slice(&y.to_le_bytes());
        bin.extend_from_slice(&z.to_le_bytes());
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);

    let gltf_json = format!(
        r#"{{
            "asset": {{ "version": "2.0" }},
            "buffers": [{{ "byteLength": {}, "uri": "data:application/octet-stream;base64,{}" }}],
            "bufferViews": [{{ "buffer": 0, "byteOffset": 0, "byteLength": {} }}],
            "accessors": [{{ "bufferView": 0, "byteOffset": 0, "componentType": 5126, "count": 3, "type": "VEC3" }}],
            "meshes": [{{
                "name": "Triangle3D",
                "primitives": [{{ "attributes": {{ "POSITION": 0 }} }}]
            }}]
        }}"#,
        bin.len(),
        b64,
        bin.len()
    );

    let script = format!(
        r##"
        fn render(context, props) {{
            let scene = scene();
            let gltf_str = `{}`;
            scene.gltf(gltf_str, #{{
                x: 960.0,
                y: 540.0,
                scale: 150.0,
                rotate_y: 30.0,
                color: "#ec4899"
            }});
            return scene;
        }}
        "##,
        gltf_json
    );

    let comp = RhaiComposition::from_source("test_gltf", &script).expect("compile rhai");
    let ctx = NativeCompositionContext {
        width: 1920,
        height: 1080,
        fps: 30.0,
        duration_in_frames: 30,
    };
    let prepared = comp.prepare(&json!({}), ctx).expect("prepare");

    let scene = prepared.render(0).expect("render gltf frame");

    assert_eq!(scene.nodes.len(), 1, "Expected 1 projected triangle face");
    if let SceneNode::Path { d, fill, .. } = &scene.nodes[0] {
        assert!(d.starts_with('M'));
        assert!(fill.is_some());
    } else {
        panic!("Expected Path node for projected 3D face");
    }
}

#[test]
fn test_rhai_gltf_skeletal_animation() {
    let mut bin = Vec::new();

    // Accessor 0: Positions (3 vertices of a triangle)
    let pos_start = bin.len();
    for &(x, y, z) in &[
        (-10.0f32, 0.0f32, 0.0f32),
        (10.0f32, 0.0f32, 0.0f32),
        (0.0f32, 20.0f32, 0.0f32),
    ] {
        bin.extend_from_slice(&x.to_le_bytes());
        bin.extend_from_slice(&y.to_le_bytes());
        bin.extend_from_slice(&z.to_le_bytes());
    }
    let pos_len = bin.len() - pos_start;

    // Accessor 1: Joints (vertex 0,1 bound to joint 0; vertex 2 bound to joint 1)
    let joints_start = bin.len();
    for j in &[[0u16, 0, 0, 0], [0u16, 0, 0, 0], [1u16, 0, 0, 0]] {
        for &val in j {
            bin.extend_from_slice(&val.to_le_bytes());
        }
    }
    let joints_len = bin.len() - joints_start;

    // Accessor 2: Weights (full weight 1.0 on first joint)
    let weights_start = bin.len();
    for _ in 0..3 {
        bin.extend_from_slice(&1.0f32.to_le_bytes());
        bin.extend_from_slice(&0.0f32.to_le_bytes());
        bin.extend_from_slice(&0.0f32.to_le_bytes());
        bin.extend_from_slice(&0.0f32.to_le_bytes());
    }
    let weights_len = bin.len() - weights_start;

    // Accessor 3: Inverse Bind Matrices (2 x Mat4 Identity)
    let ibm_start = bin.len();
    for _ in 0..2 {
        for c in 0..4 {
            for r in 0..4 {
                let v = if c == r { 1.0f32 } else { 0.0f32 };
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    let ibm_len = bin.len() - ibm_start;

    // Accessor 4: Animation timestamps [0.0, 1.0]
    let time_start = bin.len();
    bin.extend_from_slice(&0.0f32.to_le_bytes());
    bin.extend_from_slice(&1.0f32.to_le_bytes());
    let time_len = bin.len() - time_start;

    // Accessor 5: Animation rotations (2 x Quat: identity at t=0, 90 deg z-rot at t=1)
    let rot_start = bin.len();
    // t=0: Quat::IDENTITY = [0, 0, 0, 1]
    bin.extend_from_slice(&0.0f32.to_le_bytes());
    bin.extend_from_slice(&0.0f32.to_le_bytes());
    bin.extend_from_slice(&0.0f32.to_le_bytes());
    bin.extend_from_slice(&1.0f32.to_le_bytes());
    // t=1: 45 deg z rot: sin(pi/8), cos(pi/8)
    let half_angle = std::f32::consts::FRAC_PI_8;
    let s = half_angle.sin();
    let c = half_angle.cos();
    bin.extend_from_slice(&0.0f32.to_le_bytes());
    bin.extend_from_slice(&0.0f32.to_le_bytes());
    bin.extend_from_slice(&s.to_le_bytes());
    bin.extend_from_slice(&c.to_le_bytes());
    let rot_len = bin.len() - rot_start;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);

    let gltf_json = format!(
        r#"{{
            "asset": {{ "version": "2.0" }},
            "buffers": [{{ "byteLength": {}, "uri": "data:application/octet-stream;base64,{}" }}],
            "bufferViews": [
                {{ "buffer": 0, "byteOffset": {}, "byteLength": {} }},
                {{ "buffer": 0, "byteOffset": {}, "byteLength": {} }},
                {{ "buffer": 0, "byteOffset": {}, "byteLength": {} }},
                {{ "buffer": 0, "byteOffset": {}, "byteLength": {} }},
                {{ "buffer": 0, "byteOffset": {}, "byteLength": {} }},
                {{ "buffer": 0, "byteOffset": {}, "byteLength": {} }}
            ],
            "accessors": [
                {{ "bufferView": 0, "byteOffset": 0, "componentType": 5126, "count": 3, "type": "VEC3" }},
                {{ "bufferView": 1, "byteOffset": 0, "componentType": 5123, "count": 3, "type": "VEC4" }},
                {{ "bufferView": 2, "byteOffset": 0, "componentType": 5126, "count": 3, "type": "VEC4" }},
                {{ "bufferView": 3, "byteOffset": 0, "componentType": 5126, "count": 2, "type": "MAT4" }},
                {{ "bufferView": 4, "byteOffset": 0, "componentType": 5126, "count": 2, "type": "SCALAR" }},
                {{ "bufferView": 5, "byteOffset": 0, "componentType": 5126, "count": 2, "type": "VEC4" }}
            ],
            "nodes": [
                {{ "name": "RootBone", "children": [1] }},
                {{ "name": "AnimatedBone" }}
            ],
            "skins": [
                {{
                    "name": "Armature",
                    "inverseBindMatrices": 3,
                    "joints": [0, 1]
                }}
            ],
            "animations": [
                {{
                    "name": "BoneDance",
                    "channels": [
                        {{ "sampler": 0, "target": {{ "node": 1, "path": "rotation" }} }}
                    ],
                    "samplers": [
                        {{ "input": 4, "output": 5 }}
                    ]
                }}
            ],
            "meshes": [{{
                "name": "SkinnedMesh",
                "primitives": [{{
                    "attributes": {{
                        "POSITION": 0,
                        "JOINTS_0": 1,
                        "WEIGHTS_0": 2
                    }}
                }}]
            }}]
        }}"#,
        bin.len(),
        b64,
        pos_start,
        pos_len,
        joints_start,
        joints_len,
        weights_start,
        weights_len,
        ibm_start,
        ibm_len,
        time_start,
        time_len,
        rot_start,
        rot_len
    );

    let script = format!(
        r##"
        fn render(context, props) {{
            let scene = scene();
            let gltf_str = `{}`;
            scene.gltf(gltf_str, #{{
                time: context.time,
                x: 960.0,
                y: 540.0,
                scale: 100.0,
                color: "#6366f1"
            }});
            return scene;
        }}
        "##,
        gltf_json
    );

    let comp = RhaiComposition::from_source("test_skeletal_rhai", &script).expect("compile rhai");
    let ctx = NativeCompositionContext {
        width: 1920,
        height: 1080,
        fps: 30.0,
        duration_in_frames: 60,
    };
    let prepared = comp.prepare(&json!({}), ctx).expect("prepare");

    // Frame 0 (t = 0.0s)
    let scene_f0 = prepared.render(0).expect("render frame 0");
    // Frame 30 (t = 1.0s)
    let scene_f30 = prepared.render(30).expect("render frame 30");

    assert_eq!(scene_f0.nodes.len(), 1);
    assert_eq!(scene_f30.nodes.len(), 1);

    let d_f0 = match &scene_f0.nodes[0] {
        SceneNode::Path { d, .. } => d.clone(),
        _ => panic!("Expected Path node"),
    };
    let d_f30 = match &scene_f30.nodes[0] {
        SceneNode::Path { d, .. } => d.clone(),
        _ => panic!("Expected Path node"),
    };

    assert_ne!(
        d_f0, d_f30,
        "Projected SVG path must change as skeletal animation advances"
    );
}

#[test]
fn test_rhai_shader_pass() {
    let script = r##"
        fn render(context, props) {
            let scene = scene();
            scene.shader(`
                let d = length(in.uv - vec2<f32>(0.5, 0.5));
                return vec4<f32>(sin(d * 10.0 + time), cos(d * 8.0), 0.8, 1.0);
            `, #{
                x: 120.0,
                y: 80.0,
                width: 640.0,
                height: 480.0,
                time: context.time,
                p0: 1.5,
                opacity: 0.9
            });
            return scene;
        }
    "##;

    let comp = RhaiComposition::from_source("test_shader", script).expect("compile rhai");
    let ctx = NativeCompositionContext {
        width: 1920,
        height: 1080,
        fps: 30.0,
        duration_in_frames: 30,
    };
    let prepared = comp.prepare(&json!({}), ctx).expect("prepare");
    let scene = prepared.render(0).expect("render shader frame");

    assert_eq!(scene.nodes.len(), 1);
    match &scene.nodes[0] {
        SceneNode::Shader {
            x,
            y,
            w,
            h,
            opacity,
            params,
            ..
        } => {
            assert_eq!(*x, 120.0);
            assert_eq!(*y, 80.0);
            assert_eq!(*w, 640.0);
            assert_eq!(*h, 480.0);
            assert_eq!(*opacity, 0.9);
            assert_eq!(params[0], 1.5);
        }
        other => panic!("Expected SceneNode::Shader, got {other:?}"),
    }

    // Verify rasterization succeeds without errors
    let backend = rust_motion_rasterizer::tiny_skia_backend::TinySkiaBackend::new();
    let cfg = rust_motion_rasterizer::FrameConfig {
        width: 1920,
        height: 1080,
        frame: 0,
        fps: 30.0,
    };
    let img = rust_motion_rasterizer::RasterizerBackend::render_frame(&backend, &scene, &cfg)
        .expect("rasterize shader pass");
    assert_eq!(img.width(), 1920);
    assert_eq!(img.height(), 1080);
}
