//! Thread-safe Lottie animation asset cache and headless rasterizer.

use crate::backend::RasterError;
use image::RgbaImage;
use rasterlottie::{Animation, RenderConfig, Renderer};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Cached Lottie animation entry.
struct CachedLottie {
    animation: Animation,
    width: f32,
    height: f32,
    total_frames: f32,
    frame_rate: f32,
}

type LottieFrameKey = (PathBuf, u32, u32, u32);

/// Basic metadata for a Lottie animation, equivalent to Remotion's
/// `getLottieMetadata()` result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LottieMetadata {
    pub width: f32,
    pub height: f32,
    pub frame_rate: f32,
    pub duration_in_frames: f32,
}

/// Read Lottie dimensions, frame rate, and duration without rasterizing it.
pub fn get_lottie_metadata(
    src: impl AsRef<std::path::Path>,
) -> Result<LottieMetadata, RasterError> {
    let path = src.as_ref();
    let json_content = std::fs::read_to_string(path).map_err(|e| RasterError::ImageAsset {
        path: path.display().to_string(),
        reason: format!("failed to read Lottie file: {e}"),
    })?;
    let animation =
        Animation::from_json_str(&json_content).map_err(|e| RasterError::ImageAsset {
            path: path.display().to_string(),
            reason: format!("failed to parse Lottie JSON: {e}"),
        })?;
    Ok(LottieMetadata {
        width: animation.width as f32,
        height: animation.height as f32,
        frame_rate: if animation.frame_rate > 0.0 {
            animation.frame_rate
        } else {
            30.0
        },
        duration_in_frames: (animation.out_point - animation.in_point).max(1.0),
    })
}

#[derive(Default)]
pub(crate) struct LottieCache {
    animations: Mutex<HashMap<PathBuf, Arc<CachedLottie>>>,
    rendered_frames: Mutex<HashMap<LottieFrameKey, Arc<RgbaImage>>>,
}

impl LottieCache {
    /// Loads or retrieves a parsed Lottie animation.
    fn get_or_load(
        &self,
        src: &str,
        policy: &crate::security::MediaSecurityPolicy,
    ) -> Result<(Arc<CachedLottie>, PathBuf), RasterError> {
        let canonical = policy.validate_path(src).map_err(|e| match e {
            RasterError::MediaAsset { path, reason } => RasterError::ImageAsset { path, reason },
            other => other,
        })?;

        let mut cache = self.animations.lock().expect("lottie cache poisoned");
        if let Some(entry) = cache.get(&canonical) {
            return Ok((Arc::clone(entry), canonical));
        }

        let json_content =
            std::fs::read_to_string(&canonical).map_err(|e| RasterError::ImageAsset {
                path: canonical.display().to_string(),
                reason: format!("failed to read Lottie file: {e}"),
            })?;

        let animation =
            Animation::from_json_str(&json_content).map_err(|e| RasterError::ImageAsset {
                path: canonical.display().to_string(),
                reason: format!("failed to parse Lottie JSON: {e}"),
            })?;

        let width = animation.width as f32;
        let height = animation.height as f32;
        let total_frames = animation.out_point - animation.in_point;
        let frame_rate = animation.frame_rate;

        let entry = Arc::new(CachedLottie {
            animation,
            width,
            height,
            total_frames: total_frames.max(1.0),
            frame_rate: if frame_rate > 0.0 { frame_rate } else { 30.0 },
        });

        cache.insert(canonical.clone(), Arc::clone(&entry));
        Ok((entry, canonical))
    }

    /// Renders a specific frame of a Lottie animation at `target_w x target_h`.
    #[allow(dead_code)]
    pub(crate) fn render(
        &self,
        src: &str,
        time_secs: f64,
        target_w: u32,
        target_h: u32,
        loop_behavior: crate::gif_cache::LoopBehavior,
    ) -> Result<Arc<RgbaImage>, RasterError> {
        self.render_with_policy(
            src,
            time_secs,
            target_w,
            target_h,
            loop_behavior,
            &crate::security::MediaSecurityPolicy::default(),
        )
    }

