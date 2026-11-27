//! GPU rasterizer backend using `wgpu`.
#![cfg(feature = "gpu")]

pub(crate) mod compile;
pub(crate) mod compute;
pub(crate) mod context;
pub(crate) mod executor;
pub(crate) mod metrics;
pub(crate) mod pipeline;
pub(crate) mod resources;
pub(crate) mod submit;
pub(crate) mod types;

#[cfg(test)]
mod tests;

pub(crate) use compile::*;
pub(crate) use context::*;
pub use metrics::{
    CacheTelemetry, FrameProfileSample, ProfilingSummary, StageSummary, WgpuRenderStats,
    WgpuVideoTimingStats,
};
pub(crate) use pipeline::*;
pub(crate) use resources::*;
pub(crate) use submit::*;
pub(crate) use types::*;

use crate::backend::{
    BackendCapabilities, BackendRenderStats, FrameConfig, FrameSink, RasterError, RasterizerBackend,
};
use crate::image_cache::ImageCache;
use crate::scene::{Scene, SceneNode};
use crate::tiny_skia_backend::TinySkiaBackend;
use crate::video_cache::VideoFrameCache;
use image::RgbaImage;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tiny_skia::Transform;

/// GPU-accelerated rasterizer using `wgpu`.
///
/// Requires a compatible GPU with Vulkan, Metal, DX12, or WebGPU support.
/// Use `TinySkiaBackend` if GPU access is unavailable (e.g. Docker/CI).
///
/// Textures and readback buffers are cached per resolution in a `Mutex`-
/// protected `HashMap`. This eliminates the per-frame allocation pressure
/// that existed in earlier versions.
pub struct WgpuBackend {
    ctx: GpuContext,
    shader_runner: crate::shader::WgpuShaderRunner,
    /// Per-resolution GPU resource pool.  Key = `(width, height)`.
    frame_resources: GpuResourcePool,
    /// Reusable single-target slots for layer offscreen passes. These are
    /// intentionally separate from the frame readback ring so a future layer
    /// pass cannot stall the ordered output slots.
    layer_resources: GpuLayerResourcePool,
    fallback: TinySkiaBackend,
    /// Decoded image ownership for the future GPU texture cache. Keeping this
    /// separate from the CPU backend prevents a GPU render from depending on
    /// fallback implementation details while allowing both paths to share
    /// decoded pixels during the transition.
    image_cache: ImageCache,
    video_cache: VideoFrameCache,
    /// Persistent GPU texture and bind-group cache keyed by the same source
    /// identity as `ImageCache`. Texture uploads therefore happen once per
    /// source, rather than once per rendered frame.
    gpu_images: GpuImageCache,
    text_atlas: Mutex<Option<Arc<GpuTextAtlasResource>>>,
    text_atlas_upload_bytes: AtomicU64,
    text_atlas_full_uploads: AtomicU64,
    text_atlas_dirty_uploads: AtomicU64,
    text_atlas_full_upload_bytes: AtomicU64,
    text_atlas_dirty_upload_bytes: AtomicU64,
    text_atlas_cache_hits: AtomicU64,
    text_atlas_cache_misses: AtomicU64,
    gpu_texture_uploads: AtomicU64,
    gpu_texture_upload_bytes: AtomicU64,
    gpu_texture_cache_hits: AtomicU64,
    gpu_texture_cache_misses: AtomicU64,
    video_decode_ns: AtomicU64,
    texture_upload_ns: AtomicU64,
    text_atlas_upload_ns: AtomicU64,
    last_session_wall_ns: AtomicU64,
    gpu_submit_readback_ns: AtomicU64,
    gpu_submit_no_readback_ns: AtomicU64,
    gpu_frame_count: AtomicU64,
    cpu_fallback_frame_count: AtomicU64,
    cpu_island_frames: AtomicU64,
    cpu_island_pixels: AtomicU64,
    cpu_island_upload_bytes: AtomicU64,
    compute_pass_count: AtomicU64,
    last_cpu_fallback_reason: Mutex<Option<String>>,
    profiling_enabled: AtomicBool,
    profile_samples: Mutex<Vec<FrameProfileSample>>,
}

