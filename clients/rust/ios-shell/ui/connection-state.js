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

    const args = {
      host: cleanHost,
      pin: rawPin.length === 8 ? rawPin : null
    };
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
        host,
        generation,
        lastError,
        touchMode,
        isMuted,
        renderedFrames,
        stats: stats ? { ...stats } : null,
        hasNative,
        discoveryState,
        discoveredHosts: [...discoveredHosts],
        discoveryLoading,
        discoveryError,
        selectedHost: selectedHost ? { ...selectedHost } : null
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
      emit();
    }

    function getSelectedHost() {
      return selectedHost ? { ...selectedHost } : null;
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

    async function connect(req) {
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
      renderedFrames = 0;
      emit();

      try {
        if (!hasNative) {
          throw new Error('Mobile connection controls are unavailable in this browser.');
        }
        await invoke('connect', validation.args);
        if (currentGen !== generation) return false;
        state = 'waiting-video';
        emit();
        return true;
      } catch (err) {
        if (currentGen !== generation) return false;
        state = 'error';
        lastError = err instanceof Error ? err.message : String(err);
        emit();
        try {
          if (hasNative) {
            await invoke('disconnect');
          }
        } catch (_) {}
        return false;
      }
    }

    async function disconnect() {
      ++generation;
      const wasBusy = state === 'connecting' || state === 'waiting-video' || state === 'streaming';
      state = 'disconnecting';
      emit();

      try {
        if (hasNative && wasBusy) {
          await invoke('disconnect');
        }
      } catch (err) {
        lastError = err instanceof Error ? err.message : String(err);
      } finally {
        state = 'idle';
        host = null;
        if (isPeriodicEnabled && isForeground) {
          startPeriodicDiscovery(periodicIntervalMs);
        }
        emit();
      }
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
          if (res.state === 'error' && res.last_error) {
            lastError = res.last_error;
            state = 'error';
          }
          emit();
          return stats;
        }
      } catch (err) {
        if (currentGen !== generation) return null;
        lastError = err instanceof Error ? err.message : String(err);
        emit();
      }
      return null;
    }

    function dismissError() {
      lastError = null;
      if (state === 'error') {
        state = 'idle';
      }
      emit();
    }

    return {
      getState: () => state,
      getGeneration: () => generation,
      snapshot,
      isCurrent,
      connect,
      disconnect,
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
