use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

use erd_app::{
    ClientSession, ReadySession, SessionConfig, SessionError, SessionEvent,
    SessionRuntime,
};
use erd_mobile::{
    TouchGestureHandler, TouchMode, TouchPhase, TouchPoint, ViewportState,
};
use erd_proto::{ControlMessage, InputEvent, InputEventType, Modifiers};
use erd_render::{AudioOutputEvent, AudioQueue, CpalAudioOutput};
use serde::{Deserialize, Serialize};

use crate::frame::repack_nv12_frame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Idle,
    Connecting,
    Ready,
    Error,
}

impl ConnectionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Connecting => "connecting",
            Self::Ready => "ready",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionStats {
    pub state: String,
    pub host: Option<String>,
    pub frames_received: u64,
    pub frames_decoded: u64,
    pub audio_packets_received: u64,
    pub audio_samples_played: u64,
    pub width: u32,
    pub height: u32,
    pub last_error: Option<String>,
}

pub struct SessionInner {
    pub generation: u64,
    pub state: ConnectionState,
    pub host: Option<String>,
    pub session: Option<ClientSession>,
    pub touch_handler: TouchGestureHandler,
    pub touch_mode: TouchMode,
    pub audio_queue: AudioQueue,
    pub tcp_runtime: Option<SessionRuntime>,
    pub worker_handles: Vec<thread::JoinHandle<()>>,
    pub stop_flag: Arc<AtomicBool>,
    pub frames_received: Arc<AtomicU64>,
    pub frames_decoded: Arc<AtomicU64>,
    pub audio_packets_received: Arc<AtomicU64>,
    pub audio_samples_played: Arc<AtomicU64>,
    pub video_width: Arc<AtomicU32>,
    pub video_height: Arc<AtomicU32>,
    pub frame_sequence: Arc<AtomicU64>,
    pub last_polled_sequence: Arc<AtomicU64>,
    pub latest_frame: Arc<Mutex<Option<Arc<Vec<u8>>>>>,
    pub last_error: Arc<Mutex<Option<String>>>,
    pub first_frame_presented: Arc<AtomicBool>,
}

