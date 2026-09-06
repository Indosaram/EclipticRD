import { test, expect } from 'bun:test';
import { openPage, hosts } from './page-harness.mjs';

test('failed held release still releases remaining keys and does not poison later sessions', async () => {
  const page = await openPage();
  try {
    await page.inventory();
    await page.evaluate(`fixture.command('connect', () => connectToHost('release.test','Release'))`);
    await page.evaluate(`fixture.until(() => document.getElementById('viewport-container').dataset.phase==='waiting-video', () => fixture.settle('connect',null))`);
    await page.evaluate(`(() => { heldInputs.mouseDown('left'); heldInputs.keyDown(65,0); })()`);
    const first = await page.evaluate(`fixture.command('send_input', () => { fixture.stop=doDisconnect(); })`);
    expect(first.args.event.event_type).toBe('LeftMouseUp');
    const remaining = await page.evaluate(`fixture.command('send_input', () => fixture.settle('send_input','release denied',true))`);
    expect(remaining.args.event.event_type).toBe('KeyUp');
    await page.evaluate(`fixture.command('disconnect', () => fixture.settle('send_input',null))`);
    await page.evaluate(`fixture.until(() => document.getElementById('viewport-container').dataset.phase==='error', () => fixture.settle('disconnect',null))`);
    expect(await page.evaluate(`connection.snapshot().cleanupError`)).toContain('release denied');
    await page.evaluate(`fixture.command('disconnect', () => document.getElementById('btn-retry-cleanup').click())`);
    await page.evaluate(`fixture.until(() => document.getElementById('viewport-container').dataset.phase==='idle', () => fixture.settle('disconnect',null))`);
    await page.evaluate(`fixture.command('connect', () => connectToHost('next.test','Next'))`);
    await page.evaluate(`fixture.until(() => document.getElementById('viewport-container').dataset.phase==='waiting-video', () => fixture.settle('connect',null))`);
    await page.evaluate(`heldInputs.keyDown(66,0)`);
    const next = await page.evaluate(`fixture.command('send_input', () => { fixture.stop=doDisconnect(); })`);
    expect(next.args.event.key_code).toBe(66);
    await page.evaluate(`fixture.command('disconnect', () => fixture.settle('send_input',null))`);
    await page.evaluate(`fixture.until(() => document.getElementById('viewport-container').dataset.phase==='idle', () => fixture.settle('disconnect',null))`);
    expect(await page.evaluate(`connection.snapshot().cleanupError`)).toBeNull();
  } finally { await page.close(); }
}, 20000);


test('C3 failed key release does not skip remaining keys or poison cleanup and reconnect', async () => {
  const page = await openPage();
  try {
    await page.inventory();
    await page.evaluate(`fixture.command('connect', () => connectToHost('render.test','Render Workstation'))`);
    await page.evaluate(`fixture.until(() => document.getElementById('viewport-container').dataset.phase==='waiting-video', () => fixture.settle('connect',null))`);
    const first = await page.evaluate(`fixture.command('send_input', () => {
      heldInputs.keyDown(56,0); heldInputs.keyDown(0,0);
      fixture.releaseAttempt = releaseHeldInputs().then(() => null, error => String(error));
    })`);
    expect(first.args.event.key_code).toBe(56);
    const second = await page.evaluate(`fixture.command('send_input', () => fixture.settle('send_input','release failed',true))`);
    expect(second.args.event.event_type).toBe('KeyUp');
    expect(second.args.event.key_code).toBe(0);
    await page.evaluate(`fixture.settle('send_input',null)`);
    expect(await page.evaluate(`fixture.releaseAttempt`)).toContain('release failed');
    await page.evaluate(`fixture.command('disconnect', () => { fixture.stop=doDisconnect(); })`);
    await page.evaluate(`fixture.until(() => document.getElementById('viewport-container').dataset.phase==='idle', () => fixture.settle('disconnect',null))`);
    expect(await page.evaluate(`connection.snapshot().busy`)).toBe(false);
    await page.evaluate(`fixture.command('connect', () => connectToHost('render.test','Render Workstation'))`);
    await page.evaluate(`fixture.until(() => document.getElementById('viewport-container').dataset.phase==='waiting-video', () => fixture.settle('connect',null))`);
    expect(await page.evaluate(`connection.snapshot().cleanupError`)).toBeNull();
  } finally { await page.close(); }
}, 20000);

