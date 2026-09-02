use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use erd_app::{
    key_event, pointer_event, ClientSession, InputKey, PairingStore, ReadySession, SessionConfig,
    SessionError, SessionEvent, SessionRuntime, SessionState, DEFAULT_TCP_PORT, DEFAULT_UDP_PORT,
};
use erd_proto::{InputEvent, InputEventType, Modifiers};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectResponse {
    pub pairing_id: String,
    pub host_name: String,
    pub server_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionStats {
    pub connected: bool,
    pub state: String,
    pub frames_received: u64,
    pub frames_decoded: u64,
    pub audio_packets_received: u64,
    pub latency_p50_ms: Option<f64>,
    pub latency_p99_ms: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InputPayload {
    pub event_type: String,
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default)]
    pub view_width: f32,
    #[serde(default)]
    pub view_height: f32,
    #[serde(default)]
    pub key_code: Option<u16>,
    #[serde(default)]
    pub modifiers: u16,
    #[serde(default)]
    pub scroll_dx: f32,
    #[serde(default)]
    pub scroll_dy: f32,
}

#[derive(Default)]
pub struct LatencyTracker {
    samples: VecDeque<(Instant, f64)>,
}

impl LatencyTracker {
    pub fn record(&mut self, latency_ms: f64) {
        let now = Instant::now();
        self.samples.push_back((now, latency_ms));
        while self
            .samples
            .front()
            .is_some_and(|(t, _)| now.saturating_duration_since(*t) > Duration::from_secs(10))
        {
            self.samples.pop_front();
        }
    }

    pub fn percentiles(&self) -> (Option<f64>, Option<f64>) {
        if self.samples.is_empty() {
            return (None, None);
        }
        let mut vals: Vec<f64> = self.samples.iter().map(|(_, l)| *l).collect();
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let len = vals.len();
        let p50_idx = ((len as f64) * 0.50).floor() as usize;
        let p99_idx = ((len as f64) * 0.99).floor() as usize;
        let p50 = vals.get(p50_idx.min(len - 1)).copied();
        let p99 = vals.get(p99_idx.min(len - 1)).copied();
        (p50, p99)
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

pub struct AppState {
    pub session: Mutex<Option<ClientSession>>,
    pub tcp_runtime: Mutex<Option<SessionRuntime>>,
    pub stop_media_flag: Arc<AtomicBool>,
    pub media_handle: Mutex<Option<thread::JoinHandle<()>>>,
    pub frames_received: Arc<AtomicU64>,
    pub frames_decoded: Arc<AtomicU64>,
    pub audio_packets_received: Arc<AtomicU64>,
    pub latency: Arc<Mutex<LatencyTracker>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            session: Mutex::new(None),
            tcp_runtime: Mutex::new(None),
            stop_media_flag: Arc::new(AtomicBool::new(false)),
            media_handle: Mutex::new(None),
            frames_received: Arc::new(AtomicU64::new(0)),
            frames_decoded: Arc::new(AtomicU64::new(0)),
            audio_packets_received: Arc::new(AtomicU64::new(0)),
            latency: Arc::new(Mutex::new(LatencyTracker::default())),
        }
    }
}

impl AppState {
    pub fn clear_metrics(&self) {
        self.frames_received.store(0, Ordering::Relaxed);
        self.frames_decoded.store(0, Ordering::Relaxed);
        self.audio_packets_received.store(0, Ordering::Relaxed);
        if let Ok(mut lat) = self.latency.lock() {
            lat.clear();
        }
    }
}

pub fn convert_input_payload(event: &InputPayload) -> Result<InputEvent, String> {
    let event_type = match event.event_type.as_str() {
        "MouseMove" | "mousemove" | "0" => InputEventType::MouseMove,
        "LeftMouseDown" | "leftmousedown" | "1" => InputEventType::LeftMouseDown,
        "LeftMouseUp" | "leftmouseup" | "2" => InputEventType::LeftMouseUp,
        "RightMouseDown" | "rightmousedown" | "3" => InputEventType::RightMouseDown,
        "RightMouseUp" | "rightmouseup" | "4" => InputEventType::RightMouseUp,
        "ScrollWheel" | "scrollwheel" | "5" => InputEventType::ScrollWheel,
        "KeyDown" | "keydown" | "6" => InputEventType::KeyDown,
        "KeyUp" | "keyup" | "7" => InputEventType::KeyUp,
        "FlagsChanged" | "flagschanged" | "8" => InputEventType::FlagsChanged,
        "LeftMouseDragged" | "leftmousedragged" | "9" => InputEventType::LeftMouseDragged,
        "RightMouseDragged" | "rightmousedragged" | "10" => InputEventType::RightMouseDragged,
        other => return Err(format!("Unknown event type: {other}")),
    };

    let modifiers = Modifiers::from_bits_retain(event.modifiers);

    match event_type {
        InputEventType::KeyDown | InputEventType::KeyUp => {
            let key_code = event.key_code.unwrap_or(0);
            key_event(event_type, InputKey::WindowsVirtualKey(key_code), modifiers)
                .ok_or_else(|| format!("Unsupported key code: {key_code}"))
        }
        _ => {
            let vw = if event.view_width > 0.0 {
                event.view_width
            } else {
                1280.0
            };
            let vh = if event.view_height > 0.0 {
                event.view_height
            } else {
                800.0
            };
            pointer_event(
                event_type,
                event.x,
                event.y,
                vw,
                vh,
                modifiers,
                event.scroll_dx,
                event.scroll_dy,
            )
            .ok_or_else(|| "Invalid pointer coordinates".to_string())
        }
    }
}

