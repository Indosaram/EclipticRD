import test from 'node:test';
import assert from 'node:assert/strict';
import {
  validateConnectRequest,
  createConnectionManager,
  findDiscoveredHostById
} from '../connection-state.js';
import { attachLifecycle } from '../lifecycle.js';

/**
 * Deterministic manual scheduler for driving timed discovery operations without real sleeps.
 * Injected into createConnectionManager as a narrow dependency.
 */
function createManualScheduler() {
  let nextId = 1;
  const tasks = new Map();

  return {
    setTimeout(fn, ms) {
      const id = nextId++;
      tasks.set(id, { fn, ms, interval: false });
      return id;
    },
    clearTimeout(id) {
      tasks.delete(id);
    },
    setInterval(fn, ms) {
      const id = nextId++;
      tasks.set(id, { fn, ms, interval: true });
      return id;
    },
    clearInterval(id) {
      tasks.delete(id);
    },
    getPendingCount() {
      return tasks.size;
    },
    async tick() {
      const currentTasks = Array.from(tasks.entries());
      for (const [id, task] of currentTasks) {
        if (!task.interval) tasks.delete(id);
        await task.fn();
      }
      return currentTasks.length;
    },
    clear() {
      tasks.clear();
    }
  };
}

/**
 * Production-seam test fixture simulating native Tauri IPC.
 * Does not mock away the state machine or lifecycle logic.
 */
function createFixture(options = {}) {
  const calls = [];
  const responses = new Map();
  let defaultListHosts = [];
  const scheduler = options.scheduler || createManualScheduler();
  const enterCallbacks = new Map();

  const invoke = async (cmd, args) => {
    calls.push({ cmd, args });

    if (enterCallbacks.has(cmd)) {
      const waiters = enterCallbacks.get(cmd);
      enterCallbacks.delete(cmd);
      for (const w of waiters) {
        w({ cmd, args });
      }
    }

    if (responses.has(cmd)) {
      const resp = responses.get(cmd);
      if (typeof resp === 'function') return resp(args);
      if (resp instanceof Error) throw resp;
      return resp;
    }
    if (cmd === 'list_hosts') {
      return defaultListHosts;
    }
    if (cmd === 'connect') {
      return { state: 'ready' };
    }
    if (cmd === 'disconnect') {
      return true;
    }
    if (cmd === 'stop_discovery') {
      return true;
    }
    return null;
  };

  const manager = createConnectionManager({
    invoke,
    hasNative: options.hasNative !== false,
    scheduler,
    ...options
  });

  return {
    manager,
    scheduler,
    calls,
    setResponse(cmd, resp) {
      responses.set(cmd, resp);
    },
    setDefaultHosts(hosts) {
      defaultListHosts = hosts;
    },
    getCalls(cmd) {
      return calls.filter((c) => c.cmd === cmd);
    },
    waitForEntered(cmd) {
      return new Promise((resolve) => {
        if (!enterCallbacks.has(cmd)) {
          enterCallbacks.set(cmd, []);
        }
        enterCallbacks.get(cmd).push(resolve);
      });
    },
    cleanup() {
      if (manager && typeof manager.stopPeriodicDiscovery === 'function') {
        manager.stopPeriodicDiscovery();
      }
      scheduler.clear();
      enterCallbacks.clear();
    }
  };
}

// ============================================================================
// 1. SAFE PORT PASSING & ADDRESS VALIDATION
// ============================================================================

test('validateConnectRequest accepts valid custom tcpPort and udpPort', () => {
  const res = validateConnectRequest({
    host: '192.168.1.50',
    tcpPort: 19730,
    udpPort: 19731,
    pin: '12345678'
  });

  assert.equal(res.ok, true);
  assert.equal(res.args.host, '192.168.1.50');
  assert.equal(res.args.tcpPort, 19730);
  assert.equal(res.args.udpPort, 19731);
  assert.equal(res.args.pin, '12345678');
});

test('validateConnectRequest omits port fields when absent for backward compatibility', () => {
  const res = validateConnectRequest({ host: '192.168.1.50', pin: '12345678' });
  assert.equal(res.ok, true);
  assert.equal(res.args.host, '192.168.1.50');
  assert.equal(res.args.pin, '12345678');
  assert.equal('tcpPort' in res.args, false, 'tcpPort must be omitted when not provided');
  assert.equal('udpPort' in res.args, false, 'udpPort must be omitted when not provided');
});

