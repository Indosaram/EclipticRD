use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use erd_app::{
    agent_input::{
        convert_agent_action_to_events, encode_nv12_screenshot, AgentAction, InputStateTracker,
        ScreenInfo, ScreenshotFormat,
    },
    key_event, pointer_event, ClientSession, CursorState, InputKey, PairingStore, ReadySession,
    SessionConfig, SessionError, SessionEvent, SessionRuntime, SessionState, DEFAULT_TCP_PORT,
    DEFAULT_UDP_PORT,
};
use erd_decode::HevcDecoder;
use erd_proto::{InputEvent, InputEventType, Modifiers};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostItem {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub os: String,
    pub online: bool,
    pub paired: bool,
    pub last_seen: Option<String>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFramePayload {
    pub width: u32,
    pub height: u32,
    pub timestamp_ms: i64,
    pub jpeg_base64: String,
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

#[derive(Clone, Default)]
pub struct RawNv12Payload {
    pub width: u32,
    pub height: u32,
    pub timestamp_ms: i64,
    pub buffer: Vec<u8>,
}

#[derive(Default)]
pub struct FrameMailbox {
    latest: Option<Arc<RawNv12Payload>>,
    pending_display: bool,
}

impl FrameMailbox {
    fn publish(&mut self, payload: RawNv12Payload) {
        self.latest = Some(Arc::new(payload));
        self.pending_display = true;
    }

    fn take_snapshot(&mut self) -> Option<Arc<RawNv12Payload>> {
        if std::mem::take(&mut self.pending_display) {
            self.latest.clone()
        } else {
            None
        }
    }
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
    lifecycle: tokio::sync::Mutex<()>,
    pub session: Mutex<Option<ClientSession>>,
    pub tcp_runtime: Mutex<Option<SessionRuntime>>,
    pub stop_media_flag: Arc<AtomicBool>,
    pub media_handle: Mutex<Option<thread::JoinHandle<()>>>,
    pub frames_received: Arc<AtomicU64>,
    pub frames_decoded: Arc<AtomicU64>,
    pub audio_packets_received: Arc<AtomicU64>,
    pub latency: Arc<Mutex<LatencyTracker>>,
    pub latest_raw_frame: Arc<Mutex<FrameMailbox>>,
    pub latest_cursor: Arc<Mutex<CursorState>>,
    pub agent_tracker: Arc<Mutex<InputStateTracker>>,
    pub agent_pos: Arc<Mutex<(f32, f32)>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            lifecycle: tokio::sync::Mutex::new(()),
            session: Mutex::new(None),
            tcp_runtime: Mutex::new(None),
            stop_media_flag: Arc::new(AtomicBool::new(false)),
            media_handle: Mutex::new(None),
            frames_received: Arc::new(AtomicU64::new(0)),
            frames_decoded: Arc::new(AtomicU64::new(0)),
            audio_packets_received: Arc::new(AtomicU64::new(0)),
            latency: Arc::new(Mutex::new(LatencyTracker::default())),
            latest_raw_frame: Arc::new(Mutex::new(FrameMailbox::default())),
            latest_cursor: Arc::new(Mutex::new(CursorState::default())),
            agent_tracker: Arc::new(Mutex::new(InputStateTracker::default())),
            agent_pos: Arc::new(Mutex::new((0.5, 0.5))),
        }
    }
}

impl AppState {
    fn worker_stop_flag(&self) -> Arc<AtomicBool> {
        self.stop_media_flag.store(false, Ordering::SeqCst);
        self.stop_media_flag.clone()
    }

    #[cfg(test)]
    fn publish_frame(&self, payload: RawNv12Payload) {
        self.latest_raw_frame.lock().unwrap().publish(payload);
    }

    #[cfg(test)]
    fn take_display_frame(&self) -> Vec<u8> {
        let frame = {
            let mut mailbox = self
                .latest_raw_frame
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            mailbox.take_snapshot()
        };
        frame.map(|p| p.buffer.clone()).unwrap_or_default()
    }

