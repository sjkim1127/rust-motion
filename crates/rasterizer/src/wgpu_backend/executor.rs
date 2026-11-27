//! Narrow [`RenderExecutor`] implementation for [`WgpuBackend`].

use super::*;
use crate::render_ir::{
    lower_scene_to_plan, shader_inline_opacity_supported, CapabilityFlags, ClearOp, ExecutionError,
    ExecutionPlan, ExecutionReport, ExecutorCapabilities, LowerOptions, PassId, PassOp,
    PixelFormat, RenderExecutor, ResourceId, KERNEL_COLOR_MATRIX, KERNEL_SEPARABLE_BOX_BLUR_H,
    KERNEL_SEPARABLE_BOX_BLUR_V,
};
use crate::scene::Color;
use tiny_skia::Pixmap;

impl WgpuBackend {
    /// Lower a 1.x [`Scene`] into a validated [`ExecutionPlan`] using this
    /// device's capability profile.
    pub(crate) fn compile_render_plan(
        &self,
        scene: &Scene,
        config: &FrameConfig,
    ) -> Result<ExecutionPlan, ExecutionError> {
        let caps = <Self as RenderExecutor>::capabilities(self);
        let opts = LowerOptions {
            max_texture_dimension_2d: caps.max_texture_dimension_2d,
            capabilities: caps.flags,
            msaa_samples: SAMPLE_COUNT,
        };
        Ok(lower_scene_to_plan(scene, config, &opts)?)
    }

