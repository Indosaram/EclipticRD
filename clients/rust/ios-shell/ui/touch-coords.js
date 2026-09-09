(function (root, factory) {
  const api = factory();
  root.TouchCoords = api;
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  }
})(typeof window !== 'undefined' ? window : globalThis, function () {
  'use strict';

  function clamp(val, min, max) {
    return Math.max(min, Math.min(val, max));
  }

  function calculateAspectFit(containerWidth, containerHeight, videoWidth, videoHeight) {
    const cW = Number(containerWidth);
    const cH = Number(containerHeight);
    const vW = Number(videoWidth);
    const vH = Number(videoHeight);

    if (!Number.isFinite(cW) || !Number.isFinite(cH) || cW <= 0 || cH <= 0) {
      return { displayWidth: 0, displayHeight: 0, offsetX: 0, offsetY: 0 };
    }

    if (!Number.isFinite(vW) || !Number.isFinite(vH) || vW <= 0 || vH <= 0) {
      return { displayWidth: cW, displayHeight: cH, offsetX: 0, offsetY: 0 };
    }

    const videoAspect = vW / vH;
    const containerAspect = cW / cH;

    let displayWidth = cW;
    let displayHeight = cH;
    let offsetX = 0;
    let offsetY = 0;

    if (containerAspect > videoAspect) {
      displayHeight = cH;
      displayWidth = cH * videoAspect;
      offsetX = (cW - displayWidth) / 2;
    } else {
      displayWidth = cW;
      displayHeight = cW / videoAspect;
      offsetY = (cH - displayHeight) / 2;
    }

    return {
      displayWidth,
      displayHeight,
      offsetX,
      offsetY
    };
  }

  function normalizeTouchCoordinates(clientX, clientY, fit, containerRect) {
    const rectLeft = containerRect && Number.isFinite(containerRect.left) ? containerRect.left : 0;
    const rectTop = containerRect && Number.isFinite(containerRect.top) ? containerRect.top : 0;

    const dW = fit && fit.displayWidth > 0 ? fit.displayWidth : 1;
    const dH = fit && fit.displayHeight > 0 ? fit.displayHeight : 1;
    const offX = fit && Number.isFinite(fit.offsetX) ? fit.offsetX : 0;
    const offY = fit && Number.isFinite(fit.offsetY) ? fit.offsetY : 0;

    const relX = clientX - rectLeft - offX;
    const relY = clientY - rectTop - offY;

    const normX = clamp(relX / dW, 0.0, 1.0);
    const normY = clamp(relY / dH, 0.0, 1.0);

    return {
      x: normX,
      y: normY
    };
  }

  function mapTouchPhase(eventType) {
    switch (eventType) {
      case 'touchstart':
      case 'pointerdown':
        return 'began';
      case 'touchmove':
      case 'pointermove':
        return 'moved';
      case 'touchend':
      case 'pointerup':
        return 'ended';
      case 'touchcancel':
      case 'pointercancel':
        return 'cancelled';
      default:
        return 'cancelled';
    }
  }

  function createTouchTracker() {
    const active = new Map();

    return {
      addTouch(id, x, y) {
        active.set(id, { id, x, y });
      },
      updateTouch(id, x, y) {
        const existing = active.get(id);
        if (existing) {
          existing.x = x;
          existing.y = y;
        } else {
          active.set(id, { id, x, y });
        }
      },
      removeTouch(id) {
        active.delete(id);
      },
      hasTouch(id) {
        return active.has(id);
      },
      getActiveCount() {
        return active.size;
      },
      cancelAll() {
        const events = [];
        for (const [id, point] of active.entries()) {
          events.push({
            id,
            x: point.x,
            y: point.y,
            phase: 'cancelled'
          });
        }
        active.clear();
        return events;
      }
    };
  }

  return {
    calculateAspectFit,
    normalizeTouchCoordinates,
    mapTouchPhase,
    createTouchTracker
  };
});