    /// Renders a specific frame validating against a security policy.
    pub(crate) fn render_with_policy(
        &self,
        src: &str,
        time_secs: f64,
        target_w: u32,
        target_h: u32,
        loop_behavior: crate::gif_cache::LoopBehavior,
        policy: &crate::security::MediaSecurityPolicy,
    ) -> Result<Arc<RgbaImage>, RasterError> {
        let (entry, canonical) = self.get_or_load(src, policy)?;

        let total_duration_secs = (entry.total_frames / entry.frame_rate) as f64;
        let effective_time = match loop_behavior {
            crate::gif_cache::LoopBehavior::Loop => {
                if total_duration_secs > 0.0 {
                    (time_secs % total_duration_secs + total_duration_secs) % total_duration_secs
                } else {
                    0.0
                }
            }
            crate::gif_cache::LoopBehavior::Pause => time_secs.max(0.0).min(total_duration_secs),
            crate::gif_cache::LoopBehavior::Unmount => {
                if time_secs > total_duration_secs {
                    return Ok(Arc::new(RgbaImage::new(1, 1)));
                }
                time_secs.max(0.0)
            }
        };

        // Calculate frame index
        let frame_idx = (entry.animation.in_point + (effective_time as f32 * entry.frame_rate))
            .clamp(entry.animation.in_point, entry.animation.out_point);

        // `frame_idx` may be fractional when the composition and animation
        // frame rates differ. Keep those requests distinct in the cache.
        let key = (canonical.clone(), frame_idx.to_bits(), target_w, target_h);

        {
            let frame_cache = self
                .rendered_frames
                .lock()
                .expect("rendered frames lock poisoned");
            if let Some(rendered) = frame_cache.get(&key) {
                return Ok(Arc::clone(rendered));
            }
        }

        let scale = if entry.width > 0.0 {
            (target_w as f32 / entry.width)
                .min(target_h as f32 / entry.height)
                .max(0.01)
        } else {
            1.0
        };

        let mut config = RenderConfig::default();
        config.scale = scale;

        let renderer = Renderer::default();
        let frame = renderer
            .render_frame(&entry.animation, frame_idx, config)
            .map_err(|e| RasterError::ImageAsset {
                path: canonical.display().to_string(),
                reason: format!("Lottie render error: {e}"),
            })?;

        let img =
            RgbaImage::from_raw(frame.width, frame.height, frame.pixels).ok_or_else(|| {
                RasterError::ImageAsset {
                    path: canonical.display().to_string(),
                    reason: "failed to construct RgbaImage from Lottie frame pixels".into(),
                }
            })?;
        let arc_img = Arc::new(img);

        let mut frame_cache = self
            .rendered_frames
            .lock()
            .expect("rendered frames lock poisoned");
        frame_cache.insert(key, Arc::clone(&arc_img));

        Ok(arc_img)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gif_cache::LoopBehavior;

    const ANIMATED_LOTTIE: &str = r#"
    {
      "v":"5.7.6",
      "fr":30,
      "ip":0,
      "op":2,
      "w":100,
      "h":100,
      "layers":[{
        "nm":"Shape Layer 1",
        "ind":1,
        "ty":4,
        "shapes":[{
          "ty":"gr",
          "it":[
            {"ty":"rc","p":{"a":1,"k":[
              {"t":0,"s":[10,50],"e":[90,50],"i":{"x":[1,1],"y":[1,1]},"o":{"x":[0,0],"y":[0,0]}},
              {"t":2,"s":[90,50]}
            ]},"s":{"a":0,"k":[10,10]},"r":{"a":0,"k":0}},
            {"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100}},
            {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
          ]
        }]
      }]
    }
    "#;

    fn direct_render(frame_idx: f32) -> RgbaImage {
        let animation = Animation::from_json_str(ANIMATED_LOTTIE).unwrap();
        let frame = Renderer::default()
            .render_frame(&animation, frame_idx, RenderConfig::default())
            .unwrap();
        RgbaImage::from_raw(frame.width, frame.height, frame.pixels).unwrap()
    }

    #[test]
    fn fractional_frame_cache_keys_preserve_frame_and_request_order() {
        let temp_dir = tempfile::tempdir().unwrap();
        let animation_path = temp_dir.path().join("fractional-frames.json");
        std::fs::write(&animation_path, ANIMATED_LOTTIE).unwrap();
        let animation_path = animation_path.to_str().unwrap();

        // Both positions round to frame 0 in the old cache key, though they
        // produce visibly different positions in this fast-moving animation.
        let early_frame = 0.25_f32;
        let late_frame = 0.49_f32;
        let expected_early = direct_render(early_frame);
        let expected_late = direct_render(late_frame);
        assert_ne!(expected_early, expected_late);

        let render = |cache: &LottieCache, frame: f32| {
            cache
                .render(
                    animation_path,
                    f64::from(frame) / 30.0,
                    100,
                    100,
                    LoopBehavior::Pause,
                )
                .unwrap()
        };

        let forward_cache = LottieCache::default();
        let forward_early = render(&forward_cache, early_frame);
        let forward_late = render(&forward_cache, late_frame);
        assert_eq!(forward_early.as_ref(), &expected_early);
        assert_eq!(forward_late.as_ref(), &expected_late);

        let reverse_cache = LottieCache::default();
        let reverse_late = render(&reverse_cache, late_frame);
        let reverse_early = render(&reverse_cache, early_frame);
        assert_eq!(reverse_late.as_ref(), &expected_late);
        assert_eq!(reverse_early.as_ref(), &expected_early);
    }
}
