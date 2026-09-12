# Remote desktop performance review

Date: 2026-09-05. Scope: active Rust workspace and shipped Tauri UI. Revision:
all-fixes-20260905 integrated fixes; supersedes the earlier same-day counts.

## Result and evidence boundaries

The review covers startup, encrypted transport, assembly, capture/encode on all
three hosts, client decode/presentation, input, audio, agent automation, and
reported performance statistics. Every finding in the original review's
remaining-findings list is now implemented and locally verified. Statements in
the earlier revision that these mechanisms remained open described the
pre-integration source and no longer hold. Current-source checks confirm both
platform `update_bitrate` methods now return and propagate typed errors, and
`native_pipeline::Workers`/`Handoff` has replaced the two-slot capture FIFOs.

The final macOS workspace build passed, and the resumed whole-workspace suite
passed 241 tests. Native host suites passed on Windows and Linux. The isolated
Windows session produced real decoded frames and passed screenshot, input-release
and disconnect API checks. Linux desktop audio acquisition now passes actual
output-monitor capture, with command-selection and lifecycle regressions. macOS
live ScreenCaptureKit capture with granted permission remains unverified. Scripted Media Foundation
transform fixtures are distinct from actual driver behavior.
Detailed inventories remain in `.omo/perf-review-20260905/` and
`.omo/all-fixes-20260905/`. The workspace's uncommitted changes were preserved.
No repository commit was created. This report includes recovered evidence and
the resumed Linux audio fix, native API checks and final regression results.

## Verified corrections from the original review

| Area | Defect and trigger | Correction | Regression evidence |
| --- | --- | --- | --- |
| Frame assembly | Lost headers permanently occupied the 16 orphan IDs, denying subsequent reordered frames. | Expire orphan chunks after one second from their first arrival, on ingress; later chunks do not extend lifetime. | Four failing assertions before the fix, eight passing public-assembler tests afterward. `fix-orphan-expiry.md`. |
| CLI decode queue | A blocking 1024-frame FIFO could retain stale encoded data and deadlock its producer during shutdown. | Four-frame nonblocking queue; overflow discards the pending reference chain and waits for a keyframe, requesting recovery once. Stop wakes consumers and rejects subsequent pushes. | Three regressions failed before correction; four CLI tests pass. `fix-cli-pipeline.md`. |
| CLI screenshot storage | Every decoded NV12 frame was copied even without an HTTP/MCP screenshot consumer. | Gate storage before locking/allocation on enabled consumers. | Disabled storage remains empty; enabled storage preserves exact pixels. Same CLI regression report. |
| CLI statistics | Session-end statistics were logged and written twice. | One final flush. | Baseline live logs contain two summaries; both post-fix host runs contain one. |
| Agent HTTP scheduling | Screenshot conversion/compression and synchronous input writes blocked the current-thread Tokio executor. | Move complete backend/encoding jobs into `spawn_blocking`; preserve transaction ordering with owned input guards. | Four blocked-backend HTTP cases fail before the fix and pass afterward; concurrent batch/reset ordering covered. `fix-agent-http.md`. |
| Host teardown | Disconnect before UDP discovery could stop a producer while retaining its full, undrained media receiver; protocol errors bypassed cleanup. | One result/cleanup epilogue: release receiver, cancel/join UDP sender, then stop media. | Real encrypted-loopback tests establish full queues and malformed authenticated control; producer/sender exits verified. `fix-host-lifecycle.md`. |
| Host input logging | INFO diagnostics ran synchronously for every injected input. | TRACE metadata, same injection behavior. | Numeric event dispatch and actual tracing level tested, without pinning prose. |
| Pointer motion | Throttle discarded the final motion within 16 ms, including drag endpoints. | Retain latest motion for one animation-frame flush; flush before button/context releases. | Explicit fake RAF/clock regression, no sleeps. `fix-ui-flow.md`. |
| Presentation lifecycle | A late raw-frame IPC response could draw and schedule after disconnect/reconnect. | Generation-owned polling; invalidate and cancel before awaiting backend teardown. | Deferred real-script IPC tests reject stale draws/counters/RAF and preserve one active chain. |
| Renderer capability | WebGL1 fallback could not execute GLSL 300 ES and WebGL2 texture formats. | Require WebGL2; surface failure and count only submitted draws. | Seven new UI regressions fail before edits; all 17 UI/overlay tests pass. Actual WebGL2 shader link and white NV12 pixel readback also pass. |
| Client TCP scheduling | Whole-frame TLS reads held the input/stop mutex while a peer withheld bytes. | Preserve one SSL/framing owner; perform one nonblocking read step under the mutex and wait on readiness outside it. | Real authenticated partial-frame fixture verifies input delivery and stop before peer release. `fix-client-transport.md`. |
| Client UDP cancellation | Dropping stored socket ownership did not wake a receive retaining its own socket. | Socket-generation watch cancellation wakes registered receives. | Actual receive future is observed Pending before public disconnect; worker returns NotReady and joins. |
| TCP event retention | Tauri retained unconsumed Ping/clipboard events without a bound. | Coalesce Ping and latest clipboard into bounded slots, retaining terminal error. | 128 real TLS clipboard/Ping/Pong exchanges with a gated consumer; bounded retention and latest text verified. This does not add a Tauri event consumer. |
| Tauri teardown | Media worker used a different stop token; synchronous joins blocked command dispatch. | Shared token, serialized connect/disconnect and transport cancellation before joins in a blocking job. | Shared Arc identity and channel-gated executor responsiveness; transport wake checked separately. `fix-tauri-backend.md`. |
| Tauri screenshots | Display poll consumed the agent snapshot; the 16-byte IPC header was interpreted as pixels; encoding blocked async dispatch. | Retain immutable Arc-owned latest frame separately from display-pending status; encode the pixel slice in a blocking job outside the mailbox lock. | Exact decoded PNG pixels, retained dimensions after poll, latest-wins, no duplicate poll and gated encoding responsiveness. Ten backend tests pass. |
| macOS queues | Callback and encoded-output channels retained unbounded events. | Three-event callback channel with nonblocking raw drops; three-event ordered encoded output without reference-frame drops. | Saturation, receiver-drop wakeup, encoder stop/join and safe-Rust Miri seam checks. |
| HTTP framing | A single read lost fragmented headers/bodies. | Accumulate Content-Length requests up to 65,536 bytes; reject unsupported/invalid framing. | Deterministic fragments, exact-limit/malformed cases, real-socket 413; original split-write tests restored. |