test('validateConnectRequest rejects empty bracketed hosts', () => {
  for (const badHost of ['[]', '[ ]', '  [   ]  ']) {
    const res = validateConnectRequest({ host: badHost });
    assert.equal(res.ok, false, `host ${badHost} should be rejected`);
    assert.equal(res.field, 'host');
  }
});

test('validateConnectRequest rejects port 0, negative, and out-of-range ports', () => {
  for (const badPort of [0, -1, 65536, 100000, '19730', NaN]) {
    const resTcp = validateConnectRequest({ host: '192.168.1.50', tcpPort: badPort });
    assert.equal(resTcp.ok, false, `tcpPort ${badPort} should be rejected`);
    assert.equal(resTcp.field, 'tcpPort');

    const resUdp = validateConnectRequest({ host: '192.168.1.50', udpPort: badPort });
    assert.equal(resUdp.ok, false, `udpPort ${badPort} should be rejected`);
    assert.equal(resUdp.field, 'udpPort');
  }
});

test('validateConnectRequest preserves and normalizes IPv6 addresses without bracket corruption', () => {
  const rawIpv6 = validateConnectRequest({ host: '2001:db8::1', tcpPort: 19730 });
  assert.equal(rawIpv6.ok, true);
  assert.equal(rawIpv6.args.host, '2001:db8::1');

  const bracketed = validateConnectRequest({ host: '[2001:db8::1]', tcpPort: 19730 });
  assert.equal(bracketed.ok, true);
  assert.equal(bracketed.args.host, '2001:db8::1', 'bracketed IPv6 must normalize to unbracketed address');

  const scoped = validateConnectRequest({ host: 'fe80::1%en0', tcpPort: 19730 });
  assert.equal(scoped.ok, true);
  assert.equal(scoped.args.host, 'fe80::1%en0');
});

test('safe port passing propagates to native connect invoke call', async () => {
  const { manager, getCalls, cleanup } = createFixture();
  try {
    const ok = await manager.connect({
      host: '10.0.0.5',
      tcpPort: 29730,
      udpPort: 29731,
      pin: '87654321'
    });

    assert.equal(ok, true);
    const connectCalls = getCalls('connect');
    assert.equal(connectCalls.length, 1);
    assert.deepEqual(connectCalls[0].args, {
      host: '10.0.0.5',
      tcpPort: 29730,
      udpPort: 29731,
      pin: '87654321'
    });
  } finally {
    cleanup();
  }
});

test('null bridge connect cannot report success', async () => {
  const manager = createConnectionManager({ hasNative: false });
  const ok = await manager.connect({ host: '192.168.1.50' });
  assert.equal(ok, false);
  assert.equal(manager.getState(), 'error');
  assert.match(manager.snapshot().lastError || '', /unavailable/);
});

// ============================================================================
// 2. INITIAL DISCOVERY STATE & LOADING BEHAVIOR
// ============================================================================

test('initial state has idle discovery and empty discovered hosts', () => {
  const { manager, cleanup } = createFixture();
  try {
    const snap = manager.snapshot();

    assert.equal(snap.discoveryState, 'idle');
    assert.deepEqual(snap.discoveredHosts, []);
    assert.equal(snap.discoveryLoading, false);
    assert.equal(snap.discoveryError, null);
  } finally {
    cleanup();
  }
});

test('startDiscovery transitions discoveryState to loading while snapshot is pending', async () => {
  let resolveSnapshot;
  const pendingPromise = new Promise((res) => { resolveSnapshot = res; });

  const { manager, setResponse, cleanup } = createFixture();
  try {
    setResponse('list_hosts', () => pendingPromise);

    const discoveryPromise = manager.startDiscovery();

    const snap = manager.snapshot();
    assert.equal(snap.discoveryState, 'loading', 'discoveryState must be loading while snapshot query is pending');
    assert.equal(snap.discoveryLoading, true);

    resolveSnapshot([]);
    await discoveryPromise;

    const snapDone = manager.snapshot();
    assert.equal(snapDone.discoveryState, 'idle');
    assert.equal(snapDone.discoveryLoading, false);
  } finally {
    cleanup();
  }
});

