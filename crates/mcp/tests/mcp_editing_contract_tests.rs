//! Regression tests for the agent-facing editing contract (#152-#156).

use rust_motion_mcp::session::McpSession;
use rust_motion_mcp::McpServer;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

/// Call a tool over JSON-RPC and return `(is_error, first text block, full result)`.
fn call(server: &mut McpServer, name: &str, arguments: Value) -> (bool, String, Value) {
    let message = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": name, "arguments": arguments }
    });
    let response = server
        .handle_message(&message.to_string())
        .expect("response");
    let response: Value = serde_json::from_str(&response).expect("valid json");
    let result = response["result"].clone();
    let is_error = result["isError"].as_bool().expect("isError");
    let text = result["content"][0]["text"]
        .as_str()
        .expect("text")
        .to_string();
    (is_error, text, result)
}

fn ok(server: &mut McpServer, name: &str, arguments: Value) -> Value {
    let (is_error, text, result) = call(server, name, arguments);
    assert!(!is_error, "{name} failed: {text}");
    if name == "preview_frame" {
        return result;
    }
    serde_json::from_str(&text).expect("json payload")
}

fn fail(server: &mut McpServer, name: &str, arguments: Value) -> String {
    let before = snapshot(server);
    let (is_error, text, _) = call(server, name, arguments.clone());
    assert!(
        is_error,
        "{name} {arguments} unexpectedly succeeded: {text}"
    );
    assert_eq!(snapshot(server), before, "{name} {arguments} changed state");
    text
}

/// Everything an edit may change, in a comparable form.
fn snapshot(server: &McpServer) -> (Value, Value, Value, Value) {
    let session = server.session.lock().expect("lock");
    (
        serde_json::to_value(&session.timeline).expect("timeline"),
        serde_json::to_value(&session.project).expect("project"),
        serde_json::to_value(&session.active_grades).expect("grades"),
        serde_json::to_value(&session.captions).expect("captions"),
    )
}

/// A 30 fps, 10 s project with one video track.
fn project() -> (McpServer, String) {
    let mut server = McpServer::new();
    ok(
        &mut server,
        "project_create",
        json!({ "name": "Edit", "width": 640, "height": 360, "fps": 30.0, "duration_secs": 10.0 }),
    );
    let track = add_track(&mut server, "Main");
    (server, track)
}

/// [`project`] with a media root of its own.
fn media_project() -> (McpServer, String, TempDir) {
    let root = tempfile::tempdir().expect("media root");
    let session = McpSession::with_media_root(root.path());
    let mut server = McpServer::with_session(Arc::new(Mutex::new(session)));
    ok(
        &mut server,
        "project_create",
        json!({ "name": "Edit", "width": 640, "height": 360, "fps": 30.0, "duration_secs": 10.0 }),
    );
    let track = add_track(&mut server, "Main");
    (server, track, root)
}

fn write_image(
    dir: &Path,
    name: &str,
    width: u32,
    height: u32,
    color: impl Fn(u32, u32) -> [u8; 3],
) {
    image::RgbImage::from_fn(width, height, |x, y| image::Rgb(color(x, y)))
        .save(dir.join(name))
        .expect("save image");
}

fn import(server: &mut McpServer, path: &str, kind: &str, asset_id: &str) {
    ok(
        server,
        "asset_import",
        json!({ "path": path, "kind": kind, "asset_id": asset_id }),
    );
}

fn insert_media(server: &mut McpServer, track: &str, asset: &str, start: f64, secs: f64) -> String {
    ok(
        server,
        "clip_insert",
        json!({ "track_id": track, "name": asset, "start_secs": start,
                "duration_secs": secs, "asset_id": asset }),
    )["clip_id"]
        .as_str()
        .expect("clip id")
        .to_string()
}

fn add_track(server: &mut McpServer, name: &str) -> String {
    ok(
        server,
        "timeline_add_track",
        json!({ "track_type": "video", "name": name }),
    )["track_id"]
        .as_str()
        .expect("track id")
        .to_string()
}

