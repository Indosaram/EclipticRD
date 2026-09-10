import { test } from 'bun:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { validateConnection, createConnection } = require('./connection-state.js');
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};

// Each native operation has an explicit arrival signal, armed before the action.
function fixture({ releaseInputs = async () => {}, nativeAvailable = true } = {}) {
  const calls = [];
  const arrivals = new Map();
  function next(command) {
    const signal = deferred();
    const queue = arrivals.get(command) || [];
    queue.push(signal);
    arrivals.set(command, queue);
    return signal.promise;
  }
  const connection = createConnection({ nativeAvailable, releaseInputs, invoke(command, args) {
    const result = deferred();
    const call = { command, args, ...result };
    calls.push(call);
    arrivals.get(command)?.shift()?.resolve(call);
    return result.promise;
  } });
  return { connection, calls, next };
}

async function active(f) {
  const arrival = f.next('connect');
  const done = f.connection.connect({ host: ' example.test ', name: 'Example', pin: '00123456' });
  (await arrival).resolve();
  assert.equal(await done, true);
  return f.connection.token();
}

test('PIN and address validation matches backend without losing leading zeros', () => {
  assert.deepEqual(validateConnection({ host: '  ', pin: '12' }), {
    ok: false, errors: { host: 'required', pin: 'invalid-pin' }
  });
  for (const pin of ['1', '123456', '123456789', '１２３４５６７８', '١٢٣٤٥٦٧٨', '1234 678', 'abcdefgh']) {
    assert.deepEqual(validateConnection({ host: 'host', pin }), { ok: false, errors: { pin: 'invalid-pin' } });
  }
  for (const host of ['::1', 'odd hostname', 'example.test']) {
    assert.deepEqual(validateConnection({ host: ` ${host} `, pin: ' 00123456 ' }), {
      ok: true, args: { host, tcpPort: 19730, udpPort: 19731, pin: '00123456' }
    });
    assert.deepEqual(validateConnection({ host, pin: '00123456', tcpPort: 19740, udpPort: 19741 }), {
      ok: true, args: { host, tcpPort: 19740, udpPort: 19741, pin: '00123456' }
    });
    assert.equal(validateConnection({ host, pin: ' ' }).args.pin, null);
    assert.equal(validateConnection({ host, pin: null }).args.pin, null);
  }
  for (const badTcp of [0, -1, 65536, 19730.5, 'abc', '19730.5']) {
    assert.deepEqual(
      validateConnection({ host: 'example.test', tcpPort: badTcp }),
      { ok: false, errors: { tcpPort: 'invalid-port' } }
    );
  }
  for (const badUdp of [0, -1, 65536, 19731.5, 'abc', '19731.5']) {
    assert.deepEqual(
      validateConnection({ host: 'example.test', udpPort: badUdp }),
      { ok: false, errors: { udpPort: 'invalid-port' } }
    );
  }
});

test('connect passes custom ports through invoke', async () => {
  const f = fixture();
  const arrival = f.next('connect');
  const done = f.connection.connect({
    host: 'example.test',
    name: 'Example',
    pin: '00123456',
    tcpPort: 19740,
    udpPort: 19741,
  });
  const call = await arrival;
  assert.equal(call.args.tcpPort, 19740);
  assert.equal(call.args.udpPort, 19741);
  call.resolve();
  assert.equal(await done, true);
});

test('invalid and unavailable connections never invoke; subscriptions and snapshots are isolated', async () => {
  const f = fixture();
  let changes = 0;
  const off = f.connection.subscribe(() => { changes++; });
  assert.equal(changes, 0);
  assert.equal(await f.connection.connect({ host: '', pin: 'bad' }), false);
  assert.equal(changes, 1);
  f.connection.snapshot().fieldErrors.host = 'changed';
  assert.equal(f.connection.snapshot().fieldErrors.host, 'required');
  off();
  await f.connection.connect({ host: '' });
  assert.equal(changes, 1);
  assert.equal(f.calls.length, 0);
  const absent = fixture({ nativeAvailable: false });
  assert.equal(await absent.connection.connect({ host: 'host' }), false);
  await absent.connection.disconnect();
  await absent.connection.refreshStats();
  assert.equal(absent.connection.snapshot().phase, 'unavailable');
  assert.equal(absent.calls.length, 0);
});

test('exact IPC, waiting-video then rendered frame; no PIN in state and busy blocks connect', async () => {
  const f = fixture();
  const token = await active(f);
  assert.deepEqual(f.calls[0].args, { host: 'example.test', tcpPort: 19730, udpPort: 19731, pin: '00123456' });
  assert.equal(f.connection.snapshot().phase, 'waiting-video');
  assert.equal(JSON.stringify(f.connection.snapshot()).includes('00123456'), false);
  const copy = f.connection.snapshot();
  copy.host.name = 'mutated';
  assert.equal(f.connection.snapshot().host.name, 'Example');
  assert.equal(await f.connection.connect({ host: 'other' }), false);
  assert.equal(f.connection.isCurrent(token), true);
  await f.connection.markFrameRendered(token - 1);
  assert.equal(f.connection.snapshot().phase, 'waiting-video');
  await f.connection.markFrameRendered(token);
  assert.equal(f.connection.snapshot().phase, 'streaming');
});

