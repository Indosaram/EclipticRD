use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::{
    agent_input::{
        convert_agent_action_to_events, encode_nv12_screenshot, AgentAction, InputStateTracker,
        MouseButton, ScreenshotFormat,
    },
    agent_server::AgentServerBackend,
};

pub fn list_mcp_tools() -> Value {
    json!([
        {
            "name": "remote_mouse_click",
            "description": "Clicks at specific coordinates on the remote desktop screen.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "x": { "type": "number", "description": "X coordinate (pixels or normalized 0.0-1.0)" },
                    "y": { "type": "number", "description": "Y coordinate (pixels or normalized 0.0-1.0)" },
                    "button": { "type": "string", "enum": ["left", "right", "middle"], "default": "left" },
                    "count": { "type": "integer", "default": 1, "description": "Click count (1=single, 2=double, 3=triple)" },
                    "normalized": { "type": "boolean", "default": false, "description": "Whether coordinates are normalized in [0.0, 1.0]" }
                },
                "required": ["x", "y"]
            }
        },
        {
            "name": "remote_mouse_move",
            "description": "Moves the cursor to the target coordinates.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "x": { "type": "number", "description": "X coordinate" },
                    "y": { "type": "number", "description": "Y coordinate" },
                    "normalized": { "type": "boolean", "default": false }
                },
                "required": ["x", "y"]
            }
        },
        {
            "name": "remote_mouse_drag",
            "description": "Drags mouse pointer from start to end coordinates with held button.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "start_x": { "type": "number" },
                    "start_y": { "type": "number" },
                    "end_x": { "type": "number" },
                    "end_y": { "type": "number" },
                    "button": { "type": "string", "enum": ["left", "right", "middle"], "default": "left" },
                    "steps": { "type": "integer", "default": 10 },
                    "normalized": { "type": "boolean", "default": false }
                },
                "required": ["start_x", "start_y", "end_x", "end_y"]
            }
        },
        {
            "name": "remote_mouse_scroll",
            "description": "Scrolls mouse wheel horizontally (dx) and vertically (dy).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dx": { "type": "number", "default": 0.0 },
                    "dy": { "type": "number", "default": 0.0 },
                    "x": { "type": "number" },
                    "y": { "type": "number" },
                    "normalized": { "type": "boolean", "default": false }
                },
                "required": ["dx", "dy"]
            }
        },
        {
            "name": "remote_key_press",
            "description": "Presses and releases a single key (e.g. 'Enter', 'Escape', 'F5', 'Tab', 'Space').",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Key name (e.g. 'Enter', 'Space', 'Backspace', 'F5')" },
                    "hold_ms": { "type": "integer", "default": 50 }
                },
                "required": ["key"]
            }
        },
        {
            "name": "remote_hotkey",
            "description": "Executes a combination of modifier keys and target key (e.g. ['Control', 'Alt', 't'], ['Command', 'Space']).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "keys": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of keys (e.g. ['Control', 'Shift', 't'])"
                    }
                },
                "required": ["keys"]
            }
        },
        {
            "name": "remote_type_text",
            "description": "Types a text string onto the remote host.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Text to type" },
                    "delay_ms": { "type": "integer", "default": 20 }
                },
                "required": ["text"]
            }
        },
        {
            "name": "remote_release_all",
            "description": "Emergency release for all active keys and mouse buttons.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "remote_get_screen_info",
            "description": "Gets current remote screen dimensions, scale factor, and connected host name.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "remote_take_screenshot",
            "description": "Captures the active remote desktop screen as a base64 PNG image.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "format": { "type": "string", "enum": ["png", "jpeg"], "default": "png" }
                }
            }
        }
    ])
}

