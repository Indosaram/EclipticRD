use std::{
    net::{SocketAddr, TcpStream, UdpSocket},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use erd_app::{
    ClientSession, PairingRecord, ReceiverSnapshot, SessionConfig, SessionError, SessionEvent,
    RECEIVER_REORDER_GRACE,
};
use erd_net::{DatagramCipher, DatagramError, Direction, PskIdentity, TlsPskServer, TlsPskStream};
use erd_proto::{
    FrameChunk, FrameHeader, Handshake, PacketHeader, PacketType, TimestampStats, WireCodec,
};

struct Connected {
    session: ClientSession,
    udp: UdpSocket,
    peer: SocketAddr,
    cipher: DatagramCipher,
    _stream: TlsPskStream<TcpStream>,
    _temporary: tempfile::TempDir,
}

impl Connected {
    fn new() -> Self {
        let key = [0x39; 32];
        let listener = TlsPskServer::new([PskIdentity::pairing("telemetry", &key).unwrap()])
            .unwrap()
            .bind("127.0.0.1:0")
            .unwrap();
        let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        udp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let mut config = SessionConfig::direct("127.0.0.1", "telemetry");
        config.tcp_port = listener.local_addr().unwrap().port();
        config.udp_port = udp.local_addr().unwrap().port();
        config.connect_timeout = Duration::from_secs(5);
        config.handshake_ack_timeout = Duration::from_secs(5);
        config.pairing_store_path = Some(temporary.path().join("pairings.json"));
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            stream
                .ssl_stream()
                .get_ref()
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let frame = stream.read_frame().unwrap();
            assert_eq!(
                PacketHeader::decode(&frame[..PacketHeader::SIZE])
                    .unwrap()
                    .packet_type,
                PacketType::Handshake
            );
            let handshake = Handshake::decode(&frame[PacketHeader::SIZE..]).unwrap();
            let mut ack = PacketHeader::new(PacketType::HandshakeAck, 0, 0, 0)
                .encode()
                .unwrap();
            ack.extend_from_slice(&handshake.encode().unwrap());
            stream.write_frame(&ack).unwrap();
            let mut probe = [0; 1];
            let (_, peer) = udp.recv_from(&mut probe).unwrap();
            assert_eq!(probe, [0xff]);
            ready_tx
                .send((udp, peer, handshake.session_salt, stream))
                .unwrap();
        });
        let session = ClientSession::new(config).unwrap();
        session
            .connect_with_pairing(PairingRecord {
                id: "telemetry".into(),
                name: "fixture".into(),
                key: key.to_vec(),
                added_at_unix_ms: 0,
            })
            .unwrap();
        session
            .set_udp_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let (udp, peer, salt, stream) = ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        worker.join().unwrap();
        Self {
            session,
            udp,
            peer,
            cipher: DatagramCipher::derive(&key, &salt, Direction::HostToClient).unwrap(),
            _stream: stream,
            _temporary: temporary,
        }
    }

    fn seal(&mut self, kind: PacketType, sequence: u32, payload: &[u8]) -> Vec<u8> {
        self.cipher
            .seal_datagram(&PacketHeader::new(kind, sequence, 42, 0), payload)
            .unwrap()
    }

    fn receive(&self, bytes: &[u8]) -> Result<SessionEvent, SessionError> {
        self.udp.send_to(bytes, self.peer).unwrap();
        self.session.receive_udp_event()
    }
}