Transport evidence is qualified: event retention has an original-code behavioral
RED. UDP/read-step tests first failed on missing APIs and later under behavioral
mutation; combined input/stop is additional GREEN coverage, not an independently
captured original-code RED. No universal input-latency bound is claimed.

## Integrated corrections from all-fixes-20260905

| Area | Defect | Correction | Evidence |
| --- | --- | --- | --- |
| Host admission | TLS/pre-auth inactivity and capture startup could monopolize the serial accept path; PBKDF2 ran per accept. | One absolute startup budget from TCP accept across TLS and pre-auth; consent cannot outlive the pairing window; readiness-driven nonblocking SSL; pre-auth no longer drains unrelated UDP; bootstrap KDF cached per unchanged window with lockout/expiry/revocation. | Idle-preauth and silent-TLS RED exit 101; GREEN 3 admission, 3 boundary and 3 KDF-policy tests plus 9 TLS tests. `host-admission.md`. |
| TCP framing | Per-frame `drain` compacted the buffer suffix on every parsed frame in a coalesced burst. | Offset-based parse with at most one compaction per `push`; counters are cfg(test)-only, nothing ships. | RED 1,024 compactions and 33,528,832 bytes shifted; GREEN 1 and 7 with a partial tail, 0 and 0 without; byte/order/malformed semantics exact. `framing.md`. |
| UDP cipher | Each seal allocated an intermediate AEAD buffer on top of header and output. | `seal_into` appends nonce/ciphertext/tag into caller capacity; zero allocation with spare capacity; counter exhaustion does not wrap. | Debug allocator calls: seal 2 to 1, seal_datagram 4 to 2, seal_into 0; 18 tests GREEN plus Miri strict runs; exact wire bytes pinned. `udp-cipher.md`. |
| Agent HTTP bounds | Screenshot/input jobs could block the executor; connections and idle reads were unbounded. | 5-second request/read and write budgets; caps of 32 connections, 4 blocking jobs, 1 screenshot compression; 408/429/409/500 semantics; stop closes owned idle connections. | 20 server tests GREEN including shutdown; a stalled backend yields a timed-out result, never a false join. `http.md`. |
| Client runtime | UDP receive allocated a fresh buffer per attempt; worker panic was discarded. | One lazily allocated 65,536-byte buffer behind a mutex, decrypt of the actual prefix only; typed `SessionError::TcpRuntimePanicked`. | RED observed 3 allocations vs 1; GREEN 5 unit and 5 integration tests over real TLS/UDP. `client-runtime.md`. |
| Shared screenshot | CLI/agent snapshots deep-copied the retained NV12 frame. | `Arc<Vec<u8>>` shared between frame holder and HTTP/MCP encoders; a later frame replaces only the pointer. | RED pointer-equality failure; GREEN 37 library, 11 CLI and 3 agent-control E2E tests, exit 0. `screenshot-production.md`. |
| Decoder NAL | Strict validation plus public conversion parsed and allocated twice per decode. | Single-parse `prepare_annex_b` reserved to input length; public `to_annex_b` fallback unchanged. | Real 16-frame decode: 97 to 81 allocations, 4 to 2 reallocations, 36,234 to 34,162 bytes; pixel SHA-256 parity with the independent FFmpeg reference; Miri under both borrows. `client-production.md`. |
| Native session lifecycle | Two-slot raw FIFOs dropped newest under load; partial spawns leaked workers; keepalive cloned full NV12. | Shared mutex/Condvar handoff keeps the latest raw frame plus coalesced controls; `Workers<T>` gates startup and joins every worker; keepalive shares one Arc allocation; Windows outputs carry capture/start/completion identity. | RED stale raw `[2]` vs `[4]`; 15 harness and 15 cargo-seam tests GREEN. `native-session.md`. |
| Linux native | Wayland/audio waits ignored stop; recorder EOF retried forever; bitrate change was a no-op. | Cancellable bounded discovery and readiness-slice reads; EOF is terminal with a one-second reap; `open_cancellable` bounds the pactl query; bitrate reopen returns drained packets, preserves PTS and forces IDR. | Real-code harness compiles the production files with real libx264 and Unix sockets; 13 final tests, audio follow-up RED/GREEN. `linux-native.md`. |
| Windows geometry | Startup acquired pixels merely for dimensions and mis-sized rotated outputs. | Metadata-only probe reads DXGI_OUTPUT_DESC and swaps dimensions for ROTATE90/270; rejects unrepresentable sizes. | RED acquisition count 1 vs 0; rotation RED (1080,1920) vs (1920,1080); 4 portable tests GREEN; cross-target metadata check exit 0. `windows-geometry.md`. |
| Windows encoder | MF_E_NOTACCEPTING discarded sample B; outputs guessed FIFO identity; bitrate SetValue was ignored; NAL payloads were copied. | Rejection drains output and retries the identical sample; timestamp-keyed identity errors on unknown/duplicate; bitrate applies ICodecAPI or errors; borrowed NAL spans remove payload copies. | RED B discarded `[0]` vs `[0,1]`; bitrate RED returns Ok where typed errors are required; 9 portable tests plus cross-target check GREEN. `windows-encoder.md`. |
| macOS session | Pending native startup blocked stop; full output blocked join; synthetic fallback masked errors. | Handle returns after worker spawn; cancellation-aware owner joins with full output and retained receiver; startup errors become `MediaEvent::Error`; unconditional fallback removed. | RED two join timeouts; GREEN 5 integration, 4 shipped and 1 overload tests. `mac-session.md`. |
| macOS capture stop | Explicit stop plus Drop submitted two native stop requests; a timed-out stop dropped retained resources. | Stop disarms Drop; the copied completion retains stream/sink/queue until released. | RED 2 vs 1 in both cases; GREEN; 8 capture tests pass warning-free. `macos-native.md`. |
| macOS input | Every event constructed a fresh CGEventSource, 6,656 per measured workload. | One lazily created HIDSystemState source per injector; Send/Sync rejected at compile time. | RED 6,656 vs 1 at an equal 7,680-input workload; 5 native tests plus ASan. `mac-input-measurement.md`. |
| Tauri mailbox | `poll_frame_raw` consumed no bounded events; terminal errors and clipboard updates were never applied, so a dead session could look live. | Async command consumes the real bounded mailbox: latest clipboard coalescing, terminal/closed rejection and visible clipboard-failure reporting, at most four nonblocking reads per poll; blocking work in `spawn_blocking` with the lifecycle lock through completion, so a previous generation cannot overwrite a new session; disconnect joins workers before returning typed errors. No HTML change; the shipped script's existing `reportFrameError` catch already covered teardown. | RED three exact failures (empty clipboard vs `["clipboard-127"]`, closed-runtime rejection, lost stop error); GREEN 15 backend tests warning-free and 12 shipped-script UI tests. `tauri-events.md`. |