pub mod commands {
    use super::*;

    #[tauri::command]
    pub async fn connect(
        state: State<'_, AppState>,
        host: String,
        tcp_port: Option<u16>,
        udp_port: Option<u16>,
        pin: Option<String>,
    ) -> Result<ConnectResponse, String> {
        // Disconnect previous session if any
        let _ = disconnect_internal(&state);
        state.clear_metrics();

        let tcp = tcp_port.unwrap_or(DEFAULT_TCP_PORT);
        let udp = udp_port.unwrap_or(DEFAULT_UDP_PORT);

        let mut config = SessionConfig::direct(host, "EclipticRD-Tauri");
        config.tcp_port = tcp;
        config.udp_port = udp;

        let session = ClientSession::new(config).map_err(|e| format!("Failed to init session: {e}"))?;

        // Connect & pair/reconnect in a background blocking task
        let session_for_connect = session.clone();
        let pin_clone = pin.clone();
        let ready_session: ReadySession = tokio::task::spawn_blocking(move || -> Result<ReadySession, String> {
            if let Some(ref pin_str) = pin_clone {
                if !pin_str.trim().is_empty() {
                    return session_for_connect
                        .pair_with_pin(pin_str.trim())
                        .map_err(|e| format!("Pairing error: {e}"));
                }
            }
            // If no PIN provided, attempt to load the latest paired record from store
            let store = PairingStore::open_default().map_err(|e| format!("Pairing store error: {e}"))?;
            let records = store.load_all().map_err(|e| format!("Load pairings error: {e}"))?;
            if let Some(record) = records.last() {
                session_for_connect
                    .reconnect(&record.id)
                    .map_err(|e| format!("Reconnect error: {e}"))
            } else {
                Err("No PIN provided and no previous pairing found in store".to_string())
            }
        })
        .await
        .map_err(|e| format!("Tokio join error: {e}"))??;

        // Spawn TCP control runtime
        let tcp_runtime = session
            .spawn_tcp_runtime()
            .map_err(|e| format!("Failed to spawn TCP runtime: {e}"))?;

        // Spawn dedicated OS thread for the media pipeline (no main-thread blocking)
        let stop_flag = Arc::new(AtomicBool::new(false));
        state.stop_media_flag.store(false, Ordering::SeqCst);
        let stop_flag_thread = stop_flag.clone();
        let session_udp = session.clone();
        let frames_rx = state.frames_received.clone();
        let frames_dec = state.frames_decoded.clone();
        let audio_rx = state.audio_packets_received.clone();
        let latency_tracker = state.latency.clone();

        let media_thread = thread::Builder::new()
            .name("erd-media-pipeline".to_string())
            .spawn(move || {
                while !stop_flag_thread.load(Ordering::Relaxed) {
                    match session_udp.receive_udp_event() {
                        Ok(SessionEvent::Frame(frame)) => {
                            frames_rx.fetch_add(1, Ordering::Relaxed);
                            frames_dec.fetch_add(1, Ordering::Relaxed);

                            let now_ms = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u32;
                            if now_ms >= frame.timestamp_ms && frame.timestamp_ms > 0 {
                                let latency_ms = (now_ms - frame.timestamp_ms) as f64;
                                if latency_ms < 5000.0 {
                                    if let Ok(mut lat) = latency_tracker.lock() {
                                        lat.record(latency_ms);
                                    }
                                }
                            }
                        }
                        Ok(SessionEvent::Audio(_)) => {
                            audio_rx.fetch_add(1, Ordering::Relaxed);
                        }
                        Ok(SessionEvent::Cursor(_)) => {}
                        Ok(SessionEvent::Ping) | Ok(SessionEvent::Clipboard(_)) | Ok(SessionEvent::Ignored) => {}
                        Err(SessionError::Io(e))
                            if e.kind() == std::io::ErrorKind::TimedOut
                                || e.kind() == std::io::ErrorKind::WouldBlock =>
                        {
                            // Socket timeout slice, loop back to check stop_flag
                        }
                        Err(SessionError::NotReady) => {
                            break;
                        }
                        Err(e) => {
                            tracing::debug!("UDP media pipeline event error: {e}");
                            if stop_flag_thread.load(Ordering::Relaxed) {
                                break;
                            }
                            thread::sleep(Duration::from_millis(5));
                        }
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn media thread: {e}"))?;

        if let Ok(mut sess_lock) = state.session.lock() {
            *sess_lock = Some(session);
        }
        if let Ok(mut tcp_lock) = state.tcp_runtime.lock() {
            *tcp_lock = Some(tcp_runtime);
        }
        if let Ok(mut media_lock) = state.media_handle.lock() {
            *media_lock = Some(media_thread);
        }

        Ok(ConnectResponse {
            pairing_id: ready_session.pairing.id,
            host_name: ready_session.pairing.name,
            server_name: ready_session.server.name,
        })
    }

    #[tauri::command]
    pub fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
        disconnect_internal(&state)
    }

    #[tauri::command]
    pub fn stats(state: State<'_, AppState>) -> Result<SessionStats, String> {
        let (connected, state_str) = state
            .session
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            .map(|s| {
                let st = s.state().unwrap_or(SessionState::Disconnected);
                (st == SessionState::Ready, format!("{st:?}"))
            })
            .unwrap_or((false, "Disconnected".to_string()));

        let (latency_p50_ms, latency_p99_ms) = state
            .latency
            .lock()
            .map(|lat| lat.percentiles())
            .unwrap_or((None, None));

        Ok(SessionStats {
            connected,
            state: state_str,
            frames_received: state.frames_received.load(Ordering::Relaxed),
            frames_decoded: state.frames_decoded.load(Ordering::Relaxed),
            audio_packets_received: state.audio_packets_received.load(Ordering::Relaxed),
            latency_p50_ms,
            latency_p99_ms,
        })
    }

    #[tauri::command]
    pub fn send_input(state: State<'_, AppState>, event: InputPayload) -> Result<(), String> {
        let sess_opt = state.session.lock().map_err(|e| e.to_string())?.clone();
        let session = sess_opt.ok_or_else(|| "Session not initialized".to_string())?;

        let input_event = convert_input_payload(&event)?;

        session
            .send_input(input_event)
            .map_err(|e| format!("Failed to send input: {e}"))
    }
}

fn disconnect_internal(state: &AppState) -> Result<(), String> {
    state.stop_media_flag.store(true, Ordering::SeqCst);

    if let Ok(mut tcp_lock) = state.tcp_runtime.lock() {
        if let Some(mut tcp) = tcp_lock.take() {
            tcp.stop();
        }
    }

    if let Ok(mut sess_lock) = state.session.lock() {
        if let Some(sess) = sess_lock.take() {
            let _ = sess.disconnect();
        }
    }

    if let Ok(mut media_lock) = state.media_handle.lock() {
        if let Some(handle) = media_lock.take() {
            let _ = handle.join();
        }
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::connect,
            commands::disconnect,
            commands::stats,
            commands::send_input
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_tracker_calculates_percentiles() {
        let mut tracker = LatencyTracker::default();
        assert_eq!(tracker.percentiles(), (None, None));

        for i in 1..=100 {
            tracker.record(i as f64);
        }

        let (p50, p99) = tracker.percentiles();
        assert_eq!(p50, Some(51.0));
        assert_eq!(p99, Some(100.0));

        tracker.clear();
        assert_eq!(tracker.percentiles(), (None, None));
    }

    #[test]
    fn convert_pointer_input_payload() {
        let payload = InputPayload {
            event_type: "MouseMove".to_string(),
            x: 640.0,
            y: 400.0,
            view_width: 1280.0,
            view_height: 800.0,
            key_code: None,
            modifiers: 0,
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        };
        let event = convert_input_payload(&payload).unwrap();
        assert_eq!(event.event_type, InputEventType::MouseMove);
        assert!((event.x - 0.5).abs() < 1e-4);
        assert!((event.y - 0.5).abs() < 1e-4);
    }

    #[test]
    fn convert_keyboard_input_payload() {
        let payload = InputPayload {
            event_type: "KeyDown".to_string(),
            x: 0.0,
            y: 0.0,
            view_width: 0.0,
            view_height: 0.0,
            key_code: Some(0x41), // 'A' VK
            modifiers: Modifiers::COMMAND.bits(),
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        };
        let event = convert_input_payload(&payload).unwrap();
        assert_eq!(event.event_type, InputEventType::KeyDown);
        assert_eq!(event.key_code, 0x00); // mapped to macOS key code
        assert!(event.modifiers.contains(Modifiers::COMMAND));
    }

    #[test]
    fn app_state_lifecycle() {
        let state = AppState::default();
        state.frames_received.store(42, Ordering::Relaxed);
        state.frames_decoded.store(42, Ordering::Relaxed);
        state.clear_metrics();
        assert_eq!(state.frames_received.load(Ordering::Relaxed), 0);
        assert_eq!(state.frames_decoded.load(Ordering::Relaxed), 0);
        assert!(disconnect_internal(&state).is_ok());
    }
}
