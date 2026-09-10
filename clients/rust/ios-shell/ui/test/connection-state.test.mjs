import test from 'node:test';
import assert from 'node:assert/strict';
import {
  validateConnectRequest,
  createConnectionManager
} from '../connection-state.js';

test('validateConnectRequest validates required host and optional 8-digit PIN', () => {
  const empty = validateConnectRequest({ host: '' });
  assert.equal(empty.ok, false);
  assert.equal(empty.field, 'host');

  const validWithoutPin = validateConnectRequest({ host: '192.168.1.10' });
  assert.equal(validWithoutPin.ok, true);
  assert.equal(validWithoutPin.args.host, '192.168.1.10');
  assert.equal(validWithoutPin.args.pin, null);

  const validWithPin = validateConnectRequest({ host: '192.168.1.10', pin: '12345678' });
  assert.equal(validWithPin.ok, true);
  assert.equal(validWithPin.args.pin, '12345678');

  const invalidPinShort = validateConnectRequest({ host: '192.168.1.10', pin: '1234' });
  assert.equal(invalidPinShort.ok, false);
  assert.equal(invalidPinShort.field, 'pin');

  const invalidPinLetters = validateConnectRequest({ host: '192.168.1.10', pin: 'abcdefgh' });
  assert.equal(invalidPinLetters.ok, false);
  assert.equal(invalidPinLetters.field, 'pin');
});

test('createConnectionManager transitions state from idle to waiting-video to streaming', async () => {
  let connectCalledWith = null;
  const mockInvoke = async (cmd, args) => {
    if (cmd === 'connect') {
      connectCalledWith = args;
      return { state: 'ready' };
    }
    if (cmd === 'disconnect') {
      return true;
    }
    return null;
  };

  const manager = createConnectionManager({ invoke: mockInvoke, hasNative: true });
  assert.equal(manager.getState(), 'idle');

  const ok = await manager.connect({ host: '10.0.0.1', pin: '87654321' });
  assert.equal(ok, true);
  assert.equal(connectCalledWith.host, '10.0.0.1');
  assert.equal(connectCalledWith.pin, '87654321');
  assert.equal(manager.getState(), 'waiting-video');

  const currentGen = manager.getGeneration();
  manager.markFrameRendered(currentGen);
  assert.equal(manager.getState(), 'streaming');

  await manager.disconnect();
  assert.equal(manager.getState(), 'idle');
});

test('createConnectionManager ignores stale frame tokens from prior generation', async () => {
  const manager = createConnectionManager({ invoke: async () => ({}), hasNative: true });
  await manager.connect({ host: '10.0.0.1' });
  assert.equal(manager.getState(), 'waiting-video');

  const oldGen = manager.getGeneration();
  await manager.disconnect();
  assert.equal(manager.getState(), 'idle');

  manager.markFrameRendered(oldGen);
  assert.equal(manager.getState(), 'idle');
});

test('createConnectionManager reports error when native bridge is missing', async () => {
  const manager = createConnectionManager({ invoke: null, hasNative: false });
  const ok = await manager.connect({ host: '10.0.0.1' });
  assert.equal(ok, false);
  assert.equal(manager.getState(), 'error');
  assert.match(manager.snapshot().lastError, /Mobile connection controls are unavailable/);
});

test('saved-mode click passes explicit pairingId to native connect without PIN', async () => {
  let connectCalledWith = null;
  const mockInvoke = async (cmd, args) => {
    if (cmd === 'connect') {
      connectCalledWith = args;
      return { state: 'ready' };
    }
    if (cmd === 'list_pairings') {
      return [
        {
          id: 'SAVED-ID-1234',
          hostName: 'MyMac',
          addedAtUnixMs: 1725900000000,
          lastEndpoint: { host: '192.168.1.100', tcpPort: 19730, udpPort: 19731 }
        }
      ];
    }
    return null;
  };

  const manager = createConnectionManager({ invoke: mockInvoke, hasNative: true });
  await manager.refreshPairings();

  const pairings = manager.snapshot().savedPairings;
  assert.equal(pairings.length, 1);
  assert.equal(pairings[0].id, 'SAVED-ID-1234');

  manager.selectPairing(pairings[0]);
  assert.equal(manager.getSelectedPairing().id, 'SAVED-ID-1234');

  // Trigger saved-mode connect (explicit saved ID without PIN)
  const ok = await manager.connectSavedPairing();
  assert.equal(ok, true);
  assert.equal(connectCalledWith.host, '192.168.1.100');
  assert.equal(connectCalledWith.tcpPort, 19730);
  assert.equal(connectCalledWith.udpPort, 19731);
  assert.equal(connectCalledWith.pairingId, 'SAVED-ID-1234');
  assert.equal(connectCalledWith.pin, null);
});

