use super::{parse_service_metadata, DiscoveredHost, DiscoveryError, DiscoveryTracker};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

pub struct MdnsSdBrowser {
    tracker: Arc<Mutex<DiscoveryTracker>>,
    daemon: Option<mdns_sd::ServiceDaemon>,
    worker_handle: Option<thread::JoinHandle<()>>,
    last_error: Arc<Mutex<Option<DiscoveryError>>>,
    cancel_flag: Arc<AtomicBool>,
}

impl MdnsSdBrowser {
    pub fn new() -> Result<Self, DiscoveryError> {
        let daemon = mdns_sd::ServiceDaemon::new()
            .map_err(|e| DiscoveryError::Backend(format!("mdns-sd daemon error: {e}")))?;

        let monitor_rx = match daemon.monitor() {
            Ok(rx) => rx,
            Err(e) => {
                let _ = daemon.shutdown();
                return Err(DiscoveryError::Backend(format!("mdns-sd monitor error: {e}")));
            }
        };

        let service_type = "_erd._tcp.local.";
        let receiver = match daemon.browse(service_type) {
            Ok(rx) => rx,
            Err(e) => {
                let _ = daemon.shutdown();
                return Err(DiscoveryError::Backend(format!("mdns-sd browse error: {e}")));
            }
        };

        let tracker = Arc::new(Mutex::new(DiscoveryTracker::new()));
        let last_error = Arc::new(Mutex::new(None));
        let cancel_flag = Arc::new(AtomicBool::new(false));

        let tracker_clone = Arc::clone(&tracker);
        let last_error_clone = Arc::clone(&last_error);
        let cancel_flag_clone = Arc::clone(&cancel_flag);
        let worker_handle = thread::Builder::new()
            .name("erd-mdns-browser".into())
            .spawn(move || {
                while !cancel_flag_clone.load(Ordering::Relaxed) {
                    while let Ok(daemon_event) = monitor_rx.try_recv() {
                        if let mdns_sd::DaemonEvent::Error(err) = daemon_event {
                            tracing::warn!("mDNS daemon monitor error: {err}");
                            if let Ok(mut err_guard) = last_error_clone.lock() {
                                *err_guard = Some(DiscoveryError::Backend(err.to_string()));
                            }
                        }
                    }

                    match receiver.recv_timeout(Duration::from_millis(200)) {
                        Ok(event) => match event {
                            mdns_sd::ServiceEvent::ServiceResolved(info) => {
                                let fullname = info.get_fullname().to_string();
                                let host_target = info.get_hostname().to_string();
                                let srv_port = info.get_port();

                                let mut txt_items = Vec::new();
                                for prop in info.get_properties().iter() {
                                    let key = prop.key().to_string();
                                    let val = prop.val().unwrap_or_default().to_vec();
                                    txt_items.push((key, val));
                                }

                                let addrs: Vec<IpAddr> =
                                    info.get_addresses().iter().copied().collect();

                                if let Ok(host) = parse_service_metadata(
                                    &fullname,
                                    &host_target,
                                    srv_port,
                                    &txt_items,
                                    &addrs,
                                ) {
                                    if let Ok(mut tr_guard) = tracker_clone.lock() {
                                        tr_guard.upsert(host);
                                    }
                                }
                            }
                            mdns_sd::ServiceEvent::ServiceRemoved(_, fullname) => {
                                if let Ok(mut tr_guard) = tracker_clone.lock() {
                                    tr_guard.remove(&fullname);
                                }
                            }
                            mdns_sd::ServiceEvent::SearchStopped(_) => {
                                break;
                            }
                            _ => {}
                        },
                        Err(flume::RecvTimeoutError::Timeout) => {}
                        Err(flume::RecvTimeoutError::Disconnected) => {
                            if !cancel_flag_clone.load(Ordering::Relaxed) {
                                tracing::warn!("mDNS browse channel disconnected unexpectedly");
                                if let Ok(mut err_guard) = last_error_clone.lock() {
                                    *err_guard = Some(DiscoveryError::Backend(
                                        "mDNS browse channel closed unexpectedly".into(),
                                    ));
                                }
                                if let Ok(mut tr_guard) = tracker_clone.lock() {
                                    tr_guard.clear();
                                }
                            }
                            break;
                        }
                    }
                }
            })
            .map_err(|e| {
                let _ = daemon.shutdown();
                DiscoveryError::Backend(format!("failed to spawn worker: {e}"))
            })?;

        Ok(Self {
            tracker,
            daemon: Some(daemon),
            worker_handle: Some(worker_handle),
            last_error,
            cancel_flag,
        })
    }

