//! Integration tests for FrameCacheManager, LRU eviction, thread safety, and metrics.

use image::{Rgba, RgbaImage};
use rust_motion_rasterizer::backend::RasterError;
use rust_motion_rasterizer::frame_cache::{FrameCacheConfig, FrameCacheKey, FrameCacheManager};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

fn make_rgba_frame(width: u32, height: u32, fill: [u8; 4]) -> Arc<RgbaImage> {
    let mut img = RgbaImage::new(width, height);
    for p in img.pixels_mut() {
        *p = Rgba(fill);
    }
    Arc::new(img)
}

#[test]
fn test_strict_memory_bounded_eviction() {
    // 512 KB per frame (width=256, height=512, 4 bytes/px = 524,288 bytes)
    let frame_bytes = 256 * 512 * 4;
    // Capacity for exactly 4 frames = 2,097,152 bytes (2 MB)
    let max_bytes = frame_bytes * 4;
    let cache = FrameCacheManager::new(FrameCacheConfig::with_max_bytes(max_bytes));

    for frame_idx in 0..10 {
        let key = FrameCacheKey::new("comp_main", frame_idx, 256, 512, 0);
        let frame = make_rgba_frame(256, 512, [frame_idx as u8, 0, 0, 255]);
        cache.insert(key, frame);

        assert!(
            cache.current_bytes() <= max_bytes,
            "Cache bytes {} exceeded max bytes {}",
            cache.current_bytes(),
            max_bytes
        );
    }

    let metrics = cache.metrics();
    assert_eq!(metrics.entry_count, 4);
    assert_eq!(metrics.current_bytes, max_bytes);
    assert_eq!(metrics.evictions, 6);

    // Frames 0..6 should be evicted, 6..10 (i.e. 6, 7, 8, 9) should remain
    for frame_idx in 0..6 {
        let key = FrameCacheKey::new("comp_main", frame_idx, 256, 512, 0);
        assert!(
            !cache.contains(&key),
            "Frame {} should have been evicted",
            frame_idx
        );
    }
    for frame_idx in 6..10 {
        let key = FrameCacheKey::new("comp_main", frame_idx, 256, 512, 0);
        assert!(cache.contains(&key), "Frame {} should be cached", frame_idx);
        let img = cache.get(&key).expect("Frame should be present");
        assert_eq!(img.get_pixel(0, 0).0[0], frame_idx as u8);
    }
}

#[test]
fn test_lru_access_promotion_preserves_hot_frames() {
    // Capacity for 3 frames (each 100x100 = 40,000 bytes; 3 * 40,000 = 120,000 bytes)
    let cache = FrameCacheManager::new(FrameCacheConfig::with_max_bytes(120_000));

    let key_0 = FrameCacheKey::new("c", 0, 100, 100, 0);
    let key_1 = FrameCacheKey::new("c", 1, 100, 100, 0);
    let key_2 = FrameCacheKey::new("c", 2, 100, 100, 0);
    let key_3 = FrameCacheKey::new("c", 3, 100, 100, 0);

    cache.insert(key_0.clone(), make_rgba_frame(100, 100, [0, 0, 0, 255]));
    cache.insert(key_1.clone(), make_rgba_frame(100, 100, [1, 0, 0, 255]));
    cache.insert(key_2.clone(), make_rgba_frame(100, 100, [2, 0, 0, 255]));

    // Promote key_0 and key_1 by accessing them
    assert!(cache.get(&key_0).is_some());
    assert!(cache.get(&key_1).is_some());

    // Insert key_3. Since key_2 was least recently accessed, it must be evicted!
    cache.insert(key_3.clone(), make_rgba_frame(100, 100, [3, 0, 0, 255]));

    assert!(
        cache.contains(&key_0),
        "key_0 was promoted, should remain in cache"
    );
    assert!(
        cache.contains(&key_1),
        "key_1 was promoted, should remain in cache"
    );
    assert!(
        !cache.contains(&key_2),
        "key_2 was LRU, should have been evicted"
    );
    assert!(
        cache.contains(&key_3),
        "key_3 was just inserted, should be in cache"
    );
    assert_eq!(cache.metrics().evictions, 1);
}

#[test]
fn test_timeline_scrubbing_simulation_hit_ratio() {
    // 20 frames budget: 20 * 40,000 = 800,000 bytes
    let cache = FrameCacheManager::new(FrameCacheConfig::with_max_bytes(800_000));
    let frame_count = 20;

    // Pass 1: Cold render / scrub forward
    for f in 0..frame_count {
        let key = FrameCacheKey::new("timeline", f, 100, 100, 0);
        let res = cache.get_or_render(key, || {
            Ok((*make_rgba_frame(100, 100, [f as u8, 0, 0, 255])).clone())
        });
        assert!(res.is_ok());
    }

    let m1 = cache.metrics();
    assert_eq!(m1.misses, frame_count);
    assert_eq!(m1.hits, 0);
    assert_eq!(m1.entry_count, frame_count as usize);

    // Pass 2: Scrub backward across cached frames (should be 100% hits)
    for f in (0..frame_count).rev() {
        let key = FrameCacheKey::new("timeline", f, 100, 100, 0);
        let frame = cache.get(&key).expect("Frame must be cached");
        assert_eq!(frame.get_pixel(0, 0).0[0], f as u8);
    }

    let m2 = cache.metrics();
    assert_eq!(m2.hits, frame_count);
    assert_eq!(m2.misses, frame_count);
    assert_eq!(m2.hit_ratio(), 0.5);

    // Pass 3: Scrub forward again
    for f in 0..frame_count {
        let key = FrameCacheKey::new("timeline", f, 100, 100, 0);
        let frame = cache.get(&key).expect("Frame must be cached");
        assert_eq!(frame.get_pixel(0, 0).0[0], f as u8);
    }

    let m3 = cache.metrics();
    assert_eq!(m3.hits, frame_count * 2);
    assert_eq!(m3.misses, frame_count);
    // Total 60 requests: 40 hits / 60 = 0.6666...
    assert!((m3.hit_ratio() - (2.0 / 3.0)).abs() < 1e-5);
}