test('list_pairings populates savedPairings and excludes secret keys from state', async () => {
  const mockInvoke = async (cmd) => {
    if (cmd === 'list_pairings') {
      return [
        {
          id: 'ID-ALPHA',
          hostName: 'HostAlpha',
          addedAtUnixMs: 1725900000000,
          lastEndpoint: { host: '10.0.0.5', tcpPort: 19730, udpPort: 19731 }
        },
        {
          id: 'ID-BETA',
          hostName: 'HostBeta',
          addedAtUnixMs: 1725900005000,
          lastEndpoint: null
        }
      ];
    }
    return null;
  };

  const manager = createConnectionManager({ invoke: mockInvoke, hasNative: true });
  const list = await manager.refreshPairings();
  assert.equal(list.length, 2);

  const snap = manager.snapshot();
  assert.equal(snap.savedPairings.length, 2);
  assert.equal(snap.savedPairings[0].id, 'ID-ALPHA');
  assert.equal(snap.savedPairings[1].id, 'ID-BETA');

  // Verify zero secret fields leaked
  for (const p of snap.savedPairings) {
    assert.equal(p.key, undefined, 'Symmetric key must not be present in PairingSummary');
    assert.equal(Object.keys(p).includes('key'), false, 'Key field must not exist in public metadata');
  }
});

test('connect without PIN and without pairingId returns pairing-required error', async () => {
  const mockInvoke = async (cmd, args) => {
    if (cmd === 'connect') {
      if (!args.pin && !args.pairingId) {
        const err = new Error('PIN required for initial authorization');
        err.code = 'pairing-required';
        err.stage = 'preauth';
        err.retryable = false;
        throw err;
      }
      return { state: 'ready' };
    }
    return null;
  };

  const manager = createConnectionManager({ invoke: mockInvoke, hasNative: true });
  const ok = await manager.connect({ host: '192.168.1.50' });
  assert.equal(ok, false);
  assert.equal(manager.getState(), 'error');
  assert.match(manager.snapshot().lastError, /PIN required/);
});

test('UI saved-mode click to actual invoke argument passes exact pairingId without PIN', async () => {
  let actualConnectArgs = null;
  const mockInvoke = async (cmd, args) => {
    if (cmd === 'list_pairings') {
      return [
        {
          id: 'SAVED-PAIRING-UUID-999',
          hostName: 'StudioMac',
          addedAtUnixMs: 1725910000000,
          lastEndpoint: { host: '192.168.1.77', tcpPort: 19730, udpPort: 19731 }
        }
      ];
    }
    if (cmd === 'connect') {
      actualConnectArgs = args;
      return { state: 'ready' };
    }
    return null;
  };

  const manager = createConnectionManager({ invoke: mockInvoke, hasNative: true });
  await manager.refreshPairings();

  // Simulate UI DOM card creation and click:
  const snap = manager.snapshot();
  const pairing = snap.savedPairings[0];
  assert.equal(pairing.id, 'SAVED-PAIRING-UUID-999');

  // User clicks saved pairing card:
  manager.selectPairing(pairing);
  assert.equal(manager.getSelectedPairing().id, 'SAVED-PAIRING-UUID-999');

  // User clicks Connect button:
  const ok = await manager.connectSavedPairing();
  assert.equal(ok, true);

  // Assert actual invoke argument passed to native backend:
  assert.notEqual(actualConnectArgs, null);
  assert.equal(actualConnectArgs.host, '192.168.1.77');
  assert.equal(actualConnectArgs.tcpPort, 19730);
  assert.equal(actualConnectArgs.udpPort, 19731);
  assert.equal(actualConnectArgs.pairingId, 'SAVED-PAIRING-UUID-999');
  assert.equal(actualConnectArgs.pin, null);
});

