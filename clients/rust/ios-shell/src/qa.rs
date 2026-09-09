use std::path::PathBuf;
use erd_app::PairingRecord;
use erd_mobile::MobilePairingStore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartupResponse {
    pub host: Option<String>,
    #[serde(alias = "autoConnect")]
    pub auto_connect: bool,
}

#[derive(Debug, Deserialize)]
struct QaProvisioningFile {
    host: String,
    #[serde(default)]
    pairing: Option<PairingRecord>,
}

pub fn sandbox_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn check_qa_provisioning() -> StartupResponse {
    #[cfg(debug_assertions)]
    {
        let qa_path = sandbox_dir().join("Documents").join("erd-device-qa.json");
        if qa_path.exists() {
            if let Ok(data) = std::fs::read(&qa_path) {
                if let Ok(qa) = serde_json::from_slice::<QaProvisioningFile>(&data) {
                    if let Some(record) = qa.pairing {
                        if let Ok(store) = erd_app::PairingStore::open_default() {
                            let _ = store.save(record.clone());
                        }
                        let mobile_store = MobilePairingStore::default_keychain();
                        let _ = mobile_store.save_record(&record);
                        tracing::info!("Imported QA pairing record from Documents/erd-device-qa.json into Keychain");
                    }
                    let _ = std::fs::remove_file(&qa_path);
                    tracing::info!(host = %qa.host, "QA provisioning consumed and file deleted");
                    return StartupResponse {
                        host: Some(qa.host),
                        auto_connect: true,
                    };
                }
            }
            let _ = std::fs::remove_file(&qa_path);
        }
    }

    StartupResponse {
        host: None,
        auto_connect: false,
    }
}
