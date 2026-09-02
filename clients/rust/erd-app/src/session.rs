use std::{
    io,
    net::{SocketAddr, ToSocketAddrs, UdpSocket},
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use erd_net::{
    DatagramCipher, DatagramError, Direction, PskIdentity, TlsPskClient, TlsPskError, TlsPskStream,
};
use erd_proto::{
    AudioFragment, BitrateAdjust, Capabilities, ClipboardSyncDirection, ClipboardSyncOrigin,
    ClipboardSyncUpdate, ControlMessage, CursorUpdate, FrameChunk, FrameHeader, Handshake,
    InputEvent, PacketHeader, PacketType, PairingGrant, PairingReject, PairingRejectReason,
    PairingRequest, WireCodec, PROTOCOL_VERSION,
};
use rand::{rngs::OsRng, RngCore};
use thiserror::Error;

use crate::{
    AssembledFrame, AudioFragmentReassembler, CursorState, FrameAssembler, PairingRecord,
    PairingStore, PairingStoreError, PlatformClipboard,
};

pub const DEFAULT_TCP_PORT: u16 = 19_730;
pub const DEFAULT_UDP_PORT: u16 = 19_731;
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
pub const HANDSHAKE_ACK_TIMEOUT: Duration = Duration::from_secs(10);
const TCP_RUNTIME_READ_SLICE: Duration = Duration::from_millis(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Disconnected,
    Connecting,
    AwaitingPairing,
    AwaitingHandshakeAck,
    Ready,
}

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub host: String,
    pub tcp_port: u16,
    pub udp_port: u16,
    pub client_name: String,
    pub capabilities: Capabilities,
    pub pairing_store_path: Option<PathBuf>,
    pub connect_timeout: Duration,
    pub handshake_ack_timeout: Duration,
}