test('startup provisioning auto-connect dispatches connect with explicit pairingId', async () => {
  let connectCalledWith = null;
  const mockInvoke = async (cmd, args) => {
    if (cmd === 'startup') {
      return {
        host: '192.168.1.188',
        tcpPort: 29730,
        udpPort: 29731,
        pairingId: 'QA-PROVISIONED-UNIQUE-UUID',
        autoConnect: true
      };
    }
    if (cmd === 'connect') {
      connectCalledWith = args;
      return { state: 'ready' };
    }
    return null;
  };

  const manager = createConnectionManager({ invoke: mockInvoke, hasNative: true });

  // Simulate startup response consumed by app controller
  const startupInfo = await mockInvoke('startup');
  assert.equal(startupInfo.autoConnect, true);
  assert.equal(startupInfo.pairingId, 'QA-PROVISIONED-UNIQUE-UUID');

  const req = {
    host: startupInfo.host,
    tcpPort: startupInfo.tcpPort,
    udpPort: startupInfo.udpPort,
    pairingId: startupInfo.pairingId,
    pin: null
  };

  const ok = await manager.connect(req);
  assert.equal(ok, true);
  assert.notEqual(connectCalledWith, null);
  assert.equal(connectCalledWith.host, '192.168.1.188');
  assert.equal(connectCalledWith.tcpPort, 29730);
  assert.equal(connectCalledWith.udpPort, 29731);
  assert.equal(connectCalledWith.pairingId, 'QA-PROVISIONED-UNIQUE-UUID');
  assert.equal(connectCalledWith.pin, null);
});

// ============================================================================
// Phase D (R6): Lifecycle, Independent Resource Ownership & Cleanup Guarantees
// ============================================================================

const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
};

function createTaskBarrier() {
  const channel = new MessageChannel();
  let onDone;
  const promise = new Promise((resolve) => {
    onDone = resolve;
  });
  channel.port2.onmessage = () => {
    onDone();
  };
  channel.port1.postMessage(null);
  return {
    wait: () => promise,
    close: () => {
      channel.port1.close();
      channel.port2.close();
    }
  };
}

function createLifecycleFixture(options = {}) {
  const calls = [];
  const arrivals = new Map();

  function next(command) {
    const signal = deferred();
    const queue = arrivals.get(command) || [];
    queue.push(signal);
    arrivals.set(command, queue);
    return signal.promise;
  }

  const invoke = async (command, args) => {
    const result = deferred();
    const call = { command, args, ...result };
    calls.push(call);
    const queue = arrivals.get(command);
    if (queue && queue.length > 0) {
      queue.shift().resolve(call);
    }
    return result.promise;
  };

  const manager = createConnectionManager({
    invoke,
    hasNative: options.hasNative !== false,
    ...options
  });

  return {
    manager,
    calls,
    next,
    cleanup() {
      for (const queue of arrivals.values()) {
        while (queue.length > 0) {
          queue.shift().reject(new Error('fixture cleaned up'));
        }
      }
      arrivals.clear();
      for (const call of calls) {
        call.resolve(null);
      }
    }
  };
}

test('cleanup invokes native disconnect even when state is error', async () => {
  const f = createLifecycleFixture();
  const connectArrival = f.next('connect');
  const connectPromise = f.manager.connect({ host: '192.168.1.10', pin: '12345678' });
  (await connectArrival).resolve({ state: 'ready' });
  assert.equal(await connectPromise, true);
  assert.equal(f.manager.getState(), 'waiting-video');

  // Terminal stats triggers immediate cleanup ownership:
  const statsArrival = f.next('stats');
  const teardownArrival = f.next('disconnect');
  const pollPromise = f.manager.pollStats();
  (await statsArrival).resolve({ state: 'error', last_error: 'fixture audio error' });
  (await teardownArrival).resolve();
  await pollPromise;

  assert.equal(f.calls.some((c) => c.command === 'disconnect'), true);
  assert.equal(f.manager.getState(), 'error');
  assert.equal(f.manager.snapshot().lastError, 'fixture audio error');
  assert.equal(f.manager.snapshot().hasOwnedSession, false);
  assert.equal(f.manager.snapshot().busy, false);

  // Subsequent disconnect resets error state to idle:
  await f.manager.disconnect();
  assert.equal(f.manager.getState(), 'idle');
});

test('cleanup failure transitions to cleanup-failed and does not revert to idle', async () => {
  const f = createLifecycleFixture();
  const connectArrival = f.next('connect');
  const connectPromise = f.manager.connect({ host: '192.168.1.10', pin: '12345678' });
  (await connectArrival).resolve({ state: 'ready' });
  assert.equal(await connectPromise, true);

  const teardownArrival = f.next('disconnect');
  const disconnectPromise = f.manager.disconnect();
  (await teardownArrival).reject(new Error('native teardown failed'));
  await disconnectPromise;

  assert.equal(f.manager.getState(), 'cleanup-failed');
  assert.notEqual(f.manager.getState(), 'idle');
  assert.equal(f.manager.snapshot().cleanupError, 'native teardown failed');
  assert.equal(f.manager.snapshot().hasOwnedSession, true);
  assert.equal(f.manager.snapshot().busy, true);
});

