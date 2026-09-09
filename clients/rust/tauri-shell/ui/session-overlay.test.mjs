// Regression tests for the Parsec-style session overlay (tauri-shell UI).
// Run: node --test clients/rust/tauri-shell/ui/session-overlay.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const overlay = require('./session-overlay.js');
const indexHtml = readFileSync(new URL('./index.html', import.meta.url), 'utf8');

// ---------------------------------------------------------------------------
// 1. Structural regression: a persistent launcher must exist in the markup.
//    The old overlay dimmed itself to opacity 0.15 and only exposed
//    Disconnect; it must now stay visible and integrate home / expand /
//    disconnect / fullscreen / stats controls.
// ---------------------------------------------------------------------------
test('index.html ships the persistent session launcher markup', () => {
  assert.match(indexHtml, /<script src="session-overlay\.js"><\/script>/,
    'session-overlay.js must be included by the shell page');
  assert.match(indexHtml, /id="session-overlay"[^>]*data-ui-scope/,
    'overlay root must be marked as a local UI scope so input never leaks to the host');
  assert.match(indexHtml, /class="[^"]*launcher-bar/,
    'a compact launcher bar must exist above the video');
  // The bar collapsed into a toggle icon: it must start hidden and the
  // toggle must declare what it controls.
  assert.match(indexHtml, /class="launcher-bar collapsed" id="launcher-bar"/,
    'launcher bar must start collapsed behind the toggle icon');
  const toggle = indexHtml.match(/<button[^>]*id="btn-overlay-toggle"[^>]*>/);
  assert.ok(toggle, 'overlay toggle icon must exist');
  assert.match(toggle[0], /aria-expanded="false"/, 'overlay toggle must expose its state');
  assert.match(toggle[0], /aria-controls="launcher-bar"/, 'overlay toggle must name its bar');
  // The old overlay hid itself behind opacity: 0.15 — that must never return.
  assert.doesNotMatch(indexHtml, /opacity:\s*0\.15/,
    'overlay must remain visible (no opacity-dimming regression)');
});

test('launcher integrates home, expandable panel, disconnect and fullscreen controls', () => {
  assert.match(indexHtml, /id="btn-home"/, 'home (return to main screen) action must exist');
  assert.match(indexHtml, /id="btn-disconnect"/, 'disconnect action must exist');
  const expand = indexHtml.match(/<button[^>]*id="btn-expand"[^>]*>/);
  assert.ok(expand, 'expand toggle must exist');
  assert.match(expand[0], /aria-expanded="false"/, 'expand toggle must expose its state');
  assert.match(expand[0], /aria-controls="launcher-panel"/, 'expand toggle must name its panel');
  assert.match(indexHtml, /id="launcher-panel"[^>]*hidden/, 'expandable panel starts collapsed');
  const fullscreen = indexHtml.match(/<button[^>]*id="btn-fullscreen"[^>]*>/);
  assert.ok(fullscreen, 'fullscreen action must exist');
  assert.match(fullscreen[0], /aria-pressed="false"/, 'fullscreen toggle must expose pressed state');
});

// ---------------------------------------------------------------------------
// 2. Input-leak regression: keyboard events targeting overlay UI (buttons,
//    panel, connecting modal, dashboard chrome, inputs) must never be
//    forwarded to the remote host.
// ---------------------------------------------------------------------------
test('keyboard events on overlay UI are not forwarded to the remote host', () => {
  const button = { tagName: 'BUTTON', id: '' };
  const panelStat = { tagName: 'DD', id: 'session-stat-state' };
  const input = { tagName: 'INPUT', id: 'direct-ip' };
  const modalButton = { tagName: 'BUTTON', id: '' };
  assert.equal(overlay.shouldForwardKeyboardEvent({ target: button }), false);
  assert.equal(overlay.shouldForwardKeyboardEvent({ target: panelStat }), false);
  assert.equal(overlay.shouldForwardKeyboardEvent({ target: input }), false);
  assert.equal(overlay.shouldForwardKeyboardEvent({ target: modalButton }), false);
  assert.equal(overlay.shouldForwardKeyboardEvent({ target: null }), false);
  assert.equal(overlay.shouldForwardKeyboardEvent(undefined), false);
});

