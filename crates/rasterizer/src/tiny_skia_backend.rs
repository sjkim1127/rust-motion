//! CPU rasterizer backend using `tiny-skia`.
//!
//! Renders a [`Scene`] into an RGBA pixel buffer without any GPU or browser dependency.

use crate::backend::{FrameConfig, FrameSink, RasterError, RasterizerBackend};
use crate::font::FontCache;
use crate::gif_cache::GifFrameCache;
use crate::image_cache::ImageCache;

use crate::scene::{
    BlendMode, ClipRegion, Color, ImageFit, MaskMode, Scene, SceneFilter, SceneNode, SceneShadow,
};
use crate::video_cache::VideoFrameCache;
use image::{imageops, RgbaImage};
use rayon::prelude::*;
#[cfg(feature = "gpu")]
use std::sync::Arc;
use tiny_skia::{
    BlendMode as SkBlendMode, FillRule, IntSize, Mask, MaskType, Paint, Path, PathBuilder, Pixmap,
    PixmapPaint, Rect, Stroke, Transform,
};

const MAX_IMAGE_NODE_PIXELS: u64 = 16 * 1024 * 1024;

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

struct RenderResources<'a> {
    font: &'a FontCache,
    images: &'a ImageCache,
    videos: &'a VideoFrameCache,
    gifs: &'a GifFrameCache,
    lotties: &'a crate::lottie_cache::LottieCache,
    audios: &'a crate::audio_cache::AudioCache,
    sampling_fps: f64,
    security: &'a crate::security::MediaSecurityPolicy,
}

fn render_nodes(
    pixmap: &mut Pixmap,
    nodes: &[SceneNode],
    parent_transform: Transform,
    parent_opacity: f32,
    resources: &RenderResources<'_>,
) -> Result<(), RasterError> {
    for node in nodes {
        render_node(pixmap, node, parent_transform, parent_opacity, resources)?;
    }
    Ok(())
}