#[test]
fn authenticated_tls_udp_ingress_records_stats_and_assembly_without_changing_ping() {
    let mut fixture = Connected::new();
    assert_eq!(
        fixture.session.receiver_snapshot().unwrap(),
        ReceiverSnapshot::default()
    );
    let stats = TimestampStats {
        frame_id: 99,
        capture_us: 1000,
        encode_start_us: 1010,
        encode_end_us: 1030,
        send_us: 1060,
    };
    let ping = fixture.seal(PacketType::Ping, u32::MAX - 1, &stats.encode());
    let mut tampered = ping.clone();
    *tampered.last_mut().unwrap() ^= 1;
    assert!(matches!(
        fixture.receive(&tampered),
        Err(SessionError::Datagram(DatagramError::Authentication))
    ));
    assert_eq!(
        fixture.session.receiver_snapshot().unwrap(),
        ReceiverSnapshot::default()
    );
    assert_eq!(fixture.receive(&ping).unwrap(), SessionEvent::Ping);
    let observed = fixture.session.receiver_snapshot().unwrap();
    assert_eq!(observed.udp_authenticated_datagrams, 1);
    assert_eq!(observed.udp_authenticated_bytes, ping.len() as u64);
    assert_eq!(observed.host_ready_to_encode_us.unwrap().p50_us, 10);
    assert_eq!(observed.host_encode_us.unwrap().p50_us, 20);
    assert_eq!(observed.host_encode_to_send_complete_us.unwrap().p50_us, 30);
    assert!(matches!(
        fixture.receive(&ping),
        Err(SessionError::Datagram(DatagramError::Replay))
    ));
    assert_eq!(
        fixture
            .session
            .receiver_snapshot()
            .unwrap()
            .udp_authenticated_datagrams,
        1
    );

    // Orphan arrives before its header. Serial reordering grace is exercised with
    // constructed Instants in unit tests, not scheduler-speed assumptions here.
    let chunk = fixture.seal(
        PacketType::FrameChunk,
        u32::MAX,
        &FrameChunk {
            frame_id: 99,
            chunk_index: 0,
            data: b"abc".to_vec(),
        }
        .encode()
        .unwrap(),
    );
    assert_eq!(fixture.receive(&chunk).unwrap(), SessionEvent::Ignored);
    let header = fixture.seal(
        PacketType::FrameHeader,
        0,
        &FrameHeader {
            frame_id: 99,
            width: 1,
            height: 1,
            is_key_frame: false,
            total_chunks: 1,
            total_size: 3,
        }
        .encode()
        .unwrap(),
    );
    let completed_before = Instant::now();
    let frame = fixture.receive(&header).unwrap();
    let completed_after = Instant::now();
    assert!(
        matches!(frame, SessionEvent::Frame(frame) if frame.data == b"abc" && frame.timestamp_ms == 42)
    );
    assert_eq!(
        fixture
            .session
            .receiver_snapshot_at(completed_before)
            .unwrap()
            .receive_assembly_us,
        None
    );
    assert_eq!(
        fixture
            .session
            .receiver_snapshot_at(completed_after)
            .unwrap()
            .receive_assembly_us
            .unwrap()
            .frames,
        1
    );

    // A newly encrypted duplicate stats payload is accepted traffic but not another host sample.
    let duplicate = fixture.seal(PacketType::Ping, 1, &stats.encode());
    assert_eq!(fixture.receive(&duplicate).unwrap(), SessionEvent::Ping);
    let plain = fixture.seal(PacketType::Ping, 2, b"ordinary ping");
    assert_eq!(fixture.receive(&plain).unwrap(), SessionEvent::Ping);
    let malformed = fixture.seal(PacketType::Ping, 3, b"ERDTS1");
    assert_eq!(fixture.receive(&malformed).unwrap(), SessionEvent::Ping);
    let invalid = fixture.seal(
        PacketType::Ping,
        4,
        &TimestampStats {
            frame_id: 100,
            send_us: 1029,
            ..stats
        }
        .encode(),
    );
    assert_eq!(fixture.receive(&invalid).unwrap(), SessionEvent::Ping);
    let snapshot = fixture
        .session
        .receiver_snapshot_at(Instant::now() + RECEIVER_REORDER_GRACE)
        .unwrap();
    assert_eq!(snapshot.udp_authenticated_datagrams, 7);
    assert_eq!(
        snapshot.udp_authenticated_bytes,
        [&ping, &chunk, &header, &duplicate, &plain, &malformed, &invalid]
            .iter()
            .map(|d| d.len() as u64)
            .sum::<u64>()
    );
    assert_eq!(snapshot.host_encode_us.unwrap().frames, 1);
    assert_eq!(snapshot.loss_expected_packets, 7);
    assert_eq!(snapshot.loss_missing_packets, 0);
    assert_eq!(snapshot.packet_loss_ratio, Some(0.0));
    assert!(snapshot.udp_receive_bps.unwrap() > 0.0);
    let json = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(json["host_encode_us"]["p50_us"], 20);
    println!("AUTHENTICATED_RECEIVER_SNAPSHOT={json}");
    fixture.session.disconnect().unwrap();
    assert_eq!(
        fixture.session.receiver_snapshot().unwrap(),
        ReceiverSnapshot::default()
    );
    let empty = serde_json::to_value(fixture.session.receiver_snapshot().unwrap()).unwrap();
    assert!(empty["packet_loss_ratio"].is_null());
    assert!(empty["host_encode_us"].is_null());
    assert!(empty["udp_receive_bps"].is_null());
}

