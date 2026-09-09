pub mod tracker;
pub use tracker::DiscoveryTracker;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub mod apple;

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod mdns;

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "linux", target_os = "windows")))]
pub mod stub;

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use thiserror::Error;

pub fn is_usable_ipv4(ip: &IpAddr) -> bool {
    if let IpAddr::V4(v4) = ip {
        !v4.is_unspecified() && !v4.is_loopback() && !v4.is_multicast() && !v4.is_broadcast()
    } else {
        false
    }
}

pub fn is_unscoped_ipv6_link_local(v6: &std::net::Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xffc0) == 0xfe80
}

pub fn is_usable_ipv6(ip: &IpAddr) -> bool {
    if let IpAddr::V6(v6) = ip {
        !v6.is_unspecified()
            && !v6.is_loopback()
            && !v6.is_multicast()
            && !is_unscoped_ipv6_link_local(v6)
    } else {
        false
    }
}

pub fn is_usable_address(ip: &IpAddr) -> bool {
    is_usable_ipv4(ip) || is_usable_ipv6(ip)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredHost {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub os: String,
    pub tcp_port: u16,
    pub udp_port: u16,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum DiscoveryError {
    #[error("local network access denied by system policy")]
    PolicyDenied,
    #[error("discovery backend error: {0}")]
    Backend(String),
    #[error("discovery service unavailable")]
    Unavailable,
    #[error("invalid discovery payload: {0}")]
    InvalidPayload(String),
}

pub fn parse_service_metadata(
    fullname: &str,
    _host_target: &str,
    srv_port: u16,
    txt: &[(String, Vec<u8>)],
    addresses: &[IpAddr],
) -> Result<DiscoveredHost, DiscoveryError> {
    if fullname.is_empty() || fullname.len() > 255 {
        return Err(DiscoveryError::InvalidPayload(
            "fullname must be between 1 and 255 bytes".into(),
        ));
    }
    if !fullname.contains("._erd._tcp.") {
        return Err(DiscoveryError::InvalidPayload(
            "fullname must contain service type ._erd._tcp.".into(),
        ));
    }
    if srv_port == 0 {
        return Err(DiscoveryError::InvalidPayload(
            "SRV port must be nonzero".into(),
        ));
    }

    if addresses.is_empty() {
        return Err(DiscoveryError::InvalidPayload("no addresses found".into()));
    }

    let mut total_len = 0;
    for (k, v) in txt {
        total_len += k.len() + v.len();
        if k.is_empty() || k.len() > 255 || v.len() > 255 {
            return Err(DiscoveryError::InvalidPayload(
                "TXT key or value length invalid".into(),
            ));
        }
        if !k.is_ascii() || k.contains('=') {
            return Err(DiscoveryError::InvalidPayload(
                "TXT key must be ASCII without '='".into(),
            ));
        }
    }
    if total_len > 1300 {
        return Err(DiscoveryError::InvalidPayload(
            "total TXT length exceeds bounded limit".into(),
        ));
    }

    let proto_entry = txt
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("protocol"))
        .ok_or_else(|| DiscoveryError::InvalidPayload("missing protocol in TXT".into()))?;

    if proto_entry.1.as_slice() != b"3" {
        return Err(DiscoveryError::InvalidPayload(
            "unsupported protocol version".into(),
        ));
    }

    let name_entry = txt
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("name"))
        .ok_or_else(|| DiscoveryError::InvalidPayload("missing name in TXT".into()))?;

    let name = std::str::from_utf8(&name_entry.1)
        .map_err(|_| DiscoveryError::InvalidPayload("name must be valid UTF-8".into()))?
        .to_string();
    if name.is_empty() || name.len() > 255 {
        return Err(DiscoveryError::InvalidPayload(
            "name must be between 1 and 255 bytes".into(),
        ));
    }

    let os_entry = txt
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("os"))
        .ok_or_else(|| DiscoveryError::InvalidPayload("missing os in TXT".into()))?;

    let os = std::str::from_utf8(&os_entry.1)
        .map_err(|_| DiscoveryError::InvalidPayload("os must be valid UTF-8".into()))?
        .to_string();
    if os.is_empty() || os.len() > 64 {
        return Err(DiscoveryError::InvalidPayload(
            "os must be between 1 and 64 bytes".into(),
        ));
    }

    let udp_entry = txt
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("udp_port"))
        .ok_or_else(|| DiscoveryError::InvalidPayload("missing udp_port in TXT".into()))?;

    let udp_str = std::str::from_utf8(&udp_entry.1)
        .map_err(|_| DiscoveryError::InvalidPayload("udp_port must be valid ASCII".into()))?;

    let udp_port: u16 = udp_str
        .parse()
        .map_err(|_| DiscoveryError::InvalidPayload("udp_port is not a valid integer".into()))?;

    if udp_port == 0 {
        return Err(DiscoveryError::InvalidPayload(
            "udp_port must be nonzero".into(),
        ));
    }

    let chosen_ip = addresses
        .iter()
        .find(|ip| ip.is_ipv4() && is_usable_ipv4(ip))
        .or_else(|| addresses.iter().find(|ip| ip.is_ipv6() && is_usable_ipv6(ip)))
        .ok_or_else(|| {
            DiscoveryError::InvalidPayload("no usable unicast address found".into())
        })?;

    let ip_str = chosen_ip.to_string();

    Ok(DiscoveredHost {
        id: fullname.to_string(),
        name,
        ip: ip_str,
        os,
        tcp_port: srv_port,
        udp_port,
    })
}

pub struct LanDiscovery {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    inner: apple::AppleDnsServiceBrowser,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    inner: mdns::MdnsSdBrowser,
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "linux", target_os = "windows")))]
    inner: stub::StubBrowser,
}

impl LanDiscovery {
    pub fn new() -> Result<Self, DiscoveryError> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            let inner = apple::AppleDnsServiceBrowser::new()?;
            Ok(Self { inner })
        }
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            let inner = mdns::MdnsSdBrowser::new()?;
            Ok(Self { inner })
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "linux", target_os = "windows")))]
        {
            let inner = stub::StubBrowser::new()?;
            Ok(Self { inner })
        }
    }

    pub fn snapshot(&self) -> Result<Vec<DiscoveredHost>, DiscoveryError> {
        self.inner.snapshot()
    }
}

pub struct ServiceAdvertiser {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    _inner: apple::AppleDnsServiceAdvertiser,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    _inner: mdns::MdnsSdAdvertiser,
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "linux", target_os = "windows")))]
    _inner: stub::StubAdvertiser,
}

impl ServiceAdvertiser {
    pub fn start(
        name: &str,
        tcp_port: u16,
        udp_port: u16,
        bind_addr: SocketAddr,
    ) -> Result<Self, DiscoveryError> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            let _inner =
                apple::AppleDnsServiceAdvertiser::start(name, tcp_port, udp_port, bind_addr)?;
            Ok(Self { _inner })
        }
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            let _inner =
                mdns::MdnsSdAdvertiser::start(name, tcp_port, udp_port, bind_addr)?;
            Ok(Self { _inner })
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "linux", target_os = "windows")))]
        {
            let _inner =
                stub::StubAdvertiser::start(name, tcp_port, udp_port, bind_addr)?;
            Ok(Self { _inner })
        }
    }
}
