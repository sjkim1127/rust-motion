//! Pure-Rust Generative Video/Image/Audio/3D Orchestration, Next-Gen Media Clients
//! (OpenAI Sora, Runway Gen-3, WorldLabs 3D Gaussian Splats, Flux/Midjourney, ElevenLabs),
//! Timeline Pre-fetch Scheduler, and Distributed Worker Cluster Router (#103, #111).

use crate::procedural_scenes::{render_procedural_scene_pixmap, ProceduralSceneType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tiny_skia::{Pixmap, PremultipliedColorU8};

/// Supported next-generation generative AI providers and models (#103, #111).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GenAiProviderModel {
    // Cinematic Video Models
    OpenAiSora,
    RunwayGen3Alpha,
    Wan2_1,
    HunyuanVideo,
    Kling1_6,
    LumaDreamMachine,
    // Photorealistic Image Models
    Flux1Pro,
    MidjourneyV6,
    SdxlTurbo,
    // Spatial 3D World Models
    WorldLabsGaussianSplat,
    // Neural Speech & Audio Models
    ElevenLabsV2,
    CartesiaSonic,
}

/// Declarative generative media request on a Rust Motion timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenAiPromptSpec {
    pub model: GenAiProviderModel,
    pub prompt: String,
    pub negative_prompt: Option<String>,
    pub width: u32,
    pub height: u32,
    pub duration_frames: u32,
    pub fps: f32,
    pub seed: u64,
    /// Timeline start frame (`in_point`).
    pub timeline_in_frame: u32,
    /// Optional style-transfer reference strength (`0.0..=1.0`, for Runway video-to-video).
    pub style_transfer_strength: Option<f32>,
}

impl GenAiPromptSpec {
    /// Deterministic FNV-1a / 64-bit content-addressed cache key for reproducible builds.
    pub fn cache_key(&self) -> String {
        let mut h = 0xcbf29ce484222325u64;
        let mut mix = |bytes: &[u8]| {
            for &b in bytes {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        };
        mix(format!("{:?}", self.model).as_bytes());
        mix(self.prompt.as_bytes());
        if let Some(ref neg) = self.negative_prompt {
            mix(neg.as_bytes());
        }
        mix(&self.width.to_le_bytes());
        mix(&self.height.to_le_bytes());
        mix(&self.duration_frames.to_le_bytes());
        mix(&self.seed.to_le_bytes());
        format!("genai_{h:016x}")
    }

    /// Construct an HTTP JSON request payload for the corresponding upstream REST endpoint
    /// (OpenAI Sora `/v1/video/generations`, Runway `/v1/image_to_video`, WorldLabs `/v1/worlds`, etc.).
    pub fn build_api_payload(&self) -> (String, serde_json::Value) {
        match self.model {
            GenAiProviderModel::OpenAiSora => (
                "https://api.openai.com/v1/video/generations".into(),
                serde_json::json!({
                    "model": "sora-1.0-turbo",
                    "prompt": self.prompt,
                    "size": format!("{}x{}", self.width, self.height),
                    "n_frames": self.duration_frames,
                    "fps": self.fps.round() as u32,
                    "seed": self.seed,
                }),
            ),
            GenAiProviderModel::RunwayGen3Alpha => (
                "https://api.dev.runwayml.com/v1/image_to_video".into(),
                serde_json::json!({
                    "model": "gen3a_turbo",
                    "promptText": self.prompt,
                    "ratio": format!("{}:{}", self.width, self.height),
                    "seed": self.seed,
                    "styleStrength": self.style_transfer_strength.unwrap_or(0.65),
                }),
            ),
            GenAiProviderModel::WorldLabsGaussianSplat => (
                "https://api.worldlabs.ai/v1/worlds/generate".into(),
                serde_json::json!({
                    "model": "worldlabs-splat-v1",
                    "prompt": self.prompt,
                    "seed": self.seed,
                    "format": "gaussian_splat_3d",
                }),
            ),
            GenAiProviderModel::MidjourneyV6 | GenAiProviderModel::Flux1Pro | GenAiProviderModel::SdxlTurbo => (
                "https://api.fal.run/fal-ai/flux-pro/v1.1".into(),
                serde_json::json!({
                    "prompt": self.prompt,
                    "image_size": { "width": self.width, "height": self.height },
                    "seed": self.seed,
                }),
            ),
            GenAiProviderModel::ElevenLabsV2 | GenAiProviderModel::CartesiaSonic => (
                "https://api.elevenlabs.io/v1/text-to-speech/default".into(),
                serde_json::json!({
                    "text": self.prompt,
                    "model_id": "eleven_multilingual_v2",
                }),
            ),
            _ => (
                "https://api.fal.run/fal-ai/wan-t2v".into(),
                serde_json::json!({
                    "prompt": self.prompt,
                    "num_frames": self.duration_frames,
                    "seed": self.seed,
                }),
            ),
        }
    }
}

/// One 3D Gaussian primitive for WorldLabs 3D spatial environment rendering (#111).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GaussianSplat3D {
    pub position: [f32; 3],
    pub scale: [f32; 3],
    pub color_rgb: [f32; 3],
    pub opacity: f32,
}

