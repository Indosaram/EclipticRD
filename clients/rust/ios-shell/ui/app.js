(function () {
  'use strict';

  const tauriApi = window.__TAURI__
    ? (window.__TAURI__.core || window.__TAURI__.tauri)
    : null;
  const hasNative = Boolean(tauriApi && typeof tauriApi.invoke === 'function');

  const invoke = hasNative
    ? tauriApi.invoke.bind(tauriApi)
    : async (cmd) => {
        throw new Error('Mobile connection controls are unavailable in this browser: ' + cmd);
      };

  const byId = (id) => document.getElementById(id);

  const connectView = byId('view-connect');
  const sessionView = byId('view-session');
  const connectForm = byId('connect-form');
  const hostInput = byId('connect-host');
  const pinInput = byId('connect-pin');
  const hostError = byId('host-error');
  const pinError = byId('pin-error');
  const btnSubmit = byId('btn-connect-submit');
  const bridgeWarning = byId('bridge-warning');
  const alertBanner = byId('alert-banner');
  const alertText = byId('alert-text');
  const btnDismissAlert = byId('btn-dismiss-alert');

  const btnRefreshDiscovery = byId('btn-refresh-discovery');
  const discoveryLoading = byId('discovery-loading');
  const discoveryError = byId('discovery-error');
  const discoveryErrorText = byId('discovery-error-text');
  const discoveryHostsList = byId('discovery-hosts-list');
  const discoveryEmpty = byId('discovery-empty');

  const canvas = byId('screen-canvas');
  const remoteCursor = byId('remote-cursor');
  const modalConnecting = byId('modal-connecting');
  const modalStatusText = byId('modal-status-text');
  const modalHostName = byId('modal-host-name');
  const btnCancelConnect = byId('btn-cancel-connect');

  const overlayPill = byId('overlay-pill');
  const btnOverlayTrigger = byId('btn-overlay-trigger');
  const overlayBar = byId('overlay-bar');
  const statusDot = byId('status-dot');
  const btnSessionDisconnect = byId('btn-session-disconnect');
  const btnToggleTouchMode = byId('btn-toggle-touch-mode');
  const btnTouchModeLabel = byId('touch-mode-label');
  const btnToggleMute = byId('btn-toggle-mute');
  const btnSummonKeyboard = byId('btn-summon-keyboard');
  const btnToggleStats = byId('btn-toggle-stats');

  const accessoryBar = byId('accessory-bar');
  const statsSheet = byId('stats-sheet');
  const btnCloseStats = byId('btn-close-stats');
  const softKeyboardProxy = byId('soft-keyboard-proxy');

  const statState = byId('stat-state');
  const statHost = byId('stat-host');
  const statFps = byId('stat-fps');
  const statResolution = byId('stat-resolution');
  const statFramesRecv = byId('stat-frames-recv');
  const statFramesDecoded = byId('stat-frames-decoded');
  const statAudioPackets = byId('stat-audio-packets');
  const statAudioSamples = byId('stat-audio-samples');

  if (!hasNative && bridgeWarning) {
    bridgeWarning.hidden = false;
  }

  const connection = ConnectionState.createConnectionManager({ invoke, hasNative });
  const touchTracker = TouchCoords.createTouchTracker();
  const modifierTracker = KeyboardMapper.createModifierTracker();
  const inputQueue = InputQueue.createInputQueue({
    invoke,
    isCurrent: (gen) => connection.isCurrent(gen),
    getGeneration: () => connection.getGeneration()
  });

  let renderer = null;
  try {
    renderer = VideoRenderer.createRenderer(canvas, {
      onPresented: (sequence) => {
        if (hasNative) {
          const seqNum = typeof sequence === 'bigint' ? Number(sequence) : sequence;
          invoke('presented', { sequence: seqNum }).catch(() => {});
        }
      }
    });
  } catch (err) {
    console.error('Failed to initialize VideoRenderer:', err);
  }

  let renderLoopActive = false;
  let frameCount = 0;
  let lastFpsCalcTime = performance.now();
  let currentFps = 0;

  function updateFpsCounter() {
    frameCount++;
    const now = performance.now();
    const elapsed = now - lastFpsCalcTime;
    if (elapsed >= 1000) {
      currentFps = Math.round((frameCount * 1000) / elapsed);
      frameCount = 0;
      lastFpsCalcTime = now;
      if (statFps) {
        statFps.textContent = String(currentFps);
      }
    }
  }

  async function pollLoop(generation) {
    if (!renderLoopActive || !connection.isCurrent(generation)) {
      return;
    }

    if (hasNative) {
      try {
        const packet = await invoke('next_frame');
        if (connection.isCurrent(generation) && packet) {
          const parsed = FrameParser.parseFramePacket(packet);
          if (parsed && renderer) {
            renderer.drawFrame(parsed);
            connection.markFrameRendered(generation);
            updateFpsCounter();
            if (statResolution) {
              statResolution.textContent = `${parsed.width}x${parsed.height}`;
            }
          }
        }
      } catch (err) {
        console.warn('next_frame poll error:', err);
      }
    }

    if (renderLoopActive && connection.isCurrent(generation)) {
      requestAnimationFrame(() => pollLoop(generation));
    }
  }

  function startPresentation(generation) {
    if (renderLoopActive) return;
    renderLoopActive = true;
    frameCount = 0;
    lastFpsCalcTime = performance.now();
    requestAnimationFrame(() => pollLoop(generation));
  }

  function stopPresentation() {
    renderLoopActive = false;
    if (renderer) {
      renderer.clear();
    }
    currentFps = 0;
    if (statFps) {
      statFps.textContent = '—';
    }
    if (remoteCursor) {
      remoteCursor.style.display = 'none';
    }
    cancelAllRemoteTouches();
  }

  function cancelAllRemoteTouches() {
    const events = touchTracker.cancelAll();
    if (events.length > 0 && hasNative) {
      for (const ev of events) {
        inputQueue.enqueue('touch', { event: ev });
      }
    }
  }

  const renderedCards = new Map();

  function updateDiscoveryUI(snap) {
    if (!discoveryHostsList) return;

    const isLoading = snap.discoveryState === 'loading' && snap.discoveredHosts.length === 0;
    if (discoveryLoading) {
      discoveryLoading.hidden = !isLoading;
    }

    if (discoveryError) {
      discoveryError.hidden = snap.discoveryState !== 'error';
      if (discoveryErrorText) {
        discoveryErrorText.textContent = snap.discoveryError || '';
      }
    }

    const isEmpty = snap.discoveryState !== 'loading' && snap.discoveryState !== 'error' && snap.discoveredHosts.length === 0;
    if (discoveryEmpty) {
      discoveryEmpty.hidden = !isEmpty;
    }

    const activeIds = new Set();
    const selectedId = snap.selectedHost ? snap.selectedHost.id : null;

    for (const host of snap.discoveredHosts) {
      activeIds.add(host.id);
      let card = renderedCards.get(host.id);
      const isSelected = selectedId === host.id;

      if (!card) {
        card = document.createElement('button');
        card.type = 'button';
        card.className = 'host-card' + (isSelected ? ' selected' : '');
        card.setAttribute('role', 'listitem');
        card.dataset.hostId = host.id;

        card.innerHTML = [
          '<div class="host-card-info">',
          '  <div class="host-name-row">',
          '    <span class="host-name"></span>',
          '    <span class="host-badge"></span>',
          '  </div>',
          '  <span class="host-meta"></span>',
          '</div>',
          '<span class="host-chevron" aria-hidden="true">›</span>'
        ].join('');

        card.addEventListener('click', () => {
          const hostId = card.dataset.hostId;
          const currentHost = ConnectionState.findDiscoveredHostById(
            connection.snapshot().discoveredHosts,
            hostId
          );
          if (!currentHost) return;
          connection.selectDiscoveredHost(currentHost);
          hostInput.value = currentHost.ip;
          pinInput.value = '';
          pinInput.focus();
          hostError.textContent = '';
          pinError.textContent = '';
        });

        renderedCards.set(host.id, card);
        discoveryHostsList.appendChild(card);
      }

      const nameEl = card.querySelector('.host-name');
      if (nameEl && nameEl.textContent !== host.name) {
        nameEl.textContent = host.name;
      }

      const badgeEl = card.querySelector('.host-badge');
      if (badgeEl && badgeEl.textContent !== host.os) {
        badgeEl.textContent = host.os;
      }

      const metaEl = card.querySelector('.host-meta');
      const metaText = `${host.ip}:${host.tcp_port} (UDP ${host.udp_port})`;
      if (metaEl && metaEl.textContent !== metaText) {
        metaEl.textContent = metaText;
      }

      if (isSelected && !card.classList.contains('selected')) {
        card.classList.add('selected');
      } else if (!isSelected && card.classList.contains('selected')) {
        card.classList.remove('selected');
      }
    }

    for (const [id, card] of renderedCards.entries()) {
      if (!activeIds.has(id)) {
        card.remove();
        renderedCards.delete(id);
      }
    }
  }

  function renderState(snap) {
    const phase = snap.state;
    const isBusy = phase === 'connecting' || phase === 'waiting-video' || phase === 'streaming';
    const isStreaming = phase === 'streaming';
    const isConnectingModalVisible = phase === 'connecting' || phase === 'waiting-video' || phase === 'disconnecting';

    if (phase === 'streaming' || phase === 'waiting-video') {
      connectView.hidden = true;
      sessionView.hidden = false;
    } else {
      connectView.hidden = false;
      sessionView.hidden = true;
    }

    modalConnecting.hidden = !isConnectingModalVisible;
    if (isConnectingModalVisible) {
      modalStatusText.textContent = phase === 'waiting-video'
        ? 'Connected — waiting for video...'
        : phase === 'disconnecting'
          ? 'Disconnecting...'
          : 'Connecting to host...';
      modalHostName.textContent = snap.host || '';
      btnCancelConnect.disabled = phase === 'disconnecting';
    }

    btnSubmit.disabled = isBusy || !hasNative;
    btnSubmit.textContent = isBusy ? 'Connecting…' : 'Connect';
    hostInput.disabled = isBusy || !hasNative;
    pinInput.disabled = isBusy || !hasNative;

    if (snap.lastError) {
      alertText.textContent = snap.lastError;
      alertBanner.hidden = false;
    } else {
      alertBanner.hidden = true;
    }

    statusDot.className = 'status-dot ' + (isStreaming ? 'streaming' : phase === 'error' ? 'error' : '');
    btnToggleTouchMode.setAttribute('aria-pressed', String(snap.touchMode === 'trackpad'));
    btnTouchModeLabel.textContent = snap.touchMode === 'trackpad' ? 'Trackpad' : 'Direct';
    btnToggleMute.setAttribute('aria-pressed', String(snap.isMuted));
    btnToggleMute.textContent = snap.isMuted ? 'Unmute' : 'Mute';

    if (statState) statState.textContent = phase;
    if (statHost) statHost.textContent = snap.host || '—';
    if (snap.stats) {
      if (statFramesRecv) statFramesRecv.textContent = String(snap.stats.frames_received || 0);
      if (statFramesDecoded) statFramesDecoded.textContent = String(snap.stats.frames_decoded || 0);
      if (statAudioPackets) statAudioPackets.textContent = String(snap.stats.audio_packets_received || 0);
      if (statAudioSamples) statAudioSamples.textContent = String(snap.stats.audio_samples_played || 0);
    }

    updateDiscoveryUI(snap);

    if ((phase === 'waiting-video' || phase === 'streaming') && !renderLoopActive) {
      startPresentation(snap.generation);
    } else if (phase !== 'waiting-video' && phase !== 'streaming' && renderLoopActive) {
      stopPresentation();
    }
  }

  connection.subscribe(renderState);

  setInterval(() => {
    const st = connection.getState();
    if (st === 'waiting-video' || st === 'streaming') {
      connection.pollStats();
    }
  }, 500);

  if (btnRefreshDiscovery) {
    btnRefreshDiscovery.addEventListener('click', () => {
      connection.refreshHosts().catch(() => {});
    });
  }

  connectForm.addEventListener('submit', (e) => {
    e.preventDefault();
    hostError.textContent = '';
    pinError.textContent = '';
    const host = hostInput.value.trim();
    const pin = pinInput.value.trim();

    const selected = connection.getSelectedHost();
    if (selected && selected.ip === host) {
      if (!pin) {
        pinError.textContent = 'Pairing PIN is required for this host.';
        pinInput.focus();
        return;
      }
      try {
        localStorage.setItem('eclipticrd.ios.last_host', host);
      } catch (_) {}
      connection.connectSelectedHost(pin);
      return;
    }

    const val = ConnectionState.validateConnectRequest({ host, pin });
    if (!val.ok) {
      if (val.field === 'host') hostError.textContent = val.error;
      if (val.field === 'pin') pinError.textContent = val.error;
      return;
    }

    try {
      localStorage.setItem('eclipticrd.ios.last_host', host);
    } catch (_) {}

    connection.connect(val.args);
  });

  btnCancelConnect.addEventListener('click', () => {
    connection.disconnect();
  });

  btnDismissAlert.addEventListener('click', () => {
    connection.dismissError();
  });

  btnSessionDisconnect.addEventListener('click', () => {
    connection.disconnect();
  });

  btnToggleTouchMode.addEventListener('click', () => {
    const cur = connection.snapshot().touchMode;
    const next = cur === 'direct' ? 'trackpad' : 'direct';
    connection.setTouchMode(next);
  });

  btnToggleMute.addEventListener('click', () => {
    const cur = connection.snapshot().isMuted;
    connection.setMuted(!cur);
  });

  btnOverlayTrigger.addEventListener('click', () => {
    const isCollapsed = overlayBar.classList.contains('collapsed');
    if (isCollapsed) {
      overlayBar.classList.remove('collapsed');
      btnOverlayTrigger.setAttribute('aria-expanded', 'true');
    } else {
      overlayBar.classList.add('collapsed');
      btnOverlayTrigger.setAttribute('aria-expanded', 'false');
    }
  });

  btnToggleStats.addEventListener('click', () => {
    statsSheet.hidden = !statsSheet.hidden;
  });

  btnCloseStats.addEventListener('click', () => {
    statsSheet.hidden = true;
  });

  btnSummonKeyboard.addEventListener('click', () => {
    if (softKeyboardProxy) {
      softKeyboardProxy.focus();
    }
  });

  const stopLocalPropagation = (e) => {
    e.stopPropagation();
  };

  overlayPill.addEventListener('pointerdown', stopLocalPropagation);
  accessoryBar.addEventListener('pointerdown', stopLocalPropagation);
  statsSheet.addEventListener('pointerdown', stopLocalPropagation);
  modalConnecting.addEventListener('pointerdown', stopLocalPropagation);

  accessoryBar.addEventListener('click', (e) => {
    const btn = e.target.closest('.key-btn');
    if (!btn) return;
    const keyName = btn.dataset.key;
    const modBitName = btn.dataset.modifier;

    if (modBitName) {
      const bit = KeyboardMapper.MODIFIER_BITS[modBitName];
      if (bit) {
        const bits = modifierTracker.toggle(bit);
        btn.setAttribute('aria-pressed', String((bits & bit) === bit));
      }
      return;
    }

    if (keyName) {
      const keyCode = KeyboardMapper.mapNamedKeyToCode(keyName);
      if (keyCode !== null && hasNative) {
        const mods = modifierTracker.get();
        inputQueue.enqueue('send_key', { keyCode, down: true, modifiers: mods });
        inputQueue.enqueue('send_key', { keyCode, down: false, modifiers: mods });
      }
    }
  });

  if (softKeyboardProxy) {
    softKeyboardProxy.addEventListener('keydown', (e) => {
      let keyCode = null;
      if (e.key === 'Escape') keyCode = 0x35;
      else if (e.key === 'Tab') keyCode = 0x30;
      else if (e.key === 'Enter') keyCode = 0x24;
      else if (e.key === 'Backspace') keyCode = 0x33;
      else if (e.key === ' ') keyCode = 0x31;

      if (keyCode !== null && hasNative) {
        const mods = modifierTracker.get();
        inputQueue.enqueue('send_key', { keyCode, down: true, modifiers: mods });
        inputQueue.enqueue('send_key', { keyCode, down: false, modifiers: mods });
      }
    });

    softKeyboardProxy.addEventListener('input', () => {
      softKeyboardProxy.value = '';
    });
  }

  function getAspectFit() {
    const dims = renderer ? renderer.getDimensions() : { width: 1920, height: 1080 };
    const vW = dims.width > 0 ? dims.width : 1920;
    const vH = dims.height > 0 ? dims.height : 1080;
    return TouchCoords.calculateAspectFit(canvas.clientWidth, canvas.clientHeight, vW, vH);
  }

  canvas.addEventListener('pointerdown', (e) => {
    if (connection.getState() !== 'streaming') return;
    try {
      canvas.setPointerCapture(e.pointerId);
    } catch (_) {}

    const fit = getAspectFit();
    const rect = canvas.getBoundingClientRect();
    const norm = TouchCoords.normalizeTouchCoordinates(e.clientX, e.clientY, fit, rect);

    touchTracker.addTouch(e.pointerId, norm.x, norm.y);
    if (hasNative) {
      inputQueue.enqueue('touch', {
        event: {
          id: e.pointerId,
          x: norm.x,
          y: norm.y,
          phase: 'began'
        }
      });
    }
  });

  canvas.addEventListener('pointermove', (e) => {
    if (connection.getState() !== 'streaming') return;
    if (!touchTracker.hasTouch(e.pointerId)) return;

    const fit = getAspectFit();
    const rect = canvas.getBoundingClientRect();
    const norm = TouchCoords.normalizeTouchCoordinates(e.clientX, e.clientY, fit, rect);

    touchTracker.updateTouch(e.pointerId, norm.x, norm.y);
    if (hasNative) {
      inputQueue.enqueue('touch', {
        event: {
          id: e.pointerId,
          x: norm.x,
          y: norm.y,
          phase: 'moved'
        }
      });
    }
  });

  canvas.addEventListener('pointerup', (e) => {
    if (!touchTracker.hasTouch(e.pointerId)) return;
    const fit = getAspectFit();
    const rect = canvas.getBoundingClientRect();
    const norm = TouchCoords.normalizeTouchCoordinates(e.clientX, e.clientY, fit, rect);

    touchTracker.removeTouch(e.pointerId);
    if (hasNative) {
      inputQueue.enqueue('touch', {
        event: {
          id: e.pointerId,
          x: norm.x,
          y: norm.y,
          phase: 'ended'
        }
      });
    }
  });

  canvas.addEventListener('pointercancel', (e) => {
    if (!touchTracker.hasTouch(e.pointerId)) return;
    const fit = getAspectFit();
    const rect = canvas.getBoundingClientRect();
    const norm = TouchCoords.normalizeTouchCoordinates(e.clientX, e.clientY, fit, rect);

    touchTracker.removeTouch(e.pointerId);
    if (hasNative) {
      inputQueue.enqueue('touch', {
        event: {
          id: e.pointerId,
          x: norm.x,
          y: norm.y,
          phase: 'cancelled'
        }
      });
    }
  });

  Lifecycle.attachLifecycle({
    windowTarget: window,
    documentTarget: document,
    connection,
    releaseInputs: cancelAllRemoteTouches,
    onStopPolling: stopPresentation,
    onDisconnect: () => {
      connection.disconnect();
    },
    onStopDiscovery: () => {
      connection.pauseDiscovery();
    },
    onResumeDiscovery: () => {
      connection.resumeDiscovery();
    }
  });

  window.addEventListener('DOMContentLoaded', async () => {
    try {
      const savedHost = localStorage.getItem('eclipticrd.ios.last_host');
      if (savedHost && !hostInput.value) {
        hostInput.value = savedHost;
      }
    } catch (_) {}

    if (hasNative) {
      connection.startDiscovery().catch(() => {});
      connection.startPeriodicDiscovery(3000);

      try {
        const startupInfo = await invoke('startup');
        if (startupInfo && typeof startupInfo === 'object') {
          if (startupInfo.host) {
            hostInput.value = startupInfo.host;
          }
          if (startupInfo.auto_connect && startupInfo.host) {
            connection.connect({ host: startupInfo.host, pin: null });
          }
        }
      } catch (err) {
        console.warn('Startup query failed:', err);
      }
    }
  });
})();
