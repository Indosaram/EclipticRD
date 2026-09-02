use std::fs::{self, OpenOptions};
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use erd_net::{
    BootstrapLockout, DatagramCipher, Direction, PskIdentity, TlsPskError, TlsPskServer,
    BOOTSTRAP_IDENTITY, PAIRING_IDENTITY_PREFIX,
};
use erd_proto::{
    AudioFragment, AudioFragmentHeader, BitrateAdjust, Capabilities, ControlMessage, FrameChunk,
    FrameHeader, Handshake, InputEvent, PacketHeader, PacketType, PairingGrant, PairingReject,
    PairingRejectReason, PairingRequest, WireCodec, MAX_AUDIO_FRAGMENT_BYTES,
    MAX_VIDEO_CHUNK_BYTES, PROTOCOL_VERSION,
};
use openssl::base64;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};
use uuid::Uuid;

#[cfg(target_os = "macos")]
use crate::capture_macos::{CaptureConfig, CaptureEvent, CaptureFrame, ScreenCapture};
#[cfg(target_os = "windows")]
use crate::capture_windows::{CaptureError, WindowsCapture};
#[cfg(target_os = "macos")]
use crate::encode_vt::{EncoderConfig, VideoToolboxEncoder, DEFAULT_BITRATE};
#[cfg(target_os = "windows")]
use crate::encode_windows::{EncoderConfig, MediaFoundationEncoder, VideoCodec};
#[cfg(target_os = "macos")]
use crate::inject_macos::InputInjector;
#[cfg(target_os = "windows")]
use crate::inject_windows::WindowsInputInjector;

pub const DEFAULT_TCP_PORT: u16 = 19_730;
pub const DEFAULT_UDP_PORT: u16 = 19_731;
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(30);
pub const PAIRING_WINDOW: Duration = Duration::from_secs(300);
pub const TIMESTAMP_STATS_MAGIC: &[u8; 6] = b"ERDTS1";
const SWIFT_REFERENCE_DATE_OFFSET: f64 = 978_307_200.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingRecord {
    pub id: String,
    pub name: String,
    pub key: [u8; 32],
    pub added_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
pub struct PairingStore {
    path: PathBuf,
    lock: Arc<Mutex<()>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiskPairingRecord {
    id: String,
    name: String,
    key: String,
    #[serde(
        default,
        alias = "addedAt",
        alias = "added_at",
        alias = "added_at_unix_ms",
        alias = "addedAtUnixMs"
    )]
    added_at: f64,
}

impl PairingStore {
    pub fn host_default() -> Result<Self, SessionError> {
        let directory = dirs::data_dir()
            .ok_or_else(|| SessionError::Store("Application Support is unavailable".into()))?
            .join("EclipticRD");
        Ok(Self::new(directory.join("pairing-keys.json")))
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_all(&self) -> Result<Vec<PairingRecord>, SessionError> {
        let _guard = self.lock.lock().expect("pairing store lock poisoned");
        self.load_all_unlocked()
    }

    pub fn load(&self, id: &str) -> Result<Option<PairingRecord>, SessionError> {
        Ok(self.load_all()?.into_iter().find(|record| record.id == id))
    }

    pub fn save(&self, record: PairingRecord) -> Result<(), SessionError> {
        let _guard = self.lock.lock().expect("pairing store lock poisoned");
        let mut records = self.load_all_unlocked()?;
        records.retain(|existing| existing.id != record.id);
        records.push(record);
        self.write_unlocked(&records)
    }

    pub fn revoke(&self, id: &str) -> Result<bool, SessionError> {
        let _guard = self.lock.lock().expect("pairing store lock poisoned");
        let mut records = self.load_all_unlocked()?;
        let original_len = records.len();
        records.retain(|record| record.id != id);
        if records.len() == original_len {
            return Ok(false);
        }
        self.write_unlocked(&records)?;
        Ok(true)
    }

    fn load_all_unlocked(&self) -> Result<Vec<PairingRecord>, SessionError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(SessionError::Io(error)),
        };
        let disk: Vec<DiskPairingRecord> = serde_json::from_slice(&bytes)
            .map_err(|error| SessionError::Store(error.to_string()))?;
        disk.into_iter()
            .map(|record| {
                let key = base64::decode_block(&record.key)
                    .map_err(|error| SessionError::Store(error.to_string()))?;
                let key: [u8; 32] = key
                    .try_into()
                    .map_err(|_| SessionError::Store("pairing key is not 32 bytes".into()))?;
                let unix_seconds = record.added_at + SWIFT_REFERENCE_DATE_OFFSET;
                Ok(PairingRecord {
                    id: record.id,
                    name: record.name,
                    key,
                    added_at_unix_ms: (unix_seconds.max(0.0) * 1_000.0).round() as u64,
                })
            })
            .collect()
    }

    fn write_unlocked(&self, records: &[PairingRecord]) -> Result<(), SessionError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let disk = records
            .iter()
            .map(|record| DiskPairingRecord {
                id: record.id.clone(),
                name: record.name.clone(),
                key: base64::encode_block(&record.key),
                added_at: record.added_at_unix_ms as f64 / 1_000.0 - SWIFT_REFERENCE_DATE_OFFSET,
            })
            .collect::<Vec<_>>();
        let bytes =
            serde_json::to_vec(&disk).map_err(|error| SessionError::Store(error.to_string()))?;
        let temporary = self.path.with_extension("json.tmp");
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        use std::io::Write;
        file.write_all(&bytes)?;
        file.sync_all()?;
        #[cfg(unix)]
        {
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
        }
        fs::rename(&temporary, &self.path)?;
        #[cfg(unix)]
        {
            fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }
}

pub fn random_pin() -> String {
    let mut rng = rand::rng();
    let mut bytes = [0_u8; 8];
    rng.fill_bytes(&mut bytes);
    let value = u64::from_le_bytes(bytes) % 100_000_000;
    format!("{value:08}")
}

pub struct ConsentPrompt {
    pub client_name: String,
    response: SyncSender<bool>,
}

impl ConsentPrompt {
    pub fn approve(self) {
        let _ = self.response.send(true);
    }

    pub fn reject(self) {
        let _ = self.response.send(false);
    }

    pub fn respond(self, approved: bool) {
        let _ = self.response.send(approved);
    }
}

#[derive(Clone)]
pub struct HostConfig {
    pub tcp_addr: SocketAddr,
    pub udp_addr: SocketAddr,
    pub bootstrap_pin: Option<String>,
    pub pairing_window: Duration,
    pub pairing_store: PairingStore,
    pub host_name: String,
    pub display: DisplayInfo,
    pub frames_per_second: u32,
    pub bitrate: u32,
    pub capture_audio: bool,
    pub consent_sender: Option<mpsc::Sender<ConsentPrompt>>,
}

