import { test, expect } from 'bun:test';
import { openPage } from './page-harness.mjs';

test('connecting modal is child of body, not hidden session view', async () => {
  const page = await openPage();
  try {
    const info = await page.evaluate(`(() => {
      const modal = document.getElementById('modal-connecting');
      const sessionView = document.getElementById('view-session');
      return {
        modalExists: !!modal,
        parentIsBody: modal ? modal.parentElement === document.body : false,
        parentIsSession: modal ? modal.parentElement === sessionView : false,
        parentId: modal && modal.parentElement ? modal.parentElement.id : null,
      };
    })()`);

    expect(info.modalExists).toBe(true);
    expect(info.parentIsSession).toBe(false);
    expect(info.parentId).not.toBe('view-session');
    expect(info.parentIsBody).toBe(true);
  } finally {
    await page.close();
  }
});

test('cancel button is clickable and dispatches disconnect while connecting', async () => {
  const page = await openPage();
  try {
    // Fill direct connect form
    await page.evaluate(`(() => {
      document.getElementById('connect-host').value = '192.0.2.10';
      document.getElementById('connect-pin').value = '12345678';
    })()`);

    // Submit form, keeping connect IPC pending
    await page.evaluate(`fixture.command('connect', () => {
      document.getElementById('btn-connect-submit').click();
    })`);

    // Check reachability of Cancel button
    const cancelInfo = await page.evaluate(`(() => {
      const btn = document.getElementById('btn-cancel-connect');
      const modal = document.getElementById('modal-connecting');
      const r = btn.getBoundingClientRect();
      let hiddenAncestor = null;
      let cur = btn.parentElement;
      while (cur && cur !== document.body) {
        if (cur.hasAttribute('hidden') || getComputedStyle(cur).display === 'none') {
          hiddenAncestor = cur.id || cur.tagName.toLowerCase();
          break;
        }
        cur = cur.parentElement;
      }
      return {
        modalHidden: modal ? modal.hidden : true,
        modalRectWidth: modal ? modal.getBoundingClientRect().width : 0,
        btnWidth: r.width,
        btnHeight: r.height,
        hiddenAncestor,
        disabled: btn.disabled,
      };
    })()`);

    expect(cancelInfo.modalHidden).toBe(false);
    expect(cancelInfo.modalRectWidth).toBeGreaterThan(0);
    expect(cancelInfo.hiddenAncestor).toBeNull();
    expect(cancelInfo.btnWidth).toBeGreaterThan(0);
    expect(cancelInfo.btnHeight).toBeGreaterThan(0);
    expect(cancelInfo.disabled).toBe(false);

    // Click cancel to initiate prompt disconnect
    const disconnectCall = await page.evaluate(`fixture.command('disconnect', () => {
      document.getElementById('btn-cancel-connect').click();
    })`);
    expect(disconnectCall.cmd).toBe('disconnect');

    // Settle disconnect first while connect remains deferred
    await page.evaluate(`fixture.settle('disconnect', true)`);

    // Modal must REMAIN visible during cleanup until connect also settles
    const duringCleanup = await page.evaluate(`(() => {
      const modal = document.getElementById('modal-connecting');
      const btn = document.getElementById('btn-cancel-connect');
      return {
        modalHidden: modal.hidden,
        btnDisabled: btn.disabled,
      };
    })()`);
    expect(duringCleanup.modalHidden).toBe(false);
    expect(duringCleanup.btnDisabled).toBe(true);

    // Now settle deferred connect
    await page.evaluate(`fixture.settle('connect', new Error('cancelled'), true)`);

    // Modal now hides
    await page.evaluate(`fixture.until(() => document.getElementById('modal-connecting').hidden)`);
    const afterSettle = await page.evaluate(`document.getElementById('modal-connecting').hidden`);
    expect(afterSettle).toBe(true);
  } finally {
    await page.close();
  }
});

test('background views are marked inert and aria-hidden when modal is open', async () => {
  const page = await openPage();
  try {
    await page.evaluate(`(() => {
      document.getElementById('connect-host').value = '192.0.2.10';
    })()`);

    await page.evaluate(`fixture.command('connect', () => {
      document.getElementById('btn-connect-submit').click();
    })`);

    const inertInfo = await page.evaluate(`(() => {
      const connectView = document.getElementById('view-connect');
      const sessionView = document.getElementById('view-session');
      return {
        connectViewInert: connectView.inert === true || connectView.hasAttribute('inert'),
        sessionViewInert: sessionView.inert === true || sessionView.hasAttribute('inert'),
        connectViewAriaHidden: connectView.getAttribute('aria-hidden'),
      };
    })()`);

    expect(inertInfo.connectViewInert).toBe(true);
    expect(inertInfo.sessionViewInert).toBe(true);
    expect(inertInfo.connectViewAriaHidden).toBe('true');
  } finally {
    await page.close();
  }
});