fn insert(server: &mut McpServer, track: &str, name: &str, start: u32, frames: u32) -> String {
    ok(
        server,
        "clip_insert",
        json!({ "track_id": track, "name": name,
                "start_secs": f64::from(start) / 30.0, "duration_secs": f64::from(frames) / 30.0 }),
    )["clip_id"]
        .as_str()
        .expect("clip id")
        .to_string()
}

/// `(id, start, end)` of every clip, from the timeline.
fn ranges(server: &McpServer) -> Vec<(String, u32, u32)> {
    let session = server.session.lock().expect("lock");
    session
        .timeline
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .map(|c| (c.id.clone(), c.track_in, c.track_out))
        .collect()
}

/// The exported project must describe the same clips as the timeline and survive a round trip.
fn assert_synchronized(server: &McpServer) {
    let session = server.session.lock().expect("lock");
    let project = session.project.as_ref().expect("project");
    assert_eq!(project.settings.duration, session.timeline.duration_frames);
    assert_eq!(project.tracks.len(), session.timeline.tracks.len());
    for (exported, edited) in project.tracks.iter().zip(&session.timeline.tracks) {
        assert_eq!(exported.id, edited.id);
        assert_eq!(exported.clips.len(), edited.clips.len());
        for (a, b) in exported.clips.iter().zip(&edited.clips) {
            assert_eq!(
                (&a.id, a.start, a.duration),
                (&b.id, b.track_in, b.duration())
            );
            assert_eq!(a.props["source_in"], b.source_in);
            assert_eq!(a.props["source_out"], b.source_out);
        }
    }
    let text = serde_json::to_string(project).expect("serialize");
    let loaded = rust_motion_project::Project::from_json_str(&text).expect("valid project");
    assert_eq!(&loaded, project);
}

// ---------------------------------------------------------------------------------------- #152

#[test]
fn render_composition_reports_that_nothing_was_rendered() {
    let (mut server, _) = project();
    let output = std::env::temp_dir().join(format!("mcp-render-{}.mp4", std::process::id()));
    let message = fail(
        &mut server,
        "render_composition",
        json!({ "output_path": output.to_str().expect("path") }),
    );
    assert!(message.contains("not implemented"), "{message}");
    assert!(!output.exists());

    for (arguments, field) in [
        (json!({ "output_path": "" }), "output_path"),
        (json!({ "output_path": "a.mp4", "crf": 99 }), "crf"),
        (json!({ "output_path": "a.mp4", "codec": 7 }), "codec"),
        (json!({ "output_path": "a.mp4", "fps": 0 }), "fps"),
    ] {
        let message = fail(&mut server, "render_composition", arguments);
        assert!(message.contains(field), "{message}");
    }
}

// ---------------------------------------------------------------------------------------- #153

#[test]
fn split_only_cuts_the_requested_clip() {
    let (mut server, track) = project();
    let a = insert(&mut server, &track, "a", 0, 30);
    let b = insert(&mut server, &track, "b", 40, 30);

    // Frame 50 is inside B, not inside A.
    let message = fail(
        &mut server,
        "clip_split",
        json!({ "clip_id": a, "split_time_secs": 50.0 / 30.0 }),
    );
    assert!(message.contains(&a), "{message}");
    // The clip edges themselves are not strictly inside.
    for frame in [0.0, 30.0] {
        fail(
            &mut server,
            "clip_split",
            json!({ "clip_id": a, "split_time_secs": frame / 30.0 }),
        );
    }

    let split = ok(
        &mut server,
        "clip_split",
        json!({ "clip_id": a, "split_time_secs": 20.0 / 30.0 }),
    );
    let right = split["clip2"].as_str().expect("right id").to_string();
    assert_eq!(
        ranges(&server),
        vec![(a.clone(), 0, 20), (right.clone(), 20, 30), (b, 40, 70)]
    );

    // Splitting the same clip again must not reuse an ID.
    let again = ok(
        &mut server,
        "clip_split",
        json!({ "clip_id": a, "split_time_secs": 10.0 / 30.0 }),
    );
    let newest = again["clip2"].as_str().expect("right id");
    assert_ne!(newest, right);
    let ids: Vec<String> = ranges(&server).into_iter().map(|r| r.0).collect();
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "duplicate clip IDs: {ids:?}");
    assert_synchronized(&server);
}

