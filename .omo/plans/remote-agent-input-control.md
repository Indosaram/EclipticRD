# remote-agent-input-control - Work Plan

## TL;DR (For humans)
**What you'll get:** A complete, production-grade agent input control system for MahoRD allowing any AI agent (Claude, Cursor, OMO, Python scripts, or MCP clients) to programmatically operate all mouse and keyboard controls on remote Linux (Hyprland uinput) and Windows (SendInput) machines. It provides full mouse operations (move, click, middle click, double click, drag, scroll), full keyboard operations (key down/up, atomic press, hotkeys like "Ctrl+Shift+T", text typing), screen perception tools (resolution query and latest-frame screenshot capture), stuck-key fail-safes, and a dual-channel interface (local HTTP/WebSocket REST API on `127.0.0.1:19735` and native Model Context Protocol (MCP) stdio bridge).

**Why this approach:** 
1. **Zero-overhead wire protocol**: Extends `maho-proto` with Middle Click and Reset while preserving the fixed 21-byte packet format and zero-allocation Tokio pipeline.
2. **Shared core engine in `maho-app`**: Centralizes high-level action models, coordinate translation, key parsing, and state tracking so both the desktop GUI (`tauri-shell`) and headless CLI (`maho-client`) share 100% of the input logic.
3. **Universal agent accessibility**: Exposing both a local REST/WebSocket server and an MCP bridge guarantees that any agent framework can immediately interact without bespoke SDKs, while visual grounding (screenshots) enables multimodal computer-use agents to see what they click.

**What it will NOT do:**
- It will NOT alter host-side video encoding pipelines (NVENC, VAAPI, MF stay untouched).
- It will NOT bind agent control servers to public network interfaces (strictly localhost `127.0.0.1`).
- It will NOT run autonomous agent loop logic on the host (intelligence remains strictly on the client/operator side).

**Effort:** Large (13 todos across 4 waves)
**Risk:** Medium - Host-side uinput/SendInput coordinate normalization and multi-display edge cases require rigorous automated test verification.
**Decisions to sanity-check:** 
1. Defaulting agent server to localhost port `19735`.
2. Supporting both normalized `[0.0, 1.0]` and physical pixel `(x, y)` coordinate inputs.
3. Hybrid text typing (synthetic keydown/up for ASCII/shortcuts + clipboard paste for multiline/Unicode).

Your next move: review and approve the plan, then execute via `/ulw-execute`.

---

> TL;DR (machine): Large effort, medium risk. Implements agent input control across maho-proto, maho-host (Linux/Windows), maho-app (actions, key parsing, safety tracker), tauri-shell, and maho-client with local HTTP/WS server and MCP stdio bridge.

## Scope
### Must have
- `maho-proto`: Add `MiddleMouseDown (11)`, `MiddleMouseUp (12)`, `Reset (13)`, and `RelativeMove (14)` to `InputEventType` with full `WireCodec` support.
- `maho-host` Linux: `LinuxInputInjector` handling `BTN_MIDDLE` and emergency `Reset` (all-clear on pointer and keyboard virtual devices).
- `maho-host` Windows: `WindowsInputInjector` handling `MOUSEEVENTF_MIDDLEDOWN/UP` and emergency `Reset` (all-clear on modifier VKs and mouse buttons).
- `maho-app`: `AgentAction` enum, human-readable key name and hotkey parser ("Ctrl+Shift+T", "Super+Return", "Alt+Tab"), text typing synthesizer, coordinate normalizer, and `InputStateTracker` fail-safe with 5-second max hold timeout.
- Screen Perception: Expose remote display dimensions and latest decoded frame screenshot capture (base64 PNG/JPEG).
- Agent Interface: Embedded local HTTP/WebSocket server (`127.0.0.1:19735`) and MCP stdio bridge in `tauri-shell` and `maho-client`.
- Headless Automation: `maho-client --agent-server` and `maho-client --mcp` flags for headless VM driving.
- Automated Test Suite: Unit tests for protocol, parser, coordinate mapping, and end-to-end integration tests.