test('pending list_hosts completes after stop and does not repopulate list', async () => {
  let resolveOldQuery;
  const pendingPromise = new Promise((res) => {
    resolveOldQuery = res;
  });

  const { manager, setResponse, waitForEntered, cleanup } = createFixture();
  try {
    setResponse('list_hosts', () => pendingPromise);

    const entered = waitForEntered('list_hosts');
    const query = manager.startDiscovery();
    await entered;

    assert.equal(manager.snapshot().discoveryState, 'loading');

    manager.stopPeriodicDiscovery();
    assert.equal(manager.snapshot().discoveryState, 'idle');

    resolveOldQuery([
      { id: 'Stale._erd._tcp.local.', name: 'Stale Host', ip: '10.0.0.99', os: 'linux', tcp_port: 19730, udp_port: 19731 }
    ]);
    await query;

    const snap = manager.snapshot();
    assert.equal(snap.discoveredHosts.length, 0, 'Stale snapshot must be ignored due to generation invalidation');
    assert.equal(snap.discoveryState, 'idle');
  } finally {
    cleanup();
  }
});

test('stop before native call begins prevents native list_hosts from being invoked', async () => {
  let resolveStop;
  const stopPromise = new Promise((r) => { resolveStop = r; });

  const { manager, setResponse, getCalls, cleanup } = createFixture();
  try {
    setResponse('stop_discovery', () => stopPromise);

    manager.stopPeriodicDiscovery();
    const query = manager.startDiscovery();
    manager.stopPeriodicDiscovery();

    resolveStop();
    await query;

    assert.equal(getCalls('list_hosts').length, 0, 'list_hosts must not be invoked when cancelled before entry');
  } finally {
    cleanup();
  }
});

test('stop then resume waits for native stop before new list_hosts', async () => {
  let resolveStop;
  const stopPromise = new Promise((r) => { resolveStop = r; });

  const { manager, setResponse, getCalls, cleanup } = createFixture();
  try {
    setResponse('stop_discovery', () => stopPromise);

    manager.stopPeriodicDiscovery();

    const startPromise = manager.startDiscovery();

    assert.equal(getCalls('stop_discovery').length, 1);
    assert.equal(getCalls('list_hosts').length, 0, 'list_hosts must NOT run before stop_discovery completes');

    resolveStop();
    await startPromise;

    assert.equal(getCalls('list_hosts').length, 1, 'list_hosts runs after stop_discovery completes');
  } finally {
    cleanup();
  }
});

test('old completion does not clear new pending request', async () => {
  let resolveOld;
  let resolveNew;
  const oldPromise = new Promise((r) => { resolveOld = r; });
  const newPromise = new Promise((r) => { resolveNew = r; });

  let callCount = 0;
  const { manager, setResponse, waitForEntered, cleanup } = createFixture();
  try {
    setResponse('list_hosts', () => {
      callCount++;
      return callCount === 1 ? oldPromise : newPromise;
    });

    const enteredFirst = waitForEntered('list_hosts');
    const q1 = manager.startDiscovery();
    await enteredFirst;

    manager.stopPeriodicDiscovery();

    const enteredSecond = waitForEntered('list_hosts');
    const q2 = manager.startDiscovery();
    await enteredSecond;

    assert.equal(manager.snapshot().discoveryLoading, true);

    resolveOld([]);
    await q1;

    assert.equal(manager.snapshot().discoveryLoading, true, 'New in-flight query must remain loading after old finishes');

    resolveNew([
      { id: 'New._erd._tcp.local.', name: 'New Host', ip: '10.0.0.50', os: 'macos', tcp_port: 19730, udp_port: 19731 }
    ]);
    await q2;

    assert.equal(manager.snapshot().discoveryLoading, false);
    assert.equal(manager.snapshot().discoveredHosts.length, 1);
    assert.equal(manager.snapshot().discoveredHosts[0].name, 'New Host');
  } finally {
    cleanup();
  }
});

// ============================================================================
// 3. LIST DISCOVERED HOSTS & RENDERING SNAPSHOTS
// ============================================================================

