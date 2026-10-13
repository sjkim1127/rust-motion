//! Custom GPU / Shadertoy fragment shader runner (WGSL).
//!
//! Provides execution of custom procedural shaders (plasma, raymarching, generative art,
//! noise fields) via `wgpu` with zero browser overhead, with automatic pipeline caching
//! and CPU procedural fallback.

#[cfg(feature = "gpu")]
use crate::backend::RasterError;
use image::RgbaImage;
#[cfg(feature = "gpu")]
use std::sync::Arc;
#[cfg(feature = "gpu")]
use wgpu::util::DeviceExt;

/// Uniform parameters supplied to fragment shaders (WGSL).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ShaderUniforms {
    /// Render target resolution `[width, height]`.
    pub resolution: [f32; 2],
    /// Elapsed playback time in seconds (equivalent to Shadertoy `iTime`).
    pub time: f32,
    /// Memory alignment padding.
    pub _pad: f32,
    /// 4-component uniform vector for user parameters (equivalent to `iParams`).
    pub params: [f32; 4],
}

/// Wraps WGSL shader snippet into a complete compilable module.
pub fn wrap_wgsl_shader(user_source: &str) -> String {
    let trimmed = user_source.trim();
    if trimmed.contains("@fragment") {
        if trimmed.contains("@vertex") {
            trimmed.to_string()
        } else {
            format!(
                r#"
struct Uniforms {{
    resolution: vec2<f32>,
    time: f32,
    _pad: f32,
    params: vec4<f32>,
}};
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VertexOutput {{
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {{
    var pos = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
    );
    let p = pos[vid];
    var out: VertexOutput;
    out.position = vec4<f32>(p, 0.0, 1.0);
    out.uv = vec2<f32>((p.x + 1.0) * 0.5, (1.0 - p.y) * 0.5);
    return out;
}}

{trimmed}
"#
            )
        }
    } else {
        // Wrap pixel expression / body in fs_main
        format!(
            r#"
struct Uniforms {{
    resolution: vec2<f32>,
    time: f32,
    _pad: f32,
    params: vec4<f32>,
}};
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VertexOutput {{
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {{
    var pos = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
    );
    let p = pos[vid];
    var out: VertexOutput;
    out.position = vec4<f32>(p, 0.0, 1.0);
    out.uv = vec2<f32>((p.x + 1.0) * 0.5, (1.0 - p.y) * 0.5);
    return out;
}}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {{
    let uv = in.uv;
    let time = u.time;
    let resolution = u.resolution;
    let params = u.params;
    {trimmed}
}}
"#
        )
    }
}

/// Renders a fast procedural plasma / wave fallback on CPU.
pub fn render_shader_cpu(width: u32, height: u32, time: f32, params: [f32; 4]) -> RgbaImage {
    let w = width.max(1);
    let h = height.max(1);
    let mut img = RgbaImage::new(w, h);
    let p0 = params[0].max(0.1);
    let p1 = params[1].max(0.1);
    let p2 = params[2].max(0.1);

    for (x, y, pixel) in img.enumerate_pixels_mut() {
        let nx = x as f32 / w as f32;
        let ny = y as f32 / h as f32;
        let v1 = (nx * 10.0 + time).sin();
        let v2 = ((ny * 10.0 + time * 1.3).sin() + (nx * ny * 5.0 + time).cos()) * 0.5;
        let r = (((v1 * 0.5 + 0.5) * p0) * 255.0).clamp(0.0, 255.0) as u8;
        let g = (((v2 * 0.5 + 0.5) * p1) * 255.0).clamp(0.0, 255.0) as u8;
        let b = (((((nx + ny) * 5.0 + time * 0.7).sin() * 0.5 + 0.5) * p2) * 255.0)
            .clamp(0.0, 255.0) as u8;
        *pixel = image::Rgba([r, g, b, 255]);
    }
    img
}

#[cfg(feature = "gpu")]
pub struct WgpuShaderRunner {
    device: std::sync::Arc<wgpu::Device>,
    queue: std::sync::Arc<wgpu::Queue>,
    uniform_layout: wgpu::BindGroupLayout,
    pipeline_cache: std::sync::Mutex<std::collections::HashMap<String, Arc<wgpu::RenderPipeline>>>,
}

#[cfg(feature = "gpu")]
impl WgpuShaderRunner {
    /// Create a new GPU shader runner.
    pub fn new() -> Result<Self, RasterError> {
        pollster::block_on(Self::new_async())
    }

    /// Async constructor for GPU shader runner.
    pub async fn new_async() -> Result<Self, RasterError> {
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
            .ok_or_else(|| RasterError::Init("No GPU adapter found for ShaderRunner".into()))?;

        let limits = adapter.limits();
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("rust-motion-shader-runner"),
                    required_features: wgpu::Features::empty(),
                    required_limits: limits,
                    memory_hints: Default::default(),
                },
                None,
            )
            .await
            .map_err(|e| RasterError::Init(format!("Failed to create GPU device: {e}")))?;

        Ok(Self::with_device_queue(
            std::sync::Arc::new(device),
            std::sync::Arc::new(queue),
        ))
    }

    /// Construct a shader runner on an existing WGPU device. Sharing the
    /// device is required before shader output can be rendered into a texture
    /// owned by the main scene compositor.
    pub fn from_device(
        device: &std::sync::Arc<wgpu::Device>,
        queue: &std::sync::Arc<wgpu::Queue>,
    ) -> Self {
        Self::with_device_queue(std::sync::Arc::clone(device), std::sync::Arc::clone(queue))
    }

    fn with_device_queue(
        device: std::sync::Arc<wgpu::Device>,
        queue: std::sync::Arc<wgpu::Queue>,
    ) -> Self {
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shader_uniform_layout"),
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

        Self {
            device,
            queue,
            uniform_layout,
            pipeline_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Render a single frame with the custom WGSL shader to an [`RgbaImage`].
    pub fn render(
        &self,
        width: u32,
        height: u32,
        time: f32,
        params: [f32; 4],
        source: &str,
    ) -> Result<RgbaImage, RasterError> {
        let wgsl = wrap_wgsl_shader(source);
        let mut cache = self
            .pipeline_cache
            .lock()
            .map_err(|_| RasterError::Init("Pipeline cache mutex poisoned".into()))?;

        let _pipeline = if let Some(p) = cache.get(&wgsl) {
            p.clone()
        } else {
            let shader_module = self
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("custom_wgsl_shader"),
                    source: wgpu::ShaderSource::Wgsl(wgsl.clone().into()),
                });

            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("custom_shader_pipeline_layout"),
                        bind_group_layouts: &[&self.uniform_layout],
                        push_constant_ranges: &[],
                    });

            let new_pipeline = Arc::new(self.device.create_render_pipeline(
                &wgpu::RenderPipelineDescriptor {
                    label: Some("custom_shader_pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader_module,
                        entry_point: "vs_main",
                        buffers: &[],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader_module,
                        entry_point: "fs_main",
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            blend: Some(wgpu::BlendState::REPLACE),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: Default::default(),
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        strip_index_format: None,
                        front_face: wgpu::FrontFace::Ccw,
                        cull_mode: None,
                        unclipped_depth: false,
                        polygon_mode: wgpu::PolygonMode::Fill,
                        conservative: false,
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                },
            ));

            cache.insert(wgsl, new_pipeline.clone());
            new_pipeline
        };
        drop(cache);

        // Prepare uniforms
        let uniforms = ShaderUniforms {
            resolution: [width as f32, height as f32],
            time,
            _pad: 0.0,
            params,
        };

        let uniform_bytes = unsafe {
            std::slice::from_raw_parts(
                &uniforms as *const ShaderUniforms as *const u8,
                std::mem::size_of::<ShaderUniforms>(),
            )
        };

        let uniform_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("shader_uniforms_buf"),
                contents: uniform_bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let _bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shader_uniforms_bg"),
            layout: &self.uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });

        // Texture target
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shader_target_tex"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("shader_encoder"),
            });

        self.render_into(
            &mut encoder,
            &view,
            width,
            height,
            ShaderRenderParams {
                time,
                params,
                source,
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            },
        )?;

        // Staging buffer readback
        let bytes_per_row = (width * 4 + 255) & !255;
        let buffer_size = (bytes_per_row * height) as wgpu::BufferAddress;
        let staging_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shader_readback_buf"),
            size: buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &staging_buf,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(Some(encoder.finish()));

        let slice = staging_buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|e| RasterError::Scene(format!("GPU channel recv failed: {e}")))?
            .map_err(|e| RasterError::Scene(format!("GPU buffer map failed: {e}")))?;

        let mapped = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for row in 0..height {
            let start = (row * bytes_per_row) as usize;
            let end = start + (width * 4) as usize;
            pixels.extend_from_slice(&mapped[start..end]);
        }
        drop(mapped);
        staging_buf.unmap();

        RgbaImage::from_raw(width, height, pixels).ok_or_else(|| {
            RasterError::ImageEncode("Failed to construct RgbaImage from shader output".into())
        })
    }

    /// Record a shader draw directly into an existing RGBA8 render target.
    /// The caller owns submission and may continue the same command encoder
    /// with other scene passes, enabling future zero-readback compositing.
    pub fn render_into(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        params: ShaderRenderParams<'_>,
    ) -> Result<(), RasterError> {
        self.render_into_region(
            encoder,
            target,
            ShaderRenderRegion::full_target(width, height),
            params,
        )
    }

    /// Record a shader into a rectangular viewport of an existing target.
    /// The shader receives the region dimensions and its interpolated UV stays
    /// local to the region, matching the standalone shader render semantics.
    pub fn render_into_region(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        region: ShaderRenderRegion,
        params: ShaderRenderParams<'_>,
    ) -> Result<(), RasterError> {
        let wgsl = wrap_wgsl_shader(params.source);
        let cache_key = format!("target:{wgsl}");
        let mut cache = self
            .pipeline_cache
            .lock()
            .map_err(|_| RasterError::Init("Pipeline cache mutex poisoned".into()))?;
        let pipeline = if let Some(pipeline) = cache.get(&cache_key) {
            pipeline.clone()
        } else {
            let module = self
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("custom_shader_module"),
                    source: wgpu::ShaderSource::Wgsl(wgsl.clone().into()),
                });
            let layout = self
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("custom_shader_layout"),
                    bind_group_layouts: &[&self.uniform_layout],
                    push_constant_ranges: &[],
                });
            let pipeline = Arc::new(self.device.create_render_pipeline(
                &wgpu::RenderPipelineDescriptor {
                    label: Some("custom_shader_target_pipeline"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: "vs_main",
                        buffers: &[],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: "fs_main",
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba8Unorm,
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
                },
            ));
            cache.insert(cache_key, pipeline.clone());
            pipeline
        };
        drop(cache);

        let uniforms = ShaderUniforms {
            resolution: [region.width, region.height],
            time: params.time,
            _pad: 0.0,
            params: params.params,
        };
        let uniform_bytes = unsafe {
            std::slice::from_raw_parts(
                &uniforms as *const ShaderUniforms as *const u8,
                std::mem::size_of::<ShaderUniforms>(),
            )
        };
        let uniform_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("shader_target_uniforms"),
                contents: uniform_bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shader_target_bind_group"),
            layout: &self.uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shader_target_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: params.load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_viewport(
            region.x.clamp(0.0, region.target_width as f32),
            region.y.clamp(0.0, region.target_height as f32),
            region.width.max(1.0).min(region.target_width as f32),
            region.height.max(1.0).min(region.target_height as f32),
            0.0,
            1.0,
        );
        pass.draw(0..6, 0..1);
        Ok(())
    }
}

/// Target dimensions and viewport region for a shader draw.
#[cfg(feature = "gpu")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShaderRenderRegion {
    pub target_width: u32,
    pub target_height: u32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[cfg(feature = "gpu")]
impl ShaderRenderRegion {
    /// Create a region covering the entire target dimensions.
    pub fn full_target(width: u32, height: u32) -> Self {
        Self {
            target_width: width,
            target_height: height,
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
        }
    }
}

/// Dynamic shader parameters, source code, and load operation.
#[cfg(feature = "gpu")]
#[derive(Debug, Clone, Copy)]
pub struct ShaderRenderParams<'a> {
    pub time: f32,
    pub params: [f32; 4],
    pub source: &'a str,
    pub load: wgpu::LoadOp<wgpu::Color>,
}
