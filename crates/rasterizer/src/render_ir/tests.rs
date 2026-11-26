use super::*;
use crate::backend::{FrameConfig, RasterizerBackend};
use crate::scene::{
    BlendMode, Color, ImageFit, MaskMode, Scene, SceneFilter, SceneNode, Transform2D,
};
use crate::tiny_skia_backend::TinySkiaBackend;

fn color_res(id: u32, label: &str, w: u32, h: u32, lifetime: ResourceLifetime) -> ResourceDesc {
    ResourceDesc {
        id: ResourceId(id),
        label: label.into(),
        width: w,
        height: h,
        format: PixelFormat::Rgba8Unorm,
        color: ColorMetadata::srgb_premultiplied(),
        sample_count: 1,
        usage: TextureUsage::RENDER_TARGET
            .union(TextureUsage::SAMPLED)
            .union(TextureUsage::COPY_SRC)
            .union(TextureUsage::COPY_DST),
        lifetime,
    }
}

fn mask_res(id: u32, label: &str, w: u32, h: u32, lifetime: ResourceLifetime) -> ResourceDesc {
    ResourceDesc {
        id: ResourceId(id),
        label: label.into(),
        width: w,
        height: h,
        format: PixelFormat::R8Unorm,
        color: ColorMetadata::mask_linear(),
        sample_count: 1,
        usage: TextureUsage::RENDER_TARGET.union(TextureUsage::SAMPLED),
        lifetime,
    }
}

fn empty_raster_pass(id: u32, dst: u32, clear: ClearOp) -> PassNode {
    PassNode {
        id: PassId(id),
        label: format!("p{id}"),
        op: PassOp::Raster2D {
            scene: Scene::new(),
            clear,
            path_mask: None,
        },
        reads: Vec::new(),
        writes: vec![ResourceId(dst)],
        order_deps: Vec::new(),
        required_capabilities: CapabilityFlags::RASTER_2D_ANALYTIC,
    }
}

#[test]
fn validation_rejects_invalid_dimensions_and_overflow() {
    let mut zero = RenderGraph::new(0, 64);
    zero.add_resource(color_res(0, "out", 64, 64, ResourceLifetime::Output));
    zero.set_output(ResourceId(0));
    zero.add_pass(empty_raster_pass(0, 0, ClearOp::Clear(Color::BLACK)));
    assert_eq!(
        ExecutionPlan::compile(zero),
        Err(GraphValidationError::InvalidDimensions {
            width: 0,
            height: 64
        })
    );

    let mut overflow = RenderGraph::new(64, 64);
    overflow.add_resource(ResourceDesc {
        id: ResourceId(0),
        label: "huge".into(),
        width: u32::MAX,
        height: u32::MAX,
        format: PixelFormat::Rgba16Float,
        color: ColorMetadata::srgb_premultiplied(),
        sample_count: 4,
        usage: TextureUsage::RENDER_TARGET,
        lifetime: ResourceLifetime::Output,
    });
    overflow.set_output(ResourceId(0));
    overflow.add_pass(empty_raster_pass(0, 0, ClearOp::Clear(Color::BLACK)));
    assert_eq!(
        ExecutionPlan::compile(overflow),
        Err(GraphValidationError::ResourceSizeOverflow { id: ResourceId(0) })
    );

    let mut bad_msaa = RenderGraph::new(64, 64);
    let mut res = color_res(0, "out", 64, 64, ResourceLifetime::Output);
    res.sample_count = 8;
    bad_msaa.add_resource(res);
    bad_msaa.set_output(ResourceId(0));
    bad_msaa.add_pass(empty_raster_pass(0, 0, ClearOp::Clear(Color::BLACK)));
    assert_eq!(
        ExecutionPlan::compile(bad_msaa),
        Err(GraphValidationError::InvalidSampleCount {
            id: ResourceId(0),
            sample_count: 8
        })
    );
}