### Must NOT have (guardrails, anti-slop, scope boundaries)
- NO breaking changes to the 21-byte `InputEvent` wire format.
- NO binding the agent API server to non-localhost (0.0.0.0) without explicit user token configuration.
- NO host-side AI agent orchestration (host is strictly a remote display/input streaming daemon).
- NO external heavy web frameworks in Tauri/maho-app (use lightweight tokio-based HTTP/WS primitives or hyper/axum minimal).
- NO regressions in existing human user mouse/keyboard input latency or smoothness.

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: TDD for proto/parser/coordinate math; tests-after for host injector and server endpoints.
- Verification Commands:
  - `cargo test --manifest-path clients/rust/Cargo.toml -p maho-proto`
  - `cargo test --manifest-path clients/rust/Cargo.toml -p maho-app`
  - `cargo test --manifest-path clients/rust/Cargo.toml -p maho-host`
  - `cargo test --manifest-path clients/rust/Cargo.toml -p tauri-shell`
  - Headless E2E verification: `cargo run --manifest-path clients/rust/Cargo.toml -p maho-app --bin maho-client -- --help`

## Execution strategy
### Parallel execution waves
- **Wave 1 (Todos 1-3)**: Core Protocol & Host Injections (WireCodec extensions in `maho-proto`, Linux uinput injector in `maho-host`, Windows SendInput injector in `maho-host`).
- **Wave 2 (Todos 4-7)**: High-Level Agent Engine in `maho-app` (Action definitions, Key/Hotkey parser, Text typing synthesizer, Coordinate normalizer & Safety tracker).
- **Wave 3 (Todos 8-12)**: Agent Interface Surfaces & Perception (Screen info & Screenshot capture, Local HTTP/WS REST server, MCP stdio server, Tauri IPC integration, Headless `maho-client` flags).
- **Wave 4 (Todo 13)**: End-to-End Automated Integration Test Suite.

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 | None | 2, 3, 4 | None |
| 2 | 1 | 13 | 3 |
| 3 | 1 | 13 | 2 |
| 4 | 1 | 5, 6, 7 | None |
| 5 | 4 | 9, 10 | 6, 7 |
| 6 | 4 | 9, 10 | 5, 7 |
| 7 | 4 | 9, 10 | 5, 6 |
| 8 | None | 9, 10, 11 | 4, 5, 6, 7 |
| 9 | 5, 6, 7, 8 | 11, 12, 13 | 10 |
| 10 | 5, 6, 7, 8 | 12, 13 | 9 |
| 11 | 9, 10 | 13 | 12 |
| 12 | 9, 10 | 13 | 11 |
| 13 | 2, 3, 11, 12 | Final Verification | None |

## Todos
> Implementation + Test = ONE todo. Never separate.

- [ ] 1. Extend maho-proto wire protocol with MiddleMouseDown, MiddleMouseUp, Reset, and RelativeMove
  What to do / Must NOT do: Add `MiddleMouseDown = 11`, `MiddleMouseUp = 12`, `Reset = 13`, and `RelativeMove = 14` to `InputEventType` in `clients/rust/maho-proto/src/input.rs`. Update `InputEventType::ALL` and `TryFrom<u8>`. Verify fixed 21-byte wire layout remains unchanged. Note: in `clients/rust/maho-host/src/inject_macos.rs`, add matching arms/wildcard so exhaustive matching does not break compilation on macOS host targets. Add comprehensive unit tests in `maho-proto` verifying serialization, deserialization, round-trip equality, and error handling for invalid event types.
  Parallelization: Wave 1 | Blocked by: None | Blocks: 2, 3, 4
  Recommended task executor category: quick
  References: clients/rust/maho-proto/src/input.rs:11-115, clients/rust/maho-host/src/inject_macos.rs:95-125
  Acceptance criteria: `cargo test --manifest-path clients/rust/Cargo.toml -p maho-proto` passes with 100% green tests on new variants.
  QA scenarios:
    happy: Encode InputEvent with event_type MiddleMouseDown and decode back; assert equality. Evidence: .omo/evidence/task-1-proto-middle-click.txt
    failure: Decode byte slice with unknown event_type 99; assert CodecError::UnknownInputEventType(99). Evidence: .omo/evidence/task-1-proto-error.txt
  Commit: Y | feat(maho-proto): add middle click, reset, and relative move input event types