impl HostConfig {
    #[cfg(target_os = "macos")]
    pub fn macos_default(
        bootstrap_pin: Option<String>,
        pairing_store: PairingStore,
    ) -> Result<Self, SessionError> {
        let display = ScreenCapture::display_info()?;
        Ok(Self {
            tcp_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_TCP_PORT)),
            udp_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_UDP_PORT)),
            bootstrap_pin,
            pairing_window: PAIRING_WINDOW,
            pairing_store,
            host_name: hostname::get()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            display,
            frames_per_second: 60,
            bitrate: DEFAULT_BITRATE,
            capture_audio: true,
            consent_sender: None,
        })
    }

    #[cfg(target_os = "windows")]
    pub fn windows_default(
        bootstrap_pin: Option<String>,
        pairing_store: PairingStore,
    ) -> Result<Self, SessionError> {
        let (pixel_width, pixel_height) =
            crate::capture_windows::WindowsCapture::primary_output_geometry()
                .map_err(|error| SessionError::Io(io::Error::other(error.to_string())))?;
        Ok(Self {
            tcp_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_TCP_PORT)),
            udp_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_UDP_PORT)),
            bootstrap_pin,
            pairing_window: PAIRING_WINDOW,
            pairing_store,
            host_name: hostname::get()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            display: DisplayInfo {
                logical_width: pixel_width,
                logical_height: pixel_height,
                pixel_width,
                pixel_height,
                scale_factor_milli: 1_000,
            },
            frames_per_second: 60,
            bitrate: WINDOWS_DEFAULT_BITRATE,
            capture_audio: false,
            consent_sender: None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    PreAuth,
    PairingGranted,
    Authenticated,
    Closed,
}

