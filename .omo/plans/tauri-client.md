# tauri-client - Work Plan (전면 Rust/Tauri 전환)

## PIVOT v2 (2026-09-01 확정) — Swift 전면 은퇴, 전면 Rust/Tauri 전환
Swift 호스트도 은퇴. 제품 전체가 단일 Rust 코드베이스(Tauri 셸 포함)가 된다. 프로토콜 이중 구현 제거.

실행 순서 (의존성 기준):
- P0 워크스페이스 전환: 저장소 루트에 Cargo workspace (crates/*). Swift Sources/는 Rust 제품이 동등성에 도달할 때까지 보존 후 삭제.
- P0-SPIKE (최우선 검증): ScreenCaptureKit Rust FFI 스파이크 — objc2-screen-capture-kit으로 캡처 나열 + 스트림 시작 + 프레임 콜백 수신 3초. 실패 시 FFmpeg avfoundation 폴백 결정. 이게 유일한 불확실 조각.
- P1 erd-proto: v3 코덱 이식 (스펙 계약 = 본 문서 하단) + 라운드트립/절단 테스트
- P2 erd-net: TLS-PSK TCP(openssl vendored), UDP AES-GCM + 리플레이 윈도우, ntfy 시그널링 + STUN
- P3 Rust macOS 호스트: SCK 캡처 + FFmpeg hevc_videotoolbox 인코딩 + CGEvent 주입 + 페어링/TLS/GCM 서버 측 + 호스트 UI(PIN/동의/페어링 목록)
- P4 Rust 클라이언트: 페어링 클라이언트 흐름 + FFmpeg 디코드 + wgpu 렌더 + winit 입력 + cpal 오디오
- P5 E2E: Rust 호스트 ↔ Rust 클라이언트 (tart VM), 프레임 타임스탬프 계측(p50/p99) 내장 — Moonlight 비교 벤치
- P6 Tauri 셸 완성 + Windows/Linux 패키징
- P7 Swift 삭제 (동등성 도달 시)

검증 규칙: P3 이후 모든 게이트는 Rust↔Rust E2E. 성능 절대타협불가 — P5 계측으로 Moonlight 역전 확인까지 제품 아님.
TCC 참고: Rust 호스트 앱도 Screen Recording/Accessibility 권한 프롬프트 필요 (온보딩 플로우 이식).



## PIVOT (2026-09-01) — 성능 절대 우선, 전면 제품개발 전환
실무 의존(Moonlight+Sunshine) 중단. 본 플랜(T1-T16)은 Phase-A(클라이언트)로 흡수되고, 아래 Phase-B(호스트)/Phase-0(계측)가 선행 또는 병렬 추가된다. 실행 순서:
- Phase-0 (최우선): 프레임 단위 지연 계측 하니스 — capture→encode→net→decode→present 타임스탬프, p50/p99. 계측 없는 최적화 금지. Moonlight(같은 테일넷) 비교 벤치 포함. RTT 7.7ms 환경.
- Phase-1 (지연 구조 3대 수정, Mac↔Mac): 입력 TCP→UDP 이관(키는 reliable-lite, 마우스는 최신값), 영상 FEC(XOR 패리티 4+1)로 IDR 폭발 제거, 전송 페이싱. + LAN 비트레이트 상한 해제(50-150Mbps HEVC).
- Phase-2: Tauri 클라이언트 T7-T13 (본 플랜) — VM 상호운용 게이트 유지.
- Phase-3 (호스트 확장): erd-host Linux(omarchy: wlr-screencopy/Hyprland + VAAPI/NVENC 판별 + uinput + wl-clipboard) → Windows(DXGI + MFT/NVENC + SendInput). erd-proto/erd-net 공유.
- Phase-4: Windows/Linux 클라이언트 패키징 + 전 매트릭스 E2E.
보안 계층(AES-GCM/PSK/페어링)은 유지 — HW 가속이라 성능 무료. 테스트 규칙 유지: E2E는 tart VM에서.


## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** <fill last - deliverables in human terms, 1-2 sentences>

**Why this approach:** <fill last - the one or two load-bearing decisions and why>

**What it will NOT do:** <fill last - 1-3 plain lines mirroring Must NOT have>

**Effort:** Large
**Risk:** High - Windows TLS-PSK toolchain and native video surface are the two unproven integrations
**Decisions to sanity-check:** openssl-vendored TLS-PSK (vs server cert mode); FFmpeg dynamic-link licensing for personal use; Windows runtime QA requires a real Windows machine (CI only builds)

Your next move: approve, or run a high-accuracy review. Full execution detail follows below.

---

> TL;DR (machine): Large/High — Rust workspace (proto/net/decode/render/app) + Tauri shell; Windows-first HEVC viewer for the macOS EclipticRD host; conformance against Swift TestCLI/E2E in tart VM.

## Scope
### Must have
- clients/rust/ cargo workspace: erd-proto (v3 packet codec), erd-net (TLS-PSK TCP + UDP AES-256-GCM + signaling + STUN), erd-decode (FFmpeg HEVC), erd-render (wgpu surface), erd-app (session state machine, input, audio)
- docs/protocol-v3.md: byte-level wire spec (the implementation contract, extracted from Swift sources)
- Tauri shell: connection UI (PIN + direct IP), stats overlay, session controls — over a native video surface
- Interop conformance: Rust client pairs with and streams from the Swift host in the tart VM; protocol vectors ported from Swift tests
- Windows packaging: NSIS installer bundling FFmpeg DLLs; GitHub Actions windows build gate
- Linux build support (build + smoke; runtime polish secondary)

### Must NOT have (guardrails, anti-slop, scope boundaries)
- NO Swift host-side changes (the macOS host is frozen for this plan)
- NO H.264 (HEVC only), NO mDNS client (manual IP + PIN only), NO RDP/VNC compatibility
- NO video rendering inside the WebView (native wgpu surface only)
- NO zero-copy D3D11 texture sharing in v1 (staging-copy first; v2 optimization)
- NO commercial-distribution licensing work (personal use assumed; FFmpeg LGPL dynamic-link note only)
- NO Keychain/system-keyring in the Rust client (plaintext file, 0600, mirroring the Swift host's file store convention)
- NO server-driven bitrate protocol changes (implement existing ABR messages as-is)

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: tests-after for the spec doc; TDD for erd-proto codec (round-trip + truncation vectors ported from Swift Tests/main.swift) and for replay-window logic (vectors ported from Tests/XCTests/CryptoProtocolTests.swift); integration tests in erd-net against a local openssl PSK server; framework: cargo test + a tart-VM interop harness (scripts/vm-interop.sh)
- Evidence: .omo/evidence/ulw/tauri-client/task-<N>-*.{log,png,md} (attemptDir = .omo/evidence/)

## Execution strategy
### Parallel execution waves
> Target 5-8 todos per wave. Fewer than 3 (except the final) means you under-split.
- Wave 1 (foundations, parallel): T1 spec doc, T2 workspace scaffold, T3 erd-proto codec+tests
- Wave 2 (transport, parallel): T4 TLS-PSK TCP, T5 UDP GCM + replay, T6 signaling + STUN
- Wave 3 (interop gate, serial): T7 session state machine, T8 tart VM interop harness
- Wave 4 (media, parallel): T9 decode, T10 render, T11 input+audio, T12 ABR+clipboard+cursor
- Wave 5 (shell+ship, parallel): T13 Tauri shell UI, T14 pairing UI, T15 Windows packaging, T16 docs update

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| T1 | - | T3..T16 | T2 |
| T2 | - | T3..T16 | T1 |
| T3 | T1,T2 | T4,T5,T6,T7 | - |
| T4 | T3 | T7 | T5,T6 |
| T5 | T3 | T7,T9..T12 | T4,T6 |
| T6 | T3 | T7 | T4,T5 |
| T7 | T4,T5,T6 | T8..T16 | - |
| T8 | T7 | T9..T16 | - |
| T9 | T8 | T10,T13 | T11,T12 |
| T10 | T9 | T13 | T11,T12 |
| T11 | T8 | T13 | T9,T10 |
| T12 | T8 | T13 | T9,T10,T11 |
| T13 | T9..T12 | T15,T16 | T14 |
| T14 | T7 | T13 | - |
| T15 | T13 | F-wave | T16 |
| T16 | T1..T15 | F-wave | T15 |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [ ] 1. docs/protocol-v3.md — v3 wire spec from .omo/drafts/tauri-client.md inventory
  What to do / Must NOT do: transcribe the draft inventory (packet types table, framing, handshake v3, pairing, TLS-PSK derivation strings, UDP GCM layout, video/audio/input/control formats, signaling) into docs/protocol-v3.md with byte tables; MUST NOT invent fields absent from the draft; each section cites the Swift file it was extracted from
  Parallelization: Wave 1 | Blocked by: - | Blocks: T3..T16
  References (executor has NO interview context - be exhaustive): .omo/drafts/tauri-client.md (spec inventory section), Sources/Shared/PacketHeader.swift, FramePayloads.swift, InputPayloads.swift, HandshakePayload.swift, PairingPayloads.swift, ERDCrypto.swift, ERDIdentity.swift, ERDConstants.swift, ProtocolFoundation.swift, SignalingClient.swift
  Acceptance criteria (agent-executable): every section of the draft inventory appears in the doc; a reader can implement a client without reading Swift
  QA scenarios: cross-check doc vs draft line-by-line, Evidence .omo/evidence/ulw/tauri-client/task-1-spec-diff.md
  Commit: Y | docs(protocol): v3 wire spec for cross-platform clients
- [ ] 2. clients/rust/ workspace scaffold (5 crates + toolchain + CI)
  What to do: cargo workspace erd-proto/erd-net/erd-decode/erd-render/erd-app; rust-toolchain.toml (stable); .github/workflows/rust-client.yml building on macos-latest + ubuntu-latest + windows-latest (windows job uses openssl vendored feature); workspace deps pinned (tokio, openssl, aes-gcm, hkdf, ffmpeg-next, wgpu, winit, cpal, tauri 2.x)
  Parallelization: Wave 1 | Blocked by: - | Blocks: T3..T16
  References: project layout must live under clients/rust/; host repo untouched except this dir + docs/
  Acceptance criteria: cargo build --workspace && cargo test --workspace green on macOS; windows-latest CI job green (build only)
  QA scenarios: cargo test run captured, Evidence .omo/evidence/ulw/tauri-client/task-2-build.log
  Commit: Y | build(client): rust workspace scaffold with 3-platform CI
- [ ] 3. erd-proto: v3 codec + ported vectors (TDD)
  What to do: PacketHeader 12B codec, TCP frame codec (4B LE prefix, 16MB cap), handshake v3 payload, pairing payloads, input 21B payload, control messages 0-13, frame header/chunk codecs; truncation fuzz-style tests (every payload truncated at every boundary must deserialize to None/error); port Swift test vectors from Tests/main.swift + Tests/XCTests/CryptoProtocolTests.swift where byte-exact
  Parallelization: Wave 1 | Blocked by: T1,T2 | Blocks: T4..T7
  References: .omo/drafts/tauri-client.md spec section (byte-exact), Sources/Shared/PacketHeader.swift, FramePayloads.swift, InputPayloads.swift, HandshakePayload.swift, PairingPayloads.swift, ControlTypes.swift
  Acceptance criteria: cargo test -p erd-proto green; every struct has round-trip + truncation cases; PacketType table matches docs/protocol-v3.md
  QA scenarios: cargo test -p erd-proto captured; mutation check: flip magic constant in test, assert decode fails, Evidence .omo/evidence/ulw/tauri-client/task-3-proto-test.log
  Commit: Y | feat(client): v3 packet codec with ported conformance vectors
- [ ] 4. erd-net: TLS-PSK TCP channel
  What to do: openssl crate (vendored) client: PSK callback with identity + key (identity utf8 bytes, no null), pin TLS1.2 PSK ciphersuites (TLS_PSK_WITH_AES_128_GCM_SHA256 or AES_256_GCM_SHA384), min TLS1.2; 4B LE framing codec reuse from erd-proto; connect/read/write with reconnect backoff; PSK modes: bootstrap (erd-b1 + PIN key) and pairing (erd-p1.<id> + key file)
  Parallelization: Wave 2 | Blocked by: T3 | Blocks: T7
  References: .omo/drafts/tauri-client.md TLS section; Swift TCPChannel.swift for framing semantics; rust-openssl SslContextBuilder PSK callback docs
  Acceptance criteria: cargo test -p erd-net: PSK loopback (local openssl PSK server) completes handshake + framed echo; truncation and oversized-frame tests match Swift behavior (16MB drop)
  QA scenarios: PSK loopback test captured, Evidence .omo/evidence/ulw/tauri-client/task-4-psk-loopback.log
  Commit: Y | feat(client): TLS-PSK TCP channel via openssl
- [ ] 5. erd-net: UDP AES-256-GCM + HKDF keys + replay window
  What to do: port Swift DatagramCipher semantics — 12B nonce (4B prefix + 8B BE counter), AAD = plaintext header bytes, 4096 sliding replay window with 64-bit block mask, per-direction keys via HKDF chain (ikm/salt/info strings byte-identical to ERDCrypto.swift); sequence counter u32 wrap handling; datagram = plaintext header + sealed payload
  Parallelization: Wave 2 | Blocked by: T3 | Blocks: T7,T9..T12
  References: Sources/Shared/ERDCrypto.swift, UDPChannel.swift; Tests/XCTests/CryptoProtocolTests.swift (DatagramCipher tests — port all 6 cases)
  Acceptance criteria: cargo test -p erd-net: round-trip, tamper-reject, AAD-mismatch-reject, duplicate-reject, stale-outside-window-reject, direction-keys-incompatible — all green (port of Swift DatagramCipherTests)
  QA scenarios: cargo test -p erd-net cipher:: captured, Evidence .omo/evidence/ulw/tauri-client/task-5-cipher-test.log
  Commit: Y | feat(client): UDP AES-GCM cipher with replay window
- [ ] 6. erd-net: signaling (ntfy) + STUN
  What to do: ntfy candidate exchange — topic "erd3-"+HKDF(pin).prefix(14) hex, POST base64(AES-GCM(JSON candidate)), poll /json?poll=1&since=10m at 1s to 10s deadline, POST status check (non-2xx = error); STUN RFC5389 XOR-MAPPED-ADDRESS (publicIP/port) against stun.l.google.com:19302
  Parallelization: Wave 2 | Blocked by: T3 | Blocks: T7
  References: Sources/Shared/SignalingClient.swift, STUNClient.swift
  Acceptance criteria: unit tests — candidate JSON round-trip, encrypt/decrypt, role filter, topic length ≤64; live ntfy round-trip test gated behind env ERD_LIVE_TESTS=1 (skipped otherwise, never silently passing)
  QA scenarios: gated live test run captured when ERD_LIVE_TESTS=1, Evidence .omo/evidence/ulw/tauri-client/task-6-signaling.log
  Commit: Y | feat(client): ntfy signaling + STUN discovery
- [ ] 7. erd-app: session state machine (pairing → handshake → media)
  What to do: full client flow — bootstrap pairing (connect erd-b1 + PIN PSK → send pairingRequest → await grant/reject → persist pairing file 0600 → send handshake v3 with pairingID+16B salt → await handshakeAck → derive UDP keys → ready); reconnect with pairing file (erd-p1 identity); media loop (frames → decode queue, audio → playback queue, input → TCP); gating (no media before handshakeAck); timeouts mirroring Swift client (10s connect, 2s ping)
  Parallelization: Wave 3 | Blocked by: T4,T5,T6 | Blocks: T8..T16
  References: Sources/ClientCore/ClientCore.swift (flow), Sources/Shared/HandshakePayload.swift, ERDIdentity.swift
  Acceptance criteria: state machine unit tests — pairing grant path, reject path, pairingDisabled reject path, handshakeAck timeout, media-before-handshake gating (inputEvent refused pre-auth is a HOST rule; client-side mirror: refuse to send media packets pre-ready)
  QA scenarios: state machine test log, Evidence .omo/evidence/ulw/tauri-client/task-7-statemachine.log
  Commit: Y | feat(client): session state machine with pairing flow
- [ ] 8. tart VM interop harness (scripts/vm-interop.sh)
  What to do: script that — boots/uses tart VM eclipticrd-ci, rsyncs repo, builds+runs Swift TestCLI host-mode listener, builds+runs the Rust client binary against it, asserts pairing+handshake+frames-received; pkill stale E2ETest/EclipticRD processes first; used as THE conformance gate for M2
  Parallelization: Wave 3 | Blocked by: T7 | Blocks: T9..T16
  References: scripts/ conventions in repo; tart ip eclipticrd-ci; ssh -i ~/.ssh/id_ed25519 admin@<ip>; stale-process pkill mandatory (allowLocalEndpointReuse lets two processes share the port)
  Acceptance criteria: scripts/vm-interop.sh exits 0 with "RUST_CLIENT_STREAMED frames>=10" line
  QA scenarios: harness run captured, Evidence .omo/evidence/ulw/tauri-client/task-8-interop.log
  Commit: Y | test(client): tart VM interop harness for Rust client
- [ ] 9. erd-decode: FFmpeg HEVC (AVCC extradata, d3d11va + sw)
  What to do: ffmpeg-next decoder — hevc codec, extradata = VPS/SPS/PPS blob from handshake-era keyframe NALUs, feed 4B length-prefixed NALUs directly; hw accel d3d11va (hw_device_ctx) with automatic sw fallback; output NV12→RGB in shader later (decode outputs NV12 frames); frame pacing accounting
  Parallelization: Wave 4 | Blocked by: T8 | Blocks: T10,T13
  References: .omo/drafts/tauri-client.md video section; Sources/ClientCore/VideoDecoder.swift (behavior reference); ffmpeg-next docs
  Acceptance criteria: decode test — feed HEVC frames from Swift encoder capture (VM interop dump), assert ≥30 decoded fps at 640x360 sw and ≥60fps d3d11va at 1080p; graceful error on corrupt NALU
  QA scenarios: decode bench captured, Evidence .omo/evidence/ulw/tauri-client/task-9-decode-bench.log
  Commit: Y | feat(client): FFmpeg HEVC decode pipeline
- [ ] 10. erd-render: wgpu video surface (NV12 → RGB shader)
  What to do: wgpu surface sized to the video frame; NV12 upload as two textures (luma+chroma) with a YUV→RGB fragment shader (BT.709), present at decode pace; window resize handling; vsync off for latency
  Parallelization: Wave 4 | Blocked by: T9 | Blocks: T13
  References: Sources/ClientCore/MetalRenderer.swift + Shaders.metal (port shader math); Sources/ClientCore/MetalRenderer.swift NV12 handling
  Acceptance criteria: render test — 60fps present of 1080p NV12 frames measured via wgpu timestamp/present counter; screenshot artifact for color correctness vs Swift host screenshot
  QA scenarios: render bench + screenshot, Evidence .omo/evidence/ulw/tauri-client/task-10-render.{log,png}
  Commit: Y | feat(client): wgpu NV12 render surface
- [ ] 11. Input capture (winit) + audio (cpal)
  What to do: winit on the video window — mouse move/down/up/drag, scroll, keyboard down/up + modifiers → erd-proto InputEventPayload (21B) with normalization x/w, y=1-y/h clamped; keymap Windows VK→macOS keyCode table (top 60 keys + modifier mapping); cpal output stream 48kHz stereo f32 from the audio reassembly queue; volume/mute control
  Parallelization: Wave 4 | Blocked by: T8 | Blocks: T13
  References: Sources/ClientCore/InputSender.swift, ClientAudioPlayer.swift, Sources/Shared/InputPayloads.swift; key map table: Sources/HostCore/InputReceiver.swift CGEvent usage
  Acceptance criteria: unit tests — normalization clamp/Y-flip cases; keymap table test (VK code → macOS keyCode for alphabet/digits/arrows/space/enter/esc/shift/ctrl); audio test — 48kHz f32 queue drains without underrun for 5s synthetic stream
  QA scenarios: input unit test log + audio drain test, Evidence .omo/evidence/ulw/tauri-client/task-11-input-audio.log
  Commit: Y | feat(client): input capture and audio playback
- [ ] 12. ABR + clipboard sync + cursor overlay
  What to do: ABR — loss stats from frame assembly, >5% → bitrateAdjust(0.75x), 30s stable → recover (mirror Swift ClientCore ABR); clipboard — text clipboard sync receive (apply to OS clipboard, loop-prevention hash+changeCount) and send (poll 0.5s, 4KB cap, concealed-type filter port); cursor — CursorUpdate → cursor position overlay on video surface
  Parallelization: Wave 4 | Blocked by: T8 | Blocks: T13
  References: Sources/ClientCore/ClientCore.swift (ABR), Sources/Shared/ClipboardMonitor.swift (suppression/conceal logic — port semantics)
  Acceptance criteria: unit tests — ABR step-down/up thresholds, clipboard hash+changeCount echo suppression, concealed-type suppression (port ClipboardMonitorTests semantics minus live pasteboard)
  QA scenarios: unit test log, Evidence .omo/evidence/ulw/tauri-client/task-12-abr-clipboard.log
  Commit: Y | feat(client): ABR, clipboard sync, cursor overlay
- [ ] 13. Tauri shell: connection UI, stats overlay, session controls
  What to do: Tauri 2 app — connection screen (PIN manual connect + direct IP:port + remember last), in-session overlay (FPS/latency/loss stats, session controls: fullscreen, disconnect, mute, quality), system tray; native video window (winit) layered per T13 design — WebView hosts ONLY controls/overlays; IPC: tauri commands invoking erd-app APIs, events polling stats at 2Hz
  Parallelization: Wave 5 | Blocked by: T9,T10,T11,T12 | Blocks: T15,T16
  References: Sources/App/RemoteDesktopView.swift + HomeView.swift (UI feature reference — port the FEATURE SET, not the SwiftUI code); Sources/App/ConnectionManager.swift (state machine reference)
  Acceptance criteria: UI QA — connection screen functional, overlay stats render at 2Hz during session, controls work (mute/fullscreen/disconnect); screenshot artifacts
  QA scenarios: browser/dev screenshot of shell + native window composition, Evidence .omo/evidence/ulw/tauri-client/task-13-shell.{png,log}
  Commit: Y | feat(client): Tauri shell with session UI
- [ ] 14. Pairing UI + PIN flow + settings
  What to do: pairing flow UI — PIN entry screen (8-digit), host consent wait state, paired-device list (from erd-p1 file store) with remove, settings (remember host, mute default); error surfacing for pairingDisabled/lockedOut
  Parallelization: Wave 5 | Blocked by: T7 | Blocks: T13
  References: Sources/App/HomeView.swift remotePINCard + paired-devices section (feature reference)
  Acceptance criteria: UI QA — full pairing via UI against Swift host in tart VM completes; locked-out state surfaces the lockout message
  QA scenarios: pairing UI walkthrough screenshot + log, Evidence .omo/evidence/ulw/tauri-client/task-14-pairing.{png,log}
  Commit: Y | feat(client): pairing UI and PIN flow
- [ ] 15. Windows packaging: NSIS + FFmpeg DLLs + CI
  What to do: tauri bundler NSIS target; bundle ffmpeg DLLs (avcodec/avutil/swresample + deps) beside the exe; openssl static or DLL bundling; GitHub Actions windows job = build + package + smoke (installer exists, exe launches with --version); license NOTICE file (FFmpeg LGPL attribution, HEVC patent note)
  Parallelization: Wave 5 | Blocked by: T13 | Blocks: T16
  References: project.yml licensing notes; ERDConstants
  Acceptance criteria: windows-latest CI job green producing installer artifact; smoke run exit 0
  QA scenarios: CI run log, Evidence .omo/evidence/ulw/tauri-client/task-15-package.log
  Commit: Y | build(client): Windows packaging with bundled FFmpeg
- [ ] 16. Repo docs: client README + cross-platform notes
  What to do: clients/rust/README — build/run instructions per OS, VM interop harness usage, protocol doc link, limitations (HEVC only, PIN mode first, Windows runtime QA manual step)
  Parallelization: Wave 5 | Blocked by: T15 | Blocks: -
  References: AGENTS.md, docs/protocol-v3.md
  Acceptance criteria: README covers build (3 OS), test, VM harness, limitations
  QA scenarios: doc review vs implemented reality, Evidence .omo/evidence/ulw/tauri-client/task-16-readme.md
  Commit: Y | docs(client): client README and cross-platform notes

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit — every Must-have shipped, zero Must-NOT-have violations; audit vs this file
- [ ] F2. Code quality review — cargo clippy --workspace clean, no unwrap in network/decode paths, no unsafe outside ffmpeg binding glue
- [ ] F3. Real manual QA — full session in tart VM: Rust Windows-build smoke + macOS Rust client pairs, streams ≥10 frames at target fps, input round-trips, audio plays; artifacts captured
- [ ] F4. Scope fidelity — Swift host untouched (git diff Sources/ empty), protocol doc matches implemented codec

## Commit strategy
Conventional commits per task: feat(client)/test(client)/build(client)/docs(client); one commit per verified task; branch client/tauri-mvp; no WIP commits on main.

## Success criteria
- M1 gate: docs/protocol-v3.md + erd-proto round-trip/truncation suite green (cargo test -p erd-proto)
- M2 gate: tart VM interop harness exit 0 with Rust client paired + streamed from Swift host
- M3 gate: decoded frames render at ≥60fps 1080p-equivalent; input round-trip verified via Swift host trace; audio audible without underrun
- M4 gate: full UI-driven session on Windows build: pair → stream → control → disconnect
- M5 gate: windows-latest CI green producing installer; macOS/Linux builds green
- Regression guard: Swift host suites stay green (TestCLI 38/38, EclipticRDLogicTests SUCCEEDED) — verified before final handoff

## v3 Wire protocol spec (implementation contract)
### PacketHeader (12B, PacketHeader.swift)
[0..1] magic u16 LE = 0xEC1D | [2] type u8 | [3..6] seq u32 LE | [7..10] ts u32 LE (ms since epoch, truncate) | [11] flags u8
PacketType: handshake=0, handshakeAck=1, frameHeader=2, frameChunk=3, cursorUpdate=4, inputEvent=5, control=6, ping=7, audioFrame=8, pairingRequest=9, pairingGrant=10, pairingReject=11
### TCP framing (TCPChannel.swift)
4-byte LE length prefix + payload; max frame 16,777,216B (16MiB); larger or zero length → buffer drop
### Handshake v3 (HandshakePayload.swift)
[nameLen u16 LE][name utf8][width u16 LE][height u16 LE][scale f32 LE][version u8 = 3][caps u64 LE: bit0 streamConfiguration, bit1 clipboardSync, bit2 textClipboardSync][pairingIdLen u16 LE][pairingId utf8 ≤256][sessionSalt 16B]  — version must equal 3 (legacy 1 rejected)
### Pairing payloads (PairingPayloads.swift)
Request: [nameLen u16 LE][name utf8 ≤1024]. Grant: [idLen u8][id utf8][nameLen u16 LE][name utf8][keyLen u8 = 32][key]. Reject: [reason u8: 0 deniedByHost, 1 lockedOut, 2 pairingDisabled]
### Pairing / TLS (ERDIdentity.swift, ERDCrypto.swift)
PSK identities: bootstrap "erd-b1", pairing "erd-p1.<uuid>". Bootstrap PSK = HKDF-SHA256(ikm = PBKDF2-SHA256(pin, salt "erd/bootstrap/v3", 600000 rounds, 32B), salt "erd/bootstrap/v3", info "erd/tls-psk"). TLS 1.2 PSK ciphersuites (TLS 1.3 PSK fails on Apple stack), verify always-true (transcript MAC is the auth). Lockout: 5 handshake failures/60s → bootstrap disabled 300s. Pairing file store: plaintext JSON, 0600, Application Support/EclipticRD/pairing-keys.json (host convention — Rust client mirrors)
### UDP media (UDPChannel.swift, ERDCrypto.swift)
Datagram = 12B plaintext PacketHeader + AES-256-GCM(payload, aad = those 12 header bytes); ciphertext = 12B nonce [4B key-prefix][8B BE counter] || ct || 16B tag. Direction keys: ikm = HKDF-SHA256(ikm = pairingKey, salt = sessionSalt, info "erd/udp-ikm/v3"); key = HKDF(ikm, sessionSalt, "erd/udp-c2h/v3" | "erd/udp-h2c/v3"); nonce prefix = HKDF(ikm, sessionSalt, <dir-info>+"/nonce", 4B). Replay window 4096 sliding with per-64-block bitmask. Session salt: client random 16B in handshake
### Video (FrameSender.swift, FramePayloads.swift)
FrameHeaderPayload 16B: [0..3] frameId u32 LE, [4..5] width u16 LE, [6..7] height u16 LE, [8] isKeyFrame u8, [9..10] totalChunks u16 LE, [12..15] totalSize u32 LE. Chunk: [frameId u32 LE][chunkIndex u16 LE][data ≤1382]. totalChunks cap 1024, totalSize cap 32MiB. HEVC: 4B BE length-prefixed NALUs; keyframes prepend VPS/SPS/PPS as length-prefixed NALUs (AVCC-style; FFmpeg: set extradata, feed as-is)
### Audio (ServerCore.sendAudio, ClientCore.handleAudioPayload)
PCM 48000Hz stereo Float32 interleaved. Fragment: [frameId u32 LE][fragIdx u16 LE][fragCount u16 LE][data ≤1380]
### Input (InputPayloads.swift) — 21B
[type u8][x f32 LE][y f32 LE][keyCode u16 LE][modifiers u16 LE][scrollDX f32 LE][scrollDY f32 LE]. Types: 0 mouseMove, 1 leftDown, 2 leftUp, 3 rightDown, 4 rightUp, 5 scrollWheel, 6 keyDown, 7 keyUp, 8 flagsChanged, 9 leftDragged, 10 rightDragged. Modifiers u16: shift 1, ctrl 2, option 4, command 8, capsLock 16. Normalization: x = clamp(x/w, 0, 1); y = 1 - clamp(y/h, 0, 1) (client→host); host maps to absolute pixels
### Control (ControlTypes.swift)
[type u8][payload]. 0 requestKeyFrame, 1 startStream, 2 stopStream, 3 disconnect, 4 ping, 5 pong, 6 bitrateAdjust [targetBitrate i32 LE], 7 streamConfigRequest [requestId u32 + w u32 + h u32 + bitrate u32 + fps u16], 8 response, 9 reject, 10 error, 11 clipboardSyncRequest, 12 clipboardSyncUpdate [len u32 + utf8], 13 clipboardSyncError. Host rejects control/input from peers that have not completed handshake
### Heartbeat / cursor
Host ping every 2s, 3 missed → disconnect. CursorUpdate: [x f32 LE][y f32 LE][type u8] on .cursorUpdate
### Signaling (SignalingClient.swift, ntfy.sh)
topic = "erd3-" + hex(HKDF-SHA256(ikm=pin, salt "erd/signaling/v3", info "erd/topic").prefix(14)) — 33 chars (ntfy 404s >64). Candidate POST body = base64(AES-256-GCM(JSON{role, localIP, localPort, publicIP, publicPort}, key = HKDF(ikm=pin, salt "erd/signaling/v3", info "erd/payload-key"))), Content-Type text/plain. Poll GET https://ntfy.sh/<topic>/json?poll=1&since=10m at 1s to 10s deadline; parse JSONL .message → base64 → decrypt → role != own

## Must NOT have (scope guard v1)
- 상용 배포 대응(FFmpeg/HEVC 라이선스 처리), mDNS 클라이언트, H.264, D3D11 zero-copy, 웹뷰 비디오, Swift 호스트 변경 — 전부 제외 (v2 후보)