- [ ] 2. Implement Middle Click, Relative Move, and Emergency Reset injection in maho-host Linux injector
  What to do / Must NOT do: In `clients/rust/maho-host/src/inject_linux.rs`, handle `InputEventType::MiddleMouseDown` and `MiddleMouseUp` by emitting `KeyCode::BTN_MIDDLE` (value 1 and 0 respectively) to `self.pointer`. Handle `InputEventType::RelativeMove` by emitting relative motion deltas (`RelativeAxisCode::REL_X`, `REL_Y`). Handle `InputEventType::Reset` by emitting key-up events for all active mouse buttons (`BTN_LEFT`, `BTN_RIGHT`, `BTN_MIDDLE`) and releasing all pressed keyboard scancodes. Do NOT modify video capture or encoder pipelines.
  Parallelization: Wave 1 | Blocked by: 1 | Blocks: 13
  Recommended task executor category: unspecified-high
  References: clients/rust/maho-host/src/inject_linux.rs:40-160
  Acceptance criteria: `cargo test --manifest-path clients/rust/Cargo.toml -p maho-host` compiles cleanly; pure unit tests verify event translation to evdev keycodes.
  QA scenarios:
    happy: Inject MiddleMouseDown event; verify pointer device receives BTN_MIDDLE down. Evidence: .omo/evidence/task-2-linux-middle-click.txt
    failure: Inject Reset event with mock active keys; verify all buttons and keys emit 0 (up). Evidence: .omo/evidence/task-2-linux-reset.txt
  Commit: Y | feat(maho-host): support middle click and emergency reset in linux uinput injector

- [ ] 3. Implement Middle Click, Relative Move, and Emergency Reset injection in maho-host Windows injector
  What to do / Must NOT do: In `clients/rust/maho-host/src/inject_windows.rs` and `clients/rust/maho-host/src/windows_logic.rs`, map `InputEventType::MiddleMouseDown` to `MOUSEEVENTF_MIDDLEDOWN` and `MiddleMouseUp` to `MOUSEEVENTF_MIDDLEUP`. Handle `InputEventType::RelativeMove` by invoking `self.move_relative(dx, dy)`. Handle `InputEventType::Reset` by sending `KEYEVENTF_KEYUP` for all modifier VKs and mouse button up flags. Do NOT break existing multi-display virtual desktop coordinate mapping.
  Parallelization: Wave 1 | Blocked by: 1 | Blocks: 13
  Recommended task executor category: unspecified-high
  References: clients/rust/maho-host/src/inject_windows.rs:60-120, clients/rust/maho-host/src/windows_logic.rs:1-80
  Acceptance criteria: `cargo test --manifest-path clients/rust/Cargo.toml -p maho-host` passes on pure logic tests.
  QA scenarios:
    happy: Call inject() with MiddleMouseDown; verify SendInput receives MOUSEEVENTF_MIDDLEDOWN. Evidence: .omo/evidence/task-3-windows-middle-click.txt
    failure: Call inject() with Reset; verify modifier VKs emit KEYEVENTF_KEYUP. Evidence: .omo/evidence/task-3-windows-reset.txt
  Commit: Y | feat(maho-host): support middle click and emergency reset in windows injector

