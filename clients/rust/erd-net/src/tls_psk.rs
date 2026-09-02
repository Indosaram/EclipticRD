//! OpenSSL TLS 1.2 PSK transport and v3 TCP framing.

use std::{
    collections::{HashMap, VecDeque},
    io::{self, Read, Write},
    net::{TcpListener, TcpStream, ToSocketAddrs},
    sync::Arc,
    time::{Duration, Instant},
};

use erd_proto::{TcpFrameEvent, TcpFrameReader, TcpFrameWriter};
use openssl::{
    error::ErrorStack,
    hash::MessageDigest,
    pkcs5,
    ssl::{
        HandshakeError, SslAcceptor, SslConnector, SslContextBuilder, SslMethod, SslStream,
        SslVerifyMode, SslVersion,
    },
};
use thiserror::Error;

use crate::udp_gcm::hkdf_sha256;

pub const BOOTSTRAP_IDENTITY: &str = "erd-b1";
pub const PAIRING_IDENTITY_PREFIX: &str = "erd-p1.";
pub const PSK_CIPHER_LIST: &str = "PSK-AES128-GCM-SHA256:PSK-AES256-GCM-SHA384";
pub const MAX_PAIRING_ATTEMPTS: usize = 5;
pub const PAIRING_ATTEMPT_WINDOW: Duration = Duration::from_secs(60);
pub const PAIRING_LOCKOUT: Duration = Duration::from_secs(300);
const BOOTSTRAP_SALT: &[u8] = b"erd/bootstrap/v3";
const BOOTSTRAP_STRETCH_ROUNDS: usize = 600_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PskIdentity {
    identity: String,
    key: Vec<u8>,
}

impl PskIdentity {
    pub fn bootstrap(pin: &str) -> Result<Self, TlsPskError> {
        Ok(Self {
            identity: BOOTSTRAP_IDENTITY.to_owned(),
            key: bootstrap_psk(pin)?.to_vec(),
        })
    }

    pub fn pairing(pairing_id: &str, key: &[u8]) -> Result<Self, TlsPskError> {
        if pairing_id.is_empty() {
            return Err(TlsPskError::InvalidIdentity);
        }
        if key.len() != 32 {
            return Err(TlsPskError::InvalidKeyLength);
        }
        Self::new(
            format!("{PAIRING_IDENTITY_PREFIX}{pairing_id}"),
            key.to_vec(),
        )
    }

    pub fn new(identity: impl Into<String>, key: Vec<u8>) -> Result<Self, TlsPskError> {
        let identity = identity.into();
        if identity.is_empty() || identity.as_bytes().contains(&0) {
            return Err(TlsPskError::InvalidIdentity);
        }
        if key.is_empty() {
            return Err(TlsPskError::InvalidKeyLength);
        }
        Ok(Self { identity, key })
    }

    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn key(&self) -> &[u8] {
        &self.key
    }
}

