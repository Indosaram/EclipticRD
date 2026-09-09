# Pointer UI and Interaction Foundation Evidence

Status: GREEN - implementation complete, verified with behavioral RED/GREEN and browser scenario evidence.

## 1. Proof Contract Recorded Before Edits

- **Scope**: `clients/rust/tauri-shell/ui/index.html`, `session-overlay.js`, scoped UI test files (`performance.test.mjs`, `session-overlay.test.mjs`), `tests/frontend-page.test.mjs`, and `DESIGN.md` (input contract only).
- **No Rust edits**: Backend mapping in `convert_input_payload` belongs to downstream `desktop-integration` task.
- **No unrelated redesign**: Preserved Parsec-style session layout, CSS tokens, and component primitives.
- **Verification criteria**:
  1. Middle mousedown/mouseup produces `MiddleMouseDown` (11) and `MiddleMouseUp` (12) in order.
  2. Unsupported extra mouse buttons (`e.button >= 3`) are rejected and never map to left click.
  3. Focus loss (window blur, tab hidden, overlay interaction) releases all held mouse buttons and keys.
  4. Relative pointer motion maps to existing `RelativeMove` (14) wire variant, carries accumulated deltas in `scroll_dx` and `scroll_dy`, flushes on RAF, and exits/releases on blur/disconnect.
  5. User-accessible pointer lock toggle (`btn-pointer-lock`) is enabled only when real webview Pointer Lock API (`Element.prototype.requestPointerLock`, `document.exitPointerLock`, `pointerLockElement`) is present; unsupported environments never simulate a locked state.
  6. Pending motion ordering, stale generations, local overlay focus isolation, and first-frame streaming guards are preserved.

---

## 2. Behavioral RED Evidence (Captured Before Production Edits)

Log file: `.omo/mass-ulw-20260906/foundation/pointer-red.log`

### Test Suite Failures

1. `clients/rust/tauri-shell/ui/session-overlay.test.mjs` (Node):
   - `held input tracker tracks middle button and emits MiddleMouseUp`:
     `AssertionError: Expected values to be strictly equal: true !== false` (middle button coerced to `'left'`).
   - `unsupported extra buttons never map to left button`:
     `AssertionError: Expected values to be strictly equal: true !== false` (button 3/4 coerced to `'left'`).
   - `releaseEvents releases all held buttons in order`:
     `AssertionError: Expected values to be strictly equal: 2 !== 3` (middle button merged into `'left'`, producing only 2 release events).
   - `overlay state tracks pointer lock active state`:
     `AssertionError: getPointerLockActive must exist: 'undefined' !== 'function'`.
   - `index.html ships explicit user-accessible pointer lock control in launcher panel`:
     `AssertionError: pointer lock action must exist in index.html`.

2. `clients/rust/tauri-shell/ui/performance.test.mjs` (Node):
   - `relative move motion accumulates deltas and flushes on RAF`:
     `ReferenceError: sendRelativePointerEvent is not defined`.
   - `button down flushes preceding coalesced relative move first`:
     `ReferenceError: sendRelativePointerEvent is not defined`.

3. `clients/rust/tauri-shell/tests/frontend-page.test.mjs` (Bun with WebKit `Bun.WebView`):
   - `C3 middle button, extra button rejection, focus-loss release and relative pointer in live session`:
     ```
     error: expect(received).toBe(expected)
     Expected: "MiddleMouseDown"
     Received: "LeftMouseDown"
     ```

---

## 3. Implementation Summary

### `clients/rust/tauri-shell/ui/session-overlay.js`
- Added `normalizeMouseButton(button)`:
  - Maps `0` / `'left'` -> `'left'`
  - Maps `1` / `'middle'` -> `'middle'`
  - Maps `2` / `'right'` -> `'right'`
  - Returns `null` for extra buttons (`3`, `4`, `'back'`, `'forward'`), preventing extra buttons from ever coercing to `'left'`.