test('keyboard events on the video surface are forwarded to the remote host', () => {
  assert.equal(overlay.shouldForwardKeyboardEvent({ target: { tagName: 'BODY', id: '' } }), true);
  assert.equal(overlay.shouldForwardKeyboardEvent({ target: { tagName: 'DIV', id: 'viewport' } }), true);
  assert.equal(overlay.shouldForwardKeyboardEvent({ target: { tagName: 'CANVAS', id: 'screen-canvas' } }), true);
});

// ---------------------------------------------------------------------------
// 3. Safe release of held input: every key/button the UI pressed into the
//    remote must produce an explicit release event when attention moves to
//    the overlay (or the window loses focus), and releasing must be
//    idempotent.
// ---------------------------------------------------------------------------
test('held input tracker is empty by default and tracks key state', () => {
  const t = overlay.createHeldInputTracker();
  assert.equal(t.size, 0);
  assert.equal(t.isKeyDown(0x41), false);

  t.keyDown(0x41, 1);
  assert.equal(t.isKeyDown(0x41), true);
  assert.equal(t.size, 1);

  t.keyUp(0x41);
  assert.equal(t.isKeyDown(0x41), false);
  assert.equal(t.size, 0);
});

test('releaseEvents emits mouse-ups then key-ups and clears state', () => {
  const t = overlay.createHeldInputTracker();
  t.mouseDown('left');
  t.mouseDown('right');
  t.keyDown(0x41, 1); // 'A' with shift
  t.keyDown(0x2E, 0);
  assert.equal(t.size, 4);

  const events = t.releaseEvents(320.5, 240.25, 1280, 800);

  assert.equal(events.length, 4);
  // Buttons release first (mirrors the Rust InputStateTracker::release_all order).
  assert.deepEqual(
    events.map((e) => e.event_type),
    ['LeftMouseUp', 'RightMouseUp', 'KeyUp', 'KeyUp'],
  );
  assert.deepEqual(events[0], {
    event_type: 'LeftMouseUp',
    x: 320.5,
    y: 240.25,
    view_width: 1280,
    view_height: 800,
  });
  const keyA = events[2];
  assert.equal(keyA.key_code, 0x41);
  assert.equal(keyA.modifiers, 1);
  assert.equal(events[3].key_code, 0x2E);

  // Releasing again must be a no-op (idempotent, no duplicate ups).
  assert.deepEqual(t.releaseEvents(0, 0, 1280, 800), []);
  assert.equal(t.size, 0);
});

test('releasing a key before releaseEvents drops it from the release batch', () => {
  const t = overlay.createHeldInputTracker();
  t.keyDown(0x41, 0);
  t.keyDown(0x42, 0);
  t.keyUp(0x41);
  const events = t.releaseEvents(0, 0, 1280, 800);
  assert.equal(events.length, 1);
  assert.equal(events[0].key_code, 0x42);
});

test('mouse button up only releases buttons that were pressed', () => {
  const t = overlay.createHeldInputTracker();
  t.mouseDown('left');
  assert.equal(t.isButtonDown('left'), true);
  assert.equal(t.isButtonDown('right'), false);
  t.mouseUp('left');
  t.mouseUp('right'); // spurious up: ignored
  assert.equal(t.size, 0);
  assert.deepEqual(t.releaseEvents(0, 0, 1280, 800), []);
});

test('held input tracker tracks middle button and emits MiddleMouseUp', () => {
  const t = overlay.createHeldInputTracker();
  t.mouseDown('middle');
  assert.equal(t.isButtonDown('middle'), true);
  assert.equal(t.isButtonDown('left'), false);
  assert.equal(t.isButtonDown('right'), false);
  const events = t.releaseEvents(100, 200, 1280, 800);
  assert.equal(events.length, 1);
  assert.deepEqual(events[0], {
    event_type: 'MiddleMouseUp',
    x: 100,
    y: 200,
    view_width: 1280,
    view_height: 800,
  });
  assert.equal(t.size, 0);
});

test('unsupported extra buttons never map to left button', () => {
  const t = overlay.createHeldInputTracker();
  t.mouseDown(3);
  t.mouseDown(4);
  t.mouseDown('extra');
  t.mouseDown('back');
  assert.equal(t.isButtonDown('left'), false);
  assert.equal(t.isButtonDown('middle'), false);
  assert.equal(t.isButtonDown('right'), false);
  assert.equal(t.size, 0);
  assert.deepEqual(t.releaseEvents(0, 0, 1280, 800), []);
});