#[test]
fn test_concurrent_multithreaded_stress() {
    // 50 frames budget (each 50x50 = 10,000 bytes; 50 * 10,000 = 500,000 bytes)
    let max_bytes = 500_000;
    let cache = Arc::new(FrameCacheManager::new(FrameCacheConfig::with_max_bytes(
        max_bytes,
    )));
    let running = Arc::new(AtomicBool::new(true));

    let thread_count = 16;
    let iterations_per_thread = 500;
    let mut handles = Vec::new();

    for thread_id in 0..thread_count {
        let cache_clone = Arc::clone(&cache);
        let running_clone = Arc::clone(&running);

        let handle = thread::spawn(move || {
            for i in 0..iterations_per_thread {
                let frame_idx = (thread_id * 10 + (i % 30)) as u64;
                let key = FrameCacheKey::new("stress_comp", frame_idx, 50, 50, 0);

                if i % 3 == 0 {
                    // get_or_render path
                    let _ = cache_clone.get_or_render(key, || {
                        Ok(
                            (*make_rgba_frame(50, 50, [(frame_idx % 256) as u8, 0, 0, 255]))
                                .clone(),
                        )
                    });
                } else if i % 3 == 1 {
                    // insert path
                    let frame = make_rgba_frame(50, 50, [(frame_idx % 256) as u8, 0, 0, 255]);
                    cache_clone.insert(key, frame);
                } else {
                    // get path
                    let _ = cache_clone.get(&key);
                }

                // Periodic check of metrics consistency
                if i % 50 == 0 {
                    let m = cache_clone.metrics();
                    assert!(m.current_bytes <= max_bytes);
                }
            }
            running_clone.load(Ordering::Relaxed);
        });

        handles.push(handle);
    }

    for h in handles {
        h.join().expect("Worker thread panicked during stress test");
    }

    let final_metrics = cache.metrics();
    assert!(
        final_metrics.current_bytes <= max_bytes,
        "Final bytes {} exceeded max bytes {}",
        final_metrics.current_bytes,
        max_bytes
    );
    assert_eq!(
        final_metrics.entry_count * 10_000,
        final_metrics.current_bytes,
        "Entry count does not match current_bytes"
    );
    assert!(
        final_metrics.hits > 0,
        "Stress test should have recorded cache hits"
    );
    assert!(
        final_metrics.misses > 0,
        "Stress test should have recorded cache misses"
    );
    assert!(
        final_metrics.evictions > 0,
        "Stress test should have triggered evictions"
    );
}

#[test]
fn test_get_or_render_error_propagation() {
    let cache = FrameCacheManager::new(FrameCacheConfig::with_megabytes(10));
    let key = FrameCacheKey::new("error_comp", 1, 10, 10, 0);

    let result = cache.get_or_render(key.clone(), || {
        Err(RasterError::Frame {
            frame: 1,
            reason: "Simulated rasterizer failure".into(),
        })
    });

    assert!(result.is_err());
    match result.unwrap_err() {
        RasterError::Frame { frame, reason } => {
            assert_eq!(frame, 1);
            assert_eq!(reason, "Simulated rasterizer failure");
        }
        other => panic!("Unexpected error variant: {:?}", other),
    }

    // Key should NOT be in cache
    assert!(!cache.contains(&key));
    assert_eq!(cache.entry_count(), 0);
    assert_eq!(cache.current_bytes(), 0);
}

#[test]
fn test_json_props_hash_uniqueness_and_stability() {
    let p1 = serde_json::json!({ "title": "Intro", "font_size": 32, "visible": true });
    let p2 = serde_json::json!({ "title": "Intro", "font_size": 32, "visible": false });
    let p3 = serde_json::json!({ "title": "Intro", "font_size": 32, "visible": true });

    let k1 = FrameCacheKey::from_props("comp", 10, 1920, 1080, &p1);
    let k2 = FrameCacheKey::from_props("comp", 10, 1920, 1080, &p2);
    let k3 = FrameCacheKey::from_props("comp", 10, 1920, 1080, &p3);

    assert_eq!(k1, k3);
    assert_ne!(k1, k2);
    assert_eq!(k1.props_hash, k3.props_hash);
    assert_ne!(k1.props_hash, k2.props_hash);
}