test('C2 cleanup failure retry and C3 unavailable stats and renderer', async () => {
  const page = await openPage({noGl:true});
  try {
    await page.inventory();
    await page.evaluate(`fixture.command('connect', () => connectToHost('bad.test','Bad'))`);
    await page.evaluate(`fixture.command('disconnect', () => fixture.settle('connect','<pairing rejected>',true))`);
    await page.evaluate(`fixture.until(() => document.getElementById('viewport-container').dataset.phase==='error', () => fixture.settle('disconnect','<cleanup rejected>',true))`);
    expect(await page.evaluate(`({disabled:document.getElementById('btn-direct-connect').disabled,retry:!document.getElementById('btn-retry-cleanup').hidden})`)).toEqual({disabled:true,retry:true});
    await page.evaluate(`fixture.command('disconnect', () => document.getElementById('btn-retry-cleanup').click())`);
    await page.evaluate(`fixture.until(() => !document.getElementById('btn-direct-connect').disabled, () => fixture.settle('disconnect',null))`);
    await page.evaluate(`fixture.command('connect', () => connectToHost('video.test','Video'))`);
    await page.evaluate(`fixture.until(() => document.getElementById('viewport-container').dataset.phase==='waiting-video', () => fixture.settle('connect',null))`);
    await page.evaluate(`fixture.command('stats', () => fixture.intervals[0]())`);
    await page.evaluate(`fixture.until(() => document.getElementById('session-stat-p50').textContent.includes('12.5'), () => fixture.settle('stats',{connected:true,latency_p50_ms:12.5,latency_p99_ms:15,frames_received:4,frames_decoded:3,audio_packets_received:2}))`);
    await page.evaluate(`fixture.command('stats', () => fixture.intervals[0]())`);
    await page.evaluate(`fixture.until(() => !document.getElementById('session-stats-notice').hidden, () => fixture.settle('stats','stats unavailable',true))`);
    expect(await page.evaluate(`connection.snapshot().stats`)).toBeNull();
    await page.evaluate(`fixture.command('poll_frame_raw', () => { fixture.tickPromise=fixture.tick(); })`);
    await page.evaluate(`fixture.command('disconnect', () => fixture.settle('poll_frame_raw',fixture.frame()))`);
    await page.evaluate(`fixture.until(() => document.getElementById('viewport-container').dataset.phase==='error', () => fixture.settle('disconnect',null))`);
    await page.evaluate(`fixture.tickPromise`);
    expect(await page.evaluate(`({draws:fixture.draws,active:isConnected,rafs:fixture.raf.size,error:!!connection.snapshot().error})`)).toEqual({draws:0,active:false,rafs:0,error:true});
  } finally { await page.close(); }
}, 20000);

test('C1 real page search, composed filters, favorites and same-origin persistence', async () => {
  const page = await openPage();
  try {
    await page.inventory();
    expect(await page.evaluate(`!!document.getElementById('host-search')`)).toBe(true);
    const result = await page.evaluate(`(async () => {
      const search = document.getElementById('host-search');
      await fixture.until(() => document.querySelectorAll('.host-card').length === 2, () => { search.value='linux'; search.dispatchEvent(new Event('input', {bubbles:true})); });
      await fixture.until(() => document.querySelectorAll('.host-card').length === 1, () => document.getElementById('filter-available').click());
      document.querySelector('[data-action="favorite"]').click();
      document.getElementById('filter-favorites').click();
      return {ids:[...document.querySelectorAll('.host-card')].map(e=>e.dataset.hostId), stored:JSON.parse(localStorage.getItem('eclipticrd.favorites.v1')), calls:fixture.calls.map(c=>c.cmd)};
    })()`);
    expect(result.ids).toEqual(['e']); expect(result.stored).toEqual(['work.test']); expect(result.calls).toEqual(['list_hosts']);
    await page.reload(); await page.inventory(hosts.map(h => h.id === 'e' ? {...h, id:'new-pairing-id'} : h));
    expect(await page.evaluate(`document.querySelector('[data-host-id="new-pairing-id"] [data-action="favorite"]').getAttribute('aria-pressed')`)).toBe('true');
  } finally { await page.close(); }
}, 20000);