fn render_node(
    pixmap: &mut Pixmap,
    node: &SceneNode,
    transform: Transform,
    opacity: f32,
    resources: &RenderResources<'_>,
) -> Result<(), RasterError> {
    match node {
        SceneNode::Rect {
            x,
            y,
            w,
            h,
            fill,
            stroke,
            stroke_width,
            corner_radius,
        } => {
            let rect = match Rect::from_xywh(*x, *y, *w, *h) {
                Some(r) => r,
                None => return Ok(()),
            };

            let path = if *corner_radius > 0.0 {
                build_rounded_rect(*x, *y, *w, *h, *corner_radius)
            } else {
                PathBuilder::from_rect(rect)
            };

            // Fill
            let mut paint = Paint::default();
            paint.set_color(apply_opacity(*fill, opacity));
            paint.anti_alias = true;
            pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);

            // Stroke
            if let (Some(stroke_color), sw) = (stroke, stroke_width) {
                if *sw > 0.0 {
                    let mut stroke_paint = Paint::default();
                    stroke_paint.set_color(apply_opacity(*stroke_color, opacity));
                    stroke_paint.anti_alias = true;
                    let stroke = Stroke {
                        width: *sw,
                        ..Default::default()
                    };
                    pixmap.stroke_path(&path, &stroke_paint, &stroke, transform, None);
                }
            }
        }

        SceneNode::Circle {
            cx,
            cy,
            r,
            fill,
            stroke,
            stroke_width,
        } => {
            let path = build_circle(*cx, *cy, *r);

            let mut paint = Paint::default();
            paint.set_color(apply_opacity(*fill, opacity));
            paint.anti_alias = true;
            pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);

            if let (Some(stroke_color), sw) = (stroke, stroke_width) {
                if *sw > 0.0 {
                    let mut stroke_paint = Paint::default();
                    stroke_paint.set_color(apply_opacity(*stroke_color, opacity));
                    stroke_paint.anti_alias = true;
                    let stroke = Stroke {
                        width: *sw,
                        ..Default::default()
                    };
                    pixmap.stroke_path(&path, &stroke_paint, &stroke, transform, None);
                }
            }
        }

        SceneNode::Path {
            d,
            fill,
            stroke,
            stroke_width,
            opacity: node_opacity,
        } => {
            let combined_opacity = opacity * node_opacity;

            if let Some(path) = svgpath_to_tiny_skia(d) {
                if let Some(fill_color) = fill {
                    let mut paint = Paint::default();
                    paint.set_color(apply_opacity(*fill_color, combined_opacity));
                    paint.anti_alias = true;
                    pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
                }

                if let (Some(stroke_color), sw) = (stroke, stroke_width) {
                    if *sw > 0.0 {
                        let mut stroke_paint = Paint::default();
                        stroke_paint.set_color(apply_opacity(*stroke_color, combined_opacity));
                        stroke_paint.anti_alias = true;
                        let stroke = Stroke {
                            width: *sw,
                            ..Default::default()
                        };
                        pixmap.stroke_path(&path, &stroke_paint, &stroke, transform, None);
                    }
                }
            }
        }

        SceneNode::Image {
            src,
            x,
            y,
            w,
            h,
            fit,
            opacity: node_opacity,
        } => {
            let source = resources.images.load_with_policy(src, resources.security)?;
            draw_media(
                pixmap,
                &source,
                src,
                *x,
                *y,
                *w,
                *h,
                *fit,
                opacity * node_opacity,
                transform,
            )?;
        }

        SceneNode::Video {
            src,
            time,
            looped,
            x,
            y,
            w,
            h,
            fit,
            opacity: node_opacity,
        } => {
            let source = resources.videos.load_with_policy(
                src,
                *time,
                resources.sampling_fps,
                *looped,
                resources.security,
            )?;
            draw_video_media(
                pixmap,
                &source,
                src,
                *x,
                *y,
                *w,
                *h,
                *fit,
                opacity * node_opacity,
                transform,
            )?;
        }

        SceneNode::Audio { .. } => {}

        SceneNode::Gif {
            src,
            time,
            x,
            y,
            w,
            h,
            playback_rate,
            loop_behavior,
            fit,
            opacity: node_opacity,
        } => {
            let frames = resources
                .gifs
                .load_frames_with_policy(src, resources.security)?;
            // Convert composition time to GIF playback time in milliseconds.
            // `time` is already the composition time in seconds; scale by playback_rate.
            let time_ms = *time * *playback_rate as f64 * 1000.0;
            let maybe_frame = GifFrameCache::frame_at_time_ms(&frames, time_ms, *loop_behavior);
            if let Some(frame_image) = maybe_frame {
                draw_media(
                    pixmap,
                    frame_image,
                    src,
                    *x,
                    *y,
                    *w,
                    *h,
                    *fit,
                    opacity * node_opacity,
                    transform,
                )?;
            }
        }

        SceneNode::Lottie {
            src,
            time,
            x,
            y,
            w,
            h,
            playback_rate,
            loop_behavior,
            opacity: node_opacity,
        } => {
            let time_secs = *time * *playback_rate as f64;
            let target_w = (*w).round().max(1.0) as u32;
            let target_h = (*h).round().max(1.0) as u32;
            let frame_image = resources.lotties.render_with_policy(
                src,
                time_secs,
                target_w,
                target_h,
                *loop_behavior,
                resources.security,
            )?;
            draw_media(
                pixmap,
                &frame_image,
                src,
                *x,
                *y,
                *w,
                *h,
                ImageFit::Contain,
                opacity * node_opacity,
                transform,
            )?;
        }

        SceneNode::Emoji {
            emoji,
            x,
            y,
            size,
            opacity: node_opacity,
        } => {
            let target_size = (*size).round().max(8.0) as u32;
            if let Some(emoji_img) = crate::emoji::render_emoji(emoji, target_size) {
                draw_media(
                    pixmap,
                    &emoji_img,
                    emoji,
                    *x,
                    *y,
                    *size,
                    *size,
                    ImageFit::Contain,
                    opacity * node_opacity,
                    transform,
                )?;
            }
        }

        SceneNode::AudioVisualizer {
            src,
            x,
            y,
            width,
            height,
            color,
            style,
            time,
            opacity: node_opacity,
        } => {
            let eff_opacity = opacity * node_opacity;
            if eff_opacity > 0.0 && *width > 0.0 && *height > 0.0 {
                render_audio_visualizer(
                    pixmap,
                    resources,
                    src,
                    *x,
                    *y,
                    *width,
                    *height,
                    color.with_opacity(eff_opacity),
                    style,
                    *time,
                    transform,
                )?;
            }
        }

        SceneNode::LinearGradient {
            x,
            y,
            w,
            h,
            angle_deg,
            stops,
        } => {
            if stops.is_empty() {
                return Ok(());
            }

            let rect = match Rect::from_xywh(*x, *y, *w, *h) {
                Some(r) => r,
                None => return Ok(()),
            };
            let path = PathBuilder::from_rect(rect);

            // Compute gradient endpoints from angle
            let angle_rad = angle_deg.to_radians();
            let cx = x + w / 2.0;
            let cy = y + h / 2.0;
            let half_diag = (w * w + h * h).sqrt() / 2.0;

            let x1 = cx - angle_rad.sin() * half_diag;
            let y1 = cy - angle_rad.cos() * half_diag;
            let x2 = cx + angle_rad.sin() * half_diag;
            let y2 = cy + angle_rad.cos() * half_diag;

            let sk_stops: Vec<tiny_skia::GradientStop> = stops
                .iter()
                .map(|s| tiny_skia::GradientStop::new(s.position, apply_opacity(s.color, opacity)))
                .collect();

            if let Some(shader) = tiny_skia::LinearGradient::new(
                tiny_skia::Point::from_xy(x1, y1),
                tiny_skia::Point::from_xy(x2, y2),
                sk_stops,
                tiny_skia::SpreadMode::Pad,
                Transform::identity(),
            ) {
                let paint = Paint {
                    shader,
                    anti_alias: true,
                    ..Default::default()
                };
                pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
            }
        }

        SceneNode::RadialGradient { cx, cy, r, stops } => {
            if stops.is_empty() {
                return Ok(());
            }

            let path = build_circle(*cx, *cy, *r);

            let sk_stops: Vec<tiny_skia::GradientStop> = stops
                .iter()
                .map(|s| tiny_skia::GradientStop::new(s.position, apply_opacity(s.color, opacity)))
                .collect();

            if let Some(shader) = tiny_skia::RadialGradient::new(
                tiny_skia::Point::from_xy(*cx, *cy),
                tiny_skia::Point::from_xy(*cx, *cy),
                *r,
                sk_stops,
                tiny_skia::SpreadMode::Pad,
                Transform::identity(),
            ) {
                let paint = Paint {
                    shader,
                    anti_alias: true,
                    ..Default::default()
                };
                pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
            }
        }

        SceneNode::Group {
            transform: group_transform,
            opacity: group_opacity,
            children,
        } => {
            let new_transform = transform.post_concat(group_transform.to_tiny_skia());
            let new_opacity = opacity * group_opacity;
            render_nodes(pixmap, children, new_transform, new_opacity, resources)?;
        }

        SceneNode::Layer {
            opacity: layer_opacity,
            blend_mode,
            clip,
            mask,
            mask_mode,
            filters,
            shadow,
            children,
        } => {
            if !opacity.is_finite() || !layer_opacity.is_finite() {
                return Err(RasterError::Scene(
                    "layer and inherited opacity must be finite".into(),
                ));
            }
            if opacity <= 0.0 || *layer_opacity <= 0.0 {
                // A fully transparent layer cannot affect the destination;
                // avoid evaluating children or allocating compositing surfaces.
                return Ok(());
            }
            if children.is_empty() && clip.is_none() && mask.is_none() && shadow.is_none() {
                // Preserve filter validation while avoiding a full-canvas
                // surface for an empty layer (for example an overlay filter).
                let mut validation_surface = Pixmap::new(1, 1).ok_or_else(|| {
                    RasterError::Scene("failed to allocate filter validation surface".into())
                })?;
                for filter in filters {
                    apply_filter(&mut validation_surface, filter)?;
                }
                return Ok(());
            }
            if filters.is_empty()
                && shadow.is_none()
                && clip.is_none()
                && mask.is_none()
                && *blend_mode == BlendMode::Normal
                && (opacity - 1.0).abs() <= f32::EPSILON
                && (*layer_opacity - 1.0).abs() <= f32::EPSILON
            {
                // A fully opaque normal layer without compositing features is
                // equivalent to its children; avoid a full-canvas allocation
                // and copy on the common grouping path.
                render_nodes(pixmap, children, transform, opacity, resources)?;
                return Ok(());
            }
            let mut layer = Pixmap::new(pixmap.width(), pixmap.height())
                .ok_or_else(|| RasterError::Scene("failed to allocate layer surface".into()))?;
            render_nodes(&mut layer, children, transform, 1.0, resources)?;

            for filter in filters {
                apply_filter(&mut layer, filter)?;
            }
            if let Some(clip) = clip {
                let clip_mask = render_clip_mask(pixmap.width(), pixmap.height(), clip, transform)?;
                layer.apply_mask(&clip_mask);
            }
            if let Some(mask_nodes) = mask {
                let mut mask_pixmap = Pixmap::new(pixmap.width(), pixmap.height())
                    .ok_or_else(|| RasterError::Scene("failed to allocate mask surface".into()))?;
                render_nodes(&mut mask_pixmap, mask_nodes, transform, 1.0, resources)?;
                let mask_type = match mask_mode {
                    MaskMode::Alpha => MaskType::Alpha,
                    MaskMode::Luminance => MaskType::Luminance,
                };
                layer.apply_mask(&Mask::from_pixmap(mask_pixmap.as_ref(), mask_type));
            }

            let composite_paint = PixmapPaint {
                opacity: (opacity * layer_opacity).clamp(0.0, 1.0),
                blend_mode: tiny_skia_blend_mode(*blend_mode),
                ..Default::default()
            };
            if let Some(shadow) = shadow {
                let shadow_pixmap = make_shadow(&layer, shadow)?;
                pixmap.draw_pixmap(
                    shadow.offset_x.round() as i32,
                    shadow.offset_y.round() as i32,
                    shadow_pixmap.as_ref(),
                    &composite_paint,
                    Transform::identity(),
                    None,
                );
            }
            pixmap.draw_pixmap(
                0,
                0,
                layer.as_ref(),
                &composite_paint,
                Transform::identity(),
                None,
            );
        }

        SceneNode::Text {
            x,
            y,
            content,
            font_size,
            color,
            font_sources,
            font_weight,
        } => {
            let has_emoji = content.chars().any(crate::emoji::is_emoji_char);
            if has_emoji {
                let runs = crate::emoji::split_text_and_emojis(content);
                let mut pen_x = *x;
                for run in runs {
                    match run {
                        crate::emoji::TextRun::Text(text_part) => {
                            if text_part.is_empty() {
                                continue;
                            }
                            let part_width = crate::font::measure_text_width(
                                &text_part,
                                *font_size,
                                font_sources,
                            )
                            .unwrap_or_else(|_| {
                                text_part.chars().count() as f32 * *font_size * 0.5
                            });
                            let sub_node = SceneNode::Text {
                                x: pen_x,
                                y: *y,
                                content: text_part,
                                font_size: *font_size,
                                color: *color,
                                font_weight: *font_weight,
                                font_sources: font_sources.clone(),
                            };
                            render_node(pixmap, &sub_node, transform, opacity, resources)?;
                            pen_x += part_width;
                        }
                        crate::emoji::TextRun::Emoji(emoji_str) => {
                            let emoji_node = SceneNode::Emoji {
                                emoji: emoji_str,
                                x: pen_x,
                                y: *y - *font_size * 0.85,
                                size: *font_size,
                                opacity: 1.0,
                            };
                            render_node(pixmap, &emoji_node, transform, opacity, resources)?;
                            pen_x += *font_size * 1.08;
                        }
                    }
                }
                return Ok(());
            }

            let text_color = apply_opacity(*color, opacity);

            if let Some(rendered) = resources
                .font
                .rasterize_with_weight(content, *font_size, *font_weight, font_sources)
                .map_err(|error| RasterError::FontAsset {
                    path: error.path,
                    reason: error.reason,
                })?
            {
                // Blit the glyph coverage map onto the pixmap at (x, y - baseline)
                let origin_x = x.floor() as i32;
                let origin_y = (*y - rendered.baseline as f32).floor() as i32;

                let pw = pixmap.width() as i32;
                let ph = pixmap.height() as i32;
                let pixmap_width = pixmap.width(); // cache before mutable borrow

                let pixels_rgba = pixmap.pixels_mut();

                for gy in 0..rendered.height {
                    for gx in 0..rendered.width {
                        let coverage = rendered.pixels[(gy * rendered.width + gx) as usize];
                        if coverage == 0 {
                            continue;
                        }

                        let px = origin_x + gx as i32;
                        let py = origin_y + gy as i32;
                        if px < 0 || py < 0 || px >= pw || py >= ph {
                            continue;
                        }

                        let idx = (py as u32 * pixmap_width + px as u32) as usize;
                        let glyph_coverage = coverage as f32 / 255.0;
                        let src_alpha = glyph_coverage * text_color.alpha();
                        if src_alpha <= 0.0 {
                            continue;
                        }
                        let inv_src_alpha = 1.0 - src_alpha;

                        // Premultiplied source channels in 0..=255
                        let src_r = text_color.red() * 255.0 * src_alpha;
                        let src_g = text_color.green() * 255.0 * src_alpha;
                        let src_b = text_color.blue() * 255.0 * src_alpha;
                        let src_a = 255.0 * src_alpha;

                        let dst = pixels_rgba[idx];
                        let out_r = (src_r + dst.red() as f32 * inv_src_alpha)
                            .round()
                            .min(255.0) as u8;
                        let out_g = (src_g + dst.green() as f32 * inv_src_alpha)
                            .round()
                            .min(255.0) as u8;
                        let out_b = (src_b + dst.blue() as f32 * inv_src_alpha)
                            .round()
                            .min(255.0) as u8;
                        let out_a = (src_a + dst.alpha() as f32 * inv_src_alpha)
                            .round()
                            .min(255.0) as u8;

                        pixels_rgba[idx] =
                            tiny_skia::PremultipliedColorU8::from_rgba(out_r, out_g, out_b, out_a)
                                .unwrap_or(pixels_rgba[idx]);
                    }
                }
            } else {
                // Fallback: draw a solid colour block per character (no font loaded)
                let char_w = *font_size * 0.6;
                let mut cx = *x;
                for _ch in content.chars() {
                    let rect = match Rect::from_xywh(
                        cx,
                        *y - font_size,
                        char_w.max(1.0),
                        font_size.max(1.0),
                    ) {
                        Some(r) => r,
                        None => {
                            cx += char_w;
                            continue;
                        }
                    };
                    let path = PathBuilder::from_rect(rect);
                    let mut paint = Paint::default();
                    paint.set_color(text_color);
                    paint.anti_alias = true;
                    pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
                    cx += char_w;
                }
            }
        }

        SceneNode::Shader {
            x,
            y,
            w,
            h,
            source: _,
            time,
            params,
            opacity: node_opacity,
        } => {
            let eff_opacity = opacity * node_opacity;
            if eff_opacity > 0.0 && *w > 0.0 && *h > 0.0 {
                let pw = (*w as u32).clamp(16, 256);
                let ph = (*h as u32).clamp(16, 256);
                if let Some(mut sm_pixmap) = Pixmap::new(pw, ph) {
                    let pixels = sm_pixmap.data_mut();
                    let t = *time;
                    let p0 = params[0].max(0.1);
                    let p1 = params[1].max(0.1);
                    let p2 = params[2].max(0.1);
                    for py in 0..ph {
                        let ny = py as f32 / ph as f32;
                        for px in 0..pw {
                            let nx = px as f32 / pw as f32;
                            let v1 = (nx * 10.0 + t).sin();
                            let v2 =
                                ((ny * 10.0 + t * 1.3).sin() + (nx * ny * 5.0 + t).cos()) * 0.5;
                            let r = (((v1 * 0.5 + 0.5) * p0) * 255.0).clamp(0.0, 255.0) as u8;
                            let g = (((v2 * 0.5 + 0.5) * p1) * 255.0).clamp(0.0, 255.0) as u8;
                            let b = (((((nx + ny) * 5.0 + t * 0.7).sin() * 0.5 + 0.5) * p2) * 255.0)
                                .clamp(0.0, 255.0) as u8;
                            let idx = ((py * pw + px) * 4) as usize;
                            pixels[idx] = r;
                            pixels[idx + 1] = g;
                            pixels[idx + 2] = b;
                            pixels[idx + 3] = (255.0 * eff_opacity).clamp(0.0, 255.0) as u8;
                        }
                    }
                    let sx = *w / pw as f32;
                    let sy = *h / ph as f32;
                    let shader_transform = transform.pre_translate(*x, *y).pre_scale(sx, sy);
                    let paint = PixmapPaint {
                        opacity: eff_opacity.clamp(0.0, 1.0),
                        ..Default::default()
                    };
                    pixmap.draw_pixmap(0, 0, sm_pixmap.as_ref(), &paint, shader_transform, None);
                }
            }
        }
    }
    Ok(())
}

// --- Helpers ---

fn apply_opacity(color: Color, opacity: f32) -> tiny_skia::Color {
    let a = (color.a as f32 * opacity.clamp(0.0, 1.0)) as u8;
    tiny_skia::Color::from_rgba8(color.r, color.g, color.b, a)
}