## Exact integrated metrics, with honest bounds

- KDF: same PIN and window, 8 accepts: 8 PBKDF2 derivations in 349.514 ms before,
  1 derivation in 84.341 ms after. An operation proof from one debug observation,
  not a throughput claim. 600,000 rounds and known-vector coverage are retained.
- Framing: counts are deterministic operations, not wall clock; byte-for-byte
  output, ordering and malformed-length behavior are unchanged.
- Screenshot: removes exactly one 3,110,400-byte deep copy per 1080p NV12
  snapshot. No screenshot encoding-latency speedup is claimed.
- Decoder: no elapsed-time threshold and no production latency or FPS improvement
  is claimed; the saving is measured in allocation operations on the real decode
  path.
- Mac input: cold single-run timings favored the baseline (51,936 vs 58,003 us),
  so no end-to-end speedup is claimed; the justification is 6,655 redundant
  native source constructions removed at unchanged event semantics.
- macOS frame-cost probe (equal workload, warm microbenchmark): AVFrame
  construction is 0.209 us against 121.8 to 133.4 us per copied frame;
  make-writable reuse is about 2x slower (263 us) with three frames retained.
  AVFrame reuse and the callback Arc replacement are measured and rejected as
  changes; a plain cached frame would violate writable ownership if FFmpeg
  retains references, and an Arc is not an equal-semantics replacement for the
  first copy out of a locked CVPixelBuffer.
