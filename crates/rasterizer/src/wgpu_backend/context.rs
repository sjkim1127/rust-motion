use super::pipeline::*;
use super::types::*;
use crate::backend::RasterError;
use std::sync::Arc;

/// GPU render context: device, queue, pipeline, and bind group layouts.
pub(crate) struct GpuContext {
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    pub(crate) pipeline: wgpu::RenderPipeline,
    pub(crate) mesh_pipeline: wgpu::RenderPipeline,
    pub(crate) image_pipeline: wgpu::RenderPipeline,
    /// Single-sample pipeline used when compositing a resolved offscreen
    /// layer texture into the resolved frame target.
    #[allow(dead_code)]
    pub(crate) image_composite_pipeline: wgpu::RenderPipeline,
    pub(crate) path_mask_pipeline: wgpu::RenderPipeline,
    pub(crate) text_pipeline: wgpu::RenderPipeline,
    pub(crate) multiply_pipeline: wgpu::RenderPipeline,
    pub(crate) multiply_mesh_pipeline: wgpu::RenderPipeline,
    pub(crate) multiply_image_pipeline: wgpu::RenderPipeline,
    pub(crate) multiply_text_pipeline: wgpu::RenderPipeline,
    pub(crate) screen_pipeline: wgpu::RenderPipeline,
    pub(crate) screen_mesh_pipeline: wgpu::RenderPipeline,
    pub(crate) screen_image_pipeline: wgpu::RenderPipeline,
    pub(crate) screen_text_pipeline: wgpu::RenderPipeline,
    pub(crate) darken_pipeline: wgpu::RenderPipeline,
    pub(crate) darken_mesh_pipeline: wgpu::RenderPipeline,
    pub(crate) darken_image_pipeline: wgpu::RenderPipeline,
    pub(crate) darken_text_pipeline: wgpu::RenderPipeline,
    pub(crate) lighten_pipeline: wgpu::RenderPipeline,
    pub(crate) lighten_mesh_pipeline: wgpu::RenderPipeline,
    pub(crate) lighten_image_pipeline: wgpu::RenderPipeline,
    pub(crate) lighten_text_pipeline: wgpu::RenderPipeline,
    pub(crate) globals_layout: wgpu::BindGroupLayout,
    pub(crate) instance_layout: wgpu::BindGroupLayout,
    pub(crate) image_layout: wgpu::BindGroupLayout,
    pub(crate) path_mask_layout: wgpu::BindGroupLayout,
    pub(crate) max_texture_dimension_2d: u32,
    pub(crate) supports_timestamp_queries: bool,
    pub(crate) timestamp_period: f32,
}

impl GpuContext {
    pub(crate) fn new() -> Result<Self, RasterError> {
        pollster::block_on(Self::new_async())
    }

