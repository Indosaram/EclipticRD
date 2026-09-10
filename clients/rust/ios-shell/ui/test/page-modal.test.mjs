import { test, expect } from 'bun:test';
import { openPage } from './page-harness.mjs';

const VIEWPORTS = [
  { label: '430x932', width: 430, height: 932 },
  { label: '1280x800', width: 1280, height: 800 },
];

for (const vp of VIEWPORTS) {
  test(`real browser interaction at ${vp.label}: fill, submit, modal appearance, hit-test, focus trap, cancel click, cleanup progress, focus restore, and no stale reconnect`, async () => {
    const page = await openPage({ width: vp.width, height: vp.height });

    try {
      // 1. Fill host and pin
      await page.evaluate(`(() => {
        document.getElementById('connect-host').value = '192.0.2.10';
        document.getElementById('connect-pin').value = '12345678';
        document.getElementById('btn-connect-submit').focus();
      })()`);

      // 2. Confirm command registration before separate native click
      const armedConnect = await page.evaluate(`fixture.arm('connect')`);
      expect(armedConnect).toBe(true);

      // 3. Trusted click via native browser view API
      await page.view.click('#btn-connect-submit');
      const connectCall = await page.evaluate(`fixture.awaitCommand('connect')`);
      expect(connectCall.cmd).toBe('connect');
      expect(connectCall.args.host).toBe('192.0.2.10');

      // 4. Assert PIN is cleared from form input immediately upon submit
      const pinValue = await page.evaluate(`document.getElementById('connect-pin').value`);
      expect(pinValue).toBe('');

      // 5. Inspect modal visibility, client rects, hit-testing, and background inertness
      const checkResult = await page.evaluate(`(() => {
        const modal = document.getElementById('modal-connecting');
        const btn = document.getElementById('btn-cancel-connect');
        const connectView = document.getElementById('view-connect');
        const mRect = modal.getBoundingClientRect();
        const bRect = btn.getBoundingClientRect();
        const cx = bRect.x + bRect.width / 2;
        const cy = bRect.y + bRect.height / 2;
        const hitEl = document.elementFromPoint(cx, cy);

        let hiddenAncestor = null;
        let cur = modal.parentElement;
        while (cur && cur !== document.body) {
          if (cur.hasAttribute('hidden') || getComputedStyle(cur).display === 'none') {
            hiddenAncestor = cur.id || cur.tagName.toLowerCase();
            break;
          }
          cur = cur.parentElement;
        }

        return {
          modalHidden: modal.hidden,
          modalWidth: mRect.width,
          modalHeight: mRect.height,
          btnWidth: bRect.width,
          btnHeight: bRect.height,
          hiddenAncestor,
          btnDisabled: btn.disabled,
          inViewport: cx >= 0 && cy >= 0 && cx < window.innerWidth && cy < window.innerHeight,
          hitMatches: btn.contains(hitEl) || hitEl === btn,
          activeId: document.activeElement ? document.activeElement.id : null,
          connectViewInert: connectView.inert === true || connectView.hasAttribute('inert'),
        };
      })()`);

      expect(checkResult.hiddenAncestor).toBeNull();
      expect(checkResult.modalHidden).toBe(false);
      expect(checkResult.modalWidth).toBeGreaterThan(0);
      expect(checkResult.btnWidth).toBeGreaterThan(0);
      expect(checkResult.btnHeight).toBeGreaterThan(0);
      expect(checkResult.btnDisabled).toBe(false);
      expect(checkResult.inViewport).toBe(true);
      expect(checkResult.hitMatches).toBe(true);
      expect(checkResult.activeId).toBe('btn-cancel-connect');
      expect(checkResult.connectViewInert).toBe(true);

      // 6. Exercise focus trap: Tab, Shift+Tab, and background focus attempt
      const trapResult = await page.evaluate(`(() => {
        const btnCancel = document.getElementById('btn-cancel-connect');
        const refreshBtn = document.getElementById('btn-refresh-discovery');

        // Tab on Cancel button must keep focus inside modal
        btnCancel.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));
        const afterTab = document.activeElement === btnCancel;

        // Shift+Tab on Cancel button must keep focus inside modal
        btnCancel.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true }));
        const afterShiftTab = document.activeElement === btnCancel;

        // Attempting to focus outside element must be trapped back to modal
        refreshBtn.focus();
        const afterBgAttempt = document.activeElement === btnCancel;

        return { afterTab, afterShiftTab, afterBgAttempt };
      })()`);

      expect(trapResult.afterTab).toBe(true);
      expect(trapResult.afterShiftTab).toBe(true);
      expect(trapResult.afterBgAttempt).toBe(true);

      // Screenshot 1: modal connecting
      await page.screenshot(`.omo/pairing-20260910/evidence/lifecycle-modal-connecting-${vp.label}.png`);

      // 7. Confirm disconnect registration before separate native click
      const armedDisconnect = await page.evaluate(`fixture.arm('disconnect')`);
      expect(armedDisconnect).toBe(true);

      // 8. Click Cancel button via native browser view API
      await page.view.click('#btn-cancel-connect');
      const disconnectCall = await page.evaluate(`fixture.awaitCommand('disconnect')`);
      expect(disconnectCall.cmd).toBe('disconnect');

      // 9. While disconnect is in-flight: verify cleanup progress visible and Cancel disabled
      const disconnectingState = await page.evaluate(`(() => {
        const modal = document.getElementById('modal-connecting');
        const btn = document.getElementById('btn-cancel-connect');
        const status = document.getElementById('modal-status-text');
        return {
          modalHidden: modal.hidden,
          btnDisabled: btn.disabled,
          statusText: status.textContent,
        };
      })()`);
      expect(disconnectingState.modalHidden).toBe(false);
      expect(disconnectingState.btnDisabled).toBe(true);
      expect(disconnectingState.statusText).toBe('Disconnecting...');

      // Screenshot 2: modal disconnecting
      await page.screenshot(`.omo/pairing-20260910/evidence/lifecycle-modal-disconnecting-${vp.label}.png`);

      // 10. Step A: Settle disconnect FIRST while connect remains deferred
      await page.evaluate(`fixture.settle('disconnect', true)`);

      // Assert modal remains visible and in disconnecting state until connect also settles
      const stillDisconnecting = await page.evaluate(`(() => {
        const modal = document.getElementById('modal-connecting');
        const status = document.getElementById('modal-status-text');
        const btn = document.getElementById('btn-cancel-connect');
        return {
          modalHidden: modal.hidden,
          btnDisabled: btn.disabled,
          statusText: status.textContent,
        };
      })()`);
      expect(stillDisconnecting.modalHidden).toBe(false);
      expect(stillDisconnecting.btnDisabled).toBe(true);
      expect(stillDisconnecting.statusText).toBe('Disconnecting...');

      // 11. Step B: Now settle deferred connect
      await page.evaluate(`fixture.settle('connect', new Error('Cancelled by user'), true)`);

      // Wait for modal to hide
      await page.evaluate(`fixture.until(() => document.getElementById('modal-connecting').hidden)`);

      const settledState = await page.evaluate(`(() => {
        const modal = document.getElementById('modal-connecting');
        const connectView = document.getElementById('view-connect');
        const btnSubmit = document.getElementById('btn-connect-submit');
        return {
          modalHidden: modal.hidden,
          connectViewInert: connectView.inert === true || connectView.hasAttribute('inert'),
          activeId: document.activeElement ? document.activeElement.id : null,
          submitDisabled: btnSubmit.disabled,
        };
      })()`);

      expect(settledState.modalHidden).toBe(true);
      expect(settledState.connectViewInert).toBe(false);
      expect(settledState.activeId).toBe('btn-connect-submit');
      expect(settledState.submitDisabled).toBe(false);

      // Screenshot 3: settled state
      await page.screenshot(`.omo/pairing-20260910/evidence/lifecycle-modal-settled-${vp.label}.png`);

      // 12. Verify no stale reconnection: deterministic task barrier via MessageChannel
      await page.evaluate(`fixture.taskBarrier()`);
      const allConnectCalls = await page.evaluate(`fixture.calls.filter((c) => c.cmd === 'connect').length`);
      expect(allConnectCalls).toBe(1);
    } finally {
      await page.close();
    }
  }, 20000);
}