impl WgpuBackend {
    pub fn new() -> Result<Self, RasterError> {
        let ctx = GpuContext::new()?;
        let shader_runner = crate::shader::WgpuShaderRunner::from_device(&ctx.device, &ctx.queue);
        Ok(Self {
            ctx,
            shader_runner,
            frame_resources: Mutex::new(HashMap::new()),
            layer_resources: Mutex::new(HashMap::new()),
            fallback: TinySkiaBackend::new(),
            image_cache: ImageCache::default(),
            video_cache: VideoFrameCache::default(),
            gpu_images: Mutex::new(GpuImageCacheState::new(256 * 1024 * 1024)),
            text_atlas: Mutex::new(None),
            text_atlas_upload_bytes: AtomicU64::new(0),
            text_atlas_full_uploads: AtomicU64::new(0),
            text_atlas_dirty_uploads: AtomicU64::new(0),
            text_atlas_full_upload_bytes: AtomicU64::new(0),
            text_atlas_dirty_upload_bytes: AtomicU64::new(0),
            text_atlas_cache_hits: AtomicU64::new(0),
            text_atlas_cache_misses: AtomicU64::new(0),
            gpu_texture_uploads: AtomicU64::new(0),
            gpu_texture_upload_bytes: AtomicU64::new(0),
            gpu_texture_cache_hits: AtomicU64::new(0),
            gpu_texture_cache_misses: AtomicU64::new(0),
            video_decode_ns: AtomicU64::new(0),
            texture_upload_ns: AtomicU64::new(0),
            text_atlas_upload_ns: AtomicU64::new(0),
            last_session_wall_ns: AtomicU64::new(0),
            gpu_submit_readback_ns: AtomicU64::new(0),
            gpu_submit_no_readback_ns: AtomicU64::new(0),
            gpu_frame_count: AtomicU64::new(0),
            cpu_fallback_frame_count: AtomicU64::new(0),
            cpu_island_frames: AtomicU64::new(0),
            cpu_island_pixels: AtomicU64::new(0),
            cpu_island_upload_bytes: AtomicU64::new(0),
            compute_pass_count: AtomicU64::new(0),
            last_cpu_fallback_reason: Mutex::new(None),
            profiling_enabled: AtomicBool::new(false),
            profile_samples: Mutex::new(Vec::new()),
        })
    }

    /// Return the number of frames rendered by WGPU and by the CPU fallback,
    /// along with texture cache hit and miss counts.
    pub fn render_stats(&self) -> WgpuRenderStats {
        WgpuRenderStats {
            gpu_frames: self.gpu_frame_count.load(Ordering::Relaxed),
            cpu_fallback_frames: self.cpu_fallback_frame_count.load(Ordering::Relaxed),
            texture_cache_hits: self.gpu_texture_cache_hits.load(Ordering::Relaxed),
            texture_cache_misses: self.gpu_texture_cache_misses.load(Ordering::Relaxed),
        }
    }

    /// Number of frames that executed at least one bounded CPU island pass.
    pub fn cpu_island_frames(&self) -> u64 {
        self.cpu_island_frames.load(Ordering::Relaxed)
    }

    /// Cumulative pixels rasterized inside bounded CPU island passes.
    pub fn cpu_island_pixels(&self) -> u64 {
        self.cpu_island_pixels.load(Ordering::Relaxed)
    }

    /// Cumulative RGBA bytes uploaded to GPU from bounded CPU island passes.
    pub fn cpu_island_upload_bytes(&self) -> u64 {
        self.cpu_island_upload_bytes.load(Ordering::Relaxed)
    }

    /// Cumulative GPU compute shader passes dispatched.
    pub fn compute_pass_count(&self) -> u64 {
        self.compute_pass_count.load(Ordering::Relaxed)
    }

    /// Return the most recent reason a frame was rendered by the CPU fallback.
    /// This is intended for diagnostics and benchmark attribution, not for
    /// controlling rendering behavior.
    pub fn last_cpu_fallback_reason(&self) -> Option<String> {
        self.last_cpu_fallback_reason
            .lock()
            .ok()
            .and_then(|reason| reason.clone())
    }

    fn record_cpu_fallback(&self, reason: impl Into<String>) {
        self.cpu_fallback_frame_count
            .fetch_add(1, Ordering::Relaxed);
        if let Ok(mut last) = self.last_cpu_fallback_reason.lock() {
            *last = Some(reason.into());
        }
    }

    /// Number of decoded image/video textures uploaded to the GPU.
    /// Re-rendering a cached source/frame must not increment this counter.
    pub fn gpu_texture_uploads(&self) -> u64 {
        self.gpu_texture_uploads.load(Ordering::Relaxed)
    }

    /// Number of source bytes uploaded by the GPU texture cache.
    pub fn gpu_texture_upload_bytes(&self) -> u64 {
        self.gpu_texture_upload_bytes.load(Ordering::Relaxed)
    }

    /// Number of GPU texture cache lookups that reused an existing texture.
    pub fn gpu_texture_cache_hits(&self) -> u64 {
        self.gpu_texture_cache_hits.load(Ordering::Relaxed)
    }

