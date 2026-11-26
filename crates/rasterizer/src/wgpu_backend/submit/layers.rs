//! Offscreen layer slots, external-texture compositing and trailing-overlap layer rendering.

use super::*;

impl WgpuBackend {
    pub(crate) fn offscreen_layer_slot(&self, width: u32, height: u32) -> Arc<Mutex<GpuFrameSlot>> {
        let mut pool = self
            .layer_resources
            .lock()
            .expect("GPU layer resource pool mutex poisoned");
        pool.entry((width, height))
            .or_insert_with(|| {
                Arc::new(Mutex::new(GpuFrameSlot::new(
                    &self.ctx.device,
                    width,
                    height,
                    false,
                )))
            })
            .clone()
    }

    /// Create the shared texture bind group used by image, video, text-atlas,
    /// and future resolved offscreen-layer inputs. The texture view remains
    /// owned by the caller, which is required for layer-slot ownership.
    pub(crate) fn image_bind_group(
        &self,
        view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        label: &'static str,
    ) -> wgpu::BindGroup {
        self.ctx
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.ctx.image_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
    }

    #[allow(dead_code)]
    pub(crate) fn offscreen_layer_binding(&self, slot: &GpuFrameSlot) -> GpuExternalTexture {
        let sampler = self.ctx.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("offscreen_layer_sampler"),
            ..Default::default()
        });
        let bind_group =
            self.image_bind_group(&slot.texture_view, &sampler, "offscreen_layer_texture_bg");
        GpuExternalTexture {
            _sampler: sampler,
            bind_group,
        }
    }

    /// Composite a resolved layer texture into another resolved target. This
    /// pass intentionally uses the single-sample pipeline: the destination is
    /// already resolved, so loading it as an MSAA attachment would be invalid.
    #[allow(dead_code)]
    pub(crate) fn composite_external_texture(
        &self,
        source: &GpuFrameSlot,
        destination: &GpuFrameSlot,
        width: u32,
        height: u32,
        opacity: f32,
    ) -> Result<wgpu::SubmissionIndex, RasterError> {
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err(RasterError::Scene("invalid offscreen layer opacity".into()));
        }
        let device = &self.ctx.device;
        let globals = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("layer_composite_globals"),
            contents: bytemuck_cast(&[width as f32, height as f32, 0.0, 0.0]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("layer_composite_globals_bg"),
            layout: &self.ctx.globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let mut instance = GpuInstance::solid(Color::WHITE, opacity, Transform::identity());
        instance.kind_data[0] = 5;
        instance.bounds = [0.0, 0.0, width as f32, height as f32];
        instance.shape_bounds = instance.bounds;
        instance.params = [0.0, 0.0, 1.0, 1.0];
        let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("layer_composite_instance"),
            contents: bytemuck_cast(&[instance]),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let instances_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("layer_composite_instances_bg"),
            layout: &self.ctx.instance_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: instances.as_entire_binding(),
            }],
        });
        let external = self.offscreen_layer_binding(source);
        let mask_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("layer_composite_dummy_mask"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mask_view = mask_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mask_sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let mask_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("layer_composite_dummy_mask_bg"),
            layout: &self.ctx.path_mask_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&mask_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&mask_sampler),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("layer_composite_encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layer_composite_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &destination.texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.ctx.image_composite_pipeline);
            pass.set_bind_group(0, &globals_bg, &[]);
            pass.set_bind_group(1, &instances_bg, &[]);
            pass.set_bind_group(2, &external.bind_group, &[]);
            pass.set_bind_group(3, &mask_bg, &[]);
            pass.draw(0..6, 0..1);
        }
        Ok(self.ctx.queue.submit(Some(encoder.finish())))
    }

    pub(crate) fn readback_resolved_slot(
        &self,
        slot: &GpuFrameSlot,
        width: u32,
        height: u32,
    ) -> Result<RgbaImage, RasterError> {
        let mut encoder = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("layer_composite_readback_encoder"),
            });
        encoder.copy_texture_to_buffer(
            slot.texture.as_image_copy(),
            wgpu::ImageCopyBuffer {
                buffer: &slot.readback,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(slot.bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let submission = self.ctx.queue.submit(Some(encoder.finish()));
        let (tx, rx) = std::sync::mpsc::channel();
        slot.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        let _ = submission;
        self.ctx.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|error| RasterError::Scene(format!("layer readback channel failed: {error}")))?
            .map_err(|error| RasterError::Scene(format!("layer readback map failed: {error}")))?;
        let slice = slot.readback.slice(..);
        let mapped = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for row in 0..height {
            let start = (row * slot.bytes_per_row) as usize;
            pixels.extend_from_slice(&mapped[start..start + (width * 4) as usize]);
        }
        drop(mapped);
        slot.readback.unmap();
        RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| RasterError::ImageEncode("Invalid layer composite readback".into()))
    }

    pub(crate) fn render_trailing_overlap_layer(
        &self,
        base_scene: &Scene,
        layer_scene: &Scene,
        layer_opacity: f32,
        config: &FrameConfig,
    ) -> Result<RgbaImage, RasterError> {
        let Some((base_commands, base_mask, _)) =
            compile_scene_with_path_mask(base_scene, &self.fallback)
        else {
            return Err(RasterError::Scene(
                "overlap base scene requires CPU fallback".into(),
            ));
        };
        let Some((layer_commands, layer_mask, _)) =
            compile_scene_with_path_mask(layer_scene, &self.fallback)
        else {
            return Err(RasterError::Scene(
                "overlap layer requires CPU fallback".into(),
            ));
        };
        if base_mask.is_some() || layer_mask.is_some() {
            return Err(RasterError::Scene(
                "overlap layer path masks are not yet supported".into(),
            ));
        }
        let resource = {
            let mut pool = self
                .frame_resources
                .lock()
                .map_err(|_| RasterError::Init("GPU resource pool mutex poisoned".into()))?;
            pool.entry((config.width, config.height))
                .or_insert_with(|| {
                    Arc::new(Mutex::new(GpuFrameResources::new(
                        &self.ctx.device,
                        config.width,
                        config.height,
                        self.ctx.supports_timestamp_queries,
                    )))
                })
                .clone()
        };
        let mut resources = resource
            .lock()
            .map_err(|_| RasterError::Init("GPU frame resource mutex poisoned".into()))?;
        let output_index = resources.active_index % RING_BUFFER_SIZE;
        resources.active_index = resources.active_index.wrapping_add(1);
        let layer_slot = self.offscreen_layer_slot(config.width, config.height);
        let layer_slot = layer_slot
            .lock()
            .map_err(|_| RasterError::Init("GPU layer resource mutex poisoned".into()))?;
        let output_slot = &resources.slots[output_index];
        let base_frame = self.submit_frame_to_slot(FrameSubmission {
            commands: &base_commands,
            path_mask: None,
            width: config.width,
            height: config.height,
            sampling_fps: config.fps,
            slot: output_slot,
            shader_suffix: None,
            readback: false,
        })?;
        let base_submission = base_frame.submission_index;
        let layer_frame = self.submit_frame_to_slot(FrameSubmission {
            commands: &layer_commands,
            path_mask: None,
            width: config.width,
            height: config.height,
            sampling_fps: config.fps,
            slot: &layer_slot,
            shader_suffix: None,
            readback: false,
        })?;
        let layer_submission = layer_frame.submission_index;
        self.ctx
            .device
            .poll(wgpu::Maintain::wait_for(base_submission));
        self.ctx
            .device
            .poll(wgpu::Maintain::wait_for(layer_submission));
        let composite_submission = self.composite_external_texture(
            &layer_slot,
            output_slot,
            config.width,
            config.height,
            layer_opacity,
        )?;
        self.ctx
            .device
            .poll(wgpu::Maintain::wait_for(composite_submission));
        let image = self.readback_resolved_slot(output_slot, config.width, config.height)?;
        self.gpu_frame_count.fetch_add(1, Ordering::Relaxed);
        Ok(image)
    }
}
