import { readFile } from 'node:fs/promises';

const ui = new URL('../', import.meta.url);

function bootstrap(options = {}) {
  window.fixture = {
    calls: [],
    pending: [],
    events: [],
    armed: new Map(),
    pairings: options.pairings || [],
    hosts: options.hosts || [],
    autoSettleDisconnect: options.autoSettleDisconnect ?? false,
  };
  const f = window.fixture;

  f.ready = new Promise((resolve) => {
    if (document.readyState === 'complete' || document.readyState === 'interactive') {
      resolve(true);
    } else {
      document.addEventListener('DOMContentLoaded', () => resolve(true), { once: true });
    }
  });

  f.arm = (cmd) => {
    let resolveCommand;
    const promise = new Promise((resolve) => {
      resolveCommand = resolve;
    });
    const listener = (e) => {
      if (e.detail.cmd === cmd) {
        window.removeEventListener('fixture-call', listener);
        resolveCommand(e.detail);
      }
    };
    window.addEventListener('fixture-call', listener);
    f.armed.set(cmd, promise);
    return true;
  };

  f.awaitCommand = (cmd) => {
    const promise = f.armed.get(cmd);
    if (!promise) throw new Error('Missing armed IPC: ' + cmd);
    f.armed.delete(cmd);
    return promise;
  };

  f.command = (cmd, action) => {
    f.arm(cmd);
    if (typeof action === 'function') action();
    return f.awaitCommand(cmd);
  };

  f.settle = (cmd, value, reject = false) => {
    const index = f.pending.findIndex((call) => call.cmd === cmd);
    if (index < 0) throw new Error('No pending IPC for: ' + cmd);
    const call = f.pending.splice(index, 1)[0];
    if (reject) {
      call.reject(value instanceof Error ? value : new Error(String(value)));
    } else {
      call.resolve(value);
    }
  };

  f.until = (predicate, action) =>
    new Promise((resolve, reject) => {
      const finish = () => {
        if (predicate()) {
          observer.disconnect();
          clearTimeout(timeout);
          resolve(true);
        }
      };
      const observer = new MutationObserver(finish);
      const timeout = setTimeout(() => {
        observer.disconnect();
        reject(new Error('Expected DOM state did not arrive in timeout: ' + predicate.toString()));
      }, 5000);
      observer.observe(document, {
        subtree: true,
        attributes: true,
        childList: true,
        characterData: true,
      });
      if (typeof action === 'function') action();
      finish();
    });

  f.taskBarrier = () =>
    new Promise((resolve) => {
      const channel = new MessageChannel();
      channel.port2.onmessage = () => {
        channel.port1.close();
        channel.port2.close();
        resolve(true);
      };
      channel.port1.postMessage(null);
    });

  const KNOWN_COMMANDS = new Set([
    'list_hosts', 'stop_discovery', 'list_pairings', 'forget_pairing',
    'connect', 'disconnect', 'stats', 'poll_frame', 'touch',
    'set_touch_mode', 'send_key', 'set_muted', 'presented', 'startup',
  ]);

  window.__TAURI__ = {
    core: {
      invoke(cmd, args) {
        if (!KNOWN_COMMANDS.has(cmd)) {
          return Promise.reject(new Error('Unknown native command: ' + cmd));
        }

        // Redact secrets such as PIN from recorded call history to ensure no leaks in logs
        const safeArgs = args && typeof args === 'object'
          ? { ...args, ...(args.pin !== undefined ? { pin: args.pin ? '[REDACTED]' : null } : {}) }
          : args;
        const call = { cmd, args: safeArgs, timestamp: performance.now() };
        f.calls.push(call);
        window.dispatchEvent(new CustomEvent('fixture-call', { detail: { cmd, args, timestamp: call.timestamp } }));

        if (cmd === 'list_pairings') {
          return Promise.resolve(f.pairings ? [...f.pairings] : []);
        }
        if (cmd === 'list_hosts') {
          return Promise.resolve(f.hosts ? [...f.hosts] : []);
        }
        if (cmd === 'stop_discovery') {
          return Promise.resolve(true);
        }
        if (cmd === 'forget_pairing') {
          if (f.pairings && args && args.id) {
            f.pairings = f.pairings.filter((p) => p.id !== args.id);
          }
          return Promise.resolve(true);
        }
        if (cmd === 'stats') {
          return Promise.resolve({
            state: 'ready',
            frames_received: 10,
            frames_decoded: 10,
            audio_packets_received: 5,
            audio_samples_played: 480,
          });
        }
        if (cmd === 'presented') {
          return Promise.resolve(true);
        }
        if (cmd === 'poll_frame' && f.autoEmptyPollFrame) {
          return Promise.resolve(new ArrayBuffer(0));
        }
        if (cmd === 'disconnect' && f.autoSettleDisconnect) {
          return Promise.resolve(true);
        }

        const result = new Promise((resolve, reject) => {
          f.pending.push({ ...call, resolve, reject });
        });
        return result;
      },
    },
  };

  document.addEventListener(
    'DOMContentLoaded',
    () => {
      const marker = document.createElement('aside');
      marker.id = 'fixture-marker';
      marker.textContent = 'DETERMINISTIC IPC FIXTURE - NOT NATIVE';
      marker.style.cssText =
        'position:fixed;bottom:0;right:0;z-index:9000;font:10px monospace;background:#222;color:#fff;pointer-events:none';
      document.body.append(marker);
    },
    { once: true }
  );
}

