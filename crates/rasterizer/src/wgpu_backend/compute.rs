//! GPU compute pipelines and dispatch helpers for separable blur and color matrix passes.

use super::*;
use crate::backend::RasterError;
use wgpu::util::DeviceExt;

const COMPUTE_WGSL: &str = r#"
struct BlurParams {
    width: u32,
    height: u32,
    radius: u32,
    axis: u32,
};

struct ColorMatrixParams {
    width: u32,
    height: u32,
    _pad0: u32,
    _pad1: u32,
    row0: vec4<f32>,
    row1: vec4<f32>,
    row2: vec4<f32>,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var dst_tex: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> blur_params: BlurParams;

@compute @workgroup_size(8, 8, 1)
fn cs_box_blur(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= blur_params.width || gid.y >= blur_params.height {
        return;
    }
    let r = i32(blur_params.radius);
    let kernel = 2u * blur_params.radius + 1u;
    var acc = vec4<u32>(0u);
    if blur_params.axis == 0u {
        let w = i32(blur_params.width);
        let y = i32(gid.y);
        let cx = i32(gid.x);
        for (var dx = -r; dx <= r; dx = dx + 1) {
            let sx = cx + dx;
            if sx >= 0 && sx < w {
                let px = textureLoad(src_tex, vec2<i32>(sx, y), 0);
                acc = acc + vec4<u32>(round(clamp(px, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0));
            }
        }
    } else {
        let h = i32(blur_params.height);
        let x = i32(gid.x);
        let cy = i32(gid.y);
        for (var dy = -r; dy <= r; dy = dy + 1) {
            let sy = cy + dy;
            if sy >= 0 && sy < h {
                let px = textureLoad(src_tex, vec2<i32>(x, sy), 0);
                acc = acc + vec4<u32>(round(clamp(px, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0));
            }
        }
    }
    let avg = acc / vec4<u32>(kernel);
    let out_f = vec4<f32>(avg) / 255.0;
    textureStore(dst_tex, vec2<i32>(gid.xy), out_f);
}

@group(0) @binding(0) var cm_src_tex: texture_2d<f32>;
@group(0) @binding(1) var cm_dst_tex: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> cm_params: ColorMatrixParams;

@compute @workgroup_size(8, 8, 1)
fn cs_color_matrix(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= cm_params.width || gid.y >= cm_params.height {
        return;
    }
    let px = textureLoad(cm_src_tex, vec2<i32>(gid.xy), 0);
    let rgb255 = px.rgb * 255.0;
    let a255 = px.a * 255.0;
    let r = clamp(round(dot(cm_params.row0.xyz, rgb255) + cm_params.row0.w * a255), 0.0, a255) / 255.0;
    let g = clamp(round(dot(cm_params.row1.xyz, rgb255) + cm_params.row1.w * a255), 0.0, a255) / 255.0;
    let b = clamp(round(dot(cm_params.row2.xyz, rgb255) + cm_params.row2.w * a255), 0.0, a255) / 255.0;
    textureStore(cm_dst_tex, vec2<i32>(gid.xy), vec4<f32>(r, g, b, px.a));
}
"#;

#[repr(C)]
#[derive(Clone, Copy)]
struct BlurUniforms {
    width: u32,
    height: u32,
    radius: u32,
    axis: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ColorMatrixUniforms {
    width: u32,
    height: u32,
    _pad0: u32,
    _pad1: u32,
    row0: [f32; 4],
    row1: [f32; 4],
    row2: [f32; 4],
}

pub(crate) struct GpuComputePipelines {
    pub(crate) bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) box_blur_pipeline: wgpu::ComputePipeline,
    pub(crate) color_matrix_pipeline: wgpu::ComputePipeline,
}

impl GpuComputePipelines {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rust_motion_compute_shader"),
            source: wgpu::ShaderSource::Wgsl(COMPUTE_WGSL.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("compute_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: RENDER_FORMAT,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("compute_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let box_blur_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("compute_box_blur_pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: "cs_box_blur",
            compilation_options: Default::default(),
            cache: None,
        });

        let color_matrix_pipeline =
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("compute_color_matrix_pipeline"),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: "cs_color_matrix",
                compilation_options: Default::default(),
                cache: None,
            });

        Self {
            bind_group_layout,
            box_blur_pipeline,
            color_matrix_pipeline,
        }
    }
}

impl WgpuBackend {
    pub(crate) fn create_compute_texture(
        &self,
        width: u32,
        height: u32,
        label: &str,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = self.ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: RENDER_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    pub(crate) fn upload_rgba_texture(
        &self,
        width: u32,
        height: u32,
        rgba: &[u8],
        label: &str,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let (texture, view) = self.create_compute_texture(width, height, label);
        self.ctx.queue.write_texture(
            texture.as_image_copy(),
            rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        (texture, view)
    }

    pub(crate) fn encode_box_blur_compute(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        src_view: &wgpu::TextureView,
        dst_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        radius: u32,
        vertical: bool,
    ) {
        let params = BlurUniforms {
            width,
            height,
            radius,
            axis: if vertical { 1 } else { 0 },
        };
        let uniform_buf = self
            .ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("compute_blur_uniforms"),
                contents: bytemuck_cast(&[params]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bg = self
            .ctx
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("compute_blur_bg"),
                layout: &self.ctx.compute.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(src_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(dst_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform_buf.as_entire_binding(),
                    },
                ],
            });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(if vertical {
                "compute_box_blur_v"
            } else {
                "compute_box_blur_h"
            }),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.ctx.compute.box_blur_pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
    }

    pub(crate) fn encode_color_matrix_compute(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        src_view: &wgpu::TextureView,
        dst_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        matrix: &[f32; 12],
    ) {
        let params = ColorMatrixUniforms {
            width,
            height,
            _pad0: 0,
            _pad1: 0,
            row0: [matrix[0], matrix[1], matrix[2], matrix[3]],
            row1: [matrix[4], matrix[5], matrix[6], matrix[7]],
            row2: [matrix[8], matrix[9], matrix[10], matrix[11]],
        };
        let uniform_buf = self
            .ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("compute_color_matrix_uniforms"),
                contents: bytemuck_cast(&[params]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bg = self
            .ctx
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("compute_color_matrix_bg"),
                layout: &self.ctx.compute.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(src_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(dst_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform_buf.as_entire_binding(),
                    },
                ],
            });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("compute_color_matrix"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.ctx.compute.color_matrix_pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
    }

    pub(crate) fn readback_texture(
        &self,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<image::RgbaImage, RasterError> {
        let bytes_per_row = align_to_256(width * 4);
        let staging = self.ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("compute_readback_buffer"),
            size: u64::from(bytes_per_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("compute_readback_encoder"),
            });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::ImageCopyBuffer {
                buffer: &staging,
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
        self.ctx.queue.submit(Some(encoder.finish()));
        let (tx, rx) = std::sync::mpsc::channel();
        staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.ctx.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|e| RasterError::Scene(format!("readback channel failed: {e}")))?
            .map_err(|e| RasterError::Scene(format!("readback map failed: {e}")))?;
        let mapped = staging.slice(..).get_mapped_range();
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        let row_bytes = (width * 4) as usize;
        for row in 0..height {
            let start = (row * bytes_per_row) as usize;
            pixels.extend_from_slice(&mapped[start..start + row_bytes]);
        }
        drop(mapped);
        staging.unmap();
        image::RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| RasterError::ImageEncode("invalid texture readback".into()))
    }
}