test('concurrent disconnect calls share single pending cleanup promise', async () => {
  const f = createLifecycleFixture();
  const connectArrival = f.next('connect');
  const connectPromise = f.manager.connect({ host: '192.168.1.10', pin: '12345678' });
  (await connectArrival).resolve({ state: 'ready' });
  await connectPromise;

  const teardownArrival = f.next('disconnect');
  const p1 = f.manager.disconnect();
  const p2 = f.manager.disconnect();
  const p3 = f.manager.disconnect();

  assert.equal(p1, p2, 'Concurrent disconnect calls must return identical pending cleanup promise');
  assert.equal(p2, p3, 'Concurrent disconnect calls must return identical pending cleanup promise');

  (await teardownArrival).resolve();
  await p1;

  const disconnectCalls = f.calls.filter((c) => c.command === 'disconnect');
  assert.equal(disconnectCalls.length, 1, 'Only one native disconnect invoke must be executed');
  assert.equal(f.manager.getState(), 'idle');
});

test('prompt native disconnect during pending connect cancels immediately and serializes cleanup', async () => {
  const f = createLifecycleFixture();
  const connectArrival = f.next('connect');
  const connecting = f.manager.connect({ host: '192.168.1.100', pin: '12345678' });
  const connectCall = await connectArrival;
  const beforeGen = f.manager.getGeneration();

  // Cancel while connect is still pending:
  const teardownArrival = f.next('disconnect');
  const cancelPromise = f.manager.disconnect();

  // Assert native disconnect is called promptly without waiting for connect to finish:
  const teardownCall = await teardownArrival;
  assert.notEqual(teardownCall, null);
  assert.equal(f.manager.getState(), 'disconnecting');
  assert.notEqual(f.manager.getGeneration(), beforeGen);

  // Now settle connect and disconnect:
  connectCall.reject(new Error('cancelled'));
  teardownCall.resolve();
  await cancelPromise;
  const connectResult = await connecting;

  assert.equal(connectResult, false);
  assert.equal(f.manager.getState(), 'idle');
  assert.equal(f.manager.snapshot().hasOwnedSession, false);
  assert.deepEqual(f.calls.filter((c) => c.command === 'connect' || c.command === 'disconnect').map((c) => c.command), ['connect', 'disconnect']);
});

test('cleanup-failed retains lock and blocks connect until retryCleanup succeeds', async () => {
  const f = createLifecycleFixture();
  const connectArrival = f.next('connect');
  const connectPromise = f.manager.connect({ host: '10.0.0.1', pin: '12345678' });
  (await connectArrival).resolve({ state: 'ready' });
  await connectPromise;

  const teardownArrival = f.next('disconnect');
  const disconnectPromise = f.manager.disconnect();
  (await teardownArrival).reject(new Error('native lock stuck'));
  await disconnectPromise;

  assert.equal(f.manager.getState(), 'cleanup-failed');

  // Attempting to connect while cleanup-failed must be blocked:
  const blockedConnect = await f.manager.connect({ host: '10.0.0.2', pin: '12345678' });
  assert.equal(blockedConnect, false);
  assert.equal(f.calls.filter((c) => c.command === 'connect').length, 1, 'No new native connect permitted');

  // Retry cleanup succeeds:
  const retryArrival = f.next('disconnect');
  const retryPromise = f.manager.retryCleanup();
  (await retryArrival).resolve();
  await retryPromise;

  assert.equal(f.manager.getState(), 'idle');
  assert.equal(f.manager.snapshot().hasOwnedSession, false);
  assert.equal(f.manager.snapshot().cleanupError, null);

  // Subsequent connect is now unlocked:
  const freshConnectArrival = f.next('connect');
  const freshConnectPromise = f.manager.connect({ host: '10.0.0.2', pin: '12345678' });
  (await freshConnectArrival).resolve({ state: 'ready' });
  assert.equal(await freshConnectPromise, true);
  assert.equal(f.manager.getState(), 'waiting-video');
});

