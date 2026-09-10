/* DOM-free ownership of the existing Tauri connection commands. */
(function (root, factory) {
  const api = factory();
  root.ConnectionLifecycle = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof window !== 'undefined' ? window : globalThis, function () {
  'use strict';

  function validateConnection({ host = '', pin = null, tcpPort = null, udpPort = null, pairingId = null }) {
    host = host.trim();
    pin = pin == null ? '' : pin.trim();
    pairingId = pairingId == null ? null : (typeof pairingId === 'string' ? pairingId.trim() || null : null);
    const errors = {};
    if (!host) errors.host = 'required';
    if (pin && !/^[0-9]{8}$/.test(pin)) errors.pin = 'invalid-pin';

    function checkPort(val, fieldName, defaultPort) {
      if (val === null || val === undefined || val === '') return defaultPort;
      let num;
      if (typeof val === 'number') {
        num = val;
      } else if (typeof val === 'string' && val.trim() !== '') {
        const trimmed = val.trim();
        num = Number(trimmed);
        if (String(num) !== trimmed) {
          errors[fieldName] = 'invalid-port';
          return null;
        }
      } else {
        errors[fieldName] = 'invalid-port';
        return null;
      }
      if (!Number.isInteger(num) || num < 1 || num > 65535) {
        errors[fieldName] = 'invalid-port';
        return null;
      }
      return num;
    }

    const finalTcp = checkPort(tcpPort, 'tcpPort', 19730);
    const finalUdp = checkPort(udpPort, 'udpPort', 19731);
    const args = { host, tcpPort: finalTcp, udpPort: finalUdp, pin: pin || null };
    if (pairingId) args.pairingId = pairingId;
    return Object.keys(errors).length
      ? { ok: false, errors }
      : { ok: true, args };
  }

  const errorText = error => error instanceof Error ? error.message : (typeof error === 'object' && error !== null && error.message ? error.message : String(error));

  function createConnection({ invoke, nativeAvailable, releaseInputs }) {
    const state = {
      phase: nativeAvailable ? 'idle' : 'unavailable', host: null,
      fieldErrors: {}, error: null, cleanupError: null, stats: null,
      statsStatus: 'idle', statsError: null, generation: 0, busy: false
    };
    const listeners = new Set();
    let pendingConnect = null;
    let pendingCleanup = null;
    let pendingStats = null;

    function snapshot() {
      return { ...state, host: state.host && { ...state.host },
        fieldErrors: { ...state.fieldErrors }, stats: state.stats && { ...state.stats } };
    }
    function emit() { for (const listener of listeners) listener(snapshot()); }
    function clearStats() {
      state.stats = null;
      state.statsStatus = 'idle';
      state.statsError = null;
      pendingStats = null;
    }
    function isCurrent(token) {
      return token === state.generation &&
        (state.phase === 'waiting-video' || state.phase === 'streaming');
    }

    function cleanup(retry = false) {
      if (pendingCleanup) return pendingCleanup;
      if (!state.busy) return Promise.resolve();
      const connectToSettle = pendingConnect;
      ++state.generation;
      state.phase = 'disconnecting';
      state.cleanupError = null;
      clearStats();
      // Install ownership before synchronous notification can reenter an action.
      pendingCleanup = Promise.resolve().then(async () => {
        const errors = [];
        if (!retry) {
          try { await releaseInputs(); }
          catch (error) { errors.push(errorText(error)); }
        }
        // Native connect publishes only on settlement: an earlier disconnect
        // cannot cancel it. Its outcome is consumed, never allowed to revive UI.
        if (connectToSettle) await connectToSettle;
        try { await invoke('disconnect'); }
        catch (error) { errors.push(errorText(error)); }
        if (errors.length) {
          state.cleanupError = errors.join('\n');
          state.phase = 'error';
        } else {
          state.busy = false;
          state.host = null;
          state.phase = state.error ? 'error' : 'idle';
        }
        pendingCleanup = null;
        emit();
      });
      emit();
      return pendingCleanup;
    }

    async function connect(request) {
      if (!nativeAvailable || state.busy) return false;
      const validation = validateConnection(request);
      if (!validation.ok) {
        state.fieldErrors = validation.errors;
        emit();
        return false;
      }
      const generation = ++state.generation;
      state.phase = 'connecting';
      state.host = { ip: validation.args.host, name: request.name || validation.args.host };
      state.fieldErrors = {};
      state.error = null;
      state.cleanupError = null;
      state.busy = true;
      clearStats();
      const operation = Promise.resolve().then(() => invoke('connect', validation.args))
        .then(() => ({ ok: true }), error => ({ ok: false, error: errorText(error) }));
      pendingConnect = operation;
      emit();
      const result = await operation;
      if (pendingConnect === operation) pendingConnect = null;
      if (generation !== state.generation) return true;
      if (result.ok) {
        state.phase = 'waiting-video';
        emit();
      } else {
        state.error = result.error;
        await cleanup();
      }
      return true;
    }

    function refreshStats() {
      const generation = state.generation;
      if (!isCurrent(generation)) return Promise.resolve();
      if (pendingStats) return pendingStats;
      state.statsStatus = 'loading';
      state.statsError = null;
      const operation = Promise.resolve().then(async () => {
        try {
          const stats = await invoke('stats');
          if (!isCurrent(generation)) return;
          if (stats.connected === false) {
            state.error = 'Remote session ended.';
            await cleanup();
            return;
          }
          state.stats = { ...stats };
          state.statsStatus = 'ready';
        } catch (error) {
          if (!isCurrent(generation)) return;
          state.stats = null;
          state.statsStatus = 'error';
          state.statsError = errorText(error);
        } finally {
          if (pendingStats === operation) {
            pendingStats = null;
            emit();
          }
        }
      });
      pendingStats = operation;
      emit();
      return operation;
    }

    return {
      snapshot,
      subscribe(fn) { listeners.add(fn); return () => listeners.delete(fn); },
      connect,
      cancel: () => cleanup(Boolean(state.cleanupError)),
      disconnect: () => cleanup(Boolean(state.cleanupError)),
      retryCleanup: () => state.cleanupError ? cleanup(true) : pendingCleanup || Promise.resolve(),
      refreshStats,
      token: () => state.generation,
      isCurrent,
      markFrameRendered(token) {
        if (isCurrent(token) && state.phase !== 'streaming') {
          state.phase = 'streaming';
          emit();
        }
        return Promise.resolve();
      },
      reportFrameError(token, error) {
        if (!isCurrent(token)) return Promise.resolve();
        state.error = errorText(error);
        return cleanup();
      }
    };
  }

  return { validateConnection, createConnection };
});
