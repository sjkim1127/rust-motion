//! Ordered-frame streaming, pipe-config validation, frame scaling and FFmpeg process waiting.

use super::*;

/// Render at most `window` frames ahead, consuming them in timeline order.
/// A new task is issued only after the consumer releases an earlier frame.
/// Thus rendering can overlap pipe writes without retaining extra frame batches.
pub(super) fn stream_ordered_frames<F, C>(
    pool: &rayon::ThreadPool,
    total: u32,
    window: usize,
    render: F,
    mut consume: C,
) -> Result<(), RasterError>
where
    F: Fn(u32) -> Result<Vec<u8>, RasterError> + Sync,
    C: FnMut(u32, Vec<u8>) -> Result<(), RasterError>,
{
    if window == 0 {
        return Err(RasterError::Init(
            "Render concurrency must be greater than zero".into(),
        ));
    }
    let stopped = AtomicBool::new(false);
    // Signal queued tasks on errors and unwinding, before the scope joins them.
    struct StopOnDrop<'a>(&'a AtomicBool);
    impl Drop for StopOnDrop<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    pool.in_place_scope(|scope| {
        let _stop = StopOnDrop(&stopped);
        let (sender, receiver) = std::sync::mpsc::channel();
        let render = &render;
        let stopped = &stopped;
        let submit = |frame| {
            let sender = sender.clone();
            scope.spawn(move |_| {
                if stopped.load(Ordering::Relaxed) {
                    return;
                }
                // Forward panics too, so a missing frame cannot deadlock the
                // ordered consumer. The caller retains normal panic semantics.
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render(frame)));
                let _ = sender.send((frame, result));
            });
        };
        let mut submitted = total.min(u32::try_from(window).unwrap_or(u32::MAX));
        for frame in 0..submitted {
            submit(frame);
        }
        let mut next = 0;
        let mut pending = std::collections::BTreeMap::new();
        while next < total {
            let (frame, result) = receiver.recv().map_err(|_| {
                RasterError::Init(
                    "Render workers stopped before completing the frame window".into(),
                )
            })?;
            let rgba = match result {
                Ok(result) => result?,
                Err(panic) => std::panic::resume_unwind(panic),
            };
            pending.insert(frame, rgba);
            while let Some(rgba) = pending.remove(&next) {
                consume(next, rgba)?;
                next += 1;
                if submitted < total {
                    submit(submitted);
                    submitted += 1;
                }
            }
        }
        Ok(())
    })
}

pub(super) fn validate_pipe_config(config: &PipeConfig) -> Result<(), RasterError> {
    let (output_width, output_height) = config.output_dimensions()?;
    if config.width == 0 || config.height == 0 {
        return Err(RasterError::Init(
            "render width and height must be positive".into(),
        ));
    }
    if !config.fps.is_finite() || config.fps <= 0.0 {
        return Err(RasterError::Init(
            "render FPS must be finite and positive".into(),
        ));
    }
    if config.duration_in_frames == 0 {
        return Err(RasterError::Init(
            "render duration must contain at least one frame".into(),
        ));
    }
    if config.frame_step == 0 {
        return Err(RasterError::Init(
            "render frame step must be greater than zero".into(),
        ));
    }
    config
        .start_frame
        .checked_add((config.duration_in_frames - 1).saturating_mul(config.frame_step))
        .ok_or_else(|| RasterError::Init("render frame range overflows u32".into()))?;
    if config.codec != VideoCodec::Gif
        && (!output_width.is_multiple_of(2) || !output_height.is_multiple_of(2))
    {
        return Err(RasterError::Init(
            "video render width and height must be even".into(),
        ));
    }
    let max_crf = match config.codec {
        VideoCodec::H264 | VideoCodec::H265 => Some(51),
        VideoCodec::Vp9 | VideoCodec::Av1 => Some(63),
        VideoCodec::ProRes | VideoCodec::Gif => None,
    };
    if max_crf.is_some_and(|maximum| config.crf > maximum) {
        return Err(RasterError::Init(format!(
            "CRF {} exceeds the {:?} maximum of {}",
            config.crf,
            config.codec,
            max_crf.expect("checked above")
        )));
    }
    if config.codec == VideoCodec::Gif && !config.audio_tracks.is_empty() {
        return Err(RasterError::Scene(
            "GIF output does not support audio tracks".into(),
        ));
    }
    validate_audio_tracks(&config.audio_tracks, &config.security_policy)
}

pub(super) fn scale_rgba_frame(
    rgba: &[u8],
    width: u32,
    height: u32,
    output_width: u32,
    output_height: u32,
) -> Result<Vec<u8>, RasterError> {
    if width == output_width && height == output_height {
        return Ok(rgba.to_vec());
    }
    let image = image::RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or_else(|| {
        RasterError::ImageEncode("rendered RGBA buffer has an invalid length".into())
    })?;
    Ok(image::imageops::resize(
        &image,
        output_width,
        output_height,
        image::imageops::FilterType::Lanczos3,
    )
    .into_raw())
}

pub(super) fn wait_for_ffmpeg(
    mut child: std::process::Child,
    control: &RenderControl,
    started: Instant,
) -> Result<std::process::Output, RasterError> {
    tracing::debug!("FFmpeg wait loop started");
    loop {
        if let Err(error) = control.check(started) {
            tracing::debug!(error = %error, "FFmpeg wait interrupted by render control");
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                tracing::debug!(?status, "FFmpeg process exited");
                let mut stderr = Vec::new();
                if let Some(mut pipe) = child.stderr.take() {
                    pipe.read_to_end(&mut stderr).map_err(|error| {
                        RasterError::ImageEncode(format!("FFmpeg stderr read error: {error}"))
                    })?;
                }
                return Ok(std::process::Output {
                    status,
                    stdout: Vec::new(),
                    stderr,
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RasterError::ImageEncode(format!(
                    "FFmpeg wait error: {error}"
                )));
            }
        }
    }
}
