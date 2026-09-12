use super::*;

#[test]
fn receiver_export_retains_legacy_flat_fields_and_rank_meaning() {
    // Given: lifetime count exceeds the ring, with distinct two-value ranks.
    let mut recorder = LatencyRecorder::with_capacity(2);
    for value in [999, 10, 20] {
        recorder.record_us(value);
    }
    let temporary = tempfile::tempdir().unwrap();
    let mut config = SessionConfig::direct("127.0.0.1", "empty");
    config.pairing_store_path = Some(temporary.path().join("pairings.json"));
    let session = ClientSession::new(config).unwrap();
    // When: exporting a real, unsampled session and the existing recorder.
    let json: serde_json::Value = serde_json::from_str(
        &stats_json(&recorder, &session.receiver_snapshot().unwrap()).unwrap(),
    )
    .unwrap();
    // Then: all five legacy fields preserve lifetime count and upper median.
    for (field, value) in [
        ("frames", 3),
        ("p50_us", 20),
        ("p95_us", 20),
        ("p99_us", 20),
        ("max_us", 20),
    ] {
        assert_eq!(json[field], value, "{field}");
    }
    assert_eq!(
        json.get("receiver_snapshot"),
        Some(&serde_json::to_value(session.receiver_snapshot().unwrap()).unwrap())
    );
}