pub fn handle_mcp_message(
    msg: &str,
    backend: Arc<dyn AgentServerBackend>,
    tracker: &mut InputStateTracker,
    current_pos: &mut (f32, f32),
) -> Option<String> {
    let req: Value = match serde_json::from_str(msg) {
        Ok(v) => v,
        Err(_) => {
            return Some(
                json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": { "code": -32700, "message": "Parse error" }
                })
                .to_string(),
            );
        }
    };

    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(|m| m.as_str())?;

    match method {
        "initialize" => {
            let resp = json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "eclipticrd-mcp",
                        "version": "0.1.0"
                    }
                }
            });
            Some(resp.to_string())
        }
        "notifications/initialized" => None,
        "tools/list" => {
            let resp = json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": list_mcp_tools()
                }
            });
            Some(resp.to_string())
        }
        "tools/call" => {
            let params = req.get("params")?;
            let tool_name = params.get("name").and_then(|n| n.as_str())?;
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            let screen_info = backend.get_screen_info();

            let call_result = match tool_name {
                "remote_get_screen_info" => Ok(json!([
                    {
                        "type": "text",
                        "text": serde_json::to_string(&screen_info).unwrap()
                    }
                ])),
                "remote_take_screenshot" => {
                    let format_str = arguments
                        .get("format")
                        .and_then(|f| f.as_str())
                        .unwrap_or("png");
                    let format = if format_str == "jpeg" {
                        ScreenshotFormat::Jpeg
                    } else {
                        ScreenshotFormat::Png
                    };
                    match backend.get_latest_frame_nv12() {
                        Some((w, h, buf)) => match encode_nv12_screenshot(w, h, &buf, format) {
                            Ok(b64) => {
                                let mime = if format == ScreenshotFormat::Jpeg {
                                    "image/jpeg"
                                } else {
                                    "image/png"
                                };
                                Ok(json!([
                                    {
                                        "type": "image",
                                        "data": b64,
                                        "mimeType": mime
                                    }
                                ]))
                            }
                            Err(e) => Err(format!("failed to encode screenshot: {e}")),
                        },
                        None => Err("no active frame received yet".to_string()),
                    }
                }
                "remote_release_all" => {
                    let events = tracker.release_all(current_pos.0, current_pos.1);
                    let count = events.len();
                    for event in events {
                        let _ = backend.send_input_event(event);
                    }
                    Ok(
                        json!([{ "type": "text", "text": format!("Released all states ({count} events sent)") }]),
                    )
                }
                _ => {
                    let action = match tool_name {
                        "remote_mouse_click" => {
                            let x =
                                arguments.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                            let y =
                                arguments.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                            let button_str = arguments
                                .get("button")
                                .and_then(|v| v.as_str())
                                .unwrap_or("left");
                            let button = match button_str {
                                "right" => MouseButton::Right,
                                "middle" => MouseButton::Middle,
                                _ => MouseButton::Left,
                            };
                            let count =
                                arguments.get("count").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                            let normalized = arguments
                                .get("normalized")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            AgentAction::Click {
                                x,
                                y,
                                button,
                                count,
                                normalized,
                            }
                        }
                        "remote_mouse_move" => {
                            let x =
                                arguments.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                            let y =
                                arguments.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                            let normalized = arguments
                                .get("normalized")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            AgentAction::MouseMove { x, y, normalized }
                        }
                        "remote_mouse_drag" => {
                            let start_x = arguments
                                .get("start_x")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0) as f32;
                            let start_y = arguments
                                .get("start_y")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0) as f32;
                            let end_x = arguments
                                .get("end_x")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0) as f32;
                            let end_y = arguments
                                .get("end_y")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0) as f32;
                            let button_str = arguments
                                .get("button")
                                .and_then(|v| v.as_str())
                                .unwrap_or("left");
                            let button = match button_str {
                                "right" => MouseButton::Right,
                                "middle" => MouseButton::Middle,
                                _ => MouseButton::Left,
                            };
                            let steps = arguments
                                .get("steps")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(10) as u32;
                            let normalized = arguments
                                .get("normalized")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            AgentAction::Drag {
                                start_x,
                                start_y,
                                end_x,
                                end_y,
                                button,
                                steps,
                                duration_ms: 200,
                                normalized,
                            }
                        }
                        "remote_mouse_scroll" => {
                            let dx =
                                arguments.get("dx").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                            let dy =
                                arguments.get("dy").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                            let x = arguments
                                .get("x")
                                .and_then(|v| v.as_f64())
                                .map(|v| v as f32);
                            let y = arguments
                                .get("y")
                                .and_then(|v| v.as_f64())
                                .map(|v| v as f32);
                            let normalized = arguments
                                .get("normalized")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            AgentAction::Scroll {
                                dx,
                                dy,
                                x,
                                y,
                                normalized,
                            }
                        }
                        "remote_key_press" => {
                            let key = arguments
                                .get("key")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let hold_ms = arguments
                                .get("hold_ms")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(50);
                            AgentAction::KeyPress { key, hold_ms }
                        }
                        "remote_hotkey" => {
                            let keys = arguments
                                .get("keys")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                        .collect()
                                })
                                .unwrap_or_default();
                            AgentAction::Hotkey { keys }
                        }
                        "remote_type_text" => {
                            let text = arguments
                                .get("text")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let delay_ms = arguments
                                .get("delay_ms")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(20);
                            AgentAction::TypeText {
                                text,
                                delay_ms,
                                paste_mode: false,
                            }
                        }
                        _ => {
                            return Some(json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "error": { "code": -32601, "message": format!("Method not found: {tool_name}") }
                            }).to_string());
                        }
                    };

                    match convert_agent_action_to_events(
                        &action,
                        tracker,
                        current_pos,
                        screen_info.width as f32,
                        screen_info.height as f32,
                    ) {
                        Ok(events) => {
                            let count = events.len();
                            for event in events {
                                if let Err(err) = backend.send_input_event(event) {
                                    return Some(json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "result": {
                                            "content": [{ "type": "text", "text": format!("Input transmission error: {err}") }],
                                            "isError": true
                                        }
                                    }).to_string());
                                }
                            }
                            Ok(
                                json!([{ "type": "text", "text": format!("Dispatched {tool_name} successfully ({count} events sent)") }]),
                            )
                        }
                        Err(e) => Err(e.to_string()),
                    }
                }
            };

            let resp = match call_result {
                Ok(content) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": content,
                        "isError": false
                    }
                }),
                Err(err_msg) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": err_msg }],
                        "isError": true
                    }
                }),
            };
            Some(resp.to_string())
        }
        _ => Some(
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("Method not found: {method}") }
            })
            .to_string(),
        ),
    }
}