#[test]
fn validation_rejects_duplicates_missing_output_and_unknown_refs() {
    let mut dup_res = RenderGraph::new(64, 64);
    dup_res.add_resource(color_res(0, "a", 64, 64, ResourceLifetime::Output));
    dup_res.add_resource(color_res(0, "b", 64, 64, ResourceLifetime::Transient));
    dup_res.set_output(ResourceId(0));
    dup_res.add_pass(empty_raster_pass(0, 0, ClearOp::Clear(Color::BLACK)));
    assert_eq!(
        ExecutionPlan::compile(dup_res),
        Err(GraphValidationError::DuplicateResourceId(ResourceId(0)))
    );

    let mut dup_pass = RenderGraph::new(64, 64);
    dup_pass.add_resource(color_res(0, "out", 64, 64, ResourceLifetime::Output));
    dup_pass.set_output(ResourceId(0));
    dup_pass.add_pass(empty_raster_pass(1, 0, ClearOp::Clear(Color::BLACK)));
    dup_pass.add_pass(empty_raster_pass(1, 0, ClearOp::Clear(Color::WHITE)));
    assert_eq!(
        ExecutionPlan::compile(dup_pass),
        Err(GraphValidationError::DuplicatePassId(PassId(1)))
    );

    let mut no_out = RenderGraph::new(64, 64);
    no_out.add_resource(color_res(0, "out", 64, 64, ResourceLifetime::Output));
    no_out.add_pass(empty_raster_pass(0, 0, ClearOp::Clear(Color::BLACK)));
    assert_eq!(
        ExecutionPlan::compile(no_out),
        Err(GraphValidationError::MissingOutput)
    );

    let mut unwritten_out = RenderGraph::new(64, 64);
    unwritten_out.add_resource(color_res(0, "out", 64, 64, ResourceLifetime::Output));
    unwritten_out.add_resource(color_res(1, "tmp", 64, 64, ResourceLifetime::Transient));
    unwritten_out.set_output(ResourceId(0));
    unwritten_out.add_pass(empty_raster_pass(0, 1, ClearOp::Clear(Color::BLACK)));
    assert_eq!(
        ExecutionPlan::compile(unwritten_out),
        Err(GraphValidationError::OutputNeverWritten(ResourceId(0)))
    );

    let mut unknown_res = RenderGraph::new(64, 64);
    unknown_res.add_resource(color_res(0, "out", 64, 64, ResourceLifetime::Output));
    unknown_res.set_output(ResourceId(0));
    unknown_res.add_pass(empty_raster_pass(0, 99, ClearOp::Clear(Color::BLACK)));
    assert_eq!(
        ExecutionPlan::compile(unknown_res),
        Err(GraphValidationError::UnknownResource {
            pass: Some(PassId(0)),
            resource: ResourceId(99)
        })
    );

    let mut unknown_dep = RenderGraph::new(64, 64);
    unknown_dep.add_resource(color_res(0, "out", 64, 64, ResourceLifetime::Output));
    unknown_dep.set_output(ResourceId(0));
    let mut p = empty_raster_pass(0, 0, ClearOp::Clear(Color::BLACK));
    p.order_deps.push(PassId(42));
    unknown_dep.add_pass(p);
    assert_eq!(
        ExecutionPlan::compile(unknown_dep),
        Err(GraphValidationError::UnknownOrderDep {
            pass: PassId(0),
            dep: PassId(42)
        })
    );
}