#[derive(Debug, Error)]
pub enum TlsPskError {
    #[error("PSK identity must be non-empty UTF-8 without NUL bytes")]
    InvalidIdentity,
    #[error("invalid PSK key length")]
    InvalidKeyLength,
    #[error("OpenSSL setup failed: {0}")]
    OpenSsl(#[from] ErrorStack),
    #[error("TLS handshake failed: {0}")]
    Handshake(String),
    #[error("TCP I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("invalid TCP frame: {0}")]
    Frame(#[from] erd_proto::CodecError),
    #[error("peer sent an invalid frame length {0}")]
    InvalidFrameLength(u32),
}

/// Host-side bootstrap failure tracker. A new pairing window resets lockout.
#[derive(Debug, Default)]
pub struct BootstrapLockout {
    pairing_active: bool,
    failure_count: usize,
    failure_window_start: Option<Instant>,
    locked_until: Option<Instant>,
}

impl BootstrapLockout {
    pub fn begin_pairing(&mut self) {
        self.pairing_active = true;
        self.failure_count = 0;
        self.failure_window_start = None;
        self.locked_until = None;
    }

    pub fn cancel_pairing(&mut self) {
        self.pairing_active = false;
    }

    pub fn is_allowed(&self, now: Instant) -> bool {
        self.pairing_active
            && self
                .locked_until
                .map_or(true, |locked_until| now >= locked_until)
    }

    /// Records a failed bootstrap handshake and returns whether it locked out.
    pub fn record_failure(&mut self, now: Instant) -> bool {
        if self.failure_window_start.map_or(true, |start| {
            now.saturating_duration_since(start) > PAIRING_ATTEMPT_WINDOW
        }) {
            self.failure_count = 0;
            self.failure_window_start = Some(now);
        }
        self.failure_count += 1;
        if self.failure_count >= MAX_PAIRING_ATTEMPTS {
            self.locked_until = Some(now + PAIRING_LOCKOUT);
            self.pairing_active = false;
            return true;
        }
        false
    }
}

pub struct TlsPskClient {
    connector: SslConnector,
}

impl TlsPskClient {
    pub fn new(psk: PskIdentity) -> Result<Self, TlsPskError> {
        let mut builder = SslConnector::builder(SslMethod::tls_client())?;
        configure_context(&mut builder)?;
        builder.set_psk_client_callback(move |_, _, identity_buffer, psk_buffer| {
            // OpenSSL's TLS 1.2 API requires a C-string identity. The identity
            // itself is copied byte-exactly; the trailing NUL is only the API
            // terminator and is not part of the offered identity.
            let identity = psk.identity.as_bytes();
            if identity.len() + 1 > identity_buffer.len() || psk.key.len() > psk_buffer.len() {
                return Err(ErrorStack::get());
            }
            identity_buffer[..identity.len()].copy_from_slice(identity);
            identity_buffer[identity.len()] = 0;
            psk_buffer[..psk.key.len()].copy_from_slice(&psk.key);
            Ok(psk.key.len())
        });
        Ok(Self {
            connector: builder.build(),
        })
    }

    pub fn connect<A: ToSocketAddrs>(
        &self,
        address: A,
    ) -> Result<TlsPskStream<TcpStream>, TlsPskError> {
        let tcp = TcpStream::connect(address)?;
        self.connect_stream(tcp)
    }

    pub fn connect_stream<S: Read + Write + std::fmt::Debug>(
        &self,
        stream: S,
    ) -> Result<TlsPskStream<S>, TlsPskError> {
        let configuration = self
            .connector
            .configure()?
            .use_server_name_indication(false)
            .verify_hostname(false);
        let stream = configuration
            .connect("erd-psk", stream)
            .map_err(handshake_error)?;
        Ok(TlsPskStream::new(stream))
    }
}

#[derive(Clone)]
pub struct TlsPskServer {
    acceptor: SslAcceptor,
}

impl TlsPskServer {
    pub fn new(psks: impl IntoIterator<Item = PskIdentity>) -> Result<Self, TlsPskError> {
        let keys = psks
            .into_iter()
            .map(|psk| (psk.identity.into_bytes(), psk.key))
            .collect::<HashMap<_, _>>();
        if keys.is_empty() {
            return Err(TlsPskError::InvalidKeyLength);
        }
        let keys = Arc::new(keys);

        let mut builder = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server())?;
        configure_context(&mut builder)?;
        builder.set_psk_server_callback(move |_, identity, psk_buffer| {
            let Some(key) = identity.and_then(|identity| keys.get(identity)) else {
                return Ok(0);
            };
            if key.len() > psk_buffer.len() {
                return Err(ErrorStack::get());
            }
            psk_buffer[..key.len()].copy_from_slice(key);
            Ok(key.len())
        });
        Ok(Self {
            acceptor: builder.build(),
        })
    }

    pub fn bind<A: ToSocketAddrs>(&self, address: A) -> Result<TlsPskListener, TlsPskError> {
        Ok(TlsPskListener {
            listener: TcpListener::bind(address)?,
            server: self.clone(),
        })
    }

    pub fn accept_stream<S: Read + Write + std::fmt::Debug>(
        &self,
        stream: S,
    ) -> Result<TlsPskStream<S>, TlsPskError> {
        let stream = self.acceptor.accept(stream).map_err(handshake_error)?;
        Ok(TlsPskStream::new(stream))
    }
}

pub struct TlsPskListener {
    listener: TcpListener,
    server: TlsPskServer,
}

impl TlsPskListener {
    pub fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.listener.local_addr()
    }

    pub fn accept(&self) -> Result<TlsPskStream<TcpStream>, TlsPskError> {
        let (stream, _) = self.listener.accept()?;
        self.server.accept_stream(stream)
    }
}

