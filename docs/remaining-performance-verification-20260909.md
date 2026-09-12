# Remaining Performance Verification and Follow-Up Report - 2026-09-09

## Status and Verification Boundaries

This document is a documentation-only follow-up to the initial [agent-first verification report](agent-first-verification-20260909.md).
It synthesizes native execution evidence, packet captures, and remote toolchain checks conducted under `.omo/remaining-performance-20260909/`.

**Current status:**
- Working tree changes remain uncommitted; no repository commits have been created.
- This is an evidence-based draft report. A finite zero-overflow trial on Windows and geometry review corrections remain pending before final Windows stage attribution can be concluded.
- The prior historical report (`docs/agent-first-verification-20260909.md`) remains preserved as a factual baseline.

**Explicit non-claims:**
- **No final Windows acceptance:** Windows stage attribution remains preliminary due to trace buffer overflow in the initial 90-second run.
- **No cleanup or commits:** Workspace modifications remain in the working tree.
- **No portrait display support:** Rotated display handling is not validated for production desktop streaming.
- **No guaranteed 60 fps:** Streaming frame rates vary with platform, encoder, and desktop composition.
- **No NVENC or GPU zero-copy:** Host video encoding uses CPU or VA-API paths without GPU zero-copy memory transfers.
- **No causal latency optimization claim:** The historical 417 ms fresh-frame residence and the preliminary 14 ms observation come from different workloads, run lengths, and measurement conditions; they do not constitute a causal optimization proof.
- **No retrospective gap attribution:** Historical 13 sequence gaps in earlier active Linux traces are not conclusively attributed to MTU fragmentation.

---

## 1. Encrypted Media Transport and MTU Safety

### Baseline IP Fragmentation
On network paths with encapsulation (such as Tailscale or WireGuard VPN tunnels), the path MTU is often 1280 bytes.
In native baseline testing over a Tailscale inner plaintext capture on UDP port 28631 (`baseline-fragmentation.json`):
- Baseline sender emitted datagrams with up to 1,428-byte UDP payloads, producing 1,456-byte IPv4 datagrams.
- Over a 300-frame baseline run, 3,395 datagrams exceeded the 1,280-byte interface MTU, causing IP-layer fragmentation that generated 6,790 IP fragment records.
- Although all 4,292 datagrams were eventually reassembled and authenticated at the peer in that specific run, path fragmentation increases drop probability across intermediate routers.

### MTU-Safe Packetization Design
To eliminate fragmentation without breaking receiver compatibility:
- **Conservative 1200-Byte Budget:** Sender datagrams are bounded to a maximum of 1,200 bytes total UDP size (including the 8-byte UDP header, fitting within 1,228-byte IPv4 packets and well below the 1,280-byte tunnel MTU).
- **Video Payload Chunk Limit:** Video chunks are capped at 1,154 bytes per datagram (1,200 bytes minus 46 bytes for UDP framing, encryption nonce, MAC tag, and protocol chunk headers).
- **Audio Fragment Limit:** Audio fragments are capped at 1,152 bytes per datagram (1,200 bytes minus 48 bytes framing and encryption).
- **Sender Frame Limit:** With the protocol's 1,024 chunk-per-frame ceiling, the maximum sender frame size is 1,181,696 bytes (1,024 × 1,154 bytes). Frames exceeding this bound are rejected upfront before transmission.
- **Receiver Wire Bounds Preserved:** Wire acceptance constants in `maho-proto` (`MAX_VIDEO_CHUNK_BYTES = 1382` and `MAX_AUDIO_FRAGMENT_BYTES = 1380`) remain unchanged. Existing and third-party receivers continue to accept datagrams up to the historical limits.

### Native Candidate Verification
Native verification was performed on Omarchy Linux over UDP port 28631 (`mtu-native-verification.json`):
- **Frames Transmitted:** 300 frames.
- **Packet Accounting:**
  - Sender captured packets: 4,882
  - Peer captured packets: 4,882
  - Authenticated receiver packets: 4,882
  - Sequence match: 100% (zero missing at peer, zero unauthenticated, zero finalized application gaps).
