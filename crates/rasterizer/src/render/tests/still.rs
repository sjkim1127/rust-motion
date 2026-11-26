//! Still image rendering, formats and scaling.

use super::*;

#[test]
fn test_still_formats_write_decodable_images_and_report_progress() {
    let backend = TinySkiaBackend::headless();
    let temp = unique_temp_dir("stills");
    std::fs::create_dir_all(&temp).unwrap();

    for (format, extension) in [
        (StillImageFormat::Png, "png"),
        (StillImageFormat::Jpeg, "jpg"),
        (StillImageFormat::WebP, "webp"),
    ] {
        let output = temp.join(format!("frame.{extension}"));
        let progress = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&progress);
        let control = RenderControl::new().with_progress(move |event| {
            captured.lock().unwrap().push(event);
        });
        render_still_fallible(
            &backend,
            32,
            24,
            30.0,
            17,
            &output,
            format,
            &control,
            |frame| {
                assert_eq!(frame, 17);
                Ok::<_, std::convert::Infallible>(solid_scene(Color::rgb(12, 34, 56))(frame))
            },
        )
        .unwrap();

        let decoded = image::open(&output).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (32, 24));
        assert_eq!(
            *progress.lock().unwrap(),
            vec![RenderProgress {
                completed_frames: 1,
                total_frames: 1,
                frame: 17,
            }]
        );
    }
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn scaled_still_writes_scaled_dimensions() {
    let backend = TinySkiaBackend::headless();
    let temp = unique_temp_dir("scaled_still");
    std::fs::create_dir_all(&temp).unwrap();
    let output = temp.join("frame.png");
    render_still_fallible_scaled(
        &backend,
        32,
        24,
        30.0,
        0,
        &output,
        StillImageFormat::Png,
        &RenderControl::new(),
        1.5,
        |frame| Ok::<_, std::convert::Infallible>(solid_scene(Color::rgb(255, 0, 0))(frame)),
    )
    .unwrap();
    let decoded = image::open(&output).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (48, 36));
    std::fs::remove_dir_all(temp).unwrap();
}
