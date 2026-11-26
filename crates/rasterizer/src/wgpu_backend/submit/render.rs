//! Public GPU render entry points: single-frame and streamed rendering.

use super::*;

impl WgpuBackend {
    /// Render a frame directly to a GPU texture view, passing it to `consume`.
    /// The rendered texture view is valid for the duration of
    /// the callback only; callers must not retain it after returning.
    pub fn render_frame_gpu<F>(
        &self,
        scene: &Scene,
        config: &FrameConfig,
        consume: F,
    ) -> Result<(), RasterError>
    where
        F: FnOnce(&wgpu::TextureView, u32, u32),
    {
        if config.width > self.ctx.max_texture_dimension_2d
            || config.height > self.ctx.max_texture_dimension_2d
        {
            return Err(RasterError::Scene(
                "GPU-native frame dimensions exceed the device limit".into(),
            ));
        }
        if scene
            .nodes
            .iter()
            .any(|node| matches!(node, SceneNode::Shader { .. }))
        {
            return Err(RasterError::Scene(
                "GPU-native frame sink does not yet support shader nodes".into(),
            ));
        }
        let Some((commands, path_mask, _)) = compile_scene_with_path_mask(scene, &self.fallback)
        else {
            return Err(RasterError::Scene(
                "scene requires the CPU fallback and has no GPU texture handle".into(),
            ));
        };

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
        let slot_idx = resources.active_index % RING_BUFFER_SIZE;
        resources.active_index = resources.active_index.wrapping_add(1);
        let submitted = self.submit_frame_to_slot(FrameSubmission {
            commands: &commands,
            path_mask: path_mask.as_ref(),
            width: config.width,
            height: config.height,
            sampling_fps: config.fps,
            slot: &resources.slots[slot_idx],
            shader_suffix: None,
            readback: false,
        })?;
        let submission_index = submitted.submission_index;
        let rx = submitted.readback_rx;
        debug_assert!(rx.is_none());
        let gpu_submit_start = Instant::now();
        self.ctx
            .device
            .poll(wgpu::Maintain::wait_for(submission_index));
        self.gpu_submit_no_readback_ns.fetch_add(
            gpu_submit_start.elapsed().as_nanos() as u64,
            Ordering::Relaxed,
        );
        consume(
            &resources.slots[slot_idx].texture_view,
            config.width,
            config.height,
        );
        self.gpu_frame_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Stream GPU-compatible frames directly to texture-view consumers.
    ///
    /// Frames are delivered in order and never copied to the CPU. The view is
    /// valid only for the duration of `consume`; a consumer that needs to
    /// retain GPU work must enqueue it before returning from the callback.
    pub fn render_stream_gpu<S, C, F>(
        &self,
        total: u32,
        scene_fn: &S,
        config_fn: &C,
        mut consume: F,
    ) -> Result<(), RasterError>
    where
        S: Fn(u32) -> Result<Scene, RasterError> + Sync,
        C: Fn(u32) -> FrameConfig + Sync,
        F: FnMut(u32, &wgpu::TextureView, u32, u32),
    {
        if total == 0 {
            return Ok(());
        }
        let first = config_fn(0);
        if first.width > self.ctx.max_texture_dimension_2d
            || first.height > self.ctx.max_texture_dimension_2d
        {
            return Err(RasterError::Scene(
                "GPU-native stream dimensions exceed the device limit".into(),
            ));
        }
        let resource = {
            let mut pool = self
                .frame_resources
                .lock()
                .map_err(|_| RasterError::Init("GPU resource pool mutex poisoned".into()))?;
            pool.entry((first.width, first.height))
                .or_insert_with(|| {
                    Arc::new(Mutex::new(GpuFrameResources::new(
                        &self.ctx.device,
                        first.width,
                        first.height,
                        self.ctx.supports_timestamp_queries,
                    )))
                })
                .clone()
        };
        let mut resources = resource
            .lock()
            .map_err(|_| RasterError::Init("GPU frame resource mutex poisoned".into()))?;
        let mut in_flight: std::collections::VecDeque<(u32, usize, wgpu::SubmissionIndex)> =
            std::collections::VecDeque::with_capacity(RING_BUFFER_SIZE);

        for frame in 0..total {
            let scene = scene_fn(frame)?;
            let config = config_fn(frame);
            if (config.width, config.height) != (first.width, first.height) {
                return Err(RasterError::Scene(
                    "GPU-native stream cannot change dimensions mid-stream".into(),
                ));
            }
            let Some((commands, path_mask, _)) =
                compile_scene_with_path_mask(&scene, &self.fallback)
            else {
                return Err(RasterError::Scene(format!(
                    "frame {frame} requires the CPU fallback"
                )));
            };
            if scene
                .nodes
                .iter()
                .any(|node| matches!(node, SceneNode::Shader { .. }))
            {
                return Err(RasterError::Scene(format!(
                    "frame {frame} contains unsupported shader nodes"
                )));
            }
            if in_flight.len() == RING_BUFFER_SIZE {
                let (queued_frame, queued_slot, submission_index) = in_flight
                    .pop_front()
                    .expect("GPU-native in-flight ring length was checked");
                let gpu_submit_start = Instant::now();
                self.ctx
                    .device
                    .poll(wgpu::Maintain::wait_for(submission_index));
                self.gpu_submit_no_readback_ns.fetch_add(
                    gpu_submit_start.elapsed().as_nanos() as u64,
                    Ordering::Relaxed,
                );
                consume(
                    queued_frame,
                    &resources.slots[queued_slot].texture_view,
                    first.width,
                    first.height,
                );
            }
            let slot_idx = resources.active_index % RING_BUFFER_SIZE;
            resources.active_index = resources.active_index.wrapping_add(1);
            let submitted = self.submit_frame_to_slot(FrameSubmission {
                commands: &commands,
                path_mask: path_mask.as_ref(),
                width: first.width,
                height: first.height,
                sampling_fps: config.fps,
                slot: &resources.slots[slot_idx],
                shader_suffix: None,
                readback: false,
            })?;
            let submission_index = submitted.submission_index;
            let rx = submitted.readback_rx;
            debug_assert!(rx.is_none());
            in_flight.push_back((frame, slot_idx, submission_index));
            self.gpu_frame_count.fetch_add(1, Ordering::Relaxed);
        }
        while let Some((queued_frame, queued_slot, submission_index)) = in_flight.pop_front() {
            let gpu_submit_start = Instant::now();
            self.ctx
                .device
                .poll(wgpu::Maintain::wait_for(submission_index));
            self.gpu_submit_no_readback_ns.fetch_add(
                gpu_submit_start.elapsed().as_nanos() as u64,
                Ordering::Relaxed,
            );
            consume(
                queued_frame,
                &resources.slots[queued_slot].texture_view,
                first.width,
                first.height,
            );
        }
        Ok(())
    }
}