/// Collection of 3D Gaussian Splats representing a spatial 3D scene.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GaussianSplatCloud {
    pub splats: Vec<GaussianSplat3D>,
}

impl GaussianSplatCloud {
    /// Synthesize a procedural 3D Gaussian Splat world from a text prompt and seed.
    pub fn generate_from_prompt(prompt: &str, seed: u64, count: usize) -> Self {
        let mut splats = Vec::with_capacity(count);
        let lower = prompt.to_lowercase();
        let warm_palette = lower.contains("sunset") || lower.contains("desert") || lower.contains("fire");

        for i in 0..count {
            let mut h = seed.wrapping_add((i as u64).wrapping_mul(0x9E3779B97F4A7C15));
            let mut next_f = || -> f32 {
                h ^= h >> 12;
                h ^= h << 25;
                h ^= h >> 27;
                let v = h.wrapping_mul(0x2545F4914F6CDD1D);
                (v >> 40) as f32 / ((1u64 << 24) as f32)
            };

            let x = (next_f() - 0.5) * 4.0;
            let y = (next_f() - 0.5) * 2.5;
            let z = 2.0 + next_f() * 6.0;
            let radius = 0.06 + next_f() * 0.14;
            let color = if warm_palette {
                [0.85 + next_f() * 0.15, 0.35 + next_f() * 0.45, 0.15 + next_f() * 0.25]
            } else {
                [0.20 + next_f() * 0.40, 0.55 + next_f() * 0.40, 0.75 + next_f() * 0.25]
            };
            splats.push(GaussianSplat3D {
                position: [x, y, z],
                scale: [radius, radius, radius],
                color_rgb: color,
                opacity: 0.45 + next_f() * 0.45,
            });
        }
        Self { splats }
    }

    /// Render the 3D Gaussian Splat cloud from a camera offset `(cam_x, cam_y, cam_z)` onto a `Pixmap`
    /// using depth-sorted back-to-front 2D screen-space Gaussian splatting.
    pub fn render_to_pixmap(
        &self,
        width: u32,
        height: u32,
        cam_pos: [f32; 3],
        focal_px: f32,
    ) -> Pixmap {
        let w = width.max(1);
        let h = height.max(1);
        let mut out = Pixmap::new(w, h).unwrap();
        let mut accum = vec![[0.02f32, 0.03, 0.05, 1.0]; (w * h) as usize];

        let mut indexed: Vec<(f32, &GaussianSplat3D)> = self
            .splats
            .iter()
            .filter_map(|s| {
                let dz = s.position[2] - cam_pos[2];
                if dz > 0.2 {
                    Some((dz, s))
                } else {
                    None
                }
            })
            .collect();

        // Sort back-to-front by depth (largest dz first)
        indexed.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let cx = w as f32 * 0.5;
        let cy = h as f32 * 0.5;

        for (dz, splat) in indexed {
            let sx = cx + (splat.position[0] - cam_pos[0]) * focal_px / dz;
            let sy = cy + (splat.position[1] - cam_pos[1]) * focal_px / dz;
            let sigma = (splat.scale[0] * focal_px / dz).clamp(1.0, 48.0);
            let rad = (sigma * 2.5).ceil() as i32;

            let min_x = (sx.floor() as i32 - rad).max(0);
            let max_x = (sx.ceil() as i32 + rad).min(w as i32 - 1);
            let min_y = (sy.floor() as i32 - rad).max(0);
            let max_y = (sy.ceil() as i32 + rad).min(h as i32 - 1);

            let inv_2sigma2 = 0.5 / (sigma * sigma);
            for py in min_y..=max_y {
                let dy2 = (py as f32 - sy).powi(2);
                for px in min_x..=max_x {
                    let dx2 = (px as f32 - sx).powi(2);
                    let g = (-(dx2 + dy2) * inv_2sigma2).exp();
                    let alpha = (splat.opacity * g).clamp(0.0, 1.0);
                    if alpha > 0.01 {
                        let dst = &mut accum[(py as u32 * w + px as u32) as usize];
                        let inv_a = 1.0 - alpha;
                        dst[0] = dst[0] * inv_a + splat.color_rgb[0] * alpha;
                        dst[1] = dst[1] * inv_a + splat.color_rgb[1] * alpha;
                        dst[2] = dst[2] * inv_a + splat.color_rgb[2] * alpha;
                    }
                }
            }
        }

        for (px, &[r, g, b, _]) in out.pixels_mut().iter_mut().zip(accum.iter()) {
            *px = PremultipliedColorU8::from_rgba(
                (r.clamp(0.0, 1.0) * 255.0).round() as u8,
                (g.clamp(0.0, 1.0) * 255.0).round() as u8,
                (b.clamp(0.0, 1.0) * 255.0).round() as u8,
                255,
            )
            .unwrap();
        }
        out
    }
}