test('list_hosts resolution populates discoveredHosts and transitions discoveryState to idle', async () => {
  const fixtureHosts = [
    {
      id: 'Studio-Mac._erd._tcp.local.',
      name: 'Studio Mac',
      ip: '192.168.1.50',
      os: 'macos',
      tcp_port: 19730,
      udp_port: 19731
    },
    {
      id: 'Arch-Linux._erd._tcp.local.',
      name: 'Arch Linux',
      ip: '192.168.1.75',
      os: 'linux',
      tcp_port: 19730,
      udp_port: 19731
    }
  ];

  const { manager, setDefaultHosts, cleanup } = createFixture();
  try {
    setDefaultHosts(fixtureHosts);

    let emittedSnapshots = [];
    manager.subscribe((snap) => {
      emittedSnapshots.push(snap);
    });

    await manager.startDiscovery();

    const snap = manager.snapshot();
    assert.equal(snap.discoveryState, 'idle');
    assert.equal(snap.discoveryLoading, false);
    assert.equal(snap.discoveredHosts.length, 2);

    assert.equal(snap.discoveredHosts[0].name, 'Studio Mac');
    assert.equal(snap.discoveredHosts[0].ip, '192.168.1.50');
    assert.equal(snap.discoveredHosts[0].os, 'macos');
    assert.equal(snap.discoveredHosts[0].tcp_port, 19730);
    assert.equal(snap.discoveredHosts[0].udp_port, 19731);

    assert.equal(snap.discoveredHosts[1].name, 'Arch Linux');
    assert.equal(snap.discoveredHosts[1].ip, '192.168.1.75');
    assert.equal(snap.discoveredHosts[1].os, 'linux');

    assert(emittedSnapshots.length >= 2, 'listener must be notified of discovery updates');
  } finally {
    cleanup();
  }
});

test('empty discovery snapshot yields empty list without error', async () => {
  const { manager, setDefaultHosts, cleanup } = createFixture();
  try {
    setDefaultHosts([]);

    await manager.startDiscovery();

    const snap = manager.snapshot();
    assert.equal(snap.discoveryState, 'idle');
    assert.deepEqual(snap.discoveredHosts, []);
    assert.equal(snap.discoveryError, null);
  } finally {
    cleanup();
  }
});

// ============================================================================
// 4. SERVICE REMOVAL & PRUNING DISAPPEARED HOSTS
// ============================================================================

test('disappearing hosts are pruned from discoveredHosts on subsequent snapshot', async () => {
  const hostA = {
    id: 'HostA._erd._tcp.local.',
    name: 'Host A',
    ip: '192.168.1.10',
    os: 'macos',
    tcp_port: 19730,
    udp_port: 19731
  };
  const hostB = {
    id: 'HostB._erd._tcp.local.',
    name: 'Host B',
    ip: '192.168.1.20',
    os: 'windows',
    tcp_port: 19730,
    udp_port: 19731
  };

  const { manager, setDefaultHosts, cleanup } = createFixture();
  try {
    setDefaultHosts([hostA, hostB]);
    await manager.startDiscovery();
    assert.equal(manager.snapshot().discoveredHosts.length, 2);

    setDefaultHosts([hostA]);
    await manager.refreshHosts();

    const snap2 = manager.snapshot();
    assert.equal(snap2.discoveredHosts.length, 1);
    assert.equal(snap2.discoveredHosts[0].id, hostA.id);

    setDefaultHosts([]);
    await manager.refreshHosts();

    const snap3 = manager.snapshot();
    assert.equal(snap3.discoveredHosts.length, 0);
    assert.deepEqual(snap3.discoveredHosts, []);
  } finally {
    cleanup();
  }
});

// ============================================================================
// 5. DISTINCT ERROR HANDLING (LOCAL-NETWORK DENIED VS BACKEND FAILURE)
// ============================================================================

test('discovery failure sets discoveryState to error while preserving direct connect', async () => {
  const { manager, setResponse, cleanup } = createFixture();
  try {
    setResponse('list_hosts', new Error('Local network permission denied'));

    await manager.startDiscovery();

    const snap = manager.snapshot();
    assert.equal(snap.discoveryState, 'error');
    assert.equal(snap.discoveryLoading, false);
    assert.match(snap.discoveryError, /Local network permission denied/);

    assert.equal(snap.state, 'idle');
    const connectOk = await manager.connect({ host: '192.168.1.100' });
    assert.equal(connectOk, true);
    assert.equal(manager.getState(), 'waiting-video');
  } finally {
    cleanup();
  }
});

// ============================================================================
// 6. REFRESH SCHEDULING & LIFECYCLE MANAGEMENT (DETERMINISTIC SCHEDULER)
// ============================================================================