test('focus is trapped to modal and restored on close', async () => {
  const page = await openPage();
  try {
    await page.evaluate(`(() => {
      document.getElementById('connect-host').value = '192.0.2.10';
      document.getElementById('btn-connect-submit').focus();
    })()`);

    await page.evaluate(`fixture.command('connect', () => {
      document.getElementById('btn-connect-submit').click();
    })`);

    const trapResult = await page.evaluate(`(() => {
      const btnCancel = document.getElementById('btn-cancel-connect');
      const refreshBtn = document.getElementById('btn-refresh-discovery');

      const initialFocus = document.activeElement === btnCancel;

      // Tab on Cancel button must stay inside modal
      btnCancel.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));
      const afterTab = document.activeElement === btnCancel;

      // Shift+Tab on Cancel button must stay inside modal
      btnCancel.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true }));
      const afterShiftTab = document.activeElement === btnCancel;

      // Attempting to focus background element must redirect back to modal
      refreshBtn.focus();
      const afterBgAttempt = document.activeElement === btnCancel;

      return { initialFocus, afterTab, afterShiftTab, afterBgAttempt };
    })()`);

    expect(trapResult.initialFocus).toBe(true);
    expect(trapResult.afterTab).toBe(true);
    expect(trapResult.afterShiftTab).toBe(true);
    expect(trapResult.afterBgAttempt).toBe(true);

    // Click cancel and settle disconnect first while connect remains deferred
    await page.evaluate(`fixture.command('disconnect', () => {
      document.getElementById('btn-cancel-connect').click();
    })`);

    // Settle disconnect while connect is still pending
    await page.evaluate(`(() => {
      fixture.settle('disconnect', true);
    })()`);

    // Modal must STILL be visible because connect is still deferred
    const stillWaitingConnect = await page.evaluate(`(() => {
      const modal = document.getElementById('modal-connecting');
      return !modal.hidden && document.getElementById('modal-status-text').textContent === 'Disconnecting...';
    })()`);
    expect(stillWaitingConnect).toBe(true);

    // Now settle deferred connect
    await page.evaluate(`(() => {
      fixture.settle('connect', new Error('Cancelled'), true);
    })()`);

    // Wait until state returns to idle
    await page.evaluate(`fixture.until(() => document.getElementById('modal-connecting').hidden)`);

    const restoredFocus = await page.evaluate(`(() => {
      const btnSubmit = document.getElementById('btn-connect-submit');
      return document.activeElement === btnSubmit;
    })()`);
    expect(restoredFocus).toBe(true);
  } finally {
    await page.close();
  }
});

test('pin input is cleared upon submission', async () => {
  const page = await openPage();
  try {
    await page.evaluate(`(() => {
      document.getElementById('connect-host').value = '192.0.2.10';
      document.getElementById('connect-pin').value = '12345678';
    })()`);

    await page.evaluate(`fixture.command('connect', () => {
      document.getElementById('btn-connect-submit').click();
    })`);

    const pinValue = await page.evaluate(`document.getElementById('connect-pin').value`);
    expect(pinValue).toBe('');
  } finally {
    await page.close();
  }
});

test('cleanup progress visible and cancel button disabled during disconnecting', async () => {
  const page = await openPage();
  try {
    await page.evaluate(`(() => {
      document.getElementById('connect-host').value = '192.0.2.10';
    })()`);

    await page.evaluate(`fixture.command('connect', () => {
      document.getElementById('btn-connect-submit').click();
    })`);

    // Click cancel to initiate disconnecting
    await page.evaluate(`fixture.command('disconnect', () => {
      document.getElementById('btn-cancel-connect').click();
    })`);

    const disconnectingInfo = await page.evaluate(`(() => {
      const modal = document.getElementById('modal-connecting');
      const btnCancel = document.getElementById('btn-cancel-connect');
      const statusText = document.getElementById('modal-status-text');
      return {
        modalHidden: modal.hidden,
        btnDisabled: btnCancel.disabled,
        statusText: statusText.textContent,
      };
    })()`);

    expect(disconnectingInfo.modalHidden).toBe(false);
    expect(disconnectingInfo.btnDisabled).toBe(true);
    expect(disconnectingInfo.statusText).toBe('Disconnecting...');
  } finally {
    await page.close();
  }
});
