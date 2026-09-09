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
    key_event, normalize_pointer, pointer_event, ClientSession, CursorState, InputKey,
    PairingStore, ReadySession, SessionConfig, SessionError, SessionEvent, SessionRuntime,
    SessionState, DEFAULT_TCP_PORT, DEFAULT_UDP_PORT,
};
#[cfg(target_os = "macos")]
use erd_app::ClipboardMonitor;
use erd_decode::HevcDecoder;
use erd_proto::{InputEvent, InputEventType, Modifiers};
#[cfg(target_os = "macos")]
use erd_proto::{ClipboardSyncDirection, ClipboardSyncOrigin, ClipboardSyncUpdate};
use erd_render::{AudioOutputDevice, AudioOutputStatus, AudioQueue, CpalAudioOutput};
use serde::{Deserialize, Serialize};
use tauri::State;

#[cfg(target_os = "macos")]
use erd_app::platform::SystemClipboard;

#[cfg(test)]
mod discovery_tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostItem {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub os: String,
    /// Tailscale presence only; this does not establish ERD service readiness.
    pub online: bool,
    /// Legacy exact-hostname store match, not proof of peer identity.
    pub paired: bool,
    pub last_seen: Option<String>,
    pub tcp_port: Option<u16>,
    pub udp_port: Option<u16>,
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
    pub session: Arc<Mutex<Option<ClientSession>>>,
    pub tcp_runtime: Mutex<Option<SessionRuntime>>,
    pub stop_media_flag: Arc<AtomicBool>,
    pub media_handle: Mutex<Option<thread::JoinHandle<()>>>,
    #[cfg(target_os = "macos")]
    clipboard_monitor: Arc<Mutex<Option<ClipboardMonitor<SystemClipboard>>>>,
    pub frames_received: Arc<AtomicU64>,
    pub frames_decoded: Arc<AtomicU64>,
    pub audio_packets_received: Arc<AtomicU64>,
    pub latency: Arc<Mutex<LatencyTracker>>,
    pub latest_raw_frame: Arc<Mutex<FrameMailbox>>,
    pub latest_cursor: Arc<Mutex<CursorState>>,
    pub agent_tracker: Arc<Mutex<InputStateTracker>>,
    pub agent_pos: Arc<Mutex<(f32, f32)>>,
    audio_playback: Arc<Mutex<AudioPlayback>>,
    audio_runtime: Mutex<Option<AudioRuntime>>,
    pub discovery: Arc<DesktopDiscoveryState>,
}

pub struct DesktopDiscoveryState {
    pub lan_browser: Mutex<Option<erd_net::discovery::LanDiscovery>>,
    pub tailscale_cache: Mutex<Option<TailscaleCache>>,
    pub tailscale_refreshing: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct TailscaleCache {
    pub result: Result<Vec<HostItem>, String>,
    pub updated_at: Instant,
}

impl Default for DesktopDiscoveryState {
    fn default() -> Self {
        Self {
            lan_browser: Mutex::new(None),
            tailscale_cache: Mutex::new(None),
            tailscale_refreshing: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Default)]
struct AudioPlayback {
    queue: Option<AudioQueue>,
    error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DesktopAudioDevice {
    id: String,
    name: String,
    supported: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct DesktopAudioStatus {
    active: bool,
    volume: f32,
    muted: bool,
    device_id: Option<String>,
    devices: Vec<DesktopAudioDevice>,
    consumed_samples: u64,
    error: Option<String>,
}

enum AudioAction {
    Start,
    Stop,
    Status,
    List,
    Volume(f32),
    Muted(bool),
    Device(Option<String>),
}

// The only substituted boundary in deterministic tests is the physical device.
// PCM decoding, queue ownership, commands and media dispatch remain native.
trait DesktopAudioOutput {
    fn status(&self) -> Result<AudioOutputStatus, String>;
}

impl DesktopAudioOutput for CpalAudioOutput {
    fn status(&self) -> Result<AudioOutputStatus, String> {
        CpalAudioOutput::status(self).map_err(|e| e.to_string())
    }
}

trait DesktopAudioBackend {
    fn devices(&mut self) -> Result<Vec<DesktopAudioDevice>, String>;
    fn open(
        &mut self,
        queue: AudioQueue,
        device: Option<&str>,
    ) -> Result<Box<dyn DesktopAudioOutput>, String>;
}

#[derive(Default)]
struct CpalDesktopBackend {
    // IDs are process-local tokens into retained handles, never device names or
    // indices into a newly enumerated list. Explicit selection never falls back.
    devices: Option<Vec<AudioOutputDevice>>,
}

impl DesktopAudioBackend for CpalDesktopBackend {
    fn devices(&mut self) -> Result<Vec<DesktopAudioDevice>, String> {
        if self.devices.is_none() {
            self.devices = Some(CpalAudioOutput::output_devices().map_err(|e| e.to_string())?);
        }
        self.devices
            .as_ref()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, device)| {
                Ok(DesktopAudioDevice {
                    id: format!("output-{index}"),
                    name: device.name().map_err(|e| e.to_string())?,
                    supported: device.supports_pcm().map_err(|e| e.to_string())?,
                })
            })
            .collect()
    }

