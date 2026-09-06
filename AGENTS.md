# PROJECT KNOWLEDGE BASE

**Generated:** 2026-09-03 06:55:00Z
**Branch:** main

## OVERVIEW
Ultra-low latency Remote Desktop system built 100% in Rust with a Tauri v2 desktop client (`tauri-shell`), cross-platform host daemon (`erd-host`), and modular protocol/decode/render crates.

## STRUCTURE
```
.
└── clients/rust/     # Cross-platform Rust workspace
    ├── erd-proto/    # Pure v3 wire codec, packet envelopes, ChaCha20-Poly1305 handshakes
    ├── erd-net/      # Tokio async networking (TLS-PSK TCP, UDP-GCM, STUN, signaling)
    ├── erd-decode/   # Video/audio decoding pipeline (FFmpeg / hardware)
    ├── erd-render/   # GPU renderer (wgpu / Metal / Vulkan / DirectX)
    ├── erd-app/      # Client session coordinator, pairing store, input & latency tracking
    ├── erd-host/     # Multi-platform host daemon (DXGI, Hyprland, SCK capture; MF, VAAPI, VT encode)
    └── tauri-shell/  # Tauri v2 desktop GUI client
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Rust Multiplatform Host | `clients/rust/erd-host/` | `session.rs`, `capture_macos.rs`, `capture_windows.rs`, `capture_linux.rs` |
| Rust Protocol & Framing | `clients/rust/erd-proto/` | `packet.rs`, `framing.rs`, `handshake.rs`, `control.rs` |
| Rust Async Network Layer | `clients/rust/erd-net/` | `tls_psk.rs`, `udp_gcm.rs`, `stun.rs`, `signaling.rs` |
| Rust Client Session & Pairing | `clients/rust/erd-app/` | `session.rs`, `pairing.rs`, `input.rs` |
| Rust Tauri Desktop Client | `clients/rust/tauri-shell/` | `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `ui/` |

## CODE MAP
| Symbol | Type | Location | Role |
|--------|------|----------|------|
| `HostServer` | struct | `clients/rust/erd-host/src/session.rs` | Rust cross-platform streaming daemon orchestrator |
| `ClientSession` | struct | `clients/rust/erd-app/src/session.rs` | Rust client session manager |
| `WireCodec` | trait | `clients/rust/erd-proto/src/lib.rs` | Rust wire protocol serialization/deserialization contract |
| `DatagramCipher` | struct | `clients/rust/erd-net/src/udp_gcm.rs` | Direction-separated AES-GCM packet encryptor/decryptor |
| `TlsPskStream` | struct | `clients/rust/erd-net/src/tls_psk.rs` | Authenticated TCP control stream with replay defense |
| `HevcDecoder` | struct | `clients/rust/erd-decode/src/lib.rs` | Hardware/FFmpeg video decoder |

## CONVENTIONS
- Pure Rust architecture: Swift / Xcode projects are completely retired.
- Workspace commands use `--manifest-path clients/rust/Cargo.toml`.
- Strict byte-endian consistency on wire; no blocking calls on Tokio.
- UI mutations and state are managed via Tauri v2 commands and events.

## COMMANDS
```bash
# Rust Workspace Build & Test
cargo build --manifest-path clients/rust/Cargo.toml
cargo test --manifest-path clients/rust/Cargo.toml

# Run Tauri Desktop Client
cargo run --manifest-path clients/rust/Cargo.toml -p tauri-shell

# Headless Client E2E Test
cargo run --manifest-path clients/rust/Cargo.toml -p erd-app --bin erd-client -- --host <IP> --frames 10
```