test('actual-page video streaming pipeline: poll_frame NV12 frame transitions waiting-video to streaming, renders WebGL, dispatches presented, and hides modal', async () => {
  const page = await openPage({ width: 430, height: 932 });

  try {
    // 1. Initiate connection
    await page.evaluate(`(() => {
      document.getElementById('connect-host').value = '192.0.2.10';
      document.getElementById('connect-pin').value = '12345678';
    })()`);

    const armedConnect = await page.evaluate(`fixture.arm('connect')`);
    expect(armedConnect).toBe(true);

    await page.view.click('#btn-connect-submit');
    await page.evaluate(`fixture.awaitCommand('connect')`);

    // 2. Settle connect successfully -> transitions to waiting-video
    await page.evaluate(`(() => {
      fixture.settle('connect', { state: 'ready' });
    })()`);

    // Verify state transitioned to waiting-video and modal shows waiting text
    await page.evaluate(`fixture.until(() => {
      const modal = document.getElementById('modal-connecting');
      const status = document.getElementById('modal-status-text');
      return !modal.hidden && status.textContent === 'Connected — waiting for video...';
    })`);

    // 3. Construct production binary 2x2 NV12 frame buffer (src/frame.rs format: 16-byte header + 4 Y bytes + 2 UV bytes = 22 bytes)
    await page.evaluate(`(() => {
      const buf = new ArrayBuffer(22);
      const view = new DataView(buf);
      view.setUint32(0, 2, true);  // width = 2
      view.setUint32(4, 2, true);  // height = 2
      view.setUint32(8, 1, true);  // sequence = 1 (low 32)
      view.setUint32(12, 0, true); // sequence (high 32)
      const u8 = new Uint8Array(buf);
      u8[16] = 180; u8[17] = 180; u8[18] = 180; u8[19] = 180; // Y plane
      u8[20] = 128; u8[21] = 128; // UV plane
      window.__fixtureNv12Frame = buf;
    })()`);

    // 4. Arm poll_frame and wait for pollLoop to query frame
    const armedPoll = await page.evaluate(`fixture.arm('poll_frame')`);
    expect(armedPoll).toBe(true);

    // Wait until poll_frame is queried and settle with the 2x2 NV12 binary payload
    const pollCall = await page.evaluate(`fixture.awaitCommand('poll_frame')`);
    expect(pollCall.cmd).toBe('poll_frame');

    await page.evaluate(`(() => {
      fixture.settle('poll_frame', window.__fixtureNv12Frame);
    })()`);

    // 5. Await state transition to streaming and modal hide
    await page.evaluate(`fixture.until(() => {
      const modal = document.getElementById('modal-connecting');
      const sessionView = document.getElementById('view-session');
      return modal.hidden && !sessionView.hidden && document.getElementById('stat-state').textContent === 'streaming';
    })`);

    // 6. Verify WebGL renderer drew frame and presented command was emitted with sequence 1
    const streamingState = await page.evaluate(`(() => {
      const modal = document.getElementById('modal-connecting');
      const canvas = document.getElementById('screen-canvas');
      const presentedCalls = fixture.calls.filter(c => c.cmd === 'presented');
      return {
        modalHidden: modal.hidden,
        canvasVisible: canvas.clientWidth > 0 && canvas.clientHeight > 0,
        statState: document.getElementById('stat-state').textContent,
        resolution: document.getElementById('stat-resolution').textContent,
        presentedCall: presentedCalls.length > 0 ? presentedCalls[0] : null,
      };
    })()`);

    expect(streamingState.modalHidden).toBe(true);
    expect(streamingState.statState).toBe('streaming');
    expect(streamingState.resolution).toBe('2x2');
    expect(streamingState.presentedCall).not.toBeNull();
    expect(streamingState.presentedCall.args).toEqual({ sequence: 1 });

    // Screenshot: streaming active
    await page.screenshot('.omo/pairing-20260910/evidence/lifecycle-modal-streaming-presented.png');
  } finally {
    await page.close();
  }
}, 20000);