for (const outcome of ['resolve', 'reject']) {
  test(`cancel pending connect (${outcome}) invalidates immediately and serializes one final cleanup`, async () => {
    const release = deferred(), released = deferred();
    const f = fixture({ releaseInputs: () => { released.resolve(); return release.promise; } });
    const arrival = f.next('connect');
    const connecting = f.connection.connect({ host: 'A' });
    const native = await arrival;
    const before = f.connection.token();
    const teardown = f.next('disconnect');
    const canceled = f.connection.cancel();
    assert.notEqual(f.connection.token(), before);
    assert.equal(f.connection.snapshot().phase, 'disconnecting');
    assert.equal(f.connection.disconnect(), canceled);
    await released.promise;
    assert.equal(await f.connection.connect({ host: 'B' }), false);
    assert.deepEqual(f.calls.map(c => c.command), ['connect']);
    release.resolve();
    native[outcome](outcome === 'reject' ? new Error('late rejection') : undefined);
    const final = await teardown;
    assert.equal(f.connection.isCurrent(before), false);
    assert.equal(f.connection.snapshot().phase, 'disconnecting');
    assert.equal(await f.connection.connect({ host: 'B' }), false);
    final.resolve();
    await canceled;
    assert.equal(await connecting, true);
    assert.equal(f.connection.snapshot().phase, 'idle');
    assert.equal(f.connection.snapshot().busy, false);
    assert.equal(f.connection.snapshot().error, null);
    assert.deepEqual(f.calls.map(c => c.command), ['connect', 'disconnect']);
    const fresh = await active(f);
    assert.ok(fresh > before);
  });
}

test('pending connect settles before input release: teardown still waits for release', async () => {
  const release = deferred(), entered = deferred();
  const f = fixture({ releaseInputs: () => { entered.resolve(); return release.promise; } });
  const arrival = f.next('connect');
  const connecting = f.connection.connect({ host: 'host' });
  const native = await arrival;
  const canceled = f.connection.cancel();
  await entered.promise;
  native.resolve();
  await connecting;
  assert.deepEqual(f.calls.map(c => c.command), ['connect']);
  const teardown = f.next('disconnect');
  release.resolve();
  (await teardown).resolve();
  await canceled;
});

test('synchronous connecting subscription can cancel without losing native cleanup ownership', async () => {
  const f = fixture();
  const arrival = f.next('connect'), teardown = f.next('disconnect');
  let canceled, duplicate;
  const off = f.connection.subscribe(state => {
    if (state.phase === 'connecting') canceled = f.connection.cancel();
    if (state.phase === 'disconnecting') duplicate = f.connection.disconnect();
  });
  const connecting = f.connection.connect({ host: 'host' });
  assert.equal(f.connection.snapshot().phase, 'disconnecting');
  assert.equal(canceled, duplicate);
  assert.equal(await f.connection.connect({ host: 'blocked' }), false);
  (await arrival).resolve();
  (await teardown).resolve();
  await canceled;
  await connecting;
  off();
  assert.equal(f.connection.snapshot().phase, 'idle');
  assert.deepEqual(f.calls.map(c => c.command), ['connect', 'disconnect']);
});

test('completed input release cannot disconnect an unpublished pending connect', async () => {
  const release = deferred(), entered = deferred();
  const f = fixture({ releaseInputs: () => { entered.resolve(); return release.promise; } });
  const arrival = f.next('connect');
  const connecting = f.connection.connect({ host: 'host' });
  const native = await arrival;
  const teardown = f.next('disconnect');
  const canceled = f.connection.cancel();
  await entered.promise;
  release.resolve();
  // Await the exact release settlement, after cleanup's already-registered await.
  await release.promise;
  assert.deepEqual(f.calls.map(c => c.command), ['connect']);
  assert.equal(f.connection.snapshot().busy, true);
  native.resolve();
  (await teardown).resolve();
  await canceled;
  await connecting;
  assert.equal(f.connection.snapshot().busy, false);
});

test('connect rejection cleans up, retaining original error and retryable cleanup failure', async () => {
  const f = fixture();
  const arrival = f.next('connect'), teardown = f.next('disconnect');
  const connecting = f.connection.connect({ host: 'host' });
  (await arrival).reject(new Error('<unsafe> pairing denied'));
  (await teardown).reject(new Error('cleanup failed'));
  assert.equal(await connecting, true);
  let state = f.connection.snapshot();
  assert.equal(state.error, '<unsafe> pairing denied');
  assert.equal(state.cleanupError, 'cleanup failed');
  assert.equal(state.phase, 'error');
  assert.equal(state.busy, true);
  assert.equal(await f.connection.connect({ host: 'other' }), false);
  const retried = f.next('disconnect');
  const retry = f.connection.retryCleanup();
  assert.equal(f.connection.disconnect(), retry);
  (await retried).resolve();
  await retry;
  state = f.connection.snapshot();
  assert.equal(state.busy, false);
  assert.equal(state.host, null);
  assert.equal(state.cleanupError, null);
  assert.equal(state.error, '<unsafe> pairing denied');
  assert.deepEqual(f.calls.map(c => c.command), ['connect', 'disconnect', 'disconnect']);
});