    pub fn snapshot(&self) -> Result<Vec<DiscoveredHost>, DiscoveryError> {
        if let Ok(err_guard) = self.last_error.lock() {
            if let Some(err) = err_guard.clone() {
                return Err(err);
            }
        }
        self.tracker
            .lock()
            .map(|t| t.snapshot())
            .map_err(|_| DiscoveryError::Backend("tracker lock poisoned".into()))
    }
}

impl Drop for MdnsSdBrowser {
    fn drop(&mut self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
        if let Some(ref daemon) = self.daemon {
            if let Err(e) = daemon.stop_browse("_erd._tcp.local.") {
                tracing::warn!("Failed to stop mDNS browse in Drop: {e}");
            }
        }
        if let Some(daemon) = self.daemon.take() {
            match daemon.shutdown() {
                Ok(rx) => {
                    let _ = rx.recv_timeout(Duration::from_secs(1));
                }
                Err(e) => {
                    tracing::warn!("Failed to shutdown mDNS browser daemon in Drop: {e}");
                }
            }
        }
        if let Some(handle) = self.worker_handle.take() {
            if let Err(e) = handle.join() {
                tracing::warn!("Failed to join mDNS browser worker thread: {e:?}");
            }
        }
    }
}

pub struct MdnsSdAdvertiser {
    daemon: Option<mdns_sd::ServiceDaemon>,
    fullname: String,
}

impl MdnsSdAdvertiser {
    pub fn start(
        name: &str,
        tcp_port: u16,
        udp_port: u16,
        bind_addr: SocketAddr,
    ) -> Result<Self, DiscoveryError> {
        let daemon = mdns_sd::ServiceDaemon::new()
            .map_err(|e| DiscoveryError::Backend(format!("mdns-sd daemon error: {e}")))?;

        let ip_to_advertise = if bind_addr.ip().is_loopback() {
            if let Err(e) = daemon.disable_interface(mdns_sd::IfKind::All) {
                tracing::warn!("Failed to disable all interfaces for loopback bind: {e}");
            }
            if bind_addr.is_ipv4() {
                daemon
                    .enable_interface(mdns_sd::IfKind::LoopbackV4)
                    .map_err(|e| DiscoveryError::Backend(format!("failed to enable LoopbackV4: {e}")))?;
            } else {
                daemon
                    .enable_interface(mdns_sd::IfKind::LoopbackV6)
                    .map_err(|e| DiscoveryError::Backend(format!("failed to enable LoopbackV6: {e}")))?;
            }
            bind_addr.ip().to_string()
        } else if bind_addr.ip().is_unspecified() {
            let lan_interfaces = enumerate_lan_interfaces()?;
            if lan_interfaces.is_empty() {
                let _ = daemon.shutdown();
                return Err(DiscoveryError::Backend(
                    "no active physical LAN interface found for advertisement".into(),
                ));
            }

            let _ = daemon.disable_interface(mdns_sd::IfKind::LoopbackV4);
            let _ = daemon.disable_interface(mdns_sd::IfKind::LoopbackV6);

            let mut seen_names = std::collections::HashSet::new();
            let mut ips = Vec::new();
            for (if_name, ip) in lan_interfaces {
                if seen_names.insert(if_name.clone()) {
                    if let Err(e) = daemon.enable_interface(mdns_sd::IfKind::Name(if_name)) {
                        tracing::debug!("enable_interface failed: {e}");
                    }
                }
                ips.push(ip.to_string());
            }
            ips.dedup();
            ips.join(",")
        } else {
            if let Err(e) = daemon.disable_interface(mdns_sd::IfKind::All) {
                tracing::warn!("Failed to disable all interfaces for specific bind: {e}");
            }
            daemon
                .enable_interface(mdns_sd::IfKind::Addr(bind_addr.ip()))
                .map_err(|e| DiscoveryError::Backend(format!("failed to enable interface for {}: {e}", bind_addr.ip())))?;
            bind_addr.ip().to_string()
        };

        let mut properties = HashMap::new();
        properties.insert("protocol".to_string(), "3".to_string());
        properties.insert("name".to_string(), name.to_string());
        properties.insert("os".to_string(), std::env::consts::OS.to_string());
        properties.insert("udp_port".to_string(), udp_port.to_string());

        let service_type = "_erd._tcp.local.";
        let host_name = format!("{name}.local.");

        let service_info = mdns_sd::ServiceInfo::new(
            service_type,
            name,
            &host_name,
            ip_to_advertise.as_str(),
            tcp_port,
            properties,
        )
        .map_err(|e| DiscoveryError::InvalidPayload(format!("invalid service info: {e}")))?
        .enable_addr_auto();

        let fullname = service_info.get_fullname().to_string();
        if let Err(e) = daemon.register(service_info) {
            let _ = daemon.shutdown();
            return Err(DiscoveryError::Backend(format!("mdns-sd register error: {e}")));
        }

        Ok(Self {
            daemon: Some(daemon),
            fullname,
        })
    }
}