#[test]
fn clips_inserted_with_the_same_name_and_time_get_distinct_ids() {
    let (mut server, first) = project();
    let second = add_track(&mut server, "Overlay");
    let a = insert(&mut server, &first, "x", 0, 30);
    let b = insert(&mut server, &second, "x", 0, 30);
    let c = insert(&mut server, &first, "x", 0, 30);
    assert!(a != b && a != c && b != c, "{a} {b} {c}");
    assert_synchronized(&server);
}

// ---------------------------------------------------------------------------------------- #154

#[test]
fn project_follows_insert_trim_and_split() {
    let (mut server, main) = project();
    let overlay = add_track(&mut server, "Overlay");
    let a = insert(&mut server, &main, "a", 0, 90);
    let b = insert(&mut server, &overlay, "b", 30, 60);
    assert_synchronized(&server);

    // Trimming the head gives the clip a nonzero source offset.
    ok(
        &mut server,
        "clip_trim",
        json!({ "clip_id": a, "edge_in": true, "new_time_secs": 15.0 / 30.0 }),
    );
    assert_synchronized(&server);
    ok(
        &mut server,
        "color_grade_apply",
        json!({ "clip_id": a, "exposure": 1.0 }),
    );
    let split = ok(
        &mut server,
        "clip_split",
        json!({ "clip_id": a, "split_time_secs": 45.0 / 30.0 }),
    );
    assert_synchronized(&server);
    {
        let session = server.session.lock().expect("lock");
        let project = session.project.as_ref().expect("project");
        let clips = &project.tracks[0].clips;
        assert_eq!(clips[0].props["source_in"], 15);
        assert_eq!(clips[1].props["source_in"], 45);
        // The right-hand part keeps the grade of the clip it came from.
        assert_eq!(clips[1].id, split["clip2"].as_str().expect("id"));
        assert_eq!(clips[1].props["color_grade"]["exposure"], 1.0);
    }

    // Edits past the requested length extend both representations.
    insert(&mut server, &overlay, "late", 330, 30);
    assert_synchronized(&server);
    assert_eq!(
        server
            .session
            .lock()
            .expect("lock")
            .timeline
            .duration_frames,
        360
    );

    // Inserting inside a clip splits it; the export stays consistent.
    insert(&mut server, &overlay, "middle", 45, 15);
    assert_synchronized(&server);

    // A rejected edit changes neither representation (checked by `fail`).
    fail(
        &mut server,
        "clip_trim",
        json!({ "clip_id": b, "edge_in": false, "new_time_secs": 100.0 }),
    );
}

#[test]
fn trimming_a_media_clip_before_its_source_start_is_rejected() {
    let (mut server, track, root) = media_project();
    write_image(root.path(), "footage.png", 4, 4, |_, _| [255, 0, 0]);
    import(&mut server, "footage.png", "image", "footage");
    let clip = ok(
        &mut server,
        "clip_insert",
        json!({ "track_id": track, "name": "media", "start_secs": 2.0,
                "duration_secs": 2.0, "asset_id": "footage" }),
    )["clip_id"]
        .as_str()
        .expect("id")
        .to_string();
    let message = fail(
        &mut server,
        "clip_trim",
        json!({ "clip_id": clip, "edge_in": true, "new_time_secs": 1.0 }),
    );
    assert!(message.contains("source"), "{message}");
}

// ---------------------------------------------------------------------------------------- #155