pub async fn run_mcp_stdio(backend: Arc<dyn AgentServerBackend>) -> std::io::Result<()> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    let mut tracker = InputStateTracker::default();
    let mut current_pos = (0.5, 0.5);

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(resp) =
            handle_mcp_message(trimmed, backend.clone(), &mut tracker, &mut current_pos)
        {
            stdout.write_all(resp.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_input::ScreenInfo;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestBackend {
        sent: AtomicUsize,
    }

    impl AgentServerBackend for TestBackend {
        fn send_input_event(&self, _event: erd_proto::InputEvent) -> Result<(), String> {
            self.sent.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn get_screen_info(&self) -> ScreenInfo {
            ScreenInfo {
                width: 1920,
                height: 1080,
                scale: 1.0,
                connected_host: "test-mcp".to_string(),
            }
        }

        fn get_latest_frame_nv12(&self) -> Option<(u32, u32, Arc<Vec<u8>>)> {
            let w = 64u32;
            let h = 64u32;
            let len = (w * h * 3 / 2) as usize;
            Some((w, h, Arc::new(vec![128u8; len])))
        }
    }

    #[test]
    fn handles_mcp_protocol_handshake_and_tool_calls() {
        let backend = Arc::new(TestBackend {
            sent: AtomicUsize::new(0),
        });
        let mut tracker = InputStateTracker::default();
        let mut current_pos = (0.0, 0.0);

        let init_req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let init_resp =
            handle_mcp_message(init_req, backend.clone(), &mut tracker, &mut current_pos).unwrap();
        assert!(init_resp.contains("protocolVersion"));
        assert!(init_resp.contains("eclipticrd-mcp"));

        let list_req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
        let list_resp =
            handle_mcp_message(list_req, backend.clone(), &mut tracker, &mut current_pos).unwrap();
        assert!(list_resp.contains("remote_mouse_click"));
        assert!(list_resp.contains("remote_take_screenshot"));

        let click_req = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"remote_mouse_click","arguments":{"x":100,"y":200}}}"#;
        let click_resp =
            handle_mcp_message(click_req, backend.clone(), &mut tracker, &mut current_pos).unwrap();
        assert!(click_resp.contains("Dispatched remote_mouse_click successfully"));
        assert_eq!(backend.sent.load(Ordering::SeqCst), 3);

        let info_req = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"remote_get_screen_info","arguments":{}}}"#;
        let info_resp =
            handle_mcp_message(info_req, backend.clone(), &mut tracker, &mut current_pos).unwrap();
        assert!(info_resp.contains("1920"));
        assert!(info_resp.contains("test-mcp"));

        let shot_req = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"remote_take_screenshot","arguments":{"format":"png"}}}"#;
        let shot_resp =
            handle_mcp_message(shot_req, backend.clone(), &mut tracker, &mut current_pos).unwrap();
        assert!(shot_resp.contains("image/png"));
        assert!(shot_resp.contains("data"));
    }
}
