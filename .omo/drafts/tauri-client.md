---
slug: tauri-client
status: drafting
intent: clear
review_required: true
plan_path: .omo/plans/tauri-client.md
plan_sha256: null
review_round_id: null
review_round_limit: 5
pending-action: write and review .omo/plans/tauri-client.md
review:
  momus:
    status: pending
    workspace_root: null
    runtime_home: null
    target: .omo/plans/tauri-client.md
    round_id: null
    plan_sha256: null
    launch_id: null
    session: null
    result: null
approach: <fill: the approach you intend to plan>
---

# Draft: tauri-client

## Components (topology ledger)
<!-- Lock the SHAPE before depth. One row per top-level component that can succeed or fail independently. -->
<!-- id | outcome (one line) | status: active|deferred | evidence path -->

## Open assumptions (announced defaults)
<!-- Record any default you adopt instead of asking, so the user can veto it at the gate. -->
<!-- assumption | adopted default | rationale | reversible? -->

## Findings (cited - path:lines)

## Decisions (with rationale)

## Scope IN

## Scope OUT (Must NOT have)

## Open questions

## Approval gate
status: drafting
<!-- When exploration is exhausted and unknowns are answered, set status: awaiting-approval. -->
<!-- That durable record is the loop guard: on a later turn read it and resume at the gate instead of re-running exploration. -->

## Session state (ulw-plan, 2026-09-01)
- intent: clear — Tauri cross-platform client expansion (Windows-first), outcome specified in prior turns (Tauri shell + native Rust data plane)
- review_required: true (default, momus high-accuracy after approval)
- decisions so far (from prior turns, user-endorsed): Tauri shell + native Rust core; openssl crate for TLS-PSK (rustls no external PSK); FFmpeg HEVC AVCC decode; wgpu native render surface (never WebView video); winit input; cpal audio; Windows-first, Linux second; conformance testing against Swift TestCLI/E2E in tart VM
- open forks for approval brief: (pending lane results — TLS-PSK vs cert-mode for Windows, video-surface embedding pattern, milestone granularity)
- lanes: proto-inventory(explore st_01a05bd6), arch-lane(architect st_01a05bd7), detail-lane(ultrabrain st_01a05bd8), research-lane(librarian st_01a05bd9)
- approval gate: NOT yet approved — plan file written only after user okay

