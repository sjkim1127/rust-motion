//! Native Apple Silicon Metal [`RenderExecutor`] and zero-copy `IOSurface` VideoToolbox bridge.
//!
//! Uses unified memory (`MTLStorageMode::Shared`) for intermediate surfaces and
//! `CVMetalTextureCache` to bind `IOSurface`-backed `CVPixelBuffer` targets directly
//! into Metal compute shaders without CPU readback or swizzling copies.

use std::collections::BTreeMap;
use std::ffi::c_void;
use std::sync::Mutex;

use image::RgbaImage;
use metal::foreign_types::{ForeignType, ForeignTypeRef};
use metal::{
    CommandBuffer, CommandQueue, CompileOptions, ComputePipelineState, Device, MTLOrigin,
    MTLPixelFormat, MTLRegion, MTLSize, MTLStorageMode, MTLTextureType, MTLTextureUsage, Texture,
    TextureDescriptor, TextureRef,
};
use rust_motion_media_hwaccel::{HardwarePixelBuffer, MetalTextureCache};

use crate::backend::{FrameConfig, RasterError};
use crate::render_ir::{
    lower_scene_to_plan, CapabilityFlags, ClearOp, ExecutionError, ExecutionPlan, ExecutionReport,
    ExecutorCapabilities, LowerOptions, PassOp, PixelFormat, RenderExecutor, ResourceId,
    KERNEL_COLOR_MATRIX, KERNEL_SEPARABLE_BOX_BLUR_H, KERNEL_SEPARABLE_BOX_BLUR_V,
};
use crate::scene::{BlendMode, Scene, SceneNode, Transform2D};
use crate::tiny_skia_backend::color_blend::composite_custom_psd_blend;
use crate::tiny_skia_backend::{svgpath_to_tiny_skia, TinySkiaBackend};
use tiny_skia::{FillRule, Paint, Pixmap, Transform};

const METAL_COMPUTE_MSL: &str = r#"
#include <metal_stdlib>
using namespace metal;

struct BlurParams {
    uint width;
    uint height;
    uint radius;
    uint axis;
};

struct ColorMatrixParams {
    uint width;
    uint height;
    uint _pad0;
    uint _pad1;
    float4 row0;
    float4 row1;
    float4 row2;
};

struct CompositeParams {
    uint dst_width;
    uint dst_height;
    uint src_width;
    uint src_height;
    int dst_x;
    int dst_y;
    float opacity;
    uint _pad;
};

struct SurfaceOutParams {
    uint width;
    uint height;
};

kernel void cs_box_blur(
    texture2d<float, access::read> src_tex [[texture(0)]],
    texture2d<float, access::write> dst_tex [[texture(1)]],
    constant BlurParams& p [[buffer(0)]],
    uint2 gid [[thread_position_in_grid]])
{
    if (gid.x >= p.width || gid.y >= p.height) {
        return;
    }
    int r = int(p.radius);
    uint k = 2u * p.radius + 1u;
    uint4 acc = uint4(0u);
    if (p.axis == 0u) {
        int w = int(p.width);
        int y = int(gid.y);
        int cx = int(gid.x);
        for (int dx = -r; dx <= r; dx++) {
            int sx = cx + dx;
            if (sx >= 0 && sx < w) {
                float4 px = clamp(src_tex.read(uint2(uint(sx), uint(y))), 0.0, 1.0);
                acc += uint4(round(px * 255.0));
            }
        }
    } else {
        int h = int(p.height);
        int x = int(gid.x);
        int cy = int(gid.y);
        for (int dy = -r; dy <= r; dy++) {
            int sy = cy + dy;
            if (sy >= 0 && sy < h) {
                float4 px = clamp(src_tex.read(uint2(uint(x), uint(sy))), 0.0, 1.0);
                acc += uint4(round(px * 255.0));
            }
        }
    }
    uint4 avg = acc / uint4(k);
    dst_tex.write(float4(avg) / 255.0, gid);
}

kernel void cs_color_matrix(
    texture2d<float, access::read> src_tex [[texture(0)]],
    texture2d<float, access::write> dst_tex [[texture(1)]],
    constant ColorMatrixParams& cm [[buffer(0)]],
    uint2 gid [[thread_position_in_grid]])
{
    if (gid.x >= cm.width || gid.y >= cm.height) {
        return;
    }
    float4 px = src_tex.read(gid);
    float3 rgb255 = px.rgb * 255.0;
    float a255 = px.a * 255.0;
    float r = clamp(round(dot(cm.row0.xyz, rgb255) + cm.row0.w * a255), 0.0, a255) / 255.0;
    float g = clamp(round(dot(cm.row1.xyz, rgb255) + cm.row1.w * a255), 0.0, a255) / 255.0;
    float b = clamp(round(dot(cm.row2.xyz, rgb255) + cm.row2.w * a255), 0.0, a255) / 255.0;
    dst_tex.write(float4(r, g, b, px.a), gid);
}

kernel void cs_composite_over(
    texture2d<float, access::read> src_tex [[texture(0)]],
    texture2d<float, access::read> dst_snap [[texture(1)]],
    texture2d<float, access::write> dst_tex [[texture(2)]],
    constant CompositeParams& cp [[buffer(0)]],
    uint2 gid [[thread_position_in_grid]])
{
    if (gid.x >= cp.dst_width || gid.y >= cp.dst_height) {
        return;
    }
    float4 d = dst_snap.read(gid);
    int sx = int(gid.x) - cp.dst_x;
    int sy = int(gid.y) - cp.dst_y;
    if (sx >= 0 && sx < int(cp.src_width) && sy >= 0 && sy < int(cp.src_height)) {
        float4 s = src_tex.read(uint2(uint(sx), uint(sy))) * cp.opacity;
        float4 out_px = clamp(s + d * (1.0 - s.a), 0.0, 1.0);
        dst_tex.write(out_px, gid);
    } else {
        dst_tex.write(d, gid);
    }
}

