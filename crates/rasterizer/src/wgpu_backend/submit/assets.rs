//! GPU-side asset access: decoded pixels, image/video textures and the text atlas.

use super::*;

impl WgpuBackend {
    pub(crate) fn gpu_pixels(
        &self,
        key: &str,
        decoded: &image::RgbaImage,
    ) -> Result<Arc<GpuImageResource>, RasterError> {
        let mut cache = self
            .gpu_images
            .lock()
            .expect("GPU image cache lock poisoned");
        if let Some(image) = cache.images.get(key).cloned() {
            self.gpu_texture_cache_hits.fetch_add(1, Ordering::Relaxed);
            cache.lru.retain(|entry| entry != key);
            cache.lru.push_back(key.to_string());
            return Ok(image);
        }
        drop(cache);
        self.gpu_texture_cache_misses
            .fetch_add(1, Ordering::Relaxed);
        let upload_start = Instant::now();

        let device = &self.ctx.device;
        let queue = &self.ctx.queue;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image_texture"),
            size: wgpu::Extent3d {
                width: decoded.width(),
                height: decoded.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: RENDER_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            decoded.as_raw(),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(decoded.width() * 4),
                rows_per_image: Some(decoded.height()),
            },
            wgpu::Extent3d {
                width: decoded.width(),
                height: decoded.height(),
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let bind_group = self.image_bind_group(&view, &sampler, "image_bg");
        let resource = Arc::new(GpuImageResource {
            _texture: texture,
            _view: view,
            _sampler: sampler,
            bind_group,
            width: decoded.width(),
            height: decoded.height(),
        });
        self.texture_upload_ns
            .fetch_add(upload_start.elapsed().as_nanos() as u64, Ordering::Relaxed);
        self.gpu_texture_uploads.fetch_add(1, Ordering::Relaxed);
        self.gpu_texture_upload_bytes.fetch_add(
            u64::from(resource.width) * u64::from(resource.height) * 4,
            Ordering::Relaxed,
        );
        let mut cache = self
            .gpu_images
            .lock()
            .expect("GPU image cache lock poisoned");
        if let Some(existing) = cache.images.get(key).cloned() {
            cache.lru.retain(|entry| entry != key);
            cache.lru.push_back(key.to_string());
            return Ok(existing);
        }
        let bytes = resource.width as usize * resource.height as usize * 4;
        cache.images.insert(key.to_string(), Arc::clone(&resource));
        cache.lru.push_back(key.to_string());
        cache.bytes = cache.bytes.saturating_add(bytes);
        cache.trim_to_budget();
        Ok(resource)
    }

    pub(crate) fn gpu_image(&self, src: &str) -> Result<Arc<GpuImageResource>, RasterError> {
        let decode_start = Instant::now();
        let decoded = self.image_cache.load(src)?;
        self.video_decode_ns
            .fetch_add(decode_start.elapsed().as_nanos() as u64, Ordering::Relaxed);
        // ImageCache canonicalizes local paths before indexing them. Keep the
        // GPU cache on the same identity so alternate spellings of one asset
        // (relative, absolute, or file://) do not trigger duplicate uploads.
        let key = if src.trim_start().starts_with("data:") {
            src.trim().to_string()
        } else {
            canonical_local_path(src)?.display().to_string()
        };
        self.gpu_pixels(&key, &decoded)
    }

    pub(crate) fn gpu_text_atlas(
        &self,
        snapshot: &crate::text_atlas::TextAtlasSnapshot,
    ) -> Arc<GpuTextAtlasResource> {
        let queue = &self.ctx.queue;
        let mut cache = self
            .text_atlas
            .lock()
            .expect("text atlas GPU lock poisoned");
        if let Some(resource) = cache.as_ref() {
            if resource.width == snapshot.width && resource.height == snapshot.height {
                let generation = resource.generation.load(Ordering::Acquire);
                if generation == snapshot.generation {
                    self.text_atlas_cache_hits.fetch_add(1, Ordering::Relaxed);
                    return resource.clone();
                }
                if let Some(rect) = snapshot.dirty {
                    let mut upload = Vec::with_capacity((rect.width * rect.height) as usize);
                    for row in 0..rect.height {
                        let start = ((rect.y + row) * snapshot.width + rect.x) as usize;
                        upload.extend_from_slice(
                            &snapshot.pixels[start..start + rect.width as usize],
                        );
                    }
                    let upload_start = Instant::now();
                    queue.write_texture(
                        wgpu::ImageCopyTexture {
                            texture: &resource._texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: rect.x,
                                y: rect.y,
                                z: 0,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        &upload,
                        wgpu::ImageDataLayout {
                            offset: 0,
                            bytes_per_row: Some(rect.width),
                            rows_per_image: Some(rect.height),
                        },
                        wgpu::Extent3d {
                            width: rect.width,
                            height: rect.height,
                            depth_or_array_layers: 1,
                        },
                    );
                    self.text_atlas_upload_ns
                        .fetch_add(upload_start.elapsed().as_nanos() as u64, Ordering::Relaxed);
                    self.text_atlas_upload_bytes.fetch_add(
                        u64::from(rect.width) * u64::from(rect.height),
                        Ordering::Relaxed,
                    );
                    self.text_atlas_dirty_uploads
                        .fetch_add(1, Ordering::Relaxed);
                    self.text_atlas_dirty_upload_bytes.fetch_add(
                        u64::from(rect.width) * u64::from(rect.height),
                        Ordering::Relaxed,
                    );
                    resource
                        .generation
                        .store(snapshot.generation, Ordering::Release);
                    return resource.clone();
                }
            }
        }
        let device = &self.ctx.device;
        let queue = &self.ctx.queue;
        self.text_atlas_cache_misses.fetch_add(1, Ordering::Relaxed);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("text_atlas_texture"),
            size: wgpu::Extent3d {
                width: snapshot.width,
                height: snapshot.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let upload_start = Instant::now();
        queue.write_texture(
            texture.as_image_copy(),
            &snapshot.pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(snapshot.width),
                rows_per_image: Some(snapshot.height),
            },
            wgpu::Extent3d {
                width: snapshot.width,
                height: snapshot.height,
                depth_or_array_layers: 1,
            },
        );
        self.text_atlas_upload_ns
            .fetch_add(upload_start.elapsed().as_nanos() as u64, Ordering::Relaxed);
        self.text_atlas_upload_bytes.fetch_add(
            u64::from(snapshot.width) * u64::from(snapshot.height),
            Ordering::Relaxed,
        );
        self.text_atlas_full_uploads.fetch_add(1, Ordering::Relaxed);
        self.text_atlas_full_upload_bytes.fetch_add(
            u64::from(snapshot.width) * u64::from(snapshot.height),
            Ordering::Relaxed,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let bind_group = self.image_bind_group(&view, &sampler, "text_atlas_bg");
        let resource = Arc::new(GpuTextAtlasResource {
            _texture: texture,
            _view: view,
            _sampler: sampler,
            bind_group,
            width: snapshot.width,
            height: snapshot.height,
            generation: AtomicU64::new(snapshot.generation),
        });
        *cache = Some(resource.clone());
        resource
    }

    pub(crate) fn gpu_video(
        &self,
        src: &str,
        time: f64,
        sampling_fps: f64,
        looped: bool,
    ) -> Result<Arc<GpuImageResource>, RasterError> {
        let decode_start = Instant::now();
        let frame = self.video_cache.load(src, time, sampling_fps, looped)?;
        self.video_decode_ns
            .fetch_add(decode_start.elapsed().as_nanos() as u64, Ordering::Relaxed);
        let frame_index = self
            .video_cache
            .frame_index_for(src, time, sampling_fps, looped)?;
        // VideoFrameCache canonicalizes local paths, so the GPU cache must use
        // the same identity. Otherwise `clip.mkv` and `file:///.../clip.mkv`
        // decode to the same frame but upload duplicate GPU textures.
        let canonical = canonical_local_path(src)?;
        let key = format!(
            "video:{}:{sampling_fps:.6}:{frame_index}",
            canonical.display()
        );
        self.gpu_pixels(&key, &frame)
    }

    #[cfg(test)]
    pub(crate) fn gpu_image_cache_len(&self) -> usize {
        self.gpu_images
            .lock()
            .expect("GPU image cache lock poisoned")
            .images
            .len()
    }

    #[cfg(test)]
    pub(crate) fn text_atlas_cache_generation(&self) -> Option<u64> {
        self.text_atlas
            .lock()
            .expect("text atlas GPU lock poisoned")
            .as_ref()
            .map(|resource| resource.generation.load(Ordering::Acquire))
    }
}