test('connect rejection cleans up, retaining original error and retryable cleanup failure', async () => {
  const f = createLifecycleFixture();
  const connectArrival = f.next('connect');
  const teardownArrival = f.next('disconnect');

  const connectPromise = f.manager.connect({ host: '10.0.0.1', pin: '12345678' });
  (await connectArrival).reject(new Error('Pairing rejected by host'));
  (await teardownArrival).reject(new Error('teardown io failure'));

  const ok = await connectPromise;
  assert.equal(ok, false);
  assert.equal(f.manager.getState(), 'cleanup-failed');
  assert.equal(f.manager.snapshot().lastError, 'Pairing rejected by host', 'Original connect error must be retained');
  assert.equal(f.manager.snapshot().cleanupError, 'teardown io failure', 'Cleanup failure must be recorded');
  assert.equal(f.manager.snapshot().hasOwnedSession, true, 'Resource lock retained');

  // Retry cleanup releases resource lock while preserving original error:
  const retryArrival = f.next('disconnect');
  const retryPromise = f.manager.retryCleanup();
  (await retryArrival).resolve();
  await retryPromise;

  assert.equal(f.manager.snapshot().hasOwnedSession, false);
  assert.equal(f.manager.snapshot().cleanupError, null);
  assert.equal(f.manager.snapshot().lastError, 'Pairing rejected by host');
});

test('ensure error dismiss cleans resources and never reverts to idle on cleanup failure', async () => {
  const f = createLifecycleFixture();
  const connectArrival = f.next('connect');
  const connectPromise = f.manager.connect({ host: '10.0.0.1', pin: '12345678' });
  (await connectArrival).resolve({ state: 'ready' });
  await connectPromise;

  // Session enters error state via stats with initial teardown failure:
  const statsArrival = f.next('stats');
  const teardownArrival1 = f.next('disconnect');
  const pollPromise = f.manager.pollStats();
  (await statsArrival).resolve({ state: 'error', last_error: 'UDP stream corrupted' });
  (await teardownArrival1).reject(new Error('cleanup rejected'));
  await pollPromise;

  assert.equal(f.manager.getState(), 'cleanup-failed');
  assert.equal(f.manager.snapshot().hasOwnedSession, true);

  // Case A: Dismiss error when cleanup fails again -> must NOT revert to idle!
  const teardownArrival2 = f.next('disconnect');
  const dismissPromise1 = f.manager.dismissError();
  (await teardownArrival2).reject(new Error('cleanup rejected again'));
  await dismissPromise1;

  assert.equal(f.manager.getState(), 'cleanup-failed');
  assert.notEqual(f.manager.getState(), 'idle');
  assert.equal(f.manager.snapshot().hasOwnedSession, true);

  // Case B: Retry dismiss error when cleanup succeeds -> transitions to idle:
  const teardownArrival3 = f.next('disconnect');
  const dismissPromise2 = f.manager.dismissError();
  (await teardownArrival3).resolve();
  await dismissPromise2;

  assert.equal(f.manager.getState(), 'idle');
  assert.equal(f.manager.snapshot().hasOwnedSession, false);
  assert.equal(f.manager.snapshot().lastError, null);
});

test('stale connect completion after disconnect cannot revive session', async () => {
  const f = createLifecycleFixture();
  const connectArrival = f.next('connect');
  const connectPromise = f.manager.connect({ host: '10.0.0.1', pin: '12345678' });
  const connectCall = await connectArrival;

  const teardownArrival = f.next('disconnect');
  const disconnectPromise = f.manager.disconnect();

  // Late resolution of connect after disconnect began:
  connectCall.resolve({ state: 'ready' });
  (await teardownArrival).resolve();
  await disconnectPromise;
  await connectPromise;

  assert.equal(f.manager.getState(), 'idle');
  assert.notEqual(f.manager.getState(), 'waiting-video');
  assert.equal(f.manager.snapshot().hasOwnedSession, false);
});

