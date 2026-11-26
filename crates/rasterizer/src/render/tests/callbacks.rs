//! Progress, encoding-progress and diagnostics callbacks.

use super::*;

#[test]
fn diagnostics_callback_survives_control_clone() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&received);
    let control = RenderControl::new().with_diagnostics(move |diagnostics| {
        captured.lock().unwrap().push(diagnostics);
    });
    let cloned = control.clone();
    cloned.report_diagnostics(RenderDiagnostics {
        backend: "gpu",
        gpu_frames: 9,
        cpu_fallback_frames: 1,
        fallback_reason: Some("unsupported node".into()),
        elapsed_ms: 12.5,
        encoded_frames: 10,
        output_width: 1280,
        output_height: 720,
        fps: 30.0,
        texture_cache_hits: 2,
        texture_cache_misses: 1,
        video_decode_ms: 0.5,
        texture_upload_ms: 0.25,
        gpu_submit_readback_ms: 1.0,
        browser_frame_ms: 0.0,
    });
    assert_eq!(
        received.lock().unwrap().as_slice(),
        &[RenderDiagnostics {
            backend: "gpu",
            gpu_frames: 9,
            cpu_fallback_frames: 1,
            fallback_reason: Some("unsupported node".into()),
            elapsed_ms: 12.5,
            encoded_frames: 10,
            output_width: 1280,
            output_height: 720,
            fps: 30.0,
            texture_cache_hits: 2,
            texture_cache_misses: 1,
            video_decode_ms: 0.5,
            texture_upload_ms: 0.25,
            gpu_submit_readback_ms: 1.0,
            browser_frame_ms: 0.0,
        }]
    );
}

#[test]
fn progress_callbacks_compose_instead_of_replacing_engine_output() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let first = Arc::clone(&calls);
    let second = Arc::clone(&calls);
    let control = RenderControl::new()
        .with_progress(move |_| first.lock().unwrap().push("first"))
        .with_progress(move |_| second.lock().unwrap().push("second"));

    if let Some(callback) = &control.progress {
        callback(RenderProgress {
            completed_frames: 1,
            total_frames: 1,
            frame: 0,
        });
    }

    assert_eq!(*calls.lock().unwrap(), vec!["first", "second"]);
}

#[test]
fn encoding_progress_callbacks_compose() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let first = Arc::clone(&calls);
    let second = Arc::clone(&calls);
    let control = RenderControl::new()
        .with_encoding_progress(move |progress| {
            first
                .lock()
                .unwrap()
                .push(("first", progress.encoded_frames));
        })
        .with_encoding_progress(move |progress| {
            second
                .lock()
                .unwrap()
                .push(("second", progress.encoded_frames));
        });

    control.report_encoding_progress(EncodingProgress {
        encoded_frames: 2,
        total_frames: 3,
    });

    assert_eq!(*calls.lock().unwrap(), vec![("first", 2), ("second", 2)]);
}

#[test]
fn diagnostics_callbacks_compose() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let first = Arc::clone(&calls);
    let second = Arc::clone(&calls);
    let control = RenderControl::new()
        .with_diagnostics(move |diagnostics| {
            first
                .lock()
                .unwrap()
                .push(("first", diagnostics.encoded_frames));
        })
        .with_diagnostics(move |diagnostics| {
            second
                .lock()
                .unwrap()
                .push(("second", diagnostics.encoded_frames));
        });

    control.report_diagnostics(RenderDiagnostics {
        backend: "test",
        gpu_frames: 1,
        cpu_fallback_frames: 0,
        fallback_reason: None,
        elapsed_ms: 2.0,
        encoded_frames: 2,
        output_width: 16,
        output_height: 16,
        fps: 30.0,
        texture_cache_hits: 0,
        texture_cache_misses: 0,
        video_decode_ms: 0.0,
        texture_upload_ms: 0.0,
        gpu_submit_readback_ms: 0.0,
        browser_frame_ms: 0.0,
    });

    assert_eq!(*calls.lock().unwrap(), vec![("first", 2), ("second", 2)]);
}
