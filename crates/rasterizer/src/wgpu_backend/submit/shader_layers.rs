//! Interleaved shader-layer rendering and compositing.

use super::*;

impl WgpuBackend {
    pub(crate) fn render_interleaved_shader_scene(
        &self,
        scene: &Scene,
        config: &FrameConfig,
    ) -> Result<RgbaImage, RasterError> {
        let mut output = RgbaImage::new(config.width, config.height);
        let mut regular_nodes = Vec::new();
        for node in &scene.nodes {
            if matches!(node, SceneNode::Shader { .. }) {
                if !regular_nodes.is_empty() {
                    let segment = Scene {
                        nodes: std::mem::take(&mut regular_nodes),
                    };
                    let rendered = self.render_frame(&segment, config)?;
                    image::imageops::overlay(&mut output, &rendered, 0, 0);
                }
                let shader_scene = Scene {
                    nodes: vec![node.clone()],
                };
                let rendered = match node {
                    SceneNode::Shader {
                        x,
                        y,
                        w,
                        h,
                        source,
                        opacity,
                        ..
                    } if *opacity >= 0.0
                        && *opacity <= 1.0
                        && *x >= 0.0
                        && *y >= 0.0
                        && *w > 0.0
                        && *h > 0.0
                        && *x + *w <= config.width as f32
                        && *y + *h <= config.height as f32
                        && shader_opacity_supported(source) =>
                    {
                        self.render_shader_layers_direct(&shader_scene, config)?
                    }
                    _ => self.render_shader_layers(&shader_scene, config)?,
                };
                image::imageops::overlay(&mut output, &rendered, 0, 0);
            } else {
                regular_nodes.push(node.clone());
            }
        }
        if !regular_nodes.is_empty() {
            let rendered = self.render_frame(
                &Scene {
                    nodes: regular_nodes,
                },
                config,
            )?;
            image::imageops::overlay(&mut output, &rendered, 0, 0);
        }
        Ok(output)
    }

    /// Render opaque shader layers directly into one GPU target and read it
    /// back once. Opacity layers retain the compatibility path until opacity
    /// is carried as a target-pass uniform.
    pub(crate) fn render_shader_layers_direct(
        &self,
        scene: &Scene,
        config: &FrameConfig,
    ) -> Result<RgbaImage, RasterError> {
        let width = config.width;
        let height = config.height;
        let texture = self.ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shader_scene_target"),
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
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("shader_scene_encoder"),
            });
        for (index, node) in scene.nodes.iter().enumerate() {
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
                unreachable!("direct shader path was checked before rendering");
            };
            if ![*x, *y, *w, *h, *time, *opacity]
                .iter()
                .all(|value| value.is_finite())
                || !(*opacity >= 0.0 && *opacity <= 1.0)
                || *w <= 0.0
                || *h <= 0.0
                || *x < 0.0
                || *y < 0.0
            {
                return Err(RasterError::Scene("invalid direct shader region".into()));
            }
            let shader_source = shader_source_with_opacity(source, *opacity);
            self.shader_runner.render_into_region(
                &mut encoder,
                &view,
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
                    load: if index == 0 {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    } else {
                        wgpu::LoadOp::Load
                    },
                },
            )?;
        }
        let bytes_per_row = (width * 4 + 255) & !255;
        let staging = self.ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shader_scene_readback"),
            size: (bytes_per_row * height) as wgpu::BufferAddress,
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
        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.ctx.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|error| RasterError::Scene(format!("GPU readback channel failed: {error}")))?
            .map_err(|error| RasterError::Scene(format!("GPU readback map failed: {error}")))?;
        let mapped = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for row in 0..height {
            let start = (row * bytes_per_row) as usize;
            pixels.extend_from_slice(&mapped[start..start + (width * 4) as usize]);
        }
        drop(mapped);
        staging.unmap();
        RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| RasterError::ImageEncode("Invalid shader scene readback".into()))
    }

    /// Render shader nodes on the GPU and composite their rectangular outputs
    /// in scene order. Non-shader nodes deliberately remain outside this path
    /// until an offscreen GPU texture is available for general scene blending.
    pub(crate) fn render_shader_layers(
        &self,
        scene: &Scene,
        config: &FrameConfig,
    ) -> Result<RgbaImage, RasterError> {
        let mut output = RgbaImage::new(config.width, config.height);
        self.composite_shader_layers(&mut output, &scene.nodes)?;
        Ok(output)
    }

    pub(crate) fn composite_shader_layers(
        &self,
        output: &mut RgbaImage,
        nodes: &[SceneNode],
    ) -> Result<(), RasterError> {
        for node in nodes {
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
                unreachable!("shader layer path was checked before rendering");
            };
            if ![*x, *y, *w, *h, *opacity]
                .iter()
                .all(|value| value.is_finite())
                || *w <= 0.0
                || *h <= 0.0
                || !(0.0..=1.0).contains(opacity)
            {
                return Err(RasterError::Scene(
                    "invalid shader layer bounds or opacity".into(),
                ));
            }
            let width = (*w).round().max(1.0) as u32;
            let height = (*h).round().max(1.0) as u32;
            let mut layer = self
                .shader_runner
                .render(width, height, *time, *params, source)?;
            if *opacity < 1.0 {
                for pixel in layer.pixels_mut() {
                    pixel[3] = (f32::from(pixel[3]) * *opacity).round() as u8;
                }
            }
            image::imageops::overlay(output, &layer, (*x).round() as i64, (*y).round() as i64);
        }
        Ok(())
    }
}