test('release failure is visible, does not prevent teardown, and holds lock until retry', async () => {
  let releases = 0;
  const f = fixture({ releaseInputs: async () => { releases++; throw new Error('release failed'); } });
  await active(f);
  const arrival = f.next('disconnect');
  const done = f.connection.disconnect();
  (await arrival).resolve();
  await done;
  assert.equal(f.connection.snapshot().cleanupError, 'release failed');
  assert.equal(f.connection.snapshot().busy, true);
  const retryArrival = f.next('disconnect');
  const retry = f.connection.retryCleanup();
  (await retryArrival).resolve();
  await retry;
  assert.equal(releases, 1);
  assert.equal(f.connection.snapshot().busy, false);
});

test('stats deduplicate, clear unavailable values, and failure does not disconnect', async () => {
  const f = fixture();
  await active(f);
  const arrival = f.next('stats');
  const pending = f.connection.refreshStats();
  assert.equal(f.connection.refreshStats(), pending);
  (await arrival).resolve({ connected: true, latency_p50_ms: 4, latency_p99_ms: 8 });
  await pending;
  f.connection.snapshot().stats.latency_p50_ms = 99;
  assert.equal(f.connection.snapshot().stats.latency_p50_ms, 4);
  const next = f.next('stats');
  const nulls = f.connection.refreshStats();
  (await next).resolve({ connected: true, latency_p50_ms: null, latency_p99_ms: null });
  await nulls;
  assert.equal(f.connection.snapshot().stats.latency_p50_ms, null);
  const failed = f.next('stats');
  const rejected = f.connection.refreshStats();
  (await failed).reject(new Error('stats unavailable'));
  await rejected;
  assert.equal(f.connection.snapshot().stats, null);
  assert.equal(f.connection.snapshot().statsStatus, 'error');
  assert.equal(f.connection.snapshot().statsError, 'stats unavailable');
  assert.equal(f.connection.snapshot().phase, 'waiting-video');
});

for (const outcome of ['resolve', 'reject']) {
  test(`stale stats ${outcome} and stale frame errors cannot alter new generation`, async () => {
    const f = fixture();
    const oldToken = await active(f);
    const arrival = f.next('stats');
    const oldStats = f.connection.refreshStats();
    const oldNative = await arrival;
    const teardown = f.next('disconnect');
    const done = f.connection.disconnect();
    (await teardown).resolve();
    await done;
    const token = await active(f);
    const freshArrival = f.next('stats');
    const fresh = f.connection.refreshStats();
    const freshNative = await freshArrival;
    oldNative[outcome](outcome === 'reject' ? new Error('old failure') : { connected: false });
    await oldStats;
    assert.equal(f.connection.refreshStats(), fresh);
    await f.connection.reportFrameError(oldToken, new Error('stale draw'));
    await f.connection.markFrameRendered(oldToken);
    assert.equal(f.connection.snapshot().phase, 'waiting-video');
    freshNative.resolve({ connected: true, frames_decoded: 12 });
    await fresh;
    assert.equal(f.connection.snapshot().stats.frames_decoded, 12);
    assert.equal(f.connection.isCurrent(token), true);
  });
}

test('remote-ended stats and current frame failures use the cleanup path', async () => {
  for (const cause of ['stats', 'frame']) {
    const f = fixture();
    const token = await active(f);
    const teardown = f.next('disconnect');
    let done;
    if (cause === 'stats') {
      const arrival = f.next('stats');
      done = f.connection.refreshStats();
      (await arrival).resolve({ connected: false });
    } else {
      done = f.connection.reportFrameError(token, new Error('renderer failed'));
    }
    (await teardown).resolve();
    await done;
    assert.equal(f.connection.snapshot().phase, 'error');
    assert.equal(typeof f.connection.snapshot().error, 'string');
    assert.equal(f.connection.snapshot().busy, false);
    assert.equal(f.connection.isCurrent(token), false);
    assert.equal(f.connection.snapshot().stats, null);
  }
});

test('validateConnection accepts pairingId and passes through invoke', async () => {
  const parsed = validateConnection({ host: 'example.test', pairingId: 'explicit-pairing-id' });
  assert.equal(parsed.ok, true);
  assert.equal(parsed.args.pairingId, 'explicit-pairing-id');

  const f = fixture();
  const arrival = f.next('connect');
  const done = f.connection.connect({
    host: 'example.test',
    name: 'Example',
    pairingId: 'explicit-pairing-id',
  });
  const call = await arrival;
  assert.equal(call.args.pairingId, 'explicit-pairing-id');
  assert.equal(call.args.pin, null);
  call.resolve();
  assert.equal(await done, true);
});

