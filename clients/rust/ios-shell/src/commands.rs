use serde::Deserialize;
use tauri::State;

use crate::qa::{check_qa_provisioning, StartupResponse};
use crate::state::{build_session_config, AppState, SessionStats};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TouchEventPayload {
    pub id: u64,
    pub x: f32,
    pub y: f32,
    pub phase: String,
}

#[tauri::command]
pub async fn list_hosts(state: State<'_, AppState>) -> Result<Vec<erd_net::discovery::DiscoveredHost>, String> {
    state.list_discovered_hosts().await
}

#[tauri::command]
pub async fn stop_discovery(state: State<'_, AppState>) -> Result<(), String> {
    state.stop_discovery().await
}

#[tauri::command]
pub async fn connect(
    state: State<'_, AppState>,
    host: String,
    tcp_port: Option<u16>,
    udp_port: Option<u16>,
    pin: Option<String>,
) -> Result<SessionStats, String> {
    if let Some(ref p) = pin {
        let trimmed_pin = p.trim();
        if !trimmed_pin.is_empty() && (trimmed_pin.len() != 8 || !trimmed_pin.chars().all(|c| c.is_ascii_digit())) {
            return Err("PIN must be exactly 8 ASCII digits".to_string());
        }
    }
    let _ = build_session_config(&host, tcp_port, udp_port, "EclipticRD iOS")?;
    state.connect_async(host, tcp_port, udp_port, pin).await
}

#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
    state.disconnect_async().await
}

#[tauri::command]
pub fn stats(state: State<'_, AppState>) -> Result<SessionStats, String> {
    state.stats()
}

#[tauri::command]
pub fn poll_frame(state: State<'_, AppState>) -> Result<tauri::ipc::Response, String> {
    match state.poll_frame()? {
        Some(frame_bytes) => Ok(tauri::ipc::Response::new((*frame_bytes).clone())),
        None => Ok(tauri::ipc::Response::new(Vec::new())),
    }
}

#[tauri::command]
pub fn touch(state: State<'_, AppState>, event: TouchEventPayload) -> Result<(), String> {
    state.handle_touch(event.id, event.x, event.y, &event.phase)
}

#[tauri::command]
pub fn set_touch_mode(state: State<'_, AppState>, mode: String) -> Result<(), String> {
    state.set_touch_mode(&mode)
}

#[tauri::command]
pub fn send_key(
    state: State<'_, AppState>,
    key_code: u16,
    down: bool,
    modifiers: u16,
) -> Result<(), String> {
    state.handle_key(key_code, down, modifiers)
}

#[tauri::command]
pub fn set_muted(state: State<'_, AppState>, muted: bool) -> Result<(), String> {
    state.set_muted(muted)
}

#[tauri::command]
pub fn presented(state: State<'_, AppState>, sequence: u64) -> Result<(), String> {
    state.presented(sequence)
}

#[tauri::command]
pub fn startup() -> Result<StartupResponse, String> {
    Ok(check_qa_provisioning())
}
