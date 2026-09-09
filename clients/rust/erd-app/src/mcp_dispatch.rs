use crate::{
    agent_input::{
        convert_agent_action_to_events, encode_nv12_screenshot, AgentAction, InputStateTracker,
        ScreenshotFormat,
    },
    agent_server::AgentServerBackend,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Deserialize)]
struct Call {
    name: String,
    #[serde(default = "empty_arguments")]
    arguments: serde_json::Map<String, Value>,
}
fn empty_arguments() -> serde_json::Map<String, Value> {
    serde_json::Map::new()
}

fn error(id: Value, code: i32, message: impl ToString) -> String {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message.to_string()}}).to_string()
}

pub(super) fn release(
    backend: &dyn AgentServerBackend,
    tracker: &mut InputStateTracker,
    pos: (f32, f32),
) -> Result<usize, String> {
    let events = tracker.release_all(pos.0, pos.1);
    let count = events.len();
    let mut errors = Vec::new();
    for event in events {
        if let Err(err) = backend.send_input_event(event) {
            errors.push(err);
        }
    }
    if errors.is_empty() {
        Ok(count)
    } else {
        Err(errors.join("; "))
    }
}

pub fn handle_mcp_message(
    msg: &str,
    backend: Arc<dyn AgentServerBackend>,
    tracker: &mut InputStateTracker,
    pos: &mut (f32, f32),
) -> Option<String> {
    let req: Value = match serde_json::from_str(msg) {
        Ok(req) => req,
        Err(err) => return Some(error(Value::Null, -32700, err)),
    };
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    if !req.is_object()
        || req["jsonrpc"] != "2.0"
        || !req["method"].is_string()
        || !(id.is_null() || id.is_string() || id.is_number())
    {
        return Some(error(Value::Null, -32600, "Invalid request"));
    }
    // Notifications are not tool invocations: no response and no input dispatch.
    req.get("id")?;
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));
    if !params.is_object() {
        return Some(error(id, -32602, "Expected object params"));
    }
    let result = match req["method"].as_str()? {
        "initialize" => {
            json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"eclipticrd-mcp","version":"0.1.0"}})
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools":super::list_mcp_tools()}),
        "tools/call" => {
            let call: Call = match serde_json::from_value(params) {
                Ok(call) => call,
                Err(err) => return Some(error(id, -32602, err)),
            };
            let content = match call.name.as_str() {
                "remote_get_screen_info" => Ok(json!([{"type":"text","text":json!(backend.get_screen_info()).to_string()}])),
                "remote_take_screenshot" => {
                    let format = match call.arguments.get("format") {
                        None => ScreenshotFormat::Png,
                        Some(Value::String(s)) if s == "png" => ScreenshotFormat::Png,
                        Some(Value::String(s)) if s == "jpeg" => ScreenshotFormat::Jpeg,
                        _ => return Some(error(id, -32602, "Invalid screenshot format")),
                    };
                    match backend.get_latest_frame_nv12() {
                        Some((w,h,buf)) => encode_nv12_screenshot(w,h,&buf,format)
                            .map(|data| json!([{"type":"image","data":data,"mimeType":if format == ScreenshotFormat::Png {"image/png"} else {"image/jpeg"}}]))
                            .map_err(|err| err.to_string()),
                        None => Err("no active frame received yet".into()),
                    }
                }
                "remote_release_all" => release(backend.as_ref(), tracker, *pos)
                    .map(|count| json!([{"type":"text","text":format!("Released all states ({count} events sent)")}])),
                name => {
                    let tag = match name {
                        "remote_mouse_click" => "click",
                        "remote_mouse_move" => "mouse_move",
                        "remote_mouse_drag" => "drag",
                        "remote_mouse_scroll" => "scroll",
                        "remote_key_press" => "key_press",
                        "remote_hotkey" => "hotkey",
                        "remote_type_text" => "type_text",
                        _ => return Some(error(id, -32602, "Unknown tool")),
                    };
                    let mut args = call.arguments;
                    args.insert("action".into(), json!(tag));
                    let action: AgentAction = match serde_json::from_value(Value::Object(args)) {
                        Ok(action) => action,
                        Err(err) => return Some(error(id, -32602, err)),
                    };
                    let screen = backend.get_screen_info();
                    let events = match convert_agent_action_to_events(&action, tracker, pos, screen.width as f32, screen.height as f32) {
                        Ok(events) => events,
                        Err(err) => return Some(error(id, -32602, err)),
                    };
                    let count = events.len();
                    let mut sent = Ok(json!([{"type":"text","text":format!("Dispatched {name} successfully ({count} events sent)")}]));
                    for event in events {
                        if let Err(err) = backend.send_input_event(event) {
                            let cleanup = release(backend.as_ref(), tracker, *pos);
                            sent = Err(match cleanup { Ok(_) => err, Err(cleanup) => format!("{err}; cleanup: {cleanup}") });
                            break;
                        }
                    }
                    sent
                }
            };
            match content {
                Ok(content) => json!({"content":content,"isError":false}),
                Err(err) => json!({"content":[{"type":"text","text":err}],"isError":true}),
            }
        }
        _ => return Some(error(id, -32601, "Method not found")),
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}).to_string())
}