    /// Number of GPU texture cache lookups that required a texture upload.
    pub fn gpu_texture_cache_misses(&self) -> u64 {
        self.gpu_texture_cache_misses.load(Ordering::Relaxed)
    }

    /// Return cumulative timings for the native video texture pipeline.
    pub fn video_timing_stats(&self) -> WgpuVideoTimingStats {
        WgpuVideoTimingStats {
            video_decode_ns: self.video_decode_ns.load(Ordering::Relaxed),
            texture_upload_ns: self.texture_upload_ns.load(Ordering::Relaxed),
            gpu_submit_readback_ns: self.gpu_submit_readback_ns.load(Ordering::Relaxed),
            gpu_submit_no_readback_ns: self.gpu_submit_no_readback_ns.load(Ordering::Relaxed),
        }
    }

    /// Configure the image cache used when a scene falls back to CPU.
    pub fn with_image_cache_bytes(mut self, max_bytes: usize) -> Self {
        self.fallback = self.fallback.with_image_cache_bytes(max_bytes);
        self.image_cache = ImageCache::with_max_bytes(max_bytes);
        {
            let mut gpu_images = self
                .gpu_images
                .lock()
                .expect("GPU image cache lock poisoned");
            gpu_images.max_bytes = max_bytes.max(1);
            gpu_images.trim_to_budget();
        }
        self
    }

    /// Configure the rendered Lottie frame cache budget in bytes.
    pub fn with_lottie_cache_bytes(mut self, max_bytes: usize) -> Self {
        self.fallback = self.fallback.with_lottie_cache_bytes(max_bytes);
        self
    }

    /// Number of R8 atlas bytes uploaded since backend creation.
    pub fn text_atlas_upload_bytes(&self) -> u64 {
        self.text_atlas_upload_bytes.load(Ordering::Relaxed)
    }

    /// Cumulative nanoseconds spent uploading glyphs to the GPU text atlas.
    pub fn text_atlas_upload_ns(&self) -> u64 {
        self.text_atlas_upload_ns.load(Ordering::Relaxed)
    }

    /// Total wall-clock nanoseconds of the most recent render session.
    pub fn last_session_wall_ns(&self) -> u64 {
        self.last_session_wall_ns.load(Ordering::Relaxed)
    }

    /// Number of GPU text atlas snapshots reused without an upload.
    pub fn text_atlas_cache_hits(&self) -> u64 {
        self.text_atlas_cache_hits.load(Ordering::Relaxed)
    }

    /// Number of GPU text atlas allocations after the initial snapshot.
    pub fn text_atlas_cache_misses(&self) -> u64 {
        self.text_atlas_cache_misses.load(Ordering::Relaxed)
    }

    /// Number of full R8 atlas uploads since backend creation.
    pub fn text_atlas_full_uploads(&self) -> u64 {
        self.text_atlas_full_uploads.load(Ordering::Relaxed)
    }

    /// Number of dirty-rectangle atlas uploads since backend creation.
    pub fn text_atlas_dirty_uploads(&self) -> u64 {
        self.text_atlas_dirty_uploads.load(Ordering::Relaxed)
    }

    /// Number of bytes sent by full R8 atlas uploads.
    pub fn text_atlas_full_upload_bytes(&self) -> u64 {
        self.text_atlas_full_upload_bytes.load(Ordering::Relaxed)
    }

    /// Number of bytes sent by dirty-rectangle R8 atlas uploads.
    pub fn text_atlas_dirty_upload_bytes(&self) -> u64 {
        self.text_atlas_dirty_upload_bytes.load(Ordering::Relaxed)
    }

    /// Enable or disable fine-grained stage profiling.
    pub fn with_profiling(self, enabled: bool) -> Self {
        self.profiling_enabled.store(enabled, Ordering::Relaxed);
        self
    }

    /// Check whether fine-grained stage profiling is currently enabled.
    pub fn is_profiling_enabled(&self) -> bool {
        self.profiling_enabled.load(Ordering::Relaxed)
    }

    /// Record a single frame profile sample.
    pub fn record_profile_sample(&self, sample: FrameProfileSample) {
        if let Ok(mut samples) = self.profile_samples.lock() {
            samples.push(sample);
        }
    }