test('discovery refresh scheduling stops when active session begins and resumes on disconnect', async () => {
  const { manager, scheduler, setDefaultHosts, getCalls, cleanup } = createFixture();
  try {
    setDefaultHosts([]);

    manager.startPeriodicDiscovery(3000);
    assert.equal(scheduler.getPendingCount(), 1, 'Periodic discovery timer should be scheduled');

    await scheduler.tick();
    const initialCallCount = getCalls('list_hosts').length;
    assert.equal(initialCallCount, 1, 'Periodic discovery should poll list_hosts on tick');

    await manager.connect({ host: '10.0.0.1' });
    assert.equal(manager.getState(), 'waiting-video');
    assert.equal(scheduler.getPendingCount(), 0, 'Discovery timer must be stopped during active session');

    const countInSessionBefore = getCalls('list_hosts').length;
    await scheduler.tick();
    assert.equal(getCalls('list_hosts').length, countInSessionBefore, 'No discovery polls while session is active');

    await manager.disconnect();
    assert.equal(manager.getState(), 'idle');
    assert.equal(scheduler.getPendingCount(), 1, 'Discovery timer must resume after disconnect');

    await scheduler.tick();
    assert.equal(getCalls('list_hosts').length, countInSessionBefore + 1, 'Discovery polling resumes after disconnect');
  } finally {
    cleanup();
  }
});

test('session/background disconnect does not restart periodic timer', async () => {
  const { manager, scheduler, setDefaultHosts, cleanup } = createFixture();
  try {
    setDefaultHosts([]);
    manager.startPeriodicDiscovery(2500);
    assert.equal(scheduler.getPendingCount(), 1);

    await manager.connect({ host: '10.0.0.1' });
    assert.equal(scheduler.getPendingCount(), 0);

    manager.pauseDiscovery();

    await manager.disconnect();
    assert.equal(manager.getState(), 'idle');
    assert.equal(scheduler.getPendingCount(), 0, 'Discovery timer must not start while in background');

    manager.resumeDiscovery();
    assert.equal(scheduler.getPendingCount(), 1, 'Discovery timer resumes when foreground returns');
  } finally {
    cleanup();
  }
});

test('lifecycle background transitions halt discovery refresh and foreground transitions resume', async () => {
  const windowTarget = new EventTarget();
  const documentTarget = new EventTarget();
  documentTarget.hidden = false;

  const { manager, cleanup } = createFixture();

  let discoveryStopped = false;
  let discoveryResumed = false;

  const unbind = attachLifecycle({
    windowTarget,
    documentTarget,
    connection: manager,
    releaseInputs: () => {},
    onStopPolling: () => {},
    onDisconnect: () => {},
    onStopDiscovery: () => {
      discoveryStopped = true;
    },
    onResumeDiscovery: () => {
      discoveryResumed = true;
    }
  });

  try {
    documentTarget.hidden = true;
    documentTarget.dispatchEvent(new Event('visibilitychange'));

    assert.equal(discoveryStopped, true, 'Going to background must halt discovery refreshes');

    documentTarget.hidden = false;
    documentTarget.dispatchEvent(new Event('visibilitychange'));

    assert.equal(discoveryResumed, true, 'Returning to foreground must resume discovery refreshes');
  } finally {
    if (typeof unbind === 'function') {
      unbind();
    }
    cleanup();
  }
});

// ============================================================================
// 7. CARD TAP, SELECTION, AND PAIRING SECURITY INVARIANT
// ============================================================================

test('findDiscoveredHostById returns latest updated record when IP or ports change', () => {
  const initialHosts = [
    { id: 'Host1._erd._tcp.local.', name: 'Mac', ip: '192.168.1.10', os: 'macos', tcp_port: 19730, udp_port: 19731 }
  ];
  const updatedHosts = [
    { id: 'Host1._erd._tcp.local.', name: 'Mac', ip: '192.168.1.25', os: 'macos', tcp_port: 29730, udp_port: 29731 }
  ];

  const found1 = findDiscoveredHostById(initialHosts, 'Host1._erd._tcp.local.');
  assert.equal(found1.ip, '192.168.1.10');
  assert.equal(found1.tcp_port, 19730);

  const found2 = findDiscoveredHostById(updatedHosts, 'Host1._erd._tcp.local.');
  assert.equal(found2.ip, '192.168.1.25');
  assert.equal(found2.tcp_port, 29730);
});