impl Default for SessionInner {
    fn default() -> Self {
        let viewport = ViewportState::new(1.0, 1.0).unwrap_or_default();
        Self {
            generation: 0,
            state: ConnectionState::Idle,
            host: None,
            session: None,
            touch_handler: TouchGestureHandler::new(viewport, TouchMode::DirectTouch),
            touch_mode: TouchMode::DirectTouch,
            audio_queue: AudioQueue::default(),
            tcp_runtime: None,
            worker_handles: Vec::new(),
            stop_flag: Arc::new(AtomicBool::new(false)),
            frames_received: Arc::new(AtomicU64::new(0)),
            frames_decoded: Arc::new(AtomicU64::new(0)),
            audio_packets_received: Arc::new(AtomicU64::new(0)),
            audio_samples_played: Arc::new(AtomicU64::new(0)),
            video_width: Arc::new(AtomicU32::new(0)),
            video_height: Arc::new(AtomicU32::new(0)),
            frame_sequence: Arc::new(AtomicU64::new(0)),
            last_polled_sequence: Arc::new(AtomicU64::new(0)),
            latest_frame: Arc::new(Mutex::new(None)),
            last_error: Arc::new(Mutex::new(None)),
            first_frame_presented: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<Mutex<SessionInner>>,
    pub lifecycle_lock: Arc<tokio::sync::Mutex<()>>,
    pub discovery: Arc<Mutex<Option<erd_net::discovery::LanDiscovery>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(SessionInner::default())),
            lifecycle_lock: Arc::new(tokio::sync::Mutex::new(())),
            discovery: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn list_discovered_hosts(&self) -> Result<Vec<erd_net::discovery::DiscoveredHost>, String> {
        let state_clone = self.clone();
        tokio::task::spawn_blocking(move || state_clone.discovery_snapshot_blocking())
            .await
            .map_err(|e| format!("Discovery task failed: {e}"))?
    }

    pub async fn stop_discovery(&self) -> Result<(), String> {
        let state_clone = self.clone();
        tokio::task::spawn_blocking(move || state_clone.stop_discovery_blocking())
            .await
            .map_err(|e| format!("Stop discovery task failed: {e}"))?
    }

    pub fn stop_discovery_blocking(&self) -> Result<(), String> {
        let mut disc_guard = self
            .discovery
            .lock()
            .map_err(|_| "Discovery mutex is poisoned".to_string())?;
        if disc_guard.is_some() {
            tracing::info!("Stopping and dropping LAN discovery browser");
            *disc_guard = None;
        }
        Ok(())
    }

    pub fn discovery_snapshot_blocking(&self) -> Result<Vec<erd_net::discovery::DiscoveredHost>, String> {
        let mut disc_guard = self
            .discovery
            .lock()
            .map_err(|_| "Discovery mutex is poisoned".to_string())?;
        if disc_guard.is_none() {
            let browser = match erd_net::discovery::LanDiscovery::new() {
                Ok(b) => b,
                Err(e) => {
                    return Err(format!("LAN discovery initialization error: {e}"));
                }
            };
            *disc_guard = Some(browser);
        }

        if let Some(ref browser) = *disc_guard {
            match browser.snapshot() {
                Ok(hosts) => {
                    tracing::info!(count = hosts.len(), "Observed live LAN discovery snapshot");
                    Ok(hosts)
                }
                Err(e) => {
                    *disc_guard = None;
                    Err(format!("LAN discovery snapshot error: {e}"))
                }
            }
        } else {
            Ok(Vec::new())
        }
    }

    pub fn stats(&self) -> Result<SessionStats, String> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| "State mutex is poisoned".to_string())?;
        let last_err = inner.last_error.lock().ok().and_then(|g| g.clone());
        Ok(SessionStats {
            state: inner.state.as_str().to_string(),
            host: inner.host.clone(),
            frames_received: inner.frames_received.load(Ordering::Relaxed),
            frames_decoded: inner.frames_decoded.load(Ordering::Relaxed),
            audio_packets_received: inner.audio_packets_received.load(Ordering::Relaxed),
            audio_samples_played: inner.audio_samples_played.load(Ordering::Relaxed),
            width: inner.video_width.load(Ordering::Relaxed),
            height: inner.video_height.load(Ordering::Relaxed),
            last_error: last_err,
        })
    }

    pub fn poll_frame(&self) -> Result<Option<Arc<Vec<u8>>>, String> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| "State mutex is poisoned".to_string())?;
        let latest_seq = inner.frame_sequence.load(Ordering::Relaxed);
        let last_polled = inner.last_polled_sequence.load(Ordering::Relaxed);
        if latest_seq == 0 || latest_seq <= last_polled {
            return Ok(None);
        }
        let frame = inner.latest_frame.lock().ok().and_then(|g| g.clone());
        if frame.is_some() {
            inner.last_polled_sequence.store(latest_seq, Ordering::Relaxed);
        }
        Ok(frame)
    }

    pub fn set_muted(&self, muted: bool) -> Result<(), String> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| "State mutex is poisoned".to_string())?;
        inner
            .audio_queue
            .set_muted(muted)
            .map_err(|e| format!("Failed to set audio muted: {e}"))
    }

    pub fn set_touch_mode(&self, mode_str: &str) -> Result<(), String> {
        let new_mode = match mode_str {
            "direct" => TouchMode::DirectTouch,
            "trackpad" => TouchMode::TrackpadRelative,
            other => return Err(format!("Invalid touch mode: {other}")),
        };

        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "State mutex is poisoned".to_string())?;

        if inner.touch_mode == new_mode {
            return Ok(());
        }

        let release_evt = inner.touch_handler.set_mode(new_mode);
        inner.touch_mode = new_mode;

        if let Some(evt) = release_evt {
            if let Some(ref session) = inner.session {
                let _ = session.send_input(evt);
            }
        }
        Ok(())
    }

    pub fn handle_touch(&self, id: u64, x: f32, y: f32, phase_str: &str) -> Result<(), String> {
        let evt = match self.process_touch_for_test(id, x, y, phase_str)? {
            Some(evt) => evt,
            None => return Ok(()),
        };
        let inner = self
            .inner
            .lock()
            .map_err(|_| "State mutex is poisoned".to_string())?;
        if let Some(ref session) = inner.session {
            session
                .send_input(evt)
                .map_err(|e| format!("Failed to send touch input: {e}"))?;
        }
        Ok(())
    }

    pub fn process_touch_for_test(
        &self,
        id: u64,
        x: f32,
        y: f32,
        phase_str: &str,
    ) -> Result<Option<InputEvent>, String> {
        let phase = match phase_str {
            "began" => TouchPhase::Began,
            "moved" => TouchPhase::Moved,
            "ended" => TouchPhase::Ended,
            "cancelled" => TouchPhase::Cancelled,
            other => return Err(format!("Invalid touch phase: {other}")),
        };

        if (phase == TouchPhase::Began || phase == TouchPhase::Moved)
            && (!x.is_finite() || !y.is_finite())
        {
            return Err("Non-finite touch coordinates".to_string());
        }

        let point = TouchPoint { id, x, y, phase };

        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "State mutex is poisoned".to_string())?;

        let evt_res = inner.touch_handler.process_touch(point);
        let mut evt = match evt_res {
            Ok(Some(evt)) => evt,
            Ok(None) => return Ok(None),
            Err(err) => return Err(format!("Touch processing error: {err}")),
        };

        if evt.event_type == InputEventType::RelativeMove {
            let w = inner.video_width.load(Ordering::Relaxed);
            let h = inner.video_height.load(Ordering::Relaxed);
            let (scale_w, scale_h) = if w > 0 && h > 0 {
                (w as f32, h as f32)
            } else {
                (1920.0, 1080.0)
            };
            evt.scroll_dx *= scale_w;
            evt.scroll_dy *= scale_h;
        }

        Ok(Some(evt))
    }

    pub fn handle_key(&self, key_code: u16, down: bool, modifiers_bits: u16) -> Result<(), String> {
        if key_code == 0 {
            return Err("Invalid key code 0".to_string());
        }
        const VALID_MODIFIERS_MASK: u16 = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 4);
        if modifiers_bits & !VALID_MODIFIERS_MASK != 0 {
            return Err(format!("Invalid modifier bits: 0x{modifiers_bits:04x}"));
        }
        let modifiers = Modifiers::from_bits_retain(modifiers_bits);

        let event = InputEvent {
            event_type: if down {
                InputEventType::KeyDown
            } else {
                InputEventType::KeyUp
            },
            x: 0.0,
            y: 0.0,
            key_code,
            modifiers,
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        };

        let inner = self
            .inner
            .lock()
            .map_err(|_| "State mutex is poisoned".to_string())?;

        if let Some(ref session) = inner.session {
            session
                .send_input(event)
                .map_err(|e| format!("Failed to send key input: {e}"))?;
        }
        Ok(())
    }

    pub async fn disconnect_async(&self) -> Result<(), String> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let state_clone = self.clone();
        tokio::task::spawn_blocking(move || state_clone.disconnect_blocking())
            .await
            .map_err(|e| format!("Disconnect task failed: {e}"))?
    }

    pub fn disconnect_blocking(&self) -> Result<(), String> {
        let (old_handles, old_tcp_runtime) = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| "State mutex is poisoned".to_string())?;

            if inner.state == ConnectionState::Idle && inner.session.is_none() {
                return Ok(());
            }

            inner.stop_flag.store(true, Ordering::SeqCst);

            let release_evt = inner.touch_handler.set_mode(TouchMode::DirectTouch);
            if let Some(ref session) = inner.session {
                if let Some(evt) = release_evt {
                    let _ = session.send_input(evt);
                }
                let _ = session.send_input(InputEvent {
                    event_type: InputEventType::Reset,
                    x: 0.0,
                    y: 0.0,
                    key_code: 0,
                    modifiers: Modifiers::empty(),
                    scroll_dx: 0.0,
                    scroll_dy: 0.0,
                });
                let _ = session.disconnect();
            }

            let tcp_rt = inner.tcp_runtime.take();
            let handles = std::mem::take(&mut inner.worker_handles);
            inner.session = None;
            let _ = inner.audio_queue.clear();
            *inner.latest_frame.lock().unwrap() = None;
            inner.state = ConnectionState::Idle;
            inner.host = None;
            inner.first_frame_presented.store(false, Ordering::Relaxed);
            (handles, tcp_rt)
        };

        if let Some(mut runtime) = old_tcp_runtime {
            let _ = runtime.stop();
        }

        let mut join_err = None;
        for handle in old_handles {
            if let Err(e) = handle.join() {
                tracing::error!("Worker join failed: {:?}", e);
                join_err = Some("Worker thread panicked during join".to_string());
            }
        }

        if let Some(err) = join_err {
            return Err(err);
        }

        tracing::info!("iOS session disconnect completed cleanly");
        Ok(())
    }

    pub async fn connect_async(
        &self,
        host: String,
        tcp_port: Option<u16>,
        udp_port: Option<u16>,
        pin: Option<String>,
    ) -> Result<SessionStats, String> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let state_clone = self.clone();
        tokio::task::spawn_blocking(move || state_clone.connect_blocking(host, tcp_port, udp_port, pin))
            .await
            .map_err(|e| format!("Connect task panicked: {e}"))?
    }

    fn connect_blocking(
        &self,
        host: String,
        tcp_port: Option<u16>,
        udp_port: Option<u16>,
        pin: Option<String>,
    ) -> Result<SessionStats, String> {
        self.disconnect_blocking()?;

        let current_generation = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| "State mutex is poisoned".to_string())?;
            inner.generation += 1;
            inner.state = ConnectionState::Connecting;
            inner.host = Some(host.clone());
            inner.frames_received.store(0, Ordering::Relaxed);
            inner.frames_decoded.store(0, Ordering::Relaxed);
            inner.audio_packets_received.store(0, Ordering::Relaxed);
            inner.audio_samples_played.store(0, Ordering::Relaxed);
            inner.video_width.store(0, Ordering::Relaxed);
            inner.video_height.store(0, Ordering::Relaxed);
            inner.frame_sequence.store(0, Ordering::Relaxed);
            inner.last_polled_sequence.store(0, Ordering::Relaxed);
            inner.first_frame_presented.store(false, Ordering::Relaxed);
            *inner.latest_frame.lock().unwrap() = None;
            *inner.last_error.lock().unwrap() = None;
            inner.stop_flag = Arc::new(AtomicBool::new(false));
            inner.generation
        };

        let config = match build_session_config(&host, tcp_port, udp_port, "EclipticRD iOS") {
            Ok(c) => c,
            Err(e) => {
                self.set_error(current_generation, format!("Configuration error: {e}"));
                return Err(format!("Configuration error: {e}"));
            }
        };

        tracing::info!(
            host = %config.host,
            tcp_port = config.tcp_port,
            udp_port = config.udp_port,
            "Connecting to host endpoint"
        );

        let session = match ClientSession::new(config) {
            Ok(s) => s,
            Err(e) => {
                self.set_error(current_generation, format!("Session init failed: {e}"));
                return Err(format!("Session init failed: {e}"));
            }
        };

        let ready_session: ReadySession = if let Some(ref pin_str) = pin {
            if pin_str.trim().is_empty() {
                match self.connect_via_keychain(&session, &host) {
                    Ok(r) => r,
                    Err(e) => {
                        self.set_error(current_generation, e.clone());
                        return Err(e);
                    }
                }
            } else {
                match session.pair_with_pin(pin_str.trim()) {
                    Ok(ready) => {
                        tracing::info!(host = %host, "Authenticated Ready session established via PIN");
                        ready
                    }
                    Err(e) => {
                        self.set_error(current_generation, format!("PIN pairing failed: {e}"));
                        return Err(format!("PIN pairing failed: {e}"));
                    }
                }
            }
        } else {
            match self.connect_via_keychain(&session, &host) {
                Ok(r) => r,
                Err(e) => {
                    self.set_error(current_generation, e.clone());
                    return Err(e);
                }
            }
        };

        let (audio_init_tx, audio_init_rx) = mpsc::sync_channel::<Result<(), String>>(1);
        let (audio_events_tx, audio_events_rx) = mpsc::sync_channel::<AudioOutputEvent>(128);
        let audio_queue = {
            let inner = self
                .inner
                .lock()
                .map_err(|_| "State mutex is poisoned".to_string())?;
            inner.audio_queue.clone()
        };

        let tcp_runtime = match session.spawn_tcp_runtime() {
            Ok(rt) => rt,
            Err(e) => {
                self.set_error(current_generation, format!("Failed to spawn TCP runtime: {e}"));
                return Err(format!("Failed to spawn TCP runtime: {e}"));
            }
        };

        let stop_flag = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| "State mutex is poisoned".to_string())?;
            if inner.generation != current_generation {
                drop(tcp_runtime);
                let _ = session.disconnect();
                return Err("Connection cancelled by newer session".to_string());
            }
            inner.state = ConnectionState::Ready;
            inner.session = Some(session.clone());
            inner.tcp_runtime = Some(tcp_runtime);
            inner.stop_flag.clone()
        };

        let mut worker_handles = Vec::new();

        let state_audio = self.clone();
        let stop_audio = stop_flag.clone();
        let audio_worker = thread::Builder::new()
            .name("erd-ios-audio".to_string())
            .spawn(move || {
                if let Err(e) = erd_render::activate_ios_audio_session() {
                    let err_msg = format!("Failed to activate iOS audio session: {e}");
                    tracing::error!(%err_msg);
                    let _ = audio_init_tx.send(Err(err_msg));
                    return;
                }

                let audio_output = match CpalAudioOutput::start_with_events(audio_queue, None, audio_events_tx) {
                    Ok(out) => {
                        let _ = audio_init_tx.send(Ok(()));
                        out
                    }
                    Err(e) => {
                        let err_msg = format!("Failed to start CPAL audio output: {e}");
                        tracing::error!(%err_msg);
                        let _ = audio_init_tx.send(Err(err_msg));
                        return;
                    }
                };

                while !stop_audio.load(Ordering::Relaxed) {
                    match audio_events_rx.recv_timeout(Duration::from_millis(50)) {
                        Ok(AudioOutputEvent::Callback {
                            consumed_samples: _,
                            total_consumed_samples,
                        }) => {
                            if let Ok(inner) = state_audio.inner.lock() {
                                if inner.generation == current_generation {
                                    inner.audio_samples_played.store(total_consumed_samples, Ordering::Relaxed);
                                }
                            }
                        }
                        Ok(AudioOutputEvent::Error(err)) => {
                            tracing::error!(%err, "Audio output error event");
                            if let Ok(mut inner) = state_audio.inner.lock() {
                                if inner.generation == current_generation {
                                    inner.state = ConnectionState::Error;
                                    *inner.last_error.lock().unwrap() = Some(format!("Audio error: {err}"));
                                }
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }

                drop(audio_output);
            })
            .map_err(|e| format!("Failed to spawn audio worker: {e}"))?;

        match audio_init_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => {
                tracing::info!("iOS CPAL audio output started");
            }
            Ok(Err(e)) => {
                self.set_error(current_generation, e.clone());
                stop_flag.store(true, Ordering::SeqCst);
                let _ = audio_worker.join();
                return Err(e);
            }
            Err(e) => {
                let err_msg = format!("Audio initialization timed out: {e}");
                self.set_error(current_generation, err_msg.clone());
                stop_flag.store(true, Ordering::SeqCst);
                let _ = audio_worker.join();
                return Err(err_msg);
            }
        }
        worker_handles.push(audio_worker);

        let state_media = self.clone();
        let stop_media = stop_flag.clone();
        let session_udp = session.clone();
        let media_worker = thread::Builder::new()
            .name("erd-ios-media".to_string())
            .spawn(move || {
                let mut decoder: Option<erd_decode::HevcDecoder> = None;
                let mut frames_received_local = 0u64;
                let mut frames_decoded_local = 0u64;
                let mut audio_packets_local = 0u64;
                let mut consecutive_decode_failures = 0u32;

                while !stop_media.load(Ordering::Relaxed) {
                    match session_udp.receive_udp_event() {
                        Ok(SessionEvent::Frame(assembled_frame)) => {
                            if stop_media.load(Ordering::Relaxed) {
                                break;
                            }
                            frames_received_local += 1;

                            let mut decode_failed = false;
                            if decoder.is_none() {
                                match erd_decode::detect_codec(&assembled_frame.data) {
                                    Ok(kind) => match kind {
                                        erd_decode::CodecKind::Hevc => {
                                            match erd_decode::HevcDecoder::from_keyframe(&assembled_frame.data) {
                                                Ok(dec) => {
                                                    tracing::info!("Decoder initialized from HEVC keyframe");
                                                    decoder = Some(dec);
                                                }
                                                Err(e) => {
                                                    tracing::debug!(%e, "HEVC keyframe init failed");
                                                    decode_failed = true;
                                                }
                                            }
                                        }
                                        erd_decode::CodecKind::H264 => {
                                            match erd_decode::h264_parameter_set_blob(&assembled_frame.data)
                                                .and_then(|ps| erd_decode::HevcDecoder::new_h264(&ps))
                                            {
                                                Ok(dec) => {
                                                    tracing::info!("Decoder initialized from H.264 parameter sets");
                                                    decoder = Some(dec);
                                                }
                                                Err(e) => {
                                                    tracing::debug!(%e, "H264 decoder init failed");
                                                    decode_failed = true;
                                                }
                                            }
                                        }
                                    },
                                    Err(e) => {
                                        tracing::debug!(%e, "Parameter set detection failed");
                                        decode_failed = true;
                                    }
                                }
                            }

                            if let Some(ref mut dec) = decoder {
                                match dec.decode(&assembled_frame.data, assembled_frame.timestamp_ms as i64) {
                                    Ok(nv12_frames) => {
                                        consecutive_decode_failures = 0;
                                        for frame in nv12_frames {
                                            frames_decoded_local += 1;
                                            let (width, height) = (frame.width, frame.height);
                                            let inner = match state_media.inner.lock() {
                                                Ok(guard) => guard,
                                                Err(_) => return,
                                            };
                                            if inner.generation != current_generation || stop_media.load(Ordering::Relaxed) {
                                                return;
                                            }
                                            let seq = inner.frame_sequence.fetch_add(1, Ordering::Relaxed) + 1;
                                            let repacked = repack_nv12_frame(&frame, seq);
                                            inner.video_width.store(width, Ordering::Relaxed);
                                            inner.video_height.store(height, Ordering::Relaxed);
                                            *inner.latest_frame.lock().unwrap() = Some(Arc::new(repacked));
                                            inner.frames_received.store(frames_received_local, Ordering::Relaxed);
                                            inner.frames_decoded.store(frames_decoded_local, Ordering::Relaxed);

                                            if frames_decoded_local == 1 || frames_decoded_local % 60 == 0 {
                                                tracing::info!(frames_decoded = frames_decoded_local, seq, width, height, "Decoded frame progress");
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::debug!(%e, "Frame decode error");
                                        decode_failed = true;
                                    }
                                }
                            }

                            if decode_failed {
                                decoder = None;
                                consecutive_decode_failures += 1;
                                if consecutive_decode_failures >= 15 {
                                    let err_msg = "Video decode failed repeatedly: exceeded keyframe retry threshold".to_string();
                                    tracing::error!(%err_msg);
                                    if let Ok(mut inner) = state_media.inner.lock() {
                                        if inner.generation == current_generation {
                                            inner.state = ConnectionState::Error;
                                            *inner.last_error.lock().unwrap() = Some(err_msg);
                                        }
                                    }
                                    break;
                                }
                                let _ = session_udp.send_control(ControlMessage::RequestKeyFrame);
                            }
                        }
                        Ok(SessionEvent::Audio(pcm)) => {
                            if stop_media.load(Ordering::Relaxed) {
                                break;
                            }
                            audio_packets_local += 1;
                            let inner = match state_media.inner.lock() {
                                Ok(guard) => guard,
                                Err(_) => return,
                            };
                            if inner.generation == current_generation {
                                inner.audio_packets_received.store(audio_packets_local, Ordering::Relaxed);
                                let _ = inner.audio_queue.push_pcm_bytes(&pcm);
                            }
                        }
                        Ok(SessionEvent::Ping) | Ok(SessionEvent::Cursor(_)) | Ok(SessionEvent::Ignored) => {}
                        Ok(SessionEvent::Clipboard(_)) | Ok(SessionEvent::StreamConfig(_)) => {}
                        Err(err) => {
                            if stop_media.load(Ordering::Relaxed) {
                                break;
                            }
                            if let SessionError::Io(ref io_err) = err {
                                if matches!(io_err.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) {
                                    thread::yield_now();
                                    continue;
                                }
                            }
                            let err_msg = format!("UDP media receiver stopped: {err}");
                            tracing::error!(%err_msg);
                            if let Ok(mut inner) = state_media.inner.lock() {
                                if inner.generation == current_generation {
                                    inner.state = ConnectionState::Error;
                                    *inner.last_error.lock().unwrap() = Some(err_msg);
                                }
                            }
                            break;
                        }
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn media receiver: {e}"))?;
        worker_handles.push(media_worker);

        {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| "State mutex is poisoned".to_string())?;
            if inner.generation == current_generation {
                inner.worker_handles = worker_handles;
            }
        }

        let server_w = ready_session.server.width as u32;
        let server_h = ready_session.server.height as u32;

        Ok(SessionStats {
            state: "ready".to_string(),
            host: Some(host),
            frames_received: 0,
            frames_decoded: 0,
            audio_packets_received: 0,
            audio_samples_played: 0,
            width: server_w,
            height: server_h,
            last_error: None,
        })
    }

    fn connect_via_keychain(&self, session: &ClientSession, host: &str) -> Result<ReadySession, String> {
        let store = erd_app::PairingStore::open_default()
            .map_err(|e| format!("Failed to open Keychain pairing store: {e}"))?;
        let record = store
            .find_by_host(host)
            .map_err(|e| format!("Keychain error: {e}"))?
            .ok_or_else(|| format!("No pairing record found for host '{host}'; PIN required"))?;

        session
            .connect_with_pairing(record)
            .map_err(|e| format!("Keychain pairing connection failed: {e}"))
    }

    pub fn presented(&self, sequence: u64) -> Result<(), String> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| "State mutex is poisoned".to_string())?;
        let latest_seq = inner.frame_sequence.load(Ordering::Relaxed);
        if sequence == 0 || sequence > latest_seq {
            return Err(format!(
                "Presented sequence {sequence} exceeds latest decoded sequence {latest_seq}"
            ));
        }
        let was_first = !inner.first_frame_presented.swap(true, Ordering::SeqCst);
        if was_first {
            tracing::info!(sequence, "First presented frame verified against decoded sequence");
        } else {
            tracing::debug!(sequence, "Frame presented report confirmed");
        }
        Ok(())
    }

    fn set_error(&self, generation: u64, error_msg: String) {
        if let Ok(mut inner) = self.inner.lock() {
            if inner.generation == generation {
                inner.state = ConnectionState::Error;
                *inner.last_error.lock().unwrap() = Some(error_msg);
            }
        }
    }
}

pub fn build_session_config(
    host: &str,
    tcp_port: Option<u16>,
    udp_port: Option<u16>,
    client_name: &str,
) -> Result<SessionConfig, String> {
    let mut trimmed = host.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        trimmed = trimmed[1..trimmed.len() - 1].trim();
    }
    if trimmed.is_empty() {
        return Err("Host cannot be empty".to_string());
    }

    if let Some(port) = tcp_port {
        if port == 0 {
            return Err("TCP port cannot be 0".to_string());
        }
    }
    if let Some(port) = udp_port {
        if port == 0 {
            return Err("UDP port cannot be 0".to_string());
        }
    }

    let mut config = SessionConfig::direct(trimmed, client_name);
    if let Some(port) = tcp_port {
        config.tcp_port = port;
    }
    if let Some(port) = udp_port {
        config.udp_port = port;
    }

    Ok(config)
}