pub struct TlsPskStream<S> {
    stream: SslStream<S>,
    frame_reader: TcpFrameReader,
    pending_events: VecDeque<TcpFrameEvent>,
}

impl<S: Read + Write> TlsPskStream<S> {
    fn new(stream: SslStream<S>) -> Self {
        Self {
            stream,
            frame_reader: TcpFrameReader::new(),
            pending_events: VecDeque::new(),
        }
    }

    pub fn ssl_stream(&self) -> &SslStream<S> {
        &self.stream
    }

    pub fn ssl_stream_mut(&mut self) -> &mut SslStream<S> {
        &mut self.stream
    }

    pub fn negotiated_identity(&self) -> Option<&[u8]> {
        self.stream.ssl().psk_identity()
    }

    pub fn write_frame(&mut self, payload: &[u8]) -> Result<(), TlsPskError> {
        let frame = TcpFrameWriter::encode(payload)?;
        self.stream.write_all(&frame)?;
        self.stream.flush()?;
        Ok(())
    }

    /// Reads one framed payload, buffering partial reads and coalesced frames.
    /// Zero or oversized lengths drop the complete pending framing buffer.
    pub fn read_frame(&mut self) -> Result<Vec<u8>, TlsPskError> {
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            if let Some(event) = self.pending_events.pop_front() {
                return match event {
                    TcpFrameEvent::Frame(frame) => Ok(frame),
                    TcpFrameEvent::DroppedInvalidLength(length) => {
                        Err(TlsPskError::InvalidFrameLength(length))
                    }
                };
            }

            let count = self.stream.read(&mut buffer)?;
            if count == 0 {
                return Err(TlsPskError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "TLS stream closed while reading frame",
                )));
            }
            self.pending_events
                .extend(self.frame_reader.push(&buffer[..count]));
        }
    }
}

pub fn bootstrap_psk(pin: &str) -> Result<[u8; 32], TlsPskError> {
    if pin.len() != 8 || !pin.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(TlsPskError::InvalidIdentity);
    }
    let mut stretched = [0_u8; 32];
    pkcs5::pbkdf2_hmac(
        pin.as_bytes(),
        BOOTSTRAP_SALT,
        BOOTSTRAP_STRETCH_ROUNDS,
        MessageDigest::sha256(),
        &mut stretched,
    )?;
    let derived = hkdf_sha256(&stretched, BOOTSTRAP_SALT, b"erd/tls-psk", 32);
    let mut output = [0_u8; 32];
    output.copy_from_slice(&derived);
    Ok(output)
}

fn configure_context(builder: &mut SslContextBuilder) -> Result<(), ErrorStack> {
    builder.set_min_proto_version(Some(SslVersion::TLS1_2))?;
    // TLS 1.3 external PSK is incompatible with the Apple Network.framework
    // peer. Keep max TLS 1.3 for policy parity, but disable TLS 1.3 cipher
    // suites so negotiation is pinned to the interoperable TLS 1.2 PSK path.
    builder.set_max_proto_version(Some(SslVersion::TLS1_3))?;
    builder.set_cipher_list(PSK_CIPHER_LIST)?;
    // OpenSSL's legacy PSK callback is TLS 1.2-only. NO_TLSV1_3 prevents it
    // from selecting a TLS 1.3 suite while retaining the explicit v3 policy
    // maximum above for stacks that later gain compatible external PSKs.
    builder.set_options(openssl::ssl::SslOptions::NO_TLSV1_3);
    builder.set_verify_callback(SslVerifyMode::PEER, |_, _| true);
    Ok(())
}