- **Fragmentation:** 0 sender IP fragments, 0 peer IP fragments.
- **Maximum Packet Sizing:** Maximum observed UDP payload was 1,200 bytes; maximum IP packet size was 1,228 bytes.
- **Trace Overflow:** 0.

### Historical Loss Boundary
The candidate test confirmed complete elimination of IP fragmentation and achieved zero lost packets across 300 frames.
However, the 13 missing sequence gaps recorded in the historical 120-frame active Linux test (`agent-first-verification-20260909.md`) cannot be retrospectively attributed to MTU fragmentation. The historical test did not capture intermediate router packet drops, and retrospective causality is not claimed.

---

## 2. Linux Host Hardware Environment

Prior documentation and triage logs hypothesized that missing NVIDIA kernel modules or broken NVENC configurations explained encoding issues on the Omarchy Linux host.

Direct hardware inspection of the physical host (`indo@100.91.254.71`) clarified the environment (`setup.md`):
- **Physical GPU:** An AMD Radeon RX 580 Series GPU bound to the in-tree `amdgpu` kernel driver. No NVIDIA GPU hardware exists on this host.
- **Driver and Kernel:** Linux kernel `7.1.9-arch1-2-g1483-dirty`.
- **Hardware Acceleration:** The render node `/dev/dri/renderD128` is accessible and functional. `vainfo --display drm --device /dev/dri/renderD128` confirms Mesa 26.2.1, VA-API 1.24, and active hardware entry points for H.264 and HEVC encoding.
- **Correction:** Erroneous assumptions regarding NVIDIA drivers and NVENC are retired. The Linux host pipeline relies on VA-API hardware encoding or software FFmpeg encoding; no NVENC or GPU zero-copy acceleration is claimed.

---

## 3. Strict Clippy Corrections

Pre-existing compiler lints across `maho-app` and `maho-host` were resolved under strict `-D warnings` on the Omarchy builder (`clippy.md`):

1. **`maho-app/src/pairing.rs`:** Removed an unnecessary `return` expression in Unix pairing store resolution; replaced redundant `io::Error::new(io::ErrorKind::Other, ...)` constructs with `io::Error::other(...)`.
2. **`maho-host/src/inject_linux.rs`:** Removed unused constant `ABSOLUTE_AXIS_MAX` and unused helper `scale_to_uinput`.
3. **`maho-host/src/encode_linux.rs`:** Replaced indexed loop with `iter_mut().enumerate()` in NV12 software converter; converted parameter set search to `.contains()`; updated HEVC NAL pattern matching to inclusive range syntax `32..=34`.
4. **`maho-host/src/session.rs`:** Simplified redundant error closure `.map_err(|e| SessionError::Io(e))` to `.map_err(SessionError::Io)`.
5. **`maho-host/src/main.rs`:** Replaced manual reverse comparator with `sort_by_key(|left| Reverse(left.added_at_unix_ms))`.
6. **`maho-app/tests/cli_mcp_contract.rs`:** Collapsed nested `if` statement into a match pattern guard.

### Audit Summary
- **No Suppressions:** Zero `#[allow(...)]` or `#[expect(...)]` attributes were introduced.
- **Verification:**
  - `cargo clippy --locked -p maho-app -p maho-host --all-targets --no-deps -- -D warnings`: Exit code 0, zero warnings, zero errors (`clippy-pass.log`).
  - `cargo test --locked -p maho-app -p maho-host`: All 224 unit, integration, and doc tests passed warning-free (`tests-pass.log`).

---

## 4. Windows DXGI Geometry and Preliminary Stage Attribution

