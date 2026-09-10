import { test, expect } from 'bun:test';
import { openPage } from './page-harness.mjs';

const VIEWPORTS = [
  { label: '430x932', width: 430, height: 932 },
  { label: '1280x800', width: 1280, height: 800 },
];

for (const vp of VIEWPORTS) {
  test(`real browser interaction Order A at ${vp.label}: fill, submit, modal appearance, hit-test, focus trap, cancel click, connect settles before disconnect, cleanup progress visible until disconnect settles, focus restore, and no stale reconnect`, async () => {
    const page = await openPage({ width: vp.width, height: vp.height });

    try {
      // 1. Fill host and pin
      await page.evaluate(`(() => {
        document.getElementById('connect-host').value = '192.0.2.10';
        document.getElementById('connect-pin').value = '12345678';
        document.getElementById('btn-connect-submit').focus();
      })()`);

      // 2. Arm connect listener before native click
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

        btnCancel.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));
        const afterTab = document.activeElement === btnCancel;

        btnCancel.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true }));
        const afterShiftTab = document.activeElement === btnCancel;

        refreshBtn.focus();
        const afterBgAttempt = document.activeElement === btnCancel;

        return { afterTab, afterShiftTab, afterBgAttempt };
      })()`);

      expect(trapResult.afterTab).toBe(true);
      expect(trapResult.afterShiftTab).toBe(true);
      expect(trapResult.afterBgAttempt).toBe(true);

      // 7. Arm disconnect listener before native click
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

      // 10. ORDER A: Settle connect FIRST while disconnect remains deferred
      await page.evaluate(`fixture.settle('connect', new Error('Cancelled by user'), true)`);

      // Assert modal remains visible and in disconnecting state until disconnect also settles
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

      // 11. Now settle deferred disconnect
      await page.evaluate(`fixture.settle('disconnect', true)`);

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

      // 12. Verify no stale reconnection: deterministic task barrier via MessageChannel
      await page.evaluate(`fixture.taskBarrier()`);
      const allConnectCalls = await page.evaluate(`fixture.calls.filter((c) => c.cmd === 'connect').length`);
      expect(allConnectCalls).toBe(1);
    } finally {
      await page.close();
    }
  }, 20000);
}