fn handshake_error<S: std::fmt::Debug>(error: HandshakeError<S>) -> TlsPskError {
    TlsPskError::Handshake(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        net::TcpStream,
        sync::{Arc, Barrier},
        thread,
    };

    use erd_proto::MAX_TCP_FRAME_SIZE;

    use super::*;

    #[test]
    fn bootstrap_key_is_deterministic_per_pin() {
        let expected = [
            0x2f, 0x88, 0x83, 0xb2, 0xf5, 0x6f, 0x8d, 0xe0, 0x2a, 0xc8, 0x8b, 0x3b, 0xf9, 0x21,
            0x3b, 0x70, 0x1d, 0xde, 0x0e, 0x35, 0x31, 0xc0, 0x4c, 0x9f, 0x29, 0xc5, 0xf0, 0x5f,
            0xd7, 0x4a, 0xe6, 0xfb,
        ];
        assert_eq!(bootstrap_psk("12345678").unwrap(), expected);
        assert_eq!(
            bootstrap_psk("12345678").unwrap(),
            bootstrap_psk("12345678").unwrap()
        );
        assert_ne!(
            bootstrap_psk("12345678").unwrap(),
            bootstrap_psk("87654321").unwrap()
        );
    }

    #[test]
    fn pairing_identity_uses_opaque_id() {
        let psk = PskIdentity::pairing("550e8400-e29b-41d4-a716-446655440000", &[7; 32]).unwrap();
        assert_eq!(
            psk.identity(),
            "erd-p1.550e8400-e29b-41d4-a716-446655440000"
        );
        assert_eq!(psk.key(), &[7; 32]);
    }

    #[test]
    fn bootstrap_lockout_matches_identity_semantics() {
        let start = Instant::now();
        let mut lockout = BootstrapLockout::default();
        lockout.begin_pairing();
        for attempt in 1..MAX_PAIRING_ATTEMPTS {
            assert!(!lockout.record_failure(start + Duration::from_secs(attempt as u64)));
        }
        assert!(lockout.record_failure(start + Duration::from_secs(5)));
        assert!(!lockout.is_allowed(start + Duration::from_secs(6)));
        // ERDIdentity cancels the pending pairing when lockout is reached;
        // expiry alone does not reopen bootstrap without a fresh window.
        assert!(!lockout.is_allowed(start + PAIRING_LOCKOUT + Duration::from_secs(6)));
        lockout.begin_pairing();
        assert!(lockout.is_allowed(start));
    }

    #[test]
    fn psk_loopback_completes_handshake_and_framed_echo() {
        let psk = PskIdentity::bootstrap("12345678").unwrap();
        let server = TlsPskServer::new([psk.clone()]).unwrap();
        let listener = server.bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server_thread = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            assert_eq!(
                stream.negotiated_identity(),
                Some(BOOTSTRAP_IDENTITY.as_bytes())
            );
            let request = stream.read_frame().unwrap();
            assert_eq!(request, b"partial framed echo");
            stream.write_frame(&request).unwrap();
        });

        let client = TlsPskClient::new(psk).unwrap();
        let mut stream = client.connect(address).unwrap();
        assert_eq!(stream.ssl_stream().ssl().version_str(), "TLSv1.2");
        stream.write_frame(b"partial framed echo").unwrap();
        assert_eq!(stream.read_frame().unwrap(), b"partial framed echo");
        server_thread.join().unwrap();
    }

    #[test]
    fn framing_handles_partial_reads_and_drops_oversized_buffer() {
        let psk = PskIdentity::bootstrap("12345678").unwrap();
        let server = TlsPskServer::new([psk.clone()]).unwrap();
        let listener = server.bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let server_barrier = Arc::clone(&barrier);
        let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let server_observed = Arc::clone(&observed);
        let server_thread = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            let first = stream.read_frame().unwrap();
            server_observed.lock().unwrap().push(first);
            assert!(matches!(
                stream.read_frame(),
                Err(TlsPskError::InvalidFrameLength(length))
                    if length == (MAX_TCP_FRAME_SIZE as u32) + 1
            ));
            server_barrier.wait();
        });

        let client = TlsPskClient::new(psk).unwrap();
        let tcp = TcpStream::connect(address).unwrap();
        let mut stream = client.connect_stream(tcp).unwrap();
        let frame = TcpFrameWriter::encode(b"split").unwrap();
        stream.ssl_stream_mut().write_all(&frame[..2]).unwrap();
        stream.ssl_stream_mut().flush().unwrap();
        stream.ssl_stream_mut().write_all(&frame[2..]).unwrap();
        stream
            .ssl_stream_mut()
            .write_all(&((MAX_TCP_FRAME_SIZE as u32) + 1).to_le_bytes())
            .unwrap();
        stream
            .ssl_stream_mut()
            .write_all(b"discarded trailing bytes")
            .unwrap();
        stream.ssl_stream_mut().flush().unwrap();
        barrier.wait();
        server_thread.join().unwrap();
        assert_eq!(*observed.lock().unwrap(), vec![b"split".to_vec()]);
    }
}