### Root Cause of Initial Frame Halt
During initial trials of the instrumented host on the physical Windows desktop (RTX 4060 Ti, 3840 × 1600 display, 125% DPI scaling), the pipeline aborted on the very first frame (`attributed-stderr.log`):
```text
Native media pipeline stopped error="mf encode: input NV12 frame has the wrong length: expected 5898240, got 9216000"
```
- **Analysis:** DXGI Desktop Coordinates were query-scaled by the 1.25× desktop DPI factor, reporting a logical resolution of 3072 × 1280. The Media Foundation encoder initialized for 3072 × 1280 (expecting 5,898,240 NV12 bytes).
- However, DXGI Desktop Duplication (`AcquireNextFrame`) produces textures matching the physical display video mode (3840 × 1600, requiring 9,216,000 NV12 bytes).
- The encoder rejected the size mismatch and terminated immediately, yielding 0 decoded frames.

### Geometry Resolution
The capture backend was restructured to distinguish physical and logical geometries (`windows-attribution.md`):
- **Physical Resolution (3840 × 1600):** Derived from `DXGI_OUTDUPL_DESC.ModeDesc` (`IDXGIOutputDuplication::GetDesc()`). Used exclusively for texture capture, NV12 conversion buffers, and Media Foundation encoder initialization.
- **Logical Resolution (3072 × 1280, Scale 1250):** Derived from `DXGI_OUTPUT_DESC.DesktopCoordinates`. Used for client display advertisement, handshake metadata, and client-to-host pointer input coordinate mapping.
- **Result:** Native streaming was unblocked. A subsequent 90-second run successfully decoded 3,022 frames, and HTTP screenshots at 3840 × 1600 succeeded with HTTP 200.

### Preliminary Stage Attribution and Trace Overflow Caveat
Stage attribution instrumentation was evaluated during the 90-second run (`windows-preliminary-stages.json`):
- **Total Decoded Frames:** 3,022.
- **Trace Buffer Saturation:** The host trace buffer is fixed at 65,536 records. Over the 90-second execution, 103,580 overflow records were dropped after buffer saturation.
- **Validity:** Because trace records overflowed, stage timing statistics represent only the first retained observation window (1,187 fresh frames), **not** the complete 90-second distribution.

#### First-Window Observed Timings (1,187 Frames)
- **Handoff Residence (Event 16):** p50 = 243 µs, p95 = 14,170 µs (~14.2 ms), max = 24,512 µs.
- **Total Encode Wall Time (Event 40):** p50 = 21,430 µs (~21.4 ms), p95 = 25,568 µs (~25.6 ms), max = 52,435 µs.
- **NV12 Conversion Duration (Event 18):** p50 = 12,509 µs (~12.5 ms), p95 = 15,995 µs (~16.0 ms).
- **Queue Blocking (Events 22 & 24):** Cursor send block p95 = 6 µs; video send block p95 = 7 µs.

### Non-Causality of 417 ms vs. 14 ms
In the previous active Windows trace (`agent-first-verification-20260909.md`), fresh-frame residence p95 was observed at 417 ms.
In the preliminary window here, fresh residence p95 was observed at ~14.2 ms.
**This difference is not a proof of causal latency optimization:**
1. The workloads differed: the previous run used 50 ms automated pointer nudges under an earlier host revision, whereas this run used continuous motion.
2. The previous trace was captured without buffer overflow over a shorter duration; this trace dropped 103,580 records due to saturation.
3. Final Windows stage attribution remains pending a bounded, finite rerun (e.g. 300 frames with zero overflow) and lead review corrections.

---

## 5. Artifact Reference Summary

The findings in this report correspond directly to the evidence retained in `.omo/remaining-performance-20260909/`:

| Topic | Artifact File | Primary Evidence |
|---|---|---|
| Environment | `setup.md` | Linux AMD RX 580 / VA-API GPU verification; process boundaries |
| MTU Baseline | `baseline-fragmentation.json` | 3,395 fragmented datagrams (6,790 IP fragments) across 300 frames on 1280 MTU |
| MTU Candidate | `mtu-native-verification.json` | 4,882 packets sent/received/authenticated, 0 IP fragments, 0 gaps, max 1200 B payload |
| Clippy Audit | `clippy.md`, `clippy-pass.log` | Clean Clippy pass under `-D warnings` on Omarchy with zero suppressions |
| Test Suite | `tests-pass.log` | 224 unit, integration, and doc tests passing across `maho-app` and `maho-host` |
| Windows Geometry | `attributed-stderr.log`, `windows-attribution.md` | 3072×1280 vs 3840×1600 DPI error diagnosis and physical ModeDesc fix |
| Windows Preliminary Stages | `windows-preliminary-stages.json` | 3,022 frames decoded; 103,580 trace overflow count; first-window stage timings |