fn tiny_skia_blend_mode(mode: BlendMode) -> SkBlendMode {
    match mode {
        BlendMode::Normal => SkBlendMode::SourceOver,
        BlendMode::Multiply => SkBlendMode::Multiply,
        BlendMode::Screen => SkBlendMode::Screen,
        BlendMode::Overlay => SkBlendMode::Overlay,
        BlendMode::Darken => SkBlendMode::Darken,
        BlendMode::Lighten => SkBlendMode::Lighten,
        BlendMode::ColorDodge => SkBlendMode::ColorDodge,
        BlendMode::ColorBurn => SkBlendMode::ColorBurn,
        BlendMode::HardLight => SkBlendMode::HardLight,
        BlendMode::SoftLight => SkBlendMode::SoftLight,
        BlendMode::Difference => SkBlendMode::Difference,
        BlendMode::Exclusion => SkBlendMode::Exclusion,
    }
}

fn render_clip_mask(
    width: u32,
    height: u32,
    clip: &ClipRegion,
    transform: Transform,
) -> Result<Mask, RasterError> {
    let mut clip_pixmap = Pixmap::new(width, height)
        .ok_or_else(|| RasterError::Scene("failed to allocate clip surface".into()))?;
    let path = match clip {
        ClipRegion::Rect {
            x,
            y,
            w,
            h,
            corner_radius,
        } => {
            let Some(rect) = Rect::from_xywh(*x, *y, *w, *h) else {
                return Mask::new(width, height).ok_or_else(|| {
                    RasterError::Scene("failed to allocate empty clip mask".into())
                });
            };
            if *corner_radius > 0.0 {
                build_rounded_rect(*x, *y, *w, *h, *corner_radius)
            } else {
                PathBuilder::from_rect(rect)
            }
        }
        ClipRegion::Path { d } => svgpath_to_tiny_skia(d)
            .ok_or_else(|| RasterError::Scene(format!("invalid SVG clip path: {d}")))?,
    };
    let mut paint = Paint::default();
    paint.set_color(tiny_skia::Color::WHITE);
    paint.anti_alias = true;
    clip_pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
    Ok(Mask::from_pixmap(clip_pixmap.as_ref(), MaskType::Alpha))
}

