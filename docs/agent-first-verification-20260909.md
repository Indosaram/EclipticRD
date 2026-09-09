# Agent-first implementation and verification - 2026-09-09

## Result

Agent onboarding, MCP protocol/lifecycle fixes, receiver trace integration, and
native raw-frame reselection are implemented and exercised. **This is not an
all-green performance acceptance report.** Linux still has emitted-but-unseen
packets in the active trace; Windows still has substantial fresh-frame work
residence. Strict workspace Clippy still fails on pre-existing diagnostics
described below. A historical post-review session-test failure is also recorded
below, now resolved by a deterministic teardown synchronization fix and verified
in the lead's single final 495-pass workspace run.

All compilation, Cargo checks/tests, JavaScript tests, and cross-compilation ran
on the Omarchy Linux builder. Already-built binaries ran on physical macOS,
Linux, and Windows machines. No emulator was used. Temporary hosts used separate
ports, pairing stores, and process names; existing host services were not
replaced. No public binary release or permanent deployment was performed.

## Implemented behavior

- MCP stdout contains newline-delimited JSON-RPC; diagnostics use stderr.
  Agent modes no longer inherit the smoke client's default 30-second lifetime.
- MCP validates envelopes and typed tool arguments, preserves errors, releases
  input after partial dispatch failure, and joins owned workers on EOF/stop.
  Windows synchronous inherited handles are isolated in cancellable relay
  processes with acknowledged output writes.
- HTTP worker completion now terminates the owned CLI lifecycle, including
  listener bind failure. The final review found this regression; an occupied
  port reproduced it before the fix.
- The real CLI frame queue shares the session's receiver trace. Opt-in traces
  record socket sizing, receipt/authentication, finalized gaps, assembly
  outcomes, and queue recovery. Host traces record send attempts/results and
  Windows capture/publication/selection/encoding observations.
- The native encoder reselects the latest raw frame after blocked bitrate work
  and drained output emission. Compressed dependent frames are not discarded
  to manufacture freshness. Static repeats retain original content identity
  while publication residence is measured separately.
- README onboarding, `docs/agent-setup.md`, and the YAML-frontmatter skill at
  `skills/eclipticrd-remote-control/SKILL.md` describe automatic pairing
  persistence, MCP registration, skill installation/context injection, exact
  response formats, input limits, and cleanup.

## Automated verification

| Check | Result |
| --- | --- |
| Initial fresh non-iOS Rust workspace suite | 493 passed, including doctests; one existing installed-Tailscale observation excluded by its own ignore attribute |
| Explicit installed-Tailscale observation | 1 passed; observed list was empty, not proof of device discovery |
| iOS library tests compiled on Linux | 15 passed; not native iPhone/VideoToolbox QA |
| erd-app library without default features | 100 passed |
| Desktop JavaScript | 35 Bun tests, 4 Bun icon tests, 32 actual-Node tests passed |
| Mobile JavaScript | 53 Bun tests passed |
| QA driver syntax | Passed on Omarchy |
| Workspace development build | Passed |
| Linux release CLI/host/finite-host example | Passed, including post-review rebuild |
| macOS ARM64 release CLI cross-build | Passed, including post-review rebuild |
| Windows host/finite-host example cross-build | Passed |
| Windows standalone production MCP strict Clippy and cross-build | Passed after dispatcher lint correction |
| Final native Windows MCP fixture | Protocol/EOF, pending read cancellation, pending write cancellation passed; exit 0, zero owned relay processes |
| New HTTP occupied-port regression | RED: 1 failed, client did not terminate; GREEN: all 5 CLI lifecycle cases passed |
| Changed Rust file formatting | 28 files matched rustfmt |
| Final full workspace suite run | 495 passed (top-level tests/doctests, raw sum includes 6 nested audio subprocess results), 0 failed, 1 pre-existing ignored Tailscale observation; exit 0 |

The first JavaScript runner attempt incorrectly used Node for Bun-specific
imports and UMD named imports. Failures were retained, not counted as product
regressions. The corrected runners above passed.

### Failing gates retained

Strict `cargo clippy --all-targets --no-deps -- -D warnings` failed:

- `erd-app/src/pairing.rs`: one needless return and three `io_other_error`
  diagnostics.
- `erd-host/src/inject_linux.rs`: two unused items.
- `erd-host/src/encode_linux.rs`: range-loop, manual-contains, and range-pattern
  diagnostics.
- `erd-host/src/session.rs`: an unchanged redundant error-mapping closure.

These diagnostics are in pre-existing code; none was suppressed. Clippy stopping
there is not an exhaustive clean result for all later targets. Repository-wide
rustfmt also reports older formatting outside this change.

After the HTTP fix, an earlier full workspace suite run passed its five CLI
lifecycle tests, but the unchanged
`stalled_consumer_retains_latest_clipboard_and_terminal_error` test failed with
`retained 4 events` against its `<= 3` assertion (it had passed in the initial
fresh run). The fixture waited for peer send completion, but peer send completion
did not imply client runtime terminal publication; concurrent draining observed
multiple bounded snapshots as slots were refilled during shutdown.