test('findDiscoveredHostById returns null when host is removed or inputs invalid', () => {
  const hosts = [
    { id: 'Host1._erd._tcp.local.', name: 'Mac', ip: '192.168.1.10', os: 'macos', tcp_port: 19730, udp_port: 19731 }
  ];

  assert.equal(findDiscoveredHostById(hosts, 'NonExistent'), null);
  assert.equal(findDiscoveredHostById([], 'Host1._erd._tcp.local.'), null);
  assert.equal(findDiscoveredHostById(null, 'Host1._erd._tcp.local.'), null);
  assert.equal(findDiscoveredHostById(hosts, ''), null);
});

test('selecting a discovered host card populates target parameters without inferring paired state', () => {
  const { manager, cleanup } = createFixture();
  try {
    const host = {
      id: 'Office-Desktop._erd._tcp.local.',
      name: 'Office Desktop',
      ip: '192.168.1.150',
      os: 'windows',
      tcp_port: 19730,
      udp_port: 19731
    };

    manager.selectDiscoveredHost(host);
    const selected = manager.getSelectedHost();

    assert.notEqual(selected, null);
    assert.equal(selected.ip, '192.168.1.150');
    assert.equal(selected.tcpPort, 19730);
    assert.equal(selected.udpPort, 19731);
    assert.equal(selected.isPaired, false, 'Card selection must never claim paired from advertised hostname');
  } finally {
    cleanup();
  }
});

test('card-initiated connect requires PIN for unpaired host: asserts missing-PIN rejects and valid-PIN succeeds', async () => {
  const { manager, getCalls, cleanup } = createFixture();
  try {
    const host = {
      id: 'Target._erd._tcp.local.',
      name: 'Custom Target',
      ip: '192.168.1.200',
      os: 'linux',
      tcp_port: 29730,
      udp_port: 29731
    };

    manager.selectDiscoveredHost(host);

    const connectWithoutPin = await manager.connectSelectedHost(null);
    assert.equal(connectWithoutPin, false, 'Connecting unpaired card without PIN must fail');
    assert.equal(getCalls('connect').length, 0, 'No native connect invoke must be issued when PIN is missing');

    const snap = manager.snapshot();
    assert.match(snap.lastError || '', /PIN/, 'Snapshot must record PIN required error');

    const connectWithPin = await manager.connectSelectedHost('12345678');
    assert.equal(connectWithPin, true);

    const connectCalls = getCalls('connect');
    assert.equal(connectCalls.length, 1);
    assert.deepEqual(connectCalls[0].args, {
      host: '192.168.1.200',
      tcpPort: 29730,
      udpPort: 29731,
      pin: '12345678'
    });
  } finally {
    cleanup();
  }
});

test('unpaired discovered host with identical name does not choose saved credential', async () => {
  const { manager, getCalls, setResponse } = createFixture();

  setResponse('list_pairings', [
    {
      id: 'SAVED-MATCHING-NAME',
      hostName: 'IdenticalHost',
      addedAtUnixMs: 1725900000000,
      lastEndpoint: { host: '192.168.1.150', tcpPort: 19730, udpPort: 19731 }
    }
  ]);

  await manager.refreshPairings();
  assert.equal(manager.snapshot().savedPairings.length, 1);

  // An untrusted discovered host advertises the EXACT SAME name
  const discoveredHost = {
    id: 'disc-id-999',
    name: 'IdenticalHost',
    ip: '192.168.1.199',
    os: 'Linux',
    tcp_port: 19730,
    udp_port: 19731
  };

  manager.selectDiscoveredHost(discoveredHost);

  // Verify that discovered host selection does NOT bind or select the saved credential
  const snap = manager.snapshot();
  assert.equal(snap.selectedHost.name, 'IdenticalHost');
  assert.equal(snap.selectedPairing, null, 'Discovered host must not select or bind saved pairing');

  // Attempting to connect without PIN MUST fail and not fall back to saved credential
  const connectWithoutPin = await manager.connectSelectedHost(null);
  assert.equal(connectWithoutPin, false, 'Connecting unpaired discovered host without PIN must fail');
  assert.equal(getCalls('connect').length, 0, 'No connect invoke when PIN is absent on discovered host');

  // Connecting with PIN passes explicit PIN and DOES NOT pass pairingId
  const connectWithPin = await manager.connectSelectedHost('87654321');
  assert.equal(connectWithPin, true);
  const calls = getCalls('connect');
  assert.equal(calls.length, 1);
  assert.equal(calls[0].args.pin, '87654321');
  assert.equal(calls[0].args.pairingId, undefined, 'Must not pass pairingId when pairing discovered host with PIN');
});