    fn execute_single_raster_with_optional_shaders(
        &self,
        scene: &Scene,
        explicit_path_mask: Option<GpuPathMask>,
        shader_suffix: Option<&[SceneNode]>,
        config: &FrameConfig,
        full_scene_for_fallback: Option<&Scene>,
    ) -> Result<(RgbaImage, usize, Option<String>), ExecutionError> {
        if config.width > self.ctx.max_texture_dimension_2d
            || config.height > self.ctx.max_texture_dimension_2d
        {
            let reason = "frame dimensions exceed the GPU texture limit";
            self.record_cpu_fallback(reason);
            let fallback_scene = full_scene_for_fallback.unwrap_or(scene);
            let img = self.fallback.render_frame(fallback_scene, config)?;
            return Ok((img, 1, Some(reason.to_string())));
        }

        let is_profiling = self.is_profiling_enabled();
        let frame_started = Instant::now();
        let decode_before = if is_profiling {
            self.video_decode_ns.load(Ordering::Relaxed)
        } else {
            0
        };
        let upload_ns_before = if is_profiling {
            self.texture_upload_ns.load(Ordering::Relaxed)
                + self.text_atlas_upload_ns.load(Ordering::Relaxed)
        } else {
            0
        };
        let upload_bytes_before = if is_profiling {
            self.gpu_texture_upload_bytes.load(Ordering::Relaxed)
                + self.text_atlas_upload_bytes.load(Ordering::Relaxed)
        } else {
            0
        };

        let encode_start = if is_profiling {
            Some(Instant::now())
        } else {
            None
        };

        let compiled = compile_scene_with_path_mask(scene, &self.fallback).and_then(
            |(commands, compiled_mask, asset_decode_ns)| {
                let final_mask = match (compiled_mask, explicit_path_mask) {
                    (Some(_), Some(_)) => return None,
                    (Some(m), None) | (None, Some(m)) => Some(m),
                    (None, None) => None,
                };
                Some((commands, final_mask, asset_decode_ns))
            },
        );

        let Some((commands, path_mask, asset_decode_ns)) = compiled else {
            let fallback_scene = full_scene_for_fallback.unwrap_or(scene);
            let reason = gpu_fallback_reason(scene);
            if reason == "overlapping texture layer requires offscreen compositing" {
                let _ = self.offscreen_layer_slot(config.width, config.height);
            }
            self.record_cpu_fallback(reason);
            let fb_start = if is_profiling {
                Some(Instant::now())
            } else {
                None
            };
            let img = self.fallback.render_frame(fallback_scene, config)?;
            let fb_render_ns = fb_start.map(|t| t.elapsed().as_nanos() as u64).unwrap_or(0);
            if is_profiling {
                let total_ns = frame_started.elapsed().as_nanos() as u64;
                self.record_profile_sample(FrameProfileSample {
                    frame_idx: config.frame,
                    scene_eval_ns: 0,
                    decode_ns: 0,
                    upload_ns: 0,
                    upload_bytes: 0,
                    compile_encode_ns: fb_render_ns,
                    submission_wait_ns: 0,
                    gpu_exec_ns: None,
                    readback_ns: 0,
                    readback_bytes: 0,
                    video_encode_ns: 0,
                    total_frame_ns: total_ns,
                    cpu_fallback: true,
                });
                self.last_session_wall_ns.store(total_ns, Ordering::Relaxed);
            }
            return Ok((img, 1, Some(reason.to_string())));
        };

        let width = config.width;
        let height = config.height;

        let resource = {
            let mut pool = self
                .frame_resources
                .lock()
                .map_err(|_| RasterError::Init("GPU resource pool mutex poisoned".into()))?;
            pool.entry((width, height))
                .or_insert_with(|| {
                    Arc::new(Mutex::new(GpuFrameResources::new(
                        &self.ctx.device,
                        width,
                        height,
                        self.ctx.supports_timestamp_queries,
                    )))
                })
                .clone()
        };
        let mut res = resource
            .lock()
            .map_err(|_| RasterError::Init("GPU frame resource mutex poisoned".into()))?;

        let slot_idx = res.active_index % RING_BUFFER_SIZE;
        res.active_index = res.active_index.wrapping_add(1);

        let submitted = self.submit_frame_to_slot(FrameSubmission {
            commands: &commands,
            path_mask: path_mask.as_ref(),
            width,
            height,
            sampling_fps: config.fps,
            slot: &res.slots[slot_idx],
            shader_suffix,
            readback: true,
        })?;
        let submission_index = submitted.submission_index;
        let rx = submitted.readback_rx;
        let query_rx = submitted.query_rx;
        let encode_total_ns = encode_start
            .map(|t| t.elapsed().as_nanos() as u64)
            .unwrap_or(0);

        let (decode_ns, upload_ns, upload_bytes, compile_encode_ns) = if is_profiling {
            let decode_after = self.video_decode_ns.load(Ordering::Relaxed);
            let upload_ns_after = self.texture_upload_ns.load(Ordering::Relaxed)
                + self.text_atlas_upload_ns.load(Ordering::Relaxed);
            let upload_bytes_after = self.gpu_texture_upload_bytes.load(Ordering::Relaxed)
                + self.text_atlas_upload_bytes.load(Ordering::Relaxed);

            let video_decode_diff = decode_after.saturating_sub(decode_before);
            let dec_ns = video_decode_diff.saturating_add(asset_decode_ns);
            let upl_ns = upload_ns_after.saturating_sub(upload_ns_before);
            let upl_bytes = upload_bytes_after.saturating_sub(upload_bytes_before);
            let comp_enc_ns = encode_total_ns
                .saturating_sub(upl_ns)
                .saturating_sub(asset_decode_ns);
            (dec_ns, upl_ns, upl_bytes, comp_enc_ns)
        } else {
            (0, 0, 0, 0)
        };

        let gpu_submit_start = Instant::now();
        let mut out_pixels = None;
        let mut scratch = Vec::new();
        self.drain_slot(
            InFlight {
                frame_idx: config.frame,
                slot_idx,
                submission_index,
                rx: rx.expect("readback was requested for render_frame"),
                query_rx,
                frame_started,
                scene_eval_ns: 0,
                decode_ns,
                upload_ns,
                upload_bytes,
                compile_encode_ns,
            },
            &res,
            width,
            height,
            &mut scratch,
            &mut |_frame, pixels: &[u8]| {
                out_pixels = Some(pixels.to_vec());
                Ok(())
            },
        )?;

        let pixels = out_pixels.ok_or_else(|| {
            RasterError::ImageEncode("Failed to assemble RgbaImage from GPU readback".into())
        })?;
        let image = RgbaImage::from_raw(width, height, pixels).ok_or_else(|| {
            RasterError::ImageEncode("Failed to assemble RgbaImage from GPU readback".into())
        })?;
        if is_profiling {
            let total_frame_ns = frame_started.elapsed().as_nanos() as u64;
            self.last_session_wall_ns
                .store(total_frame_ns, Ordering::Relaxed);
        }
        self.gpu_submit_readback_ns.fetch_add(
            gpu_submit_start.elapsed().as_nanos() as u64,
            Ordering::Relaxed,
        );
        self.gpu_frame_count.fetch_add(1, Ordering::Relaxed);
        Ok((image, 0, None))
    }

    fn execute_shader_passes(
        &self,
        nodes: &[SceneNode],
        config: &FrameConfig,
    ) -> Result<RgbaImage, ExecutionError> {
        let scene = Scene {
            nodes: nodes.to_vec(),
        };
        let all_direct = nodes.iter().all(|node| {
            matches!(
                node,
                SceneNode::Shader { x, y, w, h, source, opacity, .. }
                if *opacity >= 0.0
                    && *opacity <= 1.0
                    && shader_inline_opacity_supported(source)
                    && *x >= 0.0
                    && *y >= 0.0
                    && *w > 0.0
                    && *h > 0.0
                    && *x + *w <= config.width as f32
                    && *y + *h <= config.height as f32
            )
        });
        let image = if all_direct {
            self.render_shader_layers_direct(&scene, config)?
        } else {
            self.render_shader_layers(&scene, config)?
        };
        self.gpu_frame_count.fetch_add(1, Ordering::Relaxed);
        Ok(image)
    }