test('lead_disconnect_waits_for_pending_connect', async () => {
  const f = createLifecycleFixture();
  const barrier = createTaskBarrier();
  try {
    const connectArrival = f.next('connect');
    const connecting = f.manager.connect({ host: '192.0.2.10', pairingId: 'lead-deferred' });
    const connectCall = await connectArrival;

    let cleanupSettled = false;
    const teardownArrival = f.next('disconnect');
    const disconnectPromise = f.manager.disconnect().then(() => {
      cleanupSettled = true;
    });

    // Native disconnect resolves immediately while connect is still deferred:
    const teardownCall = await teardownArrival;
    teardownCall.resolve();

    // Event-loop task barrier ensures all queued Promise reactions drain first:
    await barrier.wait();

    // In unpatched code with Promise.race([connectToSettle, Promise.resolve()]),
    // cleanupSettled is true and state is 'idle' prematurely.
    // PASS requires cleanup not settled and state disconnecting until connect settles:
    assert.equal(cleanupSettled, false, 'Cleanup must not settle while connect is still pending');
    assert.equal(f.manager.getState(), 'disconnecting', 'State must remain disconnecting');
    assert.equal(f.manager.snapshot().busy, true, 'Snapshot busy must remain true');

    // Now resolve native connect:
    connectCall.resolve({ state: 'ready' });
    await disconnectPromise;
    const connectResult = await connecting;

    assert.equal(connectResult, false, 'Cancelled connect must resolve to false');
    assert.equal(cleanupSettled, true, 'Cleanup must settle after connect settles');
    assert.equal(f.manager.getState(), 'idle');
    assert.equal(f.manager.snapshot().busy, false);
    assert.equal(f.manager.snapshot().hasOwnedSession, false);
  } finally {
    barrier.close();
    f.cleanup();
  }
});

test('lead_disconnected_stats_end_ui_session', async () => {
  const f = createLifecycleFixture();
  try {
    const connectArrival = f.next('connect');
    const connectPromise = f.manager.connect({ host: '192.0.2.10', pairingId: 'lead-terminal' });
    (await connectArrival).resolve({ state: 'ready' });
    assert.equal(await connectPromise, true);
    assert.equal(f.manager.getState(), 'waiting-video');

    const statsArrival = f.next('stats');
    const teardownArrival = f.next('disconnect');
    const pollPromise = f.manager.pollStats();

    // Native stats produces disconnected with last_error remote-closed:
    (await statsArrival).resolve({ state: 'disconnected', last_error: 'remote-closed' });
    (await teardownArrival).resolve();
    await pollPromise;

    assert.notEqual(f.manager.getState(), 'waiting-video', 'Must leave waiting-video upon terminal stats');
    assert.equal(f.manager.getState(), 'error');
    assert.equal(f.manager.snapshot().lastError, 'remote-closed', 'Must retain terminal reason');
    assert.equal(f.manager.snapshot().hasOwnedSession, false, 'Must release owned session');
    assert.equal(f.manager.snapshot().busy, false, 'Must report busy=false');
    assert.equal(f.calls.some((c) => c.command === 'disconnect'), true, 'Must execute native disconnect');
  } finally {
    f.cleanup();
  }
});

test('disconnect during pending connect: order A where connect settles before disconnect', async () => {
  const f = createLifecycleFixture();
  const barrier = createTaskBarrier();
  try {
    const connectArrival = f.next('connect');
    const connecting = f.manager.connect({ host: '10.0.0.1', pin: '12345678' });
    const connectCall = await connectArrival;

    let cleanupSettled = false;
    const teardownArrival = f.next('disconnect');
    const disconnectPromise = f.manager.disconnect().then(() => {
      cleanupSettled = true;
    });

    const teardownCall = await teardownArrival;

    // Order A: connect settles FIRST
    connectCall.reject(new Error('cancelled'));
    await barrier.wait();

    assert.equal(cleanupSettled, false, 'Cleanup must not finish until disconnect also settles');
    assert.equal(f.manager.getState(), 'disconnecting');
    assert.equal(f.manager.snapshot().busy, true);

    // Then disconnect settles:
    teardownCall.resolve();
    await disconnectPromise;
    const connectResult = await connecting;

    assert.equal(connectResult, false);
    assert.equal(cleanupSettled, true);
    assert.equal(f.manager.getState(), 'idle');
    assert.equal(f.manager.snapshot().busy, false);
    assert.equal(f.manager.snapshot().hasOwnedSession, false);
  } finally {
    barrier.close();
    f.cleanup();
  }
});