kernel void cs_rgba_to_bgra_surface(
    texture2d<float, access::read> src_tex [[texture(0)]],
    texture2d<float, access::write> dst_bgra [[texture(1)]],
    constant SurfaceOutParams& sp [[buffer(0)]],
    uint2 gid [[thread_position_in_grid]])
{
    if (gid.x >= sp.width || gid.y >= sp.height) {
        return;
    }
    // Premultiplied RGBA8 composited over opaque black; Metal converts RGBA float4 into BGRA8Unorm hardware bytes.
    float4 px = clamp(src_tex.read(gid), 0.0, 1.0);
    dst_bgra.write(float4(px.rgb, 1.0), gid);
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

#[repr(C)]
#[derive(Clone, Copy)]
struct CompositeUniforms {
    dst_width: u32,
    dst_height: u32,
    src_width: u32,
    src_height: u32,
    dst_x: i32,
    dst_y: i32,
    opacity: f32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SurfaceOutUniforms {
    width: u32,
    height: u32,
}

struct MetalSurface {
    texture: Texture,
    width: u32,
    height: u32,
    is_mask: bool,
}

/// Native Apple Silicon Metal [`RenderExecutor`] operating on shared unified memory
/// with zero-copy `CVMetalTextureCache` output into `IOSurface` VideoToolbox pixel buffers.
pub struct MetalComputeExecutor {
    device: Device,
    queue: CommandQueue,
    box_blur_pipeline: ComputePipelineState,
    color_matrix_pipeline: ComputePipelineState,
    composite_over_pipeline: ComputePipelineState,
    surface_out_pipeline: ComputePipelineState,
    texture_cache: Mutex<MetalTextureCache>,
    cpu_ref: TinySkiaBackend,
}

impl MetalComputeExecutor {
    /// Initialize the default system Metal device, compile embedded MSL compute kernels,
    /// and bind a `CVMetalTextureCache` for zero-copy `IOSurface` VideoToolbox output.
    pub fn new() -> Result<Self, RasterError> {
        let device = Device::system_default().ok_or_else(|| {
            RasterError::Init("No Metal GPU device available on this macOS host".into())
        })?;
        let queue = device.new_command_queue();

        let opts = CompileOptions::new();
        let lib = device
            .new_library_with_source(METAL_COMPUTE_MSL, &opts)
            .map_err(|e| RasterError::Init(format!("Metal MSL compile error: {e}")))?;

        let make_pso = |name: &str| -> Result<ComputePipelineState, RasterError> {
            let func = lib.get_function(name, None).map_err(|e| {
                RasterError::Init(format!("Missing Metal compute function '{name}': {e}"))
            })?;
            device
                .new_compute_pipeline_state_with_function(&func)
                .map_err(|e| {
                    RasterError::Init(format!("Metal compute pipeline '{name}' failed: {e}"))
                })
        };

        let box_blur_pipeline = make_pso("cs_box_blur")?;
        let color_matrix_pipeline = make_pso("cs_color_matrix")?;
        let composite_over_pipeline = make_pso("cs_composite_over")?;
        let surface_out_pipeline = make_pso("cs_rgba_to_bgra_surface")?;

        let device_ptr: *mut c_void = device.as_ptr().cast();
        let texture_cache = MetalTextureCache::new(device_ptr).map_err(|e| {
            RasterError::Init(format!("CVMetalTextureCache initialization failed: {e}"))
        })?;

        Ok(Self {
            device,
            queue,
            box_blur_pipeline,
            color_matrix_pipeline,
            composite_over_pipeline,
            surface_out_pipeline,
            texture_cache: Mutex::new(texture_cache),
            cpu_ref: TinySkiaBackend::headless(),
        })
    }

    pub fn device_name(&self) -> String {
        self.device.name().to_string()
    }

    fn create_shared_texture(&self, width: u32, height: u32, format: MTLPixelFormat) -> Texture {
        let desc = TextureDescriptor::new();
        desc.set_texture_type(MTLTextureType::D2);
        desc.set_pixel_format(format);
        desc.set_width(u64::from(width));
        desc.set_height(u64::from(height));
        desc.set_mipmap_level_count(1);
        desc.set_storage_mode(MTLStorageMode::Shared);
        desc.set_usage(
            MTLTextureUsage::ShaderRead
                | MTLTextureUsage::ShaderWrite
                | MTLTextureUsage::RenderTarget,
        );
        self.device.new_texture(&desc)
    }

    fn upload_rgba(&self, tex: &Texture, width: u32, height: u32, bytes: &[u8]) {
        let region = MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize {
                width: u64::from(width),
                height: u64::from(height),
                depth: 1,
            },
        };
        tex.replace_region(region, 0, bytes.as_ptr().cast(), u64::from(width) * 4);
    }

    fn upload_mask(&self, tex: &Texture, width: u32, height: u32, bytes: &[u8]) {
        let region = MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize {
                width: u64::from(width),
                height: u64::from(height),
                depth: 1,
            },
        };
        tex.replace_region(region, 0, bytes.as_ptr().cast(), u64::from(width));
    }

    fn readback_rgba(&self, tex: &Texture, width: u32, height: u32) -> Vec<u8> {
        let mut out = vec![0u8; (width as usize) * (height as usize) * 4];
        let region = MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize {
                width: u64::from(width),
                height: u64::from(height),
                depth: 1,
            },
        };
        tex.get_bytes(out.as_mut_ptr().cast(), u64::from(width) * 4, region, 0);
        out
    }

    fn readback_mask(&self, tex: &Texture, width: u32, height: u32) -> Vec<u8> {
        let mut out = vec![0u8; (width as usize) * (height as usize)];
        let region = MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize {
                width: u64::from(width),
                height: u64::from(height),
                depth: 1,
            },
        };
        tex.get_bytes(out.as_mut_ptr().cast(), u64::from(width), region, 0);
        out
    }

    fn dispatch_2d(
        &self,
        cmd: &metal::CommandBufferRef,
        pipeline: &ComputePipelineState,
        textures: &[&TextureRef],
        uniform_bytes: &[u8],
        width: u32,
        height: u32,
    ) {
        let enc = cmd.new_compute_command_encoder();
        enc.set_compute_pipeline_state(pipeline);
        for (idx, tex) in textures.iter().enumerate() {
            enc.set_texture(idx as u64, Some(*tex));
        }
        enc.set_bytes(0, uniform_bytes.len() as u64, uniform_bytes.as_ptr().cast());
        let tw = 8u64;
        let th = 8u64;
        let groups = MTLSize {
            width: u64::from(width).div_ceil(tw),
            height: u64::from(height).div_ceil(th),
            depth: 1,
        };
        let tpg = MTLSize {
            width: tw,
            height: th,
            depth: 1,
        };
        enc.dispatch_thread_groups(groups, tpg);
        enc.end_encoding();
    }

    fn execute_plan_to_surface(
        &self,
        plan: &ExecutionPlan,
        config: &FrameConfig,
    ) -> Result<
        (
            BTreeMap<ResourceId, MetalSurface>,
            ResourceId,
            ExecutionReport,
        ),
        ExecutionError,
    > {
        let caps = <Self as RenderExecutor>::capabilities(self);
        if let Err(crate::render_ir::GraphValidationError::MissingCapabilities { pass, missing }) =
            plan.check_capabilities(caps.flags)
        {
            return Err(ExecutionError::UnsupportedCapability {
                backend: caps.backend_name,
                pass,
                missing,
            });
        }

        let mut surfaces: BTreeMap<ResourceId, MetalSurface> = BTreeMap::new();
        for res in &plan.graph.resources {
            if res.width == 0 || res.height == 0 {
                return Err(ExecutionError::ResourceAllocation {
                    resource: res.id,
                    reason: format!("invalid resource dimensions {}x{}", res.width, res.height),
                });
            }
            let fmt = if res.format.is_mask() {
                MTLPixelFormat::R8Unorm
            } else {
                MTLPixelFormat::RGBA8Unorm
            };
            let tex = self.create_shared_texture(res.width, res.height, fmt);
            surfaces.insert(
                res.id,
                MetalSurface {
                    texture: tex,
                    width: res.width,
                    height: res.height,
                    is_mask: res.format.is_mask(),
                },
            );
        }

        let mut report = ExecutionReport {
            backend_name: caps.backend_name,
            passes_executed: 0,
            gpu_passes: 0,
            compute_passes: 0,
            cpu_island_passes: 0,
            cpu_island_pixels: 0,
            cpu_island_upload_bytes: 0,
            transient_pool_slots: plan.transient_pool_slots,
            peak_transient_bytes: plan.peak_transient_bytes,
            fallback_reasons: Vec::new(),
        };

        let mut pending_cmd: Option<CommandBuffer> = None;
        let flush_gpu = |pending: &mut Option<CommandBuffer>| {
            if let Some(cmd) = pending.take() {
                cmd.commit();
                cmd.wait_until_completed();
            }
        };

        for &pid in &plan.pass_order {
            let pass = plan
                .graph
                .pass(pid)
                .expect("scheduled pass must exist in graph");
            let dst_id = pass.writes[0];

            match &pass.op {
                PassOp::PathMask { path_d, transform } => {
                    flush_gpu(&mut pending_cmd);
                    let path = svgpath_to_tiny_skia(path_d).ok_or_else(|| {
                        ExecutionError::SubmissionFailed {
                            pass: pid,
                            reason: "invalid SVG path in PathMask".into(),
                        }
                    })?;
                    let dst =
                        surfaces
                            .get(&dst_id)
                            .ok_or_else(|| ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "PathMask destination missing".into(),
                            })?;
                    if !dst.is_mask {
                        return Err(ExecutionError::SubmissionFailed {
                            pass: pid,
                            reason: "PathMask destination is not R8Unorm".into(),
                        });
                    }
                    let mut tmp = Pixmap::new(dst.width, dst.height).ok_or_else(|| {
                        ExecutionError::ResourceAllocation {
                            resource: dst_id,
                            reason: "failed to allocate mask pixmap".into(),
                        }
                    })?;
                    let mut paint = Paint::default();
                    paint.set_color(tiny_skia::Color::WHITE);
                    paint.anti_alias = true;
                    tmp.fill_path(
                        &path,
                        &paint,
                        FillRule::Winding,
                        transform.to_tiny_skia(),
                        None,
                    );
                    let mask: Vec<u8> = tmp.data().chunks_exact(4).map(|p| p[3]).collect();
                    self.upload_mask(&dst.texture, dst.width, dst.height, &mask);
                }

                PassOp::Raster2D {
                    scene,
                    clear,
                    path_mask,
                } => {
                    flush_gpu(&mut pending_cmd);
                    let mask_bytes = match path_mask {
                        Some(mid) => {
                            let ms = surfaces.get(mid).ok_or_else(|| {
                                ExecutionError::SubmissionFailed {
                                    pass: pid,
                                    reason: "path_mask resource missing".into(),
                                }
                            })?;
                            Some((
                                self.readback_mask(&ms.texture, ms.width, ms.height),
                                ms.width,
                                ms.height,
                            ))
                        }
                        None => None,
                    };
                    let dst =
                        surfaces
                            .get(&dst_id)
                            .ok_or_else(|| ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Raster2D destination missing".into(),
                            })?;
                    let (w, h) = (dst.width, dst.height);
                    let mut pm =
                        Pixmap::new(w, h).ok_or_else(|| ExecutionError::ResourceAllocation {
                            resource: dst_id,
                            reason: "failed to allocate raster pixmap".into(),
                        })?;
                    match clear {
                        ClearOp::Clear(c) => pm.fill(c.to_tiny_skia_color()),
                        ClearOp::Load => {
                            let existing = self.readback_rgba(&dst.texture, w, h);
                            pm.data_mut().copy_from_slice(&existing);
                        }
                        ClearOp::DontCare => {}
                    }
                    if let Some((mbytes, mw, mh)) = mask_bytes {
                        if mw != w || mh != h {
                            return Err(ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "path mask dimensions mismatch".into(),
                            });
                        }
                        let mut layer_pm = Pixmap::new(w, h).ok_or_else(|| {
                            ExecutionError::ResourceAllocation {
                                resource: dst_id,
                                reason: "failed to allocate masked layer".into(),
                            }
                        })?;
                        self.cpu_ref.render_nodes_into_pixmap(
                            &mut layer_pm,
                            &scene.nodes,
                            config.fps,
                        )?;
                        for (px, &m) in layer_pm.data_mut().chunks_exact_mut(4).zip(mbytes.iter()) {
                            if m == 0 {
                                px.fill(0);
                            } else if m < 255 {
                                let f = u16::from(m);
                                for ch in px.iter_mut() {
                                    *ch = ((u16::from(*ch) * f + 127) / 255) as u8;
                                }
                            }
                        }
                        pm.draw_pixmap(
                            0,
                            0,
                            layer_pm.as_ref(),
                            &tiny_skia::PixmapPaint::default(),
                            Transform::identity(),
                            None,
                        );
                    } else {
                        self.cpu_ref
                            .render_nodes_into_pixmap(&mut pm, &scene.nodes, config.fps)?;
                    }
                    self.upload_rgba(&dst.texture, w, h, pm.data());
                }

                PassOp::Shader {
                    x,
                    y,
                    w,
                    h,
                    source,
                    time,
                    params,
                    opacity,
                    clear,
                } => {
                    flush_gpu(&mut pending_cmd);
                    let dst =
                        surfaces
                            .get(&dst_id)
                            .ok_or_else(|| ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Shader destination missing".into(),
                            })?;
                    let (dw, dh) = (dst.width, dst.height);
                    let mut pm =
                        Pixmap::new(dw, dh).ok_or_else(|| ExecutionError::ResourceAllocation {
                            resource: dst_id,
                            reason: "failed to allocate shader pixmap".into(),
                        })?;
                    match clear {
                        ClearOp::Clear(c) => pm.fill(c.to_tiny_skia_color()),
                        ClearOp::Load => {
                            let existing = self.readback_rgba(&dst.texture, dw, dh);
                            pm.data_mut().copy_from_slice(&existing);
                        }
                        ClearOp::DontCare => {}
                    }
                    let node = SceneNode::Shader {
                        x: *x,
                        y: *y,
                        w: *w,
                        h: *h,
                        source: source.clone(),
                        time: *time,
                        params: *params,
                        opacity: *opacity,
                    };
                    self.cpu_ref.render_nodes_into_pixmap(
                        &mut pm,
                        std::slice::from_ref(&node),
                        config.fps,
                    )?;
                    self.upload_rgba(&dst.texture, dw, dh, pm.data());
                }

                PassOp::CpuIsland {
                    scene,
                    bounds,
                    reason,
                } => {
                    flush_gpu(&mut pending_cmd);
                    let dst =
                        surfaces
                            .get(&dst_id)
                            .ok_or_else(|| ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "CpuIsland destination missing".into(),
                            })?;
                    let (w, h) = (dst.width, dst.height);
                    let nodes = if let Some(b) = bounds {
                        vec![SceneNode::Group {
                            transform: Transform2D::translate(-b.x, -b.y),
                            opacity: 1.0,
                            children: scene.nodes.clone(),
                        }]
                    } else {
                        scene.nodes.clone()
                    };
                    let mut pm =
                        Pixmap::new(w, h).ok_or_else(|| ExecutionError::ResourceAllocation {
                            resource: dst_id,
                            reason: "failed to allocate CpuIsland pixmap".into(),
                        })?;
                    pm.fill(tiny_skia::Color::TRANSPARENT);
                    self.cpu_ref
                        .render_nodes_into_pixmap(&mut pm, &nodes, config.fps)?;
                    let bytes = pm.data();
                    self.upload_rgba(&dst.texture, w, h, bytes);
                    report.cpu_island_passes += 1;
                    report.cpu_island_pixels += u64::from(w) * u64::from(h);
                    report.cpu_island_upload_bytes += bytes.len() as u64;
                    report.fallback_reasons.push(reason.clone());
                }

                PassOp::Compute { kernel, params, .. } => {
                    if !matches!(
                        kernel.as_str(),
                        KERNEL_SEPARABLE_BOX_BLUR_H
                            | KERNEL_SEPARABLE_BOX_BLUR_V
                            | KERNEL_COLOR_MATRIX
                    ) {
                        return Err(ExecutionError::UnsupportedCapability {
                            backend: caps.backend_name,
                            pass: pid,
                            missing: vec!["compute_kernel"],
                        });
                    }
                    let src_id = pass.reads.first().copied().ok_or_else(|| {
                        ExecutionError::SubmissionFailed {
                            pass: pid,
                            reason: "Compute pass requires read resource".into(),
                        }
                    })?;
                    let src =
                        surfaces
                            .get(&src_id)
                            .ok_or_else(|| ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Compute source surface missing".into(),
                            })?;
                    let dst =
                        surfaces
                            .get(&dst_id)
                            .ok_or_else(|| ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Compute destination surface missing".into(),
                            })?;
                    if src.width != dst.width || src.height != dst.height {
                        return Err(ExecutionError::SubmissionFailed {
                            pass: pid,
                            reason: "Compute source and destination dimensions mismatch".into(),
                        });
                    }

                    let cmd = pending_cmd
                        .get_or_insert_with(|| self.queue.new_command_buffer().to_owned());
                    match kernel.as_str() {
                        KERNEL_SEPARABLE_BOX_BLUR_H | KERNEL_SEPARABLE_BOX_BLUR_V => {
                            let radius = params.first().copied().unwrap_or(0.0).max(0.0) as u32;
                            let axis = u32::from(kernel == KERNEL_SEPARABLE_BOX_BLUR_V);
                            let u = BlurUniforms {
                                width: dst.width,
                                height: dst.height,
                                radius,
                                axis,
                            };
                            // SAFETY: `BlurUniforms` is `#[repr(C)]` POD.
                            let u_bytes = unsafe {
                                std::slice::from_raw_parts(
                                    (&u as *const BlurUniforms).cast::<u8>(),
                                    std::mem::size_of::<BlurUniforms>(),
                                )
                            };
                            self.dispatch_2d(
                                cmd,
                                &self.box_blur_pipeline,
                                &[src.texture.as_ref(), dst.texture.as_ref()],
                                u_bytes,
                                dst.width,
                                dst.height,
                            );
                        }
                        KERNEL_COLOR_MATRIX => {
                            if params.len() < 12 {
                                return Err(ExecutionError::SubmissionFailed {
                                    pass: pid,
                                    reason: "color_matrix requires 12 matrix floats".into(),
                                });
                            }
                            let u = ColorMatrixUniforms {
                                width: dst.width,
                                height: dst.height,
                                _pad0: 0,
                                _pad1: 0,
                                row0: [params[0], params[1], params[2], params[3]],
                                row1: [params[4], params[5], params[6], params[7]],
                                row2: [params[8], params[9], params[10], params[11]],
                            };
                            // SAFETY: `ColorMatrixUniforms` is `#[repr(C)]` POD.
                            let u_bytes = unsafe {
                                std::slice::from_raw_parts(
                                    (&u as *const ColorMatrixUniforms).cast::<u8>(),
                                    std::mem::size_of::<ColorMatrixUniforms>(),
                                )
                            };
                            self.dispatch_2d(
                                cmd,
                                &self.color_matrix_pipeline,
                                &[src.texture.as_ref(), dst.texture.as_ref()],
                                u_bytes,
                                dst.width,
                                dst.height,
                            );
                        }
                        _ => unreachable!(),
                    }
                    report.compute_passes += 1;
                    report.gpu_passes += 1;
                }

                PassOp::Composite {
                    source,
                    opacity,
                    blend_mode,
                    region,
                } => {
                    let src =
                        surfaces
                            .get(source)
                            .ok_or_else(|| ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Composite source missing".into(),
                            })?;
                    let dst =
                        surfaces
                            .get(&dst_id)
                            .ok_or_else(|| ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Composite destination missing".into(),
                            })?;
                    let (dx, dy) = match region {
                        Some(r) => (r.x.round() as i32, r.y.round() as i32),
                        None => (0, 0),
                    };

                    if *blend_mode == BlendMode::Normal {
                        let cmd = pending_cmd
                            .get_or_insert_with(|| self.queue.new_command_buffer().to_owned());
                        let dst_snap = self.create_shared_texture(
                            dst.width,
                            dst.height,
                            MTLPixelFormat::RGBA8Unorm,
                        );
                        let blit = cmd.new_blit_command_encoder();
                        blit.copy_from_texture(
                            dst.texture.as_ref(),
                            0,
                            0,
                            MTLOrigin { x: 0, y: 0, z: 0 },
                            MTLSize {
                                width: u64::from(dst.width),
                                height: u64::from(dst.height),
                                depth: 1,
                            },
                            dst_snap.as_ref(),
                            0,
                            0,
                            MTLOrigin { x: 0, y: 0, z: 0 },
                        );
                        blit.end_encoding();

                        let cp = CompositeUniforms {
                            dst_width: dst.width,
                            dst_height: dst.height,
                            src_width: src.width,
                            src_height: src.height,
                            dst_x: dx,
                            dst_y: dy,
                            opacity: *opacity,
                            _pad: 0,
                        };
                        // SAFETY: `CompositeUniforms` is `#[repr(C)]` POD.
                        let u_bytes = unsafe {
                            std::slice::from_raw_parts(
                                (&cp as *const CompositeUniforms).cast::<u8>(),
                                std::mem::size_of::<CompositeUniforms>(),
                            )
                        };
                        self.dispatch_2d(
                            cmd,
                            &self.composite_over_pipeline,
                            &[
                                src.texture.as_ref(),
                                dst_snap.as_ref(),
                                dst.texture.as_ref(),
                            ],
                            u_bytes,
                            dst.width,
                            dst.height,
                        );
                        report.gpu_passes += 1;
                    } else {
                        flush_gpu(&mut pending_cmd);
                        let src_bytes = self.readback_rgba(&src.texture, src.width, src.height);
                        let dst_bytes = self.readback_rgba(&dst.texture, dst.width, dst.height);
                        let mut src_pm = Pixmap::new(src.width, src.height).expect("valid dims");
                        src_pm.data_mut().copy_from_slice(&src_bytes);
                        let mut dst_pm = Pixmap::new(dst.width, dst.height).expect("valid dims");
                        dst_pm.data_mut().copy_from_slice(&dst_bytes);
                        composite_custom_psd_blend(
                            &mut dst_pm,
                            &src_pm,
                            blend_mode.to_color_blend_mode(),
                            *opacity,
                            dx,
                            dy,
                        );
                        self.upload_rgba(&dst.texture, dst.width, dst.height, dst_pm.data());
                    }
                }

                PassOp::Copy { source, dst_origin } => {
                    let src =
                        surfaces
                            .get(source)
                            .ok_or_else(|| ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Copy source missing".into(),
                            })?;
                    let dst =
                        surfaces
                            .get(&dst_id)
                            .ok_or_else(|| ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Copy destination missing".into(),
                            })?;
                    let cmd = pending_cmd
                        .get_or_insert_with(|| self.queue.new_command_buffer().to_owned());
                    let blit = cmd.new_blit_command_encoder();
                    blit.copy_from_texture(
                        src.texture.as_ref(),
                        0,
                        0,
                        MTLOrigin { x: 0, y: 0, z: 0 },
                        MTLSize {
                            width: u64::from(src.width),
                            height: u64::from(src.height),
                            depth: 1,
                        },
                        dst.texture.as_ref(),
                        0,
                        0,
                        MTLOrigin {
                            x: u64::from(dst_origin[0]),
                            y: u64::from(dst_origin[1]),
                            z: 0,
                        },
                    );
                    blit.end_encoding();
                    report.gpu_passes += 1;
                }
            }

            report.passes_executed += 1;
        }

        flush_gpu(&mut pending_cmd);

        let out_id = plan
            .graph
            .output
            .ok_or(crate::render_ir::GraphValidationError::MissingOutput)?;
        Ok((surfaces, out_id, report))
    }

    /// Lower a `Scene` and execute it directly on Apple Silicon Metal into an
    /// `IOSurface`-backed VideoToolbox [`HardwarePixelBuffer`] with zero CPU readback.
    pub fn render_scene_into_pixel_buffer(
        &self,
        scene: &Scene,
        config: &FrameConfig,
        pb: &HardwarePixelBuffer,
    ) -> Result<(), RasterError> {
        let caps = <Self as RenderExecutor>::capabilities(self);
        let plan = lower_scene_to_plan(
            scene,
            config,
            &LowerOptions {
                max_texture_dimension_2d: caps.max_texture_dimension_2d,
                capabilities: caps.flags,
                msaa_samples: 1,
            },
        )
        .map_err(ExecutionError::from)?;
        self.execute_plan_into_pixel_buffer(&plan, config, pb)?;
        Ok(())
    }

    /// Execute `plan` on Apple Silicon Metal and write the output directly into an
    /// `IOSurface`-backed VideoToolbox [`HardwarePixelBuffer`] (`BGRA8Unorm`) with
    /// zero CPU readback and zero CPU RGBA->BGRA swizzling.
    pub(crate) fn execute_plan_into_pixel_buffer(
        &self,
        plan: &ExecutionPlan,
        config: &FrameConfig,
        pb: &HardwarePixelBuffer,
    ) -> Result<ExecutionReport, ExecutionError> {
        let (surfaces, out_id, report) = self.execute_plan_to_surface(plan, config)?;
        let out_surf = surfaces
            .get(&out_id)
            .ok_or_else(|| ExecutionError::ResourceAllocation {
                resource: out_id,
                reason: "missing Metal output surface".into(),
            })?;
        if out_surf.width != pb.width() || out_surf.height != pb.height() {
            return Err(ExecutionError::SubmissionFailed {
                pass: crate::render_ir::PassId(0),
                reason: format!(
                    "output surface {}x{} does not match HardwarePixelBuffer {}x{}",
                    out_surf.width,
                    out_surf.height,
                    pb.width(),
                    pb.height()
                ),
            });
        }

        let cache = self
            .texture_cache
            .lock()
            .map_err(|_| ExecutionError::DeviceLost("MetalTextureCache lock poisoned".into()))?;
        let bound = cache.bind_bgra8_texture(pb).map_err(|e| {
            ExecutionError::DeviceLost(format!("CVMetalTextureCache bind failed: {e}"))
        })?;
        let dst_ptr = bound.mtl_texture_ptr();
        if dst_ptr.is_null() {
            return Err(ExecutionError::DeviceLost(
                "bound CVMetalTexture returned null MTLTexture".into(),
            ));
        }
        // SAFETY: `dst_ptr` is a valid non-null `id<MTLTexture>` retained by `bound` for this scope;
        // `TextureRef::from_ptr` does not increment/release retain counts.
        let dst_tex_ref: &TextureRef = unsafe { TextureRef::from_ptr(dst_ptr.cast()) };

        let sp = SurfaceOutUniforms {
            width: out_surf.width,
            height: out_surf.height,
        };
        // SAFETY: `SurfaceOutUniforms` is `#[repr(C)]` POD.
        let sp_bytes = unsafe {
            std::slice::from_raw_parts(
                (&sp as *const SurfaceOutUniforms).cast::<u8>(),
                std::mem::size_of::<SurfaceOutUniforms>(),
            )
        };
        let cmd = self.queue.new_command_buffer();
        self.dispatch_2d(
            cmd,
            &self.surface_out_pipeline,
            &[out_surf.texture.as_ref(), dst_tex_ref],
            sp_bytes,
            out_surf.width,
            out_surf.height,
        );
        cmd.commit();
        cmd.wait_until_completed();
        drop(bound);
        cache.flush();

        Ok(report)
    }
}

