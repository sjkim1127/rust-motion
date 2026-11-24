//! Frame submission: records and submits the command buffers for one frame.

use super::*;

impl WgpuBackend {
    pub(crate) fn submit_frame_to_slot(
        &self,
        submission: FrameSubmission<'_>,
    ) -> Result<SubmittedFrame, RasterError> {
        let commands = submission.commands;
        let path_mask = submission.path_mask;
        let width = submission.width;
        let height = submission.height;
        let sampling_fps = submission.sampling_fps;
        let slot = submission.slot;
        let shader_suffix = submission.shader_suffix;
        let readback = submission.readback;
        let device = &self.ctx.device;
        let queue = &self.ctx.queue;

        // Globals uniform
        let globals_data: [f32; 4] = [width as f32, height as f32, 0.0, 0.0];
        let globals_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("globals_buf"),
            contents: bytemuck_cast(&globals_data),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals_bg"),
            layout: &self.ctx.globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame_encoder"),
        });

        let mut all_instances: Vec<GpuInstance> = commands.iter().map(|c| *c.instance()).collect();
        let mut image_resources = Vec::with_capacity(commands.len());
        let atlas_snapshot = self.fallback.take_text_atlas_snapshot();
        let atlas_resource = self.gpu_text_atlas(&atlas_snapshot);
        for (index, command) in commands.iter().enumerate() {
            let (source, fit) = match command {
                DrawCommand::Image { src, fit, .. } => (self.gpu_image(src)?, *fit),
                DrawCommand::Video {
                    src,
                    time,
                    looped,
                    fit,
                    ..
                } => (self.gpu_video(src, *time, sampling_fps, *looped)?, *fit),
                DrawCommand::Lottie { key, image, .. } => {
                    (self.gpu_pixels(key, image)?, ImageFit::Contain)
                }
                DrawCommand::Gif {
                    key, image, fit, ..
                } => (self.gpu_pixels(key, image)?, *fit),
                DrawCommand::Emoji { key, image, .. } => {
                    (self.gpu_pixels(key, image)?, ImageFit::Fill)
                }
                DrawCommand::AudioVisualizer { key, image, .. } => {
                    (self.gpu_pixels(key, image)?, ImageFit::Fill)
                }
                DrawCommand::Text { entry, .. } => {
                    all_instances[index].params[0] = entry.x as f32 / atlas_snapshot.width as f32;
                    all_instances[index].params[1] = entry.y as f32 / atlas_snapshot.height as f32;
                    all_instances[index].params[2] =
                        (entry.x + entry.width) as f32 / atlas_snapshot.width as f32;
                    all_instances[index].params[3] =
                        (entry.y + entry.height) as f32 / atlas_snapshot.height as f32;
                    image_resources.push(None);
                    continue;
                }
                _ => {
                    image_resources.push(None);
                    continue;
                }
            };
            let placement = image_placement(
                fit,
                source.width as f32,
                source.height as f32,
                all_instances[index].shape_bounds[0],
                all_instances[index].shape_bounds[1],
                all_instances[index].shape_bounds[2],
                all_instances[index].shape_bounds[3],
            )
            .ok_or_else(|| RasterError::ImageAsset {
                path: "video/image".into(),
                reason: "invalid image placement dimensions".into(),
            })?;
            all_instances[index].bounds = placement.destination;
            all_instances[index].shape_bounds = placement.destination;
            all_instances[index].params[0..4].copy_from_slice(&placement.source_uv);

            image_resources.push(Some(source));
        }

        let path_mask_instance_index = all_instances.len() as u32;
        if let Some(path_mask) = path_mask {
            all_instances.push(path_mask.instance);
        }

        if !all_instances.is_empty() {
            let instance_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("instances_storage_buf"),
                contents: bytemuck_cast(&all_instances),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let instance_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("instances_bg"),
                layout: &self.ctx.instance_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: instance_buf.as_entire_binding(),
                }],
            });

            // The shared pipeline layout requires group 2 even for analytic
            // and mesh draws. A 1x1 dummy texture keeps those pipelines valid;
            // image draws replace it with their per-command texture group.
            let dummy_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("dummy_image_texture"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                dummy_texture.as_image_copy(),
                &[0, 0, 0, 0],
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            let dummy_view = dummy_texture.create_view(&wgpu::TextureViewDescriptor::default());
            let dummy_sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
            let dummy_image_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("dummy_image_bg"),
                layout: &self.ctx.image_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&dummy_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&dummy_sampler),
                    },
                ],
            });

            let default_path_mask_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("default_path_mask_texture"),
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
            queue.write_texture(
                default_path_mask_texture.as_image_copy(),
                &[255],
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(1),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            let default_path_mask_view =
                default_path_mask_texture.create_view(&wgpu::TextureViewDescriptor::default());
            let default_path_mask_sampler =
                device.create_sampler(&wgpu::SamplerDescriptor::default());
            let default_path_mask_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("default_path_mask_bg"),
                layout: &self.ctx.path_mask_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&default_path_mask_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&default_path_mask_sampler),
                    },
                ],
            });

            let (_path_mask_texture, path_mask_view) = if path_mask.is_some() {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("path_mask_texture"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R8Unorm,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                (texture, view)
            } else {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("default_path_mask_texture"),
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
                queue.write_texture(
                    texture.as_image_copy(),
                    &[255],
                    wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(1),
                        rows_per_image: Some(1),
                    },
                    wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                );
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                (texture, view)
            };
            let path_mask_sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
            let path_mask_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("path_mask_bg"),
                layout: &self.ctx.path_mask_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&path_mask_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&path_mask_sampler),
                    },
                ],
            });

            let mesh_buffers: Vec<Option<(wgpu::Buffer, wgpu::Buffer)>> = commands
                .iter()
                .map(|cmd| match cmd {
                    DrawCommand::Mesh {
                        vertices, indices, ..
                    } => {
                        let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("path_vertices"),
                            contents: bytemuck_cast(vertices),
                            usage: wgpu::BufferUsages::VERTEX,
                        });
                        let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("path_indices"),
                            contents: bytemuck_cast(indices),
                            usage: wgpu::BufferUsages::INDEX,
                        });
                        Some((vb, ib))
                    }
                    DrawCommand::Analytic { .. }
                    | DrawCommand::Image { .. }
                    | DrawCommand::Video { .. }
                    | DrawCommand::Lottie { .. }
                    | DrawCommand::Gif { .. }
                    | DrawCommand::Emoji { .. }
                    | DrawCommand::AudioVisualizer { .. }
                    | DrawCommand::Text { .. } => None,
                })
                .collect();

            let path_mask_buffers = path_mask.map(|mask| {
                let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("path_mask_vertices"),
                    contents: bytemuck_cast(&mask.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("path_mask_indices"),
                    contents: bytemuck_cast(&mask.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
                (vertex_buffer, index_buffer)
            });

            if let (Some(mask), Some((vertex_buffer, index_buffer))) =
                (path_mask, path_mask_buffers.as_ref())
            {
                let mut mask_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("path_mask_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &path_mask_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                mask_pass.set_pipeline(&self.ctx.path_mask_pipeline);
                mask_pass.set_bind_group(0, &globals_bg, &[]);
                mask_pass.set_bind_group(1, &instance_bg, &[]);
                mask_pass.set_bind_group(2, &dummy_image_bg, &[]);
                mask_pass.set_bind_group(3, &default_path_mask_bg, &[]);
                mask_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                mask_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                mask_pass.draw_indexed(
                    0..mask.indices.len() as u32,
                    0,
                    path_mask_instance_index..path_mask_instance_index + 1,
                );
            }

            let timestamp_writes = if self.is_profiling_enabled() {
                slot.query_set
                    .as_ref()
                    .map(|qs| wgpu::RenderPassTimestampWrites {
                        query_set: qs,
                        beginning_of_pass_write_index: Some(0),
                        end_of_pass_write_index: Some(1),
                    })
            } else {
                None
            };

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &slot.msaa_view,
                    resolve_target: Some(&slot.texture_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes,
                occlusion_query_set: None,
            });

            pass.set_bind_group(0, &globals_bg, &[]);
            pass.set_bind_group(1, &instance_bg, &[]);
            pass.set_bind_group(2, &dummy_image_bg, &[]);
            pass.set_bind_group(3, &path_mask_bg, &[]);

            let mut i = 0;
            while i < commands.len() {
                match &commands[i] {
                    DrawCommand::Analytic { .. } => {
                        let start = i;
                        while i < commands.len()
                            && matches!(commands[i], DrawCommand::Analytic { .. })
                            && commands[i].instance().kind_data[2]
                                == commands[start].instance().kind_data[2]
                        {
                            i += 1;
                        }
                        let pipeline = match commands[start].instance().kind_data[2] {
                            1 => &self.ctx.multiply_pipeline,
                            2 => &self.ctx.screen_pipeline,
                            3 => &self.ctx.darken_pipeline,
                            4 => &self.ctx.lighten_pipeline,
                            _ => &self.ctx.pipeline,
                        };
                        pass.set_pipeline(pipeline);
                        pass.draw(0..6, start as u32..i as u32);
                    }
                    DrawCommand::Mesh { indices, .. } => {
                        let (vb, ib) = mesh_buffers[i].as_ref().expect("mesh buffers allocated");
                        let pipeline = match commands[i].instance().kind_data[2] {
                            1 => &self.ctx.multiply_mesh_pipeline,
                            2 => &self.ctx.screen_mesh_pipeline,
                            3 => &self.ctx.darken_mesh_pipeline,
                            4 => &self.ctx.lighten_mesh_pipeline,
                            _ => &self.ctx.mesh_pipeline,
                        };
                        pass.set_pipeline(pipeline);
                        pass.set_vertex_buffer(0, vb.slice(..));
                        pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..indices.len() as u32, 0, i as u32..i as u32 + 1);
                        i += 1;
                    }
                    DrawCommand::Image { .. }
                    | DrawCommand::Video { .. }
                    | DrawCommand::Lottie { .. }
                    | DrawCommand::Gif { .. }
                    | DrawCommand::Emoji { .. }
                    | DrawCommand::AudioVisualizer { .. }
                    | DrawCommand::Text { .. } => {
                        if matches!(commands[i], DrawCommand::Text { .. }) {
                            let pipeline = match commands[i].instance().kind_data[2] {
                                1 => &self.ctx.multiply_text_pipeline,
                                2 => &self.ctx.screen_text_pipeline,
                                3 => &self.ctx.darken_text_pipeline,
                                4 => &self.ctx.lighten_text_pipeline,
                                _ => &self.ctx.text_pipeline,
                            };
                            pass.set_pipeline(pipeline);
                            pass.set_bind_group(2, &atlas_resource.bind_group, &[]);
                        } else {
                            let pipeline = match commands[i].instance().kind_data[2] {
                                1 => &self.ctx.multiply_image_pipeline,
                                2 => &self.ctx.screen_image_pipeline,
                                3 => &self.ctx.darken_image_pipeline,
                                4 => &self.ctx.lighten_image_pipeline,
                                _ => &self.ctx.image_pipeline,
                            };
                            pass.set_pipeline(pipeline);
                            pass.set_bind_group(
                                2,
                                &image_resources[i]
                                    .as_ref()
                                    .expect("image resource")
                                    .bind_group,
                                &[],
                            );
                        }
                        pass.draw(0..6, i as u32..i as u32 + 1);
                        i += 1;
                    }
                }
            }
        } else {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &slot.msaa_view,
                    resolve_target: Some(&slot.texture_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }

        if let Some(shader_nodes) = shader_suffix {
            for node in shader_nodes {
                let SceneNode::Shader {
                    x,
                    y,
                    w,
                    h,
                    source,
                    time,
                    params,
                    opacity,
                } = node
                else {
                    return Err(RasterError::Scene(
                        "GPU shader suffix contains a non-shader node".into(),
                    ));
                };
                if ![*x, *y, *w, *h, *time, *opacity]
                    .iter()
                    .all(|value| value.is_finite())
                    || !(*opacity >= 0.0 && *opacity <= 1.0)
                    || *x < 0.0
                    || *y < 0.0
                    || *w <= 0.0
                    || *h <= 0.0
                    || *x + *w > width as f32
                    || *y + *h > height as f32
                {
                    return Err(RasterError::Scene(
                        "invalid GPU shader suffix region".into(),
                    ));
                }
                let shader_source = shader_source_with_opacity(source, *opacity);
                self.shader_runner.render_into_region(
                    &mut encoder,
                    &slot.texture_view,
                    crate::shader::ShaderRenderRegion {
                        target_width: width,
                        target_height: height,
                        x: *x,
                        y: *y,
                        width: *w,
                        height: *h,
                    },
                    crate::shader::ShaderRenderParams {
                        time: *time,
                        params: *params,
                        source: &shader_source,
                        load: wgpu::LoadOp::Load,
                    },
                )?;
            }
        }

        if readback {
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
        }

        let has_queries = if self.is_profiling_enabled() && readback {
            if let (Some(qs), Some(resolve_buf), Some(readback_buf)) = (
                &slot.query_set,
                &slot.query_resolve_buffer,
                &slot.query_readback_buffer,
            ) {
                encoder.resolve_query_set(qs, 0..2, resolve_buf, 0);
                encoder.copy_buffer_to_buffer(resolve_buf, 0, readback_buf, 0, 16);
                Some(readback_buf)
            } else {
                None
            }
        } else {
            None
        };

        let submission_index = queue.submit([encoder.finish()]);

        let query_rx = if let Some(readback_buf) = has_queries {
            let (tx, rx) = std::sync::mpsc::channel();
            readback_buf
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = tx.send(result);
                });
            Some(rx)
        } else {
            None
        };

        let rx = if readback {
            let (tx, rx) = std::sync::mpsc::channel();
            slot.readback
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = tx.send(result);
                });
            Some(rx)
        } else {
            None
        };

        Ok(SubmittedFrame {
            submission_index,
            readback_rx: rx,
            query_rx,
        })
    }
}