    fn open(
        &mut self,
        queue: AudioQueue,
        device: Option<&str>,
    ) -> Result<Box<dyn DesktopAudioOutput>, String> {
        let selected = match device {
            None => None,
            Some(id) => {
                let devices = self.devices()?;
                let index = devices
                    .iter()
                    .position(|d| d.id == id)
                    .ok_or_else(|| format!("Unknown audio output: {id}"))?;
                Some(&self.devices.as_ref().unwrap()[index])
            }
        };
        CpalAudioOutput::start_on_device(queue, selected)
            .map(|output| Box::new(output) as Box<dyn DesktopAudioOutput>)
            .map_err(|e| e.to_string())
    }
}

struct DesktopAudio<B: DesktopAudioBackend> {
    backend: B,
    playback: Arc<Mutex<AudioPlayback>>,
    output: Option<Box<dyn DesktopAudioOutput>>,
    volume: f32,
    muted: bool,
    device_id: Option<String>,
    devices: Vec<DesktopAudioDevice>,
    session_active: bool,
}

impl<B: DesktopAudioBackend> DesktopAudio<B> {
    fn new(backend: B, playback: Arc<Mutex<AudioPlayback>>) -> Self {
        Self {
            backend,
            playback,
            output: None,
            volume: 1.0,
            muted: false,
            device_id: None,
            devices: Vec::new(),
            session_active: false,
        }
    }

    fn stop_output(&mut self) -> Result<(), String> {
        // Detach the producer BEFORE releasing the old callback. A racing media
        // event sees no queue; it cannot repopulate an old/new device backlog.
        let queue = self
            .playback
            .lock()
            .map_err(|e| e.to_string())?
            .queue
            .take();
        self.output.take();
        if let Some(queue) = queue {
            queue.clear().map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn start_output(&mut self) -> Result<(), String> {
        self.stop_output()?;
        let queue = AudioQueue::default();
        queue.set_volume(self.volume).map_err(|e| e.to_string())?;
        queue.set_muted(self.muted).map_err(|e| e.to_string())?;
        let output = self
            .backend
            .open(queue.clone(), self.device_id.as_deref())?;
        self.output = Some(output);
        let mut playback = self.playback.lock().map_err(|e| e.to_string())?;
        playback.queue = Some(queue);
        playback.error = None;
        Ok(())
    }

    fn apply(&mut self, action: AudioAction) -> Result<DesktopAudioStatus, String> {
        let result = (|| {
            match action {
                AudioAction::Start => {
                    self.session_active = true;
                    self.start_output()?;
                }
                AudioAction::Stop => {
                    self.session_active = false;
                    self.stop_output()?;
                }
                AudioAction::Status => {}
                AudioAction::List => {
                    self.devices = self.backend.devices()?;
                }
                AudioAction::Volume(volume) => {
                    // Validate even when no session/output is active.
                    let queue = self
                        .playback
                        .lock()
                        .map_err(|e| e.to_string())?
                        .queue
                        .clone()
                        .unwrap_or_default();
                    queue.set_volume(volume).map_err(|e| e.to_string())?;
                    self.volume = volume;
                }
                AudioAction::Muted(muted) => {
                    if let Some(queue) = &self.playback.lock().map_err(|e| e.to_string())?.queue {
                        queue.set_muted(muted).map_err(|e| e.to_string())?;
                    }
                    self.muted = muted;
                }
                AudioAction::Device(device) => {
                    // Even a rejected switch cannot leave the former device playing.
                    self.stop_output()?;
                    if let Some(id) = &device {
                        self.devices = self.backend.devices()?;
                        if !self.devices.iter().any(|d| &d.id == id && d.supported) {
                            return Err(format!("Unknown or unsupported audio output: {id}"));
                        }
                    }
                    self.device_id = device;
                    if self.session_active {
                        self.start_output()?;
                    }
                }
            }
            self.snapshot()
        })();
        if let Err(error) = &result {
            tracing::error!(%error, "Desktop audio command failed");
            self.playback
                .lock()
                .expect("private audio endpoint lock poisoned")
                .error = Some(error.clone());
        }
        result
    }

    fn snapshot(&mut self) -> Result<DesktopAudioStatus, String> {
        let status = self
            .output
            .as_ref()
            .map(|out| out.status())
            .transpose()?
            .unwrap_or_default();
        if let Some(error) = status.last_error {
            self.stop_output()?;
            self.playback.lock().map_err(|e| e.to_string())?.error = Some(error);
        }
        Ok(DesktopAudioStatus {
            active: self.output.is_some(),
            volume: self.volume,
            muted: self.muted,
            device_id: self.device_id.clone(),
            devices: self.devices.clone(),
            consumed_samples: status.consumed_samples,
            error: self
                .playback
                .lock()
                .map_err(|e| e.to_string())?
                .error
                .clone(),
        })
    }
}

impl<B: DesktopAudioBackend> Drop for DesktopAudio<B> {
    fn drop(&mut self) {
        if let Err(error) = self.stop_output() {
            tracing::error!(%error, "Audio worker cleanup failed");
        }
    }
}

type AudioRequest = (
    AudioAction,
    tokio::sync::oneshot::Sender<Result<DesktopAudioStatus, String>>,
);

struct AudioRuntime {
    sender: Option<std::sync::mpsc::Sender<AudioRequest>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl AudioRuntime {
    fn spawn(playback: Arc<Mutex<AudioPlayback>>) -> Result<Self, String> {
        Self::spawn_with_backend(playback, CpalDesktopBackend::default)
    }

    fn spawn_with_backend<B: DesktopAudioBackend + 'static>(
        playback: Arc<Mutex<AudioPlayback>>,
        backend: impl FnOnce() -> B + Send + 'static,
    ) -> Result<Self, String> {
        let (sender, receiver) = std::sync::mpsc::channel::<AudioRequest>();
        let worker = thread::Builder::new()
            .name("erd-audio-output".into())
            .spawn(move || {
                // CPAL Stream is deliberately thread-affine on some platforms.
                // Create, control and drop it on this owner, never on Tokio/UI threads.
                let mut audio = DesktopAudio::new(backend(), playback);
                while let Ok((action, reply)) = receiver.recv() {
                    // A cancelled command receiver does not cancel cleanup ownership.
                    let _ = reply.send(audio.apply(action));
                }
            })
            .map_err(|e| format!("Failed to start audio worker: {e}"))?;
        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
        })
    }
}

impl Drop for AudioRuntime {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                tracing::error!("Audio worker panicked");
            }
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            lifecycle: tokio::sync::Mutex::new(()),
            session: Arc::new(Mutex::new(None)),
            tcp_runtime: Mutex::new(None),
            stop_media_flag: Arc::new(AtomicBool::new(false)),
            media_handle: Mutex::new(None),
            frames_received: Arc::new(AtomicU64::new(0)),
            frames_decoded: Arc::new(AtomicU64::new(0)),
            audio_packets_received: Arc::new(AtomicU64::new(0)),
            #[cfg(target_os = "macos")]
            clipboard_monitor: Arc::new(Mutex::new(None)),
            latency: Arc::new(Mutex::new(LatencyTracker::default())),
            latest_raw_frame: Arc::new(Mutex::new(FrameMailbox::default())),
            latest_cursor: Arc::new(Mutex::new(CursorState::default())),
            agent_tracker: Arc::new(Mutex::new(InputStateTracker::default())),
            agent_pos: Arc::new(Mutex::new((0.5, 0.5))),
            audio_playback: Arc::new(Mutex::new(AudioPlayback::default())),
            audio_runtime: Mutex::new(None),
            discovery: Arc::new(DesktopDiscoveryState::default()),
        }
    }
}