impl SessionConfig {
    pub fn direct(host: impl Into<String>, client_name: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            tcp_port: DEFAULT_TCP_PORT,
            udp_port: DEFAULT_UDP_PORT,
            client_name: client_name.into(),
            capabilities: Capabilities::STREAM_CONFIGURATION | Capabilities::TEXT_CLIPBOARD_SYNC,
            pairing_store_path: None,
            connect_timeout: CONNECT_TIMEOUT,
            handshake_ack_timeout: HANDSHAKE_ACK_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReadySession {
    pub pairing: PairingRecord,
    pub server: Handshake,
    pub session_salt: [u8; 16],
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    Frame(AssembledFrame),
    Audio(Vec<u8>),
    Cursor(CursorState),
    Clipboard(String),
    Ping,
    Ignored,
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session is not ready")]
    NotReady,
    #[error("pairing rejected: {0:?}")]
    PairingRejected(PairingRejectReason),
    #[error("pairing record {0} was not found")]
    PairingNotFound(String),
    #[error("expected {expected:?}, received {actual:?}")]
    UnexpectedPacket {
        expected: PacketType,
        actual: PacketType,
    },
    #[error("handshake acknowledgement timed out")]
    HandshakeAckTimeout,
    #[error("session lock was poisoned")]
    Poisoned,
    #[error("address resolution returned no endpoints")]
    NoAddress,
    #[error("TCP runtime stopped")]
    TcpRuntimeStopped,
    #[error("protocol error: {0}")]
    Protocol(#[from] erd_proto::CodecError),
    #[error("TLS transport error: {0}")]
    Tls(#[from] TlsPskError),
    #[error("UDP transport error: {0}")]
    Datagram(#[from] DatagramError),
    #[error("pairing store error: {0}")]
    PairingStore(#[from] PairingStoreError),
    #[error("I/O failed: {0}")]
    Io(#[from] io::Error),
}

struct SessionStateInner {
    state: SessionState,
    next_tcp_sequence: u32,
    next_udp_sequence: u32,
    pairing: Option<PairingRecord>,
    server: Option<Handshake>,
    session_salt: Option<[u8; 16]>,
    frames: FrameAssembler,
    audio: AudioFragmentReassembler,
    cursor: CursorState,
}

impl Default for SessionStateInner {
    fn default() -> Self {
        Self {
            state: SessionState::Disconnected,
            next_tcp_sequence: 0,
            next_udp_sequence: 0,
            pairing: None,
            server: None,
            session_salt: None,
            frames: FrameAssembler::default(),
            audio: AudioFragmentReassembler::default(),
            cursor: CursorState::default(),
        }
    }
}

/// Thread-safe client session orchestrator. Network reads are explicit so callers can own their runtime.
#[derive(Clone)]
pub struct ClientSession {
    config: SessionConfig,
    store: PairingStore,
    state: Arc<Mutex<SessionStateInner>>,
    tcp: Arc<Mutex<Option<TlsPskStream<std::net::TcpStream>>>>,
    udp: Arc<Mutex<Option<Arc<UdpSocket>>>>,
    udp_send: Arc<Mutex<Option<DatagramCipher>>>,
    udp_receive: Arc<Mutex<Option<DatagramCipher>>>,
}

impl ClientSession {
    pub fn new(config: SessionConfig) -> Result<Self, SessionError> {
        let store = match config.pairing_store_path.clone() {
            Some(path) => PairingStore::new(path),
            None => PairingStore::open_default()?,
        };
        Ok(Self {
            config,
            store,
            state: Arc::new(Mutex::new(SessionStateInner::default())),
            tcp: Arc::new(Mutex::new(None)),
            udp: Arc::new(Mutex::new(None)),
            udp_send: Arc::new(Mutex::new(None)),
            udp_receive: Arc::new(Mutex::new(None)),
        })
    }

    pub fn state(&self) -> Result<SessionState, SessionError> {
        Ok(self.state.lock().map_err(|_| SessionError::Poisoned)?.state)
    }

    pub fn pair_with_pin(&self, pin: &str) -> Result<ReadySession, SessionError> {
        let psk = PskIdentity::bootstrap(pin)?;
        self.connect_with_psk(psk, SessionState::AwaitingPairing)?;
        self.send_packet(
            PacketType::PairingRequest,
            &PairingRequest {
                name: self.config.client_name.clone(),
            }
            .encode()?,
        )?;
        let (header, payload) = self.read_tcp_packet()?;
        let pairing = match header.packet_type {
            PacketType::PairingGrant => {
                let grant = PairingGrant::decode(&payload)?;
                let record = PairingRecord {
                    id: grant.pairing_id,
                    name: grant.host_name,
                    key: grant.key.to_vec(),
                    added_at_unix_ms: current_unix_ms() as u64,
                };
                self.store.save(record.clone())?;
                record
            }
            PacketType::PairingReject => {
                return Err(SessionError::PairingRejected(
                    PairingReject::decode(&payload)?.reason,
                ));
            }
            actual => {
                return Err(SessionError::UnexpectedPacket {
                    expected: PacketType::PairingGrant,
                    actual,
                });
            }
        };
        self.begin_handshake(pairing)
    }

    pub fn reconnect(&self, pairing_id: &str) -> Result<ReadySession, SessionError> {
        let pairing = self
            .store
            .load(pairing_id)?
            .ok_or_else(|| SessionError::PairingNotFound(pairing_id.to_owned()))?;
        self.connect_with_pairing(pairing)
    }

    /// Connects directly using an explicit pairing record without consulting the pairing store.
    pub fn connect_with_pairing(&self, pairing: PairingRecord) -> Result<ReadySession, SessionError> {
        let psk = PskIdentity::pairing(&pairing.id, &pairing.key)?;
        self.connect_with_psk(psk, SessionState::AwaitingHandshakeAck)?;
        self.begin_handshake(pairing)
    }

    pub fn set_udp_read_timeout(&self, timeout: Option<Duration>) -> Result<(), SessionError> {
        let udp = self.udp.lock().map_err(|_| SessionError::Poisoned)?;
        if let Some(udp) = udp.as_ref() {
            udp.set_read_timeout(timeout)?;
        }
        Ok(())
    }

    pub fn send_input(&self, event: InputEvent) -> Result<(), SessionError> {
        self.send_ready_packet(PacketType::InputEvent, &event.encode()?)
    }

    pub fn send_control(&self, control: ControlMessage) -> Result<(), SessionError> {
        self.send_ready_packet(PacketType::Control, &control.encode()?)
    }

    pub fn send_udp(&self, packet_type: PacketType, payload: &[u8]) -> Result<(), SessionError> {
        let (sequence, udp) = {
            let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
            if state.state != SessionState::Ready {
                return Err(SessionError::NotReady);
            }
            state.next_udp_sequence = state.next_udp_sequence.wrapping_add(1);
            let udp_guard = self.udp.lock().map_err(|_| SessionError::Poisoned)?;
            let udp = udp_guard.as_ref().cloned().ok_or(SessionError::NotReady)?;
            (state.next_udp_sequence, udp)
        };
        let header = PacketHeader::new(packet_type, sequence, current_unix_ms(), 0);
        let mut cipher_guard = self.udp_send.lock().map_err(|_| SessionError::Poisoned)?;
        let datagram = cipher_guard
            .as_mut()
            .ok_or(SessionError::NotReady)?
            .seal_datagram(&header, payload)?;
        drop(cipher_guard);
        udp.send(&datagram)?;
        Ok(())
    }

    pub fn send_bitrate_adjust(&self, target_bitrate: i32) -> Result<(), SessionError> {
        self.send_control(ControlMessage::BitrateAdjust(BitrateAdjust {
            target_bitrate,
        }))
    }

    pub fn evaluate_abr(
        &self,
        abr: &mut crate::AbrController,
        now: std::time::Instant,
    ) -> Result<Option<i32>, SessionError> {
        let loss_ratio = self
            .state
            .lock()
            .map_err(|_| SessionError::Poisoned)?
            .frames
            .loss_ratio(now);
        let target = abr.evaluate(loss_ratio, now);
        if let Some(target) = target {
            self.send_bitrate_adjust(target)?;
        }
        Ok(target)
    }

    pub fn send_clipboard_text(&self, text: String) -> Result<(), SessionError> {
        self.send_control(ControlMessage::ClipboardSyncUpdate(ClipboardSyncUpdate {
            request_id: 0,
            direction: ClipboardSyncDirection::ClientToHost,
            origin: ClipboardSyncOrigin::LocalPasteboard,
            text,
        }))
    }

    pub fn apply_remote_clipboard<C: PlatformClipboard>(
        &self,
        clipboard: &C,
        text: &str,
    ) -> Result<u64, SessionError> {
        clipboard
            .set_text(text)
            .map_err(|error| SessionError::Io(io::Error::other(error.to_string())))
    }

    pub fn receive_tcp_event(&self) -> Result<SessionEvent, SessionError> {
        let (header, payload) = self.read_tcp_packet()?;
        self.handle_packet(header, payload, false)
    }

    /// Starts a background TCP control loop and exposes decoded session events.
    pub fn spawn_tcp_runtime(&self) -> Result<SessionRuntime, SessionError> {
        if self.state()? != SessionState::Ready {
            return Err(SessionError::NotReady);
        }
        {
            let mut tcp = self.tcp.lock().map_err(|_| SessionError::Poisoned)?;
            tcp.as_mut()
                .ok_or(SessionError::NotReady)?
                .ssl_stream_mut()
                .get_ref()
                .set_read_timeout(Some(TCP_RUNTIME_READ_SLICE))?;
        }
        let session = self.clone();
        let (stop_tx, stop_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let worker = thread::spawn(move || loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }
            match session.receive_tcp_event() {
                Ok(SessionEvent::Ignored) => {}
                Ok(event) => {
                    if event_tx.send(Ok(event)).is_err() {
                        break;
                    }
                }
                Err(SessionError::Tls(TlsPskError::Io(error)))
                    if matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) => {}
                Err(error) => {
                    let _ = event_tx.send(Err(error));
                    break;
                }
            }
        });
        Ok(SessionRuntime {
            events: event_rx,
            stop: Some(stop_tx),
            worker: Some(worker),
        })
    }

    pub fn receive_udp_event(&self) -> Result<SessionEvent, SessionError> {
        let udp = {
            let socket_guard = self.udp.lock().map_err(|_| SessionError::Poisoned)?;
            socket_guard.as_ref().cloned().ok_or(SessionError::NotReady)?
        };
        let mut datagram = [0_u8; 65_536];
        let received = udp.recv(&mut datagram)?;
        let mut cipher_guard = self.udp_receive.lock().map_err(|_| SessionError::Poisoned)?;
        let (header, payload) = cipher_guard
            .as_mut()
            .ok_or(SessionError::NotReady)?
            .open_datagram(&datagram[..received])?;
        drop(cipher_guard);
        self.handle_packet(header, payload, true)
    }

    pub fn disconnect(&self) -> Result<(), SessionError> {
        {
            let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
            state.state = SessionState::Disconnected;
            state.frames.clear();
            state.audio.clear();
        }
        *self.tcp.lock().map_err(|_| SessionError::Poisoned)? = None;
        *self.udp.lock().map_err(|_| SessionError::Poisoned)? = None;
        *self.udp_send.lock().map_err(|_| SessionError::Poisoned)? = None;
        *self.udp_receive.lock().map_err(|_| SessionError::Poisoned)? = None;
        Ok(())
    }

    fn connect_with_psk(
        &self,
        psk: PskIdentity,
        next_state: SessionState,
    ) -> Result<(), SessionError> {
        {
            let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
            state.state = SessionState::Connecting;
        }
        let address = resolve_one((&*self.config.host, self.config.tcp_port))?;
        let tcp = std::net::TcpStream::connect_timeout(&address, self.config.connect_timeout)?;
        tcp.set_read_timeout(Some(self.config.connect_timeout))?;
        tcp.set_write_timeout(Some(self.config.connect_timeout))?;
        tcp.set_nodelay(true)?;
        let stream = TlsPskClient::new(psk)?.connect_stream(tcp)?;
        *self.tcp.lock().map_err(|_| SessionError::Poisoned)? = Some(stream);
        let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        state.state = next_state;
        Ok(())
    }

    fn begin_handshake(&self, pairing: PairingRecord) -> Result<ReadySession, SessionError> {
        let mut salt = [0_u8; 16];
        OsRng.fill_bytes(&mut salt);
        let handshake = Handshake {
            name: self.config.client_name.clone(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: self.config.capabilities,
            pairing_id: pairing.id.clone(),
            session_salt: salt,
        };
        {
            let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
            state.pairing = Some(pairing.clone());
            state.session_salt = Some(salt);
            state.state = SessionState::AwaitingHandshakeAck;
            let mut tcp = self.tcp.lock().map_err(|_| SessionError::Poisoned)?;
            if let Some(tcp) = tcp.as_mut() {
                tcp.ssl_stream_mut()
                    .get_ref()
                    .set_read_timeout(Some(self.config.handshake_ack_timeout))?;
            }
        }
        self.send_packet(PacketType::Handshake, &handshake.encode()?)?;
        let (header, payload) = match self.read_tcp_packet() {
            Ok(packet) => packet,
            Err(SessionError::Tls(TlsPskError::Io(error)))
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                return Err(SessionError::HandshakeAckTimeout);
            }
            Err(error) => return Err(error),
        };
        if header.packet_type != PacketType::HandshakeAck {
            return Err(SessionError::UnexpectedPacket {
                expected: PacketType::HandshakeAck,
                actual: header.packet_type,
            });
        }
        let server = Handshake::decode(&payload)?;
        let key = pairing.key_array()?;
        let udp_send = DatagramCipher::derive(&key, &salt, Direction::ClientToHost)?;
        let udp_receive = DatagramCipher::derive(&key, &salt, Direction::HostToClient)?;
        let udp_address = resolve_one((&*self.config.host, self.config.udp_port))?;
        let udp = UdpSocket::bind(if udp_address.is_ipv6() {
            "[::]:0"
        } else {
            "0.0.0.0:0"
        })?;
        let _ = rustix::net::sockopt::set_socket_recv_buffer_size(&udp, 4 * 1024 * 1024);
        let _ = rustix::net::sockopt::set_socket_send_buffer_size(&udp, 4 * 1024 * 1024);
        udp.connect(udp_address)?;
        udp.set_read_timeout(Some(HEARTBEAT_INTERVAL * 3))?;
        udp.send(&[0xff])?;
        {
            let mut tcp = self.tcp.lock().map_err(|_| SessionError::Poisoned)?;
            if let Some(tcp) = tcp.as_mut() {
                tcp.ssl_stream_mut().get_ref().set_read_timeout(None)?;
            }
        }
        let ready = ReadySession {
            pairing: pairing.clone(),
            server: server.clone(),
            session_salt: salt,
        };
        *self.udp.lock().map_err(|_| SessionError::Poisoned)? = Some(Arc::new(udp));
        *self.udp_send.lock().map_err(|_| SessionError::Poisoned)? = Some(udp_send);
        *self.udp_receive.lock().map_err(|_| SessionError::Poisoned)? = Some(udp_receive);
        let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        state.server = Some(server);
        state.state = SessionState::Ready;
        Ok(ready)
    }

    fn send_ready_packet(
        &self,
        packet_type: PacketType,
        payload: &[u8],
    ) -> Result<(), SessionError> {
        if self.state()? != SessionState::Ready {
            return Err(SessionError::NotReady);
        }
        self.send_packet(packet_type, payload)
    }

    fn send_packet(&self, packet_type: PacketType, payload: &[u8]) -> Result<(), SessionError> {
        let sequence = {
            let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
            state.next_tcp_sequence = state.next_tcp_sequence.wrapping_add(1);
            state.next_tcp_sequence
        };
        let header = PacketHeader::new(packet_type, sequence, current_unix_ms(), 0);
        let mut packet = header.encode()?;
        packet.extend_from_slice(payload);
        let mut tcp = self.tcp.lock().map_err(|_| SessionError::Poisoned)?;
        tcp.as_mut()
            .ok_or(SessionError::NotReady)?
            .write_frame(&packet)?;
        Ok(())
    }

    fn read_tcp_packet(&self) -> Result<(PacketHeader, Vec<u8>), SessionError> {
        let mut tcp = self.tcp.lock().map_err(|_| SessionError::Poisoned)?;
        let frame = tcp
            .as_mut()
            .ok_or(SessionError::NotReady)?
            .read_frame()?;
        drop(tcp);
        if frame.len() < PacketHeader::SIZE {
            return Err(SessionError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "TCP packet is shorter than the v3 header",
            )));
        }
        Ok((
            PacketHeader::decode(&frame[..PacketHeader::SIZE])?,
            frame[PacketHeader::SIZE..].to_vec(),
        ))
    }

    fn handle_packet(
        &self,
        header: PacketHeader,
        payload: Vec<u8>,
        from_udp: bool,
    ) -> Result<SessionEvent, SessionError> {
        let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        if state.state != SessionState::Ready {
            return Err(SessionError::NotReady);
        }
        match header.packet_type {
            PacketType::FrameHeader if from_udp => Ok(state
                .frames
                .push_header(
                    FrameHeader::decode(&payload)?,
                    header.timestamp_ms,
                    std::time::Instant::now(),
                )?
                .map_or(SessionEvent::Ignored, SessionEvent::Frame)),
            PacketType::FrameChunk if from_udp => Ok(state
                .frames
                .push_chunk(FrameChunk::decode(&payload)?, std::time::Instant::now())?
                .map_or(SessionEvent::Ignored, SessionEvent::Frame)),
            PacketType::AudioFrame if from_udp => Ok(state
                .audio
                .push(AudioFragment::decode(&payload)?)
                .map_or(SessionEvent::Ignored, SessionEvent::Audio)),
            PacketType::CursorUpdate if from_udp => {
                state.cursor.update(CursorUpdate::decode(&payload)?);
                Ok(SessionEvent::Cursor(state.cursor))
            }
            PacketType::Ping if from_udp => Ok(SessionEvent::Ping),
            PacketType::Ping => {
                drop(state);
                self.send_control(ControlMessage::Pong)?;
                Ok(SessionEvent::Ping)
            }
            PacketType::Control => match ControlMessage::decode(&payload)? {
                ControlMessage::Ping => {
                    drop(state);
                    self.send_control(ControlMessage::Pong)?;
                    Ok(SessionEvent::Ping)
                }
                ControlMessage::ClipboardSyncUpdate(update) => {
                    Ok(SessionEvent::Clipboard(update.text))
                }
                _ => Ok(SessionEvent::Ignored),
            },
            _ => Ok(SessionEvent::Ignored),
        }
    }
}

pub struct SessionRuntime {
    events: mpsc::Receiver<Result<SessionEvent, SessionError>>,
    stop: Option<mpsc::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl SessionRuntime {
    pub fn events(&self) -> &mpsc::Receiver<Result<SessionEvent, SessionError>> {
        &self.events
    }

    pub fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for SessionRuntime {
    fn drop(&mut self) {
        self.stop();
    }
}

fn resolve_one(address: impl ToSocketAddrs) -> Result<SocketAddr, SessionError> {
    address
        .to_socket_addrs()?
        .next()
        .ok_or(SessionError::NoAddress)
}

fn current_unix_ms() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u32
}

impl From<crate::MediaAssemblyError> for SessionError {
    fn from(error: crate::MediaAssemblyError) -> Self {
        SessionError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            error.to_string(),
        ))
    }
}
