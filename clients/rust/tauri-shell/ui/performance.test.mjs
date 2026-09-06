import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const html = readFileSync(new URL('./index.html', import.meta.url), 'utf8');
const script = html.match(/<script>([\s\S]*?)<\/script>/)[1];
const overlay = readFileSync(new URL('./session-overlay.js', import.meta.url), 'utf8');
const library = readFileSync(new URL('./library.js', import.meta.url), 'utf8');
const lifecycle = readFileSync(new URL('./connection-state.js', import.meta.url), 'utf8');
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function harness({ webgl2 = true } = {}) {
  let now = 100, nextId = 0, lost = false;
  const raf = new Map(), calls = [], polls = [], alerts = [], draws = [], uploads = [], contexts = [], intervals = [];
  const gl = new Proxy({}, { get: (_, key) => {
    if (key === 'isContextLost') return () => lost;
    if (key === 'getShaderParameter' || key === 'getProgramParameter') return () => true;
    if (key === 'drawArrays') return (...args) => draws.push(args);
    if (key === 'texImage2D') return (...args) => uploads.push(args);
    return () => ({});
  }});
  const elements = new Map();
  const element = id => {
    if (!elements.has(id)) elements.set(id, {
      style: {}, dataset: {}, children: [], textContent: '', isConnected: true,
      classList: { add() {}, remove() {}, toggle() {}, contains() { return false; } }, width: 100, height: 100,
      setAttribute() {}, addEventListener() {}, focus() {}, closest() { return null; },
      replaceChildren(...nodes) { this.children = nodes; }, append(...nodes) { this.children.push(...nodes); },
      getBoundingClientRect: () => ({ left: 0, top: 0, width: 100, height: 100 }),
      getContext: kind => { contexts.push(kind); return kind === 'webgl2' && !webgl2 ? null : gl; },
    });
    return elements.get(id);
  };
  const disconnect = deferred();
  const context = vm.createContext({
    console, DataView, Uint8Array, Float32Array, performance: { now: () => now },
    document: { getElementById: element, addEventListener() {}, fullscreenElement: null,
      activeElement: element('body'), createElement: tag => element('created-' + tag + '-' + ++nextId),
      createTextNode: text => ({ textContent: text }) },
    requestAnimationFrame: fn => { const id = ++nextId; raf.set(id, fn); return id; },
    cancelAnimationFrame: id => raf.delete(id), setInterval: fn => intervals.push(fn),
    alert: message => alerts.push(message),
    window: { addEventListener() {}, localStorage: { getItem: () => null, setItem() {} }, __TAURI__: { core: { invoke: (cmd, args) => {
      calls.push({ cmd, ...args });
      if (cmd === 'poll_frame_raw' || cmd === 'stats') { const d = deferred(); polls.push({ cmd, ...d }); return d.promise; }
      if (cmd === 'disconnect') return disconnect.promise;
      return Promise.resolve();
    } } } },
  });
  vm.runInContext(overlay, context);
  vm.runInContext(library, context);
  vm.runInContext(lifecycle, context);
  context.SessionOverlay = context.window.SessionOverlay;
  context.LibraryModel = context.window.LibraryModel;
  context.ConnectionLifecycle = context.window.ConnectionLifecycle;
  vm.runInContext(script, context);
  const run = code => vm.runInContext(code, context);
  return { run, raf, calls, polls, alerts, draws, uploads, contexts, intervals, disconnect, element,
    time: value => { now = value; }, loseContext: () => { lost = true; },
    tick: async () => { const callbacks = [...raf.values()]; raf.clear(); await Promise.all(callbacks.map(fn => fn(now))); },
    connect: () => run("connectToHost('host', 'Host')"),
    move: (x, type = 'MouseMove') => run(`sendPointerEvent('${type}', {clientX:${x}, clientY:${x}})`),
    inputs: () => calls.filter(c => c.cmd === 'send_input').map(c => c.event),
  };
}
function frame() {
  const buf = new ArrayBuffer(22), view = new DataView(buf);
  view.setUint32(0, 2, true); view.setUint32(4, 2, true);
  return buf;
}

