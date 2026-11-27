//! Narrow backend execution contract and CPU reference executor.
//!
//! Backends implement [`RenderExecutor`] against [`ExecutionPlan`] without
//! exposing graphics-API types in the Render IR.

use super::types::*;
use super::validate::{ExecutionPlan, GraphValidationError};
use crate::backend::{FrameConfig, RasterError};
use crate::scene::SceneNode;
use crate::tiny_skia_backend::color_blend::{composite_custom_psd_blend, tiny_skia_blend_mode};
use crate::tiny_skia_backend::{svgpath_to_tiny_skia, TinySkiaBackend};
use image::RgbaImage;
use std::collections::BTreeMap;
use thiserror::Error;
use tiny_skia::{FillRule, Paint, Pixmap, PixmapPaint, Transform};

/// Capabilities exposed by a [`RenderExecutor`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutorCapabilities {
    pub(crate) backend_name: &'static str,
    pub(crate) flags: CapabilityFlags,
    pub(crate) max_texture_dimension_2d: u32,
    pub(crate) supported_formats: Vec<PixelFormat>,
    pub(crate) supports_msaa: bool,
    pub(crate) supports_timestamp_queries: bool,
}

/// Execution telemetry returned by [`RenderExecutor::execute_plan`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ExecutionReport {
    pub(crate) backend_name: &'static str,
    pub(crate) passes_executed: usize,
    pub(crate) gpu_passes: usize,
    pub(crate) compute_passes: usize,
    pub(crate) cpu_island_passes: usize,
    pub(crate) cpu_island_pixels: u64,
    pub(crate) cpu_island_upload_bytes: u64,
    pub(crate) transient_pool_slots: usize,
    pub(crate) peak_transient_bytes: u64,
    pub(crate) fallback_reasons: Vec<String>,
}