#[test]
fn validation_rejects_cycles_self_deps_and_read_before_write() {
    let mut self_dep = RenderGraph::new(64, 64);
    self_dep.add_resource(color_res(0, "out", 64, 64, ResourceLifetime::Output));
    self_dep.set_output(ResourceId(0));
    let mut p = empty_raster_pass(0, 0, ClearOp::Clear(Color::BLACK));
    p.order_deps.push(PassId(0));
    self_dep.add_pass(p);
    assert_eq!(
        ExecutionPlan::compile(self_dep),
        Err(GraphValidationError::SelfDependency { pass: PassId(0) })
    );

    let mut cycle = RenderGraph::new(64, 64);
    cycle.add_resource(color_res(0, "out", 64, 64, ResourceLifetime::Output));
    cycle.add_resource(color_res(1, "tmp", 64, 64, ResourceLifetime::Transient));
    cycle.set_output(ResourceId(0));
    let mut p0 = empty_raster_pass(0, 0, ClearOp::Clear(Color::BLACK));
    p0.order_deps.push(PassId(1));
    let mut p1 = empty_raster_pass(1, 1, ClearOp::Clear(Color::BLACK));
    p1.order_deps.push(PassId(0));
    cycle.add_pass(p0);
    cycle.add_pass(p1);
    assert_eq!(
        ExecutionPlan::compile(cycle),
        Err(GraphValidationError::CycleDetected {
            involved: vec![PassId(0), PassId(1)]
        })
    );

    let mut rbw_missing_writer = RenderGraph::new(64, 64);
    rbw_missing_writer.add_resource(color_res(0, "out", 64, 64, ResourceLifetime::Output));
    rbw_missing_writer.add_resource(color_res(
        1,
        "unwritten",
        64,
        64,
        ResourceLifetime::Transient,
    ));
    rbw_missing_writer.set_output(ResourceId(0));
    rbw_missing_writer.add_pass(empty_raster_pass(0, 0, ClearOp::Clear(Color::BLACK)));
    rbw_missing_writer.add_pass(PassNode {
        id: PassId(1),
        label: "comp".into(),
        op: PassOp::Composite {
            source: ResourceId(1),
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            region: None,
        },
        reads: vec![ResourceId(1), ResourceId(0)],
        writes: vec![ResourceId(0)],
        order_deps: Vec::new(),
        required_capabilities: CapabilityFlags::OFFSCREEN_COMPOSITE,
    });
    assert_eq!(
        ExecutionPlan::compile(rbw_missing_writer),
        Err(GraphValidationError::ReadBeforeWrite {
            pass: PassId(1),
            resource: ResourceId(1)
        })
    );

    let mut rbw_load_on_uninit = RenderGraph::new(64, 64);
    rbw_load_on_uninit.add_resource(color_res(0, "out", 64, 64, ResourceLifetime::Output));
    rbw_load_on_uninit.set_output(ResourceId(0));
    rbw_load_on_uninit.add_pass(empty_raster_pass(0, 0, ClearOp::Load));
    assert_eq!(
        ExecutionPlan::compile(rbw_load_on_uninit),
        Err(GraphValidationError::ReadBeforeWrite {
            pass: PassId(0),
            resource: ResourceId(0)
        })
    );
}

