// allow: SIZE_OK — indivisible mobile connection lifecycle and discovery state manager
(function (root, factory) {
  if (typeof module === 'object' && module.exports) {
    module.exports = factory();
  } else {
    root.ConnectionState = factory();
  }
})(typeof window !== 'undefined' ? window : globalThis, function () {
  'use strict';

  function validateConnectRequest(req) {
    let rawHost = req && typeof req.host === 'string' ? req.host.trim() : '';
    if (!rawHost) {
      return {
        ok: false,
        field: 'host',
        error: 'Host address is required.'
      };
    }

    let cleanHost = rawHost;
    if (cleanHost.startsWith('[') && cleanHost.endsWith(']')) {
      cleanHost = cleanHost.slice(1, -1).trim();
    }

    if (!cleanHost) {
      return {
        ok: false,
        field: 'host',
        error: 'Host address is required.'
      };
    }

    let tcpPort = undefined;
    if (req && req.tcpPort !== undefined && req.tcpPort !== null) {
      if (typeof req.tcpPort !== 'number' || !Number.isInteger(req.tcpPort) || req.tcpPort <= 0 || req.tcpPort > 65535) {
        return {
          ok: false,
          field: 'tcpPort',
          error: 'TCP port must be an integer between 1 and 65535.'
        };
      }
      tcpPort = req.tcpPort;
    }

    let udpPort = undefined;
    if (req && req.udpPort !== undefined && req.udpPort !== null) {
      if (typeof req.udpPort !== 'number' || !Number.isInteger(req.udpPort) || req.udpPort <= 0 || req.udpPort > 65535) {
        return {
          ok: false,
          field: 'udpPort',
          error: 'UDP port must be an integer between 1 and 65535.'
        };
      }
      udpPort = req.udpPort;
    }

    const rawPin = req && req.pin != null ? String(req.pin).trim() : '';
    if (rawPin.length > 0 && !/^[0-9]{8}$/.test(rawPin)) {
      return {
        ok: false,
        field: 'pin',
        error: 'PIN must be exactly 8 digits.'
      };
    }

    const rawPairingId = req && req.pairingId != null ? String(req.pairingId).trim() : '';

    const args = {
      host: cleanHost,
      pin: rawPin.length === 8 ? rawPin : null
    };
    if (rawPairingId) {
      args.pairingId = rawPairingId;
    }
    if (tcpPort !== undefined) {
      args.tcpPort = tcpPort;
    }
    if (udpPort !== undefined) {
      args.udpPort = udpPort;
    }

    return {
      ok: true,
      args
    };
  }

  function findDiscoveredHostById(discoveredHosts, hostId) {
    if (!Array.isArray(discoveredHosts) || !hostId) return null;
    return discoveredHosts.find((h) => h && h.id === hostId) || null;
  }

  function createConnectionManager(options) {
    const scheduler = options && options.scheduler
      ? options.scheduler
      : {
          setTimeout: (fn, ms) => setTimeout(fn, ms),
          clearTimeout: (id) => clearTimeout(id),
          setInterval: (fn, ms) => setInterval(fn, ms),
          clearInterval: (id) => clearInterval(id)
        };
    const invoke = options && typeof options.invoke === 'function'
      ? options.invoke
      : async () => {
          throw new Error('Native invoke bridge not provided');
        };
    const hasNative = Boolean(options && options.hasNative);

    let state = 'idle';
    let host = null;
    let generation = 0;
    let lastError = null;
    let touchMode = 'direct';
    let isMuted = false;
    let renderedFrames = 0;
    let stats = null;
    let cleanupError = null;
    let hasOwnedSession = false;
    let pendingConnect = null;
    let pendingCleanup = null;
    let discoveryState = 'idle';
    let discoveredHosts = [];
    let discoveryLoading = false;
    let discoveryError = null;
    let selectedHost = null;
    let discoveryGen = 0;
    let inFlightSnapshot = null;
    let inFlightStop = null;
    let periodicTimerId = null;
    let isPeriodicEnabled = false;
    let isForeground = true;
    let periodicIntervalMs = 3000;
    let savedPairings = [];
    let selectedPairing = null;

    const listeners = new Set();

    function emit() {
      const snap = snapshot();
      listeners.forEach((listener) => {
        listener(snap);
      });
    }

    function snapshot() {
      return {
        state,
        busy: hasOwnedSession || pendingCleanup !== null || pendingConnect !== null || state === 'cleanup-failed',
        hasOwnedSession,
        host,
        generation,
        lastError,
        cleanupError,
        touchMode,
        isMuted,
        renderedFrames,
        stats: stats ? { ...stats } : null,
        hasNative,
        discoveryState,
        discoveredHosts: [...discoveredHosts],
        discoveryLoading,
        discoveryError,
        selectedHost: selectedHost ? { ...selectedHost } : null,
        savedPairings: [...savedPairings],
        selectedPairing: selectedPairing ? { ...selectedPairing } : null
      };
    }

    function isCurrent(gen) {
      return gen === generation && (state === 'waiting-video' || state === 'streaming');
    }

    function cancelDiscovery() {
      discoveryGen++;
      inFlightSnapshot = null;
      discoveryLoading = false;
      if (discoveryState === 'loading') discoveryState = 'idle';
    }

    function stopNativeBrowser() {
      if (!hasNative) return Promise.resolve();
      if (inFlightStop) {
        return inFlightStop;
      }
      const promise = (async () => {
        try {
          await invoke('stop_discovery');
        } catch (_) {}
      })();
      inFlightStop = promise;
      return promise.finally(() => {
        if (inFlightStop === promise) {
          inFlightStop = null;
        }
      });
    }

    function querySnapshot() {
      if (!hasNative) {
        discoveryLoading = false;
        discoveryState = 'error';
        discoveryError = 'Mobile connection controls are unavailable in this browser.';
        emit();
        return Promise.reject(new Error(discoveryError));
      }

      if (inFlightSnapshot) {
        return inFlightSnapshot;
      }

      const currentGen = ++discoveryGen;
      discoveryLoading = true;
      discoveryState = 'loading';
      emit();

      const promise = (async () => {
        try {
          if (inFlightStop) {
            try {
              await inFlightStop;
            } catch (_) {}
          }

          if (currentGen !== discoveryGen) {
            return discoveredHosts;
          }

          const result = await invoke('list_hosts');
          if (currentGen === discoveryGen) {
            discoveredHosts = Array.isArray(result) ? result : [];
            discoveryLoading = false;
            discoveryState = 'idle';
            discoveryError = null;
            emit();
          }
          return discoveredHosts;
        } catch (err) {
          if (currentGen === discoveryGen) {
            discoveryLoading = false;
            discoveryState = 'error';
            discoveryError = err instanceof Error ? err.message : String(err);
            emit();
          }
          throw err;
        } finally {
          if (inFlightSnapshot === promise) {
            inFlightSnapshot = null;
          }
        }
      })();

      inFlightSnapshot = promise;
      return promise;
    }

    async function startDiscovery() {
      try {
        return await querySnapshot();
      } catch (_) {
        return [];
      }
    }

    async function refreshHosts() {
      try {
        return await querySnapshot();
      } catch (_) {
        return [];
      }
    }

    function startPeriodicDiscovery(intervalMs = 3000) {
      if (periodicTimerId !== null) {
        scheduler.clearInterval(periodicTimerId);
        periodicTimerId = null;
      }
      isPeriodicEnabled = true;
      periodicIntervalMs = intervalMs;
      if (!isForeground || state !== 'idle') return;
      periodicTimerId = scheduler.setInterval(() => {
        if (isPeriodicEnabled && isForeground && state === 'idle') {
          return refreshHosts().catch(() => {});
        }
      }, intervalMs);
    }

    function stopPeriodicDiscovery() {
      isPeriodicEnabled = false;
      cancelDiscovery();
      if (periodicTimerId !== null) {
        scheduler.clearInterval(periodicTimerId);
        periodicTimerId = null;
      }
      stopNativeBrowser();
    }

    function pauseDiscovery() {
      isForeground = false;
      cancelDiscovery();
      if (periodicTimerId !== null) {
        scheduler.clearInterval(periodicTimerId);
        periodicTimerId = null;
      }
      stopNativeBrowser();
    }

    function resumeDiscovery() {
      isForeground = true;
      if (isPeriodicEnabled && state === 'idle') {
        if (periodicTimerId !== null) {
          scheduler.clearInterval(periodicTimerId);
          periodicTimerId = null;
        }
        periodicTimerId = scheduler.setInterval(() => {
          if (isPeriodicEnabled && isForeground && state === 'idle') {
            return refreshHosts().catch(() => {});
          }
        }, periodicIntervalMs);
        refreshHosts().catch(() => {});
      }
    }

    function selectDiscoveredHost(host) {
      if (!host) {
        selectedHost = null;
        emit();
        return;
      }
      selectedHost = {
        id: host.id,
        name: host.name,
        ip: host.ip,
        tcpPort: host.tcp_port,
        udpPort: host.udp_port,
        isPaired: false
      };
      selectedPairing = null;
      emit();
    }

    function getSelectedHost() {
      return selectedHost ? { ...selectedHost } : null;
    }

    async function refreshPairings() {
      if (!hasNative) {
        savedPairings = [];
        emit();
        return [];
      }
      try {
        const list = await invoke('list_pairings');
        savedPairings = Array.isArray(list) ? list : [];
        emit();
        return savedPairings;
      } catch (_) {
        savedPairings = [];
        emit();
        return [];
      }
    }

    async function forgetPairing(id) {
      if (!hasNative || !id) return false;
      try {
        await invoke('forget_pairing', { id });
        if (selectedPairing && selectedPairing.id === id) {
          selectedPairing = null;
        }
        await refreshPairings();
        return true;
      } catch (_) {
        return false;
      }
    }

    function selectPairing(pairing) {
      if (!pairing) {
        selectedPairing = null;
        emit();
        return;
      }
      selectedPairing = {
        id: pairing.id,
        hostName: pairing.hostName,
        lastEndpoint: pairing.lastEndpoint ? { ...pairing.lastEndpoint } : null
      };
      selectedHost = null;
      emit();
    }

    function getSelectedPairing() {
      return selectedPairing ? { ...selectedPairing } : null;
    }

    async function connectSavedPairing(pairingId) {
      const pId = pairingId || (selectedPairing ? selectedPairing.id : null);
      if (!pId) {
        lastError = 'No saved pairing selected.';
        emit();
        return false;
      }
      const pairing = savedPairings.find((p) => p && p.id === pId) || selectedPairing;
      const endpoint = pairing && pairing.lastEndpoint ? pairing.lastEndpoint : null;
      if (!endpoint || !endpoint.host) {
        lastError = 'No endpoint known for saved pairing; enter host address.';
        emit();
        return false;
      }
      const req = {
        host: endpoint.host,
        tcpPort: endpoint.tcpPort,
        udpPort: endpoint.udpPort,
        pairingId: pId,
        pin: null
      };
      return connect(req);
    }

    async function connectSelectedHost(pin) {
      if (!selectedHost) {
        lastError = 'No host selected.';
        emit();
        return false;
      }

      const pinStr = pin != null ? String(pin).trim() : '';
      if (!selectedHost.isPaired) {
        if (pinStr.length !== 8 || !/^[0-9]{8}$/.test(pinStr)) {
          lastError = 'Pairing PIN is required for unpaired host.';
          emit();
          return false;
        }
      }

      const req = {
        host: selectedHost.ip,
        pin: pinStr.length === 8 ? pinStr : null
      };
      if (selectedHost.tcpPort !== undefined) {
        req.tcpPort = selectedHost.tcpPort;
      }
      if (selectedHost.udpPort !== undefined) {
        req.udpPort = selectedHost.udpPort;
      }
      return connect(req);
    }

    function dismissDiscoveryError() {
      discoveryError = null;
      if (discoveryState === 'error') {
        discoveryState = 'idle';
      }
      emit();
    }

    function disconnectInternal(retry = false, retainError = false) {
      if (pendingCleanup) {
        return pendingCleanup;
      }

      if (!hasOwnedSession && state !== 'cleanup-failed' && state !== 'connecting' && state !== 'waiting-video' && state !== 'streaming' && state !== 'disconnecting') {
        if (state === 'error' && !retainError) {
          state = 'idle';
          lastError = null;
          emit();
        }
        return Promise.resolve();
      }

      const currentGen = ++generation;
      const connectToSettle = pendingConnect;
      state = 'disconnecting';
      cleanupError = null;
      if (!retainError) {
        lastError = null;
      }
      emit();

      const nativeDisconnectPromise = hasNative
        ? Promise.resolve().then(() => invoke('disconnect'))
        : Promise.resolve();

      const cleanupPromise = (async () => {
        let nativeErr = null;
        try {
          await nativeDisconnectPromise;
        } catch (err) {
          nativeErr = err;
        }

        if (connectToSettle) {
          try {
            await connectToSettle;
          } catch (_) {}
        }

        if (nativeErr) {
          const cleanErrMsg = nativeErr instanceof Error ? nativeErr.message : (nativeErr && typeof nativeErr === 'object' && nativeErr.message ? nativeErr.message : String(nativeErr));
          cleanupError = cleanErrMsg;
          if (!lastError) {
            lastError = cleanErrMsg;
          }
          state = 'cleanup-failed';
          hasOwnedSession = true;
        } else {
          hasOwnedSession = false;
          cleanupError = null;
          host = null;
          if (retainError && lastError) {
            state = 'error';
          } else {
            state = 'idle';
            if (isPeriodicEnabled && isForeground) {
              startPeriodicDiscovery(periodicIntervalMs);
            }
          }
        }
      })().finally(() => {
        if (pendingCleanup === cleanupPromise) {
          pendingCleanup = null;
        }
        emit();
      });

      pendingCleanup = cleanupPromise;
      return cleanupPromise;
    }

    function retryCleanup() {
      if (pendingCleanup) {
        return pendingCleanup;
      }
      if (state === 'cleanup-failed' || cleanupError || hasOwnedSession) {
        if (lastError && lastError === cleanupError) {
          lastError = null;
        }
        return disconnectInternal(true, Boolean(lastError));
      }
      return Promise.resolve();
    }

    async function connect(req) {
      if (hasOwnedSession || pendingCleanup !== null || pendingConnect !== null || state === 'cleanup-failed') {
        return false;
      }

      const validation = validateConnectRequest(req);
      if (!validation.ok) {
        lastError = validation.error;
        emit();
        return false;
      }

      cancelDiscovery();
      stopNativeBrowser();
      if (periodicTimerId !== null) {
        scheduler.clearInterval(periodicTimerId);
        periodicTimerId = null;
      }

      const currentGen = ++generation;
      state = 'connecting';
      host = validation.args.host;
      lastError = null;
      cleanupError = null;
      renderedFrames = 0;
      stats = null;
      emit();

      if (!hasNative) {
        state = 'error';
        lastError = 'Mobile connection controls are unavailable in this browser.';
        emit();
        return false;
      }

      hasOwnedSession = true;
      emit();

      const connectPromise = Promise.resolve().then(() => invoke('connect', validation.args));
      pendingConnect = connectPromise;

      try {
        await connectPromise;
        if (currentGen !== generation) {
          return false;
        }
        state = 'waiting-video';
        refreshPairings().catch(() => {});
        emit();
        return true;
      } catch (err) {
        if (currentGen !== generation) {
          return false;
        }
        state = 'error';
        if (err && typeof err === 'object' && err.message) {
          lastError = err.message;
        } else {
          lastError = err instanceof Error ? err.message : String(err);
        }
        emit();
        await disconnectInternal(false, true);
        return false;
      } finally {
        if (pendingConnect === connectPromise) {
          pendingConnect = null;
        }
      }
    }

    function disconnect() {
      return disconnectInternal(false, false);
    }

    function markFrameRendered(gen) {
      if (gen !== generation) return;
      renderedFrames++;
      if (state === 'waiting-video') {
        state = 'streaming';
        emit();
      }
    }

    async function setTouchMode(mode) {
      if (mode !== 'direct' && mode !== 'trackpad') return;
      touchMode = mode;
      emit();
      if (hasNative) {
        try {
          await invoke('set_touch_mode', { mode });
        } catch (err) {
          lastError = err instanceof Error ? err.message : String(err);
          emit();
        }
      }
    }

    async function setMuted(muted) {
      const val = Boolean(muted);
      isMuted = val;
      emit();
      if (hasNative) {
        try {
          await invoke('set_muted', { muted: val });
        } catch (err) {
          lastError = err instanceof Error ? err.message : String(err);
          emit();
        }
      }
    }

    async function pollStats() {
      if (!hasNative || (state !== 'waiting-video' && state !== 'streaming')) {
        return null;
      }
      const currentGen = generation;
      try {
        const res = await invoke('stats');
        if (currentGen !== generation) return null;
        if (res && typeof res === 'object') {
          stats = res;
          const isTerminal = res.state === 'error' || res.state === 'disconnected' || res.connected === false;
          if (isTerminal) {
            const reason = res.last_error || (res.state === 'disconnected' ? 'remote-closed' : 'Remote session ended.');
            lastError = reason;
            state = 'error';
            emit();
            await disconnectInternal(false, true);
            return stats;
          }
          emit();
          return stats;
        }
      } catch (err) {
        if (currentGen !== generation) return null;
        lastError = err instanceof Error ? err.message : String(err);
        state = 'error';
        emit();
        await disconnectInternal(false, true);
      }
      return null;
    }

    function dismissError() {
      if (hasOwnedSession || state === 'cleanup-failed') {
        return disconnectInternal(false, false);
      }
      lastError = null;
      cleanupError = null;
      if (state === 'error') {
        state = 'idle';
      }
      emit();
      return Promise.resolve();
    }

    return {
      getState: () => state,
      getGeneration: () => generation,
      snapshot,
      isCurrent,
      connect,
      disconnect,
      cancel: disconnect,
      retryCleanup,
      markFrameRendered,
      setTouchMode,
      setMuted,
      pollStats,
      dismissError,
      startDiscovery,
      refreshHosts,
      startPeriodicDiscovery,
      stopPeriodicDiscovery,
      pauseDiscovery,
      resumeDiscovery,
      selectDiscoveredHost,
      getSelectedHost,
      connectSelectedHost,
      dismissDiscoveryError,
      refreshPairings,
      forgetPairing,
      selectPairing,
      getSelectedPairing,
      connectSavedPairing,
      subscribe(fn) {
        listeners.add(fn);
        return () => listeners.delete(fn);
      }
    };
  }

  return {
    validateConnectRequest,
    findDiscoveredHostById,
    createConnectionManager
  };
});