test('final motion at t=105 after t=100 reaches endpoint on RAF', async () => {
  const h = harness(); h.run('isConnected = true');
  h.move(10); h.time(105); h.move(90); await h.tick();
  assert.equal(h.inputs().at(-1).x, 90);
  assert.equal(h.raf.size, 0);
});
test('button up flushes preceding coalesced drag first', () => {
  const h = harness(); h.run('isConnected = true');
  h.move(10, 'LeftMouseDragged'); h.time(105); h.move(90, 'LeftMouseDragged'); h.move(95, 'LeftMouseUp');
  assert.deepEqual(h.inputs().slice(-2).map(e => [e.event_type, e.x]), [['LeftMouseDragged', 90], ['LeftMouseUp', 95]]);
  assert.equal(h.raf.size, 0);
});
test('stop cancels pending motion and video RAF before disconnect IPC resolves', async () => {
  const h = harness(); await h.connect();
  // Input belongs to the remote only after a frame has established streaming.
  await h.run('connection.markFrameRendered(connection.token())');
  h.move(10); h.time(105); h.move(90);
  assert.equal(h.raf.size, 2, 'both video and final pointer motion are pending');
  const stop = h.run('doDisconnect()');
  assert.equal(h.raf.size, 0);
  const count = h.inputs().length; await h.tick(); assert.equal(h.inputs().length, count);
  h.disconnect.resolve(); await stop;
});
test('ending a streaming session submits pending drag before held-button release and native disconnect', async () => {
  const h = harness(); await h.connect();
  await h.run('connection.markFrameRendered(connection.token())');
  h.run('heldInputs.mouseDown("left"); sendPointerEvent("LeftMouseDown", {clientX:10, clientY:10})');
  h.move(90, 'LeftMouseDragged');
  assert.equal(h.raf.size, 2, 'video poll and final drag are pending');
  const stop = h.run('doDisconnect()');
  h.disconnect.resolve(); await stop;
  assert.deepEqual(h.inputs().slice(-2).map(e => [e.event_type, e.x]), [['LeftMouseDragged', 90], ['LeftMouseUp', 90]]);
  const kinds = h.calls.map(c => c.cmd);
  assert.ok(kinds.lastIndexOf('disconnect') > kinds.lastIndexOf('send_input'), 'native disconnect follows every input submission');
  assert.equal(h.raf.size, 0);
});
for (const reconnect of [false, true]) test(`old deferred poll after stop${reconnect ? '/reconnect' : ''} cannot draw, schedule or change counters`, async () => {
  const h = harness(); await h.connect(); const oldTick = h.tick();
  const stop = h.run('doDisconnect()'); h.disconnect.resolve(); await stop;
  if (reconnect) await h.connect();
  h.time(1500); const counters = h.run('[frameCount, lastFpsCalcTime]');
  h.polls[0].resolve(frame()); await oldTick;
  assert.equal(h.draws.length, 0);
  assert.deepEqual(h.run('[frameCount, lastFpsCalcTime]'), counters);
  assert.equal(h.raf.size, reconnect ? 1 : 0);
  if (reconnect) {
    const tick = h.tick(); assert.equal(h.polls.length, 2);
    h.polls[1].resolve(frame()); await tick;
    assert.equal(h.draws.length, 1); assert.equal(h.raf.size, 1);
  }
});
test('unavailable WebGL2 rejects via connection error path without WebGL1 fallback', async () => {
  const h = harness({ webgl2: false }); await h.connect();
  const tick = h.tick(); h.polls[0].resolve(frame());
  h.disconnect.resolve(); await tick;
  assert.deepEqual(h.contexts, ['webgl2']); assert.equal(h.alerts.length, 0);
  assert.equal(h.element('direct-error').hidden, false);
  assert.ok(h.element('direct-error').textContent.length > 0);
  assert.equal(h.run('connection.snapshot().phase'), 'error');
  assert.equal(h.calls.filter(c => c.cmd === 'disconnect').length, 1);
  assert.equal(h.draws.length, 0); assert.equal(h.raf.size, 0);
  assert.equal(h.run('isConnected'), false);
});
test('renderer reports draw status and preserves texture reuse; lost context never counts', async () => {
  const h = harness();
  assert.equal(h.run('renderNv12(2, 2, new Uint8Array(4), new Uint8Array(2))'), true);
  assert.equal(h.run('renderNv12(2, 2, new Uint8Array(4), new Uint8Array(2))'), true);
  assert.equal(h.uploads.length, 2); assert.equal(h.draws.length, 2);
  h.loseContext(); await h.connect(); const tick = h.tick(); h.polls[0].resolve(frame());
  h.disconnect.resolve(); await tick;
  assert.equal(h.draws.length, 2); assert.equal(h.run('frameCount'), 0);
  assert.equal(h.element('direct-error').hidden, false);
  assert.equal(h.run('connection.snapshot().phase'), 'error');
  assert.equal(h.raf.size, 0);
});

