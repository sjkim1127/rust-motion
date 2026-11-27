//! Ordered streaming, windowing, cancellation and frame-range behaviour of the pipe mode.

use super::*;

#[test]
fn browser_runtime_pipe_uses_sequential_frames_and_reports_both_progresses() {
    if !ffmpeg_available() {
        return;
    }
    let temp = unique_temp_dir("browser_runtime_pipe");
    std::fs::create_dir_all(&temp).unwrap();
    let output = temp.join("browser.mp4");
    let render_progress = Arc::new(Mutex::new(Vec::new()));
    let encode_progress = Arc::new(Mutex::new(Vec::new()));
    let render_capture = Arc::clone(&render_progress);
    let encode_capture = Arc::clone(&encode_progress);
    let control = RenderControl::new()
        .with_progress(move |progress| {
            render_capture
                .lock()
                .unwrap()
                .push(progress.completed_frames);
        })
        .with_encoding_progress(move |progress| {
            encode_capture.lock().unwrap().push(progress.encoded_frames);
        });
    let config = PipeConfig::new(16, 16, 30.0, 3, &output)
        .with_codec(VideoCodec::H264)
        .with_hw_accel(HwAccel::Disabled)
        .with_control(control);
    render_to_ffmpeg_pipe(&BrowserLikeBackend, &config, |frame| {
        solid_scene(Color::rgb((frame * 40) as u8, 20, 200))(frame)
    })
    .unwrap();
    assert_eq!(*render_progress.lock().unwrap(), vec![1, 2, 3]);
    assert_eq!(*encode_progress.lock().unwrap(), vec![1, 2, 3]);
    assert!(output.is_file());
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn streaming_overlaps_the_next_render_without_exceeding_the_window() {
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(3)
        .build()
        .unwrap();
    let active = AtomicUsize::new(0);
    let peak = AtomicUsize::new(0);
    let (first_ready, wait_first) = mpsc::channel();
    let wait_first = Mutex::new(wait_first);
    let (next_ready, wait_next) = mpsc::channel();
    let mut output = Vec::new();
    stream_ordered_frames(
        &pool,
        12,
        3,
        |frame| {
            let count = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(count, Ordering::SeqCst);
            if frame == 0 {
                // Force out-of-order rendering; frame one must already be ready.
                wait_first
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            } else if frame == 1 {
                first_ready.send(()).unwrap();
            } else if frame == 3 {
                next_ready.send(()).unwrap();
            }
            Ok(vec![frame as u8; 4])
        },
        |frame, bytes| {
            if frame == 1 {
                // A batch barrier would prevent frame 3 starting here.
                wait_next.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            output.extend_from_slice(&bytes);
            drop(bytes);
            active.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        output,
        (0..12u8).flat_map(|frame| [frame; 4]).collect::<Vec<_>>()
    );
    assert!(peak.load(Ordering::SeqCst) <= 3);
    assert_eq!(active.load(Ordering::SeqCst), 0);
}

#[test]
fn streaming_stops_scheduling_after_render_or_writer_error() {
    use std::sync::atomic::AtomicUsize;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    for fail_render in [false, true] {
        let rendered = AtomicUsize::new(0);
        let mut consumed = 0;
        let result = stream_ordered_frames(
            &pool,
            100,
            2,
            |frame| {
                rendered.fetch_add(1, Ordering::SeqCst);
                if fail_render && frame == 0 {
                    Err(RasterError::Scene("failed frame".into()))
                } else {
                    Ok(vec![frame as u8])
                }
            },
            |_, _| {
                consumed += 1;
                Err(RasterError::ImageEncode("failed pipe".into()))
            },
        );
        assert!(result.is_err());
        assert!(rendered.load(Ordering::SeqCst) <= 2);
        assert_eq!(consumed, if fail_render { 0 } else { 1 });
    }
}

#[test]
fn streaming_honors_cancellation_between_ordered_writes() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(3)
        .build()
        .unwrap();
    let control = RenderControl::new();
    let started = Instant::now();
    let mut consumed = Vec::new();
    let result = stream_ordered_frames(
        &pool,
        100,
        3,
        |frame| {
            control.check(started)?;
            Ok(vec![frame as u8])
        },
        |frame, _| {
            control.check(started)?;
            consumed.push(frame);
            if frame == 2 {
                control.cancellation_token().cancel();
            }
            Ok(())
        },
    );
    assert!(matches!(result, Err(RasterError::Cancelled)));
    assert_eq!(consumed, vec![0, 1, 2]);
}

#[test]
fn streaming_single_worker_and_short_final_window() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    for window in [1, 2, 8] {
        let mut frames = Vec::new();
        stream_ordered_frames(
            &pool,
            3,
            window,
            |frame| Ok(vec![frame as u8]),
            |_, bytes| {
                frames.extend(bytes);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(frames, vec![0, 1, 2]);
    }
}

#[test]
fn streaming_worker_panic_reaches_caller_instead_of_waiting_forever() {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            stream_ordered_frames(&pool, 10, 2, |_| panic!("test render panic"), |_, _| Ok(()))
        }));
        sender.send(result.is_err()).unwrap();
    });
    assert!(receiver.recv_timeout(Duration::from_secs(5)).unwrap());
}