test('disconnect during pending connect: order B where disconnect settles before connect', async () => {
  const f = createLifecycleFixture();
  const barrier = createTaskBarrier();
  try {
    const connectArrival = f.next('connect');
    const connecting = f.manager.connect({ host: '10.0.0.1', pin: '12345678' });
    const connectCall = await connectArrival;

    let cleanupSettled = false;
    const teardownArrival = f.next('disconnect');
    const disconnectPromise = f.manager.disconnect().then(() => {
      cleanupSettled = true;
    });

    // Order B: disconnect settles FIRST
    const teardownCall = await teardownArrival;
    teardownCall.resolve();
    await barrier.wait();

    assert.equal(cleanupSettled, false, 'Cleanup must not finish while connect is still pending');
    assert.equal(f.manager.getState(), 'disconnecting');
    assert.equal(f.manager.snapshot().busy, true);

    // Then connect settles:
    connectCall.reject(new Error('cancelled'));
    await disconnectPromise;
    const connectResult = await connecting;

    assert.equal(connectResult, false);
    assert.equal(cleanupSettled, true);
    assert.equal(f.manager.getState(), 'idle');
    assert.equal(f.manager.snapshot().busy, false);
    assert.equal(f.manager.snapshot().hasOwnedSession, false);
  } finally {
    barrier.close();
    f.cleanup();
  }
});

test('native stats contract: stale generation terminal stats is suppressed', async () => {
  const f = createLifecycleFixture();
  try {
    const connectArrival1 = f.next('connect');
    const connecting1 = f.manager.connect({ host: '10.0.0.1', pin: '12345678' });
    (await connectArrival1).resolve({ state: 'ready' });
    await connecting1;

    // Armed pollStats in generation 1:
    const statsArrival = f.next('stats');
    const pollPromise = f.manager.pollStats();
    const statsCall = await statsArrival;

    // Disconnect advances generation to 2:
    const teardownArrival = f.next('disconnect');
    const disconnectPromise = f.manager.disconnect();
    (await teardownArrival).resolve();
    await disconnectPromise;
    assert.equal(f.manager.getState(), 'idle');

    // Stale stats from generation 1 resolves late with terminal disconnected:
    statsCall.resolve({ state: 'disconnected', last_error: 'stale-remote-closed' });
    await pollPromise;

    // Must not alter idle state or set error:
    assert.equal(f.manager.getState(), 'idle', 'Stale stats must not alter state');
    assert.equal(f.manager.snapshot().lastError, null, 'Stale stats must not set lastError');
  } finally {
    f.cleanup();
  }
});

test('final subscription snapshot reports busy=false when ownership and connect truly ended', async () => {
  const f = createLifecycleFixture();
  const snapshots = [];
  const unsubscribe = f.manager.subscribe((snap) => {
    snapshots.push({ ...snap });
  });

  try {
    const connectArrival = f.next('connect');
    const connecting = f.manager.connect({ host: '10.0.0.1', pin: '12345678' });
    (await connectArrival).resolve({ state: 'ready' });
    await connecting;

    const teardownArrival = f.next('disconnect');
    const disconnectPromise = f.manager.disconnect();
    (await teardownArrival).resolve();
    await disconnectPromise;

    assert.ok(snapshots.length >= 2, 'Must emit snapshots across lifecycle');
    const lastSnap = snapshots[snapshots.length - 1];
    assert.equal(lastSnap.busy, false, 'Final snapshot must report busy=false');
    assert.equal(lastSnap.hasOwnedSession, false, 'Final snapshot must report hasOwnedSession=false');
    assert.equal(lastSnap.state, 'idle', 'Final snapshot must report state=idle');
  } finally {
    unsubscribe();
    f.cleanup();
  }
});

test('resource ownership: disconnect during streaming maintains owned session and busy until native invoke completes', async () => {
  const f = createLifecycleFixture();
  try {
    const connectArrival = f.next('connect');
    const connectPromise = f.manager.connect({ host: '10.0.0.1', pin: '12345678' });
    (await connectArrival).resolve({ state: 'ready' });
    await connectPromise;

    const currentGen = f.manager.getGeneration();
    f.manager.markFrameRendered(currentGen);
    assert.equal(f.manager.getState(), 'streaming');

    const teardownArrival = f.next('disconnect');
    const disconnectPromise = f.manager.disconnect();
    const teardownCall = await teardownArrival;

    // While native disconnect is in flight:
    assert.equal(f.manager.getState(), 'disconnecting');
    assert.equal(f.manager.snapshot().hasOwnedSession, true, 'hasOwnedSession must remain true during in-flight disconnect');
    assert.equal(f.manager.snapshot().busy, true, 'busy must remain true during in-flight disconnect');

    teardownCall.resolve();
    await disconnectPromise;

    assert.equal(f.manager.getState(), 'idle');
    assert.equal(f.manager.snapshot().hasOwnedSession, false, 'hasOwnedSession must be false after disconnect completes');
    assert.equal(f.manager.snapshot().busy, false, 'busy must be false after disconnect completes');
  } finally {
    f.cleanup();
  }
});

