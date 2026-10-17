//! Comprehensive tests for Model Context Protocol (MCP) server (#102).

use lightcraft_mcp::session::McpSession;
use lightcraft_mcp::McpServer;
use serde_json::json;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

#[test]
fn test_mcp_initialize_and_tools_list() {
    let mut server = McpServer::new();

    // 1. Initialize
    let init_msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "clientInfo": { "name": "Claude Desktop", "version": "1.0.0" }
        }
    });

    let resp_str = server.handle_message(&init_msg.to_string()).expect("response");
    let resp: serde_json::Value = serde_json::from_str(&resp_str).expect("valid json");
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    assert!(resp["result"]["capabilities"]["tools"].is_object());

    // 2. Tools list
    let list_msg = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });

    let list_resp_str = server.handle_message(&list_msg.to_string()).expect("response");
    let list_resp: serde_json::Value = serde_json::from_str(&list_resp_str).expect("valid json");
    let tools = list_resp["result"]["tools"].as_array().expect("tools array");

    let tool_names: Vec<&str> = tools
        .iter()
        .map(|t| t["name"].as_str().expect("name"))
        .collect();

    assert!(tool_names.contains(&"project_create"));
    assert!(tool_names.contains(&"timeline_add_track"));
    assert!(tool_names.contains(&"clip_insert"));
    assert!(tool_names.contains(&"clip_trim"));
    assert!(tool_names.contains(&"clip_split"));
    assert!(tool_names.contains(&"color_grade_apply"));
    assert!(tool_names.contains(&"subtitles_generate"));
    assert!(tool_names.contains(&"preview_frame"));
    assert!(tool_names.contains(&"render_composition"));
}

#[test]
fn test_mcp_end_to_end_autonomous_editing_workflow() {
    let mut server = McpServer::new();

    // 1. Create project
    let create_call = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "tools/call",
        "params": {
            "name": "project_create",
            "arguments": {
                "name": "Cinematic Teaser",
                "width": 1920,
                "height": 1080,
                "fps": 24.0,
                "duration_secs": 15.0
            }
        }
    });
    let resp1 = server.handle_message(&create_call.to_string()).unwrap();
    assert!(resp1.contains("Cinematic Teaser"));

    // 2. Add video track
    let add_track_call = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "tools/call",
        "params": {
            "name": "timeline_add_track",
            "arguments": {
                "track_type": "video",
                "name": "A-Roll"
            }
        }
    });
    let resp2 = server.handle_message(&add_track_call.to_string()).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&resp2).unwrap();
    assert!(!v2["result"]["isError"].as_bool().unwrap());

    // 3. Insert clip
    let insert_call = json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "tools/call",
        "params": {
            "name": "clip_insert",
            "arguments": {
                "track_id": "track_a-roll_1",
                "name": "Hero_Shot",
                "start_secs": 0.0,
                "duration_secs": 5.0
            }
        }
    });
    let resp3 = server.handle_message(&insert_call.to_string()).unwrap();
    assert!(resp3.contains("clip_hero_shot_0"));

    // 4. Color grading
    let grade_call = json!({
        "jsonrpc": "2.0",
        "id": 13,
        "method": "tools/call",
        "params": {
            "name": "color_grade_apply",
            "arguments": {
                "clip_id": "clip_hero_shot_0",
                "exposure": 0.5,
                "contrast": 1.15,
                "saturation": 1.05
            }
        }
    });
    let resp4 = server.handle_message(&grade_call.to_string()).unwrap();
    assert!(resp4.contains("Applied color grading"));

    // 5. Generate subtitles
    let sub_call = json!({
        "jsonrpc": "2.0",
        "id": 14,
        "method": "tools/call",
        "params": {
            "name": "subtitles_generate",
            "arguments": {
                "track_id": "track_a-roll_1",
                "text": "The adventure begins today.",
                "start_secs": 6.0,
                "duration_secs": 3.0
            }
        }
    });
    let resp5 = server.handle_message(&sub_call.to_string()).unwrap();
    assert!(resp5.contains("The adventure begins today."));

    // 6. Preview frame (Visual inspection feedback loop for AI agent)
    let preview_call = json!({
        "jsonrpc": "2.0",
        "id": 15,
        "method": "tools/call",
        "params": {
            "name": "preview_frame",
            "arguments": {
                "time_secs": 2.0,
                "width": 320,
                "height": 180
            }
        }
    });
    let resp6 = server.handle_message(&preview_call.to_string()).unwrap();
    assert!(resp6.contains("image/png"));
    assert!(resp6.contains("\"type\":\"image\""));

    // 7. Read resources
    let read_res_call = json!({
        "jsonrpc": "2.0",
        "id": 16,
        "method": "resources/read",
        "params": {
            "uri": "motion://timeline"
        }
    });
    let resp7 = server.handle_message(&read_res_call.to_string()).unwrap();
    assert!(resp7.contains("Hero_Shot"));
}

#[test]
fn test_mcp_stdio_stream() {
    let session = Arc::new(Mutex::new(McpSession::new()));
    let mut server = McpServer::with_session(session);

    let input = "{\"jsonrpc\":\"2.0\",\"id\":100,\"method\":\"ping\"}\n";
    let reader = Cursor::new(input.as_bytes());
    let mut output = Vec::new();

    server.run_stdio(reader, &mut output).expect("stdio run");
    let out_str = String::from_utf8(output).expect("valid utf8");
    assert!(out_str.contains("\"id\":100"));
}