/// The preview's report (the JSON in the text block) and its RGBA pixels.
fn preview_with_report(server: &mut McpServer, time_secs: f64) -> (Value, image::RgbaImage) {
    let result = ok(
        server,
        "preview_frame",
        json!({ "time_secs": time_secs, "width": 320, "height": 180 }),
    );
    let text = result["content"][0]["text"].as_str().expect("text");
    let (summary, report) = text.split_once('\n').expect("report");
    assert!(summary.contains("not a final render"), "{summary}");
    let report: Value = serde_json::from_str(report).expect("report json");
    assert!(!report["limitations"].as_array().expect("list").is_empty());
    assert!(report.get("image_data_url").is_none());
    let data = result["content"][1]["data"].as_str().expect("png");
    let png =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data).expect("base64");
    let pixels = image::load_from_memory(&png).expect("png").to_rgba8();
    (report, pixels)
}

/// Pixels of a preview made of placeholders and captions only.
fn preview(server: &mut McpServer, time_secs: f64) -> Vec<u8> {
    let (report, pixels) = preview_with_report(server, time_secs);
    assert_ne!(report["preview_kind"], "mixed");
    pixels.into_raw()
}

#[test]
fn captions_and_exposure_visibly_change_the_preview() {
    let (mut server, track) = project();
    let clip = insert(&mut server, &track, "a", 0, 60);
    let plain = preview(&mut server, 0.5);

    ok(
        &mut server,
        "color_grade_apply",
        json!({ "clip_id": clip, "exposure": -2.0 }),
    );
    let darker = preview(&mut server, 0.5);
    let brightness = |pixels: &[u8]| {
        pixels
            .chunks(4)
            .map(|p| u64::from(p[0]) + u64::from(p[1]) + u64::from(p[2]))
            .sum::<u64>()
    };
    assert!(brightness(&darker) < brightness(&plain) * 3 / 4);

    // The largest exposure exceeds a single brightness filter and must still render.
    ok(
        &mut server,
        "color_grade_apply",
        json!({ "clip_id": clip, "exposure": 5.0 }),
    );
    assert!(brightness(&preview(&mut server, 0.5)) > brightness(&plain));

    let titles = add_track(&mut server, "Titles");
    ok(
        &mut server,
        "subtitles_generate",
        json!({ "track_id": titles, "text": "HELLO WORLD", "start_secs": 0.0, "duration_secs": 1.0 }),
    );
    let first = preview(&mut server, 0.5);
    {
        let mut session = server.session.lock().expect("lock");
        session.captions[0].1 = "OTHER TEXT".to_string();
    }
    assert_ne!(
        first,
        preview(&mut server, 0.5),
        "caption glyphs are not drawn"
    );
    // Outside the caption's time range the text does not matter.
    let outside = preview(&mut server, 1.5);
    {
        let mut session = server.session.lock().expect("lock");
        session.captions[0].1 = "HELLO WORLD".to_string();
    }
    assert_eq!(outside, preview(&mut server, 1.5));
}

#[test]
fn unsupported_or_unknown_requests_are_errors() {
    let (mut server, track) = project();
    let clip = insert(&mut server, &track, "a", 0, 60);
    for (tool, arguments, needle) in [
        (
            "color_grade_apply",
            json!({ "clip_id": "nope", "contrast": 1.1 }),
            "nope",
        ),
        (
            "color_grade_apply",
            json!({ "clip_id": clip, "exposure": 9.0 }),
            "exposure",
        ),
        (
            "color_grade_apply",
            json!({ "clip_id": clip, "lut_cube": "teal_orange" }),
            "lut_cube",
        ),
        (
            "subtitles_generate",
            json!({ "track_id": track, "text": "hi", "start_secs": 3.0,
                    "duration_secs": 1.0, "style": "kinetic_pop" }),
            "style",
        ),
    ] {
        let message = fail(&mut server, tool, arguments);
        assert!(message.contains(needle), "{tool}: {message}");
    }
}