- Updated `buttonUpType(button)` to return `'LeftMouseUp'`, `'MiddleMouseUp'`, or `'RightMouseUp'`.
- Updated `createHeldInputTracker()`:
  - Tracks `'left'`, `'middle'`, and `'right'` in `heldButtons` `Set`.
  - Discards null buttons on `mouseDown`, `mouseUp`, and `isButtonDown`.
  - `releaseEvents` releases all held buttons in order, followed by keys.
- Updated `createOverlayState()`:
  - Added `pointerLockActive` observable state with `getPointerLockActive()` and `setPointerLockActive(value)`.

### `clients/rust/tauri-shell/ui/index.html`
- Markup: Added `btn-pointer-lock` inside `.launcher-panel .panel-actions` using existing `.overlay-btn` tokens, SVG crosshair icon, and `aria-pressed="false"`.
- Real Webview API detection (`hasPointerLockApi()`):
  - Checks `typeof Element.prototype.requestPointerLock === 'function'`, `typeof document.exitPointerLock === 'function'`, and `'pointerLockElement' in document`.
  - When API is absent, `btn-pointer-lock` is hidden (`hidden = true`) and lock is never simulated.
- Pointer Lock interaction:
  - `togglePointerLock()` requests or exits pointer lock.
  - Listens to `pointerlockchange` and `pointerlockerror` to update `launcherState.setPointerLockActive` and `btn-pointer-lock` `aria-pressed` / label.
- Button mapping (`mapMouseButton`, `mouseButtonEventType`):
  - In `viewport` `mousedown`: `e.button === 1` calls `e.preventDefault()` (suppressing browser autoscroll) and sends `MiddleMouseDown`.
  - In `window` `mouseup`: releases held button and sends matching `*MouseUp`.
  - Extra buttons (`e.button >= 3`) return `null` and are ignored; never sent to host or tracked as left clicks.
- Relative pointer interaction (`sendRelativePointerEvent`):
  - Activated when `isPointerLocked()` is true.
  - Coalesces relative movement deltas into `scroll_dx` and `scroll_dy` across animation frames.
  - Motion ordering preserved: any subsequent button event or session teardown flushes pending `RelativeMove` before the button/disconnect event.
- Safe release on focus loss:
  - Window `blur`, document `visibilitychange` (hidden), overlay click/focus, and session disconnect call `releasePointerLock()` and `releaseLocalInputs()`.

### `clients/rust/tauri-shell/DESIGN.md`
- Documented stable hooks `btn-pointer-lock` and `btn-pointer-lock-label`.
- Documented pointer input contract: button mappings (0, 1, 2; rejection of >= 3), relative motion format, real pointer lock availability, and teardown release semantics.

---

## 4. Behavioral GREEN Evidence

### Commands Executed
```bash
node --test clients/rust/tauri-shell/ui/performance.test.mjs clients/rust/tauri-shell/ui/session-overlay.test.mjs
bun test clients/rust/tauri-shell/tests/frontend-page.test.mjs
```

