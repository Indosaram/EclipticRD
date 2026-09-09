(function (root, factory) {
  const api = factory();
  root.Lifecycle = api;
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  }
})(typeof window !== 'undefined' ? window : globalThis, function () {
  'use strict';

  function attachLifecycle(options) {
    const windowTarget = options && options.windowTarget ? options.windowTarget : null;
    const documentTarget = options && options.documentTarget ? options.documentTarget : null;
    const connection = options && options.connection ? options.connection : null;
    const releaseInputs = options && typeof options.releaseInputs === 'function'
      ? options.releaseInputs
      : () => {};
    const onStopPolling = options && typeof options.onStopPolling === 'function'
      ? options.onStopPolling
      : null;
    const onDisconnect = options && typeof options.onDisconnect === 'function'
      ? options.onDisconnect
      : null;
    const onStopDiscovery = options && typeof options.onStopDiscovery === 'function'
      ? options.onStopDiscovery
      : null;
    const onResumeDiscovery = options && typeof options.onResumeDiscovery === 'function'
      ? options.onResumeDiscovery
      : null;

    const handleBlur = () => {
      if (documentTarget && documentTarget.hidden) {
        return;
      }
      releaseInputs();
    };

    const handleBackground = () => {
      if (typeof onStopDiscovery === 'function') {
        onStopDiscovery();
      }
      const state = connection ? connection.getState() : null;
      const isActive = state === 'connecting' || state === 'waiting-video' || state === 'streaming';
      if (isActive) {
        releaseInputs();
        if (typeof onStopPolling === 'function') {
          onStopPolling();
        }
        if (typeof onDisconnect === 'function') {
          onDisconnect();
        }
        if (connection && typeof connection.disconnect === 'function') {
          connection.disconnect();
        }
      }
    };

    const handleVisibilityChange = () => {
      if (documentTarget && documentTarget.hidden) {
        handleBackground();
      } else if (documentTarget && !documentTarget.hidden) {
        if (typeof onResumeDiscovery === 'function') {
          onResumeDiscovery();
        }
      }
    };

    const handlePageHide = () => {
      handleBackground();
    };

    if (windowTarget && typeof windowTarget.addEventListener === 'function') {
      windowTarget.addEventListener('blur', handleBlur);
      windowTarget.addEventListener('pagehide', handlePageHide);
    }
    if (documentTarget && typeof documentTarget.addEventListener === 'function') {
      documentTarget.addEventListener('visibilitychange', handleVisibilityChange);
    }

    return function unbind() {
      if (windowTarget && typeof windowTarget.removeEventListener === 'function') {
        windowTarget.removeEventListener('blur', handleBlur);
        windowTarget.removeEventListener('pagehide', handlePageHide);
      }
      if (documentTarget && typeof documentTarget.removeEventListener === 'function') {
        documentTarget.removeEventListener('visibilitychange', handleVisibilityChange);
      }
    };
  }

  return {
    attachLifecycle
  };
});