---

## 6. Bounded Finite Windows 300-Frame Run (Zero Overflow)

To validate timing metrics without the 103,580 overflow records observed in the initial 90-second run, a bounded 300-frame run was conducted against the physical Windows desktop (`windows-finite-verification.json` and `windows-finite-stats.json`):

- **Decoded Frames:** Exactly 300 video frames decoded.
- **Trace Buffer Saturation:** 0 overflow records dropped (`hostOverflow: 0`). Total captured host records: 15,770.
- **Receiver Integrity:** 9,818 receiver trace records; 0 receiver overflow; **0 missing packets** (`packet_loss_ratio: 0.0`, `loss_missing_packets: 0`).
- **Process Clean Exit:** Native host exit code 0 (`nativeHostExit: 0`), client exit code 0 (`clientExit: 0`).

### Stage Timing Attributions (Complete 300-Frame Distribution from finite-host-trace.json)

| Event ID | Metric Description | Sample Count ($n$) | Median (p50) | 95th Percentile (p95) | Maximum |
|---|---|---|---|---|---|
| **15** | Content Age / Frame Age (`frame.ages`) | 301 | 14,003 µs (14.0 ms) | 29,941 µs (29.9 ms) | 32,858 µs (32.9 ms) |
| **16** | Fresh Residence (Publication Wait to Encode) | 301 | 293 µs (0.29 ms) | **16,949 µs (16.9 ms)** | 19,023 µs (19.0 ms) |
| **18** | NV12 Texture Conversion Duration | 301 | 12,634 µs (12.6 ms) | 14,951 µs (15.0 ms) | 20,997 µs (21.0 ms) |
| **22** | Cursor Queue Send Block | 322 | 3 µs | 4 µs | 64 µs |
| **24** | Video Queue Send Block | 301 | 4 µs | 5 µs | 14 µs |
| **26** | Sample Allocation Duration | 301 | 1,171 µs (1.17 ms) | 1,304 µs (1.30 ms) | 1,528 µs (1.53 ms) |
| **28** | MF ProcessInput Duration | 301 | 645 µs (0.65 ms) | 846 µs (0.85 ms) | 15,040 µs (15.0 ms) |
| **34** | MF ProcessOutput (301 Output + 301 NeedInput) | 602 | Combined median: 8,224 µs (arithmetic) / 16,433 µs (upper p50); Active output median: 18,475 µs | 20,557 µs (20.6 ms) | 44,301 µs (44.3 ms) |
| **40** | **Total Encode Wall Time** | 301 | **20,680 µs (20.7 ms)** | **24,383 µs (24.4 ms)** | **56,703 µs (56.7 ms)** |

### Causality Disclaimer and Workload Distinction
The observed p95 fresh residence of **16.9 ms** reflects the execution characteristics of the current workload (Edge browser rendering continuous GPU CSS 3D transforms). As noted in the `comparisonLimit` metadata (`windows-finite-verification.json`), this metric cannot be causally compared against the historical 417–490 ms residence delay observed under earlier host revisions, because the previous benchmark was conducted with 50 ms periodic synthetic pointer nudges on a static desktop, whereas the current benchmark was driven by continuous active GPU compositing.

---

## 7. Multiplatform Strict Clippy Scope and Full Workspace Regression

All compiler gates were executed exclusively on the Omarchy builder under `-D warnings`:

1. **Full Workspace Linux Clippy Gate (`--workspace` without package narrowing):**
   ```bash
   cargo clippy --manifest-path clients/rust/Cargo.toml --locked --workspace --exclude maho-ios --all-targets --no-deps -- -D warnings
   ```
   - **Result:** 0 warnings, 0 errors (Exit Code 0).
   - **Receipt Artifact:** `.omo/remaining-performance-20260909/workspace-clippy-full.log`.
   - **Scope:** Complete, unnarrowed workspace sweep across all Linux member crates (`maho-proto`, `maho-net`, `maho-decode`, `maho-render`, `maho-app`, `maho-host`, `tauri-shell`).

2. **Windows Target Clippy Gate (Explicitly Scoped to Windows Host Daemon):**
   ```bash
   cargo clippy --manifest-path clients/rust/Cargo.toml --locked -p maho-host --all-targets --target x86_64-pc-windows-gnu --no-deps -- -D warnings
   ```
   - **Result:** 0 warnings, 0 errors (`WINDOWS_CLIPPY_PASS`).
   - **Receipt Artifact:** `.omo/remaining-performance-20260909/windows-clippy.log`.
   - **Criterion Replacement & Boundary:** Cross-compiling the entire workspace (`--workspace`) for `x86_64-pc-windows-gnu` on Linux fails at `ffmpeg-sys-next` (a dependency of client decoder `maho-decode`) because `pkg-config` has no MinGW FFmpeg sysroot installed (`.omo/remaining-performance-20260909/windows-workspace-clippy-error.log`). Because `maho-host` is the sole native Windows host daemon and uses native Media Foundation (not FFmpeg), the Windows acceptance criterion is explicitly defined as **Windows-host-only verification** (`-p maho-host`), which passes cleanly with zero warnings under `-D warnings`.

3. **Full Workspace Regression:**
   ```bash
   cargo test --manifest-path clients/rust/Cargo.toml --locked --workspace --exclude maho-ios
   ```
   - **Result:** 510 tests passed across all crates, 0 failed, 1 ignored pre-existing network test (`ALL_WORKSPACE_TESTS_PASS`).

---

## 8. Real-Surface Physical Host Verification & Agent Lifecycle

The Mac release client (`maho-client`) was executed against the physical Windows host (`100.126.171.58:28530`):

1. **Transport & Pairing:** Initiated bootstrap pairing via 8-digit PIN; completed handshake in 102 ms; decoded 30 frames of 3840 × 1600 H.264 video with 0 packet loss across 510 datagrams.
2. **HTTP Agent API (`127.0.0.1:19735`):**
   - `GET /api/v1/health` -> HTTP 200 `{"ok": true}`.
   - `GET /api/v1/screen/info` -> HTTP 200 `{"width": 3840, "height": 1600, "scale": 1, "connected_host": "100.126.171.58"}`.
   - `GET /api/v1/screen/screenshot` -> HTTP 200 returning valid 3840 × 1600 PNG image (1,573,410 bytes, verified by `file` utility).
   - `POST /api/v1/input/action` -> Physical Windows cursor successfully displaced from (1201, 795) to (1541, 640).
   - `POST /api/v1/session/disconnect` -> HTTP 200; client process exited with code 0; Windows native host completed with `QA_PROCESS_EXIT=0`.

---

## 9. Resource Cleanup Receipts

- **Windows Testbed (`DESKTOP-1LAPJMP`):**
  - Scheduled tasks `maho-performance-workload-IgdJVT` and `maho-performance-host-IgdJVT` unregistered (`TaskWorkloadExists: false`, `TaskHostExists: false`).
  - Temporary Edge test profile and directory `C:\Users\sook\AppData\Local\Temp\maho-performance-20260909-IgdJVT` removed (`TempDirExists: false`).
  - Production daemon PID 18760 preserved continuously (`ProductionHostPid: 18760`).
- **Omarchy Builder (`indo@100.91.254.71`):**
  - Isolated builder root `/home/indo/projects/maho-performance-20260909-IgdJVT/` removed (`DIR_EXISTS=false`).
  - Production daemon PID 2505717 preserved continuously (`PROD_BEFORE=2505717`, `PROD_AFTER=2505717`).