#[test]
fn preview_decodes_distinct_source_images_with_captions_and_exposure() {
    let (mut server, track, root) = media_project();
    write_image(root.path(), "red.png", 64, 36, |_, _| [200, 0, 0]);
    // Left half green, right half blue, so that placement is observable.
    write_image(root.path(), "split.png", 64, 36, |x, _| {
        if x < 32 {
            [0, 200, 0]
        } else {
            [0, 0, 200]
        }
    });
    import(&mut server, "red.png", "image", "red");
    import(&mut server, "split.png", "image", "split");
    let red = insert_media(&mut server, &track, "red", 0.0, 1.0);
    let split = insert_media(&mut server, &track, "split", 1.0, 1.0);

    let (report, first) = preview_with_report(&mut server, 0.5);
    assert_eq!(report["preview_kind"], "media");
    assert_eq!(report["media_clips"], json!([red]));
    assert_eq!(report["placeholder_clips"], json!([]));
    assert_eq!(first.get_pixel(160, 90).0, [200, 0, 0, 255]);
    assert_eq!(first.get_pixel(5, 5).0, [200, 0, 0, 255]);

    let (report, second) = preview_with_report(&mut server, 1.5);
    assert_eq!(report["media_clips"], json!([split]));
    assert_eq!(second.get_pixel(60, 90).0, [0, 200, 0, 255]);
    assert_eq!(second.get_pixel(260, 90).0, [0, 0, 200, 255]);

    ok(
        &mut server,
        "color_grade_apply",
        json!({ "clip_id": red, "exposure": -1.0 }),
    );
    let (_, darker) = preview_with_report(&mut server, 0.5);
    let level = darker.get_pixel(160, 90).0[0];
    assert!((98..=102).contains(&level), "-1 EV gave {level}");

    let titles = add_track(&mut server, "Titles");
    let caption = ok(
        &mut server,
        "subtitles_generate",
        json!({ "track_id": titles, "text": "HELLO WORLD", "start_secs": 0.0, "duration_secs": 1.0 }),
    )["clip_id"]
        .as_str()
        .expect("clip id")
        .to_string();
    let (report, captioned) = preview_with_report(&mut server, 0.5);
    // A caption is drawn over the media, not as a block that hides it.
    assert_eq!(report["preview_kind"], "media");
    assert_eq!(report["placeholder_clips"], json!([]));
    assert_eq!(captioned.get_pixel(5, 5), darker.get_pixel(5, 5));
    assert_ne!(captioned, darker, "caption glyphs are not drawn");
    ok(
        &mut server,
        "color_grade_apply",
        json!({ "clip_id": caption, "exposure": -5.0 }),
    );
    let (_, dim_caption) = preview_with_report(&mut server, 0.5);
    assert_ne!(dim_caption, captioned, "caption grade is ignored");
}

#[test]
fn preview_fits_media_inside_the_project_canvas() {
    let (mut server, track, root) = media_project();
    write_image(root.path(), "square.png", 36, 36, |_, _| [200, 0, 0]);
    import(&mut server, "square.png", "image", "square");
    insert_media(&mut server, &track, "square", 0.0, 1.0);

    let (_, pixels) = preview_with_report(&mut server, 0.0);
    // 180 pixels square in the middle of a 320x180 frame.
    assert_eq!(pixels.get_pixel(160, 90).0, [200, 0, 0, 255]);
    assert_eq!(pixels.get_pixel(75, 90).0, [200, 0, 0, 255]);
    assert_eq!(pixels.get_pixel(60, 90).0, [18, 18, 24, 255]);

    // A square preview of a 16:9 project letterboxes the canvas.
    let result = ok(
        &mut server,
        "preview_frame",
        json!({ "width": 320, "height": 320 }),
    );
    let data = result["content"][1]["data"].as_str().expect("png");
    let png =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data).expect("base64");
    let square = image::load_from_memory(&png).expect("png").to_rgba8();
    assert_eq!(square.get_pixel(160, 160).0, [200, 0, 0, 255]);
    assert_eq!(square.get_pixel(160, 30).0, [18, 18, 24, 255]);
}

