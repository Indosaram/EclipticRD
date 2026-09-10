use std::{io::Write, net::SocketAddr, path::PathBuf};

use erd_host::{HostConfig, HostServer, PairingRecord, PairingStore};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store_path = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("expected an isolated QA store path")?,
    );
    let store = PairingStore::new(store_path);
    store.save(PairingRecord {
        id: "qa-r3-registration".into(),
        name: "isolated-r3-qa-client".into(),
        key: [0x52; 32],
        added_at_unix_ms: 0,
    })?;
    let mut config =
        HostConfig::linux_default(None, store, std::env::var("ERD_OUTPUT").ok())?;
    config.tcp_addr = SocketAddr::from(([127, 0, 0, 1], 0));
    config.udp_addr = SocketAddr::from(([127, 0, 0, 1], 0));
    config.host_name = "ERD isolated R3 QA".into();
    config.capture_audio = false;
    let server = HostServer::bind(config)?;
    println!("QA_READY {} {}", server.tcp_addr()?.port(), server.udp_addr()?.port());
    std::io::stdout().flush()?;
    server.serve_n(1)?;
    Ok(())
}
