use super::*;

#[test]
fn sender_failed_sink_records_attempt_error_and_nonce_progression() {
    // Given the actual encryption/send seam and an isolated trace sink.
    let directory = tempfile::tempdir().unwrap();
    let trace = host_trace::Trace::for_test(directory.path().join("host.json"));
    let mut sender = UdpSender {
        trace: Some(Arc::clone(&trace)),
        ..Default::default()
    };
    let mut cipher =
        DatagramCipher::derive(&[0x31; 32], &[0x72; 16], Direction::HostToClient).unwrap();
    let mut receiver =
        DatagramCipher::derive(&[0x31; 32], &[0x72; 16], Direction::HostToClient).unwrap();
    let mut emitted = Vec::new();
    // When only attempted packet 2 fails, without rollback or retry.
    for sequence in 1..=3 {
        let result = sender.send_selected_packet(
            &mut cipher,
            PacketType::FrameChunk,
            &[7, 0, 0, 0, 0, 0],
            |bytes| {
                if sequence == 2 {
                    return Err(io::Error::from_raw_os_error(105));
                }
                emitted.push(bytes.to_vec());
                Ok(bytes.len())
            },
        );
        assert_eq!(result.is_err(), sequence == 2);
    }
    // Then authenticated emissions have one sequence gap and distinct nonces.
    let sequences: Vec<_> = emitted
        .iter()
        .map(|packet| receiver.open_datagram(packet).unwrap().0.sequence)
        .collect();
    assert_eq!(sequences, [1, 3]);
    assert_ne!(
        &emitted[0][PacketHeader::SIZE..PacketHeader::SIZE + 12],
        &emitted[1][PacketHeader::SIZE..PacketHeader::SIZE + 12]
    );
    let records = trace.records.lock().unwrap();
    let actions: Vec<_> = records
        .records
        .iter()
        .map(|r| (r.sequence, r.event))
        .collect();
    assert_eq!(actions, [(1, 1), (1, 2), (2, 1), (2, 3), (3, 1), (3, 2)]);
    assert_eq!(records.records[3].value, 105);
    assert!(records
        .records
        .iter()
        .all(|r| r.frame == 7 && r.size == emitted[0].len()));
    drop(records);
    trace.dump().unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.path().join("host.json")).unwrap()).unwrap();
    assert_eq!(json["records"].as_array().unwrap().len(), 6);
}

#[test]
fn sender_trace_retains_first_fixed_records_and_overflow() {
    let mut records = host_trace::Records::default();
    for sequence in 0..65_539 {
        records.push(host_trace::Record {
            sequence,
            event: 1,
            kind: 2,
            size: 123,
            frame: 7,
            ..Default::default()
        });
    }
    assert_eq!(records.records.len(), 65_536);
    assert_eq!(records.overflow, 3);
    assert_eq!(records.records[65_535].sequence, 65_535);
    assert_eq!(records.records[0].frame, 7);
}
