//! Regression tests for the agent-facing editing contract (#152-#156).

use rust_motion_mcp::McpServer;
use serde_json::{json, Value};

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
    let (mut server, track) = project();
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

fn preview(server: &mut McpServer, time_secs: f64) -> Vec<u8> {
    let result = ok(
        server,
        "preview_frame",
        json!({ "time_secs": time_secs, "width": 320, "height": 180 }),
    );
    assert!(result["content"][0]["text"]
        .as_str()
        .expect("text")
        .contains("Schematic"));
    let data = result["content"][1]["data"].as_str().expect("png");
    let png =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data).expect("base64");
    image::load_from_memory(&png)
        .expect("png")
        .to_rgba8()
        .into_raw()
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
