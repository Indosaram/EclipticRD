//! Encrypted ntfy candidate exchange for v3 NAT traversal.

use std::time::Duration;

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use openssl::rand::rand_bytes;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::time::{sleep, timeout, Instant};

use crate::udp_gcm::hkdf_sha256;

const SIGNALING_SALT: &[u8] = b"maho/signaling/v3";
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_BASE_URL: &str = "https://ntfy.sh";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCandidate {
    pub role: String,
    #[serde(rename = "localIP")]
    pub local_ip: String,
    #[serde(rename = "localPort")]
    pub local_port: u16,
    #[serde(rename = "publicIP")]
    pub public_ip: String,
    #[serde(rename = "publicPort")]
    pub public_port: u16,
}

impl SessionCandidate {
    pub fn new(
        role: impl Into<String>,
        local_ip: impl Into<String>,
        local_port: u16,
        public_ip: impl Into<String>,
        public_port: u16,
    ) -> Self {
        Self {
            role: role.into(),
            local_ip: local_ip.into(),
            local_port,
            public_ip: public_ip.into(),
            public_port,
        }
    }
}

#[derive(Debug, Error)]
pub enum SignalingError {
    #[error("failed to initialize HTTP client: {0}")]
    Client(#[source] reqwest::Error),
    #[error("failed to encode candidate JSON: {0}")]
    Encode(#[source] serde_json::Error),
    #[error("failed to generate AES-GCM nonce: {0}")]
    Random(#[source] openssl::error::ErrorStack),
    #[error("failed to encrypt signaling payload")]
    Encrypt,
    #[error("candidate POST failed: {0}")]
    Post(#[source] reqwest::Error),
    #[error("candidate POST rejected with HTTP {0}")]
    PostRejected(StatusCode),
    #[error("signaling timeout waiting for peer")]
    Timeout,
}

/// One side of an encrypted candidate exchange.
pub struct SignalingClient {
    role: String,
    topic: String,
    payload_key: [u8; 32],
    base_url: String,
    http: Client,
}

impl SignalingClient {
    pub fn new(pin: &str, role: impl Into<String>) -> Result<Self, SignalingError> {
        Self::with_base_url(pin, role, DEFAULT_BASE_URL)
    }

    /// Creates a client against a compatible ntfy endpoint. This is primarily
    /// useful for deterministic local integration tests.
    pub fn with_base_url(
        pin: &str,
        role: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Result<Self, SignalingError> {
        let topic_seed = hkdf_sha256(pin.as_bytes(), SIGNALING_SALT, b"maho/topic", 32);
        let topic = format!("erd3-{}", lower_hex(&topic_seed[..14]));
        let key = hkdf_sha256(pin.as_bytes(), SIGNALING_SALT, b"maho/payload-key", 32);
        let mut payload_key = [0_u8; 32];
        payload_key.copy_from_slice(&key);
        let http = Client::builder()
            .timeout(EXCHANGE_TIMEOUT)
            .build()
            .map_err(SignalingError::Client)?;

        Ok(Self {
            role: role.into(),
            topic,
            payload_key,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            http,
        })
    }

    pub fn topic(&self) -> &str {
        &self.topic
    }

    pub async fn exchange_candidate(
        &self,
        local: &SessionCandidate,
    ) -> Result<SessionCandidate, SignalingError> {
        let payload = serde_json::to_vec(local).map_err(SignalingError::Encode)?;
        let message = encrypt_payload(&payload, &self.payload_key)?;
        let topic_url = format!("{}/{}", self.base_url, self.topic);

        let status = self
            .http
            .post(&topic_url)
            .header(reqwest::header::CONTENT_TYPE, "text/plain")
            .body(message)
            .send()
            .await
            .map_err(SignalingError::Post)?
            .status();
        if !status.is_success() {
            return Err(SignalingError::PostRejected(status));
        }

        let poll_url = format!("{topic_url}/json?poll=1&since=10m");
        let deadline = Instant::now() + EXCHANGE_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(SignalingError::Timeout);
            }

            if let Ok(Ok(response)) = timeout(remaining, self.http.get(&poll_url).send()).await {
                if let Ok(body) = response.bytes().await {
                    if let Some(candidate) =
                        peer_candidate_from_jsonl(&body, &self.role, &self.payload_key)
                    {
                        return Ok(candidate);
                    }
                }
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(SignalingError::Timeout);
            }
            sleep(POLL_INTERVAL.min(remaining)).await;
        }
    }
}

fn encrypt_payload(payload: &[u8], key: &[u8; 32]) -> Result<String, SignalingError> {
    let mut nonce = [0_u8; 12];
    rand_bytes(&mut nonce).map_err(SignalingError::Random)?;
    let ciphertext = Aes256Gcm::new_from_slice(key)
        .expect("AES-256 key length is fixed")
        .encrypt(Nonce::from_slice(&nonce), payload)
        .map_err(|_| SignalingError::Encrypt)?;

    let mut combined = Vec::with_capacity(nonce.len() + ciphertext.len());
    combined.extend_from_slice(&nonce);
    combined.extend_from_slice(&ciphertext);
    Ok(BASE64.encode(combined))
}

fn decrypt_payload(encoded: &str, key: &[u8; 32]) -> Option<Vec<u8>> {
    let combined = BASE64.decode(encoded).ok()?;
    if combined.len() < 12 + 16 {
        return None;
    }
    Aes256Gcm::new_from_slice(key)
        .ok()?
        .decrypt(Nonce::from_slice(&combined[..12]), &combined[12..])
        .ok()
}

fn peer_candidate_from_jsonl(
    body: &[u8],
    own_role: &str,
    key: &[u8; 32],
) -> Option<SessionCandidate> {
    #[derive(Deserialize)]
    struct Envelope {
        message: String,
    }

    let text = std::str::from_utf8(body).ok()?;
    text.lines().rev().find_map(|line| {
        let envelope = serde_json::from_str::<Envelope>(line).ok()?;
        let payload = decrypt_payload(&envelope.message, key)?;
        let candidate = serde_json::from_slice::<SessionCandidate>(&payload).ok()?;
        (candidate.role != own_role).then_some(candidate)
    })
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(role: &str, port: u16) -> SessionCandidate {
        SessionCandidate::new(role, "192.0.2.10", port, "198.51.100.20", port + 1)
    }

    #[test]
    fn candidate_json_uses_wire_property_names() {
        let value = candidate("client", 19730);
        let json = serde_json::to_string(&value).unwrap();
        assert!(json.contains("\"localIP\""));
        assert!(json.contains("\"publicPort\""));
        assert_eq!(
            serde_json::from_str::<SessionCandidate>(&json).unwrap(),
            value
        );
    }

    #[test]
    fn payload_encrypt_decrypt_round_trip() {
        let client = SignalingClient::new("12345678", "client").unwrap();
        let plaintext = serde_json::to_vec(&candidate("server", 19730)).unwrap();
        let encrypted = encrypt_payload(&plaintext, &client.payload_key).unwrap();
        assert_eq!(
            decrypt_payload(&encrypted, &client.payload_key).unwrap(),
            plaintext
        );
    }

    #[test]
    fn role_filter_searches_newest_to_oldest() {
        let client = SignalingClient::new("12345678", "client").unwrap();
        let peer = candidate("server", 19730);
        let own = candidate("client", 19731);
        let peer_message =
            encrypt_payload(&serde_json::to_vec(&peer).unwrap(), &client.payload_key).unwrap();
        let own_message =
            encrypt_payload(&serde_json::to_vec(&own).unwrap(), &client.payload_key).unwrap();
        let jsonl = format!(
            "not-json\n{{\"message\":\"{peer_message}\"}}\n{{\"message\":\"{own_message}\"}}\n"
        );
        assert_eq!(
            peer_candidate_from_jsonl(jsonl.as_bytes(), "client", &client.payload_key),
            Some(peer)
        );
    }

    #[test]
    fn topic_is_stable_and_within_ntfy_limit() {
        let first = SignalingClient::new("12345678", "client").unwrap();
        let second = SignalingClient::new("12345678", "server").unwrap();
        let different = SignalingClient::new("87654321", "client").unwrap();
        assert_eq!(first.topic(), second.topic());
        assert_eq!(first.topic(), "erd3-a1ac9ca40211ed7a122586e77c18");
        assert_eq!(
            first.payload_key,
            [
                0xdd, 0x99, 0x49, 0x59, 0xd1, 0xdd, 0xf2, 0x31, 0x60, 0x4a, 0xc4, 0xad, 0x12, 0x9c,
                0xf2, 0xb5, 0x65, 0xf6, 0x25, 0xe0, 0x5c, 0x19, 0x69, 0x47, 0x54, 0xe7, 0xa1, 0x7a,
                0xa3, 0xff, 0xef, 0x28,
            ]
        );
        assert_ne!(first.topic(), different.topic());
        assert_eq!(first.topic().len(), 33);
        assert!(first.topic().len() <= 64);
        assert!(first.topic().starts_with("erd3-"));
    }

    #[tokio::test]
    async fn live_ntfy_round_trip_when_enabled() {
        if std::env::var("MAHO_LIVE_TESTS").as_deref() != Ok("1") {
            eprintln!("skipping live ntfy test; set MAHO_LIVE_TESTS=1 to enable");
            return;
        }

        let suffix = std::process::id();
        let pin = format!("{suffix:08}");
        let server = SignalingClient::new(&pin, "server").unwrap();
        let client = SignalingClient::new(&pin, "client").unwrap();
        let server_candidate = candidate("server", 19730);
        let client_candidate = candidate("client", 19731);
        let (server_result, client_result) = tokio::join!(
            server.exchange_candidate(&server_candidate),
            client.exchange_candidate(&client_candidate),
        );
        assert_eq!(server_result.unwrap(), client_candidate);
        assert_eq!(client_result.unwrap(), server_candidate);
    }
}