- All elapsed values here and in the prior smoke checks are single observations,
  not controlled speedup estimates.

## Latest integrated verification status

- Final macOS workspace suite: 241 tests, 0 failed, 0 ignored, exit 0, no compiler
  warnings (`resume-workspace-tests.log`), including the final CLI shutdown fix
  (12 CLI tests). The earlier 240-test run preceded that last regression.
- Final macOS workspace build: exit 0, no warnings, 25.46 seconds
  (`final-build.log`). This resume reused that executable without another build.
- Shipped UI runners: 23 Bun tests and 22 Node tests passed
  (`final-ui-bun-tests.log`, `final-ui-node-tests.log`); separate suites.
- Native Windows host: 50 tests passed, including actual software Media Foundation
  encode/flush and capture identity (`native-windows-tests-final.log`).
- Native Linux host: 58 tests passed with `--test-threads=1`; HostServer fixtures
  share actual uinput device resources (`native-linux-tests-serial.log`). Two
  existing unused input-scaling warnings remain. Actual compositor capture
  returned three 3840x1600 frames (`native-linux-captest.log`).

- Host integrated suite: `cargo test --manifest-path clients/rust/Cargo.toml
  -p maho-host --lib` passed 59 tests, 0 failed, warning-free
  (`host-integrated-tests.log`). It covers admission/KDF, the native pipeline,
  macOS session and capture stop, input injection, VideoToolbox encoding and
  Windows input logic, and shows 1 PBKDF2 derivation for 8 accepts in run output.
- Screenshot share: 37 library, 11 CLI and 3 agent-control integration tests
  passed, exit 0 (`screenshot-share-green.log`).
- Lane GREEN totals: framing 48 maho-proto and 24 maho-net; UDP cipher 18 native
  plus Miri strict 17 and the stale-boundary test; HTTP 20; client runtime 5
  unit and 5 integration; client production 5 unit and 3 integration plus Miri
  under both borrows; native session 15 harness, 15 cargo-seam and 33 extracted
  Linux tests with a Windows region metadata check; Linux harness 13 plus the
  audio startup follow-up; Windows geometry 4 portable plus cross-check;
  Windows encoder 9 portable plus cross-check and 3 bitrate; macOS session 5,
  4 and 1; macOS input 5 plus ASan 5; macOS capture final 8 warning-free and
  VideoToolbox encoder 7 after a 2-failure RED; Tauri 15 backend tests warning
  free after a 3-failure RED plus 12 shipped-script UI tests (`tauri-final.log`,
  `tauri-ui-green.log`, `tauri-mailbox-red.log`).