const ALLOWED_FILES = new Set([
  'index.html', 'styles.css', 'app.js', 'connection-state.js',
  'frame-parser.js', 'touch-coords.js', 'keyboard-mapper.js',
  'input-queue.js', 'video-renderer.js', 'lifecycle.js',
]);

export async function startServer(options = {}) {
  return Bun.serve({
    hostname: '127.0.0.1',
    port: 0,
    async fetch(request) {
      const path = new URL(request.url).pathname;
      const file = path === '/' ? 'index.html' : path.slice(1);
      if (!ALLOWED_FILES.has(file)) {
        return new Response('Not found', { status: 404 });
      }

      let body = await readFile(new URL(file, ui), 'utf8');
      if (file === 'index.html' && !options.noBridge) {
        const snippet = `<head><script>(${bootstrap.toString()})(${JSON.stringify(options)})</script>`;
        body = body.replace('<head>', snippet);
      }

      const contentType = file.endsWith('.html')
        ? 'text/html'
        : file.endsWith('.css')
        ? 'text/css'
        : 'text/javascript';

      return new Response(body, {
        headers: { 'Content-Type': contentType },
      });
    },
  });
}

export async function openPage(options = {}) {
  const server = await startServer(options);
  const width = options.width || 430;
  const height = options.height || 932;
  const view = new Bun.WebView({ width, height });
  const receipts = {
    origin: server.url.origin,
    width,
    height,
    webViewClosed: false,
    serverStopped: false,
  };

  async function load() {
    const navigation = new Promise((resolve, reject) => {
      view.onNavigated = resolve;
      view.onNavigationFailed = reject;
    });
    await view.navigate(server.url.href);
    await navigation;
    if (!options.noBridge) {
      await view.evaluate('fixture.ready.then(() => true)');
    }
  }

  try {
    await load();
  } catch (err) {
    view.close();
    await server.stop(true);
    throw err;
  }

  return {
    view,
    receipts,
    serverUrl: server.url.href,
    evaluate: (code) => view.evaluate(code),
    async screenshot(path) {
      const img = await view.screenshot();
      await Bun.write(path, img);
    },
    async close() {
      view.close();
      receipts.webViewClosed = true;
      await server.stop(true);
      receipts.serverStopped = true;
    },
  };
}