/// Explicit errors returned by [`RenderExecutor`].
#[derive(Error, Debug)]
pub(crate) enum ExecutionError {
    #[error("render graph validation failed: {0}")]
    Validation(#[from] GraphValidationError),
    #[error("backend '{backend}' lacks capabilities for pass {pass}: {missing:?}")]
    UnsupportedCapability {
        backend: &'static str,
        pass: PassId,
        missing: Vec<&'static str>,
    },
    #[error("failed to allocate resource {resource}: {reason}")]
    ResourceAllocation {
        resource: ResourceId,
        reason: String,
    },
    #[error("pass {pass} execution failed: {reason}")]
    SubmissionFailed { pass: PassId, reason: String },
    #[error("GPU device lost: {0}")]
    DeviceLost(String),
    #[error("render cancelled")]
    Cancelled,
    #[error(transparent)]
    Raster(#[from] RasterError),
}

impl From<ExecutionError> for RasterError {
    fn from(err: ExecutionError) -> Self {
        match err {
            ExecutionError::Raster(r) => r,
            ExecutionError::Cancelled => RasterError::Cancelled,
            ExecutionError::DeviceLost(msg) => RasterError::Init(format!("GPU device lost: {msg}")),
            other => RasterError::Scene(other.to_string()),
        }
    }
}

/// Narrow, backend-independent execution contract over compiled [`ExecutionPlan`]s.
pub(crate) trait RenderExecutor: Send + Sync {
    fn capabilities(&self) -> ExecutorCapabilities;

    fn execute_plan(
        &self,
        plan: &ExecutionPlan,
        config: &FrameConfig,
    ) -> Result<(RgbaImage, ExecutionReport), ExecutionError>;
}

/// Deterministic CPU reference executor backed by [`TinySkiaBackend`].
pub(crate) struct CpuReferenceExecutor<'a> {
    backend: &'a TinySkiaBackend,
}

impl<'a> CpuReferenceExecutor<'a> {
    pub(crate) fn new(backend: &'a TinySkiaBackend) -> Self {
        Self { backend }
    }
}

enum CpuSurface {
    Color(Pixmap),
    Mask(Vec<u8>, u32, u32),
}

impl<'a> RenderExecutor for CpuReferenceExecutor<'a> {
    fn capabilities(&self) -> ExecutorCapabilities {
        ExecutorCapabilities {
            backend_name: "cpu_reference",
            flags: CapabilityFlags::RASTER_2D_ANALYTIC
                .union(CapabilityFlags::RASTER_2D_MESH)
                .union(CapabilityFlags::RASTER_2D_IMAGE)
                .union(CapabilityFlags::RASTER_2D_TEXT)
                .union(CapabilityFlags::BLEND_BASIC)
                .union(CapabilityFlags::BLEND_EXTENDED)
                .union(CapabilityFlags::OFFSCREEN_COMPOSITE)
                .union(CapabilityFlags::PATH_MASK)
                .union(CapabilityFlags::SHADER_PASS)
                .union(CapabilityFlags::COMPUTE_PASS)
                .union(CapabilityFlags::COPY_OP)
                .union(CapabilityFlags::CPU_ISLAND)
                .union(CapabilityFlags::RASTER_EXTENDED),
            max_texture_dimension_2d: u32::MAX,
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
        let caps = self.capabilities();
        if let Err(GraphValidationError::MissingCapabilities { pass, missing }) =
            plan.check_capabilities(caps.flags)
        {
            return Err(ExecutionError::UnsupportedCapability {
                backend: caps.backend_name,
                pass,
                missing,
            });
        }

        let mut surfaces: BTreeMap<ResourceId, CpuSurface> = BTreeMap::new();
        for res in &plan.graph.resources {
            let surface = if res.format.is_mask() {
                let len = (res.width as usize)
                    .checked_mul(res.height as usize)
                    .ok_or_else(|| ExecutionError::ResourceAllocation {
                        resource: res.id,
                        reason: "mask buffer size overflow".into(),
                    })?;
                CpuSurface::Mask(vec![0u8; len], res.width, res.height)
            } else {
                let pixmap = Pixmap::new(res.width, res.height).ok_or_else(|| {
                    ExecutionError::ResourceAllocation {
                        resource: res.id,
                        reason: format!("invalid pixmap dimensions {}x{}", res.width, res.height),
                    }
                })?;
                CpuSurface::Color(pixmap)
            };
            surfaces.insert(res.id, surface);
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

        for &pid in &plan.pass_order {
            let pass = plan
                .graph
                .pass(pid)
                .expect("scheduled pass must exist in graph");
            let dst_id = pass.writes[0];

            match &pass.op {
                PassOp::PathMask { path_d, transform } => {
                    let path = svgpath_to_tiny_skia(path_d).ok_or_else(|| {
                        ExecutionError::SubmissionFailed {
                            pass: pid,
                            reason: "invalid SVG path in PathMask".into(),
                        }
                    })?;
                    let (w, h) = match surfaces.get(&dst_id) {
                        Some(CpuSurface::Mask(_, w, h)) => (*w, *h),
                        _ => {
                            return Err(ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "PathMask destination is not an R8 mask surface".into(),
                            })
                        }
                    };
                    let mut tmp =
                        Pixmap::new(w, h).ok_or_else(|| ExecutionError::ResourceAllocation {
                            resource: dst_id,
                            reason: "failed to allocate temporary mask pixmap".into(),
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
                    let mask_bytes = match surfaces.get_mut(&dst_id) {
                        Some(CpuSurface::Mask(bytes, _, _)) => bytes,
                        _ => unreachable!(),
                    };
                    for (dst_alpha, src_px) in mask_bytes.iter_mut().zip(tmp.data().chunks_exact(4))
                    {
                        *dst_alpha = src_px[3];
                    }
                }

                PassOp::Raster2D {
                    scene,
                    clear,
                    path_mask,
                } => {
                    let mask_snapshot = path_mask.and_then(|mid| match surfaces.get(&mid) {
                        Some(CpuSurface::Mask(bytes, w, h)) => Some((bytes.clone(), *w, *h)),
                        _ => None,
                    });

                    let dst = match surfaces.get_mut(&dst_id) {
                        Some(CpuSurface::Color(p)) => p,
                        _ => {
                            return Err(ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Raster2D destination is not a color surface".into(),
                            })
                        }
                    };

                    if let Some((mask_bytes, mw, mh)) = mask_snapshot {
                        if mw != dst.width() || mh != dst.height() {
                            return Err(ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "path mask dimensions do not match target".into(),
                            });
                        }
                        if let ClearOp::Clear(c) = clear {
                            dst.fill(c.to_tiny_skia_color());
                        }
                        let mut layer_pm =
                            Pixmap::new(dst.width(), dst.height()).ok_or_else(|| {
                                ExecutionError::ResourceAllocation {
                                    resource: dst_id,
                                    reason: "failed to allocate masked raster layer".into(),
                                }
                            })?;
                        self.backend.render_nodes_into_pixmap(
                            &mut layer_pm,
                            &scene.nodes,
                            config.fps,
                        )?;
                        for (px, &m) in layer_pm
                            .data_mut()
                            .chunks_exact_mut(4)
                            .zip(mask_bytes.iter())
                        {
                            if m == 0 {
                                px.fill(0);
                            } else if m < 255 {
                                let factor = u16::from(m);
                                px[0] = ((u16::from(px[0]) * factor + 127) / 255) as u8;
                                px[1] = ((u16::from(px[1]) * factor + 127) / 255) as u8;
                                px[2] = ((u16::from(px[2]) * factor + 127) / 255) as u8;
                                px[3] = ((u16::from(px[3]) * factor + 127) / 255) as u8;
                            }
                        }
                        dst.draw_pixmap(
                            0,
                            0,
                            layer_pm.as_ref(),
                            &PixmapPaint::default(),
                            Transform::identity(),
                            None,
                        );
                    } else {
                        match clear {
                            ClearOp::Clear(c) => dst.fill(c.to_tiny_skia_color()),
                            ClearOp::Load | ClearOp::DontCare => {}
                        }
                        self.backend
                            .render_nodes_into_pixmap(dst, &scene.nodes, config.fps)?;
                    }
                }

                PassOp::Composite {
                    source,
                    opacity,
                    blend_mode,
                    region,
                } => {
                    let src_clone = match surfaces.get(source) {
                        Some(CpuSurface::Color(p)) => p.clone(),
                        _ => {
                            return Err(ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Composite source is not a color surface".into(),
                            })
                        }
                    };
                    let dst = match surfaces.get_mut(&dst_id) {
                        Some(CpuSurface::Color(p)) => p,
                        _ => {
                            return Err(ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Composite destination is not a color surface".into(),
                            })
                        }
                    };

                    let (dx, dy) = match region {
                        Some(r) => (r.x.round() as i32, r.y.round() as i32),
                        None => (0, 0),
                    };
                    if let Some(sk_mode) = tiny_skia_blend_mode(*blend_mode) {
                        let paint = PixmapPaint {
                            opacity: *opacity,
                            blend_mode: sk_mode,
                            quality: tiny_skia::FilterQuality::Bilinear,
                        };
                        dst.draw_pixmap(
                            dx,
                            dy,
                            src_clone.as_ref(),
                            &paint,
                            Transform::identity(),
                            None,
                        );
                    } else {
                        composite_custom_psd_blend(
                            dst,
                            &src_clone,
                            blend_mode.to_color_blend_mode(),
                            *opacity,
                            dx,
                            dy,
                        );
                    }
                }

                PassOp::Copy { source, dst_origin } => {
                    let src_data = match surfaces.get(source) {
                        Some(CpuSurface::Color(p)) => {
                            (p.width(), p.height(), p.data().to_vec(), false)
                        }
                        Some(CpuSurface::Mask(bytes, w, h)) => (*w, *h, bytes.clone(), true),
                        None => {
                            return Err(ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Copy source missing".into(),
                            })
                        }
                    };
                    let (sw, sh, bytes, is_mask) = src_data;
                    match surfaces.get_mut(&dst_id) {
                        Some(CpuSurface::Color(dst)) if !is_mask => {
                            let dw = dst.width();
                            let dst_data = dst.data_mut();
                            for row in 0..sh {
                                let dst_row = dst_origin[1] + row;
                                let dst_offset = ((dst_row * dw + dst_origin[0]) * 4) as usize;
                                let src_offset = (row * sw * 4) as usize;
                                let row_bytes = (sw * 4) as usize;
                                dst_data[dst_offset..dst_offset + row_bytes]
                                    .copy_from_slice(&bytes[src_offset..src_offset + row_bytes]);
                            }
                        }
                        Some(CpuSurface::Mask(dst_bytes, dw, _)) if is_mask => {
                            let dw = *dw;
                            for row in 0..sh {
                                let dst_row = dst_origin[1] + row;
                                let dst_offset = (dst_row * dw + dst_origin[0]) as usize;
                                let src_offset = (row * sw) as usize;
                                let row_bytes = sw as usize;
                                dst_bytes[dst_offset..dst_offset + row_bytes]
                                    .copy_from_slice(&bytes[src_offset..src_offset + row_bytes]);
                            }
                        }
                        _ => {
                            return Err(ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Copy resource kind mismatch".into(),
                            })
                        }
                    }
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
                    let dst = match surfaces.get_mut(&dst_id) {
                        Some(CpuSurface::Color(p)) => p,
                        _ => {
                            return Err(ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Shader destination is not a color surface".into(),
                            })
                        }
                    };
                    if let ClearOp::Clear(c) = clear {
                        dst.fill(c.to_tiny_skia_color());
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
                    self.backend.render_nodes_into_pixmap(
                        dst,
                        std::slice::from_ref(&node),
                        config.fps,
                    )?;
                }