test('C2 real direct form rejects fields and cancels late connect with final cleanup', async () => {
  const page = await openPage();
  try {
    await page.inventory();
    await page.evaluate(`(() => { document.getElementById('direct-ip').value=''; return connectDirect(); })()`);
    expect(await page.evaluate(`document.getElementById('direct-ip').getAttribute('aria-invalid')`)).toBe('true');
    await page.evaluate(`(() => { document.getElementById('direct-ip').value='host.test'; document.getElementById('direct-pin').value='abc'; return connectDirect(); })()`);
    expect(await page.evaluate(`document.getElementById('direct-pin').getAttribute('aria-invalid')`)).toBe('true');
    expect(await page.evaluate(`fixture.calls.filter(c=>c.cmd==='connect').length`)).toBe(0);
    const call = await page.evaluate(`fixture.command('connect', () => { document.getElementById('direct-pin').value='00123456'; document.getElementById('direct-form').requestSubmit(); })`);
    expect(call.args).toEqual({host:'host.test',tcpPort:19730,udpPort:19731,pin:'00123456'});
    await page.evaluate(`fixture.until(() => document.getElementById('viewport-container').dataset.phase==='disconnecting', () => document.getElementById('btn-cancel-connect').click())`);
    expect(await page.evaluate(`fixture.calls.filter(c=>c.cmd==='disconnect').length`)).toBe(0);
    await page.evaluate(`fixture.command('disconnect', () => fixture.settle('connect', null))`);
    expect(await page.evaluate(`({active:isConnected,rafs:fixture.raf.size,pin:document.getElementById('direct-pin').value})`)).toEqual({active:false,rafs:0,pin:''});
    await page.evaluate(`fixture.until(() => document.getElementById('viewport-container').dataset.phase==='idle', () => fixture.settle('disconnect', null))`);
    expect(await page.evaluate(`document.getElementById('viewport-container').classList.contains('active')`)).toBe(false);
  } finally { await page.close(); }
}, 20000);

test('C4 external labels are literal; favorite is isolated and quoted Connect captures original target', async () => {
  const page = await openPage();
  try {
    await page.inventory();
    const result = await page.evaluate(`({ names:[...document.querySelectorAll('.host-name')].map(e=>[e.textContent,e.title]), images:document.querySelectorAll('#host-grid img').length, handlers:[...document.querySelectorAll('#host-grid *')].flatMap(e=>[...e.attributes].filter(a=>a.name.startsWith('on'))).length })`);
    expect(result.images).toBe(0); expect(result.handlers).toBe(0);
    expect(result.names).toEqual(hosts.map(h=>[h.name,h.name]));
    await page.evaluate(`document.querySelector('[data-action="favorite"]').click()`);
    expect(await page.evaluate(`fixture.calls.filter(c=>c.cmd==='connect').length`)).toBe(0);
    const call = await page.evaluate(`fixture.command('connect', () => document.querySelectorAll('[data-action="connect"]')[1].click())`);
    expect(call.args.host).toBe(hosts[1].ip); expect(call.args.pin).toBeNull();
  } finally { await page.close(); }
}, 20000);

