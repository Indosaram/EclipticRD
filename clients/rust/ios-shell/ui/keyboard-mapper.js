(function (root, factory) {
  const api = factory();
  root.KeyboardMapper = api;
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  }
})(typeof window !== 'undefined' ? window : globalThis, function () {
  'use strict';

  const KEY_CODES = {
    Escape: 0x35,
    Tab: 0x30,
    ArrowUp: 0x7e,
    ArrowDown: 0x7d,
    ArrowLeft: 0x7b,
    ArrowRight: 0x7c,
    Return: 0x24,
    Enter: 0x24,
    Backspace: 0x33,
    Space: 0x31
  };

  const MODIFIER_BITS = {
    SHIFT: 1 << 0,
    CONTROL: 1 << 1,
    OPTION: 1 << 2,
    COMMAND: 1 << 3,
    CAPS_LOCK: 1 << 4
  };

  function mapNamedKeyToCode(name) {
    if (!name || typeof name !== 'string') return null;
    return Object.prototype.hasOwnProperty.call(KEY_CODES, name)
      ? KEY_CODES[name]
      : null;
  }

  function createModifierTracker(initialBits = 0) {
    let current = Number.isInteger(initialBits) && initialBits >= 0 ? (initialBits & 0xffff) : 0;
    const listeners = new Set();

    function notify() {
      for (const listener of listeners) {
        listener(current);
      }
    }

    return {
      get() {
        return current;
      },
      has(bit) {
        return (current & bit) === bit;
      },
      toggle(bit) {
        const mask = bit & 0xffff;
        if ((current & mask) === mask) {
          current = current & ~mask;
        } else {
          current = current | mask;
        }
        notify();
        return current;
      },
      set(bits) {
        current = Number.isInteger(bits) && bits >= 0 ? (bits & 0xffff) : 0;
        notify();
        return current;
      },
      reset() {
        current = 0;
        notify();
        return current;
      },
      subscribe(fn) {
        listeners.add(fn);
        return () => listeners.delete(fn);
      }
    };
  }

  function validateKeyPayload(payload) {
    if (!payload || typeof payload !== 'object') {
      return { ok: false, error: 'Payload must be an object' };
    }
    const keyCode = payload.keyCode;
    const down = payload.down;
    const modifiers = payload.modifiers !== undefined ? payload.modifiers : 0;

    if (!Number.isInteger(keyCode) || keyCode < 0 || keyCode > 0xffff) {
      return { ok: false, error: 'Invalid physical keyCode: ' + keyCode };
    }
    if (typeof down !== 'boolean') {
      return { ok: false, error: 'down field must be a boolean' };
    }
    if (!Number.isInteger(modifiers) || modifiers < 0 || modifiers > 0xffff) {
      return { ok: false, error: 'Invalid modifiers flags: ' + modifiers };
    }

    return {
      ok: true,
      payload: {
        keyCode,
        down,
        modifiers
      }
    };
  }

  return {
    KEY_CODES,
    MODIFIER_BITS,
    mapNamedKeyToCode,
    createModifierTracker,
    validateKeyPayload
  };
});