    async fn new_async() -> Result<Self, RasterError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| {
                RasterError::Init(
                    "No GPU adapter found. Ensure a GPU is available or use --backend native."
                        .into(),
                )
            })?;

        let limits = adapter.limits();
        let max_texture_dimension_2d = limits.max_texture_dimension_2d;

        let adapter_features = adapter.features();
        let supports_timestamp_queries = adapter_features.contains(wgpu::Features::TIMESTAMP_QUERY);
        let mut required_features = wgpu::Features::empty();
        if supports_timestamp_queries {
            required_features |= wgpu::Features::TIMESTAMP_QUERY;
        }

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("rust-motion-rasterizer"),
                    required_features,
                    required_limits: limits,
                    memory_hints: Default::default(),
                },
                None,
            )
            .await
            .map_err(|e| RasterError::Init(format!("GPU device creation failed: {e}")))?;

        let timestamp_period = if supports_timestamp_queries {
            queue.get_timestamp_period()
        } else {
            1.0
        };

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rust_motion_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });

        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let instance_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("instance_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let path_mask_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("path_mask_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline_layout"),
            bind_group_layouts: &[
                &globals_layout,
                &instance_layout,
                &image_layout,
                &path_mask_layout,
            ],
            push_constant_ranges: &[],
        });

        let make_blend_pipeline = |label: &'static str,
                                   entry_point: &'static str,
                                   mesh: bool,
                                   blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: if mesh { "vs_mesh" } else { "vs_main" },
                    buffers: if mesh {
                        &[wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<GpuVertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &MESH_ATTRIBUTES,
                        }]
                    } else {
                        &[]
                    },
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point,
                    targets: &[Some(wgpu::ColorTargetState {
                        format: RENDER_FORMAT,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: if mesh {
                    wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        ..Default::default()
                    }
                } else {
                    wgpu::PrimitiveState::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: SAMPLE_COUNT,
                    ..Default::default()
                },
                multiview: None,
                cache: None,
            })
        };
        let multiply = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Dst,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        let screen = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::OneMinusDst,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        let multiply_pipeline =
            make_blend_pipeline("rust_motion_multiply_pipeline", "fs_main", false, multiply);
        let multiply_mesh_pipeline = make_blend_pipeline(
            "rust_motion_multiply_mesh_pipeline",
            "fs_solid",
            true,
            multiply,
        );
        let multiply_image_pipeline = make_blend_pipeline(
            "rust_motion_multiply_image_pipeline",
            "fs_image",
            false,
            multiply,
        );
        let multiply_text_pipeline = make_blend_pipeline(
            "rust_motion_multiply_text_pipeline",
            "fs_text",
            false,
            multiply,
        );
        let screen_pipeline =
            make_blend_pipeline("rust_motion_screen_pipeline", "fs_main", false, screen);
        let screen_mesh_pipeline =
            make_blend_pipeline("rust_motion_screen_mesh_pipeline", "fs_solid", true, screen);
        let screen_image_pipeline = make_blend_pipeline(
            "rust_motion_screen_image_pipeline",
            "fs_image",
            false,
            screen,
        );
        let screen_text_pipeline =
            make_blend_pipeline("rust_motion_screen_text_pipeline", "fs_text", false, screen);
        let darken = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Min,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        let lighten = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Max,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        let darken_pipeline =
            make_blend_pipeline("rust_motion_darken_pipeline", "fs_main", false, darken);
        let darken_mesh_pipeline =
            make_blend_pipeline("rust_motion_darken_mesh_pipeline", "fs_solid", true, darken);
        let darken_image_pipeline = make_blend_pipeline(
            "rust_motion_darken_image_pipeline",
            "fs_image",
            false,
            darken,
        );
        let darken_text_pipeline =
            make_blend_pipeline("rust_motion_darken_text_pipeline", "fs_text", false, darken);
        let lighten_pipeline =
            make_blend_pipeline("rust_motion_lighten_pipeline", "fs_main", false, lighten);
        let lighten_mesh_pipeline = make_blend_pipeline(
            "rust_motion_lighten_mesh_pipeline",
            "fs_solid",
            true,
            lighten,
        );
        let lighten_image_pipeline = make_blend_pipeline(
            "rust_motion_lighten_image_pipeline",
            "fs_image",
            false,
            lighten,
        );
        let lighten_text_pipeline = make_blend_pipeline(
            "rust_motion_lighten_text_pipeline",
            "fs_text",
            false,
            lighten,
        );

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rust_motion_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: RENDER_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: SAMPLE_COUNT,
                ..Default::default()
            },
            multiview: None,
            cache: None,
        });

        let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rust_motion_mesh_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_mesh",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GpuVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &MESH_ATTRIBUTES,
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_solid",
                targets: &[Some(wgpu::ColorTargetState {
                    format: RENDER_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: SAMPLE_COUNT,
                ..Default::default()
            },
            multiview: None,
            cache: None,
        });

        let image_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rust_motion_image_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_image",
                targets: &[Some(wgpu::ColorTargetState {
                    format: RENDER_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: SAMPLE_COUNT,
                ..Default::default()
            },
            multiview: None,
            cache: None,
        });

        let image_composite_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("rust_motion_image_composite_pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_image",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: RENDER_FORMAT,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        let path_mask_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rust_motion_path_mask_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_mesh",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GpuVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &MESH_ATTRIBUTES,
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_solid",
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::R8Unorm,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::RED,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let text_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rust_motion_text_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_text",
                targets: &[Some(wgpu::ColorTargetState {
                    format: RENDER_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: SAMPLE_COUNT,
                ..Default::default()
            },
            multiview: None,
            cache: None,
        });

        Ok(Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
            pipeline,
            mesh_pipeline,
            image_pipeline,
            image_composite_pipeline,
            path_mask_pipeline,
            text_pipeline,
            multiply_pipeline,
            multiply_mesh_pipeline,
            multiply_image_pipeline,
            multiply_text_pipeline,
            screen_pipeline,
            screen_mesh_pipeline,
            screen_image_pipeline,
            screen_text_pipeline,
            darken_pipeline,
            darken_mesh_pipeline,
            darken_image_pipeline,
            darken_text_pipeline,
            lighten_pipeline,
            lighten_mesh_pipeline,
            lighten_image_pipeline,
            lighten_text_pipeline,
            globals_layout,
            instance_layout,
            image_layout,
            path_mask_layout,
            max_texture_dimension_2d,
            supports_timestamp_queries,
            timestamp_period,
        })
    }
}