#[test]
fn validation_rejects_format_dimension_and_param_mismatches() {
    let mut bad_mask_fmt = RenderGraph::new(64, 64);
    bad_mask_fmt.add_resource(color_res(0, "out", 64, 64, ResourceLifetime::Output));
    bad_mask_fmt.set_output(ResourceId(0));
    bad_mask_fmt.add_pass(PassNode {
        id: PassId(0),
        label: "mask".into(),
        op: PassOp::PathMask {
            path_d: "M 0 0 L 10 10 Z".into(),
            transform: Transform2D::identity(),
        },
        reads: Vec::new(),
        writes: vec![ResourceId(0)],
        order_deps: Vec::new(),
        required_capabilities: CapabilityFlags::PATH_MASK,
    });
    assert_eq!(
        ExecutionPlan::compile(bad_mask_fmt),
        Err(GraphValidationError::IncompatibleFormat {
            pass: PassId(0),
            resource: ResourceId(0),
            expected: "mask (R8Unorm)",
            actual: PixelFormat::Rgba8Unorm,
        })
    );

    let mut bad_copy_dims = RenderGraph::new(64, 64);
    bad_copy_dims.add_resource(color_res(0, "out", 64, 64, ResourceLifetime::Output));
    bad_copy_dims.add_resource(color_res(1, "src", 64, 64, ResourceLifetime::Transient));
    bad_copy_dims.set_output(ResourceId(0));
    bad_copy_dims.add_pass(empty_raster_pass(0, 1, ClearOp::Clear(Color::BLACK)));
    bad_copy_dims.add_pass(PassNode {
        id: PassId(1),
        label: "copy".into(),
        op: PassOp::Copy {
            source: ResourceId(1),
            dst_origin: [16, 0],
        },
        reads: vec![ResourceId(1)],
        writes: vec![ResourceId(0)],
        order_deps: Vec::new(),
        required_capabilities: CapabilityFlags::COPY_OP,
    });
    assert!(matches!(
        ExecutionPlan::compile(bad_copy_dims),
        Err(GraphValidationError::IncompatibleDimensions {
            pass: PassId(1),
            ..
        })
    ));

    let mut bad_compute = RenderGraph::new(64, 64);
    bad_compute.add_resource(color_res(0, "out", 64, 64, ResourceLifetime::Output));
    bad_compute.set_output(ResourceId(0));
    bad_compute.add_pass(PassNode {
        id: PassId(0),
        label: "compute".into(),
        op: PassOp::Compute {
            kernel: "   ".into(),
            workgroups: [1, 1, 1],
            params: Vec::new(),
        },
        reads: Vec::new(),
        writes: vec![ResourceId(0)],
        order_deps: Vec::new(),
        required_capabilities: CapabilityFlags::COMPUTE_PASS,
    });
    assert!(matches!(
        ExecutionPlan::compile(bad_compute),
        Err(GraphValidationError::InvalidPassParams {
            pass: PassId(0),
            ..
        })
    ));
}