## Protocol spec inventory (verified, file:line from Sources/)
### PacketHeader (12B, PacketHeader.swift)
[0..1] magic u16 LE = 0xEC1D | [2] type u8 | [3..6] seq u32 LE | [7..10] ts u32 LE (ms) | [11] flags u8
PacketType: handshake=0, handshakeAck=1, frameHeader=2, frameChunk=3, cursorUpdate=4, inputEvent=5, control=6, ping=7, audioFrame=8, pairingRequest=9, pairingGrant=10, pairingReject=11 (PacketHeader.swift)
### TCP framing (TCPChannel.swift)
4-byte LE length prefix + payload; max frame 16,777,216B; receive buffer 256KB reads; invalid length → buffer drop.
### Handshake v3 (HandshakePayload.swift)
[nameLen u16 LE][name utf8][width u16][height u16][scale f32][version u8=3][caps u64 LE: bit0 streamConfig, bit2 textClipboardSync][pairingIdLen u16][pairingId utf8 ≤256][sessionSalt 16B]
### Pairing payloads (PairingPayloads.swift)
Request: [nameLen u16][name≤1024]. Grant: [idLen u8][id][nameLen u16][name][keyLen u8=32][key]. Reject: [reason u8: 0 denied/1 locked/2 disabled].
### Pairing/TLS (ERDIdentity.swift, ERDCrypto.swift, TCPChannel.swift)
PSK identities: bootstrap "erd-b1", pairing "erd-p1.<uuid>". Bootstrap PSK = HKDF-SHA256(ikm=PBKDF2-SHA256(pin, salt="erd/bootstrap/v3", 600000 rounds), salt="erd/bootstrap/v3", info="erd/tls-psk"). TLS min v1.2 max v1.3, PSK ciphersuites, verify-block always-true. Lockout: 5 failures/60s → 300s. Session salt: client-generated 16B (SecRandom), sent in handshake.
### UDP media (UDPChannel.swift, ERDCrypto.swift)
Datagram = plaintext 12B PacketHeader + AES-256-GCM(payload, aad=header bytes). Ciphertext = 12B nonce [4B key-prefix][8B BE counter] || ct || 16B tag. Keys per direction: HKDF-SHA256(ikm=HKDF(ikm=pairingKey, salt=sessionSalt, info="erd/udp-ikm/v3"), salt=sessionSalt, info="erd/udp-c2h/v3" | "erd/udp-h2c/v3"; nonce prefix = HKDF(same ikm, info+"/nonce", 4B). Replay window 4096 sliding.
### Video (FrameSender.swift, FramePayloads.swift, ERDConstants.swift)
FrameHeaderPayload 16B: [0..3] frameId u32 LE, [4..5] width u16, [6..7] height u16, [8] isKeyFrame u8, [9..10] totalChunks u16, [12..15] totalSize u32 LE. Chunks: [frameId u32][chunkIndex u16][data]. maxPayload = 1400-12 = 1388; chunk data max = 1388-6 = 1382; maxChunks 1024; maxFrame 32MiB. HEVC AVCC: 4B BE length prefix per NALU; keyframes prepend VPS/SPS/PPS as length-prefixed NALUs.
### Audio (ServerCore.sendAudio, ClientCore.handleAudioPayload)
PCM 48kHz stereo Float32 interleaved (8B/frame). Fragment: [frameId u32][fragIdx u16][fragCount u16][data]; maxChunk = maxPayload-8 = 1380.
### Input (InputPayloads.swift) — 21B
[type u8][x f32 LE][y f32 LE][keyCode u16 LE][modifiers u16 LE][scrollDX f32][scrollDY f32]. Types 0-10 (mouseMove..rightMouseDragged). Modifiers u16: shift=1,ctrl=2,option=4,command=8,capsLock=16. Normalization (client): x/=w clamped [0,1]; y = 1 - clamp(y/h). Host: x*=w, y*=h.
### Control (ControlTypes.swift)
ControlMessage: [type u8][payload]. Types: requestKeyFrame=0, startStream=1, stopStream=2, disconnect=3, ping=4, pong=5, bitrateAdjust=6(targetBitrate i32), streamConfigRequest=7(requestId u32 + StreamConfig 14B: w u32,h u32,bitrate u32,fps u16), response=8, reject=9, error=10, clipboardSyncRequest=11, clipboardSyncUpdate=12, clipboardSyncError=13.
### Heartbeat/session
Host ping every 2s; timeout at 3× interval → disconnect. Cursor: CursorUpdate [x f32][y f32][type u8].
### Signaling (SignalingClient.swift, ntfy.sh)
Topic = "erd3-" + HKDF(pin).prefix(14) hex (33 chars; ntfy 404s >64). POST body = base64(AES-GCM(payload, key=HKDF(pin, info="erd/payload-key"))). Poll GET /json?poll=1&since=10m, 1s interval, 10s deadline. Candidates: {role, localIP, localPort, publicIP, publicPort} JSON.

## Decisions ledger (adopted defaults)
- Rust workspace: erd-proto / erd-net / erd-decode / erd-render / erd-app + Tauri shell
- TLS-PSK via openssl crate (vendored), TLS1.2 PSK suites; rustls external-PSK unsupported (verify: research lane inconclusive due to provider outage — reopen if needed)
- FFmpeg/libav HEVC AVCC extradata direct; d3d11va accel with software fallback; staging-copy frame to wgpu first
- PIN-manual connect first (no mDNS client); HEVC only; app is personal-use (FFmpeg LGPL dynamic link noted)
- Conformance oracle: Swift TestCLI/E2E in tart VM