    fn execute_general_dag(
        &self,
        plan: &ExecutionPlan,
        config: &FrameConfig,
    ) -> Result<(RgbaImage, ExecutionReport), ExecutionError> {
        struct GpuSurface {
            texture: wgpu::Texture,
            view: wgpu::TextureView,
            width: u32,
            height: u32,
        }
        enum Surface {
            GpuTexture(GpuSurface),
            Mask(Box<GpuPathMask>),
        }

        let mut surfaces: std::collections::BTreeMap<ResourceId, Surface> =
            std::collections::BTreeMap::new();
        for res in &plan.graph.resources {
            if res.format.is_color() {
                let (texture, view) =
                    self.create_compute_texture(res.width, res.height, &res.label);
                surfaces.insert(
                    res.id,
                    Surface::GpuTexture(GpuSurface {
                        texture,
                        view,
                        width: res.width,
                        height: res.height,
                    }),
                );
            }
        }

        let mut gpu_passes = 0usize;
        let mut compute_passes = 0usize;
        let mut cpu_island_passes = 0usize;
        let mut cpu_island_pixels = 0u64;
        let mut cpu_island_upload_bytes = 0u64;
        let mut fallback_reasons = Vec::new();

        for &pid in &plan.pass_order {
            let pass = plan
                .graph
                .pass(pid)
                .expect("scheduled pass must exist in graph");
            let dst_id = pass.writes[0];

            match &pass.op {
                PassOp::PathMask { path_d, transform } => {
                    let mask =
                        gpu_path_mask_from_svg(path_d, transform.to_tiny_skia(), Color::WHITE)
                            .ok_or_else(|| ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "failed to tessellate PathMask SVG path".into(),
                            })?;
                    surfaces.insert(dst_id, Surface::Mask(Box::new(mask)));
                    gpu_passes += 1;
                }

                PassOp::Raster2D {
                    scene,
                    clear,
                    path_mask,
                } => {
                    let explicit_mask = path_mask.and_then(|mid| match surfaces.remove(&mid) {
                        Some(Surface::Mask(mask)) => Some(*mask),
                        _ => None,
                    });
                    let (dst_w, dst_h) = match surfaces.get(&dst_id) {
                        Some(Surface::GpuTexture(s)) => (s.width, s.height),
                        _ => {
                            return Err(ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Raster2D target is not a color surface".into(),
                            })
                        }
                    };

                    // Prepend clear color if non-transparent Clear is requested.
                    let effective_scene = match clear {
                        ClearOp::Clear(c) if c.a > 0 => {
                            let mut nodes = vec![SceneNode::Rect {
                                x: 0.0,
                                y: 0.0,
                                w: dst_w as f32,
                                h: dst_h as f32,
                                fill: *c,
                                stroke: None,
                                stroke_width: 0.0,
                                corner_radius: 0.0,
                            }];
                            nodes.extend(scene.nodes.iter().cloned());
                            Scene { nodes }
                        }
                        _ => scene.clone(),
                    };

                    let compiled = compile_scene_with_path_mask(&effective_scene, &self.fallback)
                        .and_then(|(cmds, mask, _)| {
                            let final_mask = match (mask, explicit_mask) {
                                (Some(_), Some(_)) => return None,
                                (Some(m), None) | (None, Some(m)) => Some(m),
                                (None, None) => None,
                            };
                            Some((cmds, final_mask))
                        });

                    if let Some((commands, final_mask)) = compiled {
                        let slot_arc = self.offscreen_layer_slot(dst_w, dst_h);
                        let slot = slot_arc.lock().map_err(|_| {
                            RasterError::Init("GPU layer resource mutex poisoned".into())
                        })?;
                        let submitted = self.submit_frame_to_slot(FrameSubmission {
                            commands: &commands,
                            path_mask: final_mask.as_ref(),
                            width: dst_w,
                            height: dst_h,
                            sampling_fps: config.fps,
                            slot: &slot,
                            shader_suffix: None,
                            readback: false,
                        })?;
                        self.ctx
                            .device
                            .poll(wgpu::Maintain::wait_for(submitted.submission_index));

                        let Some(Surface::GpuTexture(dst_surf)) = surfaces.get(&dst_id) else {
                            unreachable!();
                        };
                        match clear {
                            ClearOp::Clear(_) | ClearOp::DontCare => {
                                let mut encoder = self.ctx.device.create_command_encoder(
                                    &wgpu::CommandEncoderDescriptor {
                                        label: Some("raster2d_copy_to_surface"),
                                    },
                                );
                                encoder.copy_texture_to_texture(
                                    slot.texture.as_image_copy(),
                                    dst_surf.texture.as_image_copy(),
                                    wgpu::Extent3d {
                                        width: dst_w,
                                        height: dst_h,
                                        depth_or_array_layers: 1,
                                    },
                                );
                                let sub = self.ctx.queue.submit(Some(encoder.finish()));
                                self.ctx.device.poll(wgpu::Maintain::wait_for(sub));
                            }
                            ClearOp::Load => {
                                let sub = self.composite_external_texture_view(
                                    &slot.texture_view,
                                    &dst_surf.view,
                                    dst_w,
                                    dst_h,
                                    None,
                                    1.0,
                                    true,
                                )?;
                                self.ctx.device.poll(wgpu::Maintain::wait_for(sub));
                            }
                        }
                        gpu_passes += 1;
                    } else {
                        // Local pass-level CPU fallback if compile_scene_with_path_mask declines.
                        let reason = gpu_fallback_reason(&effective_scene);
                        let img = self.fallback.render_frame(
                            &effective_scene,
                            &FrameConfig::new(dst_w, dst_h, config.frame, config.fps),
                        )?;
                        let pixels = img.into_raw();
                        let upload_bytes = pixels.len() as u64;
                        let (tmp_tex, tmp_view) =
                            self.upload_rgba_texture(dst_w, dst_h, &pixels, "raster_cpu_fallback");
                        let Some(Surface::GpuTexture(dst_surf)) = surfaces.get(&dst_id) else {
                            unreachable!();
                        };
                        match clear {
                            ClearOp::Clear(_) | ClearOp::DontCare => {
                                let mut encoder = self.ctx.device.create_command_encoder(
                                    &wgpu::CommandEncoderDescriptor {
                                        label: Some("raster_cpu_fallback_copy"),
                                    },
                                );
                                encoder.copy_texture_to_texture(
                                    tmp_tex.as_image_copy(),
                                    dst_surf.texture.as_image_copy(),
                                    wgpu::Extent3d {
                                        width: dst_w,
                                        height: dst_h,
                                        depth_or_array_layers: 1,
                                    },
                                );
                                let sub = self.ctx.queue.submit(Some(encoder.finish()));
                                self.ctx.device.poll(wgpu::Maintain::wait_for(sub));
                            }
                            ClearOp::Load => {
                                let sub = self.composite_external_texture_view(
                                    &tmp_view,
                                    &dst_surf.view,
                                    dst_w,
                                    dst_h,
                                    None,
                                    1.0,
                                    true,
                                )?;
                                self.ctx.device.poll(wgpu::Maintain::wait_for(sub));
                            }
                        }
                        cpu_island_passes += 1;
                        cpu_island_pixels += u64::from(dst_w) * u64::from(dst_h);
                        cpu_island_upload_bytes += upload_bytes;
                        fallback_reasons.push(reason.to_string());
                    }
                }

                PassOp::Compute { kernel, params, .. } => {
                    if !matches!(
                        kernel.as_str(),
                        KERNEL_SEPARABLE_BOX_BLUR_H
                            | KERNEL_SEPARABLE_BOX_BLUR_V
                            | KERNEL_COLOR_MATRIX
                    ) {
                        return Err(ExecutionError::UnsupportedCapability {
                            backend: "wgpu",
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
                    let (src_surf, dst_surf) = match (surfaces.get(&src_id), surfaces.get(&dst_id))
                    {
                        (Some(Surface::GpuTexture(s)), Some(Surface::GpuTexture(d))) => (s, d),
                        _ => {
                            return Err(ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Compute pass requires color texture resources".into(),
                            })
                        }
                    };
                    if src_surf.width != dst_surf.width || src_surf.height != dst_surf.height {
                        return Err(ExecutionError::SubmissionFailed {
                            pass: pid,
                            reason: "Compute source and destination dimensions must match".into(),
                        });
                    }
                    let mut encoder =
                        self.ctx
                            .device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("compute_pass_encoder"),
                            });
                    match kernel.as_str() {
                        KERNEL_SEPARABLE_BOX_BLUR_H | KERNEL_SEPARABLE_BOX_BLUR_V => {
                            let radius = params.first().copied().unwrap_or(0.0).max(0.0) as u32;
                            let vertical = kernel == KERNEL_SEPARABLE_BOX_BLUR_V;
                            self.encode_box_blur_compute(
                                &mut encoder,
                                &src_surf.view,
                                &dst_surf.view,
                                src_surf.width,
                                src_surf.height,
                                radius,
                                vertical,
                            );
                        }
                        KERNEL_COLOR_MATRIX => {
                            if params.len() < 12 {
                                return Err(ExecutionError::SubmissionFailed {
                                    pass: pid,
                                    reason: "color_matrix requires 12 float parameters".into(),
                                });
                            }
                            let matrix: [f32; 12] = params[..12].try_into().unwrap();
                            self.encode_color_matrix_compute(
                                &mut encoder,
                                &src_surf.view,
                                &dst_surf.view,
                                src_surf.width,
                                src_surf.height,
                                &matrix,
                            );
                        }
                        _ => {
                            return Err(ExecutionError::UnsupportedCapability {
                                backend: "wgpu",
                                pass: pid,
                                missing: vec!["compute_kernel"],
                            });
                        }
                    }
                    let sub = self.ctx.queue.submit(Some(encoder.finish()));
                    self.ctx.device.poll(wgpu::Maintain::wait_for(sub));
                    self.compute_pass_count.fetch_add(1, Ordering::Relaxed);
                    compute_passes += 1;
                    gpu_passes += 1;
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
                    let shader_scene = Scene { nodes: vec![node] };
                    let rendered = if *opacity >= 0.0
                        && *opacity <= 1.0
                        && *x >= 0.0
                        && *y >= 0.0
                        && *w > 0.0
                        && *h > 0.0
                        && *x + *w <= config.width as f32
                        && *y + *h <= config.height as f32
                        && shader_inline_opacity_supported(source)
                    {
                        self.render_shader_layers_direct(&shader_scene, config)?
                    } else {
                        self.render_shader_layers(&shader_scene, config)?
                    };
                    let Some(Surface::GpuTexture(dst_surf)) = surfaces.get(&dst_id) else {
                        return Err(ExecutionError::SubmissionFailed {
                            pass: pid,
                            reason: "Shader destination missing".into(),
                        });
                    };
                    let (tmp_tex, tmp_view) = self.upload_rgba_texture(
                        rendered.width(),
                        rendered.height(),
                        rendered.as_raw(),
                        "shader_segment_tex",
                    );
                    match clear {
                        ClearOp::Clear(_) | ClearOp::DontCare => {
                            let mut encoder = self.ctx.device.create_command_encoder(
                                &wgpu::CommandEncoderDescriptor {
                                    label: Some("shader_segment_copy"),
                                },
                            );
                            encoder.copy_texture_to_texture(
                                tmp_tex.as_image_copy(),
                                dst_surf.texture.as_image_copy(),
                                wgpu::Extent3d {
                                    width: dst_surf.width,
                                    height: dst_surf.height,
                                    depth_or_array_layers: 1,
                                },
                            );
                            let sub = self.ctx.queue.submit(Some(encoder.finish()));
                            self.ctx.device.poll(wgpu::Maintain::wait_for(sub));
                        }
                        ClearOp::Load => {
                            let sub = self.composite_external_texture_view(
                                &tmp_view,
                                &dst_surf.view,
                                dst_surf.width,
                                dst_surf.height,
                                None,
                                1.0,
                                false,
                            )?;
                            self.ctx.device.poll(wgpu::Maintain::wait_for(sub));
                        }
                    }
                    gpu_passes += 1;
                }

                PassOp::Composite {
                    source,
                    opacity,
                    region,
                    ..
                } => {
                    let (src_surf, dst_surf) = match (surfaces.get(source), surfaces.get(&dst_id)) {
                        (Some(Surface::GpuTexture(s)), Some(Surface::GpuTexture(d))) => (s, d),
                        _ => {
                            return Err(ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Composite source/destination missing".into(),
                            })
                        }
                    };
                    let sub = self.composite_external_texture_view(
                        &src_surf.view,
                        &dst_surf.view,
                        dst_surf.width,
                        dst_surf.height,
                        *region,
                        *opacity,
                        true,
                    )?;
                    self.ctx.device.poll(wgpu::Maintain::wait_for(sub));
                    gpu_passes += 1;
                }

