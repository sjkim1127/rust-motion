//! PNG-sequence render modes (sequential and Rayon-parallel) and single-frame helpers.

use super::*;

// ── Mode 1: Sequential PNG ───────────────────────────────────────────────────

/// Render all frames sequentially to PNG files.
///
/// Each frame is rendered in order and saved as `frame_000001.png`, etc.
/// Simple and debuggable, but slow for large frame counts.
pub fn render_all_frames<F>(
    backend: &dyn RasterizerBackend,
    config: &NativeRenderConfig,
    mut scene_fn: F,
) -> Result<Vec<PathBuf>, RasterError>
where
    F: FnMut(u32) -> Scene,
{
    std::fs::create_dir_all(&config.output_dir)?;

    let mut paths = Vec::with_capacity(config.duration_in_frames as usize);

    for frame in 0..config.duration_in_frames {
        let scene = scene_fn(frame);
        let frame_config = FrameConfig::new(config.width, config.height, frame, config.fps);
        let img = backend.render_frame(&scene, &frame_config)?;

        let path = config
            .output_dir
            .join(format!("frame_{:06}.png", frame + 1));
        img.save(&path)
            .map_err(|e| RasterError::ImageEncode(e.to_string()))?;
        paths.push(path);
    }

    Ok(paths)
}

// ── Mode 2: Parallel PNG ─────────────────────────────────────────────────────

/// Render all frames in parallel using Rayon, then write PNGs.
///
/// Frames are rendered concurrently across all available CPU cores.
/// Requires `backend` to implement `Send + Sync`.
///
/// # Performance
/// On a machine with N cores, this is roughly N× faster than sequential
/// for the rasterization step. Each worker writes its indexed PNG immediately,
/// so peak image memory is bounded by the worker count rather than frame count.
pub fn render_parallel<F, B>(
    backend: &B,
    config: &NativeRenderConfig,
    scene_fn: F,
) -> Result<Vec<PathBuf>, RasterError>
where
    B: RasterizerBackend + Send + Sync,
    F: Fn(u32) -> Scene + Send + Sync,
{
    std::fs::create_dir_all(&config.output_dir)?;

    let width = config.width;
    let height = config.height;
    let fps = config.fps;
    let total = config.duration_in_frames;
    let dir = &config.output_dir;

    // Configure Rayon thread pool if explicit concurrency was requested
    let pool = match config.concurrency {
        Some(n) => rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build()
            .map_err(|e| RasterError::Init(format!("Failed to build thread pool: {e}")))?,
        None => rayon::ThreadPoolBuilder::new()
            .build()
            .map_err(|e| RasterError::Init(format!("Failed to build thread pool: {e}")))?,
    };

    pool.install(|| {
        (0..total).into_par_iter().try_for_each(|frame| {
            let scene = scene_fn(frame);
            let frame_cfg = FrameConfig::new(width, height, frame, fps);
            let img = backend.render_frame(&scene, &frame_cfg)?;
            let path = dir.join(format!("frame_{:06}.png", frame + 1));
            img.save(&path)
                .map_err(|e| RasterError::ImageEncode(e.to_string()))
        })
    })?;

    let paths = (0..total)
        .map(|frame| dir.join(format!("frame_{:06}.png", frame + 1)))
        .collect();

    Ok(paths)
}

/// Save a single `RgbaImage` frame to disk.
pub fn save_frame(img: &RgbaImage, path: &Path) -> Result<(), RasterError> {
    img.save(path)
        .map_err(|e| RasterError::ImageEncode(e.to_string()))
}

// ── Benchmark helper ─────────────────────────────────────────────────────────

/// Render a single frame and return elapsed time (for benchmarking).
pub fn render_frame_timed(
    backend: &dyn RasterizerBackend,
    scene: &Scene,
    config: &FrameConfig,
) -> Result<(RgbaImage, std::time::Duration), RasterError> {
    let start = std::time::Instant::now();
    let img = backend.render_frame(scene, config)?;
    Ok((img, start.elapsed()))
}