#[test]
fn mixed_udp_kinds_share_packet_inference_but_tcp_stats_and_invalid_auth_do_not() {
    use erd_proto::{AudioFragment, AudioFragmentHeader, ControlMessage, CursorUpdate};
    let mut fixture = Connected::new();
    let stats = TimestampStats {
        frame_id: 7,
        capture_us: 0,
        encode_start_us: 10,
        encode_end_us: 30,
        send_us: 60,
    };
    let mut tcp_ping = PacketHeader::new(PacketType::Ping, 500, 0, 0)
        .encode()
        .unwrap();
    tcp_ping.extend_from_slice(&stats.encode());
    fixture._stream.write_frame(&tcp_ping).unwrap();
    assert_eq!(
        fixture.session.receive_tcp_event().unwrap(),
        SessionEvent::Ping
    );
    let pong = fixture._stream.read_frame().unwrap();
    assert_eq!(
        ControlMessage::decode(&pong[PacketHeader::SIZE..]).unwrap(),
        ControlMessage::Pong
    );
    assert_eq!(
        fixture.session.receiver_snapshot().unwrap(),
        ReceiverSnapshot::default()
    );

    let cursor = fixture.seal(
        PacketType::CursorUpdate,
        10,
        &CursorUpdate {
            x: 0.25,
            y: 0.75,
            cursor_type: 1,
        }
        .encode()
        .unwrap(),
    );
    assert!(
        matches!(fixture.receive(&cursor).unwrap(), SessionEvent::Cursor(cursor) if cursor.x == 0.25)
    );
    let audio = fixture.seal(
        PacketType::AudioFrame,
        11,
        &AudioFragment {
            header: AudioFragmentHeader {
                frame_id: 1000,
                fragment_index: 0,
                fragment_count: 1,
            },
            data: vec![1, 2, 3],
        }
        .encode()
        .unwrap(),
    );
    assert_eq!(
        fixture.receive(&audio).unwrap(),
        SessionEvent::Audio(vec![1, 2, 3])
    );
    let malformed_frame = fixture.seal(PacketType::FrameHeader, 12, &[]);
    assert!(matches!(
        fixture.receive(&malformed_frame),
        Err(SessionError::Protocol(_))
    ));
    // A different nonce carrying the same sequence is real traffic, not another position.
    let duplicate_sequence = fixture.seal(PacketType::Ping, 12, &stats.encode());
    assert_eq!(
        fixture.receive(&duplicate_sequence).unwrap(),
        SessionEvent::Ping
    );
    let mut invalid = fixture.seal(PacketType::Ping, 14, &stats.encode());
    *invalid.last_mut().unwrap() ^= 1;
    assert!(matches!(
        fixture.receive(&invalid),
        Err(SessionError::Datagram(DatagramError::Authentication))
    ));
    let snapshot = fixture
        .session
        .receiver_snapshot_at(Instant::now() + RECEIVER_REORDER_GRACE)
        .unwrap();
    assert_eq!(snapshot.udp_authenticated_datagrams, 4);
    assert_eq!(
        snapshot.udp_authenticated_bytes,
        [&cursor, &audio, &malformed_frame, &duplicate_sequence]
            .iter()
            .map(|d| d.len() as u64)
            .sum::<u64>()
    );
    assert_eq!(
        (
            snapshot.loss_expected_packets,
            snapshot.loss_missing_packets
        ),
        (3, 0)
    );
    assert_eq!(snapshot.host_encode_us.unwrap().frames, 1);
    assert_eq!(snapshot.receive_assembly_us, None);
    println!(
        "MIXED_UDP_RECEIVER_SNAPSHOT={}",
        serde_json::to_string(&snapshot).unwrap()
    );
    fixture.session.disconnect().unwrap();
}