for (const streaming of [false, true]) test(`TCP terminal IPC error ends ${streaming ? 'streaming' : 'waiting-video'} and preserves detail through cleanup`, async () => {
  // Given the shipped script with an in-flight native frame poll.
  const h = harness(); await h.connect();
  if (streaming) await h.run('connection.markFrameRendered(connection.token())');
  const tick = h.tick();
  const terminal = 'tcp-terminal-fixture-42';
  // When the backend reports its consumed terminal mailbox error.
  h.polls[0].reject(terminal); h.disconnect.resolve(); await tick;
  // Then the local error surface retains it and tears down once.
  assert.equal(h.run('connection.snapshot().error'), terminal);
  assert.equal(h.element('direct-error').textContent, terminal);
  assert.equal(h.element('direct-error').hidden, false);
  assert.equal(h.run('connection.snapshot().phase'), 'error');
  assert.equal(h.calls.filter(call => call.cmd === 'disconnect').length, 1);
  assert.equal(h.run('isConnected'), false);
  assert.equal(h.raf.size, 0);
});

test('old TCP terminal rejection cannot end a reconnected generation', async () => {
  // Given an old poll retained across successful cleanup and reconnect.
  const h = harness(); await h.connect(); const oldTick = h.tick();
  const stop = h.run('doDisconnect()'); h.disconnect.resolve(); await stop;
  await h.connect();
  const generation = h.run('connection.token()');
  // When the prior native poll rejects after the new generation owns the UI.
  h.polls[0].reject('stale-tcp-terminal-fixture'); await oldTick;
  // Then it cannot report an error, disconnect or cancel the new poll.
  assert.equal(h.run('connection.token()'), generation);
  assert.equal(h.run('connection.snapshot().phase'), 'waiting-video');
  assert.equal(h.run('connection.snapshot().error'), null);
  assert.equal(h.calls.filter(call => call.cmd === 'disconnect').length, 1);
  assert.equal(h.raf.size, 1);
});

test('TCP terminal cleanup exposes join failure without losing terminal detail', async () => {
  const h = harness(); await h.connect(); const tick = h.tick();
  const terminal = 'tcp-terminal-fixture', joinError = 'tcp-join-fixture';
  h.polls[0].reject(terminal); h.disconnect.reject(joinError); await tick;
  assert.equal(h.run('connection.snapshot().error'), terminal);
  assert.equal(h.run('connection.snapshot().cleanupError'), joinError);
  assert.equal(h.element('session-error-text').textContent, joinError);
  assert.equal(h.element('btn-retry-cleanup').hidden, false);
  assert.equal(h.run('isConnected'), false);
  assert.equal(h.raf.size, 0);
});
