//! CPU rasterizer backend using `tiny-skia`.
//!
//! Renders a [`Scene`] into an RGBA pixel buffer without any GPU or browser dependency.

pub mod color_blend;
pub mod filters;
pub mod media;
pub mod render_node;
pub mod shapes;
#[cfg(test)]
mod tests;
pub mod visualizer;

pub use filters::apply_photo_filter;
pub(crate) use render_node::render_nodes;
#[cfg(feature = "gpu")]
pub(crate) use shapes::svgpath_to_tiny_skia;
#[cfg(feature = "gpu")]
pub(crate) use visualizer::render_audio_visualizer;

use crate::backend::{FrameConfig, FrameSink, RasterError, RasterizerBackend};
use crate::font::FontCache;
use crate::gif_cache::GifFrameCache;
use crate::image_cache::ImageCache;
#[cfg(feature = "gpu")]
use crate::scene::Color;
use crate::scene::Scene;
use crate::video_cache::VideoFrameCache;
use image::RgbaImage;
#[cfg(feature = "gpu")]
use std::sync::Arc;
use tiny_skia::{Pixmap, Transform};

/// The `tiny-skia` CPU rasterizer.
///
/// Zero dependencies on GPU drivers, Chrome, or any external process.
/// Works in CI, Docker, and serverless environments out of the box.
/// Text is rendered using real TTF glyph data via `ab_glyph`.
pub struct TinySkiaBackend {
    font: FontCache,
    images: ImageCache,
    videos: VideoFrameCache,
    gifs: GifFrameCache,
    lotties: crate::lottie_cache::LottieCache,
    audios: crate::audio_cache::AudioCache,
    security: crate::security::MediaSecurityPolicy,
}

impl TinySkiaBackend {
    /// Create a new backend, loading a system font automatically.
    pub fn new() -> Self {
        Self::new_with_policy(crate::security::MediaSecurityPolicy::default())
    }

    /// Create a new backend with a media security sandbox policy.
    pub fn new_with_policy(security: crate::security::MediaSecurityPolicy) -> Self {
        Self {
            font: FontCache::load(),
            images: ImageCache::default(),
            videos: VideoFrameCache::default(),
            gifs: GifFrameCache::new(),
            lotties: crate::lottie_cache::LottieCache::default(),
            audios: crate::audio_cache::AudioCache::default(),
            security,
        }
    }

    /// Configure the decoded image cache budget in bytes.
    ///
    /// This is useful for embedded hosts such as Tauri, where the renderer
    /// should share an explicit memory budget with the application. The
    /// default constructors retain the 256 MiB cache budget.
    pub fn with_image_cache_bytes(mut self, max_bytes: usize) -> Self {
        self.images = ImageCache::with_max_bytes(max_bytes);
        self
    }

    /// Configure the rendered Lottie frame cache budget in bytes.
    ///
    /// The default cache budget is 512 MiB. Frames larger than the configured
    /// budget are rendered but are not retained in the cache.
    pub fn with_lottie_cache_bytes(mut self, max_bytes: usize) -> Self {
        self.lotties = crate::lottie_cache::LottieCache::with_rendered_frame_cache_bytes(max_bytes);
        self
    }

    /// Create without loading a font (text will use placeholder blocks).
    pub fn headless() -> Self {
        Self::headless_with_policy(crate::security::MediaSecurityPolicy::default())
    }

    /// Create without loading a font, with a media security sandbox policy.
    pub fn headless_with_policy(security: crate::security::MediaSecurityPolicy) -> Self {
        Self {
            font: FontCache::headless(),
            images: ImageCache::default(),
            videos: VideoFrameCache::default(),
            gifs: GifFrameCache::new(),
            lotties: crate::lottie_cache::LottieCache::default(),
            audios: crate::audio_cache::AudioCache::default(),
            security,
        }
    }

    /// Configure the media security sandbox policy on the backend.
    pub fn with_security_policy(mut self, security: crate::security::MediaSecurityPolicy) -> Self {
        self.security = security;
        self
    }

    /// Stop all idle persistent FFmpeg decoder processes immediately.
    ///
    /// Decoders are also stopped automatically when the backend is dropped.
    pub fn shutdown_media(&self) {
        self.videos.shutdown();
    }

    #[cfg(feature = "gpu")]
    pub(crate) fn rasterize_text(
        &self,
        content: &str,
        font_size: f32,
        font_weight: u16,
        font_sources: &[String],
    ) -> Option<crate::font::RenderedText> {
        self.font
            .rasterize_with_weight(content, font_size, font_weight, font_sources)
            .ok()
            .flatten()
    }

    #[cfg(feature = "gpu")]
    pub(crate) fn text_atlas_entry(&self, key: &str) -> Option<crate::text_atlas::AtlasEntry> {
        self.font.text_atlas_entry(key)
    }