- [ ] 4. Build AgentAction data model and human-readable key/hotkey parser in maho-app
  What to do / Must NOT do: Create `clients/rust/maho-app/src/agent_input.rs`. Define `AgentAction` enum with variants: `MouseMove { x: f32, y: f32, normalized: bool }`, `MouseDown { button: MouseButton }`, `MouseUp { button: MouseButton }`, `Click { x: f32, y: f32, button: MouseButton, count: u32, normalized: bool }`, `Drag { start_x: f32, start_y: f32, end_x: f32, end_y: f32, button: MouseButton, steps: u32, duration_ms: u64, normalized: bool }`, `Scroll { dx: f32, dy: f32, x: Option<f32>, y: Option<f32>, normalized: bool }`, `KeyDown { key: String }`, `KeyUp { key: String }`, `KeyPress { key: String, hold_ms: u64 }`, `Hotkey { keys: Vec<String> }`, `TypeText { text: String, delay_ms: u64 }`, and `ReleaseAll`. Implement a robust case-insensitive parser mapping strings like "Enter", "Space", "Tab", "Escape", "Ctrl", "Alt", "Shift", "Super", "F1"-"F12", "a"-"z" to virtual keycodes. Implement hotkey string parsing (e.g. "Ctrl+Shift+T", "Super+Return").
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 5, 6, 7
  Recommended task executor category: unspecified-high
  References: clients/rust/maho-app/src/input.rs:1-110
  Acceptance criteria: `cargo test --manifest-path clients/rust/Cargo.toml -p maho-app` passes comprehensive parser unit tests.
  QA scenarios:
    happy: Parse "Ctrl+Alt+T" and "Super+Return"; assert exact modifier bitmask and target keycode. Evidence: .omo/evidence/task-4-key-parser-happy.txt
    failure: Parse invalid key name "UnknownKey123"; assert descriptive ParseError. Evidence: .omo/evidence/task-4-key-parser-error.txt
  Commit: Y | feat(maho-app): implement AgentAction model and human-readable key parser

- [ ] 5. Implement character-to-key synthesis and clipboard typing hybrid engine in maho-app
  What to do / Must NOT do: In `clients/rust/maho-app/src/agent_input.rs`, implement text typing synthesis: for ASCII printable characters, map each char to keycode + Shift modifier if uppercase/symbol, emitting keydown + keyup pairs with configurable inter-key delay. For complex multiline or Unicode strings, implement an automated clipboard-paste helper that sends `ClipboardSyncUpdate` followed by a synthetic `Ctrl+V` (or `Ctrl+Shift+V` for terminal) hotkey.
  Parallelization: Wave 2 | Blocked by: 4 | Blocks: 9, 10
  Recommended task executor category: unspecified-high
  References: clients/rust/maho-app/src/clipboard.rs, clients/rust/maho-app/src/input.rs
  Acceptance criteria: Unit tests verify typing string "echo hello" emits proper key event sequence; verify clipboard paste fallback triggers for CJK strings.
  QA scenarios:
    happy: Synthesize "curl -s http://localhost"; verify ordered sequence of KeyDown/KeyUp events. Evidence: .omo/evidence/task-5-typing-synthesis.txt
    failure: Synthesize empty string; verify zero events emitted and Ok result. Evidence: .omo/evidence/task-5-typing-empty.txt
  Commit: Y | feat(maho-app): implement text typing synthesis and clipboard paste fallback

- [ ] 6. Implement bidirectional coordinate normalizer and multi-display mapper in maho-app
  What to do / Must NOT do: In `clients/rust/maho-app/src/agent_input.rs`, implement coordinate translation functions supporting both normalized `[0.0, 1.0]` space and host physical pixel `(0..width, 0..height)` space. Correctly apply Y-axis inversion (`1.0 - y`) matching `maho-proto` wire convention, and clamp out-of-bounds coordinates safely to `[0.0, 1.0]`.
  Parallelization: Wave 2 | Blocked by: 4 | Blocks: 9, 10
  Recommended task executor category: quick
  References: clients/rust/maho-proto/src/input.rs:118-142, clients/rust/maho-app/src/input.rs:88-105
  Acceptance criteria: Unit tests verify round-trip conversion between (x, y) pixels and wire format across multiple resolutions (1920x1080, 3840x1600, 2560x1440).
  QA scenarios:
    happy: Convert (960, 540) on 1920x1080 display; assert normalized (0.5, 0.5). Evidence: .omo/evidence/task-6-coord-mapping.txt
    failure: Convert NaN or infinite coordinates; assert graceful fallback to clamped bounds (0.0, 0.0) without panic. Evidence: .omo/evidence/task-6-coord-nan.txt
  Commit: Y | feat(maho-app): implement robust coordinate normalizer for agent actions

