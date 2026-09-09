use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::{agent_input::InputStateTracker, agent_server::AgentServerBackend};

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

#[path = "mcp_dispatch.rs"]
mod dispatch;
pub use dispatch::handle_mcp_message;

#[path = "mcp_stdio.rs"]
mod stdio;
pub use stdio::run as run_mcp_stdio_until;

pub async fn run_mcp_stdio(backend: Arc<dyn AgentServerBackend>) -> std::io::Result<()> {
    let (_stop, receiver) = tokio::sync::watch::channel(false);
    run_mcp_stdio_until(backend, receiver).await
}

pub async fn run_mcp_io(
    backend: Arc<dyn AgentServerBackend>,
    stdin: impl tokio::io::AsyncRead + Unpin,
    stdout: impl tokio::io::AsyncWrite + Unpin,
) -> std::io::Result<()> {
    let (_stop, receiver) = tokio::sync::watch::channel(false);
    run_mcp_io_until(backend, (stdin, stdout), receiver).await
}

pub async fn run_mcp_io_until(
    backend: Arc<dyn AgentServerBackend>,
    (stdin, mut stdout): (
        impl tokio::io::AsyncRead + Unpin,
        impl tokio::io::AsyncWrite + Unpin,
    ),
    mut stop: tokio::sync::watch::Receiver<bool>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    let mut tracker = InputStateTracker::default();
    let mut current_pos = (0.5, 0.5);

    let work = async {
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

        Ok::<(), std::io::Error>(())
    };
    let result = tokio::select! {
        biased;
        _ = stop.wait_for(|stopped| *stopped) => Ok(()),
        result = work => result,
    };
    let cleanup = dispatch::release(backend.as_ref(), &mut tracker, current_pos)
        .map_err(std::io::Error::other);
    match (result, cleanup) {
        (Ok(()), result) => result.map(|_| ()),
        (result, Ok(_)) => result,
        (Err(error), Err(cleanup)) => Err(std::io::Error::other(format!(
            "{error}; cleanup: {cleanup}"
        ))),
    }
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
