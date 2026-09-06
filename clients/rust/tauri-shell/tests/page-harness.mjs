import { readFile } from 'node:fs/promises';

const ui = new URL('../ui/', import.meta.url);
export const hosts = [
  { id: 'a', name: 'Studio\'s "PC"', ip: 'studio.test', os: 'Windows', online: true, paired: true },
  { id: 'b', name: '<img src=x onerror="window.injected=true">', ip: 'quote\'"<host>', os: 'Unknown', online: true, paired: true },
  { id: 'c', name: 'Offline Linux', ip: 'offline.test', os: 'Linux', online: false, paired: true },
  { id: 'd', name: 'New Mac', ip: 'new.test', os: 'macOS', online: true, paired: false },
  { id: 'e', name: 'Work Linux', ip: 'work.test', os: 'Linux', online: true, paired: true },
];

function bootstrap({ noGl, fullscreenFailure }) {
  window.fixture = { calls: [], pending: [], raf: new Map(), intervals: [], draws: 0, next: 0 };
  const f = window.fixture;
  f.ready = new Promise(resolve => document.addEventListener('DOMContentLoaded', resolve, { once: true }));
  f.until = (predicate, action) => new Promise((resolve, reject) => {
    const finish = () => { if (predicate()) { observer.disconnect(); clearTimeout(timeout); resolve(true); } };
    const observer = new MutationObserver(finish);
    const timeout = setTimeout(() => { observer.disconnect(); reject(new Error('Expected DOM state did not arrive')); }, 5000);
    observer.observe(document, { subtree: true, attributes: true, childList: true, characterData: true });
    action(); finish();
  });
  f.command = (cmd, action) => new Promise((resolve, reject) => {
    const timeout = setTimeout(() => { window.removeEventListener('fixture-call', listener); reject(new Error('Missing IPC: ' + cmd)); }, 5000);
    const listener = e => { if (e.detail.cmd === cmd) { clearTimeout(timeout); window.removeEventListener('fixture-call', listener); resolve(e.detail); } };
    window.addEventListener('fixture-call', listener); action();
  });
  f.settle = (cmd, value, reject = false) => {
    const index = f.pending.findIndex(call => call.cmd === cmd);
    if (index < 0) throw new Error('No pending IPC: ' + cmd);
    const call = f.pending.splice(index, 1)[0];
    (reject ? call.reject : call.resolve)(value);
  };
  f.frame = () => {
    const buffer = new ArrayBuffer(22), view = new DataView(buffer);
    view.setUint32(0, 2, true); view.setUint32(4, 2, true);
    new Uint8Array(buffer, 16).set([180, 180, 180, 180, 128, 128]);
    return buffer;
  };
  window.requestAnimationFrame = fn => { const id = ++f.next; f.raf.set(id, fn); return id; };
  window.cancelAnimationFrame = id => f.raf.delete(id);
  window.setInterval = fn => { f.intervals.push(fn); return f.intervals.length; };
  f.tick = () => { const callbacks = [...f.raf.values()]; f.raf.clear(); return Promise.all(callbacks.map(fn => fn(performance.now()))); };
  const getContext = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function(kind, ...args) {
    if (noGl && kind === 'webgl2') return null;
    const gl = getContext.call(this, kind, ...args);
    if (gl && kind === 'webgl2') {
      const draw = gl.drawArrays.bind(gl);
      gl.drawArrays = (...values) => { f.draws++; return draw(...values); };
    }
    return gl;
  };
  window.__TAURI__ = { core: { invoke(cmd, args) {
    const call = { cmd, args }; f.calls.push(call);
    const result = new Promise((resolve, reject) => f.pending.push({ ...call, resolve, reject }));
    window.dispatchEvent(new CustomEvent('fixture-call', { detail: call }));
    return result;
  } } };
  if (fullscreenFailure) {
    Element.prototype.requestFullscreen = () => Promise.reject(new Error('Element fullscreen denied'));
    window.__TAURI__.window = { getCurrentWindow: () => ({
      isFullscreen: async () => false,
      setFullscreen: async () => { throw new Error('Native fullscreen denied'); },
    }) };
  }
  document.addEventListener('DOMContentLoaded', () => {
    const marker = document.createElement('aside'); marker.id = 'fixture-marker';
    marker.textContent = 'DETERMINISTIC IPC FIXTURE - NOT NATIVE';
    marker.style.cssText = 'position:fixed;bottom:0;right:0;z-index:9000;font:10px monospace;background:#222;color:#fff;pointer-events:none';
    document.body.append(marker);
  }, { once: true });
}

export async function openPage(options = {}) {
  const server = Bun.serve({ hostname: '127.0.0.1', port: 0, async fetch(request) {
    const path = new URL(request.url).pathname;
    const file = path === '/' ? 'index.html' : path.slice(1);
    if (!['index.html', 'styles.css', 'session-overlay.js', 'library.js', 'connection-state.js'].includes(file)) return new Response('', { status: 404 });
    let body = await readFile(new URL(file, ui), 'utf8');
    if (file === 'index.html' && !options.noBridge) body = body.replace('<head>', '<head><script>(' + bootstrap.toString() + ')(' + JSON.stringify(options) + ')</script>');
    return new Response(body, { headers: { 'Content-Type': file.endsWith('.html') ? 'text/html' : file.endsWith('.css') ? 'text/css' : 'text/javascript' } });
  } });
  const view = new Bun.WebView({ width: options.width || 1280, height: options.height || 800 });
  const receipts = { origin: server.url.origin, webViewClosed: false, serverStopped: false };
  async function load() {
    const navigation = new Promise((resolve, reject) => { view.onNavigated = resolve; view.onNavigationFailed = reject; });
    await view.navigate(server.url.href); await navigation;
    if (!options.noBridge) await view.evaluate('fixture.ready.then(() => true)');
  }
  try { await load(); } catch (error) { view.close(); await server.stop(true); throw error; }
  return {
    view, receipts, evaluate: source => view.evaluate(source),
    async inventory(records = hosts) {
      return view.evaluate(`fixture.until(() => document.querySelectorAll('.host-card').length === ${records.length}, () => fixture.settle('list_hosts', ${JSON.stringify(records)}))`);
    },
    async reload() { await load(); },
    async screenshot(path) { const image = await view.screenshot(); await Bun.write(path, image); },
    async close() { view.close(); receipts.webViewClosed = true; await server.stop(true); receipts.serverStopped = true; console.log('PAGE CLEANUP', JSON.stringify(receipts)); },
  };
}