- [ ] 7. Implement InputStateTracker fail-safe and emergency ReleaseAll in maho-app
  What to do / Must NOT do: In `clients/rust/maho-app/src/agent_input.rs`, create `InputStateTracker` maintaining thread-safe sets of currently pressed mouse buttons and keyboard keys. Implement:
    1) Automatic tracking of all KeyDown/MouseDown/Drag operations.
    2) `release_all()`: generates KeyUp and MouseUp events for every active item and sends wire `Reset` packet.
    3) Configurable hold timeout (default 5000ms): if an agent script leaves a key or mouse button pressed longer than timeout, automatically trigger auto-release.
    4) Automatic cleanup on session disconnect or drop.
  Parallelization: Wave 2 | Blocked by: 4 | Blocks: 9, 10
  Recommended task executor category: unspecified-high
  References: clients/rust/maho-app/src/session.rs:120-140
  Acceptance criteria: Unit tests verify that calling `press_down` followed by `release_all` cleanly clears all tracked states and emits required release packets.
  QA scenarios:
    happy: Press Ctrl and Left mouse button; call release_all(); verify tracker becomes empty and release events emitted. Evidence: .omo/evidence/task-7-tracker-release.txt
    failure: Simulate timeout expiration; verify auto-release triggers and logs warning. Evidence: .omo/evidence/task-7-tracker-timeout.txt
  Commit: Y | feat(maho-app): implement InputStateTracker with auto-release fail-safe

- [ ] 8. Implement Screen Perception Provider (Screen Info & Screenshot Capture)
  What to do / Must NOT do: In `clients/rust/maho-app/Cargo.toml`, add `image = { version = "0.25", default-features = false, features = ["png", "jpeg"] }`. In `clients/rust/maho-app/src/agent_input.rs` (or `media.rs`) and `tauri-shell/src-tauri/src/lib.rs`, expose:
    1) `get_screen_info()`: returns `{ width: u32, height: u32, scale: f32, connected_host: String }`.
    2) `capture_screenshot(format: "png" | "jpeg", max_dimension?: u32)`: reads the active decoded frame from `latest_raw_frame` (NV12), converts YUV to RGB, encodes to PNG or JPEG buffer, and returns base64 string.
    Do NOT lock the frame buffer longer than necessary; clone the slice and release lock immediately.
  Parallelization: Wave 3 | Blocked by: None | Blocks: 9, 10, 11
  Recommended task executor category: unspecified-high
  References: clients/rust/maho-app/Cargo.toml, clients/rust/tauri-shell/src-tauri/src/lib.rs:82-120, clients/rust/maho-decode/src/lib.rs
  Acceptance criteria: Unit tests verify NV12 to PNG/JPEG conversion produces valid image headers and correct dimensions.
  QA scenarios:
    happy: Pass mock 1920x1080 NV12 buffer; assert generated PNG has valid PNG signature [0x89, 0x50, 0x4E, 0x47]. Evidence: .omo/evidence/task-8-screenshot-valid.txt
    failure: Call capture_screenshot when session has no frames yet; return clean error "No active frame received yet". Evidence: .omo/evidence/task-8-screenshot-empty.txt
  Commit: Y | feat(maho-app): implement screen info query and decoded frame screenshot capture

- [ ] 9. Implement embedded local HTTP/WebSocket agent control server
  What to do / Must NOT do: In `clients/rust/maho-app` (or dedicated module `agent_server.rs`), implement a lightweight local HTTP/WebSocket server listening strictly on `127.0.0.1:19735`. Expose endpoints:
    - `POST /api/v1/input/action`: executes single `AgentAction` JSON payload.
    - `POST /api/v1/input/batch`: executes ordered array of actions with optional delays.
    - `POST /api/v1/input/reset`: triggers emergency `ReleaseAll`.
    - `GET /api/v1/screen/info`: returns screen dimensions and host status.
    - `GET /api/v1/screen/screenshot`: returns base64 PNG or binary image.
    - `WS /api/v1/stream`: bidirectional WebSocket for low-latency action dispatch.
    Bind ONLY to `127.0.0.1`. Support optional bearer token authentication if configured.
  Parallelization: Wave 3 | Blocked by: 5, 6, 7, 8 | Blocks: 11, 12, 13
  Recommended task executor category: unspecified-high
  References: clients/rust/maho-app/src/agent_input.rs, clients/rust/tauri-shell/src-tauri/src/lib.rs
  Acceptance criteria: Integration test spins up server on random port, hits `POST /api/v1/input/action` with `{"action": "click", "x": 100, "y": 200}` using `reqwest` or curl, and asserts 200 OK.
  QA scenarios:
    happy: Send POST /api/v1/input/action with valid click payload; assert 200 OK and action dispatched. Evidence: .omo/evidence/task-9-http-action.txt
    failure: Send malformed JSON; assert 400 Bad Request with descriptive error. Evidence: .omo/evidence/task-9-http-error.txt
  Commit: Y | feat(maho-app): implement embedded local HTTP and WebSocket agent control server

