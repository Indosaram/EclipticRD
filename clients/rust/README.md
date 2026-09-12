# MahoRD Rust workspace

This is the active MahoRD workspace, containing the host, desktop and iOS
applications, protocol and transport crates, media pipeline, and agent automation API.

**Agent-first:** Use `maho-client --mcp` for stdio Model Context Protocol (JSON-RPC) access via Claude, Codex, or custom agents.
**Automation API:** Use `maho-client --agent-server 19735` for HTTP/WebSocket API on loopback.
**Desktop GUI:** Use `tauri-shell` for graphical remote desktop client on macOS/Linux.

Start with the [project README](../../README.md) for platform status, agent setup,
dependencies, and performance notes. See [docs/agent-setup.md](../../docs/agent-setup.md)
for MCP registration steps.

## Agent-First Automation

### MCP (Model Context Protocol) - Recommended for Agents

Expose ten remote-control tools via stdio JSON-RPC:

```sh
# With pairing ID (no PIN required)
cargo run --locked --release -p maho-app --bin maho-client -- \
  --host 192.168.1.50 \
  --pairing-id YOUR-UUID \
  --pairing-store ~/.pairings.json \
  --mcp

# Agent then calls:
#   initialize          → server info
#   tools/list          → list all remote control tools
#   tools/call          → remote_take_screenshot, remote_mouse_click, remote_key_press, etc.
```

Registration:
```sh
# Claude
claude mcp add --transport stdio --scope project mahord \
  -- /path/to/maho-client --host HOST --pairing-id ID --pairing-store FILE --mcp

# Codex
codex mcp add mahord \
  -- /path/to/maho-client --host HOST --pairing-id ID --pairing-store FILE --mcp
```

See [docs/agent-setup.md](../../docs/agent-setup.md) for full setup and [skills/mahord-remote-control/SKILL.md](../../skills/mahord-remote-control/SKILL.md) for tool reference.

### HTTP API (Loopback)

```sh
cargo run --locked --release -p maho-app --bin maho-client -- \
  --host 192.168.1.50 \
  --pairing-id YOUR-UUID \
  --pairing-store ~/.pairings.json \
  --agent-server 19735

# Loopback API: http://127.0.0.1:19735/api/v1/{health,screen/info,screen/screenshot,...}
```

## Build and test

Run from this directory after installing the native dependencies:

```sh
cargo build --locked --release -p maho-host -p maho-app
cargo build --locked --release -p tauri-shell --features tauri/custom-protocol
cargo test --locked --workspace --exclude maho-ios
```

Both desktop and MCP/HTTP client builds **require FFmpeg 7.0.2 headers and runtime**. The Rust wrapper is `ffmpeg-next` 8.1.0. Point `PKG_CONFIG_PATH` at your FFmpeg prefix; both headers and shared libraries must be present.

**iOS:** Separate Xcode tooling, signing, and physical-device requirements. Excluding `maho-ios` from tests does not verify iOS.

## Workspace members

- **`maho-proto`**: Wire types, framing, handshake, protocol limits
- **`maho-net`**: TCP/UDP transport, replay protection, discovery, signaling
- **`maho-decode`**: FFmpeg and iOS VideoToolbox video decoding
- **`maho-render`**: Presentation, audio, WebGL/native platform integration
- **`maho-app`**: Main CLI (`maho-client`), MCP dispatcher, HTTP API (`AgentServer`), session lifecycle
- **`maho-host`**: Platform capture, encoding, audio, input injection
- **`maho-mobile`**: Shared mobile input, lifecycle, storage abstractions
- **`tauri-shell`**: Desktop GUI application (native Tauri, WebKit)
- **`ios-shell`**: iOS app and native integration

The nested `tauri-shell/src-tauri/Cargo.toml` is not the workspace root; build `tauri-shell` from this directory.

## Platform Support and Performance

- **macOS**: ScreenCaptureKit, VideoToolbox paths; desktop launch and CLI streaming verified, native GUI session QA incomplete
- **Linux** (Hyprland/wlroots): wlr-screencopy, FFmpeg / VA-API (AMD Radeon RX 580; no NVIDIA hardware); host streaming verified; GUI QA incomplete
- **Windows**: DXGI capture (ModeDesc physical geometry fix), Media Foundation H.264; host streaming and CLI verified; stage attribution preliminary pending finite zero-overflow trace rerun; native Windows GUI QA incomplete
- **iOS**: VideoToolbox decoding; shared input/lifecycle code; full QA incomplete
- **Android**: Shared support only; no runnable app yet

**Performance:** bounded frame queues, keyframe recovery, native encoding, and
an MTU-safe 1200-byte UDP datagram budget avoiding IP-layer fragmentation.
Decode time is not end-to-end latency; sequence gaps are not a direct measure
of network loss. Static-frame content age is distinct from encoding work
residence, and latency measurements across differing workloads and runs (such as
prior 417 ms vs preliminary 14 ms fresh residence) are not causal comparison proofs.
There is no guaranteed screenshot or input latency, guaranteed 60 fps, or GPU zero-copy.
Rotated portrait displays are not supported for streaming.

See [docs/release-deployment-20260909.md](../../docs/release-deployment-20260909.md),
[docs/agent-first-verification-20260909.md](../../docs/agent-first-verification-20260909.md),
and [docs/remaining-performance-verification-20260909.md](../../docs/remaining-performance-verification-20260909.md) for full evidence and limits.

## Licensing

Original project source: [MIT](../../LICENSE).
Native dependencies and Rust packages retain their own terms:
[third-party notices](../../THIRD_PARTY_NOTICES.md) and [dependency inventory](../../docs/dependency-licenses.md).

The private Omarchy FFmpeg build is nonfree and not a public recipe.
