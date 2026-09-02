use std::{net::UdpSocket, sync::mpsc, thread, time::Duration};

use erd_app::{ClientSession, SessionConfig, SessionError, SessionState};
use erd_net::{PskIdentity, TlsPskServer};
use erd_proto::{
    Capabilities, Handshake, InputEvent, InputEventType, Modifiers, PacketHeader, PacketType,
    PairingGrant, PairingRequest, WireCodec, PROTOCOL_VERSION,
};

fn packet(packet_type: PacketType, payload: &[u8]) -> Vec<u8> {
    let mut packet = PacketHeader::new(packet_type, 0, 0, 0).encode().unwrap();
    packet.extend_from_slice(payload);
    packet
}

fn split_packet(packet: &[u8]) -> (PacketHeader, &[u8]) {
    (
        PacketHeader::decode(&packet[..PacketHeader::SIZE]).unwrap(),
        &packet[PacketHeader::SIZE..],
    )
}

#[test]
fn mock_server_pairing_handshake_and_input_round_trip() {
    let pin = "12345678";
    let psk = PskIdentity::bootstrap(pin).unwrap();
    let listener = TlsPskServer::new([psk])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let tcp_address = listener.local_addr().unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let udp_port = udp.local_addr().unwrap().port();
    let (input_tx, input_rx) = mpsc::channel();

    let server = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let request_packet = stream.read_frame().unwrap();
        let (header, payload) = split_packet(&request_packet);
        assert_eq!(header.packet_type, PacketType::PairingRequest);
        assert_eq!(PairingRequest::decode(payload).unwrap().name, "rust-client");

        let grant = PairingGrant {
            pairing_id: "pairing-1".to_owned(),
            host_name: "mock-host".to_owned(),
            key: [0x5a; 32],
        };
        stream
            .write_frame(&packet(PacketType::PairingGrant, &grant.encode().unwrap()))
            .unwrap();

        let handshake_packet = stream.read_frame().unwrap();
        let (header, payload) = split_packet(&handshake_packet);
        assert_eq!(header.packet_type, PacketType::Handshake);
        let handshake = Handshake::decode(payload).unwrap();
        assert_eq!(handshake.pairing_id, "pairing-1");
        assert_ne!(handshake.session_salt, [0; 16]);

        let acknowledgement = Handshake {
            name: "mock-host".to_owned(),
            width: 1920,
            height: 1080,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::TEXT_CLIPBOARD_SYNC,
            pairing_id: String::new(),
            session_salt: [0; 16],
        };
        stream
            .write_frame(&packet(
                PacketType::HandshakeAck,
                &acknowledgement.encode().unwrap(),
            ))
            .unwrap();

        let mut ping = [0_u8; 1];
        udp.recv_from(&mut ping).unwrap();
        assert_eq!(ping, [0xff]);

        let input_packet = stream.read_frame().unwrap();
        let (header, payload) = split_packet(&input_packet);
        assert_eq!(header.packet_type, PacketType::InputEvent);
        input_tx.send(InputEvent::decode(payload).unwrap()).unwrap();
    });

    let temporary = tempfile::tempdir().unwrap();
    let pairing_path = temporary.path().join("pairing-keys.json");
    let config = SessionConfig {
        host: "127.0.0.1".to_owned(),
        tcp_port: tcp_address.port(),
        udp_port,
        client_name: "rust-client".to_owned(),
        capabilities: Capabilities::TEXT_CLIPBOARD_SYNC,
        pairing_store_path: Some(pairing_path.clone()),
        connect_timeout: erd_app::CONNECT_TIMEOUT,
        handshake_ack_timeout: erd_app::HANDSHAKE_ACK_TIMEOUT,
    };
    let session = ClientSession::new(config).unwrap();
    let ready = session.pair_with_pin(pin).unwrap();
    assert_eq!(ready.server.name, "mock-host");
    assert_eq!(session.state().unwrap(), SessionState::Ready);

    let input = InputEvent {
        event_type: InputEventType::KeyDown,
        x: 0.0,
        y: 0.0,
        key_code: 0x00,
        modifiers: Modifiers::COMMAND,
        scroll_dx: 0.0,
        scroll_dy: 0.0,
    };
    session.send_input(input).unwrap();
    assert_eq!(input_rx.recv().unwrap(), input);
    server.join().unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(pairing_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn handshake_ack_timeout_is_reported() {
    let pin = "12345678";
    let psk = PskIdentity::bootstrap(pin).unwrap();
    let listener = TlsPskServer::new([psk])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let tcp_address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let _ = stream.read_frame().unwrap();
        let grant = PairingGrant {
            pairing_id: "pairing-timeout".to_owned(),
            host_name: "mock-host".to_owned(),
            key: [0x11; 32],
        };
        stream
            .write_frame(&packet(PacketType::PairingGrant, &grant.encode().unwrap()))
            .unwrap();
        let _ = stream.read_frame().unwrap();
        thread::sleep(Duration::from_millis(150));
    });
    let temporary = tempfile::tempdir().unwrap();
    let config = SessionConfig {
        host: "127.0.0.1".to_owned(),
        tcp_port: tcp_address.port(),
        udp_port: 9,
        client_name: "rust-client".to_owned(),
        capabilities: Capabilities::empty(),
        pairing_store_path: Some(temporary.path().join("pairings.json")),
        connect_timeout: Duration::from_secs(1),
        handshake_ack_timeout: Duration::from_millis(50),
    };
    let session = ClientSession::new(config).unwrap();
    assert!(matches!(
        session.pair_with_pin(pin),
        Err(SessionError::HandshakeAckTimeout)
    ));
    server.join().unwrap();
}