    pub fn clear_metrics(&self) {
        self.frames_received.store(0, Ordering::Relaxed);
        self.frames_decoded.store(0, Ordering::Relaxed);
        self.audio_packets_received.store(0, Ordering::Relaxed);
        if let Ok(mut lat) = self.latency.lock() {
            lat.clear();
        }
        if let Ok(mut frame) = self.latest_raw_frame.lock() {
            *frame = FrameMailbox::default();
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
    pub async fn list_hosts() -> Result<Vec<HostItem>, String> {
        let store =
            PairingStore::open_default().map_err(|e| format!("Pairing store error: {e}"))?;
        let records = store.load_all().unwrap_or_default();

        let mut hosts: Vec<HostItem> = Vec::new();

        // 1. Probe Tailscale peers if available
        let mut found_ips = std::collections::HashSet::new();
        if let Ok(output) =
            std::process::Command::new("/Applications/Tailscale.app/Contents/MacOS/Tailscale")
                .args(["status", "--json"])
                .output()
        {
            if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                if let Some(peers) = val.get("Peer").and_then(|p| p.as_object()) {
                    for (_k, v) in peers {
                        let hostname = v
                            .get("HostName")
                            .and_then(|h| h.as_str())
                            .unwrap_or("Unknown")
                            .to_string();
                        let os = v
                            .get("OS")
                            .and_then(|o| o.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let online = v.get("Online").and_then(|o| o.as_bool()).unwrap_or(false);
                        let ip = v
                            .get("TailscaleIPs")
                            .and_then(|ips| ips.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|ip| ip.as_str())
                            .unwrap_or("")
                            .to_string();

                        if !ip.is_empty() {
                            found_ips.insert(ip.clone());
                            let paired = records.iter().any(|r| {
                                r.name.eq_ignore_ascii_case(&hostname)
                                    || (ip == "100.91.254.71"
                                        && r.name.eq_ignore_ascii_case("indo"))
                                    || (ip == "100.126.171.58" && r.name.contains("DESKTOP"))
                            });

                            let id = records
                                .iter()
                                .rev()
                                .find(|r| {
                                    r.name.eq_ignore_ascii_case(&hostname)
                                        || (ip == "100.91.254.71"
                                            && r.name.eq_ignore_ascii_case("indo"))
                                        || (ip == "100.126.171.58" && r.name.contains("DESKTOP"))
                                })
                                .map(|r| r.id.clone())
                                .unwrap_or_else(|| ip.clone());

                            hosts.push(HostItem {
                                id,
                                name: hostname,
                                ip,
                                os,
                                online,
                                paired,
                                last_seen: None,
                            });
                        }
                    }
                }
            }
        }

        // Add default known testbeds if not already discovered
        let defaults = [
            ("indo", "100.91.254.71", "linux"),
            ("DESKTOP-1LAPJMP", "100.126.171.58", "windows"),
        ];

        for (d_name, d_ip, d_os) in defaults {
            if !found_ips.contains(d_ip) {
                let paired = records.iter().any(|r| r.name.eq_ignore_ascii_case(d_name));
                let id = records
                    .iter()
                    .rev()
                    .find(|r| r.name.eq_ignore_ascii_case(d_name))
                    .map(|r| r.id.clone())
                    .unwrap_or_else(|| d_ip.to_string());
                hosts.push(HostItem {
                    id,
                    name: d_name.to_string(),
                    ip: d_ip.to_string(),
                    os: d_os.to_string(),
                    online: true,
                    paired,
                    last_seen: None,
                });
            }
        }

        Ok(hosts)
    }

    #[tauri::command]
    pub async fn connect(
        state: State<'_, AppState>,
        host: String,
        tcp_port: Option<u16>,
        udp_port: Option<u16>,
        pin: Option<String>,
    ) -> Result<ConnectResponse, String> {
        let _lifecycle = state.lifecycle.lock().await;
        disconnect_internal(&state).await?;
        state.clear_metrics();

        let tcp = tcp_port.unwrap_or(DEFAULT_TCP_PORT);
        let udp = udp_port.unwrap_or(DEFAULT_UDP_PORT);

        let mut config = SessionConfig::direct(host.clone(), "EclipticRD-Tauri");
        config.tcp_port = tcp;
        config.udp_port = udp;

        let session =
            ClientSession::new(config).map_err(|e| format!("Failed to init session: {e}"))?;

        let session_for_connect = session.clone();
        let pin_clone = pin.clone();
        let host_for_connect = host.clone();
        let ready_session: ReadySession =
            tokio::task::spawn_blocking(move || -> Result<ReadySession, String> {
                if let Some(ref pin_str) = pin_clone {
                    if !pin_str.trim().is_empty() {
                        return session_for_connect
                            .pair_with_pin(pin_str.trim())
                            .map_err(|e| format!("Pairing error: {e}"));
                    }
                }
                let store = PairingStore::open_default()
                    .map_err(|e| format!("Pairing store error: {e}"))?;
                let records = store
                    .load_all()
                    .map_err(|e| format!("Load pairings error: {e}"))?;

                let matched = records.iter().rev().find(|r| {
                    host_for_connect.eq_ignore_ascii_case(&r.name)
                        || host_for_connect.starts_with(&r.name)
                        || (host_for_connect == "100.91.254.71" && r.name == "indo")
                        || (host_for_connect == "100.126.171.58" && r.name.contains("DESKTOP"))
                });

                if let Some(record) = matched {
                    session_for_connect
                        .reconnect(&record.id)
                        .map_err(|e| format!("Reconnect error: {e}"))
                } else {
                    Err("No PIN provided and no previous pairing found in store".to_string())
                }
            })
            .await
            .map_err(|e| format!("Tokio join error: {e}"))??;

        let tcp_runtime = session
            .spawn_tcp_runtime()
            .map_err(|e| format!("Failed to spawn TCP runtime: {e}"))?;

        // Ask for an immediate keyframe so the canvas paints as soon as the
        // host's first (possibly keepalive) frame arrives.
        let _ = session.send_control(erd_proto::ControlMessage::RequestKeyFrame);

        let stop_flag = state.worker_stop_flag();
        let stop_flag_thread = stop_flag.clone();
        let session_udp = session.clone();
        let frames_rx = state.frames_received.clone();
        let frames_dec = state.frames_decoded.clone();
        let audio_rx = state.audio_packets_received.clone();
        let latency_tracker = state.latency.clone();
        let latest_frame_target = state.latest_raw_frame.clone();
        let latest_cursor_target = state.latest_cursor.clone();

        let media_thread = thread::Builder::new()
            .name("erd-media-pipeline".to_string())
            .spawn(move || {
                let mut decoder: Option<HevcDecoder> = None;

                while !stop_flag_thread.load(Ordering::Relaxed) {
                    match session_udp.receive_udp_event() {
                        Ok(SessionEvent::Frame(frame)) => {
                            frames_rx.fetch_add(1, Ordering::Relaxed);
                            let frame_recv_instant = Instant::now();

                            if decoder.is_none() {
                                if let Ok((_kind, dec)) =
                                    HevcDecoder::from_keyframe_auto(&frame.data)
                                {
                                    decoder = Some(dec);
                                }
                            }

                            if let Some(ref mut dec) = decoder {
                                if let Ok(nv12_frames) =
                                    dec.decode(&frame.data, frame.timestamp_ms as i64)
                                {
                                    for nv12 in nv12_frames {
                                        frames_dec.fetch_add(1, Ordering::Relaxed);

                                        let latency_ms =
                                            frame_recv_instant.elapsed().as_secs_f64() * 1000.0;
                                        if let Ok(mut lat) = latency_tracker.lock() {
                                            lat.record(latency_ms);
                                        }

                                        let width = nv12.width as usize;
                                        let height = nv12.height as usize;
                                        let y_len = width * height;
                                        let uv_len = width * (height / 2);
                                        let total_bytes = 16 + y_len + uv_len + 9;

                                        let mut buffer = Vec::with_capacity(total_bytes);
                                        buffer.extend_from_slice(&nv12.width.to_le_bytes());
                                        buffer.extend_from_slice(&nv12.height.to_le_bytes());
                                        buffer.extend_from_slice(&nv12.timestamp_ms.to_le_bytes());

                                        if nv12.y_plane.len() >= y_len {
                                            buffer.extend_from_slice(&nv12.y_plane[..y_len]);
                                        } else {
                                            buffer.extend_from_slice(&nv12.y_plane);
                                            buffer.resize(16 + y_len, 0);
                                        }

                                        if nv12.uv_plane.len() >= uv_len {
                                            buffer.extend_from_slice(&nv12.uv_plane[..uv_len]);
                                        } else {
                                            buffer.extend_from_slice(&nv12.uv_plane);
                                            buffer.resize(16 + y_len + uv_len, 128);
                                        }

                                        let cursor = latest_cursor_target
                                            .lock()
                                            .map(|c| *c)
                                            .unwrap_or_default();
                                        buffer.extend_from_slice(&cursor.x.to_le_bytes());
                                        buffer.extend_from_slice(&cursor.y.to_le_bytes());
                                        buffer.push(cursor.cursor_type);

                                        let payload = RawNv12Payload {
                                            width: nv12.width,
                                            height: nv12.height,
                                            timestamp_ms: nv12.timestamp_ms,
                                            buffer,
                                        };

                                        if let Ok(mut frame_target) = latest_frame_target.lock() {
                                            frame_target.publish(payload);
                                        }
                                    }
                                }
                            }
                        }
                        Ok(SessionEvent::Audio(_)) => {
                            audio_rx.fetch_add(1, Ordering::Relaxed);
                        }
                        Ok(SessionEvent::Cursor(cursor)) => {
                            if let Ok(mut c) = latest_cursor_target.lock() {
                                *c = cursor;
                            }
                        }
                        Ok(SessionEvent::Ping)
                        | Ok(SessionEvent::Clipboard(_))
                        | Ok(SessionEvent::Ignored) => {}
                        Err(SessionError::Io(e))
                            if e.kind() == std::io::ErrorKind::TimedOut
                                || e.kind() == std::io::ErrorKind::WouldBlock => {}
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
    pub fn get_cursor_position(state: State<'_, AppState>) -> Result<CursorState, String> {
        state
            .latest_cursor
            .lock()
            .map(|c| *c)
            .map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub async fn poll_frame_raw(
        state: State<'_, AppState>,
    ) -> Result<tauri::ipc::Response, String> {
        poll_frame_with_clipboard(&state, |text| {
            #[cfg(target_os = "macos")]
            {
                use erd_app::PlatformClipboard;
                erd_app::platform::SystemClipboard
                    .set_text(text)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = text;
                Err("System clipboard is unavailable on this platform".to_string())
            }
        })
        .await
        .map(tauri::ipc::Response::new)
    }

    pub(super) async fn poll_frame_with_clipboard(
        state: &AppState,
        mut apply_clipboard: impl FnMut(&str) -> Result<(), String> + Send + 'static,
    ) -> Result<Vec<u8>, String> {
        // Keep this session's clipboard work ahead of teardown/reconnect. No
        // old worker may apply clipboard content after a new session starts.
        let _lifecycle = state.lifecycle.lock().await;
        let events = state
            .tcp_runtime
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            .map(|runtime| runtime.events().clone());
        let frames = state.latest_raw_frame.clone();
        tokio::task::spawn_blocking(move || {
            let mut clipboard = None;
            let mut errors = Vec::new();
            if let Some(events) = events {
                // Three bounded slots plus the empty/closed observation. Bound
                // work even if the producer refills while this command runs.
                for _ in 0..4 {
                    match events.try_recv() {
                        Ok(Ok(SessionEvent::Clipboard(text))) => clipboard = Some(text),
                        Ok(Ok(
                            SessionEvent::Ping
                            | SessionEvent::Ignored
                            | SessionEvent::Frame(_)
                            | SessionEvent::Audio(_)
                            | SessionEvent::Cursor(_),
                        )) => {}
                        Ok(Err(error)) => {
                            errors.push(error.to_string());
                            break;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            errors.push("TCP runtime disconnected".to_string());
                            break;
                        }
                    }
                }
            }
            if let Some(text) = clipboard {
                if let Err(error) = apply_clipboard(&text) {
                    errors.push(error);
                }
            }
            if !errors.is_empty() {
                return Err(errors.join("\n"));
            }
            let frame = frames.lock().map_err(|e| e.to_string())?.take_snapshot();
            Ok(frame.map(|p| p.buffer.clone()).unwrap_or_default())
        })
        .await
        .map_err(|e| format!("Frame event worker failed: {e}"))?
    }

    #[tauri::command]
    pub async fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
        let _lifecycle = state.lifecycle.lock().await;
        disconnect_internal(&state).await
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
        if state.stop_media_flag.load(Ordering::SeqCst) {
            return Err("Session disconnecting or inactive".to_string());
        }
        let sess_opt = state.session.lock().map_err(|e| e.to_string())?.clone();
        let session = match sess_opt {
            Some(s) => s,
            None => {
                return Err("Session not initialized".to_string());
            }
        };

        let input_event = match convert_input_payload(&event) {
            Ok(evt) => evt,
            Err(e) => {
                return Err(e);
            }
        };

        session
            .send_input(input_event)
            .map_err(|e| format!("Failed to send input: {e}"))
    }

    #[tauri::command]
    pub fn agent_execute_action(
        state: State<'_, AppState>,
        action: AgentAction,
    ) -> Result<usize, String> {
        let sess_opt = state.session.lock().map_err(|e| e.to_string())?.clone();
        let session = match sess_opt {
            Some(s) => s,
            None => return Err("Session not active".to_string()),
        };

        let info = screen_info(&state)?;
        let (width, height) = (info.width as f32, info.height as f32);

        let mut tracker = state.agent_tracker.lock().map_err(|e| e.to_string())?;
        let mut pos = state.agent_pos.lock().map_err(|e| e.to_string())?;

        let events = convert_agent_action_to_events(&action, &mut tracker, &mut pos, width, height)
            .map_err(|e| e.to_string())?;

        let count = events.len();
        for event in events {
            session.send_input(event).map_err(|e| e.to_string())?;
        }
        Ok(count)
    }

    #[tauri::command]
    pub fn agent_get_screen_info(state: State<'_, AppState>) -> Result<ScreenInfo, String> {
        screen_info(&state)
    }

    pub(super) fn screen_info(state: &AppState) -> Result<ScreenInfo, String> {
        let lock = state.latest_raw_frame.lock().map_err(|e| e.to_string())?;
        if let Some(frame) = lock.latest.as_ref() {
            Ok(ScreenInfo {
                width: frame.width,
                height: frame.height,
                scale: 1.0,
                connected_host: "remote-host".to_string(),
            })
        } else {
            Ok(ScreenInfo {
                width: 1920,
                height: 1080,
                scale: 1.0,
                connected_host: "unconnected".to_string(),
            })
        }
    }

    #[tauri::command]
    pub async fn agent_capture_screen(
        state: State<'_, AppState>,
        format: Option<String>,
    ) -> Result<String, String> {
        capture_with_job(&state, move |frame| encode_capture(frame, format)).await
    }

    pub(super) async fn capture_with_job(
        state: &AppState,
        encode: impl FnOnce(Arc<RawNv12Payload>) -> Result<String, String> + Send + 'static,
    ) -> Result<String, String> {
        let snapshot = state
            .latest_raw_frame
            .lock()
            .map_err(|e| e.to_string())?
            .latest
            .clone()
            .ok_or_else(|| "No active frame received yet".to_string())?;
        tokio::task::spawn_blocking(move || encode(snapshot))
            .await
            .map_err(|e| format!("Screenshot worker failed: {e}"))?
    }

    #[cfg(test)]
    pub(super) fn capture_screen(
        state: &AppState,
        format: Option<String>,
    ) -> Result<String, String> {
        let snapshot = state
            .latest_raw_frame
            .lock()
            .map_err(|e| e.to_string())?
            .latest
            .clone();
        if let Some(frame) = snapshot {
            encode_capture(frame, format)
        } else {
            Err("No active frame received yet".to_string())
        }
    }

    fn encode_capture(
        frame: Arc<RawNv12Payload>,
        format: Option<String>,
    ) -> Result<String, String> {
        let fmt = if format.as_deref() == Some("jpeg") {
            ScreenshotFormat::Jpeg
        } else {
            ScreenshotFormat::Png
        };
        encode_nv12_screenshot(frame.width, frame.height, &frame.buffer[16..], fmt)
    }

    #[tauri::command]
    pub fn agent_release_all(state: State<'_, AppState>) -> Result<usize, String> {
        let sess_opt = state.session.lock().map_err(|e| e.to_string())?.clone();
        let session = match sess_opt {
            Some(s) => s,
            None => return Err("Session not active".to_string()),
        };

        let mut tracker = state.agent_tracker.lock().map_err(|e| e.to_string())?;
        let pos = state.agent_pos.lock().map_err(|e| e.to_string())?;
        let events = tracker.release_all(pos.0, pos.1);
        let count = events.len();
        for event in events {
            session.send_input(event).map_err(|e| e.to_string())?;
        }
        Ok(count)
    }
}

async fn disconnect_internal(state: &AppState) -> Result<(), String> {
    disconnect_with_stop(state, SessionRuntime::stop).await
}

async fn disconnect_with_stop(
    state: &AppState,
    stop_tcp: impl FnOnce(&mut SessionRuntime) -> Result<(), SessionError> + Send + 'static,
) -> Result<(), String> {
    state.stop_media_flag.store(true, Ordering::SeqCst);
    let session = state.session.lock().map_err(|e| e.to_string())?.take();
    let tcp = state.tcp_runtime.lock().map_err(|e| e.to_string())?.take();
    let media = state.media_handle.lock().map_err(|e| e.to_string())?.take();
    let result = tokio::task::spawn_blocking(move || {
        // Disconnect wakes transport reads before either worker is joined.
        let disconnected = session
            .map(|s| s.disconnect())
            .transpose()
            .map_err(|e| e.to_string());
        let stopped = tcp
            .map(|mut tcp| stop_tcp(&mut tcp))
            .transpose()
            .map_err(|e| e.to_string());
        let joined = media
            .map(|handle| handle.join())
            .transpose()
            .map_err(|_| "Media worker panicked".to_string());
        let errors: Vec<_> = [
            disconnected.map(|_| ()),
            stopped.map(|_| ()),
            joined.map(|_| ()),
        ]
        .into_iter()
        .filter_map(Result::err)
        .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    })
    .await
    .map_err(|e| format!("Disconnect worker failed: {e}"));
    state.clear_metrics();
    result?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::list_hosts,
            commands::connect,
            commands::disconnect,
            commands::stats,
            commands::send_input,
            commands::agent_execute_action,
            commands::agent_get_screen_info,
            commands::agent_capture_screen,
            commands::agent_release_all,
            commands::get_cursor_position,
            commands::poll_frame_raw
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
#[path = "mailbox_tests.rs"]
mod mailbox_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn distinctive_frame() -> RawNv12Payload {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&2u32.to_le_bytes());
        buffer.extend_from_slice(&2u32.to_le_bytes());
        buffer.extend_from_slice(&123i64.to_le_bytes());
        buffer.extend_from_slice(&[40, 80, 120, 160, 90, 200]);
        RawNv12Payload {
            width: 2,
            height: 2,
            timestamp_ms: 123,
            buffer,
        }
    }

    fn decoded_capture(state: &AppState) -> image::RgbImage {
        let encoded = commands::capture_screen(state, None).unwrap();
        // Decode the command's base64 wire value without adding a dependency.
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut bits = 0u32;
        let mut count = 0;
        let mut bytes = Vec::new();
        for c in encoded.bytes().take_while(|c| *c != b'=') {
            bits = (bits << 6) | alphabet.iter().position(|v| *v == c).unwrap() as u32;
            count += 6;
            if count >= 8 {
                count -= 8;
                bytes.push((bits >> count) as u8);
            }
        }
        image::load_from_memory(&bytes).unwrap().to_rgb8()
    }

    #[test]
    fn poll_retains_agent_dimensions_and_capture() {
        let state = AppState::default();
        state.publish_frame(distinctive_frame());
        assert_eq!(state.take_display_frame(), distinctive_frame().buffer);
        assert!(state.take_display_frame().is_empty());
        let info = commands::screen_info(&state).unwrap();
        assert_eq!((info.width, info.height), (2, 2));
        assert_eq!(decoded_capture(&state).dimensions(), (2, 2));
    }

    #[test]
    fn screenshot_excludes_ipc_header_exact_pixels() {
        let state = AppState::default();
        state.publish_frame(distinctive_frame());
        assert_eq!(
            decoded_capture(&state).as_raw(),
            &[153, 13, 0, 193, 53, 9, 233, 93, 49, 255, 133, 89,]
        );
    }

    #[test]
    fn media_worker_uses_shared_stop_token() {
        let state = AppState::default();
        let worker_flag = state.worker_stop_flag();
        state.stop_media_flag.store(true, Ordering::SeqCst);
        assert!(
            worker_flag.load(Ordering::SeqCst),
            "worker must observe teardown cancellation"
        );
        assert!(Arc::ptr_eq(&worker_flag, &state.stop_media_flag));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn screenshot_encoding_leaves_executor_and_mailbox_responsive() {
        let state = Arc::new(AppState::default());
        state.publish_frame(distinctive_frame());
        let executor_thread = thread::current().id();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let capture_state = state.clone();
        let capture = tokio::spawn(async move {
            commands::capture_with_job(&capture_state, move |frame| {
                assert_ne!(
                    thread::current().id(),
                    executor_thread,
                    "encoding must leave executor"
                );
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                encode_nv12_screenshot(
                    frame.width,
                    frame.height,
                    &frame.buffer[16..],
                    ScreenshotFormat::Png,
                )
            })
            .await
        });
        let progress = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(5), entered_rx)
                .await
                .unwrap()
                .unwrap();
            state.publish_frame(distinctive_frame());
            release_tx.send(()).unwrap();
        });
        let (capture, progress) = tokio::join!(capture, progress);
        assert!(!capture.unwrap().unwrap().is_empty());
        progress.unwrap();
    }

    #[test]
    fn display_is_latest_wins_and_snapshot_ownership_is_shared() {
        let state = AppState::default();
        state.publish_frame(distinctive_frame());
        let snapshot = state
            .latest_raw_frame
            .lock()
            .unwrap()
            .latest
            .clone()
            .unwrap();
        let retained = state
            .latest_raw_frame
            .lock()
            .unwrap()
            .latest
            .clone()
            .unwrap();
        assert!(Arc::ptr_eq(&snapshot, &retained));
        let mut next = distinctive_frame();
        next.buffer[16] = 99;
        state.publish_frame(next.clone());
        assert_eq!(state.take_display_frame(), next.buffer);
        assert!(state.take_display_frame().is_empty());
        assert_eq!(snapshot.buffer[16], 40);
        state.clear_metrics();
        assert!(commands::capture_screen(&state, None).is_err());
        assert!(state.take_display_frame().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn disconnect_join_leaves_executor_responsive_and_clears_frames() {
        let state = AppState::default();
        state.publish_frame(distinctive_frame());
        let stop = state.worker_stop_flag();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        *state.media_handle.lock().unwrap() = Some(thread::spawn(move || {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(stop.load(Ordering::SeqCst));
        }));
        entered_rx.await.unwrap();
        let disconnect = disconnect_internal(&state);
        tokio::pin!(disconnect);
        // Poll teardown once: it must yield while the worker is still gated.
        std::future::poll_fn(|cx| {
            use std::future::Future;
            assert!(disconnect.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert!(state.stop_media_flag.load(Ordering::SeqCst));
        release_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), disconnect)
            .await
            .unwrap()
            .unwrap();
        assert!(state.media_handle.lock().unwrap().is_none());
        assert!(commands::capture_screen(&state, None).is_err());
        let new_stop = state.worker_stop_flag();
        assert!(!new_stop.load(Ordering::SeqCst));
    }

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
            key_code: Some(0x41),
            modifiers: Modifiers::COMMAND.bits(),
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        };
        let event = convert_input_payload(&payload).unwrap();
        assert_eq!(event.event_type, InputEventType::KeyDown);
        assert_eq!(event.key_code, 0x00);
        assert!(event.modifiers.contains(Modifiers::COMMAND));
    }

    #[tokio::test]
    async fn app_state_lifecycle() {
        let state = AppState::default();
        state.frames_received.store(42, Ordering::Relaxed);
        state.frames_decoded.store(42, Ordering::Relaxed);
        state.clear_metrics();
        assert_eq!(state.frames_received.load(Ordering::Relaxed), 0);
        assert_eq!(state.frames_decoded.load(Ordering::Relaxed), 0);
        assert!(disconnect_internal(&state).await.is_ok());
    }
}