test('releaseEvents releases all held buttons in order', () => {
  const t = overlay.createHeldInputTracker();
  t.mouseDown('left');
  t.mouseDown('middle');
  t.mouseDown('right');
  assert.equal(t.size, 3);
  const events = t.releaseEvents(10, 20, 1280, 800);
  assert.equal(events.length, 3);
  assert.deepEqual(
    events.map((e) => e.event_type),
    ['LeftMouseUp', 'MiddleMouseUp', 'RightMouseUp']
  );
  assert.equal(t.size, 0);
});

// ---------------------------------------------------------------------------
// 4. Overlay UI state: expand/collapse and fullscreen pressed-state must be
//    observable state changes (driving aria-expanded / aria-pressed).
// ---------------------------------------------------------------------------
test('overlay state toggles expansion and notifies subscribers', () => {
  const s = overlay.createOverlayState();
  assert.equal(s.getExpanded(), false);

  const seen = [];
  const unsubscribe = s.subscribe((state) => seen.push(state.expanded));

  s.toggleExpanded();
  assert.equal(s.getExpanded(), true);
  s.toggleExpanded();
  assert.equal(s.getExpanded(), false);
  assert.deepEqual(seen, [true, false]);

  unsubscribe();
  s.toggleExpanded();
  assert.deepEqual(seen, [true, false], 'unsubscribed listeners stop receiving updates');
});

test('overlay state tracks fullscreen pressed-state', () => {
  const s = overlay.createOverlayState();
  assert.equal(s.getFullscreenActive(), false);
  s.setFullscreenActive(true);
  assert.equal(s.getFullscreenActive(), true);
  s.setFullscreenActive(false);
  assert.equal(s.getFullscreenActive(), false);
});

test('overlay state tracks pointer lock active state', () => {
  const s = overlay.createOverlayState();
  assert.equal(typeof s.getPointerLockActive, 'function', 'getPointerLockActive must exist');
  assert.equal(s.getPointerLockActive(), false);
  s.setPointerLockActive(true);
  assert.equal(s.getPointerLockActive(), true);
  s.setPointerLockActive(false);
  assert.equal(s.getPointerLockActive(), false);
});

test('index.html ships explicit user-accessible pointer lock control in launcher panel', () => {
  assert.match(indexHtml, /id="btn-pointer-lock"/, 'pointer lock action must exist in index.html');
  const lockMatch = indexHtml.match(/<button[^>]*id="btn-pointer-lock"[^>]*>/);
  assert.ok(lockMatch, 'pointer lock button tag must exist');
  assert.match(lockMatch[0], /aria-pressed="false"/, 'pointer lock toggle must expose pressed state');
});

// ---------------------------------------------------------------------------
// 5. Layout and accessibility regressions (200% text zoom and overlay hit-testing)
// ---------------------------------------------------------------------------
test('styles.css gives session-error pointer-events: auto so overlay alerts remain clickable', () => {
  const stylesCss = readFileSync(new URL('./styles.css', import.meta.url), 'utf8');
  assert.match(stylesCss, /#session-error\s*\{[^}]*pointer-events:\s*auto/s,
    'session-error must restore pointer-events: auto when mounted inside pointer-events: none session-overlay');
});

test('styles.css guarantees min-width on overlay-badge and content-aware wrapping on launcher-bar', () => {
  const stylesCss = readFileSync(new URL('./styles.css', import.meta.url), 'utf8');
  assert.match(stylesCss, /\.overlay-badge\s*\{[^}]*min-width:\s*[56]rem/,
    'overlay-badge must reserve min-width so host identity never collapses to 0 at enlarged text');
  assert.match(stylesCss, /@container\s*\(max-width:\s*41rem\)\s*\{[^}]*\.launcher-bar\s*\{[^}]*flex-wrap:\s*wrap/s,
    'launcher-bar must wrap via container query rather than overflowing at enlarged text');
  assert.match(stylesCss, /#session-overlay\s*\{[^}]*width:\s*min\(40rem/,
    'session-overlay width must scale with rem rather than rigid pixel width');
});