                PassOp::Copy { source, dst_origin } => {
                    let (src_surf, dst_surf) = match (surfaces.get(source), surfaces.get(&dst_id)) {
                        (Some(Surface::GpuTexture(s)), Some(Surface::GpuTexture(d))) => (s, d),
                        _ => {
                            return Err(ExecutionError::SubmissionFailed {
                                pass: pid,
                                reason: "Copy source/destination missing".into(),
                            })
                        }
                    };
                    let mut encoder =
                        self.ctx
                            .device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("copy_pass_encoder"),
                            });
                    encoder.copy_texture_to_texture(
                        src_surf.texture.as_image_copy(),
                        wgpu::ImageCopyTexture {
                            texture: &dst_surf.texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: dst_origin[0],
                                y: dst_origin[1],
                                z: 0,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::Extent3d {
                            width: src_surf.width,
                            height: src_surf.height,
                            depth_or_array_layers: 1,
                        },
                    );
                    let sub = self.ctx.queue.submit(Some(encoder.finish()));
                    self.ctx.device.poll(wgpu::Maintain::wait_for(sub));
                    gpu_passes += 1;
                }

                PassOp::CpuIsland {
                    scene,
                    bounds,
                    reason,
                } => {
                    let Some(Surface::GpuTexture(dst_surf)) = surfaces.get(&dst_id) else {
                        return Err(ExecutionError::SubmissionFailed {
                            pass: pid,
                            reason: "CpuIsland destination missing".into(),
                        });
                    };
                    let island_w = dst_surf.width;
                    let island_h = dst_surf.height;
                    let mut pixmap = Pixmap::new(island_w, island_h).ok_or_else(|| {
                        ExecutionError::ResourceAllocation {
                            resource: dst_id,
                            reason: "invalid CPU island dimensions".into(),
                        }
                    })?;
                    let island_nodes = if let Some(b) = bounds {
                        vec![SceneNode::Group {
                            transform: crate::scene::Transform2D::translate(-b.x, -b.y),
                            opacity: 1.0,
                            children: scene.nodes.clone(),
                        }]
                    } else {
                        scene.nodes.clone()
                    };
                    self.fallback.render_nodes_into_pixmap(
                        &mut pixmap,
                        &island_nodes,
                        config.fps,
                    )?;
                    let pixels = pixmap.take();
                    let upload_bytes = pixels.len() as u64;
                    self.ctx.queue.write_texture(
                        dst_surf.texture.as_image_copy(),
                        &pixels,
                        wgpu::ImageDataLayout {
                            offset: 0,
                            bytes_per_row: Some(island_w * 4),
                            rows_per_image: Some(island_h),
                        },
                        wgpu::Extent3d {
                            width: island_w,
                            height: island_h,
                            depth_or_array_layers: 1,
                        },
                    );
                    cpu_island_passes += 1;
                    cpu_island_pixels += u64::from(island_w) * u64::from(island_h);
                    cpu_island_upload_bytes += upload_bytes;
                    fallback_reasons.push(reason.clone());
                }
            }
        }

        if cpu_island_passes > 0 {
            self.cpu_island_frames.fetch_add(1, Ordering::Relaxed);
            self.cpu_island_pixels
                .fetch_add(cpu_island_pixels, Ordering::Relaxed);
            self.cpu_island_upload_bytes
                .fetch_add(cpu_island_upload_bytes, Ordering::Relaxed);
        }
        if gpu_passes > 0 {
            self.gpu_frame_count.fetch_add(1, Ordering::Relaxed);
        } else if cpu_island_passes > 0 {
            if let Some(first_reason) = fallback_reasons.first() {
                self.record_cpu_fallback(first_reason.as_str());
            }
        }

        let out_id = plan
            .graph
            .output
            .ok_or(crate::render_ir::GraphValidationError::MissingOutput)?;
        let out_surf = match surfaces.remove(&out_id) {
            Some(Surface::GpuTexture(s)) => s,
            _ => {
                return Err(ExecutionError::ResourceAllocation {
                    resource: out_id,
                    reason: "graph output is not a color surface".into(),
                })
            }
        };
        let out_img = self.readback_texture(&out_surf.texture, out_surf.width, out_surf.height)?;

        Ok((
            out_img,
            ExecutionReport {
                backend_name: "wgpu",
                passes_executed: plan.pass_order.len(),
                gpu_passes,
                compute_passes,
                cpu_island_passes,
                cpu_island_pixels,
                cpu_island_upload_bytes,
                transient_pool_slots: plan.transient_pool_slots,
                peak_transient_bytes: plan.peak_transient_bytes,
                fallback_reasons,
            },
        ))
    }
}