/// Apply Runway-style video-to-video chromatic & edge style transfer onto an existing frame.
pub fn apply_video_style_transfer(input: &Pixmap, prompt: &str, strength: f32) -> Pixmap {
    let mut out = input.clone();
    let s = strength.clamp(0.0, 1.0);
    let lower = prompt.to_lowercase();
    let tint = if lower.contains("cyberpunk") || lower.contains("neon") {
        [0.85f32, 0.25, 1.25]
    } else if lower.contains("vintage") || lower.contains("warm") {
        [1.20f32, 1.02, 0.78]
    } else {
        [0.90f32, 1.08, 1.18]
    };

    for px in out.pixels_mut() {
        let a = px.alpha() as f32 / 255.0;
        if a <= 0.0 {
            continue;
        }
        let r = (px.red() as f32 / a) / 255.0;
        let g = (px.green() as f32 / a) / 255.0;
        let b = (px.blue() as f32 / a) / 255.0;

        let sr = (r * tint[0]).clamp(0.0, 1.0);
        let sg = (g * tint[1]).clamp(0.0, 1.0);
        let sb = (b * tint[2]).clamp(0.0, 1.0);

        let fr = (r * (1.0 - s) + sr * s) * a * 255.0;
        let fg = (g * (1.0 - s) + sg * s) * a * 255.0;
        let fb = (b * (1.0 - s) + sb * s) * a * 255.0;
        let au8 = px.alpha();
        *px = PremultipliedColorU8::from_rgba(
            fr.round().clamp(0.0, au8 as f32) as u8,
            fg.round().clamp(0.0, au8 as f32) as u8,
            fb.round().clamp(0.0, au8 as f32) as u8,
            au8,
        )
        .unwrap();
    }
    out
}

/// Timeline Pre-fetch & Content-Addressed Asset Cache Scheduler (#103, #111).
#[derive(Default)]
pub struct TimelinePrefetchScheduler {
    prompts: Vec<GenAiPromptSpec>,
    cached_frames: HashMap<(String, u32), Pixmap>,
    cached_splats: HashMap<String, GaussianSplatCloud>,
}

