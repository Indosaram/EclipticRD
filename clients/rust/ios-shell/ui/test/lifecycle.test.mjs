import test from 'node:test';
import assert from 'node:assert/strict';
import { attachLifecycle } from '../lifecycle.js';
import { createConnectionManager } from '../connection-state.js';

function waitForState(connection, targetState) {
  if (connection.getState() === targetState) {
    return Promise.resolve();
  }
  return new Promise((resolve) => {
    const unsubscribe = connection.subscribe((snap) => {
      if (snap.state === targetState) {
        unsubscribe();
        resolve();
      }
    });
  });
}

test('hidden during streaming invokes input release, disconnect, and invalidates generation', async () => {
  const windowTarget = new EventTarget();
  const documentTarget = new EventTarget();
  documentTarget.hidden = false;

  let disconnectCalled = false;
  const connection = createConnectionManager({
    invoke: async (cmd) => {
      if (cmd === 'connect') return { state: 'ready' };
      if (cmd === 'disconnect') {
        disconnectCalled = true;
        return true;
      }
      return null;
    },
    hasNative: true
  });

  await connection.connect({ host: '192.168.1.10' });
  const gen1 = connection.getGeneration();
  connection.markFrameRendered(gen1);
  assert.equal(connection.getState(), 'streaming');

  let inputsReleased = false;
  let pollingStopped = false;
  let onDisconnectCalled = false;
  attachLifecycle({
    windowTarget,
    documentTarget,
    connection,
    releaseInputs: () => {
      inputsReleased = true;
    },
    onStopPolling: () => {
      pollingStopped = true;
    },
    onDisconnect: () => {
      onDisconnectCalled = true;
      disconnectCalled = true;
    }
  });

  const idlePromise = waitForState(connection, 'idle');
  documentTarget.hidden = true;
  documentTarget.dispatchEvent(new Event('visibilitychange'));
  await idlePromise;

  assert.equal(inputsReleased, true);
  assert.equal(onDisconnectCalled, true);
  assert.equal(disconnectCalled, true);
  assert.equal(pollingStopped, true);
  assert.equal(connection.getState(), 'idle');
  assert.notEqual(connection.getGeneration(), gen1);
  assert.equal(connection.isCurrent(gen1), false);
});

test('hidden during connecting invokes input release and disconnect', async () => {
  const windowTarget = new EventTarget();
  const documentTarget = new EventTarget();
  documentTarget.hidden = false;

  let disconnectCalled = false;
  let settleConnect;
  const connection = createConnectionManager({
    invoke: (cmd) => {
      if (cmd === 'connect') {
        return new Promise((resolve, reject) => {
          settleConnect = reject;
        });
      }
      if (cmd === 'disconnect') {
        disconnectCalled = true;
        if (settleConnect) {
          settleConnect(new Error('cancelled'));
        }
        return Promise.resolve(true);
      }
      return Promise.resolve(null);
    },
    hasNative: true
  });

  connection.connect({ host: '192.168.1.10' });
  assert.equal(connection.getState(), 'connecting');
  const gen = connection.getGeneration();

  let inputsReleased = false;
  let pollingStopped = false;
  let nativeDisconnectCalled = false;
  attachLifecycle({
    windowTarget,
    documentTarget,
    connection,
    releaseInputs: () => {
      inputsReleased = true;
    },
    onStopPolling: () => {
      pollingStopped = true;
    },
    onDisconnect: () => {
      nativeDisconnectCalled = true;
      disconnectCalled = true;
    }
  });

  const idlePromise = waitForState(connection, 'idle');
  documentTarget.hidden = true;
  documentTarget.dispatchEvent(new Event('visibilitychange'));
  await idlePromise;

  assert.equal(inputsReleased, true);
  assert.equal(nativeDisconnectCalled, true);
  assert.equal(disconnectCalled, true);
  assert.equal(pollingStopped, true);
  assert.notEqual(connection.getGeneration(), gen);
  assert.equal(connection.isCurrent(gen), false);
});