#[test]
fn topological_scheduler_is_deterministic_and_reuses_transient_pool_slots() {
    // Build a graph where consumer (PassId 0) is declared BEFORE producer (PassId 1),
    // and two non-overlapping transient buffers share the same pool slot.
    let mut graph = RenderGraph::new(100, 100);
    graph.add_resource(color_res(0, "out", 100, 100, ResourceLifetime::Output));
    graph.add_resource(color_res(1, "t1", 100, 100, ResourceLifetime::Transient));
    graph.add_resource(color_res(2, "t2", 100, 100, ResourceLifetime::Transient));
    graph.set_output(ResourceId(0));

    // Consumer of t1
    graph.add_pass(PassNode {
        id: PassId(0),
        label: "copy_t1_to_t2".into(),
        op: PassOp::Copy {
            source: ResourceId(1),
            dst_origin: [0, 0],
        },
        reads: vec![ResourceId(1)],
        writes: vec![ResourceId(2)],
        order_deps: Vec::new(),
        required_capabilities: CapabilityFlags::COPY_OP,
    });
    // Producer of t1 (declared later, higher PassId!)
    graph.add_pass(empty_raster_pass(1, 1, ClearOp::Clear(Color::WHITE)));
    // Consumer of t2 -> out
    graph.add_pass(PassNode {
        id: PassId(2),
        label: "copy_t2_to_out".into(),
        op: PassOp::Copy {
            source: ResourceId(2),
            dst_origin: [0, 0],
        },
        reads: vec![ResourceId(2)],
        writes: vec![ResourceId(0)],
        order_deps: Vec::new(),
        required_capabilities: CapabilityFlags::COPY_OP,
    });

    let plan_a = ExecutionPlan::compile(graph.clone()).expect("valid DAG");
    let plan_b = ExecutionPlan::compile(graph).expect("valid DAG");
    assert_eq!(plan_a.pass_order, vec![PassId(1), PassId(0), PassId(2)]);
    assert_eq!(plan_a.pass_order, plan_b.pass_order);
    assert_eq!(plan_a.dump_text(), plan_b.dump_text());
    assert_eq!(plan_a.dump_json().unwrap(), plan_b.dump_json().unwrap());

    // t1 is live [0..1], t2 is live [1..2] -> they overlap at step 1, so 2 slots!
    assert_eq!(plan_a.transient_pool_slots, 2);
    assert_eq!(plan_a.peak_transient_bytes, 100 * 100 * 4 * 2);

    // Now test disjoint transient lifetimes: t1 live [0..1], pass 2 writes out without t2,
    // pass 3 writes t2, pass 4 copies t2 -> out. t1 and t2 share 1 slot!
    let mut disjoint = RenderGraph::new(100, 100);
    disjoint.add_resource(color_res(0, "out", 100, 100, ResourceLifetime::Output));
    disjoint.add_resource(color_res(1, "t1", 100, 100, ResourceLifetime::Transient));
    disjoint.add_resource(color_res(2, "t2", 100, 100, ResourceLifetime::Transient));
    disjoint.set_output(ResourceId(0));

    disjoint.add_pass(empty_raster_pass(0, 1, ClearOp::Clear(Color::BLACK)));
    disjoint.add_pass(PassNode {
        id: PassId(1),
        label: "copy_t1".into(),
        op: PassOp::Copy {
            source: ResourceId(1),
            dst_origin: [0, 0],
        },
        reads: vec![ResourceId(1)],
        writes: vec![ResourceId(0)],
        order_deps: Vec::new(),
        required_capabilities: CapabilityFlags::COPY_OP,
    });
    let mut p2 = empty_raster_pass(2, 2, ClearOp::Clear(Color::WHITE));
    p2.order_deps.push(PassId(1));
    disjoint.add_pass(p2);
    disjoint.add_pass(PassNode {
        id: PassId(3),
        label: "composite_t2".into(),
        op: PassOp::Composite {
            source: ResourceId(2),
            opacity: 0.5,
            blend_mode: BlendMode::Normal,
            region: None,
        },
        reads: vec![ResourceId(2), ResourceId(0)],
        writes: vec![ResourceId(0)],
        order_deps: Vec::new(),
        required_capabilities: CapabilityFlags::OFFSCREEN_COMPOSITE,
    });

    let disjoint_plan = ExecutionPlan::compile(disjoint).expect("disjoint DAG");
    assert_eq!(
        disjoint_plan.pass_order,
        vec![PassId(0), PassId(1), PassId(2), PassId(3)]
    );
    assert_eq!(disjoint_plan.transient_pool_slots, 1);
    assert_eq!(disjoint_plan.peak_transient_bytes, 100 * 100 * 4);

    let json: serde_json::Value =
        serde_json::from_str(&disjoint_plan.dump_json().unwrap()).unwrap();
    assert_eq!(json["transient_pool_slots"], 1);
    assert_eq!(json["passes"].as_array().unwrap().len(), 4);
    assert!(disjoint_plan.dump_text().contains("RenderPlan 100x100"));
}

