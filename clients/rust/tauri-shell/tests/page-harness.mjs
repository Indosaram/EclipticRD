import { join } from 'node:path';

const distDir = new URL('../dist', import.meta.url).pathname;

export const hosts = [
  { id: 'a', name: 'Studio PC', ip: 'studio.test', os: 'Windows', online: true, paired: true },
  { id: 'b', name: 'Clean Linux', ip: 'clean.test', os: 'Linux', online: true, paired: true },
  { id: 'c', name: 'Offline Linux', ip: 'offline.test', os: 'Linux', online: false, paired: true },
  { id: 'd', name: 'New Mac', ip: 'new.test', os: 'macOS', online: true, paired: false },
  { id: 'e', name: 'Work Linux', ip: 'work.test', os: 'Linux', online: true, paired: true },
];

function bootstrap(options = {}) {
  window.fixture = {
    calls: [],
    hostCalls: [],
    pairingCalls: [],
    pending: [],
    hosts: options.hosts ? [...options.hosts] : [
      { id: 'a', name: 'Studio PC', ip: 'studio.test', os: 'Windows', online: true, paired: true },
      { id: 'b', name: 'Clean Linux', ip: 'clean.test', os: 'Linux', online: true, paired: true },
      { id: 'c', name: 'Offline Linux', ip: 'offline.test', os: 'Linux', online: false, paired: true },
      { id: 'd', name: 'New Mac', ip: 'new.test', os: 'macOS', online: true, paired: false },
      { id: 'e', name: 'Work Linux', ip: 'work.test', os: 'Linux', online: true, paired: true },
    ],
    hostStatus: options.hostStatus
      ? { ...options.hostStatus }
      : { running: true, ip: '127.0.0.1', port: 19730, pin: '87654321', auto_approve: false },
    pairings: options.pairings ? [...options.pairings] : [],
    canned: options.canned ? { ...options.canned } : {},
    manualHosts: Boolean(options.manualHosts),
  };

  const f = window.fixture;
  f.ready = new Promise((resolve) => {
    if (document.readyState === 'complete') {
      resolve();
    } else {
      window.addEventListener('DOMContentLoaded', () => resolve(), { once: true });
    }
  });

  f.until = (predicate, action) =>
    new Promise((resolve, reject) => {
      const finish = () => {
        try {
          if (predicate()) {
            observer.disconnect();
            clearTimeout(timeout);
            resolve(true);
          }
        } catch (err) {
          observer.disconnect();
          clearTimeout(timeout);
          reject(err);
        }
      };
      const observer = new MutationObserver(finish);
      const timeout = setTimeout(() => {
        observer.disconnect();
        reject(new Error('Expected DOM state did not arrive within timeout'));
      }, 5000);
      observer.observe(document, { subtree: true, attributes: true, childList: true, characterData: true });
      if (action) action();
      finish();
    });

  f.command = (cmd, action) =>
    new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        window.removeEventListener('fixture-call', listener);
        reject(new Error('Missing IPC: ' + cmd));
      }, 5000);
      const listener = (e) => {
        if (e.detail && e.detail.cmd === cmd) {
          clearTimeout(timeout);
          window.removeEventListener('fixture-call', listener);
          resolve(e.detail);
        }
      };
      window.addEventListener('fixture-call', listener);
      if (action) action();
    });

  f.settle = (cmd, value, reject = false) => {
    const index = f.pending.findIndex((call) => call.cmd === cmd);
    if (index < 0) throw new Error('No pending IPC: ' + cmd);
    const call = f.pending.splice(index, 1)[0];
    (reject ? call.reject : call.resolve)(value);
  };

  f.type = (selector, text) => {
    const el = typeof selector === 'string' ? document.querySelector(selector) : selector;
    if (!el) throw new Error('Element not found: ' + selector);
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set;
    setter.call(el, text);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  };

  const invokeHandler = (cmd, args) => {
    const call = { cmd, args };
    f.calls.push(call);
    window.dispatchEvent(new CustomEvent('fixture-call', { detail: call }));

    if (cmd === 'get_host_status') {
      f.hostCalls.push(call);
      return Promise.resolve({ ...f.hostStatus });
    }
    if (cmd === 'start_host') {
      f.hostStatus.running = true;
      f.hostCalls.push(call);
      return Promise.resolve({ ...f.hostStatus });
    }
    if (cmd === 'stop_host') {
      f.hostStatus.running = false;
      f.hostCalls.push(call);
      return Promise.resolve({ ...f.hostStatus });
    }
    if (cmd === 'list_pairings') {
      f.pairingCalls.push(call);
      return Promise.resolve(f.pairings ? [...f.pairings] : []);
    }
    if (cmd === 'forget_pairing') {
      if (f.pairings && args && args.id) {
        f.pairings = f.pairings.filter((p) => p.id !== args.id);
      }
      return Promise.resolve();
    }
    if (cmd === 'list_hosts') {
      if (f.manualHosts) {
        return new Promise((resolve, reject) => f.pending.push({ ...call, resolve, reject }));
      }
      return Promise.resolve(f.hosts ? [...f.hosts] : []);
    }
    if (f.canned && cmd in f.canned) {
      const resp = typeof f.canned[cmd] === 'function' ? f.canned[cmd](args) : f.canned[cmd];
      return Promise.resolve(resp);
    }
    const result = new Promise((resolve, reject) => f.pending.push({ ...call, resolve, reject }));
    return result;
  };

  window.__TAURI__ = {
    core: { invoke: invokeHandler },
    tauri: { invoke: invokeHandler },
  };
}