test('pagehide during streaming invokes input release and disconnect', async () => {
  const windowTarget = new EventTarget();
  const documentTarget = new EventTarget();
  documentTarget.hidden = false;

  let disconnectCalled = false;
  const connection = createConnectionManager({
    invoke: async (cmd) => {
      if (cmd === 'connect') return { state: 'ready' };
      if (cmd === 'disconnect') {
        disconnectCalled = true;
        return true;
      }
      return null;
    },
    hasNative: true
  });

  await connection.connect({ host: '192.168.1.10' });
  const gen = connection.getGeneration();
  connection.markFrameRendered(gen);
  assert.equal(connection.getState(), 'streaming');

  let inputsReleased = false;
  let pollingStopped = false;
  let onDisconnectCalled = false;
  attachLifecycle({
    windowTarget,
    documentTarget,
    connection,
    releaseInputs: () => {
      inputsReleased = true;
    },
    onStopPolling: () => {
      pollingStopped = true;
    },
    onDisconnect: () => {
      onDisconnectCalled = true;
      disconnectCalled = true;
    }
  });

  const idlePromise = waitForState(connection, 'idle');
  windowTarget.dispatchEvent(new Event('pagehide'));
  await idlePromise;

  assert.equal(inputsReleased, true);
  assert.equal(onDisconnectCalled, true);
  assert.equal(disconnectCalled, true);
  assert.equal(pollingStopped, true);
  assert.equal(connection.getState(), 'idle');
  assert.notEqual(connection.getGeneration(), gen);
  assert.equal(connection.isCurrent(gen), false);
});

test('blur while still visible releases held inputs but preserves polling and does not disconnect stream', async () => {
  const windowTarget = new EventTarget();
  const documentTarget = new EventTarget();
  documentTarget.hidden = false;

  let disconnectCalled = false;
  const connection = createConnectionManager({
    invoke: async (cmd) => {
      if (cmd === 'connect') return { state: 'ready' };
      if (cmd === 'disconnect') {
        disconnectCalled = true;
        return true;
      }
      return null;
    },
    hasNative: true
  });

  await connection.connect({ host: '192.168.1.10' });
  const gen = connection.getGeneration();
  connection.markFrameRendered(gen);
  assert.equal(connection.getState(), 'streaming');

  let inputsReleased = false;
  let pollingStopped = false;
  let onDisconnectCalled = false;
  attachLifecycle({
    windowTarget,
    documentTarget,
    connection,
    releaseInputs: () => {
      inputsReleased = true;
    },
    onStopPolling: () => {
      pollingStopped = true;
    },
    onDisconnect: () => {
      onDisconnectCalled = true;
      disconnectCalled = true;
    }
  });

  windowTarget.dispatchEvent(new Event('blur'));

  assert.equal(inputsReleased, true);
  assert.equal(onDisconnectCalled, false);
  assert.equal(disconnectCalled, false);
  assert.equal(pollingStopped, false);
  assert.equal(connection.getState(), 'streaming');
  assert.equal(connection.getGeneration(), gen);
  assert.equal(connection.isCurrent(gen), true);
  connection.markFrameRendered(gen);
  assert.equal(connection.snapshot().renderedFrames, 2);
});

test('returning to foreground does not resurrect disconnected session or accept stale frame completions', async () => {
  const windowTarget = new EventTarget();
  const documentTarget = new EventTarget();
  documentTarget.hidden = false;

  const connection = createConnectionManager({
    invoke: async (cmd) => {
      if (cmd === 'connect') return { state: 'ready' };
      if (cmd === 'disconnect') return true;
      return null;
    },
    hasNative: true
  });

  await connection.connect({ host: '192.168.1.10' });
  const gen = connection.getGeneration();
  connection.markFrameRendered(gen);
  assert.equal(connection.getState(), 'streaming');

  attachLifecycle({
    windowTarget,
    documentTarget,
    connection,
    releaseInputs: () => {}
  });

  const idlePromise = waitForState(connection, 'idle');
  documentTarget.hidden = true;
  documentTarget.dispatchEvent(new Event('visibilitychange'));
  await idlePromise;

  documentTarget.hidden = false;
  documentTarget.dispatchEvent(new Event('visibilitychange'));

  assert.equal(connection.getState(), 'idle');
  assert.equal(connection.isCurrent(gen), false);

  connection.markFrameRendered(gen);
  assert.equal(connection.getState(), 'idle');
});