impl RenderExecutor for WgpuBackend {
    fn capabilities(&self) -> ExecutorCapabilities {
        let mut flags = CapabilityFlags::RASTER_2D_ANALYTIC
            .union(CapabilityFlags::RASTER_2D_MESH)
            .union(CapabilityFlags::RASTER_2D_IMAGE)
            .union(CapabilityFlags::RASTER_2D_TEXT)
            .union(CapabilityFlags::BLEND_BASIC)
            .union(CapabilityFlags::OFFSCREEN_COMPOSITE)
            .union(CapabilityFlags::PATH_MASK)
            .union(CapabilityFlags::SHADER_PASS)
            .union(CapabilityFlags::COMPUTE_PASS)
            .union(CapabilityFlags::COPY_OP)
            .union(CapabilityFlags::CPU_ISLAND);
        if self.ctx.supports_timestamp_queries {
            flags = flags.union(CapabilityFlags::TIMESTAMP_QUERY);
        }
        ExecutorCapabilities {
            backend_name: "wgpu",
            flags,
            max_texture_dimension_2d: self.ctx.max_texture_dimension_2d,
            supported_formats: vec![PixelFormat::Rgba8Unorm, PixelFormat::R8Unorm],
            supports_msaa: true,
            supports_timestamp_queries: self.ctx.supports_timestamp_queries,
        }
    }

