//! Model Context Protocol (MCP) Server for Autonomous AI Video/Audio Editing (#102).
//!
//! Provides a standardized JSON-RPC 2.0 Model Context Protocol (Anthropic MCP) server
//! enabling LLMs (Claude, Gemini, ChatGPT) to autonomously compose, edit, grade,
//! and visually inspect complete video productions through standardized tool calls.

pub mod session;
pub mod tools;

use serde_json::{json, Value};
use session::McpSession;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};
use tools::{execute_tool, get_tools};

pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// Core MCP Server state and request dispatcher.
pub struct McpServer {
    pub session: Arc<Mutex<McpSession>>,
    pub initialized: bool,
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl McpServer {
    pub fn new() -> Self {
        Self {
            session: Arc::new(Mutex::new(McpSession::new())),
            initialized: false,
        }
    }

    pub fn with_session(session: Arc<Mutex<McpSession>>) -> Self {
        Self {
            session,
            initialized: false,
        }
    }

    /// Process a single incoming JSON-RPC 2.0 message and return response JSON string.
    pub fn handle_message(&mut self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        let parsed: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": { "code": -32700, "message": format!("Parse error: {e}") }
                });
                return Some(err_resp.to_string());
            }
        };

        let id = parsed.get("id").cloned();
        let method = parsed.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let params = parsed.get("params").cloned().unwrap_or(json!({}));

        // Notifications without id don't produce responses
        let is_notification = id.is_none();

        let response = match method {
            "initialize" => {
                self.initialized = true;
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {
                        "tools": { "listChanged": false },
                        "resources": { "subscribe": false, "listChanged": false }
                    },
                    "serverInfo": {
                        "name": "Rust Motion & LightCraft MCP Server",
                        "version": "1.0.0"
                    },
                    "instructions": "Autonomous video/audio production server: create projects, manipulate timeline tracks/clips, apply color grading, generate subtitles, and render visual preview screenshots."
                })
            }
            "notifications/initialized" | "initialized" => {
                return None;
            }
            "ping" => {
                json!({})
            }
            "tools/list" => {
                let tool_list: Vec<Value> = get_tools()
                    .into_iter()
                    .map(|t| {
                        json!({
                            "name": t.name,
                            "description": t.description,
                            "inputSchema": t.input_schema
                        })
                    })
                    .collect();
                json!({ "tools": tool_list })
            }
            "tools/call" => {
                let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

                match execute_tool(&self.session, tool_name, arguments) {
                    Ok(result) => {
                        let is_preview = tool_name == "preview_frame";
                        let content = if is_preview {
                            let data_url = result.get("image_data_url").and_then(|v| v.as_str()).unwrap_or("");
                            vec![
                                json!({ "type": "text", "text": format!("Rendered preview at frame {}", result.get("frame_number").unwrap_or(&json!(0))) }),
                                json!({ "type": "image", "data": data_url.trim_start_matches("data:image/png;base64,"), "mimeType": "image/png" }),
                            ]
                        } else {
                            vec![json!({ "type": "text", "text": serde_json::to_string_pretty(&result).unwrap_or_default() })]
                        };

                        json!({ "content": content, "isError": false })
                    }
                    Err(err) => {
                        json!({
                            "content": [{ "type": "text", "text": format!("Error: {err}") }],
                            "isError": true
                        })
                    }
                }
            }
            "resources/list" => {
                json!({
                    "resources": [
                        {
                            "uri": "motion://project",
                            "name": "Active Project State",
                            "mimeType": "application/json",
                            "description": "Current composition dimensions, settings, and metadata"
                        },
                        {
                            "uri": "motion://timeline",
                            "name": "NLE Timeline State",
                            "mimeType": "application/json",
                            "description": "Hierarchy of tracks, clips, and transitions"
                        }
                    ]
                })
            }
            "resources/read" => {
                let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
                let sess = self.session.lock().map_err(|e| e.to_string());

                match (uri, sess) {
                    ("motion://project", Ok(s)) => {
                        let text = serde_json::to_string_pretty(&s.project).unwrap_or_default();
                        json!({ "contents": [{ "uri": uri, "mimeType": "application/json", "text": text }] })
                    }
                    ("motion://timeline", Ok(s)) => {
                        let text = serde_json::to_string_pretty(&s.timeline).unwrap_or_default();
                        json!({ "contents": [{ "uri": uri, "mimeType": "application/json", "text": text }] })
                    }
                    _ => {
                        let err_resp = json!({
                            "jsonrpc": "2.0",
                            "id": id.unwrap_or(Value::Null),
                            "error": { "code": -32602, "message": format!("Resource '{uri}' not found") }
                        });
                        return Some(err_resp.to_string());
                    }
                }
            }
            other => {
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "id": id.unwrap_or(Value::Null),
                    "error": { "code": -32601, "message": format!("Method '{other}' not found") }
                });
                return Some(err_resp.to_string());
            }
        };

        if is_notification {
            None
        } else {
            let resp = json!({
                "jsonrpc": "2.0",
                "id": id.unwrap_or(Value::Null),
                "result": response
            });
            Some(resp.to_string())
        }
    }

    /// Run stdio JSON-RPC loop reading from `reader` and writing to `writer`.
    pub fn run_stdio<R: BufRead, W: Write>(&mut self, mut reader: R, mut writer: W) -> std::io::Result<()> {
        let mut line = String::new();
        while reader.read_line(&mut line)? > 0 {
            if let Some(resp) = self.handle_message(&line) {
                writeln!(writer, "{resp}")?;
                writer.flush()?;
            }
            line.clear();
        }
        Ok(())
    }
}
