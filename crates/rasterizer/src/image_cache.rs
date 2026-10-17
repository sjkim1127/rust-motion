//! Thread-safe decoded raster image cache shared by frame renders.

use crate::backend::RasterError;
use base64::Engine as _;
use image::RgbaImage;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

fn rasterize_svg(bytes: &[u8], source: &str) -> Result<RgbaImage, RasterError> {
    let tree =
        resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).map_err(|error| {
            RasterError::ImageAsset {
                path: source.to_string(),
                reason: format!("invalid SVG: {error}"),
            }
        })?;
    let size = tree.size().to_int_size();
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(size.width(), size.height()).ok_or_else(|| {
            RasterError::ImageAsset {
                path: source.to_string(),
                reason: "SVG has invalid or zero dimensions".into(),
            }
        })?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    RgbaImage::from_raw(size.width(), size.height(), pixmap.take()).ok_or_else(|| {
        RasterError::ImageAsset {
            path: source.to_string(),
            reason: "failed to construct RGBA image from SVG".into(),
        }
    })
}

pub(crate) struct ImageCache {
    decoded: Mutex<ImageCacheState>,
    max_bytes: usize,
}

const MAX_DATA_URI_BYTES: usize = 32 * 1024 * 1024;
/// Default decoded image cache budget for the native backend.
pub const DEFAULT_IMAGE_CACHE_BYTES: usize = 256 * 1024 * 1024;

#[derive(Default)]
struct ImageCacheState {
    images: HashMap<String, Arc<RgbaImage>>,
    lru: VecDeque<String>,
    bytes: usize,
}

impl ImageCacheState {
    fn get(&mut self, key: &str) -> Option<Arc<RgbaImage>> {
        let image = self.images.get(key).cloned()?;
        self.lru.retain(|entry| entry != key);
        self.lru.push_back(key.to_string());
        Some(image)
    }

    fn insert(&mut self, key: String, image: Arc<RgbaImage>, max_bytes: usize) -> Arc<RgbaImage> {
        if let Some(existing) = self.get(&key) {
            return existing;
        }
        let size = image.as_raw().len();
        self.images.insert(key.clone(), Arc::clone(&image));
        self.lru.push_back(key);
        self.bytes = self.bytes.saturating_add(size);
        while self.bytes > max_bytes {
            let Some(oldest) = self.lru.pop_front() else {
                break;
            };
            if let Some(evicted) = self.images.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(evicted.as_raw().len());
            }
        }
        image
    }
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::with_max_bytes(DEFAULT_IMAGE_CACHE_BYTES)
    }
}