### Node Test Suite Output (32/32 PASS)
```
TAP version 13
ok 1 - final motion at t=105 after t=100 reaches endpoint on RAF
ok 2 - button up flushes preceding coalesced drag first
ok 3 - stop cancels pending motion and video RAF before disconnect IPC resolves
ok 4 - ending a streaming session submits pending drag before held-button release and native disconnect
ok 5 - old deferred poll after stop cannot draw, schedule or change counters
ok 6 - old deferred poll after stop/reconnect cannot draw, schedule or change counters
ok 7 - unavailable WebGL2 rejects via connection error path without WebGL1 fallback
ok 8 - renderer reports draw status and preserves texture reuse; lost context never counts
ok 9 - TCP terminal IPC error ends waiting-video and preserves detail through cleanup
ok 10 - TCP terminal IPC error ends streaming and preserves detail through cleanup
ok 11 - old TCP terminal rejection cannot end a reconnected generation
ok 12 - TCP terminal cleanup exposes join failure without losing terminal detail
ok 13 - relative move motion accumulates deltas and flushes on RAF
ok 14 - button down flushes preceding coalesced relative move first
ok 15 - middle mouse down and up emit MiddleMouseDown and MiddleMouseUp in order
ok 16 - index.html ships the persistent session launcher markup
ok 17 - launcher integrates home, expandable panel, disconnect and fullscreen controls
ok 18 - keyboard events on overlay UI are not forwarded to the remote host
ok 19 - keyboard events on the video surface are forwarded to the remote host
ok 20 - held input tracker is empty by default and tracks key state
ok 21 - releaseEvents emits mouse-ups then key-ups and clears state
ok 22 - releasing a key before releaseEvents drops it from the release batch
ok 23 - mouse button up only releases buttons that were pressed
ok 24 - held input tracker tracks middle button and emits MiddleMouseUp
ok 25 - unsupported extra buttons never map to left button
ok 26 - releaseEvents releases all held buttons in order
ok 27 - overlay state toggles expansion and notifies subscribers
ok 28 - overlay state tracks fullscreen pressed-state
ok 29 - overlay state tracks pointer lock active state
ok 30 - index.html ships explicit user-accessible pointer lock control in launcher panel
ok 31 - styles.css gives session-error pointer-events: auto so overlay alerts remain clickable
ok 32 - styles.css guarantees min-width on overlay-badge and content-aware wrapping on launcher-bar
1..32
# tests 32
# suites 0
# pass 32
# fail 0
# cancelled 0
# skipped 0
# todo 0
```

### Bun Frontend Page Test Suite Output (9/9 PASS)
```
bun test v1.4.0 (34cbb9a40)

clients/rust/tauri-shell/tests/frontend-page.test.mjs:
PAGE CLEANUP {"origin":"http://127.0.0.1:57802","webViewClosed":true,"serverStopped":true}
(pass) failed held release still releases remaining keys and does not poison later sessions
PAGE CLEANUP {"origin":"http://127.0.0.1:57811","webViewClosed":true,"serverStopped":true}
(pass) C3 failed key release does not skip remaining keys or poison cleanup and reconnect
PAGE CLEANUP {"origin":"http://127.0.0.1:57816","webViewClosed":true,"serverStopped":true}
(pass) C2 cleanup failure retry and C3 unavailable stats and renderer
PAGE CLEANUP {"origin":"http://127.0.0.1:57821","webViewClosed":true,"serverStopped":true}
(pass) C1 real page search, composed filters, favorites and same-origin persistence
PAGE CLEANUP {"origin":"http://127.0.0.1:57827","webViewClosed":true,"serverStopped":true}
(pass) C2 real direct form rejects fields and cancels late connect with final cleanup
PAGE CLEANUP {"origin":"http://127.0.0.1:57832","webViewClosed":true,"serverStopped":true}
(pass) C4 external labels are literal; favorite is isolated and quoted Connect captures original target
PAGE CLEANUP {"origin":"http://127.0.0.1:57837","webViewClosed":true,"serverStopped":true}
(pass) C3 real WebGL frame, stale poll, local keys, held release and fullscreen failure
PAGE CLEANUP {"origin":"http://127.0.0.1:57842","webViewClosed":true,"serverStopped":true}
PAGE CLEANUP {"origin":"http://127.0.0.1:57847","webViewClosed":true,"serverStopped":true}
(pass) C1 refresh failure/retry, empty inventory, unpaired prefill and no bridge
PAGE CLEANUP {"origin":"http://127.0.0.1:57852","webViewClosed":true,"serverStopped":true}
(pass) C3 middle button, extra button rejection, focus-loss release and relative pointer in live session

 9 pass
 0 fail
 53 expect() calls
Ran 9 tests across 1 file.
```

---

## 5. Live Browser Scenario Evidence

Executed script: `.omo/mass-ulw-20260906/foundation/browser-pointer-scenario.mjs`
Execution runner: `Bun.WebView` (WebKit) with local mock IPC server.
**LABEL NOTE**: Mock IPC in `Bun.WebView` verifies DOM event mapping, component state, and IPC call generation. It is NOT native host injection proof. Native OS desktop injection QA belongs to lead.