#[test]
fn preview_reports_placeholders_and_omitted_clips() {
    let (mut server, track, root) = media_project();
    write_image(root.path(), "red.png", 64, 36, |_, _| [200, 0, 0]);
    std::fs::write(root.path().join("music.wav"), b"not decoded").expect("audio");
    import(&mut server, "red.png", "image", "red");
    import(&mut server, "music.wav", "audio", "music");
    let red = insert_media(&mut server, &track, "red", 0.0, 1.0);
    let sound = add_track(&mut server, "Sound");
    let music = insert_media(&mut server, &sound, "music", 0.0, 1.0);

    // Audio does not paint over the picture.
    let (report, pixels) = preview_with_report(&mut server, 0.5);
    assert_eq!(report["preview_kind"], "media");
    assert_eq!(report["omitted_clips"], json!([music]));
    assert_eq!(pixels.get_pixel(160, 90).0, [200, 0, 0, 255]);

    let overlay = add_track(&mut server, "Overlay");
    let block = insert(&mut server, &overlay, "block", 0, 30);
    let (report, pixels) = preview_with_report(&mut server, 0.5);
    assert_eq!(report["preview_kind"], "mixed");
    assert_eq!(report["media_clips"], json!([red]));
    assert_eq!(report["placeholder_clips"], json!([block]));
    assert_ne!(pixels.get_pixel(160, 90).0, [200, 0, 0, 255]);
}

#[test]
fn preview_shows_the_source_video_frame_the_clip_points_at() {
    let (mut server, track, root) = media_project();
    // One second of red followed by one second of blue.
    let generated = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error"])
        .args(["-f", "lavfi", "-i", "color=c=red:s=64x36:r=30:d=1"])
        .args(["-f", "lavfi", "-i", "color=c=blue:s=64x36:r=30:d=1"])
        .args(["-filter_complex", "[0:v][1:v]concat=n=2:v=1:a=0"])
        .args(["-c:v", "ffv1"])
        .arg(root.path().join("two.mkv"))
        .status();
    let Ok(generated) = generated else {
        eprintln!("skipping video preview test: FFmpeg is unavailable");
        return;
    };
    assert!(generated.success());
    import(&mut server, "two.mkv", "video", "two");
    let clip = insert_media(&mut server, &track, "two", 2.0, 2.0);

    let center = |server: &mut McpServer, time| {
        let (report, pixels) = preview_with_report(server, time);
        assert_eq!(report["preview_kind"], "media");
        pixels.get_pixel(160, 90).0
    };
    let is_red = |p: [u8; 4]| p[0] > 200 && p[2] < 50;
    let is_blue = |p: [u8; 4]| p[2] > 200 && p[0] < 50;
    assert!(is_red(center(&mut server, 2.5)));
    assert!(is_blue(center(&mut server, 3.5)));

    // Splitting keeps each part's place in the source.
    let parts = ok(
        &mut server,
        "clip_split",
        json!({ "clip_id": clip, "split_time_secs": 3.5 }),
    );
    assert!(parts["clip2"].is_string());
    assert!(is_blue(center(&mut server, 3.75)));
    // Trimming the head moves the source in-point along with it.
    ok(
        &mut server,
        "clip_trim",
        json!({ "clip_id": clip, "edge_in": true, "new_time_secs": 2.9 }),
    );
    assert!(is_blue(center(&mut server, 3.2)));
    assert!(is_red(center(&mut server, 2.9)));
}