impl SessionState {
    pub fn allows(self, packet_type: PacketType) -> bool {
        match self {
            Self::PreAuth => matches!(
                packet_type,
                PacketType::PairingRequest | PacketType::Handshake
            ),
            Self::PairingGranted => matches!(packet_type, PacketType::Handshake),
            Self::Authenticated => matches!(
                packet_type,
                PacketType::Handshake | PacketType::InputEvent | PacketType::Control
            ),
            Self::Closed => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimestampStats {
    pub frame_id: u32,
    pub capture_us: u64,
    pub encode_start_us: u64,
    pub encode_end_us: u64,
    pub send_us: u64,
}

impl TimestampStats {
    pub const SIZE: usize = 6 + 4 + 8 * 4;

    pub fn encode(self) -> Vec<u8> {
        let mut output = Vec::with_capacity(Self::SIZE);
        output.extend_from_slice(TIMESTAMP_STATS_MAGIC);
        output.extend_from_slice(&self.frame_id.to_le_bytes());
        output.extend_from_slice(&self.capture_us.to_le_bytes());
        output.extend_from_slice(&self.encode_start_us.to_le_bytes());
        output.extend_from_slice(&self.encode_end_us.to_le_bytes());
        output.extend_from_slice(&self.send_us.to_le_bytes());
        output
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::SIZE || &bytes[..6] != TIMESTAMP_STATS_MAGIC {
            return None;
        }
        Some(Self {
            frame_id: u32::from_le_bytes(bytes[6..10].try_into().ok()?),
            capture_us: u64::from_le_bytes(bytes[10..18].try_into().ok()?),
            encode_start_us: u64::from_le_bytes(bytes[18..26].try_into().ok()?),
            encode_end_us: u64::from_le_bytes(bytes[26..34].try_into().ok()?),
            send_us: u64::from_le_bytes(bytes[34..42].try_into().ok()?),
        })
    }
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("TLS-PSK failed: {0}")]
    Tls(#[from] TlsPskError),
    #[error("protocol codec failed: {0}")]
    Codec(#[from] erd_proto::CodecError),
    #[error("UDP cipher failed: {0}")]
    Cipher(#[from] erd_net::DatagramError),
    #[cfg(target_os = "macos")]
    #[error("capture failed: {0}")]
    Capture(#[from] crate::capture_macos::CaptureError),
    #[cfg(target_os = "macos")]
    #[error("encoder failed: {0}")]
    Encode(#[from] crate::encode_vt::EncodeError),
    #[error("pairing store failed: {0}")]
    Store(String),
    #[error("invalid bootstrap PIN")]
    InvalidPin,
    #[error("pairing consent is unavailable")]
    ConsentUnavailable,
    #[error("pairing consent timed out")]
    ConsentTimeout,
    #[error("peer is not authenticated")]
    PreAuth,
    #[error("handshake pairing ID does not match the TLS identity")]
    IdentityMismatch,
    #[error("unknown pairing ID")]
    UnknownPairing,
    #[error("session salt is missing")]
    MissingSessionSalt,
    #[error("UDP peer is unavailable")]
    UdpPeerUnavailable,
    #[error("media pipeline stopped")]
    MediaStopped,
}

#[derive(Debug)]
enum MediaEvent {
    Video(VideoFrame),
    /// Windows pipeline has no audio source yet; the variant is kept so the
    /// wire shape stays identical across platforms.
    #[allow(dead_code)]
    Audio(Vec<u8>),
    Error(String),
}

/// Platform-neutral encoded frame handed from a [`MediaSource`] to the wire.
#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub data: Vec<u8>,
    pub is_key_frame: bool,
    pub capture_at: Instant,
    pub encode_started_at: Instant,
    pub encode_completed_at: Instant,
}

/// Platform-neutral display geometry shared by every media backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayInfo {
    pub logical_width: u32,
    pub logical_height: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub scale_factor_milli: u32,
}

impl DisplayInfo {
    pub fn scale_factor(self) -> f32 {
        self.scale_factor_milli as f32 / 1_000.0
    }
}

#[cfg(target_os = "windows")]
const WINDOWS_DEFAULT_BITRATE: u32 = 8_000_000;

trait MediaHandle {
    fn force_key_frame(&self) -> Result<(), SessionError>;
    fn update_bitrate(&self, bitrate: u32) -> Result<(), SessionError>;
    fn stop(&mut self);
}

trait MediaSource: Send + Sync {
    fn start(&self, sender: SyncSender<MediaEvent>) -> Result<Box<dyn MediaHandle>, SessionError>;
}

#[cfg(target_os = "macos")]
struct MacMediaSource {
    config: CaptureConfig,
    encoder: EncoderConfig,
}
#[cfg(target_os = "macos")]
struct MacMediaHandle {
    capture: Option<ScreenCapture>,
    encoder: Option<VideoToolboxEncoder>,
    capture_bridge: Option<thread::JoinHandle<()>>,
    encoded_bridge: Option<thread::JoinHandle<()>>,
}

#[cfg(target_os = "macos")]
impl MediaSource for MacMediaSource {
    fn start(&self, sender: SyncSender<MediaEvent>) -> Result<Box<dyn MediaHandle>, SessionError> {
        let (capture, capture_rx) = match ScreenCapture::start(self.config) {
            Ok((capture, rx)) => (Some(capture), rx),
            Err(err) => {
                warn!(%err, "ScreenCaptureKit unavailable; falling back to synthetic capture");
                let (tx, rx) = mpsc::sync_channel(2);
                let width = self.config.width;
                let height = self.config.height;
                let bytes_per_row = (width as usize) * 4;
                let frame_size = bytes_per_row * (height as usize);
                thread::Builder::new()
                    .name("erd-host-synthetic-capture".into())
                    .spawn(move || {
                        let mut frame_idx: u32 = 0;
                        let mut bgra = vec![0u8; frame_size];
                        for chunk in bgra.chunks_exact_mut(4) {
                            chunk[1] = 128;
                            chunk[2] = 200;
                            chunk[3] = 255;
                        }
                        loop {
                            let c = (frame_idx % 256) as u8;
                            // Update only the first 256 bytes for fast pattern
                            for (i, chunk) in bgra.chunks_exact_mut(4).take(1024).enumerate() {
                                chunk[0] = c.wrapping_add((i & 0xff) as u8);
                            }
                            let frame = CaptureFrame {
                                width,
                                height,
                                bytes_per_row,
                                bgra: bgra.clone(),
                                captured_at: Instant::now(),
                            };
                            if tx.send(CaptureEvent::Video(frame)).is_err() {
                                break;
                            }
                            frame_idx = frame_idx.wrapping_add(1);
                        }
                    })?;
                (None, rx)
            }
        };
        let (encoder, encoded_rx) = VideoToolboxEncoder::start(self.encoder)?;
        let command_sender = encoder.sender.clone();
        let capture_sender = sender.clone();
        let capture_bridge = thread::Builder::new()
            .name("erd-host-capture-bridge".into())
            .spawn(move || {
                let mut captured_count: u64 = 0;
                while let Ok(event) = capture_rx.recv() {
                    match event {
                        CaptureEvent::Video(frame) => {
                            captured_count += 1;
                            if captured_count == 1 || captured_count % 60 == 0 {
                                info!(captured_count, "Capture bridge received video frame");
                            }
                            if command_sender
                                .send(crate::encode_vt::Command::Frame(frame))
                                .is_err()
                            {
                                break;
                            }
                        }
                        CaptureEvent::Audio { pcm_f32_le, .. } => {
                            if capture_sender.send(MediaEvent::Audio(pcm_f32_le)).is_err() {
                                break;
                            }
                        }
                        CaptureEvent::Stopped(error) => {
                            let _ = capture_sender.send(MediaEvent::Error(error));
                            break;
                        }
                    }
                }
            })?;
        let encoded_bridge = thread::Builder::new()
            .name("erd-host-encoded-bridge".into())
            .spawn(move || {
                let mut enc_count: u64 = 0;
                while let Ok(result) = encoded_rx.recv() {
                    let event = match result {
                        Ok(frame) => {
                            enc_count += 1;
                            if enc_count == 1 || enc_count % 60 == 0 {
                                info!(
                                    enc_count,
                                    size = frame.data.len(),
                                    is_key = frame.is_key_frame,
                                    "Encoded bridge received frame"
                                );
                            }
                            MediaEvent::Video(VideoFrame {
                                data: frame.data,
                                is_key_frame: frame.is_key_frame,
                                capture_at: frame.capture_at,
                                encode_started_at: frame.encode_started_at,
                                encode_completed_at: frame.encode_completed_at,
                            })
                        }
                        Err(error) => {
                            warn!(%error, "Encoded bridge error");
                            MediaEvent::Error(error.to_string())
                        }
                    };
                    if sender.send(event).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Box::new(MacMediaHandle {
            capture,
            encoder: Some(encoder),
            capture_bridge: Some(capture_bridge),
            encoded_bridge: Some(encoded_bridge),
        }))
    }
}

#[cfg(target_os = "macos")]
impl MediaHandle for MacMediaHandle {
    fn force_key_frame(&self) -> Result<(), SessionError> {
        self.encoder
            .as_ref()
            .ok_or(SessionError::MediaStopped)?
            .force_key_frame()?;
        Ok(())
    }

    fn update_bitrate(&self, bitrate: u32) -> Result<(), SessionError> {
        self.encoder
            .as_ref()
            .ok_or(SessionError::MediaStopped)?
            .update_bitrate(bitrate)?;
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(capture) = self.capture.take() {
            let _ = capture.stop();
        }
        if let Some(encoder) = self.encoder.take() {
            encoder.stop();
        }
        if let Some(bridge) = self.capture_bridge.take() {
            let _ = bridge.join();
        }
        if let Some(bridge) = self.encoded_bridge.take() {
            let _ = bridge.join();
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for MacMediaHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
#[derive(Clone)]
struct SyntheticMediaSource {
    frame_count: u32,
    interval: Duration,
}

#[cfg(test)]
struct SyntheticMediaHandle;

#[cfg(test)]
impl MediaSource for SyntheticMediaSource {
    fn start(&self, sender: SyncSender<MediaEvent>) -> Result<Box<dyn MediaHandle>, SessionError> {
        let count = self.frame_count;
        let interval = self.interval;
        thread::spawn(move || {
            for index in 0..count {
                let capture_at = Instant::now();
                let encode_started_at = Instant::now();
                let mut data = Vec::new();
                let nalu = [0x26, 0x01, index as u8];
                data.extend_from_slice(&(nalu.len() as u32).to_be_bytes());
                data.extend_from_slice(&nalu);
                let frame = VideoFrame {
                    data,
                    is_key_frame: index == 0,
                    capture_at,
                    encode_started_at,
                    encode_completed_at: Instant::now(),
                };
                if sender.send(MediaEvent::Video(frame)).is_err() {
                    break;
                }
                if !interval.is_zero() {
                    thread::sleep(interval);
                }
            }
        });
        Ok(Box::new(SyntheticMediaHandle))
    }
}

#[cfg(test)]
impl MediaHandle for SyntheticMediaHandle {
    fn force_key_frame(&self) -> Result<(), SessionError> {
        Ok(())
    }

    fn update_bitrate(&self, _bitrate: u32) -> Result<(), SessionError> {
        Ok(())
    }

    fn stop(&mut self) {}
}

#[cfg(target_os = "windows")]
struct WindowsMediaSource {
    display_index: usize,
    fps: u32,
    width: u32,
    height: u32,
    bitrate: u32,
    codec: VideoCodec,
}

#[cfg(target_os = "windows")]
impl MediaSource for WindowsMediaSource {
    fn start(&self, sender: SyncSender<MediaEvent>) -> Result<Box<dyn MediaHandle>, SessionError> {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::mpsc::channel;

        enum PipelineFrame {
            Video {
                nv12: Vec<u8>,
            },
            /// Explicit stop frame; currently the channel closing plays this role.
            #[allow(dead_code)]
            Stop,
        }

        let stop = Arc::new(AtomicBool::new(false));
        let (frame_tx, frame_rx) = channel::<PipelineFrame>();

        // Capture thread: polls DXGI Desktop Duplication at the target cadence and
        // converts BGRA frames to NV12 for the hardware encoder.
        {
            let sender = sender.clone();
            let stop = Arc::clone(&stop);
            let display_index = self.display_index;
            let fps = self.fps.max(1);
            std::thread::Builder::new()
                .name("erd-win-capture".into())
                .spawn(move || {
                    let interval = Duration::from_micros(1_000_000 / fps as u64);
                    let mut capture =
                        match WindowsCapture::new(display_index, Duration::from_millis(1_000)) {
                            Ok(capture) => capture,
                            Err(error) => {
                                let _ =
                                    sender.send(MediaEvent::Error(format!("dxgi init: {error}")));
                                return;
                            }
                        };
                    while !stop.load(Ordering::Relaxed) {
                        let started = Instant::now();
                        match capture.acquire_next_frame(Duration::from_millis(250)) {
                            Ok(frame) => {
                                let nv12 = match bgra_to_nv12(
                                    frame.width,
                                    frame.height,
                                    &frame.bgra,
                                    frame.stride as usize,
                                ) {
                                    Ok(nv12) => nv12,
                                    Err(error) => {
                                        let _ = sender.send(MediaEvent::Error(format!(
                                            "nv12 conversion: {error}"
                                        )));
                                        return;
                                    }
                                };
                                if frame_tx.send(PipelineFrame::Video { nv12 }).is_err() {
                                    return;
                                }
                            }
                            Err(CaptureError::Timeout) => {}
                            Err(CaptureError::AccessLost) => {
                                capture = match WindowsCapture::new(
                                    display_index,
                                    Duration::from_millis(1_000),
                                ) {
                                    Ok(capture) => capture,
                                    Err(error) => {
                                        let _ = sender.send(MediaEvent::Error(format!(
                                            "dxgi reacquire: {error}"
                                        )));
                                        return;
                                    }
                                };
                            }
                            Err(error) => {
                                let _ = sender.send(MediaEvent::Error(format!("dxgi: {error}")));
                                return;
                            }
                        }
                        let elapsed = started.elapsed();
                        if elapsed < interval {
                            std::thread::sleep(interval - elapsed);
                        }
                    }
                })
                .map_err(|error| SessionError::Io(io::Error::other(error)))?;
        }

        // Encode thread: NV12 -> H264/HEVC via Media Foundation (or NVENC when present).
        {
            let stop = Arc::clone(&stop);
            let config = EncoderConfig {
                width: self.width,
                height: self.height,
                bitrate: self.bitrate,
                fps: self.fps.max(1),
                keyframe_interval: self.fps.max(1),
                preferred_codec: self.codec,
            };
            std::thread::Builder::new()
                .name("erd-win-encode".into())
                .spawn(move || {
                    let mut encoder = match MediaFoundationEncoder::new(config) {
                        Ok(encoder) => encoder,
                        Err(error) => {
                            let _ = sender.send(MediaEvent::Error(format!("mf init: {error}")));
                            return;
                        }
                    };
                    for frame in frame_rx {
                        match frame {
                            PipelineFrame::Stop => break,
                            PipelineFrame::Video { nv12 } => match encoder.encode_nv12(&nv12) {
                                Ok(Some(encoded)) => {
                                    let now = Instant::now();
                                    let _ = sender.send(MediaEvent::Video(VideoFrame {
                                        data: encoded.data,
                                        is_key_frame: encoded.is_key_frame,
                                        capture_at: now,
                                        encode_started_at: now,
                                        encode_completed_at: now,
                                    }));
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    let _ = sender
                                        .send(MediaEvent::Error(format!("mf encode: {error}")));
                                    return;
                                }
                            },
                        }
                    }
                    stop.store(true, Ordering::Relaxed);
                })
                .map_err(|error| SessionError::Io(io::Error::other(error)))?;
        }

        Ok(Box::new(WindowsMediaHandle { stop }))
    }
}

#[cfg(target_os = "windows")]
struct WindowsMediaHandle {
    stop: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(target_os = "windows")]
impl MediaHandle for WindowsMediaHandle {
    fn force_key_frame(&self) -> Result<(), SessionError> {
        // Media Foundation inserts key frames on its own cadence; the wire
        // protocol tolerates waiting for the next natural one.
        Ok(())
    }

    fn update_bitrate(&self, _bitrate: u32) -> Result<(), SessionError> {
        Ok(())
    }

    fn stop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// BT.601 limited-range BGRA8 -> NV12 conversion for tightly packed input.
#[cfg(target_os = "windows")]
fn bgra_to_nv12(
    width: u32,
    height: u32,
    bgra: &[u8],
    stride: usize,
) -> Result<Vec<u8>, &'static str> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || w % 2 != 0 || h % 2 != 0 {
        return Err("display dimensions must be non-zero and even");
    }
    if bgra.len() < stride * h || stride < w * 4 {
        return Err("bgra buffer smaller than stride * height");
    }
    let y_plane = w * h;
    let mut nv12 = vec![128u8; y_plane + y_plane / 2];
    let (y_plane, uv_plane) = nv12.split_at_mut(y_plane);
    for row in 0..h {
        let src = &bgra[row * stride..row * stride + w * 4];
        let y_row = &mut y_plane[row * w..(row + 1) * w];
        for col in (0..w).step_by(2) {
            let (b0, g0, r0) = (
                src[col * 4] as u32,
                src[col * 4 + 1] as u32,
                src[col * 4 + 2] as u32,
            );
            let (b1, g1, r1) = (
                src[col * 4 + 4] as u32,
                src[col * 4 + 5] as u32,
                src[col * 4 + 6] as u32,
            );
            y_row[col] = ((77 * r0 + 150 * g0 + 29 * b0) >> 8) as u8;
            y_row[col + 1] = ((77 * r1 + 150 * g1 + 29 * b1) >> 8) as u8;
            let (b_avg, g_avg, r_avg) = ((b0 + b1) / 2, (g0 + g1) / 2, (r0 + r1) / 2);
            let uv_index = (row / 2) * w + col;
            uv_plane[uv_index] =
                (128 + ((-43 * r_avg as i32 - 85 * g_avg as i32 + 128 * b_avg as i32) >> 8)) as u8;
            uv_plane[uv_index + 1] =
                (128 + ((128 * r_avg as i32 - 107 * g_avg as i32 - 21 * b_avg as i32) >> 8)) as u8;
        }
    }
    Ok(nv12)
}

pub struct HostServer {
    config: HostConfig,
    tcp_listener: TcpListener,
    udp_socket: UdpSocket,
    media_source: Arc<dyn MediaSource>,
    lockout: Arc<Mutex<BootstrapLockout>>,
    pairing_deadline: Option<Instant>,
}

impl HostServer {
    pub fn bind(config: HostConfig) -> Result<Self, SessionError> {
        let fps = if config.frames_per_second == 0 {
            60
        } else {
            config.frames_per_second
        };
        let media_source = Self::default_media_source(&config, fps)?;
        Self::bind_with_media(config, media_source)
    }

    #[cfg(target_os = "macos")]
    fn default_media_source(
        config: &HostConfig,
        fps: u32,
    ) -> Result<Arc<dyn MediaSource>, SessionError> {
        Ok(Arc::new(MacMediaSource {
            config: CaptureConfig {
                width: config.display.pixel_width,
                height: config.display.pixel_height,
                frames_per_second: fps,
                capture_audio: config.capture_audio,
            },
            encoder: EncoderConfig {
                width: config.display.pixel_width,
                height: config.display.pixel_height,
                frames_per_second: fps,
                bitrate: config.bitrate,
                key_frame_interval: fps,
            },
        }))
    }

    #[cfg(target_os = "windows")]
    fn default_media_source(
        config: &HostConfig,
        fps: u32,
    ) -> Result<Arc<dyn MediaSource>, SessionError> {
        Ok(Arc::new(WindowsMediaSource {
            display_index: 0,
            fps,
            width: config.display.pixel_width,
            height: config.display.pixel_height,
            bitrate: config.bitrate,
            codec: VideoCodec::H264,
        }))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn default_media_source(
        _config: &HostConfig,
        _fps: u32,
    ) -> Result<Arc<dyn MediaSource>, SessionError> {
        Err(SessionError::Store(
            "no media source is wired for this platform yet".into(),
        ))
    }

    #[cfg(test)]
    fn bind_synthetic(config: HostConfig, frame_count: u32) -> Result<Self, SessionError> {
        Self::bind_with_media(
            config,
            Arc::new(SyntheticMediaSource {
                frame_count,
                interval: Duration::ZERO,
            }),
        )
    }

    fn bind_with_media(
        config: HostConfig,
        media_source: Arc<dyn MediaSource>,
    ) -> Result<Self, SessionError> {
        if config
            .bootstrap_pin
            .as_ref()
            .is_some_and(|pin| pin.len() != 8 || !pin.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(SessionError::InvalidPin);
        }
        let tcp_listener = TcpListener::bind(config.tcp_addr)?;
        let udp_socket = UdpSocket::bind(config.udp_addr)?;
        let _ = rustix::net::sockopt::set_socket_recv_buffer_size(&udp_socket, 4 * 1024 * 1024);
        let _ = rustix::net::sockopt::set_socket_send_buffer_size(&udp_socket, 4 * 1024 * 1024);
        udp_socket.set_nonblocking(true)?;
        let lockout = Arc::new(Mutex::new(BootstrapLockout::default()));
        let pairing_deadline = config
            .bootstrap_pin
            .as_ref()
            .map(|_| Instant::now() + config.pairing_window);
        if pairing_deadline.is_some() {
            lockout.lock().expect("lockout poisoned").begin_pairing();
        }
        Ok(Self {
            config,
            tcp_listener,
            udp_socket,
            media_source,
            lockout,
            pairing_deadline,
        })
    }

    pub fn tcp_addr(&self) -> Result<SocketAddr, SessionError> {
        Ok(self.tcp_listener.local_addr()?)
    }

    pub fn udp_addr(&self) -> Result<SocketAddr, SessionError> {
        Ok(self.udp_socket.local_addr()?)
    }

    pub fn serve(self) -> Result<(), SessionError> {
        loop {
            if let Err(error) = self.serve_next() {
                warn!(%error, "connection ended with an error");
            }
        }
    }

    pub fn serve_n(&self, connection_count: usize) -> Result<(), SessionError> {
        for _ in 0..connection_count {
            self.serve_next()?;
        }
        Ok(())
    }

    fn serve_next(&self) -> Result<(), SessionError> {
        let (tcp, peer) = self.tcp_listener.accept()?;
        tcp.set_nodelay(true)?;
        let tls_server = TlsPskServer::new(self.current_psks()?)?;
        match tls_server.accept_stream(tcp) {
            Ok(stream) => self.handle_connection(stream, peer),
            Err(error) => {
                let locked = self
                    .lockout
                    .lock()
                    .expect("lockout poisoned")
                    .record_failure(Instant::now());
                if locked {
                    warn!("bootstrap TLS path locked after repeated failures");
                }
                Err(SessionError::Tls(error))
            }
        }
    }

    fn current_psks(&self) -> Result<Vec<PskIdentity>, SessionError> {
        let mut psks = self
            .config
            .pairing_store
            .load_all()?
            .into_iter()
            .map(|record| PskIdentity::pairing(&record.id, &record.key))
            .collect::<Result<Vec<_>, _>>()?;
        let pairing_active = self
            .pairing_deadline
            .is_some_and(|deadline| Instant::now() < deadline)
            && self
                .lockout
                .lock()
                .expect("lockout poisoned")
                .is_allowed(Instant::now());
        if pairing_active {
            if let Some(pin) = &self.config.bootstrap_pin {
                psks.push(PskIdentity::bootstrap(pin)?);
            }
        }
        if psks.is_empty() {
            let mut disabled_key = [0_u8; 32];
            rand::rng().fill_bytes(&mut disabled_key);
            psks.push(PskIdentity::new("erd-disabled", disabled_key.to_vec())?);
        }
        Ok(psks)
    }

    fn is_pairing_active(&self) -> bool {
        self.pairing_deadline
            .is_some_and(|deadline| Instant::now() < deadline)
            && self
                .lockout
                .lock()
                .expect("lockout poisoned")
                .is_allowed(Instant::now())
    }

    fn handle_connection(
        &self,
        mut stream: erd_net::TlsPskStream<TcpStream>,
        tcp_peer: SocketAddr,
    ) -> Result<(), SessionError> {
        stream
            .ssl_stream_mut()
            .get_mut()
            .set_read_timeout(Some(Duration::from_millis(5)))?;
        let negotiated_identity = stream
            .negotiated_identity()
            .and_then(|identity| std::str::from_utf8(identity).ok())
            .unwrap_or_default()
            .to_owned();
        let mut state = SessionState::PreAuth;
        let mut c2h_cipher = None;
        let mut h2c_cipher = None;
        let mut udp_peer = None;
        let mut media_receiver: Option<Receiver<MediaEvent>> = None;
        let mut media_handle: Option<Box<dyn MediaHandle>> = None;
        #[cfg(target_os = "macos")]
        let input = InputInjector::new(
            self.config.display.logical_width as f32,
            self.config.display.logical_height as f32,
        );
        #[cfg(target_os = "windows")]
        let mut input = WindowsInputInjector::new(None).map_err(|error| SessionError::Io(error))?;
        let session_origin = Instant::now();
        info!(identity = negotiated_identity, peer = %tcp_peer, "TLS-PSK session established");
        let mut last_pong = Instant::now();
        let mut next_ping = Instant::now() + HEARTBEAT_INTERVAL;
        let mut stop_sender_tx = None;
        let mut sender_thread = None;

        // If authenticated and media_receiver is set, we run UDP sending in a dedicated thread to avoid TCP blocking it.
        loop {
            self.discover_udp_peer(tcp_peer, &mut udp_peer, c2h_cipher.as_mut())?;
            if state == SessionState::Authenticated {
                if sender_thread.is_none() {
                    if let (Some(_), Some(peer)) = (&media_receiver, udp_peer) {
                        let receiver = media_receiver.take().unwrap();
                        let udp_socket = self.udp_socket.try_clone()?;
                        let mut cipher = h2c_cipher.take().ok_or(SessionError::PreAuth)?;
                        let pixel_width = self.config.display.pixel_width;
                        let pixel_height = self.config.display.pixel_height;
                        let (stx, srx) = mpsc::channel();
                        stop_sender_tx = Some(stx);
                        let handle = thread::spawn(move || {
                            let mut sender = UdpSender::default();
                            info!(%peer, "Starting UDP sender thread");
                            loop {
                                if srx.try_recv().is_ok() {
                                    info!("UDP sender received stop signal");
                                    break;
                                }
                                match receiver.recv_timeout(Duration::from_millis(5)) {
                                    Ok(MediaEvent::Video(frame)) => {
                                        let size = frame.data.len();
                                        let is_key = frame.is_key_frame;
                                        if let Err(err) = sender.send_frame(
                                            &udp_socket,
                                            peer,
                                            &mut cipher,
                                            pixel_width,
                                            pixel_height,
                                            frame,
                                            session_origin,
                                        ) {
                                            warn!(%err, "Failed to send video frame over UDP");
                                        } else {
                                            debug!(size, is_key, %peer, "Successfully sent video frame over UDP");
                                        }
                                    }
                                    Ok(MediaEvent::Audio(bytes)) => {
                                        let _ = sender.send_audio(
                                            &udp_socket,
                                            peer,
                                            &mut cipher,
                                            &bytes,
                                        );
                                    }
                                    Ok(MediaEvent::Error(err)) => {
                                        warn!(%err, "Media event error in sender thread");
                                        break;
                                    }
                                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                                        warn!("Media receiver disconnected, exiting sender thread");
                                        break;
                                    }
                                }
                            }
                        });
                        sender_thread = Some(handle);
                    }
                }
                let now = Instant::now();
                if now >= next_ping {
                    send_tcp_control(&mut stream, ControlMessage::Ping)?;
                    next_ping = now + HEARTBEAT_INTERVAL;
                }
                if now.duration_since(last_pong) >= HEARTBEAT_TIMEOUT {
                    warn!(peer = %tcp_peer, "heartbeat timeout");
                    break;
                }
            }

            let packet = match stream.read_frame() {
                Ok(packet) => packet,
                Err(TlsPskError::Io(error))
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    continue
                }
                Err(TlsPskError::Io(error))
                    if matches!(
                        error.kind(),
                        io::ErrorKind::UnexpectedEof
                            | io::ErrorKind::ConnectionReset
                            | io::ErrorKind::BrokenPipe
                    ) =>
                {
                    warn!(%error, "read_frame saw EOF/reset/broken pipe, closing connection");
                    break;
                }
                Err(error) => {
                    warn!(%error, "read_frame failed with error, terminating connection");
                    return Err(SessionError::Tls(error));
                }
            };
            let (header, payload) = split_packet(&packet)?;
            if !state.allows(header.packet_type) {
                debug!(?state, ?header.packet_type, "refusing pre-auth packet");
                continue;
            }

            match header.packet_type {
                PacketType::PairingRequest => {
                    if negotiated_identity != BOOTSTRAP_IDENTITY || !self.is_pairing_active() {
                        send_pairing_reject(&mut stream, PairingRejectReason::PairingDisabled)?;
                        continue;
                    }
                    let request = PairingRequest::decode(payload)?;
                    let Some(consent_sender) = &self.config.consent_sender else {
                        send_pairing_reject(&mut stream, PairingRejectReason::PairingDisabled)?;
                        continue;
                    };
                    let (response_tx, response_rx) = mpsc::sync_channel(1);
                    consent_sender
                        .send(ConsentPrompt {
                            client_name: request.name.clone(),
                            response: response_tx,
                        })
                        .map_err(|_| SessionError::ConsentUnavailable)?;
                    let approved = response_rx
                        .recv_timeout(self.config.pairing_window)
                        .map_err(|_| SessionError::ConsentTimeout)?;
                    if !approved {
                        send_pairing_reject(&mut stream, PairingRejectReason::DeniedByHost)?;
                        break;
                    }
                    let record = PairingRecord {
                        id: Uuid::new_v4().to_string().to_uppercase(),
                        name: request.name,
                        key: random_key(),
                        added_at_unix_ms: unix_ms_u64(),
                    };
                    self.config.pairing_store.save(record.clone())?;
                    let grant = PairingGrant {
                        pairing_id: record.id,
                        host_name: self.config.host_name.clone(),
                        key: record.key,
                    };
                    send_tcp_packet(&mut stream, PacketType::PairingGrant, &grant.encode()?)?;
                    state = SessionState::PairingGranted;
                }
                PacketType::Handshake => {
                    let handshake = Handshake::decode(payload)?;
                    let record = self
                        .config
                        .pairing_store
                        .load(&handshake.pairing_id)?
                        .ok_or(SessionError::UnknownPairing)?;
                    if let Some(identity_pairing_id) =
                        negotiated_identity.strip_prefix(PAIRING_IDENTITY_PREFIX)
                    {
                        if identity_pairing_id != handshake.pairing_id {
                            return Err(SessionError::IdentityMismatch);
                        }
                    } else if negotiated_identity != BOOTSTRAP_IDENTITY {
                        return Err(SessionError::IdentityMismatch);
                    }
                    if handshake.version != PROTOCOL_VERSION {
                        return Err(SessionError::Codec(
                            erd_proto::CodecError::UnsupportedVersion(handshake.version),
                        ));
                    }
                    c2h_cipher = Some(DatagramCipher::derive(
                        &record.key,
                        &handshake.session_salt,
                        Direction::ClientToHost,
                    )?);
                    h2c_cipher = Some(DatagramCipher::derive(
                        &record.key,
                        &handshake.session_salt,
                        Direction::HostToClient,
                    )?);
                    state = SessionState::Authenticated;
                    let ack = Handshake {
                        name: self.config.host_name.clone(),
                        width: self.config.display.logical_width.min(u16::MAX as u32) as u16,
                        height: self.config.display.logical_height.min(u16::MAX as u32) as u16,
                        scale: self.config.display.scale_factor(),
                        version: PROTOCOL_VERSION,
                        capabilities: Capabilities::STREAM_CONFIGURATION,
                        pairing_id: String::new(),
                        session_salt: [0_u8; 16],
                    };
                    send_tcp_packet(&mut stream, PacketType::HandshakeAck, &ack.encode()?)?;
                    let (media_tx, media_rx) = mpsc::sync_channel(16);
                    media_handle = Some(self.media_source.start(media_tx)?);
                    media_receiver = Some(media_rx);
                    last_pong = Instant::now();
                    next_ping = Instant::now() + HEARTBEAT_INTERVAL;
                    info!(
                        client = handshake.name,
                        "v3 handshake authenticated; UDP ciphers armed"
                    );
                }
                PacketType::InputEvent => {
                    if state != SessionState::Authenticated {
                        continue;
                    }
                    let event = InputEvent::decode(payload)?;
                    if let Err(error) = input.inject(&event) {
                        debug!(%error, "input event was not injected");
                    }
                }
                PacketType::Control => {
                    if state != SessionState::Authenticated {
                        continue;
                    }
                    match ControlMessage::decode(payload)? {
                        ControlMessage::RequestKeyFrame => {
                            if let Some(media) = &media_handle {
                                media.force_key_frame()?;
                            }
                        }
                        ControlMessage::BitrateAdjust(BitrateAdjust { target_bitrate })
                            if target_bitrate > 0 =>
                        {
                            if let Some(media) = &media_handle {
                                media.update_bitrate(target_bitrate as u32)?;
                            }
                        }
                        ControlMessage::Ping => {
                            send_tcp_control(&mut stream, ControlMessage::Pong)?;
                        }
                        ControlMessage::Pong => last_pong = Instant::now(),
                        ControlMessage::Disconnect | ControlMessage::StopStream => break,
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        if let Some(mut media) = media_handle {
            media.stop();
        }
        if let Some(tx) = stop_sender_tx {
            let _ = tx.send(());
        }
        if let Some(handle) = sender_thread {
            let _ = handle.join();
        }
        state = SessionState::Closed;
        debug!(?state, "session closed");
        Ok(())
    }

    fn discover_udp_peer(
        &self,
        tcp_peer: SocketAddr,
        udp_peer: &mut Option<SocketAddr>,
        receive_cipher: Option<&mut DatagramCipher>,
    ) -> Result<(), SessionError> {
        let mut buffer = [0_u8; 2_048];
        loop {
            match self.udp_socket.recv_from(&mut buffer) {
                Ok((length, peer)) if peer.ip() == tcp_peer.ip() => {
                    if length == 1 && buffer[0] == 0xff {
                        *udp_peer = Some(peer);
                    } else if let Some(cipher) = receive_cipher {
                        let _ = cipher.open_datagram(&buffer[..length]);
                        *udp_peer = Some(peer);
                    }
                    return Ok(());
                }
                Ok(_) => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) => return Err(SessionError::Io(error)),
            }
        }
    }
}

#[derive(Default)]
struct UdpSender {
    sequence: u32,
    frame_id: u32,
    audio_frame_id: u32,
}

impl UdpSender {
    fn send_packet(
        &mut self,
        socket: &UdpSocket,
        peer: SocketAddr,
        cipher: &mut DatagramCipher,
        packet_type: PacketType,
        payload: &[u8],
    ) -> Result<(), SessionError> {
        self.sequence = self.sequence.wrapping_add(1);
        let header = PacketHeader::new(packet_type, self.sequence, unix_ms_u32(), 0);
        let datagram = cipher.seal_datagram(&header, payload)?;
        socket.send_to(&datagram, peer)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn send_frame(
        &mut self,
        socket: &UdpSocket,
        peer: SocketAddr,
        cipher: &mut DatagramCipher,
        width: u32,
        height: u32,
        frame: VideoFrame,
        session_origin: Instant,
    ) -> Result<(), SessionError> {
        self.frame_id = self.frame_id.wrapping_add(1);
        let frame_id = self.frame_id;
        let chunk_count = frame.data.len().div_ceil(MAX_VIDEO_CHUNK_BYTES);
        let header = FrameHeader {
            frame_id,
            width: width.min(u16::MAX as u32) as u16,
            height: height.min(u16::MAX as u32) as u16,
            is_key_frame: frame.is_key_frame,
            total_chunks: u16::try_from(chunk_count)
                .map_err(|_| SessionError::Store("encoded frame has too many chunks".into()))?,
            total_size: u32::try_from(frame.data.len())
                .map_err(|_| SessionError::Store("encoded frame is too large".into()))?,
        };
        self.send_packet(
            socket,
            peer,
            cipher,
            PacketType::FrameHeader,
            &header.encode()?,
        )?;
        for (index, bytes) in frame.data.chunks(MAX_VIDEO_CHUNK_BYTES).enumerate() {
            let chunk = FrameChunk {
                frame_id,
                chunk_index: index as u16,
                data: bytes.to_vec(),
            };
            self.send_packet(
                socket,
                peer,
                cipher,
                PacketType::FrameChunk,
                &chunk.encode()?,
            )?;
        }
        let send_at = Instant::now();
        let stats = TimestampStats {
            frame_id,
            capture_us: monotonic_us(session_origin, frame.capture_at),
            encode_start_us: monotonic_us(session_origin, frame.encode_started_at),
            encode_end_us: monotonic_us(session_origin, frame.encode_completed_at),
            send_us: monotonic_us(session_origin, send_at),
        };
        self.send_packet(socket, peer, cipher, PacketType::Ping, &stats.encode())?;
        Ok(())
    }

    fn send_audio(
        &mut self,
        socket: &UdpSocket,
        peer: SocketAddr,
        cipher: &mut DatagramCipher,
        bytes: &[u8],
    ) -> Result<(), SessionError> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.audio_frame_id = self.audio_frame_id.wrapping_add(1);
        let fragment_count = bytes.len().div_ceil(MAX_AUDIO_FRAGMENT_BYTES);
        let fragment_count = u16::try_from(fragment_count)
            .map_err(|_| SessionError::Store("audio frame has too many fragments".into()))?;
        for (index, data) in bytes.chunks(MAX_AUDIO_FRAGMENT_BYTES).enumerate() {
            let fragment = AudioFragment {
                header: AudioFragmentHeader {
                    frame_id: self.audio_frame_id,
                    fragment_index: index as u16,
                    fragment_count,
                },
                data: data.to_vec(),
            };
            self.send_packet(
                socket,
                peer,
                cipher,
                PacketType::AudioFrame,
                &fragment.encode()?,
            )?;
        }
        Ok(())
    }
}

fn split_packet(packet: &[u8]) -> Result<(PacketHeader, &[u8]), SessionError> {
    if packet.len() < PacketHeader::SIZE {
        return Err(SessionError::Codec(erd_proto::CodecError::Truncated {
            field: "packet header",
            needed: PacketHeader::SIZE,
            remaining: packet.len(),
        }));
    }
    let header = PacketHeader::decode(&packet[..PacketHeader::SIZE])?;
    Ok((header, &packet[PacketHeader::SIZE..]))
}

fn send_tcp_packet(
    stream: &mut erd_net::TlsPskStream<TcpStream>,
    packet_type: PacketType,
    payload: &[u8],
) -> Result<(), SessionError> {
    let mut packet = PacketHeader::new(packet_type, 0, unix_ms_u32(), 0).encode()?;
    packet.extend_from_slice(payload);
    stream.write_frame(&packet)?;
    Ok(())
}

fn send_tcp_control(
    stream: &mut erd_net::TlsPskStream<TcpStream>,
    message: ControlMessage,
) -> Result<(), SessionError> {
    send_tcp_packet(stream, PacketType::Control, &message.encode()?)
}

fn send_pairing_reject(
    stream: &mut erd_net::TlsPskStream<TcpStream>,
    reason: PairingRejectReason,
) -> Result<(), SessionError> {
    send_tcp_packet(
        stream,
        PacketType::PairingReject,
        &PairingReject { reason }.encode()?,
    )
}

fn random_key() -> [u8; 32] {
    let mut key = [0_u8; 32];
    rand::rng().fill_bytes(&mut key);
    key
}

fn unix_ms_u64() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn unix_ms_u32() -> u32 {
    unix_ms_u64() as u32
}

fn monotonic_us(origin: Instant, value: Instant) -> u64 {
    value
        .checked_duration_since(origin)
        .unwrap_or_default()
        .as_micros() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use erd_net::{PskIdentity, TlsPskClient};
    use tempfile::tempdir;

    fn test_config(store: PairingStore, consent_sender: mpsc::Sender<ConsentPrompt>) -> HostConfig {
        HostConfig {
            tcp_addr: "127.0.0.1:0".parse().unwrap(),
            udp_addr: "127.0.0.1:0".parse().unwrap(),
            bootstrap_pin: Some("12345678".into()),
            pairing_window: Duration::from_secs(5),
            pairing_store: store,
            host_name: "test-host".into(),
            display: DisplayInfo {
                logical_width: 640,
                logical_height: 360,
                pixel_width: 640,
                pixel_height: 360,
                scale_factor_milli: 1_000,
            },
            frames_per_second: 60,
            bitrate: DEFAULT_BITRATE,
            capture_audio: false,
            consent_sender: Some(consent_sender),
        }
    }

    fn decode_tcp_packet(packet: &[u8]) -> (PacketHeader, &[u8]) {
        split_packet(packet).unwrap()
    }

    #[test]
    fn pre_auth_gating_refuses_input_and_control() {
        assert!(!SessionState::PreAuth.allows(PacketType::InputEvent));
        assert!(!SessionState::PreAuth.allows(PacketType::Control));
        assert!(SessionState::PreAuth.allows(PacketType::PairingRequest));
        assert!(SessionState::Authenticated.allows(PacketType::InputEvent));
    }

    #[test]
    fn lockout_disables_bootstrap_after_five_failures() {
        let now = Instant::now();
        let mut lockout = BootstrapLockout::default();
        lockout.begin_pairing();
        for attempt in 0..4 {
            assert!(!lockout.record_failure(now + Duration::from_secs(attempt)));
        }
        assert!(lockout.record_failure(now + Duration::from_secs(4)));
        assert!(!lockout.is_allowed(now + Duration::from_secs(5)));
    }

    #[test]
    fn direction_keys_arm_and_only_matching_direction_opens() {
        let key = [7_u8; 32];
        let salt = [9_u8; 16];
        let header = PacketHeader::new(PacketType::Ping, 1, 2, 0);
        let mut host_send = DatagramCipher::derive(&key, &salt, Direction::HostToClient).unwrap();
        let mut client_receive =
            DatagramCipher::derive(&key, &salt, Direction::HostToClient).unwrap();
        let mut wrong = DatagramCipher::derive(&key, &salt, Direction::ClientToHost).unwrap();
        let datagram = host_send.seal_datagram(&header, b"armed").unwrap();
        assert_eq!(client_receive.open_datagram(&datagram).unwrap().1, b"armed");
        assert!(wrong.open_datagram(&datagram).is_err());
    }

    #[test]
    fn pairing_store_mirrors_swift_json_and_is_mode_0600() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let record = PairingRecord {
            id: "id".into(),
            name: "client".into(),
            key: [3; 32],
            added_at_unix_ms: 1_700_000_000_000,
        };
        store.save(record.clone()).unwrap();
        assert_eq!(store.load("id").unwrap(), Some(record));
        assert_eq!(
            fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
        assert!(json[0]["key"].is_string());
        assert!(json[0]["addedAt"].is_number());
    }

    #[test]
    fn timestamp_stats_round_trip() {
        let stats = TimestampStats {
            frame_id: 7,
            capture_us: 10,
            encode_start_us: 20,
            encode_end_us: 30,
            send_us: 40,
        };
        assert_eq!(TimestampStats::decode(&stats.encode()), Some(stats));
    }

    #[test]
    fn scripted_loopback_pairs_arms_ciphers_and_receives_ten_timestamped_frames() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let (consent_tx, consent_rx) = mpsc::channel();
        let server = HostServer::bind_synthetic(test_config(store, consent_tx), 12).unwrap();
        let tcp_addr = server.tcp_addr().unwrap();
        let udp_addr = server.udp_addr().unwrap();
        let server_thread = thread::spawn(move || server.serve_n(1).unwrap());
        let consent_thread = thread::spawn(move || {
            let prompt = consent_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            assert_eq!(prompt.client_name, "scripted-client");
            prompt.approve();
        });

        let client = TlsPskClient::new(PskIdentity::bootstrap("12345678").unwrap()).unwrap();
        let mut tcp = client.connect(tcp_addr).unwrap();
        let request = PairingRequest {
            name: "scripted-client".into(),
        };
        let mut packet = PacketHeader::new(PacketType::PairingRequest, 0, 0, 0)
            .encode()
            .unwrap();
        packet.extend_from_slice(&request.encode().unwrap());
        tcp.write_frame(&packet).unwrap();
        let grant_packet = tcp.read_frame().unwrap();
        let (header, payload) = decode_tcp_packet(&grant_packet);
        assert_eq!(header.packet_type, PacketType::PairingGrant);
        let grant = PairingGrant::decode(payload).unwrap();

        let salt = [0x55_u8; 16];
        let handshake = Handshake {
            name: "scripted-client".into(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::empty(),
            pairing_id: grant.pairing_id,
            session_salt: salt,
        };
        let mut packet = PacketHeader::new(PacketType::Handshake, 0, 0, 0)
            .encode()
            .unwrap();
        packet.extend_from_slice(&handshake.encode().unwrap());
        tcp.write_frame(&packet).unwrap();
        let ack = tcp.read_frame().unwrap();
        assert_eq!(
            decode_tcp_packet(&ack).0.packet_type,
            PacketType::HandshakeAck
        );

        let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        udp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        udp.send_to(&[0xff], udp_addr).unwrap();
        let mut receive =
            DatagramCipher::derive(&grant.key, &salt, Direction::HostToClient).unwrap();
        let mut buffer = [0_u8; 2_048];
        let mut timestamps = Vec::new();
        while timestamps.len() < 10 {
            let (length, _) = udp.recv_from(&mut buffer).unwrap();
            let (header, payload) = receive.open_datagram(&buffer[..length]).unwrap();
            if header.packet_type == PacketType::Ping {
                if let Some(stats) = TimestampStats::decode(&payload) {
                    assert!(stats.capture_us <= stats.encode_start_us);
                    assert!(stats.encode_start_us <= stats.encode_end_us);
                    assert!(stats.encode_end_us <= stats.send_us);
                    timestamps.push(stats);
                }
            }
        }
        assert!(timestamps
            .windows(2)
            .all(|pair| pair[0].frame_id < pair[1].frame_id));
        send_tcp_control(&mut tcp, ControlMessage::Disconnect).unwrap();
        consent_thread.join().unwrap();
        server_thread.join().unwrap();
    }
}