impl AppState {
    async fn start_session_audio(&self) {
        if let Err(error) = self.audio_request(AudioAction::Start).await {
            // An unavailable preferred output must not lock the user out of the
            // remote video and its device-recovery controls. The status command
            // exposes this error; no alternate device is started implicitly.
            tracing::error!(%error, "Session audio unavailable");
            self.audio_playback
                .lock()
                .expect("private audio endpoint lock poisoned")
                .error = Some(error);
        }
    }

    async fn audio_request(&self, action: AudioAction) -> Result<DesktopAudioStatus, String> {
        let (reply, received) = tokio::sync::oneshot::channel();
        {
            let mut runtime = self.audio_runtime.lock().map_err(|e| e.to_string())?;
            if runtime.is_none() {
                *runtime = Some(AudioRuntime::spawn(self.audio_playback.clone())?);
            }
            runtime
                .as_ref()
                .unwrap()
                .sender
                .as_ref()
                .unwrap()
                .send((action, reply))
                .map_err(|e| format!("Audio worker unavailable: {e}"))?;
        }
        received
            .await
            .map_err(|e| format!("Audio worker failed: {e}"))?
    }

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
        "MiddleMouseDown" | "middlemousedown" | "11" => InputEventType::MiddleMouseDown,
        "MiddleMouseUp" | "middlemouseup" | "12" => InputEventType::MiddleMouseUp,
        "RelativeMove" | "relativemove" | "14" => InputEventType::RelativeMove,
        "GamepadAxis" | "gamepadaxis" => InputEventType::GamepadAxis,
        "GamepadButtonDown" | "gamepadbuttondown" => InputEventType::GamepadButtonDown,
        "GamepadButtonUp" | "gamepadbuttonup" => InputEventType::GamepadButtonUp,
        "PenMove" | "penmove" => InputEventType::PenMove,
        "PenDown" | "pendown" => InputEventType::PenDown,
        "PenUp" | "penup" => InputEventType::PenUp,
        other => return Err(format!("Unknown event type: {other}")),
    };

    let modifiers = Modifiers::from_bits_retain(event.modifiers);

    if ![
        event.x,
        event.y,
        event.view_width,
        event.view_height,
        event.scroll_dx,
        event.scroll_dy,
    ]
    .into_iter()
    .all(f32::is_finite)
    {
        return Err("Input coordinates and deltas must be finite".to_string());
    }

    match event_type {
        InputEventType::KeyDown | InputEventType::KeyUp => {
            let key_code = event.key_code.unwrap_or(0);
            key_event(event_type, InputKey::WindowsVirtualKey(key_code), modifiers)
                .ok_or_else(|| format!("Unsupported key code: {key_code}"))
        }
        InputEventType::GamepadAxis
        | InputEventType::GamepadButtonDown
        | InputEventType::GamepadButtonUp => Ok(InputEvent {
            event_type,
            x: event.x,
            y: event.y,
            key_code: event.key_code.unwrap_or(0),
            modifiers,
            scroll_dx: event.scroll_dx,
            scroll_dy: event.scroll_dy,
        }),
        InputEventType::PenMove | InputEventType::PenDown | InputEventType::PenUp => {
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
            let (norm_x, norm_y) = normalize_pointer(event.x, event.y, vw, vh)
                .ok_or_else(|| "Invalid pointer coordinates".to_string())?;
            Ok(InputEvent {
                event_type,
                x: norm_x,
                y: norm_y,
                key_code: event.key_code.unwrap_or(0),
                modifiers,
                scroll_dx: event.scroll_dx,
                scroll_dy: event.scroll_dy,
            })
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
    pub async fn list_hosts(state: State<'_, AppState>) -> Result<Vec<HostItem>, String> {
        list_hosts_internal(&state).await
    }

    pub async fn list_hosts_default() -> Result<Vec<HostItem>, String> {
        list_hosts_internal(&AppState::default()).await
    }

    pub async fn list_hosts_internal(state: &AppState) -> Result<Vec<HostItem>, String> {
        let store =
            PairingStore::open_default().map_err(|e| format!("Pairing store error: {e}"))?;
        let records = store
            .load_all()
            .map_err(|e| format!("Load pairings error: {e}"))?;

        let lan_res = {
            let mut browser_guard = state.discovery.lan_browser.lock().unwrap();
            if browser_guard.is_none() {
                match erd_net::discovery::LanDiscovery::new() {
                    Ok(browser) => {
                        *browser_guard = Some(browser);
                    }
                    Err(e) => {
                        tracing::warn!("LAN discovery browser initialization failed: {e}");
                    }
                }
            }
            match browser_guard.as_ref() {
                Some(browser) => browser.snapshot(),
                None => Err(erd_net::discovery::DiscoveryError::Unavailable),
            }
        };

        let cached_ts = {
            let guard = state.discovery.tailscale_cache.lock().unwrap();
            guard.clone()
        };

        if !state.discovery.tailscale_refreshing.swap(true, Ordering::SeqCst) {
            let discovery_clone = Arc::clone(&state.discovery);
            let records_clone = records.clone();
            tokio::spawn(async move {
                #[cfg(target_os = "macos")]
                let program = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";
                #[cfg(not(target_os = "macos"))]
                let program = "tailscale";
                let output = tailscale_status(std::ffi::OsStr::new(program)).await;
                let ts_res = hosts_from_tailscale_output(output, &records_clone);
                if let Ok(mut cache_guard) = discovery_clone.tailscale_cache.lock() {
                    *cache_guard = Some(TailscaleCache {
                        result: ts_res,
                        updated_at: Instant::now(),
                    });
                }
                discovery_clone.tailscale_refreshing.store(false, Ordering::SeqCst);
            });
        }

        let ts_result = match cached_ts {
            Some(cache) => cache.result,
            None => {
                if lan_res.is_ok() {
                    Err("Tailscale background query in progress".into())
                } else {
                    tokio::time::timeout(Duration::from_millis(100), async {
                        while state.discovery.tailscale_refreshing.load(Ordering::Relaxed) {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                    })
                    .await
                    .ok();
                    state
                        .discovery
                        .tailscale_cache
                        .lock()
                        .unwrap()
                        .as_ref()
                        .map(|c| c.result.clone())
                        .unwrap_or_else(|| Err("Tailscale status unavailable".into()))
                }
            }
        };

        merge_discovery_results(lan_res, ts_result, &records)
    }

    pub(super) async fn tailscale_status(
        program: &std::ffi::OsStr,
    ) -> std::io::Result<std::process::Output> {
        // Dropping the timed-out/cancelled output future also kills its child.
        tokio::time::timeout(
            Duration::from_secs(5),
            tokio::process::Command::new(program)
                .args(["status", "--json"])
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "Tailscale status timed out")
        })?
    }

    pub(super) fn hosts_from_tailscale_output(
        output: std::io::Result<std::process::Output>,
        records: &[erd_app::PairingRecord],
    ) -> Result<Vec<HostItem>, String> {
        let output = output.map_err(|e| format!("Tailscale status execution failed: {e}"))?;
        if !output.status.success() {
            // Do not expose raw stdout/stderr: daemon diagnostics can contain account data.
            return Err(format!("Tailscale status failed: {}", output.status));
        }
        let status: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("Invalid Tailscale status JSON: {e}"))?;
        let peers = match status.get("Peer") {
            Some(serde_json::Value::Object(peers)) => peers,
            // Tailscale may serialize a nil peer map as null.
            Some(serde_json::Value::Null) => return Ok(Vec::new()),
            _ => return Err("Invalid Tailscale status: Peer must be an object or null".into()),
        };

        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Peer {
            host_name: String,
            #[serde(rename = "OS")]
            os: Option<String>,
            online: bool,
            #[serde(rename = "TailscaleIPs")]
            ips: Vec<String>,
        }

        let mut hosts = Vec::new();
        for value in peers.values() {
            let peer: Peer = serde_json::from_value(value.clone())
                .map_err(|_| "Invalid Tailscale peer fields".to_string())?;
            // Mobile and non-host platforms cannot run erd-host.
            if let Some(ref os) = peer.os {
                let os_lower = os.to_ascii_lowercase();
                if matches!(
                    os_lower.as_str(),
                    "ios" | "android" | "tvos" | "watchos" | "visionos"
                ) {
                    continue;
                }
            }

            if let Some(ip) = peer.ips.first().filter(|ip| !ip.is_empty()) {
                // Legacy display hint only, not stable identity or authentication proof.
                let record = records.iter().rev().find(|r| r.name == peer.host_name);
                hosts.push(HostItem {
                    id: record.map(|r| r.id.clone()).unwrap_or_else(|| ip.clone()),
                    name: peer.host_name,
                    ip: ip.clone(),
                    os: peer.os.unwrap_or_else(|| "unknown".into()),
                    online: peer.online,
                    paired: record.is_some(),
                    last_seen: None,
                    tcp_port: None,
                    udp_port: None,
                });
            }
        }
        Ok(hosts)
    }

    pub fn merge_discovery_results(
        lan: Result<Vec<erd_net::discovery::DiscoveredHost>, erd_net::discovery::DiscoveryError>,
        tailscale: Result<Vec<HostItem>, String>,
        _records: &[erd_app::PairingRecord],
    ) -> Result<Vec<HostItem>, String> {
        match (lan, tailscale) {
            (Err(lan_err), Err(ts_err)) => {
                Err(format!(
                    "All discovery sources failed: LAN discovery error ({lan_err}); Tailscale error ({ts_err})"
                ))
            }
            (Ok(lan_hosts), Err(_)) => {
                let items = lan_hosts
                    .into_iter()
                    .map(|h| HostItem {
                        id: h.id,
                        name: h.name,
                        ip: h.ip,
                        os: h.os,
                        online: true,
                        paired: false,
                        last_seen: None,
                        tcp_port: Some(h.tcp_port),
                        udp_port: Some(h.udp_port),
                    })
                    .collect();
                Ok(items)
            }
            (Err(_), Ok(ts_hosts)) => Ok(ts_hosts),
            (Ok(lan_hosts), Ok(ts_hosts)) => {
                let mut merged: Vec<HostItem> = Vec::new();
                let mut seen_ips = std::collections::HashSet::new();

                for h in lan_hosts {
                    seen_ips.insert(h.ip.clone());
                    merged.push(HostItem {
                        id: h.id,
                        name: h.name,
                        ip: h.ip,
                        os: h.os,
                        online: true,
                        paired: false,
                        last_seen: None,
                        tcp_port: Some(h.tcp_port),
                        udp_port: Some(h.udp_port),
                    });
                }

                for th in ts_hosts {
                    if !seen_ips.contains(&th.ip) {
                        seen_ips.insert(th.ip.clone());
                        merged.push(th);
                    }
                }

                Ok(merged)
            }
        }
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

        match connect_session(&state, host, tcp_port, udp_port, pin).await {
            Ok(response) => Ok(response),
            Err(error) => match disconnect_internal(&state).await {
                Ok(()) => Err(error),
                Err(cleanup) => Err(format!("{error}\n{cleanup}")),
            },
        }
    }

    async fn connect_session(
        state: &AppState,
        host: String,
        tcp_port: Option<u16>,
        udp_port: Option<u16>,
        pin: Option<String>,
    ) -> Result<ConnectResponse, String> {
        let tcp = tcp_port.unwrap_or(DEFAULT_TCP_PORT);
        let udp = udp_port.unwrap_or(DEFAULT_UDP_PORT);

        let mut config = SessionConfig::direct(host.clone(), "EclipticRD-Tauri");
        config.tcp_port = tcp;
        config.udp_port = udp;

        let session =
            ClientSession::new(config).map_err(|e| format!("Failed to init session: {e}"))?;
        // Publish cleanup ownership before any fallible connect/start work.
        *state.session.lock().map_err(|e| e.to_string())? = Some(session.clone());

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
        *state.tcp_runtime.lock().map_err(|e| e.to_string())? = Some(tcp_runtime);
        state.start_session_audio().await;

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
        let audio_playback = state.audio_playback.clone();

        let media_thread = thread::Builder::new()
            .name("erd-media-pipeline".to_string())
            .spawn(move || {
                let mut decoder: Option<HevcDecoder> = None;
                let request_session = session_udp.clone();
                let decode_cursor_target = latest_cursor_target.clone();
                run_media_pipeline(
                    stop_flag_thread,
                    move || {
                        let event = session_udp.receive_udp_event();
                        if matches!(event, Ok(SessionEvent::Frame(_))) {
                            frames_rx.fetch_add(1, Ordering::Relaxed);
                        }
                        event
                    },
                    move |message| request_session.send_control(message),
                    move |frame, frame_recv_instant| {
                        for nv12 in decode_media_frame(&mut decoder, &frame)? {
                            frames_dec.fetch_add(1, Ordering::Relaxed);

                            let latency_ms = frame_recv_instant.elapsed().as_secs_f64() * 1000.0;
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

                            let cursor =
                                decode_cursor_target.lock().map(|c| *c).unwrap_or_default();
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
                        Ok(())
                    },
                    move |event| {
                        dispatch_media_event(
                            event,
                            &audio_rx,
                            &latest_cursor_target,
                            &audio_playback,
                        )
                    },
                );
            })
            .map_err(|e| format!("Failed to spawn media thread: {e}"))?;

        if let Ok(mut media_lock) = state.media_handle.lock() {
            *media_lock = Some(media_thread);
        }

        start_clipboard_monitor(&state, &session);

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
    pub fn set_bitrate(state: State<'_, AppState>, bitrate_mbps: u32) -> Result<(), String> {
        let target = bitrate_mbps.clamp(1, 300) as i32 * 1_000_000;
        let session = state.session.lock().map_err(|e| e.to_string())?;
        let session = session.as_ref().ok_or("Not connected")?;
        session
            .send_control(erd_proto::ControlMessage::BitrateAdjust(
                erd_proto::BitrateAdjust { target_bitrate: target },
            ))
            .map_err(|e| e.to_string())
    }

    /// Watches the local pasteboard while connected and pushes changes to the
    /// host. Best effort: clipboard sync failure never blocks a session.
    #[cfg(target_os = "macos")]
    fn start_clipboard_monitor(state: &AppState, session: &ClientSession) {
        if let Ok(mut slot) = state.clipboard_monitor.lock() {
            if let Some(mut stale) = slot.take() {
                stale.stop();
            }
        }
        let mut monitor = match ClipboardMonitor::new(SystemClipboard::default()) {
            Ok(monitor) => monitor,
            Err(error) => {
                tracing::warn!(%error, "clipboard monitor unavailable");
                return;
            }
        };
        let session = session.clone();
        let start = monitor.start(move |text| {
            let update = ClipboardSyncUpdate {
                request_id: 0,
                direction: ClipboardSyncDirection::ClientToHost,
                origin: ClipboardSyncOrigin::LocalPasteboard,
                text,
            };
            if let Err(error) =
                session.send_control(erd_proto::ControlMessage::ClipboardSyncUpdate(update))
            {
                tracing::debug!(%error, "clipboard push failed");
            }
        });
        if let Err(error) = start {
            tracing::warn!(%error, "clipboard monitor failed to start");
            return;
        }
        if let Ok(mut slot) = state.clipboard_monitor.lock() {
            *slot = Some(monitor);
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn start_clipboard_monitor(_state: &AppState, _session: &ClientSession) {}

    pub(super) fn stop_clipboard_monitor(state: &AppState) {
        #[cfg(target_os = "macos")]
        if let Ok(mut slot) = state.clipboard_monitor.lock() {
            if let Some(mut monitor) = slot.take() {
                monitor.stop();
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = state;
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
        #[cfg(target_os = "macos")]
        let clipboard_monitor = state.clipboard_monitor.clone();
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
                            | SessionEvent::Cursor(_)
                            | SessionEvent::StreamConfig(_),
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
                // The session monitor owns echo suppression: when present it
                // applies remote text and records the write so the local
                // pasteboard poller never bounces the text back to the host.
                #[cfg(target_os = "macos")]
                let applied = (|| {
                    let slot = clipboard_monitor.lock().ok()?;
                    let monitor = slot.as_ref()?;
                    monitor.apply_remote(&text).ok()?;
                    Some(())
                })()
                .is_some();
                #[cfg(not(target_os = "macos"))]
                let applied = false;
                if !applied {
                    if let Err(error) = apply_clipboard(&text) {
                        errors.push(error);
                    }
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
    pub async fn audio_status(state: State<'_, AppState>) -> Result<DesktopAudioStatus, String> {
        let _lifecycle = state.lifecycle.lock().await;
        state.audio_request(AudioAction::Status).await
    }

    #[tauri::command]
    pub async fn list_audio_devices(
        state: State<'_, AppState>,
    ) -> Result<DesktopAudioStatus, String> {
        let _lifecycle = state.lifecycle.lock().await;
        state.audio_request(AudioAction::List).await
    }

    #[tauri::command]
    pub async fn set_audio_volume(
        state: State<'_, AppState>,
        volume: f32,
    ) -> Result<DesktopAudioStatus, String> {
        let _lifecycle = state.lifecycle.lock().await;
        state.audio_request(AudioAction::Volume(volume)).await
    }

    #[tauri::command]
    pub async fn set_audio_muted(
        state: State<'_, AppState>,
        muted: bool,
    ) -> Result<DesktopAudioStatus, String> {
        let _lifecycle = state.lifecycle.lock().await;
        state.audio_request(AudioAction::Muted(muted)).await
    }

    #[tauri::command]
    pub async fn set_audio_device(
        state: State<'_, AppState>,
        device_id: Option<String>,
    ) -> Result<DesktopAudioStatus, String> {
        let _lifecycle = state.lifecycle.lock().await;
        let sessions = state.session.clone();
        let tracker = state.agent_tracker.clone();
        let position = state.agent_pos.clone();
        let released = tokio::task::spawn_blocking(move || {
            let session = sessions.lock().map_err(|e| e.to_string())?;
            reset_session_inputs(session.as_ref(), &tracker, &position)
        })
        .await
        .map_err(|e| format!("Input reset worker failed: {e}"))
        .and_then(|result| result);
        // A failed input release must not prevent detaching the old audio output.
        let switched = state.audio_request(AudioAction::Device(device_id)).await;
        match (released, switched) {
            (Ok(()), result) => result,
            (Err(error), Ok(_)) => Err(error),
            (Err(error), Err(audio)) => Err(format!("{error}\n{audio}")),
        }
    }

    #[tauri::command]
    pub fn list_pairings() -> Result<Vec<erd_app::PairingRecord>, String> {
        let store =
            PairingStore::open_default().map_err(|e| format!("Pairing store error: {e}"))?;
        store
            .load_all()
            .map_err(|e| format!("Load pairings error: {e}"))
    }

    #[tauri::command]
    pub fn forget_pairing(id: String) -> Result<(), String> {
        let store =
            PairingStore::open_default().map_err(|e| format!("Pairing store error: {e}"))?;
        store
            .delete(&id)
            .map_err(|e| format!("Delete pairing error: {e}"))
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
        // Serialize submission with reset/teardown; a cloned session could send
        // a late key-down after cleanup had already released the host's keys.
        let sess_opt = state.session.lock().map_err(|e| e.to_string())?;
        if state.stop_media_flag.load(Ordering::SeqCst) {
            return Err("Session disconnecting or inactive".to_string());
        }
        let session = match sess_opt.as_ref() {
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
        let sess_opt = state.session.lock().map_err(|e| e.to_string())?;
        if state.stop_media_flag.load(Ordering::SeqCst) {
            return Err("Session disconnecting or inactive".to_string());
        }
        let session = match sess_opt.as_ref() {
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
        let sess_opt = state.session.lock().map_err(|e| e.to_string())?;
        let session = match sess_opt.as_ref() {
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

// Shared by the actual connect worker and media integration tests.
fn dispatch_media_event(
    event: SessionEvent,
    audio_rx: &AtomicU64,
    cursor_target: &Mutex<CursorState>,
    audio: &Mutex<AudioPlayback>,
) {
    match event {
        SessionEvent::Audio(bytes) => {
            audio_rx.fetch_add(1, Ordering::Relaxed);
            let mut playback = audio.lock().expect("private audio endpoint lock poisoned");
            if let Some(queue) = &playback.queue {
                // v3 transports little-endian f32 stereo PCM, not a compressed codec.
                if let Err(error) = queue.push_pcm_bytes(&bytes) {
                    tracing::error!(%error, "Desktop PCM packet rejected");
                    playback.error = Some(error.to_string());
                }
            }
        }
        SessionEvent::Cursor(cursor) => {
            if let Ok(mut c) = cursor_target.lock() {
                *c = cursor;
            }
        }
        _ => {}
    }
}

// Reset a failed codec before the shared queue admits another reference chain.
fn decode_media_frame(
    decoder: &mut Option<HevcDecoder>,
    frame: &erd_app::AssembledFrame,
) -> Result<Vec<erd_decode::Nv12Frame>, String> {
    let result = (|| {
        let dec = match decoder {
            Some(dec) => dec,
            None => {
                let (_, dec) = HevcDecoder::from_keyframe_auto(&frame.data)?;
                decoder.insert(dec)
            }
        };
        dec.decode(&frame.data, frame.timestamp_ms as i64)
    })();
    result.map_err(|error: erd_decode::DecodeError| {
        *decoder = None;
        error.to_string()
    })
}

// The connect worker and recovery regressions share this media dispatch seam.
// Decoder/publisher ownership stays in the caller; events are real session events.
fn run_media_pipeline(
    stop: Arc<AtomicBool>,
    mut receive: impl FnMut() -> Result<SessionEvent, SessionError>,
    request: impl Fn(erd_proto::ControlMessage) -> Result<(), SessionError> + Send + Sync + 'static,
    mut decode: impl FnMut(erd_app::AssembledFrame, Instant) -> Result<(), String> + Send + 'static,
    mut other: impl FnMut(SessionEvent),
) {
    use erd_app::frame_queue::FrameQueue;

    // Scope owns exactly one decoder. Closing the queue also happens on unwind,
    // before scope joins, so an idle decoder can never strand its UDP owner.
    struct CloseQueue<'a>(&'a FrameQueue);
    impl Drop for CloseQueue<'_> {
        fn drop(&mut self) {
            if let Err(error) = self.0.stop() {
                tracing::error!(%error, "Failed to close media queue");
            }
        }
    }

    let queue = FrameQueue::new();
    let request_keyframe = || {
        if let Err(error) = request(erd_proto::ControlMessage::RequestKeyFrame) {
            tracing::warn!(%error, "Media recovery keyframe request failed");
        }
    };
    thread::scope(|scope| {
        let close = CloseQueue(&queue);
        let decoder = match thread::Builder::new()
            .name("erd-media-decode".to_string())
            .spawn_scoped(scope, || {
                while let Ok((frame, received)) = queue.recv() {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Err(error) = decode(frame, received) {
                        tracing::warn!(%error, "Frame decode failed");
                        match queue.decode_failed() {
                            Ok(true) => request_keyframe(),
                            Ok(false) => {}
                            Err(error) => {
                                tracing::error!(%error, "Decode recovery queue failed");
                                stop.store(true, Ordering::SeqCst);
                                break;
                            }
                        }
                    }
                }
            }) {
            Ok(worker) => worker,
            Err(error) => {
                tracing::error!(%error, "Failed to spawn media decoder");
                stop.store(true, Ordering::SeqCst);
                return;
            }
        };
        while !stop.load(Ordering::Relaxed) {
            match receive() {
                Ok(SessionEvent::Frame(frame)) => {
                    let received = Instant::now();
                    match queue.push((frame, received)) {
                        Ok(true) => request_keyframe(),
                        Ok(false) => {}
                        Err(error) => {
                            tracing::error!(%error, "Media queue closed");
                            break;
                        }
                    }
                }
                Ok(event) => other(event),
                Err(SessionError::Io(e))
                    if e.kind() == std::io::ErrorKind::TimedOut
                        || e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(SessionError::NotReady) => break,
                Err(e) => {
                    tracing::debug!("UDP media pipeline event error: {e}");
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
            }
        }
        drop(close);
        if let Err(panic) = decoder.join() {
            stop.store(true, Ordering::SeqCst);
            // Preserve disconnect's existing media-worker panic reporting.
            std::panic::resume_unwind(panic);
        }
    });
}

async fn disconnect_internal(state: &AppState) -> Result<(), String> {
    disconnect_with_stop(state, SessionRuntime::stop).await
}

fn reset_session_inputs(
    session: Option<&ClientSession>,
    tracker: &Mutex<InputStateTracker>,
    position: &Mutex<(f32, f32)>,
) -> Result<(), String> {
    let mut tracker = tracker.lock().map_err(|e| e.to_string())?;
    tracker.clear();
    *position.lock().map_err(|e| e.to_string())? = (0.5, 0.5);
    if let Some(session) = session {
        if session.state().map_err(|e| e.to_string())? == SessionState::Ready {
            session
                .send_input(InputEvent {
                    event_type: InputEventType::Reset,
                    x: 0.5,
                    y: 0.5,
                    key_code: 0,
                    modifiers: Modifiers::empty(),
                    scroll_dx: 0.0,
                    scroll_dy: 0.0,
                })
                .map_err(|e| format!("Failed to reset remote input: {e}"))?;
        }
    }
    Ok(())
}

async fn disconnect_with_stop(
    state: &AppState,
    stop_tcp: impl FnOnce(&mut SessionRuntime) -> Result<(), SessionError> + Send + 'static,
) -> Result<(), String> {
    state.stop_media_flag.store(true, Ordering::SeqCst);
    commands::stop_clipboard_monitor(state);
    let sessions = state.session.clone();
    let tracker = state.agent_tracker.clone();
    let position = state.agent_pos.clone();
    let tcp = state.tcp_runtime.lock().map_err(|e| e.to_string())?.take();
    let media = state.media_handle.lock().map_err(|e| e.to_string())?.take();
    // Do not create an audio worker for an unused/failed pre-connect teardown.
    let has_audio = state
        .audio_runtime
        .lock()
        .map_err(|e| e.to_string())?
        .is_some();
    let audio_stopped = if has_audio {
        state.audio_request(AudioAction::Stop).await.map(|_| ())
    } else {
        Ok(())
    };
    let result = tokio::task::spawn_blocking(move || {
        let (session, released) = match sessions.lock() {
            Ok(mut session) => {
                let released = reset_session_inputs(session.as_ref(), &tracker, &position);
                (session.take(), released)
            }
            Err(error) => (None, Err(error.to_string())),
        };
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
            released,
            audio_stopped,
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
            commands::list_pairings,
            commands::forget_pairing,
            commands::connect,
            commands::disconnect,
            commands::audio_status,
            commands::list_audio_devices,
            commands::set_audio_volume,
            commands::set_audio_muted,
            commands::set_audio_device,
            commands::stats,
            commands::send_input,
            commands::agent_execute_action,
            commands::agent_get_screen_info,
            commands::agent_capture_screen,
            commands::agent_release_all,
            commands::get_cursor_position,
            commands::poll_frame_raw,
            commands::set_bitrate
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

#[cfg(test)]
mod recovery_tests;

#[cfg(test)]
mod desktop_integration_tests;