#[test]
fn unusable_media_is_rejected_without_changing_state() {
    let (mut server, track, root) = media_project();
    let outside = tempfile::tempdir().expect("outside");
    write_image(outside.path(), "secret.png", 4, 4, |_, _| [1, 2, 3]);
    let secret = outside.path().join("secret.png");
    let secret = secret.to_str().expect("path");
    write_image(root.path(), "ok.png", 4, 4, |_, _| [1, 2, 3]);
    std::fs::write(root.path().join("broken.png"), b"not an image").expect("broken");
    std::fs::write(root.path().join("broken.mp4"), b"not a video").expect("broken");
    std::fs::write(
        root.path().join("vector.svg"),
        "<svg xmlns='http://www.w3.org/2000/svg'/>",
    )
    .expect("svg");
    import(&mut server, "ok.png", "image", "ok");

    for (arguments, needle) in [
        (json!({ "path": secret, "kind": "image" }), "outside"),
        (
            json!({ "path": format!("file://{secret}"), "kind": "audio" }),
            "outside",
        ),
        (
            json!({ "path": "https://example.com/a.png", "kind": "image" }),
            "local",
        ),
        (json!({ "path": "missing.png", "kind": "image" }), "path"),
        (json!({ "path": ".", "kind": "audio" }), "not a file"),
        (json!({ "path": "broken.png", "kind": "image" }), "decode"),
        (json!({ "path": "broken.mp4", "kind": "video" }), "video"),
        (json!({ "path": "vector.svg", "kind": "image" }), "SVG"),
        (json!({ "path": "ok.png", "kind": "font" }), "kind"),
        (
            json!({ "path": "ok.png", "kind": "image", "asset_id": "ok" }),
            "already",
        ),
        (
            json!({ "path": "ok.png", "kind": "image", "asset_id": " " }),
            "asset_id",
        ),
    ] {
        let message = fail(&mut server, "asset_import", arguments);
        assert!(message.contains(needle), "{needle}: {message}");
    }

    let message = fail(
        &mut server,
        "clip_insert",
        json!({ "track_id": track, "name": "x", "start_secs": 0.0,
                "duration_secs": 1.0, "asset_id": "never_imported" }),
    );
    assert!(message.contains("never_imported"), "{message}");

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(secret, root.path().join("link.png")).expect("symlink");
        let message = fail(
            &mut server,
            "asset_import",
            json!({ "path": "link.png", "kind": "image" }),
        );
        assert!(message.contains("outside"), "{message}");

        // A file swapped for a link after import is caught when it is read.
        insert_media(&mut server, &track, "ok", 0.0, 1.0);
        std::fs::remove_file(root.path().join("ok.png")).expect("remove");
        std::os::unix::fs::symlink(secret, root.path().join("ok.png")).expect("symlink");
        let message = fail(&mut server, "preview_frame", json!({}));
        assert!(message.contains("outside"), "{message}");
    }
}

#[test]
fn preview_notices_changed_and_damaged_files() {
    let (mut server, track, root) = media_project();
    write_image(root.path(), "a.png", 64, 36, |_, _| [200, 0, 0]);
    import(&mut server, "a.png", "image", "a");
    insert_media(&mut server, &track, "a", 0.0, 1.0);
    let (_, before) = preview_with_report(&mut server, 0.0);
    assert_eq!(before.get_pixel(160, 90).0, [200, 0, 0, 255]);

    write_image(root.path(), "a.png", 64, 36, |_, _| [0, 200, 0]);
    let (_, after) = preview_with_report(&mut server, 0.0);
    assert_eq!(after.get_pixel(160, 90).0, [0, 200, 0, 255]);

    std::fs::write(root.path().join("a.png"), b"damaged").expect("damage");
    let message = fail(&mut server, "preview_frame", json!({}));
    assert!(message.contains("a.png"), "{message}");
    std::fs::remove_file(root.path().join("a.png")).expect("remove");
    let message = fail(&mut server, "preview_frame", json!({}));
    assert!(message.contains("asset 'a'"), "{message}");
}