    /// Retrieve all recorded frame profiling samples.
    pub fn profile_samples(&self) -> Vec<FrameProfileSample> {
        self.profile_samples
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    /// Reset all collected frame profiling samples.
    pub fn reset_profiling(&self) {
        if let Ok(mut samples) = self.profile_samples.lock() {
            samples.clear();
        }
        self.last_session_wall_ns.store(0, Ordering::Relaxed);
    }

    /// Compute an aggregated stage summary of all collected frame profiles.
    pub fn profile_summary(&self) -> ProfilingSummary {
        let samples = self.profile_samples();
        let session_wall_ns = self.last_session_wall_ns.load(Ordering::Relaxed);
        let img_hits = self.gpu_texture_cache_hits.load(Ordering::Relaxed);
        let img_misses = self.gpu_texture_cache_misses.load(Ordering::Relaxed);
        let atlas_hits = self.text_atlas_cache_hits.load(Ordering::Relaxed);
        let atlas_misses = self.text_atlas_cache_misses.load(Ordering::Relaxed);
        ProfilingSummary::from_samples(
            &samples,
            session_wall_ns,
            CacheTelemetry::new(img_hits, img_misses),
            CacheTelemetry::new(atlas_hits, atlas_misses),
        )
    }
}

impl RasterizerBackend for WgpuBackend {
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            native_scene: true,
            browser_runtime: false,
            gpu_accelerated: true,
            supports_streaming: self.supports_streaming(),
        }
    }

    fn render_stats(&self) -> BackendRenderStats {
        let stats = self.render_stats();
        let timing = self.video_timing_stats();
        BackendRenderStats {
            gpu_frames: stats.gpu_frames,
            cpu_fallback_frames: stats.cpu_fallback_frames,
            texture_cache_hits: stats.texture_cache_hits,
            texture_cache_misses: stats.texture_cache_misses,
            video_decode_ns: timing.video_decode_ns,
            texture_upload_ns: timing.texture_upload_ns,
            gpu_submit_readback_ns: timing.gpu_submit_readback_ns,
            browser_frame_ns: 0,
        }
    }

    fn render_frame(&self, scene: &Scene, config: &FrameConfig) -> Result<RgbaImage, RasterError> {
        let plan = self.compile_render_plan(scene, config)?;
        let (image, _report) = crate::render_ir::RenderExecutor::execute_plan(self, &plan, config)?;
        Ok(image)
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    #[allow(clippy::type_complexity)]
    fn render_stream(
        &self,
        total: u32,
        scene_fn: &(dyn Fn(u32) -> Result<Scene, RasterError> + Sync),
        config_fn: &(dyn Fn(u32) -> FrameConfig + Sync),
        sink: &mut dyn FrameSink,
    ) -> Result<(), RasterError> {
        if total == 0 {
            return Ok(());
        }

        let is_profiling = self.is_profiling_enabled();
        let session_start = Instant::now();

        let first_cfg = config_fn(0);
        let width = first_cfg.width;
        let height = first_cfg.height;

        if width > self.ctx.max_texture_dimension_2d || height > self.ctx.max_texture_dimension_2d {
            let res = self
                .fallback
                .render_stream(total, scene_fn, config_fn, sink);
            if is_profiling {
                let session_wall_ns = session_start.elapsed().as_nanos() as u64;
                self.last_session_wall_ns
                    .store(session_wall_ns, Ordering::Relaxed);
            }
            return res;
        }

        let resource = {
            let mut pool = self
                .frame_resources
                .lock()
                .map_err(|_| RasterError::Init("GPU resource pool mutex poisoned".into()))?;
            pool.entry((width, height))
                .or_insert_with(|| {
                    std::sync::Arc::new(Mutex::new(GpuFrameResources::new(
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

        // Keep the whole readback ring occupied before waiting. The previous
        // implementation retained only one in-flight frame, which serialized
        // GPU submission with map/readback and left two persistent slots idle.
        let mut in_flight = std::collections::VecDeque::with_capacity(RING_BUFFER_SIZE);
        let mut scratch = Vec::new();

        for frame in 0..total {
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

            let scene_eval_start = if is_profiling {
                Some(Instant::now())
            } else {
                None
            };
            let scene = scene_fn(frame)?;
            let scene_eval_ns = scene_eval_start
                .map(|t| t.elapsed().as_nanos() as u64)
                .unwrap_or(0);

            let cfg = config_fn(frame);

            let encode_start = if is_profiling {
                Some(Instant::now())
            } else {
                None
            };
            let Some((commands, path_mask, asset_decode_ns)) =
                compile_scene_with_path_mask(&scene, &self.fallback)
            else {
                while let Some(prev) = in_flight.pop_front() {
                    self.drain_slot(prev, &res, width, height, &mut scratch, sink)?;
                }
                if let Ok(plan) = self.compile_render_plan(&scene, &cfg) {
                    let is_single_cpu_island = plan.pass_order.len() == 1
                        && plan
                            .pass_order
                            .first()
                            .and_then(|pid| plan.graph.pass(*pid))
                            .is_some_and(|p| {
                                matches!(p.op, crate::render_ir::PassOp::CpuIsland { .. })
                            });
                    if !is_single_cpu_island {
                        drop(res);
                        let (img, report) =
                            crate::render_ir::RenderExecutor::execute_plan(self, &plan, &cfg)?;
                        let enc_start = if is_profiling {
                            Some(Instant::now())
                        } else {
                            None
                        };
                        sink.consume(frame, img.as_raw())?;
                        let enc_ns = enc_start
                            .map(|t| t.elapsed().as_nanos() as u64)
                            .unwrap_or(0);
                        if is_profiling {
                            self.record_profile_sample(FrameProfileSample {
                                frame_idx: frame,
                                scene_eval_ns,
                                decode_ns: 0,
                                upload_ns: 0,
                                upload_bytes: report.cpu_island_upload_bytes,
                                compile_encode_ns: 0,
                                submission_wait_ns: 0,
                                gpu_exec_ns: None,
                                readback_ns: 0,
                                readback_bytes: u64::from(width) * u64::from(height) * 4,
                                video_encode_ns: enc_ns,
                                total_frame_ns: frame_started.elapsed().as_nanos() as u64,
                                cpu_fallback: report.cpu_island_passes > 0
                                    && report.gpu_passes == 0,
                            });
                        }
                        res = resource.lock().map_err(|_| {
                            RasterError::Init("GPU frame resource mutex poisoned".into())
                        })?;
                        continue;
                    }
                }
                self.record_cpu_fallback(gpu_fallback_reason(&scene));
                let fb_start = if is_profiling {
                    Some(Instant::now())
                } else {
                    None
                };
                let img = self.fallback.render_frame(&scene, &cfg)?;
                let fb_render_ns = fb_start.map(|t| t.elapsed().as_nanos() as u64).unwrap_or(0);
                let enc_start = if is_profiling {
                    Some(Instant::now())
                } else {
                    None
                };
                sink.consume(frame, img.as_raw())?;
                let enc_ns = enc_start
                    .map(|t| t.elapsed().as_nanos() as u64)
                    .unwrap_or(0);
                if is_profiling {
                    self.record_profile_sample(FrameProfileSample {
                        frame_idx: frame,
                        scene_eval_ns,
                        decode_ns: 0,
                        upload_ns: 0,
                        upload_bytes: 0,
                        compile_encode_ns: fb_render_ns,
                        submission_wait_ns: 0,
                        gpu_exec_ns: None,
                        readback_ns: 0,
                        readback_bytes: 0,
                        video_encode_ns: enc_ns,
                        total_frame_ns: frame_started.elapsed().as_nanos() as u64,
                        cpu_fallback: true,
                    });
                }
                continue;
            };

            let slot_idx = (frame as usize) % RING_BUFFER_SIZE;

            // Do not reuse a slot until its oldest submission has been
            // consumed. FIFO draining preserves the stream's frame order.
            if in_flight.len() == RING_BUFFER_SIZE {
                let prev = in_flight
                    .pop_front()
                    .expect("in-flight ring length was checked");
                debug_assert_eq!(prev.slot_idx, slot_idx);
                self.drain_slot(prev, &res, width, height, &mut scratch, sink)?;
            }

            // Submit this frame to GPU
            let submitted = self.submit_frame_to_slot(FrameSubmission {
                commands: &commands,
                path_mask: path_mask.as_ref(),
                width,
                height,
                sampling_fps: cfg.fps,
                slot: &res.slots[slot_idx],
                shader_suffix: None,
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

            self.gpu_frame_count.fetch_add(1, Ordering::Relaxed);

            in_flight.push_back(InFlight {
                frame_idx: frame,
                slot_idx,
                submission_index,
                rx: rx.expect("readback was requested for render_stream"),
                query_rx,
                frame_started,
                scene_eval_ns,
                decode_ns,
                upload_ns,
                upload_bytes,
                compile_encode_ns,
            });
        }

        // Drain any remaining in-flight frame at the end of the stream
        while let Some(prev) = in_flight.pop_front() {
            self.drain_slot(prev, &res, width, height, &mut scratch, sink)?;
        }

        if is_profiling {
            let session_wall_ns = session_start.elapsed().as_nanos() as u64;
            self.last_session_wall_ns
                .store(session_wall_ns, Ordering::Relaxed);
        }

        Ok(())
    }
}