- Prior-session evidence is retained: workspace 165 tests and Node 18/18, the
  final build exit 0 with no warnings in 12.47 seconds, the real-host CLI smokes
  and agent API checks in the table below, and the single gate reviewer APPROVE
  with its stated scope. `.omo/perf-review-20260905/` holds those artifacts.

## Native evidence boundaries

- **Windows:** native tests and an isolated interactive Session 1 HostServer
  passed. The final client decoded 124 H.264 frames. Health, info, PNG, release-all
  and disconnect returned HTTP 200. The 943,598-byte PNG and negotiated dimensions
  were 3840x1600. Client exit and scheduled task result were 0; the host logged
  `NATIVE_SESSION_CLEAN_EXIT`. Evidence: `resume-windows-api.json`,
  `resume-windows-client.log`, `resume-windows-host-final.log`,
  `resume-windows-stats.json`. This proves actual DXGI/MF streaming, not every
  driver fault or live bitrate transition.
- **Linux:** native host tests, actual compositor capture and a 573-frame HEVC
  session passed. Real capture exposed a missing `pw-record` stdout argument
  and then selection of an unavailable default input. Default capture now sets
  `stream.capture.sink=true`; an empty Pulse sink result fails before launching
  a recorder. Six command-selection and seven existing audio lifecycle tests
  pass after two behavioral RED failures. The real production API captured
  10,112 finite samples (5,056 stereo frames, 40,448 bytes) at 48 kHz, with two
  verified output-monitor graph links, then cancelled and reaped its recorder.
  The samples were silence; audible playback is not claimed. Evidence is in
  `resume-linux-audio.md` and its RED/GREEN/capture/cleanup logs. The final
  isolated host was rebuilt with this change and streamed 625 actual NVENC HEVC
  frames. Health, info, PNG, release-all and disconnect returned HTTP 200; the
  359,705-byte PNG matched screen/info at 3840x1600. Client and host exited 0,
  with no recorder or validation listener left running. Evidence:
  `resume-linux-api.json`, `resume-linux-client.log`, `resume-linux-host.log`,
  `resume-linux-stats.json`, `resume-cleanup.md`. GPU-specific forced-IDR and
  live bitrate transitions are not inferred from scripted/software tests.
- **macOS live capture: not exercised.** Tests never request Screen Recording
  permission, start a stream or post input. No real permission-granted capture
  or full VideoToolbox encode evidence exists.
- The documented late-callback thread transfer is a native FFI contract
  assumption, not a proven owner-thread-affinity guarantee.

## Original findings, final dispositions

| Original finding | Integrated disposition |
| --- | --- |
| P1: no complete startup admission deadline | Implemented: bounded handshake/pre-auth lifetime with lifecycle regressions. |
| P1: Linux keyframe and bitrate controls did not reach the encoder | Implemented: forced-IDR options, bitrate reopen with drained packets; Windows SetValue or typed error. |
| P1: Linux blocked capture/audio outlived stop; unjoined native workers | Implemented: cancellable APIs, terminal EOF, bounded reap, `Workers<T>` joins every worker. |
| P1: Windows geometry required a captured frame | Implemented: metadata-only probe with rotation handling. |
| P1: Media Foundation rejection dropped the current input | Implemented: drain-and-retry with a timestamp-keyed identity map. |
| P2: raw overload retained old captures in two-slot FIFOs | Implemented: latest-raw handoff; dependent compressed frames are not arbitrarily dropped. |
| P2: Linux recorder EOF retried forever; stale PCM | Implemented: EOF is terminal; the audio slot drops stale blocks older than the freshness bound. |

## Remaining limits

- TCP std mutex fairness is not guaranteed under continuous inbound traffic.
  Direct `receive_tcp_event` remains blocking; the corrected production path is
  `spawn_tcp_runtime`.
- A truly stalled synchronous HTTP backend cannot be forcibly cancelled; it
  produces a timed-out server result, not a false successful join.
- Queue limits count application items, not bytes, native codec buffers or frame
  age. macOS overload may drop raw audio; encoded frames remain ordered.
- Linux mean bitrate is configured at codec open; strict VBV peak-rate
  enforcement is unchanged (the x264 bufsize notice is retained). An
  uninterruptible recorder exit is reported after the bounded reap deadline;
  kernel-level reap cannot be promised.
- Automatic ABR remains unwired. Manual bitrate control now reaches both native
  encoders; that is a control path fix, not measured ABR oscillation.