    fn execute_plan(
        &self,
        plan: &ExecutionPlan,
        config: &FrameConfig,
    ) -> Result<(RgbaImage, ExecutionReport), ExecutionError> {
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

        let ordered_passes: Vec<&_> = plan
            .pass_order
            .iter()
            .filter_map(|pid| plan.graph.pass(*pid))
            .collect();

        // Fast path 1: single full-frame CpuIsland pass.
        if let [pass] = ordered_passes.as_slice() {
            if let PassOp::CpuIsland {
                scene,
                bounds: None,
                reason,
            } = &pass.op
            {
                self.record_cpu_fallback(reason.as_str());
                let is_profiling = self.is_profiling_enabled();
                let frame_started = Instant::now();
                let fb_start = if is_profiling {
                    Some(Instant::now())
                } else {
                    None
                };
                let img = self.fallback.render_frame(scene, config)?;
                let fb_render_ns = fb_start.map(|t| t.elapsed().as_nanos() as u64).unwrap_or(0);
                if is_profiling {
                    let total_ns = frame_started.elapsed().as_nanos() as u64;
                    self.record_profile_sample(FrameProfileSample {
                        frame_idx: config.frame,
                        scene_eval_ns: 0,
                        decode_ns: 0,
                        upload_ns: 0,
                        upload_bytes: 0,
                        compile_encode_ns: fb_render_ns,
                        submission_wait_ns: 0,
                        gpu_exec_ns: None,
                        readback_ns: 0,
                        readback_bytes: 0,
                        video_encode_ns: 0,
                        total_frame_ns: total_ns,
                        cpu_fallback: true,
                    });
                    self.last_session_wall_ns.store(total_ns, Ordering::Relaxed);
                }
                let pixels = u64::from(config.width) * u64::from(config.height);
                return Ok((
                    img,
                    ExecutionReport {
                        backend_name: caps.backend_name,
                        passes_executed: 1,
                        gpu_passes: 0,
                        compute_passes: 0,
                        cpu_island_passes: 1,
                        cpu_island_pixels: pixels,
                        cpu_island_upload_bytes: 0,
                        transient_pool_slots: plan.transient_pool_slots,
                        peak_transient_bytes: plan.peak_transient_bytes,
                        fallback_reasons: vec![reason.clone()],
                    },
                ));
            }
        }

        // Fast path 2: pure Shader passes targeting output.
        if !ordered_passes.is_empty()
            && ordered_passes
                .iter()
                .all(|p| matches!(p.op, PassOp::Shader { .. }))
        {
            let shader_nodes: Vec<SceneNode> = ordered_passes
                .iter()
                .map(|p| match &p.op {
                    PassOp::Shader {
                        x,
                        y,
                        w,
                        h,
                        source,
                        time,
                        params,
                        opacity,
                        ..
                    } => SceneNode::Shader {
                        x: *x,
                        y: *y,
                        w: *w,
                        h: *h,
                        source: source.clone(),
                        time: *time,
                        params: *params,
                        opacity: *opacity,
                    },
                    _ => unreachable!(),
                })
                .collect();
            let img = self.execute_shader_passes(&shader_nodes, config)?;
            return Ok((
                img,
                ExecutionReport {
                    backend_name: caps.backend_name,
                    passes_executed: ordered_passes.len(),
                    gpu_passes: ordered_passes.len(),
                    compute_passes: 0,
                    cpu_island_passes: 0,
                    cpu_island_pixels: 0,
                    cpu_island_upload_bytes: 0,
                    transient_pool_slots: plan.transient_pool_slots,
                    peak_transient_bytes: plan.peak_transient_bytes,
                    fallback_reasons: Vec::new(),
                },
            ));
        }

        // Fast path 3: 3-pass trailing overlap texture layer composite.
        if let [p0, p1, p2] = ordered_passes.as_slice() {
            if let (
                PassOp::Raster2D {
                    scene: base_scene,
                    path_mask: None,
                    ..
                },
                PassOp::Raster2D {
                    scene: layer_scene,
                    path_mask: None,
                    ..
                },
                PassOp::Composite {
                    source,
                    opacity,
                    blend_mode: crate::scene::BlendMode::Normal,
                    region: None,
                },
            ) = (&p0.op, &p1.op, &p2.op)
            {
                if p1.writes.first() == Some(source) && p0.writes.first() == p2.writes.first() {
                    if let Ok(img) = self.render_trailing_overlap_layer(
                        base_scene,
                        layer_scene,
                        *opacity,
                        config,
                    ) {
                        return Ok((
                            img,
                            ExecutionReport {
                                backend_name: caps.backend_name,
                                passes_executed: 3,
                                gpu_passes: 3,
                                compute_passes: 0,
                                cpu_island_passes: 0,
                                cpu_island_pixels: 0,
                                cpu_island_upload_bytes: 0,
                                transient_pool_slots: plan.transient_pool_slots,
                                peak_transient_bytes: plan.peak_transient_bytes,
                                fallback_reasons: Vec::new(),
                            },
                        ));
                    }
                    let mut combined_nodes = base_scene.nodes.clone();
                    combined_nodes.push(SceneNode::Layer {
                        opacity: *opacity,
                        blend_mode: crate::scene::BlendMode::Normal,
                        clip: None,
                        mask: None,
                        mask_mode: crate::scene::MaskMode::Alpha,
                        filters: Vec::new(),
                        shadow: None,
                        children: layer_scene.nodes.clone(),
                    });
                    let combined_scene = Scene {
                        nodes: combined_nodes,
                    };
                    let reason = gpu_fallback_reason(&combined_scene);
                    if reason == "overlapping texture layer requires offscreen compositing" {
                        let _ = self.offscreen_layer_slot(config.width, config.height);
                    }
                    self.record_cpu_fallback(reason);
                    let img = self.fallback.render_frame(&combined_scene, config)?;
                    return Ok((
                        img,
                        ExecutionReport {
                            backend_name: caps.backend_name,
                            passes_executed: 3,
                            gpu_passes: 0,
                            compute_passes: 0,
                            cpu_island_passes: 1,
                            cpu_island_pixels: u64::from(config.width) * u64::from(config.height),
                            cpu_island_upload_bytes: 0,
                            transient_pool_slots: plan.transient_pool_slots,
                            peak_transient_bytes: plan.peak_transient_bytes,
                            fallback_reasons: vec![reason.to_string()],
                        },
                    ));
                }
            }
        }

        // Fast path 4: single Raster2D pass (+ optional trailing inline-opacity Shader suffix).
        if let Some((first, rest)) = ordered_passes.split_first() {
            if let PassOp::Raster2D {
                scene,
                path_mask: None,
                ..
            } = &first.op
            {
                if rest.iter().all(|p| matches!(p.op, PassOp::Shader { .. })) {
                    let shader_nodes: Vec<SceneNode> = rest
                        .iter()
                        .map(|p| match &p.op {
                            PassOp::Shader {
                                x,
                                y,
                                w,
                                h,
                                source,
                                time,
                                params,
                                opacity,
                                ..
                            } => SceneNode::Shader {
                                x: *x,
                                y: *y,
                                w: *w,
                                h: *h,
                                source: source.clone(),
                                time: *time,
                                params: *params,
                                opacity: *opacity,
                            },
                            _ => unreachable!(),
                        })
                        .collect();
                    let full_scene = if shader_nodes.is_empty() {
                        None
                    } else {
                        let mut nodes = scene.nodes.clone();
                        nodes.extend(shader_nodes.iter().cloned());
                        Some(Scene { nodes })
                    };
                    let suffix = (!shader_nodes.is_empty()).then_some(shader_nodes.as_slice());
                    let (img, islands, reason) = self.execute_single_raster_with_optional_shaders(
                        scene,
                        None,
                        suffix,
                        config,
                        full_scene.as_ref(),
                    )?;
                    return Ok((
                        img,
                        ExecutionReport {
                            backend_name: caps.backend_name,
                            passes_executed: ordered_passes.len(),
                            gpu_passes: if islands == 0 {
                                ordered_passes.len()
                            } else {
                                0
                            },
                            compute_passes: 0,
                            cpu_island_passes: islands,
                            cpu_island_pixels: if islands > 0 {
                                u64::from(config.width) * u64::from(config.height)
                            } else {
                                0
                            },
                            cpu_island_upload_bytes: 0,
                            transient_pool_slots: plan.transient_pool_slots,
                            peak_transient_bytes: plan.peak_transient_bytes,
                            fallback_reasons: reason.into_iter().collect(),
                        },
                    ));
                }
            }
        }

        // Fast path 5: explicit PathMask -> Raster2D two-pass graph.
        if let [p0, p1] = ordered_passes.as_slice() {
            if let (
                PassOp::PathMask { path_d, transform },
                PassOp::Raster2D {
                    scene,
                    path_mask: Some(mask_id),
                    ..
                },
            ) = (&p0.op, &p1.op)
            {
                if p0.writes.first() == Some(mask_id) {
                    let mask =
                        gpu_path_mask_from_svg(path_d, transform.to_tiny_skia(), Color::WHITE)
                            .ok_or_else(|| ExecutionError::SubmissionFailed {
                                pass: PassId(p0.id.0),
                                reason: "failed to tessellate PathMask SVG path".into(),
                            })?;
                    let (img, islands, reason) = self.execute_single_raster_with_optional_shaders(
                        scene,
                        Some(mask),
                        None,
                        config,
                        None,
                    )?;
                    return Ok((
                        img,
                        ExecutionReport {
                            backend_name: caps.backend_name,
                            passes_executed: 2,
                            gpu_passes: if islands == 0 { 2 } else { 0 },
                            compute_passes: 0,
                            cpu_island_passes: islands,
                            cpu_island_pixels: if islands > 0 {
                                u64::from(config.width) * u64::from(config.height)
                            } else {
                                0
                            },
                            cpu_island_upload_bytes: 0,
                            transient_pool_slots: plan.transient_pool_slots,
                            peak_transient_bytes: plan.peak_transient_bytes,
                            fallback_reasons: reason.into_iter().collect(),
                        },
                    ));
                }
            }
        }

        // General multi-pass GPU-resident DAG execution (compute passes, bounded CPU islands, composites).
        self.execute_general_dag(plan, config)
    }
}