#[test]
fn cpu_executor_matches_tiny_skia_backend_bit_for_bit() {
    let tiny_png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
    let make_scene = |frame: u32| Scene {
        nodes: vec![
            SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: 160.0,
                h: 120.0,
                fill: Color::rgb(18, 26, 44),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            },
            SceneNode::Rect {
                x: 12.0 + frame as f32 * 2.0,
                y: 16.0,
                w: 56.0,
                h: 40.0,
                fill: Color::rgba(240, 120, 40, 220),
                stroke: Some(Color::WHITE),
                stroke_width: 2.0,
                corner_radius: 8.0,
            },
            SceneNode::Path {
                d: "M 90 20 L 140 20 L 115 70 Z".into(),
                fill: Some(Color::rgb(64, 210, 180)),
                stroke: None,
                stroke_width: 0.0,
                opacity: 0.9,
            },
            SceneNode::Layer {
                opacity: 0.65,
                blend_mode: BlendMode::Normal,
                clip: None,
                mask: None,
                mask_mode: MaskMode::Alpha,
                filters: Vec::new(),
                shadow: None,
                children: vec![
                    SceneNode::Image {
                        src: tiny_png.into(),
                        x: 20.0,
                        y: 60.0,
                        w: 48.0,
                        h: 40.0,
                        fit: ImageFit::Fill,
                        opacity: 1.0,
                    },
                    SceneNode::Image {
                        src: tiny_png.into(),
                        x: 40.0,
                        y: 70.0,
                        w: 48.0,
                        h: 40.0,
                        fit: ImageFit::Fill,
                        opacity: 1.0,
                    },
                ],
            },
        ],
    };

    let backend = TinySkiaBackend::headless();
    let executor = CpuReferenceExecutor::new(&backend);
    let opts = LowerOptions::default();

    for frame in [0u32, 3, 1, 3, 0] {
        let cfg = FrameConfig::new(160, 120, frame, 30.0);
        let scene = make_scene(frame);
        let plan = lower_scene_to_plan(&scene, &cfg, &opts).expect("lowering should succeed");
        let (exec_img, report) = executor.execute_plan(&plan, &cfg).expect("execute_plan");
        let ref_img = backend.render_frame(&scene, &cfg).expect("render_frame");
        assert_eq!(exec_img.as_raw(), ref_img.as_raw());
        assert_eq!(report.backend_name, "cpu_reference");
        assert_eq!(report.cpu_island_passes, 0);
        assert_eq!(report.passes_executed, 3);
    }
}

#[test]
fn cpu_executor_runs_explicit_path_mask_and_copy_and_rejects_compute() {
    let backend = TinySkiaBackend::headless();
    let executor = CpuReferenceExecutor::new(&backend);

    let mut graph = RenderGraph::new(64, 64);
    graph.add_resource(color_res(0, "out", 64, 64, ResourceLifetime::Output));
    graph.add_resource(mask_res(1, "mask", 64, 64, ResourceLifetime::Transient));
    graph.add_resource(color_res(2, "half", 32, 32, ResourceLifetime::Transient));
    graph.set_output(ResourceId(0));

    graph.add_pass(PassNode {
        id: PassId(0),
        label: "path_mask".into(),
        op: PassOp::PathMask {
            path_d: "M 0 0 L 32 0 L 32 32 L 0 32 Z".into(),
            transform: Transform2D::identity(),
        },
        reads: Vec::new(),
        writes: vec![ResourceId(1)],
        order_deps: Vec::new(),
        required_capabilities: CapabilityFlags::PATH_MASK,
    });
    graph.add_pass(PassNode {
        id: PassId(1),
        label: "masked_fill".into(),
        op: PassOp::Raster2D {
            scene: Scene {
                nodes: vec![SceneNode::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 64.0,
                    h: 64.0,
                    fill: Color::rgb(255, 0, 0),
                    stroke: None,
                    stroke_width: 0.0,
                    corner_radius: 0.0,
                }],
            },
            clear: ClearOp::Clear(Color::BLACK),
            path_mask: Some(ResourceId(1)),
        },
        reads: vec![ResourceId(1)],
        writes: vec![ResourceId(0)],
        order_deps: Vec::new(),
        required_capabilities: CapabilityFlags::RASTER_2D_ANALYTIC
            .union(CapabilityFlags::PATH_MASK),
    });
    graph.add_pass(PassNode {
        id: PassId(2),
        label: "blue_tile".into(),
        op: PassOp::Raster2D {
            scene: Scene {
                nodes: vec![SceneNode::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 32.0,
                    h: 32.0,
                    fill: Color::rgb(0, 0, 255),
                    stroke: None,
                    stroke_width: 0.0,
                    corner_radius: 0.0,
                }],
            },
            clear: ClearOp::Clear(Color::TRANSPARENT),
            path_mask: None,
        },
        reads: Vec::new(),
        writes: vec![ResourceId(2)],
        order_deps: Vec::new(),
        required_capabilities: CapabilityFlags::RASTER_2D_ANALYTIC,
    });
    graph.add_pass(PassNode {
        id: PassId(3),
        label: "copy_tile".into(),
        op: PassOp::Copy {
            source: ResourceId(2),
            dst_origin: [32, 32],
        },
        reads: vec![ResourceId(2), ResourceId(0)],
        writes: vec![ResourceId(0)],
        order_deps: vec![PassId(1)],
        required_capabilities: CapabilityFlags::COPY_OP,
    });

    let plan = ExecutionPlan::compile(graph).expect("valid masked + copy plan");
    let (img, report) = executor
        .execute_plan(&plan, &FrameConfig::new(64, 64, 0, 30.0))
        .expect("execute_plan");
    assert_eq!(report.passes_executed, 4);
    assert_eq!(img.get_pixel(16, 16).0, [255, 0, 0, 255]);
    assert_eq!(img.get_pixel(48, 16).0, [0, 0, 0, 255]);
    assert_eq!(img.get_pixel(48, 48).0, [0, 0, 255, 255]);

    // Unsupported compute pass must return explicit UnsupportedCapability error.
    let mut compute_graph = RenderGraph::new(32, 32);
    compute_graph.add_resource(color_res(0, "out", 32, 32, ResourceLifetime::Output));
    compute_graph.set_output(ResourceId(0));
    compute_graph.add_pass(PassNode {
        id: PassId(0),
        label: "blur_compute".into(),
        op: PassOp::Compute {
            kernel: "separable_blur".into(),
            workgroups: [4, 4, 1],
            params: vec![2.0],
        },
        reads: Vec::new(),
        writes: vec![ResourceId(0)],
        order_deps: Vec::new(),
        required_capabilities: CapabilityFlags::COMPUTE_PASS,
    });
    let compute_plan = ExecutionPlan::compile(compute_graph).unwrap();
    let err = executor
        .execute_plan(&compute_plan, &FrameConfig::new(32, 32, 0, 30.0))
        .unwrap_err();
    assert!(matches!(
        err,
        ExecutionError::UnsupportedCapability {
            backend: "cpu_reference",
            pass: PassId(0),
            ..
        }
    ));
}