- [ ] 10. Implement native Model Context Protocol (MCP) stdio server bridge
  What to do / Must NOT do: Implement an MCP tool provider (JSON-RPC 2.0 over stdio) conforming to the Model Context Protocol specification. Expose tools:
    - `remote_mouse_click`: x, y, button ("left"|"right"|"middle"), count (1|2|3), normalized (bool).
    - `remote_mouse_move`: x, y, normalized (bool).
    - `remote_mouse_drag`: start_x, start_y, end_x, end_y, button, steps, normalized.
    - `remote_mouse_scroll`: dx, dy, x?, y?.
    - `remote_key_press`: key (string, e.g. "Enter", "Escape", "F5").
    - `remote_hotkey`: keys (array of strings, e.g. ["Control", "Alt", "t"]).
    - `remote_type_text`: text (string), paste_mode (bool).
    - `remote_release_all`: emergency reset.
    - `remote_get_screen_info`: query dimensions and host OS.
    - `remote_take_screenshot`: captures remote desktop image as base64 PNG.
  Parallelization: Wave 3 | Blocked by: 5, 6, 7, 8 | Blocks: 12, 13
  Recommended task executor category: unspecified-high
  References: clients/rust/maho-app/src/agent_input.rs
  Acceptance criteria: Unit tests run MCP JSON-RPC `tools/list` and `tools/call` requests over mock stdio pipes and assert compliant MCP tool responses.
  QA scenarios:
    happy: Send MCP tools/call for remote_mouse_click; verify tool returns success content. Evidence: .omo/evidence/task-10-mcp-call.txt
    failure: Send MCP tools/call with invalid parameter; verify JSON-RPC error response returned. Evidence: .omo/evidence/task-10-mcp-error.txt
  Commit: Y | feat(maho-app): implement native MCP stdio tool server for AI agents

- [x] 11. Integrate Agent Control Server and IPC commands into tauri-shell
  What to do / Must NOT do: In `clients/rust/tauri-shell/src-tauri/src/lib.rs`:
    1) Add Tauri commands: `agent_execute_action`, `agent_get_screen_info`, `agent_capture_screen`, and `agent_release_all`.
    2) Initialize and manage the local agent HTTP/WebSocket server in `AppState` when connection becomes active (enabled by default on `127.0.0.1:19735`, toggleable via settings).
    3) Update `clients/rust/tauri-shell/ui/index.html` to show an Agent Status indicator badge (showing port and active connection count).
  Parallelization: Wave 3 | Blocked by: 9, 10 | Blocks: 13
  Recommended task executor category: visual-engineering
  References: clients/rust/tauri-shell/src-tauri/src/lib.rs:70-220, clients/rust/tauri-shell/ui/index.html
  Acceptance criteria: `cargo check --manifest-path clients/rust/Cargo.toml -p tauri-shell` compiles cleanly; Tauri commands callable via IPC.
  QA scenarios:
    happy: Invoke agent_get_screen_info via IPC; returns remote display dimensions. Evidence: .omo/evidence/task-11-tauri-ipc.txt
    failure: Invoke agent_execute_action when disconnected; returns error "Session not active". Evidence: .omo/evidence/task-11-tauri-disconnected.txt
  Commit: Y | feat(tauri-shell): integrate agent server, Tauri IPC commands, and status UI

