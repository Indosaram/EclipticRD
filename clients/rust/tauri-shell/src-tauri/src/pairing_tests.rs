use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use erd_app::PairingStore;

use super::commands::{self, list_pairings_internal};

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct TempStoreGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    dir: PathBuf,
    store_file: PathBuf,
    prev_xdg: Option<String>,
    prev_home: Option<String>,
}

impl TempStoreGuard {
    fn new() -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("erd-pairing-test-{unique}"));
        let store_dir = dir.join("EclipticRD");
        fs::create_dir_all(&store_dir).unwrap();

        let mac_store_dir = dir
            .join("Library")
            .join("Application Support")
            .join("EclipticRD");
        fs::create_dir_all(&mac_store_dir).unwrap();

        let prev_xdg = std::env::var("XDG_DATA_HOME").ok();
        let prev_home = std::env::var("HOME").ok();

        std::env::set_var("XDG_DATA_HOME", &dir);
        std::env::set_var("HOME", &dir);

        let store_file = store_dir.join("pairing-keys.json");
        Self {
            _lock: lock,
            dir,
            store_file,
            prev_xdg,
            prev_home,
        }
    }

    fn write_pairing_record(&self, id: &str, name: &str, key_b64: &str, added_at: u64) {
        let json = format!(
            r#"[
  {{
    "id": "{id}",
    "name": "{name}",
    "key": "{key_b64}",
    "addedAt": {added_at}
  }}
]"#
        );
        let linux_client = self.dir.join("EclipticRD").join("client-pairings.json");
        let linux_legacy = self.dir.join("EclipticRD").join("pairing-keys.json");
        let mac_client = self
            .dir
            .join("Library")
            .join("Application Support")
            .join("EclipticRD")
            .join("client-pairings.json");
        let mac_legacy = self
            .dir
            .join("Library")
            .join("Application Support")
            .join("EclipticRD")
            .join("pairing-keys.json");

        let _ = fs::write(&linux_client, &json);
        let _ = fs::write(&linux_legacy, &json);
        let _ = fs::write(&mac_client, &json);
        let _ = fs::write(&mac_legacy, &json);
    }

    fn open_store(&self) -> PairingStore {
        PairingStore::new(&self.store_file)
    }
}

impl Drop for TempStoreGuard {
    fn drop(&mut self) {
        match &self.prev_xdg {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match &self.prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn test_list_pairings_json_excludes_key_field() {
    let guard = TempStoreGuard::new();
    guard.write_pairing_record(
        "TEST-PAIRING-UUID",
        "mock-remote-host",
        "c2VjcmV0LXNoYXJlZC1rZXktYnl0ZXMtMTIzNDU2Nzg=", // 32 bytes base64
        1725900000000,
    );

    let pairings = commands::list_pairings().expect("list_pairings should succeed");
    let json = serde_json::to_string(&pairings).expect("serialization should succeed");

    // The serialized JSON must contain allowed public metadata fields:
    assert!(json.contains("\"id\""), "JSON must contain 'id': {json}");
    assert!(
        json.contains("\"hostName\""),
        "JSON must contain camelCase 'hostName': {json}"
    );
    assert!(
        json.contains("\"addedAtUnixMs\""),
        "JSON must contain camelCase 'addedAtUnixMs': {json}"
    );

    // The serialized JSON must NOT contain 'key' field or secret key material:
    assert!(
        !json.contains("\"key\""),
        "JSON must NOT contain 'key' field: {json}"
    );
    assert!(
        !json.contains("c2VjcmV0"),
        "JSON must NOT contain secret key bytes: {json}"
    );
}

#[test]
fn test_list_pairings_store_seam_isolated_store_serializes_only_allowed_metadata() {
    let guard = TempStoreGuard::new();
    guard.write_pairing_record(
        "SEAM-UUID-999",
        "desktop-host-target",
        "ZmFrZS1zZWNyZXQta2V5LTMyei1ieXRlcy12ZWN0b3I=",
        1725999999000,
    );

    let store = guard.open_store();
    let summaries = list_pairings_internal(&store).expect("store seam should succeed");
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].id, "SEAM-UUID-999");
    assert_eq!(summaries[0].host_name, "desktop-host-target");
    assert_eq!(summaries[0].added_at_unix_ms, 1725999999000);
    assert_eq!(summaries[0].last_endpoint, None);

    let serialized = serde_json::to_string(&summaries).expect("serialize should succeed");
    println!("SEAM_SERIALIZED_JSON: {}", serialized);
    assert!(
        !serialized.contains("\"key\""),
        "Serialized seam JSON must NOT contain 'key': {serialized}"
    );
    assert!(
        !serialized.contains("ZmFrZS1zZWNyZXQ"),
        "Serialized seam JSON must NOT leak base64 key material: {serialized}"
    );

    let parsed: Vec<serde_json::Value> =
        serde_json::from_str(&serialized).expect("deserialization should succeed");
    assert_eq!(parsed.len(), 1);
    let obj = parsed[0].as_object().expect("entry must be a JSON object");

    // Allowed keys only
    let mut keys: Vec<&String> = obj.keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        vec!["addedAtUnixMs", "hostName", "id"],
        "JSON must contain ONLY allowed metadata fields"
    );
}