#[test]
fn lowering_routes_unsupported_scene_nodes_to_cpu_island_when_allowed() {
    let scene = Scene {
        nodes: vec![SceneNode::FractalNoise {
            x: 0.0,
            y: 0.0,
            w: 64.0,
            h: 64.0,
            noise_type: Default::default(),
            fractal_type: Default::default(),
            contrast: 1.0,
            brightness: 0.0,
            complexity: 3,
            scale_x: 32.0,
            scale_y: 32.0,
            evolution: 0.0,
            seed: 42,
            opacity: 1.0,
        }],
    };
    let cfg = FrameConfig::new(64, 64, 0, 30.0);
    // Simulate a GPU target that supports CPU_ISLAND fallback but not RASTER_EXTENDED.
    let opts = LowerOptions {
        max_texture_dimension_2d: 8192,
        capabilities: CapabilityFlags::RASTER_2D_ANALYTIC.union(CapabilityFlags::CPU_ISLAND),
        msaa_samples: 4,
    };
    let plan = lower_scene_to_plan(&scene, &cfg, &opts).unwrap();
    assert_eq!(plan.pass_order, vec![PassId(0)]);
    assert!(matches!(
        plan.graph.pass(PassId(0)).unwrap().op,
        PassOp::CpuIsland { .. }
    ));

    let backend = TinySkiaBackend::headless();
    let executor = CpuReferenceExecutor::new(&backend);
    let (_img, report) = executor.execute_plan(&plan, &cfg).unwrap();
    assert_eq!(report.cpu_island_passes, 1);

    // Non-empty unsupported filters (e.g. Blur) remain verifiable via Scene lowering
    let _ = SceneFilter::Blur { sigma: 1.0 };
}
