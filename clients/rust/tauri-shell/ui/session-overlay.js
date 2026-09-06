/**
 * Session overlay core logic for the EclipticRD Tauri shell.
 *
 * Pure, DOM-free logic so it can be unit tested with `node --test`
 * (see session-overlay.test.mjs). Loaded as a classic script by
 * ui/index.html BEFORE the inline application script; also exports
 * CommonJS for the test runner.
 *
 * Design contract (Parsec-style session overlay):
 *  - A compact launcher bar stays visible above the video at all times.
 *  - An expandable panel integrates stats + fullscreen + session actions.
 *  - Keyboard/pointer input must never leak from overlay UI to the remote
 *    host; only events targeting the video surface are forwarded.
 *  - Input held down on the video (keys / mouse buttons) is released
 *    explicitly when attention moves to local UI (mirrors the host-side
 *    `InputStateTracker::release_all` semantics: buttons first, then keys).
 */
(function (root, factory) {
  const api = factory();
  root.SessionOverlay = api;
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  }
})(typeof window !== 'undefined' ? window : globalThis, function () {
  'use strict';

  /** The only targets whose keyboard input belongs to the remote machine. */
  const REMOTE_TARGET_IDS = new Set(['viewport', 'screen-canvas']);

  /**
   * True when a DOM element is part of the remote video surface (i.e. its
   * keyboard events should be forwarded to the host). Everything else —
   * overlay buttons, the launcher panel, the connecting modal, dashboard
   * chrome, text inputs — is local UI and must never receive forwarding.
   */
  function isRemoteInputTarget(target) {
    if (!target || typeof target.tagName !== 'string') {
      return false;
    }
    if (target.tagName === 'BODY') {
      return true;
    }
    return typeof target.id === 'string' && REMOTE_TARGET_IDS.has(target.id);
  }

  /** Guard for window-level keydown/keyup forwarding. */
  function shouldForwardKeyboardEvent(event) {
    const target = event ? event.target : null;
    return isRemoteInputTarget(target);
  }

  /**
   * Tracks input the UI pressed down into the remote session so it can be
   * released safely when the user moves attention to the overlay, the
   * window loses focus, or the session ends.
   */
  function createHeldInputTracker() {
    const heldKeys = new Map(); // keyCode -> modifiers captured at press time
    const heldButtons = new Set(); // 'left' | 'right'

    function buttonUpType(button) {
      return button === 'right' ? 'RightMouseUp' : 'LeftMouseUp';
    }

    return {
      keyDown(keyCode, modifiers) {
        heldKeys.set(keyCode, modifiers || 0);
      },
      keyUp(keyCode) {
        heldKeys.delete(keyCode);
      },
      isKeyDown(keyCode) {
        return heldKeys.has(keyCode);
      },
      mouseDown(button) {
        heldButtons.add(button === 'right' ? 'right' : 'left');
      },
      mouseUp(button) {
        heldButtons.delete(button === 'right' ? 'right' : 'left');
      },
      isButtonDown(button) {
        return heldButtons.has(button === 'right' ? 'right' : 'left');
      },
      get size() {
        return heldKeys.size + heldButtons.size;
      },
      /**
       * Builds send_input payloads releasing everything held: mouse buttons
       * first, then keys (same ordering as InputStateTracker::release_all).
       * Clears the tracker; a second call is a no-op.
       */
      releaseEvents(x, y, viewWidth, viewHeight) {
        const events = [];
        for (const button of heldButtons) {
          events.push({
            event_type: buttonUpType(button),
            x: x,
            y: y,
            view_width: viewWidth,
            view_height: viewHeight,
          });
        }
        for (const entry of heldKeys) {
          events.push({
            event_type: 'KeyUp',
            key_code: entry[0],
            modifiers: entry[1],
          });
        }
        heldButtons.clear();
        heldKeys.clear();
        return events;
      },
    };
  }

  /**
   * Small observable state for the launcher: expansion of the controls
   * panel and the fullscreen toggle's pressed state.
   */
  function createOverlayState(initial) {
    const options = initial || {};
    let expanded = !!options.expanded;
    let fullscreenActive = !!options.fullscreenActive;
    const listeners = new Set();

    function emit() {
      const snapshot = { expanded: expanded, fullscreenActive: fullscreenActive };
      for (const listener of listeners) {
        listener(snapshot);
      }
    }

    return {
      getExpanded() {
        return expanded;
      },
      setExpanded(value) {
        expanded = !!value;
        emit();
      },
      toggleExpanded() {
        expanded = !expanded;
        emit();
      },
      getFullscreenActive() {
        return fullscreenActive;
      },
      setFullscreenActive(value) {
        const next = !!value;
        if (next !== fullscreenActive) {
          fullscreenActive = next;
          emit();
        }
      },
      subscribe(listener) {
        listeners.add(listener);
        return function unsubscribe() {
          listeners.delete(listener);
        };
      },
    };
  }

  return {
    isRemoteInputTarget: isRemoteInputTarget,
    shouldForwardKeyboardEvent: shouldForwardKeyboardEvent,
    createHeldInputTracker: createHeldInputTracker,
    createOverlayState: createOverlayState,
  };
});