    #[cfg(feature = "gpu")]
    pub(crate) fn take_text_atlas_snapshot(&self) -> crate::text_atlas::TextAtlasSnapshot {
        self.font.take_text_atlas_snapshot()
    }

    #[cfg(feature = "gpu")]
    pub(crate) fn lottie_frame(
        &self,
        src: &str,
        time_secs: f64,
        target_w: u32,
        target_h: u32,
        loop_behavior: crate::gif_cache::LoopBehavior,
    ) -> Result<Arc<image::RgbaImage>, RasterError> {
        self.lotties.render_with_policy(
            src,
            time_secs,
            target_w,
            target_h,
            loop_behavior,
            &crate::security::MediaSecurityPolicy::default(),
        )
    }

    #[cfg(feature = "gpu")]
    pub(crate) fn gif_frame_with_index(
        &self,
        src: &str,
        time_secs: f64,
        loop_behavior: crate::gif_cache::LoopBehavior,
    ) -> Result<Option<(usize, Arc<image::RgbaImage>)>, RasterError> {
        let frames = self
            .gifs
            .load_frames_with_policy(src, &crate::security::MediaSecurityPolicy::default())?;
        Ok(crate::gif_cache::GifFrameCache::frame_index_at_time_ms(
            &frames,
            time_secs * 1000.0,
            loop_behavior,
        )
        .map(|(index, image)| (index, Arc::new(image.clone()))))
    }

    #[cfg(feature = "gpu")]
    pub(crate) fn audio_visualizer_frame(
        &self,
        src: &str,
        width: u32,
        height: u32,
        color: Color,
        style: &crate::scene::VisualizerStyle,
        time: f64,
    ) -> Result<Arc<image::RgbaImage>, RasterError> {
        let mut pixmap = Pixmap::new(width.max(1), height.max(1))
            .ok_or_else(|| RasterError::Init("invalid audio visualizer dimensions".into()))?;
        let resources = RenderResources {
            font: &self.font,
            images: &self.images,
            videos: &self.videos,
            gifs: &self.gifs,
            lotties: &self.lotties,
            audios: &self.audios,
            sampling_fps: 30.0,
            security: &self.security,
        };
        render_audio_visualizer(
            &mut pixmap,
            &resources,
            src,
            0.0,
            0.0,
            width.max(1) as f32,
            height.max(1) as f32,
            color,
            style,
            time,
            Transform::identity(),
        )?;
        let image = RgbaImage::from_raw(width.max(1), height.max(1), pixmap.take())
            .ok_or_else(|| RasterError::ImageEncode("invalid visualizer pixels".into()))?;
        Ok(Arc::new(image))
    }
}

impl Default for TinySkiaBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl RasterizerBackend for TinySkiaBackend {
    fn render_frame(&self, scene: &Scene, config: &FrameConfig) -> Result<RgbaImage, RasterError> {
        let mut pixmap = Pixmap::new(config.width, config.height).ok_or_else(|| {
            RasterError::Init("Failed to create Pixmap — invalid dimensions".into())
        })?;

        // Pixmap::new already allocates zeroed (transparent) pixels.

        let resources = RenderResources {
            font: &self.font,
            images: &self.images,
            videos: &self.videos,
            gifs: &self.gifs,
            lotties: &self.lotties,
            audios: &self.audios,
            sampling_fps: config.fps,
            security: &self.security,
        };
        render_nodes(
            &mut pixmap,
            &scene.nodes,
            Transform::identity(),
            1.0,
            &resources,
        )?;

        // Transfer the pixel allocation; copying a full frame here doubles
        // output-buffer traffic and temporarily retains two frame buffers.
        let raw_data = pixmap.take();
        RgbaImage::from_raw(config.width, config.height, raw_data).ok_or_else(|| {
            RasterError::ImageEncode("Failed to build RgbaImage from pixel data".into())
        })
    }

    #[allow(clippy::type_complexity)]
    fn render_stream(
        &self,
        total: u32,
        scene_fn: &(dyn Fn(u32) -> Result<Scene, RasterError> + Sync),
        config_fn: &(dyn Fn(u32) -> FrameConfig + Sync),
        sink: &mut dyn FrameSink,
    ) -> Result<(), RasterError> {
        for frame in 0..total {
            let scene = scene_fn(frame)?;
            let cfg = config_fn(frame);
            let img = self.render_frame(&scene, &cfg)?;
            sink.consume(frame, img.as_raw())?;
        }
        Ok(())
    }
}

pub(crate) struct RenderResources<'a> {
    font: &'a FontCache,
    images: &'a ImageCache,
    videos: &'a VideoFrameCache,
    gifs: &'a GifFrameCache,
    lotties: &'a crate::lottie_cache::LottieCache,
    audios: &'a crate::audio_cache::AudioCache,
    sampling_fps: f64,
    security: &'a crate::security::MediaSecurityPolicy,
}