- Audio playback, automatic clipboard, STUN and signaling integration remain
  dormant with absent production callers.
- CLI and Tauri statistics measure local decode duration only. They exclude host
  capture/encode, transport, packing, IPC transfer and presentation; host and
  client clocks cannot simply be subtracted. The UI labels remain Decode
  p50/p99. No glass-to-glass latency is established anywhere.
- Measured-and-rejected candidates: AVFrame reuse, callback Arc replacement,
  per-frame statistics clone removal (the record path allocates nothing),
  RGB/compression/base64 elimination (outputs are required by the current API),
  a zero-allocation iterator rewrite, direct owned-packet fill, and eliminating
  the compact owned NV12 planes (would need a new lifetime/rendering contract).
- Native fault-path coverage does not establish every GPU driver behavior.
  Agent-level rather than GUI QA was the explicit requirement, so Tauri
  verification is real-script level, not desktop client manipulation.

## Rejected or dormant claims

- The client has GPU presentation: the shipped `tauri-shell/ui/index.html` uses
  WebGL2 and reuses textures for same-size frames. Absence of native wgpu is not
  a missing renderer. The nested `src-tauri/ui` copy is not the shipped asset.
- Software-default decoding is a configuration fact, not proof that enabling
  hardware decoding improves end-to-end performance. Readback still exists.
- The macOS LAN-bitrate range warning was false: clap already enforces
  `value_parser!(u32).range(50..=150)` in `maho-host/src/main.rs`.
- MCP's sequential request loop is separate from the HTTP runtime; offloading
  its encoder alone would not provide concurrent stdio handling.
- Stopping a UDP consumer does not reliably backpressure a UDP sender; overload
  tests must gate the actual queue consumer.

## Executed real-surface checks (prior session)

| Surface | Result | Evidence |
| --- | --- | --- |
| Linux CLI, existing paired host `100.91.254.71` | 10 decoded HEVC frames; exit 0; one summary. First decoded frame 340.742 ms after CLI initialization. | `post-fix-linux.log` |
| Windows CLI, explicit existing pairing on `100.126.171.58` | 10 decoded H.264 frames; exit 0; one summary. First decoded frame 330.995 ms after CLI initialization. | `post-fix-windows.log` |
| Agent HTTP, real Windows video session | PNG HTTP 200, valid 3840x1600 PNG; concurrent health/info HTTP 200 at about 0.6 ms while PNG took 2406 ms. | `post-fix-agent-http.json` |
| Shipped WebGL2 renderer in Bun WebView | Shader linked; `renderNv12(4,4,Y=235,UV=128)` submitted; pixel readback [255,255,255,255], GL error 0. | `webgl-pixels.json` |
| Final agent-only API, both real hosts | HTTP 200 and 3840x1600 PNG screenshots; input dispatched; Shift held through disconnect returned `released_inputs: 2`; 104 Windows and 1353 Linux decoded frames, exit 0, one summary each. | `verified-agent-api.json`, `verified-agent-{windows,linux}.log` |

Timing values above are single smoke observations, not controlled speedup
estimates. Baseline startup was 465.593 ms Linux / 556.946 ms Windows; old
approximately four-second Windows notes are historical. Decoded-frame counts and
protocol success do not establish glass-to-glass latency or native backend branch
coverage. The immediate Windows snapshot is not proof that the just-dispatched
Win+R was already presented.

Commands:

```sh
cargo run --manifest-path clients/rust/Cargo.toml -p maho-app --bin maho-client -- --host 100.91.254.71 --frames 10 --timeout-secs 30
cargo run --manifest-path clients/rust/Cargo.toml -p maho-app --bin maho-client -- --host 100.126.171.58 --pairing-id EF0460B1-FC0B-4CAE-B9C2-46727C8F26A6 --frames 10 --timeout-secs 30
node --test clients/rust/tauri-shell/ui/performance.test.mjs clients/rust/tauri-shell/ui/session-overlay.test.mjs
```

## Preservation notes

Both canonical and stale UI copies already had trailing whitespace; those lines
and concurrent visual redesign files were left intact. The resumed verification
reused the final build, ran the final workspace regressions and exercised isolated
native hosts through CLI/API. Every number above cites a saved log under
`.omo/all-fixes-20260905/` or `.omo/perf-review-20260905/`. Darwin tests do not
compile Windows/Linux cfg-gated code; native testbed results are separate. No
repository commit or deployed daemon replacement was performed.