### Execution Log
```
=== EXERCISING BROWSER POINTER SCENARIO ===
NOTE: Uses deterministic mock IPC fixture in Bun.WebView; NOT native host injection proof.
1. Computer library loaded
2. Connected, phase waiting-video
3. First video frame rendered, session streaming
4. Middle mousedown sent: MiddleMouseDown
5. Middle mouseup sent: MiddleMouseUp
6. Extra button 3 rejected (no send_input calls): true
7. Pointer lock control in launcher panel: {
  exists: true,
  hidden: false,
  pressed: "false",
  label: "Pointer lock",
}
8. RelativeMove event generated: {
  event_type: "RelativeMove",
  dx: 24,
  dy: -16,
}
9. Screenshot saved to: .omo/mass-ulw-20260906/foundation/pointer-session.png
10. Disconnected cleanly, returning to library
PAGE CLEANUP {"origin":"http://127.0.0.1:57789","webViewClosed":true,"serverStopped":true}
11. Page and server teardown complete
```

### Artifacts Generated
- Screenshot: `.omo/mass-ulw-20260906/foundation/pointer-session.png` (71 KB PNG)
  - Shows live remote session, WebGL NV12 render viewport, expanded launcher panel with Session details, Fullscreen action, and the newly integrated Pointer lock action (`btn-pointer-lock`).
- Cleanup: Clean WebView close and server teardown receipts confirmed (`webViewClosed: true`, `serverStopped: true`).

---

## 6. InputEventType Payload Specification for Downstream Desktop Integration

The following wire payloads are emitted by the UI and must be decoded in `clients/rust/tauri-shell/src-tauri/src/lib.rs:convert_input_payload`:

### 1. `MiddleMouseDown` (Wire ID: 11)
```json
{
  "event_type": "MiddleMouseDown",
  "x": 300.0,
  "y": 200.0,
  "view_width": 1280.0,
  "view_height": 800.0,
  "modifiers": 0,
  "scroll_dx": 0.0,
  "scroll_dy": 0.0
}
```
Rust mapping target: `erd_proto::InputEventType::MiddleMouseDown`.

### 2. `MiddleMouseUp` (Wire ID: 12)
```json
{
  "event_type": "MiddleMouseUp",
  "x": 300.0,
  "y": 200.0,
  "view_width": 1280.0,
  "view_height": 800.0,
  "modifiers": 0,
  "scroll_dx": 0.0,
  "scroll_dy": 0.0
}
```
Rust mapping target: `erd_proto::InputEventType::MiddleMouseUp`.

### 3. `RelativeMove` (Wire ID: 14)
```json
{
  "event_type": "RelativeMove",
  "x": 300.0,
  "y": 200.0,
  "view_width": 1280.0,
  "view_height": 800.0,
  "modifiers": 0,
  "scroll_dx": 24.0,
  "scroll_dy": -16.0
}
```
Rust mapping target: `erd_proto::InputEventType::RelativeMove`.
Note: Relative deltas are stored in `scroll_dx` (horizontal delta) and `scroll_dy` (vertical delta) per `erd-host` injection implementations on Linux (`inject_linux.rs:358`) and Windows (`inject_windows.rs:106`).

---

## 7. Out-of-Scope Discoveries for Lead Registration

1. **Rust backend string parsing**: `convert_input_payload` in `tauri-shell/src-tauri/src/lib.rs` currently lacks branches for `"MiddleMouseDown"`, `"MiddleMouseUp"`, and `"RelativeMove"`. These will return `Unknown event type: ...` when received in native Tauri builds until `desktop-integration` connects them.
2. **Mac host RelativeMove injection**: In `erd-host/src/inject_macos.rs:193`, `InputEventType::RelativeMove` currently returns `Ok(None)`. Relative pointer injection on macOS host daemon is a known platform limitation.
3. **Duplicate `index.html` copy**: An identical duplicate exists at `clients/rust/tauri-shell/src-tauri/ui/index.html`. `tauri.conf.json` defines `"frontendDist": "../ui"`, so the authoritative copy is `clients/rust/tauri-shell/ui/index.html`.