fn apply_filter(pixmap: &mut Pixmap, filter: &SceneFilter) -> Result<(), RasterError> {
    match *filter {
        SceneFilter::Blur { sigma } => {
            if !sigma.is_finite() || !(0.0..=100.0).contains(&sigma) {
                return Err(RasterError::Scene(format!(
                    "blur sigma must be finite and between 0 and 100, got {sigma}"
                )));
            }
            let radius = (sigma * 1.5).ceil().clamp(0.0, 128.0) as usize;
            box_blur(pixmap, radius);
        }
        SceneFilter::Brightness { amount } => {
            if !amount.is_finite() || !(0.0..=10.0).contains(&amount) {
                return Err(RasterError::Scene(format!(
                    "brightness must be finite and between 0 and 10, got {amount}"
                )));
            }
            pixmap.data_mut().par_chunks_exact_mut(4).for_each(|pixel| {
                let alpha = pixel[3];
                for channel in &mut pixel[..3] {
                    *channel = (f32::from(*channel) * amount)
                        .round()
                        .clamp(0.0, f32::from(alpha)) as u8;
                }
            });
        }
        SceneFilter::Grayscale { amount } => {
            if !amount.is_finite() || !(0.0..=1.0).contains(&amount) {
                return Err(RasterError::Scene(format!(
                    "grayscale must be finite and between 0 and 1, got {amount}"
                )));
            }
            pixmap.data_mut().par_chunks_exact_mut(4).for_each(|pixel| {
                let alpha = pixel[3];
                let gray = 0.2126 * f32::from(pixel[0])
                    + 0.7152 * f32::from(pixel[1])
                    + 0.0722 * f32::from(pixel[2]);
                for channel in &mut pixel[..3] {
                    *channel = (f32::from(*channel) + (gray - f32::from(*channel)) * amount)
                        .round()
                        .clamp(0.0, f32::from(alpha)) as u8;
                }
            });
        }
        SceneFilter::Opacity { amount } => {
            if !amount.is_finite() || !(0.0..=1.0).contains(&amount) {
                return Err(RasterError::Scene(format!(
                    "filter opacity must be finite and between 0 and 1, got {amount}"
                )));
            }
            for channel in pixmap.data_mut() {
                *channel = (f32::from(*channel) * amount).round() as u8;
            }
        }
        SceneFilter::ChromaticAberration {
            offset_x,
            offset_y,
            angle_rad,
        } => {
            if !offset_x.is_finite() || !offset_y.is_finite() || !angle_rad.is_finite() {
                return Err(RasterError::Scene(
                    "chromatic aberration parameters must be finite".into(),
                ));
            }
            let cos_a = angle_rad.cos();
            let sin_a = angle_rad.sin();
            let dx = offset_x * cos_a - offset_y * sin_a;
            let dy = offset_x * sin_a + offset_y * cos_a;

            if dx.abs() < 1e-6 && dy.abs() < 1e-6 {
                return Ok(());
            }

            let width = pixmap.width() as i32;
            let height = pixmap.height() as i32;
            let src = pixmap.data().to_vec();

            let sample_channel = |x: f32, y: f32, channel: usize| -> f32 {
                if x < 0.0 || x > (width - 1) as f32 || y < 0.0 || y > (height - 1) as f32 {
                    return 0.0;
                }
                let x0 = x.floor() as i32;
                let y0 = y.floor() as i32;
                let x1 = (x0 + 1).min(width - 1);
                let y1 = (y0 + 1).min(height - 1);
                let fx = x - x.floor();
                let fy = y - y.floor();

                let p00 = src[((y0 * width + x0) * 4) as usize + channel] as f32;
                let p10 = src[((y0 * width + x1) * 4) as usize + channel] as f32;
                let p01 = src[((y1 * width + x0) * 4) as usize + channel] as f32;
                let p11 = src[((y1 * width + x1) * 4) as usize + channel] as f32;

                let top = p00 * (1.0 - fx) + p10 * fx;
                let bot = p01 * (1.0 - fx) + p11 * fx;
                top * (1.0 - fy) + bot * fy
            };

            let dst = pixmap.data_mut();
            for y in 0..height {
                for x in 0..width {
                    let idx = ((y * width + x) * 4) as usize;
                    let fx = x as f32;
                    let fy = y as f32;

                    let a_r = sample_channel(fx - dx, fy - dy, 3);
                    let a_g = src[idx + 3] as f32;
                    let a_b = sample_channel(fx + dx, fy + dy, 3);
                    let alpha = a_r.max(a_g).max(a_b);

                    if alpha == 0.0 {
                        continue;
                    }

                    let r = sample_channel(fx - dx, fy - dy, 0);
                    let g = src[idx + 1] as f32;
                    let b = sample_channel(fx + dx, fy + dy, 2);

                    dst[idx] = r.round().clamp(0.0, alpha) as u8;
                    dst[idx + 1] = g.round().clamp(0.0, alpha) as u8;
                    dst[idx + 2] = b.round().clamp(0.0, alpha) as u8;
                    dst[idx + 3] = alpha.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        SceneFilter::Vignette {
            offset,
            darkness,
            roundness,
        } => {
            if !offset.is_finite() || !darkness.is_finite() || !roundness.is_finite() {
                return Err(RasterError::Scene(
                    "vignette parameters must be finite".into(),
                ));
            }
            let off = offset.clamp(0.0, 2.0);
            let dark = darkness.clamp(0.0, 1.0);
            let rnd = roundness.clamp(0.0, 1.0);

            let width = pixmap.width() as f32;
            let height = pixmap.height() as f32;
            let max_diag = (1.0 - rnd) * 1.0 + rnd * 2.0f32.sqrt();
            let span = (max_diag - off).max(1e-5);
            let row_len = pixmap.width() as usize * 4;
            let px_by_x: Vec<f32> = (0..pixmap.width())
                .map(|x| {
                    let u = (x as f32 + 0.5) / width;
                    2.0 * (u - 0.5).abs()
                })
                .collect();

            pixmap
                .data_mut()
                .par_chunks_exact_mut(row_len)
                .enumerate()
                .for_each(|(y, row)| {
                    let v = (y as f32 + 0.5) / height;
                    let py = 2.0 * (v - 0.5).abs();
                    for (x, pixel) in row.chunks_exact_mut(4).enumerate() {
                        let px = px_by_x[x];

                        let d_rect = px.max(py);
                        let d_ellipse = (px * px + py * py).sqrt();
                        let d = (1.0 - rnd) * d_rect + rnd * d_ellipse;

                        let factor = if d <= off {
                            1.0
                        } else {
                            let t = ((d - off) / span).clamp(0.0, 1.0);
                            let s = t * t * (3.0 - 2.0 * t);
                            1.0 - dark * s
                        };

                        let alpha = pixel[3] as f32;
                        for ch in pixel[0..3].iter_mut() {
                            *ch = (*ch as f32 * factor).round().clamp(0.0, alpha) as u8;
                        }
                    }
                });
        }
        SceneFilter::Contrast { factor } => {
            if !factor.is_finite() || factor < 0.0 {
                return Err(RasterError::Scene(
                    "contrast factor must be non-negative and finite".into(),
                ));
            }
            pixmap.data_mut().par_chunks_exact_mut(4).for_each(|pixel| {
                let alpha = pixel[3] as f32;
                if alpha == 0.0 {
                    return;
                }
                for channel in &mut pixel[..3] {
                    let unpre = (*channel as f32 / alpha) * 255.0;
                    let contrasted = (unpre - 128.0) * factor + 128.0;
                    *channel = ((contrasted / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
                }
            });
        }
        SceneFilter::Saturation { factor } => {
            if !factor.is_finite() || factor < 0.0 {
                return Err(RasterError::Scene(
                    "saturation factor must be non-negative and finite".into(),
                ));
            }
            pixmap.data_mut().par_chunks_exact_mut(4).for_each(|pixel| {
                let alpha = pixel[3] as f32;
                if alpha == 0.0 {
                    return;
                }
                let r_unpre = (pixel[0] as f32 / alpha) * 255.0;
                let g_unpre = (pixel[1] as f32 / alpha) * 255.0;
                let b_unpre = (pixel[2] as f32 / alpha) * 255.0;

                let luma = 0.299 * r_unpre + 0.587 * g_unpre + 0.114 * b_unpre;

                let r_sat = (luma + (r_unpre - luma) * factor).clamp(0.0, 255.0);
                let g_sat = (luma + (g_unpre - luma) * factor).clamp(0.0, 255.0);
                let b_sat = (luma + (b_unpre - luma) * factor).clamp(0.0, 255.0);

                pixel[0] = ((r_sat / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
                pixel[1] = ((g_sat / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
                pixel[2] = ((b_sat / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
            });
        }
        SceneFilter::HueRotate { degrees } => {
            if !degrees.is_finite() {
                return Err(RasterError::Scene("hue degrees must be finite".into()));
            }
            let shift = (degrees % 360.0 + 360.0) % 360.0;
            if shift.abs() < 1e-4 {
                return Ok(());
            }

            pixmap.data_mut().par_chunks_exact_mut(4).for_each(|pixel| {
                let alpha = pixel[3] as f32;
                if alpha == 0.0 {
                    return;
                }
                let r_u = pixel[0] as f32 / alpha;
                let g_u = pixel[1] as f32 / alpha;
                let b_u = pixel[2] as f32 / alpha;

                let max = r_u.max(g_u).max(b_u);
                let min = r_u.min(g_u).min(b_u);
                let delta = max - min;
                let v = max;
                let s = if max > 0.0 { delta / max } else { 0.0 };
                let mut h = if delta.abs() < 1e-6 {
                    0.0
                } else if (max - r_u).abs() < 1e-6 {
                    60.0 * (((g_u - b_u) / delta) % 6.0)
                } else if (max - g_u).abs() < 1e-6 {
                    60.0 * (((b_u - r_u) / delta) + 2.0)
                } else {
                    60.0 * (((r_u - g_u) / delta) + 4.0)
                };
                if h < 0.0 {
                    h += 360.0;
                }

                h = (h + shift) % 360.0;

                let c = v * s;
                let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
                let m = v - c;

                let (r_norm, g_norm, b_norm) = match (h / 60.0) as u32 {
                    0 => (c, x, 0.0),
                    1 => (x, c, 0.0),
                    2 => (0.0, c, x),
                    3 => (0.0, x, c),
                    4 => (x, 0.0, c),
                    _ => (c, 0.0, x),
                };

                pixel[0] = ((r_norm + m) * alpha).round().clamp(0.0, alpha) as u8;
                pixel[1] = ((g_norm + m) * alpha).round().clamp(0.0, alpha) as u8;
                pixel[2] = ((b_norm + m) * alpha).round().clamp(0.0, alpha) as u8;
            });
        }
        SceneFilter::Invert { amount } => {
            if !amount.is_finite() || !(0.0..=1.0).contains(&amount) {
                return Err(RasterError::Scene(format!(
                    "invert amount must be finite and between 0 and 1, got {amount}"
                )));
            }
            pixmap.data_mut().par_chunks_exact_mut(4).for_each(|pixel| {
                let alpha = pixel[3] as f32;
                if alpha == 0.0 {
                    return;
                }
                for channel in &mut pixel[..3] {
                    let unpre = (*channel as f32 / alpha) * 255.0;
                    let inverted = unpre + (255.0 - 2.0 * unpre) * amount;
                    *channel = ((inverted / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
                }
            });
        }
        SceneFilter::Tint { color, amount } => {
            if !amount.is_finite() || !(0.0..=1.0).contains(&amount) {
                return Err(RasterError::Scene(format!(
                    "tint amount must be finite and between 0 and 1, got {amount}"
                )));
            }
            let tr = color[0] as f32;
            let tg = color[1] as f32;
            let tb = color[2] as f32;
            let ta = (color[3] as f32 / 255.0) * amount;

            pixmap.data_mut().par_chunks_exact_mut(4).for_each(|pixel| {
                let alpha = pixel[3] as f32;
                if alpha == 0.0 {
                    return;
                }
                let r_unpre = (pixel[0] as f32 / alpha) * 255.0;
                let g_unpre = (pixel[1] as f32 / alpha) * 255.0;
                let b_unpre = (pixel[2] as f32 / alpha) * 255.0;

                let r_tint = r_unpre * (1.0 - ta) + tr * ta;
                let g_tint = g_unpre * (1.0 - ta) + tg * ta;
                let b_tint = b_unpre * (1.0 - ta) + tb * ta;

                pixel[0] = ((r_tint / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
                pixel[1] = ((g_tint / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
                pixel[2] = ((b_tint / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
            });
        }
        SceneFilter::Duotone { primary, secondary } => {
            let pr = primary[0] as f32;
            let pg = primary[1] as f32;
            let pb = primary[2] as f32;
            let sr = secondary[0] as f32;
            let sg = secondary[1] as f32;
            let sb = secondary[2] as f32;

            for pixel in pixmap.data_mut().chunks_exact_mut(4) {
                let alpha = pixel[3] as f32;
                if alpha == 0.0 {
                    continue;
                }
                let r_unpre = (pixel[0] as f32 / alpha) * 255.0;
                let g_unpre = (pixel[1] as f32 / alpha) * 255.0;
                let b_unpre = (pixel[2] as f32 / alpha) * 255.0;

                let luma = (0.299 * r_unpre + 0.587 * g_unpre + 0.114 * b_unpre) / 255.0;
                let luma = luma.clamp(0.0, 1.0);

                let r_duo = pr * (1.0 - luma) + sr * luma;
                let g_duo = pg * (1.0 - luma) + sg * luma;
                let b_duo = pb * (1.0 - luma) + sb * luma;

                pixel[0] = ((r_duo / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
                pixel[1] = ((g_duo / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
                pixel[2] = ((b_duo / 255.0) * alpha).round().clamp(0.0, alpha) as u8;
            }
        }
        SceneFilter::ColorGrading {
            contrast,
            saturation,
            gamma,
            tint,
        } => {
            if !contrast.is_finite()
                || !saturation.is_finite()
                || !gamma.is_finite()
                || gamma <= 0.0
                || contrast < 0.0
                || saturation < 0.0
            {
                return Err(RasterError::Scene(
                    "color grading parameters must be positive/finite".into(),
                ));
            }
            let inv_gamma = 1.0 / gamma;

            for pixel in pixmap.data_mut().chunks_exact_mut(4) {
                let alpha = pixel[3] as f32;
                if alpha == 0.0 {
                    continue;
                }
                let r_unpre = (pixel[0] as f32 / alpha) * 255.0;
                let g_unpre = (pixel[1] as f32 / alpha) * 255.0;
                let b_unpre = (pixel[2] as f32 / alpha) * 255.0;

                // 1. Gamma
                let mut r = 255.0 * (r_unpre / 255.0).clamp(0.0, 1.0).powf(inv_gamma);
                let mut g = 255.0 * (g_unpre / 255.0).clamp(0.0, 1.0).powf(inv_gamma);
                let mut b = 255.0 * (b_unpre / 255.0).clamp(0.0, 1.0).powf(inv_gamma);

                // 2. Contrast
                r = (r - 128.0) * contrast + 128.0;
                g = (g - 128.0) * contrast + 128.0;
                b = (b - 128.0) * contrast + 128.0;

                // 3. Saturation (Rec.601)
                let luma = 0.299 * r + 0.587 * g + 0.114 * b;
                r = luma + (r - luma) * saturation;
                g = luma + (g - luma) * saturation;
                b = luma + (b - luma) * saturation;

                // 4. Tint
                if let Some(tint_col) = tint {
                    let tr = tint_col[0] as f32;
                    let tg = tint_col[1] as f32;
                    let tb = tint_col[2] as f32;
                    let ta = (tint_col[3] as f32) / 255.0;
                    r = r * (1.0 - ta) + tr * ta;
                    g = g * (1.0 - ta) + tg * ta;
                    b = b * (1.0 - ta) + tb * ta;
                }

                pixel[0] = ((r.clamp(0.0, 255.0) / 255.0) * alpha)
                    .round()
                    .clamp(0.0, alpha) as u8;
                pixel[1] = ((g.clamp(0.0, 255.0) / 255.0) * alpha)
                    .round()
                    .clamp(0.0, alpha) as u8;
                pixel[2] = ((b.clamp(0.0, 255.0) / 255.0) * alpha)
                    .round()
                    .clamp(0.0, alpha) as u8;
            }
        }
        SceneFilter::ColorKey {
            key_color,
            similarity,
            smoothness,
            spill_suppression,
        } => {
            if !similarity.is_finite()
                || !smoothness.is_finite()
                || !spill_suppression.is_finite()
                || similarity < 0.0
                || smoothness < 0.0
            {
                return Err(RasterError::Scene(
                    "color key parameters must be non-negative and finite".into(),
                ));
            }
            let kr = key_color[0] as f32 / 255.0;
            let kg = key_color[1] as f32 / 255.0;
            let kb = key_color[2] as f32 / 255.0;
            let sqrt3 = 3.0f32.sqrt();

            let edge0 = (similarity - smoothness).max(0.0);
            let edge1 = (similarity + smoothness).min(1.0);
            let ss = spill_suppression.clamp(0.0, 1.0);

            for pixel in pixmap.data_mut().chunks_exact_mut(4) {
                let alpha = pixel[3] as f32;
                if alpha <= 0.25 {
                    continue;
                }
                let mut r_u = pixel[0] as f32 / alpha;
                let mut g_u = pixel[1] as f32 / alpha;
                let mut b_u = pixel[2] as f32 / alpha;

                let dr = r_u - kr;
                let dg = g_u - kg;
                let db = b_u - kb;
                let dist = ((dr * dr + dg * dg + db * db).sqrt() / sqrt3).clamp(0.0, 1.0);

                let mask = if edge1 > edge0 {
                    let t = ((dist - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
                    t * t * (3.0 - 2.0 * t)
                } else if dist >= similarity {
                    1.0
                } else {
                    0.0
                };

                if ss > 0.0 {
                    if kg > kr && kg > kb {
                        let max_g = (r_u + b_u) / 2.0;
                        if g_u > max_g {
                            g_u = g_u * (1.0 - ss) + max_g * ss;
                        }
                    } else if kb > kr && kb > kg {
                        let max_b = (r_u + g_u) / 2.0;
                        if b_u > max_b {
                            b_u = b_u * (1.0 - ss) + max_b * ss;
                        }
                    } else if kr > kg && kr > kb {
                        let max_r = (g_u + b_u) / 2.0;
                        if r_u > max_r {
                            r_u = r_u * (1.0 - ss) + max_r * ss;
                        }
                    }
                }

                let new_alpha = (alpha * mask).round().clamp(0.0, 255.0);
                pixel[0] = (r_u * new_alpha).round().clamp(0.0, new_alpha) as u8;
                pixel[1] = (g_u * new_alpha).round().clamp(0.0, new_alpha) as u8;
                pixel[2] = (b_u * new_alpha).round().clamp(0.0, new_alpha) as u8;
                pixel[3] = new_alpha as u8;
            }
        }
        SceneFilter::CameraMotionBlur {
            shutter_angle,
            samples,
            shutter_phase,
        } => {
            // Validate inputs
            if !shutter_angle.is_finite() || !shutter_phase.is_finite() {
                return Err(RasterError::Scene(
                    "CameraMotionBlur parameters must be finite".into(),
                ));
            }
            let n = samples.max(1) as usize;
            let angle_clamped = shutter_angle.clamp(0.0, 360.0);

            // If angle is effectively zero, nothing to blur
            if angle_clamped < 0.01 || n == 1 {
                return Ok(());
            }

            // The shutter covers `shutter_angle / 360` of a frame duration.
            // `shutter_phase` offsets where the exposure window starts
            // (−0.5 = centred on current frame, 0.0 = trailing blur).
            //
            // We sample at fractional pixel displacements: each sample i
            // corresponds to time offset t_i = (phase + i/(n−1) * exposure) within frame.
            // We encode this as a horizontal pixel shift proportional to t_i.
            // The maximum shift (at t = 1 full frame) is a small fraction of width.
            let w = pixmap.width() as i32;
            let h = pixmap.height() as i32;
            let exposure = angle_clamped / 360.0; // fraction of one frame
            let max_shift_px = (w as f32 * 0.04 * exposure).max(1.0); // ≤4% of width

            // Snapshot the original pixel data before accumulation
            let src = pixmap.data().to_vec();
            // Accumulator: f32 RGBA per pixel
            let mut acc: Vec<f32> = vec![0.0; (w * h * 4) as usize];
            let weight = 1.0 / n as f32;

            for i in 0..n {
                // Fractional position within the exposure window [0, 1]
                let frac = if n == 1 {
                    0.5
                } else {
                    i as f32 / (n - 1) as f32
                };
                // Time offset within the frame (can be negative)
                let t = shutter_phase + frac * exposure;
                let shift = (t * max_shift_px).round() as i32;

                for y in 0..h {
                    for x in 0..w {
                        let sx = (x + shift).clamp(0, w - 1);
                        let src_idx = ((y * w + sx) * 4) as usize;
                        let dst_idx = ((y * w + x) * 4) as usize;
                        for c in 0..4 {
                            acc[dst_idx + c] += src[src_idx + c] as f32 * weight;
                        }
                    }
                }
            }

            // Write accumulated result back into pixmap
            let dst = pixmap.data_mut();
            for (i, v) in acc.iter().enumerate() {
                dst[i] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(())
}

fn make_shadow(source: &Pixmap, shadow: &SceneShadow) -> Result<Pixmap, RasterError> {
    if !shadow.blur_sigma.is_finite() || !(0.0..=100.0).contains(&shadow.blur_sigma) {
        return Err(RasterError::Scene(format!(
            "shadow blur sigma must be finite and between 0 and 100, got {}",
            shadow.blur_sigma
        )));
    }
    if !shadow.offset_x.is_finite() || !shadow.offset_y.is_finite() {
        return Err(RasterError::Scene("shadow offsets must be finite".into()));
    }
    let mut output = Pixmap::new(source.width(), source.height())
        .ok_or_else(|| RasterError::Scene("failed to allocate shadow surface".into()))?;
    for (source_pixel, shadow_pixel) in source
        .data()
        .chunks_exact(4)
        .zip(output.data_mut().chunks_exact_mut(4))
    {
        let alpha = (u16::from(source_pixel[3]) * u16::from(shadow.color.a) / 255) as u8;
        shadow_pixel[0] = (u16::from(shadow.color.r) * u16::from(alpha) / 255) as u8;
        shadow_pixel[1] = (u16::from(shadow.color.g) * u16::from(alpha) / 255) as u8;
        shadow_pixel[2] = (u16::from(shadow.color.b) * u16::from(alpha) / 255) as u8;
        shadow_pixel[3] = alpha;
    }
    let radius = (shadow.blur_sigma * 1.5).ceil().clamp(0.0, 128.0) as usize;
    box_blur(&mut output, radius);
    Ok(output)
}

fn box_blur(pixmap: &mut Pixmap, radius: usize) {
    if radius == 0 {
        return;
    }
    let width = pixmap.width() as usize;
    let height = pixmap.height() as usize;
    let kernel = (radius * 2 + 1) as u32;
    let source = pixmap.data().to_vec();
    let mut horizontal = vec![0_u8; source.len()];

    horizontal
        .par_chunks_exact_mut(width * 4)
        .enumerate()
        .for_each(|(y, row)| {
            let mut sums = [0_u32; 4];
            for x in 0..=radius.min(width - 1) {
                let index = (y * width + x) * 4;
                for channel in 0..4 {
                    sums[channel] += u32::from(source[index + channel]);
                }
            }
            for x in 0..width {
                let offset = x * 4;
                for channel in 0..4 {
                    row[offset + channel] = (sums[channel] / kernel) as u8;
                }
                if x >= radius {
                    let remove = (y * width + x - radius) * 4;
                    for channel in 0..4 {
                        sums[channel] -= u32::from(source[remove + channel]);
                    }
                }
                if x + radius + 1 < width {
                    let add = (y * width + x + radius + 1) * 4;
                    for channel in 0..4 {
                        sums[channel] += u32::from(source[add + channel]);
                    }
                }
            }
        });

    let output = pixmap.data_mut();
    output
        .par_chunks_exact_mut(width * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..width {
                let mut sums = [0_u32; 4];
                let start_y = y.saturating_sub(radius);
                let end_y = y.saturating_add(radius).min(height - 1);
                for sample_y in start_y..=end_y {
                    let index = (sample_y * width + x) * 4;
                    for channel in 0..4 {
                        sums[channel] += u32::from(horizontal[index + channel]);
                    }
                }
                let offset = x * 4;
                for channel in 0..4 {
                    row[offset + channel] = (sums[channel] / kernel) as u8;
                }
                if y >= radius {
                    let remove = ((y - radius) * width + x) * 4;
                    for channel in 0..4 {
                        sums[channel] -= u32::from(horizontal[remove + channel]);
                    }
                }
                if y + radius + 1 < height {
                    let add = (y + radius + 1) * width + x;
                    for channel in 0..4 {
                        sums[channel] += u32::from(horizontal[add + channel]);
                    }
                }
            }
        });
}

#[allow(clippy::too_many_arguments)]
fn render_audio_visualizer(
    pixmap: &mut Pixmap,
    resources: &RenderResources<'_>,
    src: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Color,
    style: &crate::scene::VisualizerStyle,
    time: f64,
    transform: Transform,
) -> Result<(), RasterError> {
    resources.security.validate_path(src)?;
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
    paint.anti_alias = true;

    match style {
        crate::scene::VisualizerStyle::Bars {
            count,
            gap,
            radius,
            mirror,
        } => {
            let n_bars = (*count).max(1);
            let bins = resources
                .audios
                .get_spectrum_with_policy(src, time, n_bars, resources.security)
                .unwrap_or_else(|_| vec![0.0; n_bars]);
            let total_gap = gap * (n_bars - 1) as f32;
            let bar_width = ((w - total_gap) / n_bars as f32).max(1.0);

            for (i, &mag) in bins.iter().enumerate() {
                let bx = x + i as f32 * (bar_width + gap);
                let bar_h = (mag * h).max(2.0);

                let (by, bh) = if *mirror {
                    let half_h = bar_h * 0.5;
                    let cy = y + h * 0.5;
                    (cy - half_h, bar_h)
                } else {
                    (y + h - bar_h, bar_h)
                };

                if let Some(rect) = Rect::from_xywh(bx, by, bar_width, bh) {
                    if *radius > 0.0 {
                        let path = build_rounded_rect(bx, by, bar_width, bh, *radius);
                        pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
                    } else {
                        let path = PathBuilder::from_rect(rect);
                        pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
                    }
                }
            }
        }
        crate::scene::VisualizerStyle::Wave {
            stroke_width,
            filled,
        } => {
            let n_points = ((w / 4.0) as usize).clamp(16, 256);
            let window_secs = 0.05; // 50ms window
            let points = resources
                .audios
                .get_waveform_slice_with_policy(
                    src,
                    time,
                    window_secs,
                    n_points,
                    resources.security,
                )
                .unwrap_or_else(|_| vec![0.0; n_points]);

            if points.len() < 2 {
                return Ok(());
            }

            let mid_y = y + h * 0.5;
            let half_h = h * 0.45;
            let step_x = w / (points.len() - 1) as f32;

            let mut pb = PathBuilder::new();
            pb.move_to(x, mid_y + points[0] * half_h);

            for (i, &val) in points.iter().enumerate().skip(1) {
                let px = x + i as f32 * step_x;
                let py = mid_y + val * half_h;
                pb.line_to(px, py);
            }

            if *filled {
                let mut fill_pb = pb.clone();
                fill_pb.line_to(x + w, y + h);
                fill_pb.line_to(x, y + h);
                fill_pb.close();
                if let Some(fill_path) = fill_pb.finish() {
                    let mut fill_paint = paint.clone();
                    fill_paint.set_color_rgba8(
                        color.r,
                        color.g,
                        color.b,
                        (color.a as f32 * 0.35) as u8,
                    );
                    pixmap.fill_path(&fill_path, &fill_paint, FillRule::Winding, transform, None);
                }
            }

            if let Some(stroke_path) = pb.finish() {
                let stroke = Stroke {
                    width: *stroke_width,
                    line_cap: tiny_skia::LineCap::Round,
                    line_join: tiny_skia::LineJoin::Round,
                    ..Default::default()
                };
                pixmap.stroke_path(&stroke_path, &paint, &stroke, transform, None);
            }
        }
        crate::scene::VisualizerStyle::Radial {
            radius,
            bar_count,
            bar_length,
        } => {
            let n_bars = (*bar_count).max(8);
            let bins = resources
                .audios
                .get_spectrum_with_policy(src, time, n_bars, resources.security)
                .unwrap_or_else(|_| vec![0.0; n_bars]);

            let cx = x + w * 0.5;
            let cy = y + h * 0.5;
            let r_base = *radius;

            let stroke = Stroke {
                width: (2.0 * std::f32::consts::PI * r_base / n_bars as f32 * 0.6).clamp(1.5, 6.0),
                line_cap: tiny_skia::LineCap::Round,
                ..Default::default()
            };

            for (i, &mag) in bins.iter().enumerate() {
                let angle = (i as f32 / n_bars as f32) * 2.0 * std::f32::consts::PI
                    - std::f32::consts::FRAC_PI_2;
                let cos_a = angle.cos();
                let sin_a = angle.sin();

                let len = (mag * bar_length).max(3.0);
                let x0 = cx + cos_a * r_base;
                let y0 = cy + sin_a * r_base;
                let x1 = cx + cos_a * (r_base + len);
                let y1 = cy + sin_a * (r_base + len);

                let mut pb = PathBuilder::new();
                pb.move_to(x0, y0);
                pb.line_to(x1, y1);
                if let Some(path) = pb.finish() {
                    pixmap.stroke_path(&path, &paint, &stroke, transform, None);
                }
            }
        }
    }

    Ok(())
}

fn rounded_dimension(value: f32) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else {
        value.round().clamp(1.0, u32::MAX as f32) as u32
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_media(
    pixmap: &mut Pixmap,
    source: &RgbaImage,
    src: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fit: ImageFit,
    opacity: f32,
    transform: Transform,
) -> Result<(), RasterError> {
    draw_media_with_filter(
        pixmap,
        source,
        src,
        x,
        y,
        w,
        h,
        fit,
        opacity,
        transform,
        imageops::FilterType::Lanczos3,
    )
}

#[allow(clippy::too_many_arguments)]
fn draw_video_media(
    pixmap: &mut Pixmap,
    source: &RgbaImage,
    src: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fit: ImageFit,
    opacity: f32,
    transform: Transform,
) -> Result<(), RasterError> {
    draw_media_with_filter(
        pixmap,
        source,
        src,
        x,
        y,
        w,
        h,
        fit,
        opacity,
        transform,
        imageops::FilterType::Triangle,
    )
}

#[allow(clippy::too_many_arguments)]
fn draw_media_with_filter(
    pixmap: &mut Pixmap,
    source: &RgbaImage,
    src: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fit: ImageFit,
    opacity: f32,
    transform: Transform,
    resize_filter: imageops::FilterType,
) -> Result<(), RasterError> {
    let width = rounded_dimension(w);
    let height = rounded_dimension(h);
    if width == 0 || height == 0 {
        return Ok(());
    }
    if u64::from(width) * u64::from(height) > MAX_IMAGE_NODE_PIXELS {
        return Err(RasterError::MediaAsset {
            path: src.into(),
            reason: format!(
                "destination {width}x{height} exceeds the {} pixel safety limit",
                MAX_IMAGE_NODE_PIXELS
            ),
        });
    }

    let fitted = fit_image_with_filter(source, width, height, fit, resize_filter);
    let media_pixmap = rgba_to_pixmap(fitted).ok_or_else(|| RasterError::MediaAsset {
        path: src.into(),
        reason: "media dimensions are too large for the rasterizer".into(),
    })?;
    let paint = PixmapPaint {
        opacity: opacity.clamp(0.0, 1.0),
        ..Default::default()
    };
    pixmap.draw_pixmap(
        x.round() as i32,
        y.round() as i32,
        media_pixmap.as_ref(),
        &paint,
        transform,
        None,
    );
    Ok(())
}

fn fit_image_with_filter(
    source: &RgbaImage,
    width: u32,
    height: u32,
    fit: ImageFit,
    resize_filter: imageops::FilterType,
) -> RgbaImage {
    let mut output = RgbaImage::new(width, height);
    if source.width() == 0 || source.height() == 0 || width == 0 || height == 0 {
        return output;
    }

    let effective_fit = match fit {
        ImageFit::ScaleDown if source.width() <= width && source.height() <= height => {
            ImageFit::None
        }
        ImageFit::ScaleDown => ImageFit::Contain,
        other => other,
    };

    match effective_fit {
        ImageFit::Fill => imageops::resize(source, width, height, resize_filter),
        ImageFit::Cover => {
            let scale =
                (width as f64 / source.width() as f64).max(height as f64 / source.height() as f64);
            let scaled_width = ((source.width() as f64 * scale).ceil() as u32).max(width);
            let scaled_height = ((source.height() as f64 * scale).ceil() as u32).max(height);
            let resized = imageops::resize(source, scaled_width, scaled_height, resize_filter);
            let crop_x = (scaled_width - width) / 2;
            let crop_y = (scaled_height - height) / 2;
            imageops::crop_imm(&resized, crop_x, crop_y, width, height).to_image()
        }
        ImageFit::Contain => {
            let scale =
                (width as f64 / source.width() as f64).min(height as f64 / source.height() as f64);
            let scaled_width = ((source.width() as f64 * scale).round() as u32).clamp(1, width);
            let scaled_height = ((source.height() as f64 * scale).round() as u32).clamp(1, height);
            let resized = imageops::resize(source, scaled_width, scaled_height, resize_filter);
            imageops::overlay(
                &mut output,
                &resized,
                ((width - scaled_width) / 2) as i64,
                ((height - scaled_height) / 2) as i64,
            );
            output
        }
        ImageFit::None => {
            imageops::overlay(
                &mut output,
                source,
                (width as i64 - source.width() as i64) / 2,
                (height as i64 - source.height() as i64) / 2,
            );
            output
        }
        ImageFit::ScaleDown => unreachable!("scale-down is normalized above"),
    }
}

fn rgba_to_pixmap(image: RgbaImage) -> Option<Pixmap> {
    let size = IntSize::from_wh(image.width(), image.height())?;
    let mut data = image.into_raw();
    for pixel in data.chunks_exact_mut(4) {
        let alpha = pixel[3] as u16;
        pixel[0] = ((pixel[0] as u16 * alpha + 127) / 255) as u8;
        pixel[1] = ((pixel[1] as u16 * alpha + 127) / 255) as u8;
        pixel[2] = ((pixel[2] as u16 * alpha + 127) / 255) as u8;
    }
    Pixmap::from_vec(data, size)
}

fn build_circle(cx: f32, cy: f32, r: f32) -> Path {
    let mut pb = PathBuilder::new();
    // Approximate circle with 4 cubic bezier curves (standard approximation)
    let k = 0.552_284_8 * r;
    pb.move_to(cx, cy - r);
    pb.cubic_to(cx + k, cy - r, cx + r, cy - k, cx + r, cy);
    pb.cubic_to(cx + r, cy + k, cx + k, cy + r, cx, cy + r);
    pb.cubic_to(cx - k, cy + r, cx - r, cy + k, cx - r, cy);
    pb.cubic_to(cx - r, cy - k, cx - k, cy - r, cx, cy - r);
    pb.close();
    pb.finish()
        .unwrap_or_else(|| PathBuilder::new().finish().unwrap())
}

fn build_rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();

    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();

    pb.finish()
        .unwrap_or_else(|| PathBuilder::from_rect(Rect::from_xywh(x, y, w, h).unwrap()))
}

/// Minimal SVG `d` attribute parser → tiny-skia PathBuilder.
/// Supports M, L, H, V, C, Q, A, Z commands. Shape emitters currently use
/// absolute commands; relative elliptical arcs are accepted as well.
pub(crate) fn svgpath_to_tiny_skia(d: &str) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let tokens = tokenize_path(d);
    let mut pos = 0usize;

    let mut cx = 0.0f32;
    let mut cy = 0.0f32;
    let mut subpath_x = 0.0f32;
    let mut subpath_y = 0.0f32;

    while pos < tokens.len() {
        match tokens[pos].as_str() {
            "M" => {
                pos += 1;
                let x = parse_f32(&tokens, &mut pos)?;
                let y = parse_f32(&tokens, &mut pos)?;
                pb.move_to(x, y);
                cx = x;
                cy = y;
                subpath_x = x;
                subpath_y = y;
            }
            "L" => {
                pos += 1;
                let x = parse_f32(&tokens, &mut pos)?;
                let y = parse_f32(&tokens, &mut pos)?;
                pb.line_to(x, y);
                cx = x;
                cy = y;
            }
            "H" => {
                pos += 1;
                let x = parse_f32(&tokens, &mut pos)?;
                pb.line_to(x, cy);
                cx = x;
            }
            "V" => {
                pos += 1;
                let y = parse_f32(&tokens, &mut pos)?;
                pb.line_to(cx, y);
                cy = y;
            }
            "C" => {
                pos += 1;
                let x1 = parse_f32(&tokens, &mut pos)?;
                let y1 = parse_f32(&tokens, &mut pos)?;
                let x2 = parse_f32(&tokens, &mut pos)?;
                let y2 = parse_f32(&tokens, &mut pos)?;
                let x = parse_f32(&tokens, &mut pos)?;
                let y = parse_f32(&tokens, &mut pos)?;
                pb.cubic_to(x1, y1, x2, y2, x, y);
                cx = x;
                cy = y;
            }
            "Q" => {
                pos += 1;
                let x1 = parse_f32(&tokens, &mut pos)?;
                let y1 = parse_f32(&tokens, &mut pos)?;
                let x = parse_f32(&tokens, &mut pos)?;
                let y = parse_f32(&tokens, &mut pos)?;
                pb.quad_to(x1, y1, x, y);
                cx = x;
                cy = y;
            }
            "A" | "a" => {
                let relative = tokens[pos] == "a";
                pos += 1;
                let rx = parse_f32(&tokens, &mut pos)?;
                let ry = parse_f32(&tokens, &mut pos)?;
                let rotation = parse_f32(&tokens, &mut pos)?;
                let large_arc = parse_f32(&tokens, &mut pos)? != 0.0;
                let sweep = parse_f32(&tokens, &mut pos)? != 0.0;
                let mut x = parse_f32(&tokens, &mut pos)?;
                let mut y = parse_f32(&tokens, &mut pos)?;
                if relative {
                    x += cx;
                    y += cy;
                }
                append_svg_arc(&mut pb, cx, cy, rx, ry, rotation, large_arc, sweep, x, y);
                cx = x;
                cy = y;
            }
            "Z" | "z" => {
                pb.close();
                pos += 1;
                cx = subpath_x;
                cy = subpath_y;
            }
            _ => return None,
        }
    }

    pb.finish()
}

#[allow(clippy::too_many_arguments)]
fn append_svg_arc(
    path: &mut PathBuilder,
    start_x: f32,
    start_y: f32,
    radius_x: f32,
    radius_y: f32,
    rotation_degrees: f32,
    large_arc: bool,
    sweep: bool,
    end_x: f32,
    end_y: f32,
) {
    if (start_x - end_x).abs() < f32::EPSILON && (start_y - end_y).abs() < f32::EPSILON {
        return;
    }
    let mut rx = radius_x.abs();
    let mut ry = radius_y.abs();
    if rx <= f32::EPSILON || ry <= f32::EPSILON {
        path.line_to(end_x, end_y);
        return;
    }

    let phi = rotation_degrees
        .to_radians()
        .rem_euclid(std::f32::consts::TAU);
    let (sin_phi, cos_phi) = phi.sin_cos();
    let half_dx = (start_x - end_x) * 0.5;
    let half_dy = (start_y - end_y) * 0.5;
    let start_prime_x = cos_phi * half_dx + sin_phi * half_dy;
    let start_prime_y = -sin_phi * half_dx + cos_phi * half_dy;

    let radii_scale = start_prime_x.powi(2) / rx.powi(2) + start_prime_y.powi(2) / ry.powi(2);
    if radii_scale > 1.0 {
        let scale = radii_scale.sqrt();
        rx *= scale;
        ry *= scale;
    }

    let rx_squared = rx.powi(2);
    let ry_squared = ry.powi(2);
    let x_squared = start_prime_x.powi(2);
    let y_squared = start_prime_y.powi(2);
    let numerator =
        (rx_squared * ry_squared - rx_squared * y_squared - ry_squared * x_squared).max(0.0);
    let denominator = rx_squared * y_squared + ry_squared * x_squared;
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let factor = if denominator <= f32::EPSILON {
        0.0
    } else {
        sign * (numerator / denominator).sqrt()
    };
    let center_prime_x = factor * rx * start_prime_y / ry;
    let center_prime_y = factor * -ry * start_prime_x / rx;
    let center_x = cos_phi * center_prime_x - sin_phi * center_prime_y + (start_x + end_x) * 0.5;
    let center_y = sin_phi * center_prime_x + cos_phi * center_prime_y + (start_y + end_y) * 0.5;

    let start_vector = (
        (start_prime_x - center_prime_x) / rx,
        (start_prime_y - center_prime_y) / ry,
    );
    let end_vector = (
        (-start_prime_x - center_prime_x) / rx,
        (-start_prime_y - center_prime_y) / ry,
    );
    let start_angle = start_vector.1.atan2(start_vector.0);
    let mut sweep_angle = vector_angle(start_vector, end_vector);
    if !sweep && sweep_angle > 0.0 {
        sweep_angle -= std::f32::consts::TAU;
    } else if sweep && sweep_angle < 0.0 {
        sweep_angle += std::f32::consts::TAU;
    }

    let segment_count = (sweep_angle.abs() / std::f32::consts::FRAC_PI_2)
        .ceil()
        .max(1.0) as usize;
    let segment_angle = sweep_angle / segment_count as f32;
    for segment in 0..segment_count {
        let angle_start = start_angle + segment_angle * segment as f32;
        let angle_end = angle_start + segment_angle;
        let alpha = 4.0 / 3.0 * (segment_angle * 0.25).tan();
        let start = ellipse_point(center_x, center_y, rx, ry, sin_phi, cos_phi, angle_start);
        let end = ellipse_point(center_x, center_y, rx, ry, sin_phi, cos_phi, angle_end);
        let start_derivative = ellipse_derivative(rx, ry, sin_phi, cos_phi, angle_start);
        let end_derivative = ellipse_derivative(rx, ry, sin_phi, cos_phi, angle_end);
        path.cubic_to(
            start.0 + alpha * start_derivative.0,
            start.1 + alpha * start_derivative.1,
            end.0 - alpha * end_derivative.0,
            end.1 - alpha * end_derivative.1,
            end.0,
            end.1,
        );
    }
}

fn vector_angle(from: (f32, f32), to: (f32, f32)) -> f32 {
    (from.0 * to.1 - from.1 * to.0).atan2(from.0 * to.0 + from.1 * to.1)
}

fn ellipse_point(
    center_x: f32,
    center_y: f32,
    rx: f32,
    ry: f32,
    sin_phi: f32,
    cos_phi: f32,
    angle: f32,
) -> (f32, f32) {
    let (sin_angle, cos_angle) = angle.sin_cos();
    (
        center_x + rx * cos_phi * cos_angle - ry * sin_phi * sin_angle,
        center_y + rx * sin_phi * cos_angle + ry * cos_phi * sin_angle,
    )
}

fn ellipse_derivative(rx: f32, ry: f32, sin_phi: f32, cos_phi: f32, angle: f32) -> (f32, f32) {
    let (sin_angle, cos_angle) = angle.sin_cos();
    (
        -rx * cos_phi * sin_angle - ry * sin_phi * cos_angle,
        -rx * sin_phi * sin_angle + ry * cos_phi * cos_angle,
    )
}

fn tokenize_path(d: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in d.chars() {
        if c.is_alphabetic() {
            if !current.trim().is_empty() {
                tokens.push(current.trim().to_string());
                current = String::new();
            }
            tokens.push(c.to_string());
        } else if c == ',' || c.is_whitespace() {
            if !current.trim().is_empty() {
                tokens.push(current.trim().to_string());
                current = String::new();
            }
        } else {
            current.push(c);
        }
    }
    if !current.trim().is_empty() {
        tokens.push(current.trim().to_string());
    }
    tokens
}

fn parse_f32(tokens: &[String], pos: &mut usize) -> Option<f32> {
    if *pos >= tokens.len() {
        return None;
    }
    let v = tokens[*pos].parse::<f32>().ok()?;
    *pos += 1;
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::FrameConfig;
    use crate::scene::{Color, ImageFit, Scene, SceneNode};
    use image::Rgba;

    fn render(scene: &Scene, w: u32, h: u32) -> RgbaImage {
        let backend = TinySkiaBackend::new();
        let config = FrameConfig::new(w, h, 0, 30.0);
        backend.render_frame(scene, &config).expect("render failed")
    }

    #[test]
    fn test_solid_red_rect() {
        let mut scene = Scene::new();
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            fill: Color::rgb(255, 0, 0),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });

        let img = render(&scene, 100, 100);
        let px = img.get_pixel(50, 50);
        assert_eq!(px[0], 255, "Red channel should be 255");
        assert_eq!(px[1], 0, "Green channel should be 0");
        assert_eq!(px[2], 0, "Blue channel should be 0");
    }

    #[test]
    fn test_circle_center_pixel() {
        let mut scene = Scene::new();
        scene.push(SceneNode::Circle {
            cx: 100.0,
            cy: 100.0,
            r: 80.0,
            fill: Color::rgb(0, 0, 255),
            stroke: None,
            stroke_width: 0.0,
        });

        let img = render(&scene, 200, 200);
        let px = img.get_pixel(100, 100);
        assert_eq!(px[2], 255, "Blue channel at circle center should be 255");
    }

    #[test]
    fn svg_elliptical_arcs_render_full_circles() {
        let scene = Scene {
            nodes: vec![SceneNode::Path {
                d: "M 50 0 A 50 50 0 1 0 50 100 A 50 50 0 1 0 50 0 Z".into(),
                fill: Some(Color::rgb(255, 0, 0)),
                stroke: None,
                stroke_width: 0.0,
                opacity: 1.0,
            }],
        };
        let image = render(&scene, 100, 100);

        assert!(image.get_pixel(50, 50)[0] > 240);
        assert_eq!(image.get_pixel(0, 0)[3], 0);
        assert_eq!(image.get_pixel(99, 99)[3], 0);
    }

    #[test]
    fn svg_arc_flags_select_the_expected_pie_quadrant() {
        let scene = Scene {
            nodes: vec![SceneNode::Path {
                d: "M 50 50 L 50 0 A 50 50 0 0 1 100 50 Z".into(),
                fill: Some(Color::rgb(0, 255, 0)),
                stroke: None,
                stroke_width: 0.0,
                opacity: 1.0,
            }],
        };
        let image = render(&scene, 100, 100);

        assert!(image.get_pixel(75, 25)[1] > 240);
        assert_eq!(image.get_pixel(25, 25)[3], 0);
        assert_eq!(image.get_pixel(75, 75)[3], 0);
    }

    #[test]
    fn test_transparent_background() {
        let scene = Scene::new(); // empty
        let img = render(&scene, 64, 64);
        let px = img.get_pixel(32, 32);
        assert_eq!(px[3], 0, "Empty scene should be fully transparent");
    }

    #[test]
    fn test_linear_gradient_renders() {
        use crate::scene::GradientStop;
        let mut scene = Scene::new();
        scene.push(SceneNode::LinearGradient {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 100.0,
            angle_deg: 90.0,
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: Color::rgb(255, 0, 0),
                },
                GradientStop {
                    position: 1.0,
                    color: Color::rgb(0, 0, 255),
                },
            ],
        });

        let img = render(&scene, 200, 100);
        // Left pixel should be more red than blue
        let left = img.get_pixel(5, 50);
        let right = img.get_pixel(195, 50);
        assert!(left[0] > left[2], "Left side of gradient should be redder");
        assert!(
            right[2] > right[0],
            "Right side of gradient should be bluer"
        );
    }

    #[test]
    fn test_group_with_opacity() {
        let mut scene = Scene::new();
        scene.push(SceneNode::Group {
            transform: Default::default(),
            opacity: 0.5,
            children: vec![SceneNode::Rect {
                x: 10.0,
                y: 10.0,
                w: 80.0,
                h: 80.0,
                fill: Color::rgb(255, 255, 255),
                stroke: None,
                stroke_width: 0.0,
                corner_radius: 0.0,
            }],
        });

        let img = render(&scene, 100, 100);
        let px = img.get_pixel(50, 50);
        // At 50% opacity over transparent, alpha should be ~127
        assert!(
            px[3] > 50 && px[3] < 200,
            "Group opacity should reduce alpha"
        );
    }

    fn composited_layer(children: Vec<SceneNode>) -> SceneNode {
        SceneNode::Layer {
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: MaskMode::Alpha,
            filters: Vec::new(),
            shadow: None,
            children,
        }
    }

    fn solid_rect(fill: Color, x: f32, y: f32, w: f32, h: f32) -> SceneNode {
        SceneNode::Rect {
            x,
            y,
            w,
            h,
            fill,
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        }
    }

    #[test]
    fn layer_rect_clip_limits_pixels() {
        let mut layer = composited_layer(vec![solid_rect(
            Color::rgb(255, 0, 0),
            0.0,
            0.0,
            64.0,
            64.0,
        )]);
        let SceneNode::Layer { clip, .. } = &mut layer else {
            unreachable!();
        };
        *clip = Some(ClipRegion::Rect {
            x: 16.0,
            y: 16.0,
            w: 32.0,
            h: 32.0,
            corner_radius: 0.0,
        });
        let image = render(&Scene { nodes: vec![layer] }, 64, 64);

        assert_eq!(image.get_pixel(32, 32), &Rgba([255, 0, 0, 255]));
        assert_eq!(image.get_pixel(8, 8)[3], 0);
    }

    #[test]
    fn opaque_normal_layer_matches_direct_children() {
        let children = vec![
            solid_rect(Color::rgb(255, 0, 0), 4.0, 5.0, 20.0, 12.0),
            solid_rect(Color::rgb(0, 0, 255), 12.0, 9.0, 20.0, 12.0),
        ];
        let direct = render(
            &Scene {
                nodes: children.clone(),
            },
            40,
            32,
        );
        let layered = render(
            &Scene {
                nodes: vec![composited_layer(children)],
            },
            40,
            32,
        );

        assert_eq!(direct, layered);
    }

    #[test]
    fn transparent_layer_does_not_render_children() {
        let layer = SceneNode::Layer {
            opacity: 0.0,
            blend_mode: BlendMode::Normal,
            clip: None,
            mask: None,
            mask_mode: MaskMode::Alpha,
            filters: vec![SceneFilter::Blur { sigma: 20.0 }],
            shadow: Some(SceneShadow {
                offset_x: 10.0,
                offset_y: 10.0,
                blur_sigma: 5.0,
                color: Color::WHITE,
            }),
            children: vec![solid_rect(Color::WHITE, 0.0, 0.0, 40.0, 32.0)],
        };
        let image = render(&Scene { nodes: vec![layer] }, 40, 32);

        assert!(image.pixels().all(|pixel| pixel[3] == 0));
    }

    #[test]
    fn layer_alpha_mask_controls_coverage() {
        let mut layer = composited_layer(vec![solid_rect(
            Color::rgb(255, 0, 0),
            0.0,
            0.0,
            64.0,
            64.0,
        )]);
        let SceneNode::Layer { mask, .. } = &mut layer else {
            unreachable!();
        };
        *mask = Some(vec![solid_rect(Color::WHITE, 0.0, 0.0, 32.0, 64.0)]);
        let image = render(&Scene { nodes: vec![layer] }, 64, 64);

        assert_eq!(image.get_pixel(16, 32), &Rgba([255, 0, 0, 255]));
        assert_eq!(image.get_pixel(48, 32)[3], 0);
    }

    #[test]
    fn layer_luminance_mask_uses_rendered_color() {
        let mut layer = composited_layer(vec![solid_rect(
            Color::rgb(0, 0, 255),
            0.0,
            0.0,
            64.0,
            64.0,
        )]);
        let SceneNode::Layer {
            mask, mask_mode, ..
        } = &mut layer
        else {
            unreachable!();
        };
        *mask_mode = MaskMode::Luminance;
        *mask = Some(vec![
            solid_rect(Color::WHITE, 0.0, 0.0, 32.0, 64.0),
            solid_rect(Color::BLACK, 32.0, 0.0, 32.0, 64.0),
        ]);
        let image = render(&Scene { nodes: vec![layer] }, 64, 64);

        assert_eq!(image.get_pixel(16, 32), &Rgba([0, 0, 255, 255]));
        assert_eq!(image.get_pixel(48, 32)[3], 0);
    }

    #[test]
    fn layer_multiply_blends_with_destination() {
        let background = solid_rect(Color::rgb(200, 100, 50), 0.0, 0.0, 32.0, 32.0);
        let mut layer = composited_layer(vec![solid_rect(
            Color::rgb(128, 255, 255),
            0.0,
            0.0,
            32.0,
            32.0,
        )]);
        let SceneNode::Layer { blend_mode, .. } = &mut layer else {
            unreachable!();
        };
        *blend_mode = BlendMode::Multiply;
        let image = render(
            &Scene {
                nodes: vec![background, layer],
            },
            32,
            32,
        );
        let pixel = image.get_pixel(16, 16);

        assert!((98..=102).contains(&pixel[0]));
        assert!((98..=102).contains(&pixel[1]));
        assert!((48..=52).contains(&pixel[2]));
    }

    #[test]
    fn layer_filters_apply_in_order() {
        let mut layer = composited_layer(vec![solid_rect(
            Color::rgb(255, 0, 0),
            0.0,
            0.0,
            16.0,
            16.0,
        )]);
        let SceneNode::Layer { filters, .. } = &mut layer else {
            unreachable!();
        };
        *filters = vec![
            SceneFilter::Grayscale { amount: 1.0 },
            SceneFilter::Opacity { amount: 0.5 },
        ];
        let image = render(&Scene { nodes: vec![layer] }, 16, 16);
        let pixel = image.get_pixel(8, 8);

        assert!((26..=28).contains(&pixel[0]));
        assert!((26..=28).contains(&pixel[1]));
        assert!((26..=28).contains(&pixel[2]));
        assert!((127..=128).contains(&pixel[3]));
    }

    #[test]
    fn layer_shadow_uses_masked_alpha_and_blur() {
        let mut layer = composited_layer(vec![solid_rect(Color::WHITE, 8.0, 8.0, 12.0, 12.0)]);
        let SceneNode::Layer { shadow, .. } = &mut layer else {
            unreachable!();
        };
        *shadow = Some(SceneShadow {
            offset_x: 20.0,
            offset_y: 0.0,
            blur_sigma: 2.0,
            color: Color::rgb(255, 0, 0),
        });
        let image = render(&Scene { nodes: vec![layer] }, 48, 32);
        let shadow_pixel = image.get_pixel(34, 14);

        assert!(shadow_pixel[0] > 0);
        assert_eq!(shadow_pixel[1], 0);
        assert_eq!(shadow_pixel[2], 0);
        assert!(image.get_pixel(26, 14)[3] > 0, "blur should expand alpha");
    }

    #[test]
    fn invalid_layer_filter_returns_scene_error() {
        let mut layer = composited_layer(Vec::new());
        let SceneNode::Layer { filters, .. } = &mut layer else {
            unreachable!();
        };
        filters.push(SceneFilter::Blur { sigma: f32::NAN });
        let error = TinySkiaBackend::headless()
            .render_frame(
                &Scene { nodes: vec![layer] },
                &FrameConfig::new(16, 16, 0, 30.0),
            )
            .unwrap_err();

        assert!(error.to_string().contains("Scene compositing error"));
        assert!(error.to_string().contains("blur sigma"));
    }

    #[test]
    fn missing_explicit_font_returns_asset_error() {
        let scene = Scene {
            nodes: vec![SceneNode::Text {
                x: 0.0,
                y: 16.0,
                content: "missing".into(),
                font_size: 16.0,
                color: Color::WHITE,
                font_weight: 400,
                font_sources: vec!["/dioxuscut/does-not-exist.ttf".into()],
            }],
        };
        let error = TinySkiaBackend::headless()
            .render_frame(&scene, &FrameConfig::new(64, 32, 0, 30.0))
            .unwrap_err();

        assert!(error.to_string().contains("Font asset error"));
        assert!(error.to_string().contains("does-not-exist.ttf"));
    }

    #[test]
    fn local_image_is_fitted_and_decoded_once() {
        let path = std::env::temp_dir().join(format!(
            "dioxuscut-rasterizer-image-{}.png",
            std::process::id()
        ));
        RgbaImage::from_pixel(4, 2, Rgba([255, 0, 0, 255]))
            .save(&path)
            .unwrap();

        let scene = Scene {
            nodes: vec![SceneNode::Image {
                src: path.display().to_string(),
                x: 0.0,
                y: 0.0,
                w: 4.0,
                h: 4.0,
                fit: ImageFit::Contain,
                opacity: 1.0,
            }],
        };
        let backend = TinySkiaBackend::headless();
        let config = FrameConfig::new(4, 4, 0, 30.0);
        let first = backend.render_frame(&scene, &config).unwrap();
        let second = backend.render_frame(&scene, &config).unwrap();

        assert_eq!(first.get_pixel(2, 0)[3], 0, "letterbox should be clear");
        assert_eq!(first.get_pixel(2, 2), &Rgba([255, 0, 0, 255]));
        assert_eq!(first, second);
        assert_eq!(backend.images.len(), 1);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn missing_image_returns_an_asset_error() {
        let path = std::env::temp_dir().join(format!(
            "dioxuscut-missing-image-{}.png",
            std::process::id()
        ));
        let scene = Scene {
            nodes: vec![SceneNode::Image {
                src: path.display().to_string(),
                x: 0.0,
                y: 0.0,
                w: 4.0,
                h: 4.0,
                fit: ImageFit::Cover,
                opacity: 1.0,
            }],
        };
        let error = TinySkiaBackend::headless()
            .render_frame(&scene, &FrameConfig::new(4, 4, 0, 30.0))
            .unwrap_err();

        assert!(error.to_string().contains("Image asset error"));
        assert!(error.to_string().contains("dioxuscut-missing-image"));
    }

    #[test]
    fn video_frame_is_decoded_and_cached() {
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_err()
        {
            eprintln!("skipping video decode test: FFmpeg is unavailable");
            return;
        }

        let dir =
            std::env::temp_dir().join(format!("dioxuscut-video-frame-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("red.mkv");
        let generated = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=16x16:r=2:d=7",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000:duration=7",
                "-shortest",
                "-c:v",
                "ffv1",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&source)
            .status()
            .unwrap();
        assert!(generated.success());

        let scene = Scene {
            nodes: vec![SceneNode::Video {
                src: source.display().to_string(),
                time: 0.0,
                looped: false,
                x: 0.0,
                y: 0.0,
                w: 16.0,
                h: 16.0,
                fit: ImageFit::Cover,
                opacity: 1.0,
            }],
        };
        let backend = TinySkiaBackend::headless();
        let config = FrameConfig::new(16, 16, 0, 2.0);
        let first = backend.render_frame(&scene, &config).unwrap();
        let mut next_scene = scene.clone();
        let SceneNode::Video { time, .. } = &mut next_scene.nodes[0] else {
            unreachable!();
        };
        *time = 0.5;
        let second = backend.render_frame(&next_scene, &config).unwrap();

        let pixel = first.get_pixel(8, 8);
        assert!(pixel[0] > 240 && pixel[1] < 20 && pixel[2] < 20);
        assert_eq!(first, second);
        assert!(backend.videos.bytes() > 0);
        assert_eq!(
            backend.videos.spawn_count(),
            1,
            "sequential frames should reuse one persistent decoder"
        );

        let mut jump_scene = scene.clone();
        let SceneNode::Video { time, .. } = &mut jump_scene.nodes[0] else {
            unreachable!();
        };
        *time = 6.5;
        backend.render_frame(&jump_scene, &config).unwrap();
        assert_eq!(
            backend.videos.spawn_count(),
            2,
            "large seeks should restart"
        );

        let mut reverse_scene = scene.clone();
        let SceneNode::Video { time, .. } = &mut reverse_scene.nodes[0] else {
            unreachable!();
        };
        *time = 1.0;
        backend.render_frame(&reverse_scene, &config).unwrap();
        assert_eq!(
            backend.videos.spawn_count(),
            3,
            "uncached reverse seeks should restart"
        );

        let mut loop_scene = scene.clone();
        let SceneNode::Video { time, looped, .. } = &mut loop_scene.nodes[0] else {
            unreachable!();
        };
        *time = 7.25;
        *looped = true;
        backend.render_frame(&loop_scene, &config).unwrap();

        let mut eof_scene = scene.clone();
        let SceneNode::Video { time, .. } = &mut eof_scene.nodes[0] else {
            unreachable!();
        };
        *time = 20.0;
        backend.render_frame(&eof_scene, &config).unwrap();
        assert_eq!(backend.videos.decoder_count(), 1);

        let metadata = crate::probe_video_metadata(source.to_str().unwrap()).unwrap();
        assert_eq!((metadata.width, metadata.height), (16, 16));
        assert_eq!((metadata.display_width, metadata.display_height), (16, 16));
        assert_eq!(metadata.fps, Some(2.0));
        assert_eq!(metadata.audio_stream_indices.len(), 1);
        assert!(metadata.duration.is_some_and(|duration| duration >= 7.0));

        backend.shutdown_media();
        assert_eq!(backend.videos.decoder_count(), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