#[test]
fn connect_with_pairing_direct_round_trip() {
    let pairing_id = "test-psk-pairing-id";
    let key = [0x42; 32];
    let psk = PskIdentity::pairing(pairing_id, &key).unwrap();
    let listener = TlsPskServer::new([psk])
        .unwrap()
        .bind("127.0.0.1:0")
        .unwrap();
    let tcp_address = listener.local_addr().unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let udp_port = udp.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let mut stream = listener.accept().unwrap();
        let handshake_packet = stream.read_frame().unwrap();
        let (header, payload) = split_packet(&handshake_packet);
        assert_eq!(header.packet_type, PacketType::Handshake);
        let handshake = Handshake::decode(payload).unwrap();
        assert_eq!(handshake.pairing_id, pairing_id);

        let acknowledgement = Handshake {
            name: "mock-host".to_owned(),
            width: 1920,
            height: 1080,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::empty(),
            pairing_id: String::new(),
            session_salt: [0; 16],
        };
        stream
            .write_frame(&packet(
                PacketType::HandshakeAck,
                &acknowledgement.encode().unwrap(),
            ))
            .unwrap();

        let mut ping = [0_u8; 1];
        udp.recv_from(&mut ping).unwrap();
        assert_eq!(ping, [0xff]);
    });

    let config = SessionConfig {
        host: "127.0.0.1".to_owned(),
        tcp_port: tcp_address.port(),
        udp_port,
        client_name: "rust-client".to_owned(),
        capabilities: Capabilities::empty(),
        pairing_store_path: None,
        connect_timeout: Duration::from_secs(1),
        handshake_ack_timeout: Duration::from_secs(1),
    };
    let session = ClientSession::new(config).unwrap();
    let record = erd_app::PairingRecord {
        id: pairing_id.to_string(),
        name: "mock-host".to_string(),
        key: key.to_vec(),
        added_at_unix_ms: 0,
    };
    let ready = session.connect_with_pairing(record).unwrap();
    assert_eq!(ready.server.name, "mock-host");
    assert_eq!(session.state().unwrap(), SessionState::Ready);
    server.join().unwrap();
}

#[test]
fn pre_ready_input_is_refused() {
    let temporary = tempfile::tempdir().unwrap();
    let mut config = SessionConfig::direct("127.0.0.1", "rust-client");
    config.pairing_store_path = Some(temporary.path().join("pairings.json"));
    let session = ClientSession::new(config).unwrap();
    let event = InputEvent {
        event_type: InputEventType::MouseMove,
        x: 0.5,
        y: 0.5,
        key_code: 0,
        modifiers: Modifiers::empty(),
        scroll_dx: 0.0,
        scroll_dy: 0.0,
    };
    assert!(matches!(
        session.send_input(event),
        Err(SessionError::NotReady)
    ));
}