- [x] 12. Add headless agent server and MCP CLI flags to maho-client
  What to do / Must NOT do: In `clients/rust/maho-app/src/bin/maho_client.rs`, add CLI arguments:
    - `--agent-server <PORT>`: starts local HTTP/WS agent server on given port (default 19735).
    - `--mcp`: starts stdio MCP server mode, connecting standard input/output to the MCP tool handler.
    Ensure `maho-client` keeps the connection alive and dispatches incoming agent actions directly to the remote host.
  Parallelization: Wave 3 | Blocked by: 9, 10 | Blocks: 13
  Recommended task executor category: unspecified-high
  References: clients/rust/maho-app/src/bin/maho_client.rs:30-100
  Acceptance criteria: `maho-client --help` lists `--agent-server` and `--mcp` flags with documentation.
  QA scenarios:
    happy: Run maho-client with --agent-server flag; verify server binds to port and prints startup log. Evidence: .omo/evidence/task-12-cli-agent-server.txt
    failure: Run maho-client with invalid port (e.g. privileged port 0); exit with descriptive error. Evidence: .omo/evidence/task-12-cli-error.txt
  Commit: Y | feat(maho-client): add --agent-server and --mcp CLI modes for headless agent driving

- [x] 13. Build end-to-end automated integration test suite for remote agent control
  What to do / Must NOT do: Create comprehensive automated integration tests in `clients/rust/maho-app/tests/agent_control_e2e.rs`:
    1) Mock Host & Client Session setup.
    2) Dispatch full spectrum of `AgentAction`s (Move, Left/Right/Middle Click, Drag, Scroll, KeyPress, Hotkey, TypeText, ReleaseAll).
    3) Assert exact wire packet reception and payload decoding on the host side.
    4) Assert coordinate translation precision and clamp behavior.
    5) Assert that `release_all()` successfully clears all active states.
  Parallelization: Wave 4 | Blocked by: 2, 3, 11, 12 | Blocks: Final Verification
  Recommended task executor category: unspecified-high
  References: clients/rust/maho-app/tests/, clients/rust/maho-app/src/agent_input.rs
  Acceptance criteria: `cargo test --manifest-path clients/rust/Cargo.toml -p maho-app --test agent_control_e2e` passes 100% green.
  QA scenarios:
    happy: Run E2E test exercising all agent actions; assert all packets received matching exact expectations. Evidence: .omo/evidence/task-13-e2e-all-actions.txt
    failure: Test connection drop during active drag; assert host receives clean Reset/button-up packet. Evidence: .omo/evidence/task-13-e2e-disconnect-cleanup.txt
  Commit: Y | test(maho-app): add end-to-end integration test suite for agent input control

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [x] F1. Plan compliance audit (verify all 13 todos implemented, all acceptance criteria met, all evidence files captured)
- [x] F2. Code quality review (run cargo clippy --manifest-path clients/rust/Cargo.toml --all-targets -- -D warnings and cargo fmt --check)
- [x] F3. Real manual QA (exercise live HTTP endpoint curl -X POST http://127.0.0.1:19735/api/v1/input/action and query screen info)
- [x] F4. Scope fidelity (verify no unrequested features added, no video pipeline regressions, localhost security boundary strictly enforced)

## Commit strategy
Follow Conventional Commits (`<type>(<scope>): <summary>`):
- One atomic commit per verified todo (RED -> GREEN + evidence).
- Final commit footer: `Plan: .omo/plans/remote-agent-input-control.md`.

## Success criteria
1. **Full Input Protocol**: `maho-proto` encodes and decodes `MiddleMouseDown`, `MiddleMouseUp`, `Reset`, and `RelativeMove` with zero wire size expansion.
2. **Dual-Platform Injection**: Linux Hyprland uinput injects `BTN_MIDDLE` and emergency reset; Windows SendInput injects middle clicks and emergency reset.
3. **Agent Action Engine**: `maho-app` parses human-readable key names, hotkeys ("Ctrl+Shift+T"), types text, normalizes coordinates, and tracks held states with auto-release fail-safe.
4. **Perception & Control API**: Local HTTP/WS REST server (`127.0.0.1:19735`) and MCP stdio server allow any agent to execute actions, query screen geometry, and capture real-time screenshots.
5. **Headless & GUI Parity**: Both `tauri-shell` and `maho-client` expose identical agent control capabilities.
6. **Passing Test Suites**: `cargo test --manifest-path clients/rust/Cargo.toml` passes cleanly across all workspace crates.