export async function startServer(options = {}) {
  const server = Bun.serve({
    hostname: '127.0.0.1',
    port: 0,
    async fetch(request) {
      const pathname = new URL(request.url).pathname;
      if (pathname === '/' || pathname === '/index.html') {
        const indexFile = Bun.file(join(distDir, 'index.html'));
        if (!(await indexFile.exists())) {
          return new Response('Built index.html not found. Run bun run build.', { status: 500 });
        }
        let body = await indexFile.text();
        if (!options.noBridge) {
          const initScript = `<script>(${bootstrap.toString()})(${JSON.stringify(options)});</script>`;
          body = body.replace('<head>', '<head>' + initScript);
        }
        return new Response(body, {
          headers: { 'Content-Type': 'text/html; charset=utf-8' },
        });
      }

      if (pathname.startsWith('/assets/')) {
        const assetFile = Bun.file(join(distDir, pathname));
        if (await assetFile.exists()) {
          return new Response(assetFile);
        }
      }

      return new Response('Not Found', { status: 404 });
    },
  });
  return server;
}

export async function stopServer(server) {
  if (server) {
    await server.stop(true);
  }
}

export async function openPage(options = {}) {
  const server = await startServer(options);
  let view = null;
  const receipts = {
    origin: server.url.origin,
    webViewClosed: false,
    serverStopped: false,
  };

  try {
    view = new Bun.WebView({
      width: options.width || 1280,
      height: options.height || 800,
    });

    async function load() {
      const navigation = new Promise((resolve, reject) => {
        view.onNavigated = resolve;
        view.onNavigationFailed = reject;
      });
      await view.navigate(server.url.href);
      await navigation;
      if (!options.noBridge) {
        await view.evaluate('window.fixture ? window.fixture.ready.then(() => true) : true');
      }
    }

    await load();

    return {
      view,
      server,
      receipts,
      evaluate: (source) => view.evaluate(source),
      reload: async () => {
        await load();
      },
      close: async () => {
        if (!receipts.webViewClosed && view) {
          view.close();
          receipts.webViewClosed = true;
        }
        if (!receipts.serverStopped && server) {
          await stopServer(server);
          receipts.serverStopped = true;
        }
      },
    };
  } catch (error) {
    if (view && !receipts.webViewClosed) {
      view.close();
      receipts.webViewClosed = true;
    }
    if (server && !receipts.serverStopped) {
      await stopServer(server);
      receipts.serverStopped = true;
    }
    throw error;
  }
}