#[test]
fn streaming_progress_reports_the_sampled_source_frame() {
    let frames = Arc::new(Mutex::new(Vec::new()));
    let captured_frames = Arc::clone(&frames);
    let control = RenderControl::new().with_progress(move |progress| {
        captured_frames.lock().unwrap().push(progress.frame);
    });
    let mut output = Vec::new();
    let mut sink = FfmpegFrameSink {
        control: &control,
        started: Instant::now(),
        input_width: 1,
        input_height: 1,
        output_width: 1,
        output_height: 1,
        total: 2,
        start_frame: 10,
        frame_step: 3,
        stdin: &mut output,
    };

    sink.consume(1, &[0, 0, 0, 255]).unwrap();

    assert_eq!(*frames.lock().unwrap(), vec![13]);
    assert_eq!(output, vec![0, 0, 0, 255]);
}

#[test]
fn test_frame_range_streams_absolute_frames_in_order() {
    if !ffmpeg_available() {
        eprintln!("Skipping frame-range pipe test: FFmpeg is unavailable");
        return;
    }
    let temp = unique_temp_dir("frame_range");
    std::fs::create_dir_all(&temp).unwrap();
    let output = temp.join("range.mp4");
    let rendered_frames = Arc::new(Mutex::new(Vec::new()));
    let rendered_capture = Arc::clone(&rendered_frames);
    let progress = Arc::new(Mutex::new(Vec::new()));
    let progress_capture = Arc::clone(&progress);
    let control = RenderControl::new().with_progress(move |event| {
        progress_capture.lock().unwrap().push(event);
    });
    let config = PipeConfig::new(32, 24, 30.0, 3, &output)
        .with_frame_start(10)
        .with_frame_step(2)
        .with_concurrency(2)
        .with_control(control);

    render_to_ffmpeg_pipe(&TinySkiaBackend::headless(), &config, move |frame| {
        rendered_capture.lock().unwrap().push(frame);
        solid_scene(Color::rgb(frame as u8, 20, 30))(frame)
    })
    .unwrap();

    let mut actual_frames = rendered_frames.lock().unwrap().clone();
    actual_frames.sort_unstable();
    assert_eq!(actual_frames, vec![10, 12, 14]);
    assert_eq!(
        progress.lock().unwrap().as_slice(),
        &[
            RenderProgress {
                completed_frames: 1,
                total_frames: 3,
                frame: 10,
            },
            RenderProgress {
                completed_frames: 2,
                total_frames: 3,
                frame: 12,
            },
            RenderProgress {
                completed_frames: 3,
                total_frames: 3,
                frame: 14,
            },
        ]
    );
    assert!(std::fs::metadata(&output).unwrap().len() > 0);
    std::fs::remove_dir_all(temp).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn native_videotoolbox_renders_h264_and_hevc_without_ffmpeg() {
    let temp = unique_temp_dir("vt_native_render");
    std::fs::create_dir_all(&temp).unwrap();

    for (name, codec, hw_accel, marker) in [
        (
            "vt_h264.mp4",
            VideoCodec::H264,
            HwAccel::VideoToolbox,
            b"avc1",
        ),
        ("vt_hevc.mp4", VideoCodec::H265, HwAccel::Auto, b"hvc1"),
    ] {
        let out = temp.join(name);
        let config = PipeConfig::new(128, 96, 24.0, 6, &out)
            .with_codec(codec)
            .with_hw_accel(hw_accel)
            .with_encoder_backend(EncoderBackend::Native);
        render_to_video(&TinySkiaBackend::headless(), &config, |frame| {
            solid_scene(Color::rgb((frame * 35) as u8, 80, 180))(frame)
        })
        .unwrap();

        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(&bytes[4..8], b"ftyp");
        assert!(bytes.windows(4).any(|w| w == marker));
        let demux = rust_motion_media_container::Mp4Demuxer::open_path(&out).unwrap();
        assert_eq!(demux.video_tracks()[0].samples.len(), 6);
    }

    std::fs::remove_dir_all(temp).unwrap();
}