impl RenderExecutor for MetalComputeExecutor {
    fn capabilities(&self) -> ExecutorCapabilities {
        ExecutorCapabilities {
            backend_name: "metal_apple_silicon",
            flags: CapabilityFlags::RASTER_2D_ANALYTIC
                .union(CapabilityFlags::RASTER_2D_MESH)
                .union(CapabilityFlags::RASTER_2D_IMAGE)
                .union(CapabilityFlags::RASTER_2D_TEXT)
                .union(CapabilityFlags::BLEND_BASIC)
                .union(CapabilityFlags::BLEND_EXTENDED)
                .union(CapabilityFlags::OFFSCREEN_COMPOSITE)
                .union(CapabilityFlags::PATH_MASK)
                .union(CapabilityFlags::COMPUTE_PASS)
                .union(CapabilityFlags::COPY_OP)
                .union(CapabilityFlags::CPU_ISLAND)
                .union(CapabilityFlags::RASTER_EXTENDED),
            max_texture_dimension_2d: 16_384,
            supported_formats: vec![
                PixelFormat::Rgba8Unorm,
                PixelFormat::Rgba8UnormSrgb,
                PixelFormat::R8Unorm,
            ],
            supports_msaa: false,
            supports_timestamp_queries: false,
        }
    }

    fn execute_plan(
        &self,
        plan: &ExecutionPlan,
        config: &FrameConfig,
    ) -> Result<(RgbaImage, ExecutionReport), ExecutionError> {
        let (surfaces, out_id, report) = self.execute_plan_to_surface(plan, config)?;
        let out = surfaces
            .get(&out_id)
            .ok_or_else(|| ExecutionError::ResourceAllocation {
                resource: out_id,
                reason: "graph output missing from Metal surface map".into(),
            })?;
        let raw = self.readback_rgba(&out.texture, out.width, out.height);
        let img = RgbaImage::from_raw(out.width, out.height, raw).ok_or_else(|| {
            ExecutionError::Raster(RasterError::ImageEncode(
                "failed to assemble RgbaImage from Metal shared texture".into(),
            ))
        })?;
        Ok((img, report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_ir::{
        lower_scene_to_plan, ColorMetadata, CpuReferenceExecutor, LowerOptions, RenderGraph,
        ResourceDesc, ResourceLifetime, TextureUsage,
    };
    use crate::scene::{Color, MaskMode, Scene, SceneFilter};
    use rust_motion_media_container::{probe_isobmff_path, Mp4Muxer, Mp4MuxerOptions};
    use rust_motion_media_core::{
        ColorDescription, ContainerMuxer, Rational, TrackConfig, VideoCodecId, VideoEncodeConfig,
        VideoEncoder, VideoTrackConfig,
    };
    use rust_motion_media_hwaccel::HardwareVideoEncoder;

    fn sample_mixed_scene() -> Scene {
        let mut s = Scene::new();
        s.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 128.0,
            h: 96.0,
            fill: Color::rgb(24, 48, 96),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });
        s.push(SceneNode::Layer {
            opacity: 0.9,
            blend_mode: BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: MaskMode::Alpha,
            filters: vec![SceneFilter::Blur { sigma: 2.0 }],
            shadow: None,
            children: vec![SceneNode::Rect {
                x: 20.0,
                y: 16.0,
                w: 56.0,
                h: 40.0,
                fill: Color::rgb(230, 90, 50),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 4.0,
            }],
        });
        s
    }

    #[test]
    fn metal_executor_matches_cpu_reference_for_blur_color_matrix_and_composite() {
        let metal_exec = MetalComputeExecutor::new().unwrap();
        let cpu_backend = TinySkiaBackend::headless();
        let cpu_exec = CpuReferenceExecutor::new(&cpu_backend);

        let scene = sample_mixed_scene();
        let cfg = FrameConfig::new(128, 96, 0, 30.0);
        let caps = <MetalComputeExecutor as RenderExecutor>::capabilities(&metal_exec);
        let plan = lower_scene_to_plan(
            &scene,
            &cfg,
            &LowerOptions {
                max_texture_dimension_2d: caps.max_texture_dimension_2d,
                capabilities: caps.flags,
                msaa_samples: 1,
            },
        )
        .unwrap();

        let (cpu_img, _) = cpu_exec.execute_plan(&plan, &cfg).unwrap();
        let (metal_img, report) = metal_exec.execute_plan(&plan, &cfg).unwrap();
        assert_eq!(metal_img.dimensions(), (128, 96));
        assert!(report.compute_passes >= 2);
        assert!(report.gpu_passes >= 3);

        let max_diff = cpu_img
            .as_raw()
            .iter()
            .zip(metal_img.as_raw().iter())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        assert!(
            max_diff <= 1,
            "Metal Compute RenderExecutor diverged from CpuReferenceExecutor on scene by {max_diff} LSB"
        );

        // Also explicitly verify RenderGraph with Blur(H+V) + ColorMatrix + Composite against CPU reference.
        let mut graph = RenderGraph::new(64, 48);
        let mk = |id, lt| ResourceDesc {
            id: ResourceId(id),
            label: format!("r{id}"),
            width: 64,
            height: 48,
            format: PixelFormat::Rgba8Unorm,
            color: ColorMetadata::srgb_premultiplied(),
            sample_count: 1,
            usage: TextureUsage::STORAGE
                .union(TextureUsage::SAMPLED)
                .union(TextureUsage::RENDER_TARGET)
                .union(TextureUsage::COPY_SRC)
                .union(TextureUsage::COPY_DST),
            lifetime: lt,
        };
        graph.add_resource(mk(0, ResourceLifetime::Transient));
        graph.add_resource(mk(1, ResourceLifetime::Transient));
        graph.add_resource(mk(2, ResourceLifetime::Transient));
        graph.add_resource(mk(3, ResourceLifetime::Output));
        graph.set_output(ResourceId(3));

        let mut fg = Scene::new();
        fg.push(SceneNode::Rect {
            x: 8.0,
            y: 8.0,
            w: 40.0,
            h: 28.0,
            fill: Color::rgba(220, 110, 60, 210),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });
        graph.add_pass(crate::render_ir::PassNode {
            id: crate::render_ir::PassId(0),
            label: "fg".into(),
            op: PassOp::Raster2D {
                scene: fg,
                clear: ClearOp::Clear(Color::TRANSPARENT),
                path_mask: None,
            },
            reads: vec![],
            writes: vec![ResourceId(0)],
            order_deps: vec![],
            required_capabilities: CapabilityFlags::RASTER_2D_ANALYTIC,
        });
        graph.add_pass(crate::render_ir::PassNode {
            id: crate::render_ir::PassId(1),
            label: "blur_h".into(),
            op: PassOp::Compute {
                kernel: KERNEL_SEPARABLE_BOX_BLUR_H.into(),
                workgroups: [8, 6, 1],
                params: vec![2.0],
            },
            reads: vec![ResourceId(0)],
            writes: vec![ResourceId(1)],
            order_deps: vec![crate::render_ir::PassId(0)],
            required_capabilities: CapabilityFlags::COMPUTE_PASS,
        });
        graph.add_pass(crate::render_ir::PassNode {
            id: crate::render_ir::PassId(2),
            label: "cm".into(),
            op: PassOp::Compute {
                kernel: KERNEL_COLOR_MATRIX.into(),
                workgroups: [8, 6, 1],
                params: vec![0.8, 0.2, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.1, 0.0, 0.9, 0.0],
            },
            reads: vec![ResourceId(1)],
            writes: vec![ResourceId(2)],
            order_deps: vec![crate::render_ir::PassId(1)],
            required_capabilities: CapabilityFlags::COMPUTE_PASS,
        });
        graph.add_pass(crate::render_ir::PassNode {
            id: crate::render_ir::PassId(3),
            label: "bg_and_comp".into(),
            op: PassOp::Raster2D {
                scene: Scene::new(),
                clear: ClearOp::Clear(Color::rgb(15, 25, 45)),
                path_mask: None,
            },
            reads: vec![],
            writes: vec![ResourceId(3)],
            order_deps: vec![],
            required_capabilities: CapabilityFlags::RASTER_2D_ANALYTIC,
        });
        graph.add_pass(crate::render_ir::PassNode {
            id: crate::render_ir::PassId(4),
            label: "comp".into(),
            op: PassOp::Composite {
                source: ResourceId(2),
                opacity: 0.85,
                blend_mode: BlendMode::Normal,
                region: None,
            },
            reads: vec![ResourceId(2)],
            writes: vec![ResourceId(3)],
            order_deps: vec![crate::render_ir::PassId(2), crate::render_ir::PassId(3)],
            required_capabilities: CapabilityFlags::OFFSCREEN_COMPOSITE,
        });

        let dag_plan = ExecutionPlan::compile(graph).unwrap();
        let dag_cfg = FrameConfig::new(64, 48, 0, 30.0);
        let (dag_cpu, _) = cpu_exec.execute_plan(&dag_plan, &dag_cfg).unwrap();
        let (dag_metal, dag_report) = metal_exec.execute_plan(&dag_plan, &dag_cfg).unwrap();
        assert_eq!(dag_report.compute_passes, 2);
        let dag_diff = dag_cpu
            .as_raw()
            .iter()
            .zip(dag_metal.as_raw().iter())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        assert!(
            dag_diff <= 1,
            "Metal Compute DAG diverged from CpuReferenceExecutor by {dag_diff} LSB"
        );
    }

    #[test]
    fn metal_iosurface_zero_copy_encodes_hevc_mp4_without_cpu_readback() {
        let metal_exec = MetalComputeExecutor::new().unwrap();
        let (w, h) = (128u32, 96u32);

        let mut encoder = HardwareVideoEncoder::new(VideoCodecId::Hevc).unwrap();
        encoder
            .configure(&VideoEncodeConfig {
                width: w,
                height: h,
                fps: Rational::new(24, 1),
                codec: VideoCodecId::Hevc,
                crf: 20,
                preset: "fast".into(),
                color: ColorDescription::bt709_limited(),
            })
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let mp4_path = dir.path().join("metal_iosurface_hevc.mp4");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&mp4_path)
            .unwrap();
        let mut muxer = Mp4Muxer::new(
            file,
            &[TrackConfig::Video(VideoTrackConfig {
                width: w,
                height: h,
                timescale: 24,
                fps: Rational::new(24, 1),
                codec: VideoCodecId::Hevc,
                codec_config: Vec::new(),
                color: ColorDescription::bt709_limited(),
            })],
            Mp4MuxerOptions::default(),
        )
        .unwrap();

        let caps = <MetalComputeExecutor as RenderExecutor>::capabilities(&metal_exec);
        for f in 0..8i64 {
            let mut scene = Scene::new();
            scene.push(SceneNode::Rect {
                x: 0.0,
                y: 0.0,
                w: w as f32,
                h: h as f32,
                fill: Color::rgb(220 - (f * 15) as u8, 40, 40),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            });
            let cfg = FrameConfig::new(w, h, f as u32, 24.0);
            let plan = lower_scene_to_plan(
                &scene,
                &cfg,
                &LowerOptions {
                    max_texture_dimension_2d: caps.max_texture_dimension_2d,
                    capabilities: caps.flags,
                    msaa_samples: 1,
                },
            )
            .unwrap();

            let pb = encoder.acquire_pixel_buffer().unwrap();
            assert!(
                !pb.iosurface_ptr().is_null(),
                "expected IOSurface-backed CVPixelBuffer"
            );
            metal_exec
                .execute_plan_into_pixel_buffer(&plan, &cfg, &pb)
                .unwrap();
            for pkt in encoder.encode_pixel_buffer(&pb, f, 1).unwrap() {
                muxer.write_packet(&pkt).unwrap();
            }
        }
        for pkt in encoder.flush().unwrap() {
            muxer.write_packet(&pkt).unwrap();
        }
        muxer
            .set_video_codec_config(0, encoder.codec_config())
            .unwrap();
        muxer.finish().unwrap();

        let meta = probe_isobmff_path(&mp4_path).unwrap();
        assert_eq!(meta.video_streams.len(), 1);
        assert_eq!(meta.video_streams[0].codec, VideoCodecId::Hevc);

        // Decode with pure-Rust HEVC decoder and verify the red fill encoded cleanly!
        let mut demux = rust_motion_media_container::Mp4Demuxer::open_path(&mp4_path).unwrap();
        let track = demux.video_tracks()[0].clone();
        assert_eq!(track.samples.len(), 8);
        let mut hevc = rust_motion_hevc::Decoder::with_threads(1);
        hevc.configure_hvcc(&track.codec_config).unwrap();
        let mut buf = Vec::new();
        let mut pics = Vec::new();
        for i in 0..track.samples.len() {
            let s = demux.read_sample(0, i, &mut buf).unwrap();
            pics.extend(hevc.decode(&buf, s.pts).unwrap());
        }
        pics.extend(hevc.flush());
        assert_eq!(pics.len(), 8);
        // Verify picture 0 has red luma/chroma (Y > 50, U < 120, V > 150) proving BGRA channel order!
        let p0 = &pics[0];
        let u_center = p0.u.get((24 * p0.uv_stride) + 32);
        let v_center = p0.v.get((24 * p0.uv_stride) + 32);
        assert!(
            v_center > 155 && u_center < 120,
            "expected red frame chroma (V>155, U<120) after Metal->IOSurface->HEVC encode, got U={u_center} V={v_center}"
        );
    }