#[test]
fn assets_need_a_project_and_are_exported_with_it() {
    let root = tempfile::tempdir().expect("media root");
    write_image(root.path(), "My Shot.png", 4, 4, |_, _| [1, 2, 3]);
    let session = McpSession::with_media_root(root.path());
    let mut server = McpServer::with_session(Arc::new(Mutex::new(session)));
    let arguments = json!({ "path": "My Shot.png", "kind": "image" });
    let message = fail(&mut server, "asset_import", arguments.clone());
    assert!(message.contains("project_create"), "{message}");

    ok(&mut server, "project_create", json!({ "name": "P" }));
    let first = ok(&mut server, "asset_import", arguments.clone());
    assert_eq!(first["asset_id"], "asset_my_shot");
    assert_eq!(first["path"], "My Shot.png");
    let second = ok(&mut server, "asset_import", arguments);
    assert_eq!(second["asset_id"], "asset_my_shot_2");

    let track = add_track(&mut server, "Main");
    insert_media(&mut server, &track, "asset_my_shot", 0.0, 1.0);
    assert_synchronized(&server);
    let session = server.session.lock().expect("lock");
    let project = session.project.clone().expect("project");
    assert_eq!(project.assets.len(), 2);
    // The exported paths resolve when the project is saved in the media root.
    project
        .validate_asset_files(root.path())
        .expect("assets resolve");
}

// ---------------------------------------------------------------------------------------- #156

#[test]
fn malformed_numbers_are_rejected_without_changing_state() {
    let (mut server, track) = project();
    let clip = insert(&mut server, &track, "a", 30, 60);
    let base = json!({ "name": "P" });
    for (field, value) in [
        ("width", json!(0)),
        ("width", json!(-1)),
        ("width", json!(1.5)),
        ("width", json!("640")),
        ("width", json!(4_294_967_296_u64 + 640)),
        ("height", json!(100_000)),
        ("fps", json!(0)),
        ("fps", json!(-30.0)),
        ("fps", json!(1e9)),
        ("fps", json!("30")),
        ("duration_secs", json!(0)),
        ("duration_secs", json!(-5.0)),
        ("duration_secs", json!(1e12)),
        ("duration_secs", json!(0.001)),
    ] {
        let mut arguments = base.clone();
        arguments[field] = value;
        let message = fail(&mut server, "project_create", arguments);
        assert!(message.contains(field), "{field}: {message}");
    }

    for (field, value) in [
        ("start_secs", json!(-1.0)),
        ("start_secs", json!(1e12)),
        ("start_secs", json!("0")),
        ("duration_secs", json!(0)),
        ("duration_secs", json!(-2.0)),
        ("duration_secs", json!(0.001)),
        ("duration_secs", json!(1e12)),
    ] {
        let mut arguments = json!({ "track_id": track, "name": "b",
                                    "start_secs": 0.0, "duration_secs": 1.0 });
        arguments[field] = value;
        let message = fail(&mut server, "clip_insert", arguments);
        assert!(message.contains(field), "{field}: {message}");
    }

    for (tool, arguments, field) in [
        (
            "clip_trim",
            json!({ "clip_id": clip, "edge_in": true, "new_time_secs": -1.0 }),
            "new_time_secs",
        ),
        (
            "clip_trim",
            json!({ "clip_id": clip, "edge_in": false, "new_time_secs": 1e12 }),
            "new_time_secs",
        ),
        (
            "clip_trim",
            json!({ "clip_id": clip, "edge_in": "yes", "new_time_secs": 1.0 }),
            "edge_in",
        ),
        (
            "clip_trim",
            json!({ "clip_id": clip, "edge_in": false, "new_time_secs": 0.5 }),
            "new_time_secs",
        ),
        (
            "clip_split",
            json!({ "clip_id": clip, "split_time_secs": -1.0 }),
            "split_time_secs",
        ),
        ("preview_frame", json!({ "time_secs": -1.0 }), "time_secs"),
        ("preview_frame", json!({ "width": 0 }), "width"),
        ("preview_frame", json!({ "height": 1_000_000 }), "height"),
    ] {
        let message = fail(&mut server, tool, arguments);
        assert!(message.contains(field), "{tool} {field}: {message}");
    }
}