test('concurrent disconnect, cancel, dismissError, and retryCleanup share identical in-flight cleanup promise', async () => {
  const f = createLifecycleFixture();
  try {
    const connectArrival = f.next('connect');
    const connectPromise = f.manager.connect({ host: '10.0.0.1', pin: '12345678' });
    (await connectArrival).resolve({ state: 'ready' });
    await connectPromise;

    const teardownArrival = f.next('disconnect');
    const p1 = f.manager.disconnect();
    const p2 = f.manager.cancel();
    const p3 = f.manager.dismissError();
    const p4 = f.manager.retryCleanup();

    assert.equal(p1, p2, 'All concurrent cleanup callers must receive identical promise reference');
    assert.equal(p2, p3, 'All concurrent cleanup callers must receive identical promise reference');
    assert.equal(p3, p4, 'All concurrent cleanup callers must receive identical promise reference');

    const teardownCall = await teardownArrival;
    teardownCall.resolve();
    await p1;

    const disconnectCalls = f.calls.filter((c) => c.command === 'disconnect');
    assert.equal(disconnectCalls.length, 1, 'Only one native disconnect invoke must be executed');
    assert.equal(f.manager.getState(), 'idle');
  } finally {
    f.cleanup();
  }
});

test('stale connect rejection after disconnect cannot transition state to error or overwrite lastError', async () => {
  const f = createLifecycleFixture();
  try {
    const connectArrival = f.next('connect');
    const connecting = f.manager.connect({ host: '10.0.0.1', pin: '12345678' });
    const connectCall = await connectArrival;

    const teardownArrival = f.next('disconnect');
    const disconnectPromise = f.manager.disconnect();
    const teardownCall = await teardownArrival;

    // Native disconnect settles first:
    teardownCall.resolve();

    // Stale connect rejects with network error while disconnect is settling:
    connectCall.reject(new Error('late connection timeout'));

    await disconnectPromise;
    const connectResult = await connecting;

    assert.equal(connectResult, false, 'Stale connect must return false');
    assert.equal(f.manager.getState(), 'idle', 'Stale connect rejection must not alter state to error');
    assert.equal(f.manager.snapshot().lastError, null, 'Stale connect rejection must not set lastError');
    assert.equal(f.manager.snapshot().hasOwnedSession, false, 'hasOwnedSession must remain false');
  } finally {
    f.cleanup();
  }
});

test('saved pairing connection rejection preserves exact failure reason without PIN fallback or duplicate retry', async () => {
  let actualCalls = [];
  const mockInvoke = async (cmd, args) => {
    actualCalls.push({ cmd, args });
    if (cmd === 'list_pairings') {
      return [
        {
          id: 'EXACT-SAVED-ID-777',
          hostName: 'TestHost',
          addedAtUnixMs: 1725900000000,
          lastEndpoint: { host: '192.168.1.88', tcpPort: 19730, udpPort: 19731 }
        }
      ];
    }
    if (cmd === 'connect') {
      const err = new Error('Pairing key revoked by host');
      err.code = 'credential-rejected';
      err.stage = 'tls-psk';
      err.retryable = false;
      throw err;
    }
    if (cmd === 'disconnect') {
      return true;
    }
    return null;
  };

  const manager = createConnectionManager({ invoke: mockInvoke, hasNative: true });
  await manager.refreshPairings();

  const pairing = manager.snapshot().savedPairings[0];
  assert.equal(pairing.id, 'EXACT-SAVED-ID-777');
  manager.selectPairing(pairing);

  // Attempt connect with saved pairing:
  const ok = await manager.connectSavedPairing();
  assert.equal(ok, false, 'Connect must return false on rejection');

  // Verify exact arguments sent to native connect:
  const connectCalls = actualCalls.filter((c) => c.cmd === 'connect');
  assert.equal(connectCalls.length, 1, 'Saved pairing failure must never trigger second connect attempt with PIN');
  assert.equal(connectCalls[0].args.pairingId, 'EXACT-SAVED-ID-777');
  assert.equal(connectCalls[0].args.pin, null);

  // State must be error with exact original error message:
  assert.equal(manager.getState(), 'error');
  assert.match(manager.snapshot().lastError, /Pairing key revoked by host/);
  assert.equal(manager.snapshot().hasOwnedSession, false);
  assert.equal(manager.snapshot().busy, false);
});