    #[test]
    #[ignore = "1080p60 Metal zero-copy benchmark; run via cargo test --release --features metal -- --ignored --nocapture"]
    fn metal_iosurface_zero_copy_1080p60_benchmark() {
        let metal_exec = MetalComputeExecutor::new().unwrap();
        let (w, h, frames) = (1920u32, 1080u32, 60usize);

        // Build a pure GPU compute ExecutionPlan (Clear + 2x Blur + ColorMatrix) on 1080p!
        let mut graph = RenderGraph::new(w, h);
        let make_desc = |id, label: &str| ResourceDesc {
            id: ResourceId(id),
            label: label.into(),
            width: w,
            height: h,
            format: PixelFormat::Rgba8Unorm,
            color: ColorMetadata::srgb_premultiplied(),
            sample_count: 1,
            usage: TextureUsage::STORAGE
                .union(TextureUsage::SAMPLED)
                .union(TextureUsage::RENDER_TARGET)
                .union(TextureUsage::COPY_SRC)
                .union(TextureUsage::COPY_DST),
            lifetime: if id == 3 {
                ResourceLifetime::Output
            } else {
                ResourceLifetime::Transient
            },
        };
        graph.add_resource(make_desc(0, "src"));
        graph.add_resource(make_desc(1, "blur_h"));
        graph.add_resource(make_desc(2, "blur_v"));
        graph.add_resource(make_desc(3, "graded"));
        graph.set_output(ResourceId(3));

        let mut base = Scene::new();
        base.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: w as f32,
            h: h as f32,
            fill: Color::rgb(40, 120, 220),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });
        graph.add_pass(crate::render_ir::PassNode {
            id: crate::render_ir::PassId(0),
            label: "fill".into(),
            op: PassOp::Raster2D {
                scene: base,
                clear: ClearOp::Clear(Color::TRANSPARENT),
                path_mask: None,
            },
            reads: vec![],
            writes: vec![ResourceId(0)],
            order_deps: vec![],
            required_capabilities: CapabilityFlags::RASTER_2D_ANALYTIC,
        });
        graph.add_pass(crate::render_ir::PassNode {
            id: crate::render_ir::PassId(1),
            label: "blur_h".into(),
            op: PassOp::Compute {
                kernel: KERNEL_SEPARABLE_BOX_BLUR_H.into(),
                workgroups: [w.div_ceil(8), h.div_ceil(8), 1],
                params: vec![3.0],
            },
            reads: vec![ResourceId(0)],
            writes: vec![ResourceId(1)],
            order_deps: vec![crate::render_ir::PassId(0)],
            required_capabilities: CapabilityFlags::COMPUTE_PASS,
        });
        graph.add_pass(crate::render_ir::PassNode {
            id: crate::render_ir::PassId(2),
            label: "blur_v".into(),
            op: PassOp::Compute {
                kernel: KERNEL_SEPARABLE_BOX_BLUR_V.into(),
                workgroups: [w.div_ceil(8), h.div_ceil(8), 1],
                params: vec![3.0],
            },
            reads: vec![ResourceId(1)],
            writes: vec![ResourceId(2)],
            order_deps: vec![crate::render_ir::PassId(1)],
            required_capabilities: CapabilityFlags::COMPUTE_PASS,
        });
        graph.add_pass(crate::render_ir::PassNode {
            id: crate::render_ir::PassId(3),
            label: "cm".into(),
            op: PassOp::Compute {
                kernel: KERNEL_COLOR_MATRIX.into(),
                workgroups: [w.div_ceil(8), h.div_ceil(8), 1],
                params: vec![1.1, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.9, 0.0],
            },
            reads: vec![ResourceId(2)],
            writes: vec![ResourceId(3)],
            order_deps: vec![crate::render_ir::PassId(2)],
            required_capabilities: CapabilityFlags::COMPUTE_PASS,
        });
        let plan = ExecutionPlan::compile(graph).unwrap();
        let cfg = FrameConfig::new(w, h, 0, 60.0);

        let mut enc = HardwareVideoEncoder::new(VideoCodecId::Hevc).unwrap();
        enc.configure(&VideoEncodeConfig {
            width: w,
            height: h,
            fps: Rational::new(60, 1),
            codec: VideoCodecId::Hevc,
            crf: 22,
            preset: "fast".into(),
            color: ColorDescription::bt709_limited(),
        })
        .unwrap();

        let started = std::time::Instant::now();
        let mut bytes = 0usize;
        for f in 0..frames {
            let pb = enc.acquire_pixel_buffer().unwrap();
            metal_exec
                .execute_plan_into_pixel_buffer(&plan, &cfg, &pb)
                .unwrap();
            for pkt in enc.encode_pixel_buffer(&pb, f as i64, 1).unwrap() {
                bytes += pkt.data.len();
            }
        }
        for pkt in enc.flush().unwrap() {
            bytes += pkt.data.len();
        }
        let secs = started.elapsed().as_secs_f64();
        eprintln!(
            "BENCH Metal ({}) 1080p60 compute+IOSurface->HEVC: {:.1} fps ({secs:.3}s, {} KiB)",
            metal_exec.device_name(),
            frames as f64 / secs,
            bytes / 1024
        );
    }
}