                PassOp::CpuIsland {
                    scene,
                    bounds,
                    reason,
                } => {
                    let island_nodes = if let Some(b) = bounds {
                        vec![SceneNode::Group {
                            transform: crate::scene::Transform2D::translate(-b.x, -b.y),
                            opacity: 1.0,
                            children: scene.nodes.clone(),
                        }]
                    } else {
                        scene.nodes.clone()
                    };
                    let dst = match surfaces.get_mut(&dst_id) {
                        Some(CpuSurface::Color(p)) => p,
                        _ => {
                            return Err(ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "CpuIsland destination is not a color surface".into(),
                            })
                        }
                    };
                    let target_w = dst.width();
                    let target_h = dst.height();
                    dst.fill(tiny_skia::Color::TRANSPARENT);
                    self.backend
                        .render_nodes_into_pixmap(dst, &island_nodes, config.fps)?;
                    report.cpu_island_passes += 1;
                    report.cpu_island_pixels += u64::from(target_w) * u64::from(target_h);
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
                            reason: "Compute pass requires at least one read resource".into(),
                        }
                    })?;
                    let src_snapshot = match surfaces.get(&src_id) {
                        Some(CpuSurface::Color(p)) => (p.width(), p.height(), p.data().to_vec()),
                        _ => {
                            return Err(ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Compute source must be a color surface".into(),
                            })
                        }
                    };
                    let dst = match surfaces.get_mut(&dst_id) {
                        Some(CpuSurface::Color(p)) => p,
                        _ => {
                            return Err(ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Compute destination must be a color surface".into(),
                            })
                        }
                    };
                    let (sw, sh, src_bytes) = src_snapshot;
                    if sw != dst.width() || sh != dst.height() {
                        return Err(ExecutionError::SubmissionFailed {
                            pass: pid,
                            reason: "Compute source and destination dimensions must match".into(),
                        });
                    }
                    let w = sw as usize;
                    let h = sh as usize;
                    let out = dst.data_mut();
                    match kernel.as_str() {
                        KERNEL_SEPARABLE_BOX_BLUR_H => {
                            let radius = params.first().copied().unwrap_or(0.0).max(0.0) as usize;
                            if radius == 0 {
                                out.copy_from_slice(&src_bytes);
                            } else {
                                let k = (radius * 2 + 1) as u32;
                                for y in 0..h {
                                    for x in 0..w {
                                        let mut sums = [0u32; 4];
                                        let x0 = x.saturating_sub(radius);
                                        let x1 = x.saturating_add(radius).min(w - 1);
                                        for sx in x0..=x1 {
                                            let si = (y * w + sx) * 4;
                                            for c in 0..4 {
                                                sums[c] += u32::from(src_bytes[si + c]);
                                            }
                                        }
                                        let di = (y * w + x) * 4;
                                        for c in 0..4 {
                                            out[di + c] = (sums[c] / k) as u8;
                                        }
                                    }
                                }
                            }
                        }
                        KERNEL_SEPARABLE_BOX_BLUR_V => {
                            let radius = params.first().copied().unwrap_or(0.0).max(0.0) as usize;
                            if radius == 0 {
                                out.copy_from_slice(&src_bytes);
                            } else {
                                let k = (radius * 2 + 1) as u32;
                                for y in 0..h {
                                    for x in 0..w {
                                        let mut sums = [0u32; 4];
                                        let y0 = y.saturating_sub(radius);
                                        let y1 = y.saturating_add(radius).min(h - 1);
                                        for sy in y0..=y1 {
                                            let si = (sy * w + x) * 4;
                                            for c in 0..4 {
                                                sums[c] += u32::from(src_bytes[si + c]);
                                            }
                                        }
                                        let di = (y * w + x) * 4;
                                        for c in 0..4 {
                                            out[di + c] = (sums[c] / k) as u8;
                                        }
                                    }
                                }
                            }
                        }
                        KERNEL_COLOR_MATRIX => {
                            if params.len() < 12 {
                                return Err(ExecutionError::SubmissionFailed {
                                    pass: pid,
                                    reason: "color_matrix requires 12 matrix floats".into(),
                                });
                            }
                            for (dst_px, src_px) in
                                out.chunks_exact_mut(4).zip(src_bytes.chunks_exact(4))
                            {
                                let r = f32::from(src_px[0]);
                                let g = f32::from(src_px[1]);
                                let b = f32::from(src_px[2]);
                                let a = f32::from(src_px[3]);
                                dst_px[0] =
                                    (params[0] * r + params[1] * g + params[2] * b + params[3] * a)
                                        .round()
                                        .clamp(0.0, a) as u8;
                                dst_px[1] =
                                    (params[4] * r + params[5] * g + params[6] * b + params[7] * a)
                                        .round()
                                        .clamp(0.0, a) as u8;
                                dst_px[2] = (params[8] * r
                                    + params[9] * g
                                    + params[10] * b
                                    + params[11] * a)
                                    .round()
                                    .clamp(0.0, a)
                                    as u8;
                                dst_px[3] = src_px[3];
                            }
                        }
                        _ => {
                            return Err(ExecutionError::UnsupportedCapability {
                                backend: caps.backend_name,
                                pass: pid,
                                missing: vec!["compute_kernel"],
                            });
                        }
                    }
                    report.compute_passes += 1;
                }
            }

            report.passes_executed += 1;
        }

        let out_id = plan
            .graph
            .output
            .ok_or(GraphValidationError::MissingOutput)?;
        let out_pixmap = match surfaces.remove(&out_id) {
            Some(CpuSurface::Color(p)) => p,
            _ => {
                return Err(ExecutionError::ResourceAllocation {
                    resource: out_id,
                    reason: "graph output is not a color surface".into(),
                })
            }
        };
        let w = out_pixmap.width();
        let h = out_pixmap.height();
        let image = RgbaImage::from_raw(w, h, out_pixmap.take()).ok_or_else(|| {
            RasterError::ImageEncode("Failed to assemble RgbaImage from CPU RenderGraph".into())
        })?;

        Ok((image, report))
    }
}