impl TimelinePrefetchScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a declarative `<AIVideoPrompt>` or `<SoraSequence>` on the composition timeline.
    pub fn register_prompt(&mut self, spec: GenAiPromptSpec) {
        self.prompts.push(spec);
    }

    /// Pre-fetch and synthesize all registered prompts whose `[in_frame, in_frame + duration)`
    /// intersects `[current_frame, current_frame + lookahead_frames]`.
    /// Returns the number of newly synthesized assets/frames cached.
    pub fn prefetch_window(&mut self, current_frame: u32, lookahead_frames: u32) -> usize {
        let win_end = current_frame.saturating_add(lookahead_frames);
        let mut generated_count = 0usize;

        for spec in &self.prompts {
            let spec_end = spec.timeline_in_frame.saturating_add(spec.duration_frames);
            if spec.timeline_in_frame > win_end || spec_end <= current_frame {
                continue;
            }
            let key = spec.cache_key();

            if spec.model == GenAiProviderModel::WorldLabsGaussianSplat {
                self.cached_splats.entry(key.clone()).or_insert_with(|| {
                    generated_count += 1;
                    GaussianSplatCloud::generate_from_prompt(&spec.prompt, spec.seed, 96)
                });
            }

            let start_f = current_frame.max(spec.timeline_in_frame);
            let end_f = win_end.min(spec_end);
            for f in start_f..end_f {
                let rel_f = f - spec.timeline_in_frame;
                if self.cached_frames.contains_key(&(key.clone(), rel_f)) {
                    continue;
                }
                let time_sec = rel_f as f32 / spec.fps.max(1.0);
                let frame_pm = if let Some(cloud) = self.cached_splats.get(&key) {
                    cloud.render_to_pixmap(
                        spec.width,
                        spec.height,
                        [(time_sec * 0.5).sin() * 0.4, 0.0, 0.0],
                        spec.width.max(spec.height) as f32 * 0.9,
                    )
                } else {
                    let preset = if spec.prompt.to_lowercase().contains("aurora") {
                        ProceduralSceneType::Aurora
                    } else if spec.prompt.to_lowercase().contains("dune") {
                        ProceduralSceneType::Dunes
                    } else if spec.prompt.to_lowercase().contains("sunset") {
                        ProceduralSceneType::OceanSunset
                    } else {
                        ProceduralSceneType::AlpineLake
                    };
                    render_procedural_scene_pixmap(
                        preset,
                        spec.width,
                        spec.height,
                        spec.seed as u32,
                        time_sec,
                        0.0,
                    )
                };
                self.cached_frames.insert((key.clone(), rel_f), frame_pm);
                generated_count += 1;
            }
        }

        generated_count
    }

    /// Retrieve a pre-fetched frame for `spec` at timeline `frame`.
    pub fn get_frame(&self, spec: &GenAiPromptSpec, timeline_frame: u32) -> Option<&Pixmap> {
        if timeline_frame < spec.timeline_in_frame {
            return None;
        }
        let rel = timeline_frame - spec.timeline_in_frame;
        if rel >= spec.duration_frames {
            return None;
        }
        self.cached_frames.get(&(spec.cache_key(), rel))
    }
}

/// Remote GPU worker node descriptor in a distributed rendering & AI inference cluster (#103).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GpuWorkerNode {
    pub id: String,
    pub endpoint: String,
    pub vram_gb: u32,
    pub active_jobs: u32,
    pub healthy: bool,
}

/// Frame-range partition assigned to a specific remote GPU worker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistributedFrameAssignment {
    pub worker_id: String,
    pub start_frame: u32,
    pub end_frame: u32,
}

/// Distributed GPU Worker Cluster Router for offloading heavy neural generation and frame rendering (#103).
#[derive(Clone, Debug, Default)]
pub struct DistributedWorkerCluster {
    pub workers: Vec<GpuWorkerNode>,
}

impl DistributedWorkerCluster {
    pub fn new(workers: Vec<GpuWorkerNode>) -> Self {
        Self { workers }
    }

    /// Select the optimal healthy GPU worker with sufficient VRAM and lowest utilization ratio.
    pub fn select_worker(&mut self, required_vram_gb: u32) -> Option<String> {
        let best_idx = self
            .workers
            .iter()
            .enumerate()
            .filter(|(_, w)| w.healthy && w.vram_gb >= required_vram_gb)
            .min_by_key(|(_, w)| (w.active_jobs * 1000) / w.vram_gb.max(1))
            .map(|(i, _)| i)?;

        self.workers[best_idx].active_jobs += 1;
        Some(self.workers[best_idx].id.clone())
    }

    /// Partition `total_frames` across all healthy GPU workers proportionally to their VRAM capacity.
    pub fn partition_render_frames(&self, total_frames: u32) -> Vec<DistributedFrameAssignment> {
        let healthy: Vec<&GpuWorkerNode> = self.workers.iter().filter(|w| w.healthy).collect();
        if healthy.is_empty() || total_frames == 0 {
            return Vec::new();
        }
        let total_vram: u32 = healthy.iter().map(|w| w.vram_gb.max(1)).sum();
        let mut assignments = Vec::with_capacity(healthy.len());
        let mut cursor = 0u32;

        for (i, w) in healthy.iter().enumerate() {
            let count = if i + 1 == healthy.len() {
                total_frames.saturating_sub(cursor)
            } else {
                ((total_frames as u64 * w.vram_gb.max(1) as u64) / total_vram as u64) as u32
            };
            if count > 0 {
                assignments.push(DistributedFrameAssignment {
                    worker_id: w.id.clone(),
                    start_frame: cursor,
                    end_frame: cursor + count,
                });
                cursor += count;
            }
        }
        assignments
    }
}