impl Drop for MdnsSdAdvertiser {
    fn drop(&mut self) {
        if let Some(daemon) = self.daemon.take() {
            if let Err(e) = daemon.unregister(&self.fullname) {
                tracing::warn!("Failed to unregister mDNS service '{}' in Drop: {e}", self.fullname);
            }
            match daemon.shutdown() {
                Ok(rx) => {
                    let _ = rx.recv_timeout(Duration::from_secs(1));
                }
                Err(e) => {
                    tracing::warn!("Failed to shutdown mDNS advertiser daemon in Drop: {e}");
                }
            }
        }
    }
}

pub fn enumerate_lan_interfaces() -> Result<Vec<(String, IpAddr)>, DiscoveryError> {
    let addrs = if_addrs::get_if_addrs()
        .map_err(|e| DiscoveryError::Backend(format!("failed to enumerate interfaces: {e}")))?;

    let mut result = Vec::new();
    for iface in addrs {
        if iface.is_loopback() {
            continue;
        }
        let name_lower = iface.name.to_ascii_lowercase();
        if is_excluded_interface_name(&name_lower) {
            continue;
        }
        let ip = iface.addr.ip();
        if is_excluded_ip(&ip) {
            continue;
        }
        result.push((iface.name, ip));
    }
    Ok(result)
}

pub fn is_excluded_interface_name(name: &str) -> bool {
    let excluded_prefixes = [
        "tailscale", "tun", "tap", "wg", "wireguard", "docker", "veth", "br-", "cni",
        "flannel", "dummy", "virbr", "vmnet",
    ];
    excluded_prefixes
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

pub fn is_excluded_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_unspecified()
                || v4.is_multicast()
                || v4.is_broadcast()
                || is_tailscale_ipv4(v4)
                || is_docker_ipv4(v4)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || is_unscoped_link_local_ipv6(v6)
                || is_tailscale_ipv6(v6)
        }
    }
}

fn is_tailscale_ipv4(v4: &std::net::Ipv4Addr) -> bool {
    let octets = v4.octets();
    octets[0] == 100 && (octets[1] >= 64 && octets[1] <= 127)
}

fn is_tailscale_ipv6(v6: &std::net::Ipv6Addr) -> bool {
    let segs = v6.segments();
    segs[0] == 0xfd7a && segs[1] == 0x115c && segs[2] == 0xa1e0
}

fn is_docker_ipv4(v4: &std::net::Ipv4Addr) -> bool {
    let octets = v4.octets();
    octets[0] == 172 && octets[1] == 17
}

fn is_unscoped_link_local_ipv6(v6: &std::net::Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xffc0) == 0xfe80
}