This historical failure was resolved via a deterministic test-only fix: the mock
server now executes a causal transport teardown (TCP `Shutdown::Write` followed
by reading the client's reciprocal EOF shutdown before signaling done), and the
test calls `runtime.stop()` (`worker.join()`) before draining events so the worker
is fully terminated and `EventSlots` closed before consumption begins. In the lead's
single final full workspace run (`final-workspace.log`), all 495 top-level tests
and doctests passed with exit 0 (`FINAL_WORKSPACE_PASS`).

The language-server daemon was unreachable. Compiler/test evidence substitutes
for unavailable LSP diagnostics, not for a claim that LSP checks passed.

## Actual remote-control checks

The release macOS client completed MCP and HTTP sessions against separate
physical Linux and Windows hosts:

- PIN enrollment persisted a private store; HTTP reconnect used its saved ID.
- MCP initialization and all ten advertised tool names were observed.
- Malformed pointer input was rejected (`-32602` for MCP, HTTP 400).
- PNG screenshots matched negotiated 3840 x 1600 geometry.
- Pointer move and input release succeeded.
- MCP stdin EOF and HTTP disconnect each ended the client with exit 0.
- Enabled receiver traces contained real CLI `QueueAdmission` records.

Linux native cursor observation was `(1727, 719)` after the normalized
`(0.45, 0.45)` action on a 3840 x 1600 screen, consistent with integer coordinate
conversion. Native cursor coordinates, not screenshot differences alone, are
used for pointer-position evidence.

The final Windows HTTP run used the post-review release and passed screenshot,
input, release, and disconnect checks. Its native observer reported logical
desktop bounds of 3072 x 1280 and a cursor change from `(1536, 640)` to
`(1382, 575)`, consistent with the requested normalized position after Windows
DPI coordinate conversion. The observer and finite host both exited normally;
the native host exit code was 0.

An actual Codex process was given invocation-only MCP configuration and the
shipped skill. Its first attempt was denied by its tool approval policy.
With authorization scoped to the next invocation, all five calls completed:
screen info, screenshot, pointer move, screenshot, release. The agent consumed
the images and described the Hyprland safe-mode dialog and configuration warning.
It correctly did not claim that the two images alone proved pointer motion.
No model configuration file was modified.

Codex shutdown closed its connection abruptly after explicit input release,
producing an unexpected-EOF host receipt. This is separate from the clean EOF
tests performed by the owned MCP harness; registration success is not evidence
that every third-party client gracefully closes stdin.

## Bounded performance observations

Fresh host processes were used for each static/active trace. Both endpoints
retained their full trace within the 65,536-record bound: all four compared
endpoint traces reported zero overflow.

### Linux

The original NVENC environment changed during this work: `nvidia-smi` could not
communicate with the driver, and the desktop was in Hyprland safe mode. The
candidate used the available software encoder. It cannot support a matched
NVENC before/after improvement claim.

| 120-frame candidate run | Static | Active, 50 ms nudges |
| --- | ---: | ---: |
| Decoded frames | 120 | 120 |
| Final-window missing / expected sequences | 0 / 1559 | 13 / 1631 |
| Host encode p95 | 63.079 ms | 81.838 ms |
| Decode p95 | 6.554 ms | 6.601 ms |
| Host socket-send errors | 0 | 0 |
| Queue recoveries in full trace | 0 | 2 |
| Queue overflows / decode failures | 0 / 0 | 0 / 0 |

All 13 finalized missing positions had successful host socket sends and no
authenticated receiver arrival or late-arrival record. This separates them
from sender errors and late-after-grace accounting, but does not distinguish
network path loss from receiver socket loss. Actual receiver buffer observation
was 4 MiB with successful socket configuration. Local interface capture was
unavailable because noninteractive sudo required a password. No speculative
buffer/pacing patch was made, and historical 29.5% loss is **not declared fixed**.

### Windows

Static and active candidate runs each selected a 60-second duration using an
unreachable 100,000-frame goal. Their client exit 1 was the intentional
duration-selection result, not a successful frame-goal run. Both finite host
processes exited 0. The runs decoded 53 and 117 frames respectively.

| Host trace population | Samples | Content-age p95 | Publication-residence p95 |
| --- | ---: | ---: | ---: |
| Static, fresh content | 44 | 1298.947 ms | 489.813 ms |
| Static, repeated content | 10 | 514.167 ms | 37.631 ms |
| Active, fresh content | 74 | 447.846 ms | 417.311 ms |
| Active, repeated content | 46 | 510.927 ms | 86.745 ms |

The deterministic stall regression proves superseded raw work is not submitted
after blocked bitrate handling. The native trace proves content age and work
residence are distinct, but **does not prove the remaining Windows latency is
solved**. Fresh work still waited hundreds of milliseconds, and the static run
also recorded long conversion times. These are not directly comparable to
earlier aggregates from a different workload/environment.

## Evidence and remaining limits

Private command logs, protocol transcripts, images, native receipts, RED/GREEN
proofs, and the independent review are retained locally under
`.omo/agent-first-20260909/`, with the final matrix in `verification-final/`.
Pairing secrets and SDK/toolchain archives are not part of this source report.

Native desktop GUI session/audio playback, full physical-iPhone session QA,
and a complete native Windows FFmpeg-client session were not established by
this run. These limits do not negate the verified MCP/HTTP and native Windows
stdio behavior, but must not be described as completed platform QA.