impl ImageCache {
    pub(crate) fn with_max_bytes(max_bytes: usize) -> Self {
        Self {
            decoded: Mutex::new(ImageCacheState::default()),
            max_bytes: max_bytes.max(1),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn load(&self, src: &str) -> Result<Arc<RgbaImage>, RasterError> {
        self.load_with_policy(src, &crate::security::MediaSecurityPolicy::default())
    }

    pub(crate) fn load_with_policy(
        &self,
        src: &str,
        policy: &crate::security::MediaSecurityPolicy,
    ) -> Result<Arc<RgbaImage>, RasterError> {
        let trimmed = src.trim();
        if let Some(data) = trimmed.strip_prefix("data:") {
            return self.load_data_uri(trimmed, data);
        }

        let canonical = policy.validate_path(trimmed).map_err(|e| match e {
            RasterError::MediaAsset { path, reason } => RasterError::ImageAsset { path, reason },
            other => other,
        })?;

        let key = canonical.display().to_string();
        if let Some(image) = self
            .decoded
            .lock()
            .expect("image cache lock poisoned")
            .get(&key)
        {
            return Ok(image);
        }

        // Decode outside the global cache lock so different assets can load in
        // parallel. A second lookup below prevents replacing an image decoded
        // concurrently for the same key.
        let bytes = std::fs::read(&canonical).map_err(|error| RasterError::ImageAsset {
            path: canonical.display().to_string(),
            reason: error.to_string(),
        })?;
        let decoded = if canonical
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
        {
            rasterize_svg(&bytes, &canonical.display().to_string())?
        } else if canonical
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dng"))
            || rust_motion_dng::sniff(&bytes)
        {
            let dng_img = rust_motion_dng::decode(&bytes).map_err(|e| RasterError::ImageAsset {
                path: canonical.display().to_string(),
                reason: format!("failed to decode DNG: {e}"),
            })?;
            let mut rgba_bytes = Vec::with_capacity(dng_img.width * dng_img.height * 4);
            for px in dng_img.rgba.chunks_exact(4) {
                let to_srgb = |c: f32| -> u8 {
                    let s = if c <= 0.0031308 {
                        12.92 * c
                    } else {
                        1.055 * c.powf(1.0 / 2.4) - 0.055
                    };
                    (s * 255.0).clamp(0.0, 255.0).round() as u8
                };
                rgba_bytes.push(to_srgb(px[0]));
                rgba_bytes.push(to_srgb(px[1]));
                rgba_bytes.push(to_srgb(px[2]));
                rgba_bytes.push((px[3] * 255.0).clamp(0.0, 255.0).round() as u8);
            }
            RgbaImage::from_raw(dng_img.width as u32, dng_img.height as u32, rgba_bytes)
                .ok_or_else(|| RasterError::ImageAsset {
                    path: canonical.display().to_string(),
                    reason: "failed to construct RGBA image from DNG".into(),
                })?
        } else {
            image::load_from_memory(&bytes)
                .map_err(|error| RasterError::ImageAsset {
                    path: canonical.display().to_string(),
                    reason: error.to_string(),
                })?
                .to_rgba8()
        };
        let decoded = Arc::new(decoded);
        let mut cache = self.decoded.lock().expect("image cache lock poisoned");
        Ok(cache.insert(key, decoded, self.max_bytes))
    }

    fn load_data_uri(&self, src: &str, data: &str) -> Result<Arc<RgbaImage>, RasterError> {
        if let Some(image) = self
            .decoded
            .lock()
            .expect("image cache lock poisoned")
            .get(src)
        {
            return Ok(image);
        }
        let Some((metadata, encoded)) = data.split_once(',') else {
            return Err(RasterError::ImageAsset {
                path: src.to_string(),
                reason: "data URI is missing its payload".into(),
            });
        };
        let mime = metadata
            .split(';')
            .next()
            .filter(|value| {
                matches!(
                    *value,
                    "image/png" | "image/jpeg" | "image/webp" | "image/svg+xml"
                )
            })
            .ok_or_else(|| RasterError::ImageAsset {
                path: src.to_string(),
                reason: "only PNG, JPEG, WebP, and SVG data URIs are supported".into(),
            })?;
        let is_base64 = metadata.split(';').any(|part| part == "base64");
        if !is_base64 && mime != "image/svg+xml" {
            return Err(RasterError::ImageAsset {
                path: src.to_string(),
                reason: "PNG, JPEG, and WebP data URIs must be base64-encoded".into(),
            });
        }
        if encoded.len() > (MAX_DATA_URI_BYTES * 4 / 3) + 4 {
            return Err(RasterError::ImageAsset {
                path: src.to_string(),
                reason: format!("data URI exceeds the {MAX_DATA_URI_BYTES} byte limit"),
            });
        }
        let bytes = if is_base64 {
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|error| RasterError::ImageAsset {
                    path: src.to_string(),
                    reason: format!("invalid base64 image data: {error}"),
                })?
        } else {
            urlencoding::decode(encoded)
                .map_err(|error| RasterError::ImageAsset {
                    path: src.to_string(),
                    reason: format!("invalid percent-encoded SVG data: {error}"),
                })?
                .into_owned()
                .into_bytes()
        };
        if bytes.len() > MAX_DATA_URI_BYTES {
            return Err(RasterError::ImageAsset {
                path: src.to_string(),
                reason: format!("data URI exceeds the {MAX_DATA_URI_BYTES} byte limit"),
            });
        }
        let decoded = if mime == "image/svg+xml" {
            rasterize_svg(&bytes, src)?
        } else {
            image::load_from_memory_with_format(
                &bytes,
                match mime {
                    "image/png" => image::ImageFormat::Png,
                    "image/jpeg" => image::ImageFormat::Jpeg,
                    _ => image::ImageFormat::WebP,
                },
            )
            .map_err(|error| RasterError::ImageAsset {
                path: src.to_string(),
                reason: error.to_string(),
            })?
            .to_rgba8()
        };
        let decoded = Arc::new(decoded);
        let mut cache = self.decoded.lock().expect("image cache lock poisoned");
        Ok(cache.insert(src.to_string(), decoded, self.max_bytes))
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.decoded
            .lock()
            .expect("image cache lock poisoned")
            .images
            .len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_remote_sources_before_io() {
        let cache = ImageCache::default();
        let error = cache.load("https://example.com/image.png").unwrap_err();
        assert!(error.to_string().contains("only local paths"));
    }

    #[test]
    fn decodes_and_caches_png_data_uri() {
        let cache = ImageCache::default();
        let source = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
        let image = cache.load(source).unwrap();
        assert_eq!((image.width(), image.height()), (1, 1));
        assert_eq!(cache.len(), 1);
        let cached = cache.load(source).unwrap();
        assert!(std::sync::Arc::ptr_eq(&image, &cached));
    }

    #[test]
    fn cache_reuses_decoded_pixels_until_lru_eviction() {
        let source_a = "data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%221%22%20height%3D%221%22%3E%3Crect%20width%3D%221%22%20height%3D%221%22%20fill%3D%22%23f00%22%2F%3E%3C%2Fsvg%3E";
        let source_b = "data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%221%22%20height%3D%221%22%3E%3Crect%20width%3D%221%22%20height%3D%221%22%20fill%3D%22%2300f%22%2F%3E%3C%2Fsvg%3E";
        let cache = ImageCache::with_max_bytes(4);

        let first = cache.load(source_a).unwrap();
        let second = cache.load(source_a).unwrap();
        assert!(std::sync::Arc::ptr_eq(&first, &second));
        assert_eq!(cache.len(), 1);

        let _ = cache.load(source_b).unwrap();
        assert_eq!(
            cache.len(),
            1,
            "the byte budget must evict the least-recently-used image"
        );
        let reloaded = cache.load(source_a).unwrap();
        assert!(!std::sync::Arc::ptr_eq(&first, &reloaded));
    }

    #[test]
    fn rasterizes_base64_svg_data_uri() {
        let cache = ImageCache::default();
        let source = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIyIiBoZWlnaHQ9IjEiPjxyZWN0IHdpZHRoPSIyIiBoZWlnaHQ9IjEiIGZpbGw9InJlZCIvPjwvc3ZnPg==";
        let image = cache.load(source).unwrap();
        assert_eq!((image.width(), image.height()), (2, 1));
        assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn rasterizes_percent_encoded_svg_data_uri() {
        let cache = ImageCache::default();
        let source = "data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%221%22%20height%3D%221%22%3E%3Crect%20width%3D%221%22%20height%3D%221%22%20fill%3D%22%2300ff00%22%2F%3E%3C%2Fsvg%3E";
        let image = cache.load(source).unwrap();
        assert_eq!((image.width(), image.height()), (1, 1));
        assert_eq!(image.get_pixel(0, 0).0, [0, 255, 0, 255]);
    }
}