test('C3 real WebGL frame, stale poll, local keys, held release and fullscreen failure', async () => {
  const page = await openPage({ fullscreenFailure:true });
  try {
    await page.inventory();
    await page.evaluate(`fixture.command('connect', () => document.querySelector('[data-action="connect"]').click())`);
    await page.evaluate(`fixture.until(() => document.getElementById('viewport-container').dataset.phase==='waiting-video', () => fixture.settle('connect', null))`);
    expect(await page.evaluate(`document.getElementById('session-overlay').inert`)).toBe(true);
    await page.evaluate(`fixture.command('poll_frame_raw', () => { fixture.tickPromise=fixture.tick(); })`);
    await page.evaluate(`fixture.until(() => document.getElementById('viewport-container').dataset.phase==='streaming', () => fixture.settle('poll_frame_raw', fixture.frame()))`);
    await page.evaluate(`fixture.tickPromise`);
    expect(await page.evaluate(`({draws:fixture.draws,width:canvas.width,height:canvas.height,focus:document.activeElement.id})`)).toEqual({draws:1,width:2,height:2,focus:'screen-canvas'});
    await page.evaluate(`fixture.until(() => document.getElementById('btn-expand').getAttribute('aria-expanded')==='true', () => document.getElementById('btn-expand').click())`);
    await page.evaluate(`fixture.until(() => !document.getElementById('session-error').hidden, () => document.getElementById('btn-fullscreen').click())`);
    expect(await page.evaluate(`document.getElementById('btn-fullscreen').getAttribute('aria-pressed')`)).toBe('false');
    await page.evaluate(`(() => { const b=document.getElementById('btn-expand'); b.focus(); b.dispatchEvent(new KeyboardEvent('keydown',{key:'a',keyCode:65,bubbles:true})); b.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',keyCode:27,bubbles:true})); })()`);
    expect(await page.evaluate(`fixture.calls.filter(c=>c.cmd==='send_input').length`)).toBe(0);
    expect(await page.evaluate(`document.getElementById('launcher-panel').hidden`)).toBe(true);
    await page.evaluate(`fixture.command('send_input', () => { canvas.focus(); canvas.dispatchEvent(new KeyboardEvent('keydown',{key:'a',keyCode:65,bubbles:true})); })`);
    await page.evaluate(`fixture.settle('send_input', null)`);
    await page.evaluate(`fixture.command('send_input', () => canvas.dispatchEvent(new MouseEvent('mousedown',{button:0,clientX:640,clientY:400,bubbles:true})))`);
    await page.evaluate(`fixture.settle('send_input', null)`);
    await page.evaluate(`fixture.command('poll_frame_raw', () => { fixture.lateTick=fixture.tick(); })`);
    const firstRelease = await page.evaluate(`fixture.command('send_input', () => { fixture.teardown=endSessionToHome(); })`);
    expect(firstRelease.args.event.event_type).toBe('LeftMouseUp');
    expect(await page.evaluate(`({active:isConnected,disconnects:fixture.calls.filter(c=>c.cmd==='disconnect').length})`)).toEqual({active:false,disconnects:0});
    const nextRelease = await page.evaluate(`fixture.command('send_input', () => fixture.settle('send_input',null))`);
    expect(nextRelease.args.event.event_type).toBe('KeyUp');
    await page.evaluate(`fixture.command('disconnect', () => fixture.settle('send_input',null))`);
    await page.evaluate(`fixture.until(() => document.getElementById('viewport-container').dataset.phase==='idle', () => fixture.settle('disconnect',null))`);
    await page.evaluate(`(() => { fixture.settle('poll_frame_raw',fixture.frame()); return fixture.lateTick; })()`);
    expect(await page.evaluate(`({draws:fixture.draws,rafs:fixture.raf.size,active:isConnected})`)).toEqual({draws:1,rafs:0,active:false});
  } finally { await page.close(); }
}, 20000);

test('C1 refresh failure/retry, empty inventory, unpaired prefill and no bridge', async () => {
  const page = await openPage();
  try {
    expect(await page.evaluate(`document.getElementById('library-status').dataset.state`)).toBe('loading');
    await page.inventory();
    await page.evaluate(`document.querySelector('[data-host-id="d"] [data-action="connect"]').click()`);
    expect(await page.evaluate(`({ip:document.getElementById('direct-ip').value,focus:document.activeElement.id,connects:fixture.calls.filter(c=>c.cmd==='connect').length})`)).toEqual({ip:'new.test',focus:'direct-pin',connects:0});
    await page.evaluate(`fixture.command('list_hosts', () => document.getElementById('btn-refresh').click())`);
    expect(await page.evaluate(`document.querySelectorAll('.host-card').length`)).toBe(5);
    await page.evaluate(`fixture.until(() => document.getElementById('library-status').dataset.state==='error', () => fixture.settle('list_hosts','<refresh failed>',true))`);
    expect(await page.evaluate(`document.querySelectorAll('.host-card').length`)).toBe(5);
    await page.evaluate(`fixture.command('list_hosts', () => document.getElementById('btn-retry-hosts').click())`);
    await page.evaluate(`fixture.until(() => document.getElementById('library-status').dataset.state==='empty', () => fixture.settle('list_hosts',[]))`);
  } finally { await page.close(); }
  const unavailable = await openPage({noBridge:true});
  try {
    expect(await unavailable.evaluate(`({state:document.getElementById('library-status').dataset.state,disabled:document.getElementById('btn-direct-connect').disabled,cards:document.querySelectorAll('.host-card').length})`)).toEqual({state:'unavailable',disabled:true,cards:0});
  } finally { await unavailable.close(); }
}, 20000);
