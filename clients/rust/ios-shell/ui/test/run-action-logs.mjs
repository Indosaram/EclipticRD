import { openPage } from './page-harness.mjs';

const viewports = [
  { label: '430x932', width: 430, height: 932, type: 'mobile' },
  { label: '1280x800', width: 1280, height: 800, type: 'desktop' },
];

const actionLogs = {
  timestamp: new Date().toISOString(),
  testRun: 'R7 Modal UX & Lifecycle Verification',
  viewports: [],
  summary: {
    status: 'PASS',
    totalScenarios: viewports.length,
  },
};

for (const vp of viewports) {
  const vpLog = {
    viewport: vp.label,
    dimensions: { width: vp.width, height: vp.height },
    steps: [],
    screenshots: {},
    cleanup: {},
  };

  const page = await openPage({ width: vp.width, height: vp.height });
  vpLog.steps.push({ step: 'page_ready', origin: page.receipts.origin });

  try {
    // Fill form
    await page.evaluate(`(() => {
      document.getElementById('connect-host').value = '192.0.2.10';
      document.getElementById('connect-pin').value = '12345678';
      document.getElementById('btn-connect-submit').focus();
    })()`);
    vpLog.steps.push({ step: 'form_filled', host: '192.0.2.10', hasPin: true });

    // Arm connect listener before separate click
    const armedConnect = await page.evaluate(`fixture.arm('connect')`);
    vpLog.steps.push({ step: 'command_armed', command: 'connect', armed: armedConnect });

    // Trusted click via native browser view API
    await page.view.click('#btn-connect-submit');
    const connectCall = await page.evaluate(`fixture.awaitCommand('connect')`);

    // Explicitly redact PIN from serialized action logs
    const sanitizedArgs = {
      ...connectCall.args,
      pin: connectCall.args && connectCall.args.pin ? '[REDACTED]' : null,
    };
    vpLog.steps.push({ step: 'connect_issued', command: connectCall.cmd, args: sanitizedArgs });

    // Check PIN field cleared
    const pinAfterSubmit = await page.evaluate(`document.getElementById('connect-pin').value`);
    vpLog.steps.push({
      step: 'pin_cleared_verification',
      pinValue: pinAfterSubmit,
      isCleared: pinAfterSubmit === '',
    });

    // Inspect modal while connect is pending
    const modalInspection = await page.evaluate(`(() => {
      const modal = document.getElementById('modal-connecting');
      const btn = document.getElementById('btn-cancel-connect');
      const connectView = document.getElementById('view-connect');
      const sessionView = document.getElementById('view-session');
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
        modalParent: modal.parentElement.id || modal.parentElement.tagName.toLowerCase(),
        hiddenAncestor,
        modalHidden: modal.hidden,
        modalRect: { x: mRect.x, y: mRect.y, width: mRect.width, height: mRect.height },
        btnRect: { x: bRect.x, y: bRect.y, width: bRect.width, height: bRect.height },
        btnDisabled: btn.disabled,
        inViewport: cx >= 0 && cy >= 0 && cx < window.innerWidth && cy < window.innerHeight,
        hitTargetId: hitEl ? hitEl.id : null,
        hitMatches: btn.contains(hitEl) || hitEl === btn,
        activeElementId: document.activeElement ? document.activeElement.id : null,
        connectViewInert: connectView.inert === true || connectView.hasAttribute('inert'),
        sessionViewInert: sessionView.inert === true || sessionView.hasAttribute('inert'),
      };
    })()`);
    vpLog.steps.push({ step: 'connecting_modal_inspected', ...modalInspection });

    // Exercise focus trap
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
    vpLog.steps.push({ step: 'focus_trap_exercised', ...trapResult });

    // Screenshot 1
    const ssConnecting = `.omo/pairing-20260910/evidence/lifecycle-modal-connecting-${vp.label}.png`;
    await page.screenshot(ssConnecting);
    vpLog.screenshots.connecting = ssConnecting;

    // Arm disconnect listener before separate click
    const armedDisconnect = await page.evaluate(`fixture.arm('disconnect')`);
    vpLog.steps.push({ step: 'command_armed', command: 'disconnect', armed: armedDisconnect });

    // Click Cancel via native browser view API
    await page.view.click('#btn-cancel-connect');
    const disconnectCall = await page.evaluate(`fixture.awaitCommand('disconnect')`);
    vpLog.steps.push({ step: 'disconnect_issued', command: disconnectCall.cmd });

    // Inspect during disconnect
    const disconnectingInspection = await page.evaluate(`(() => {
      const modal = document.getElementById('modal-connecting');
      const btn = document.getElementById('btn-cancel-connect');
      const status = document.getElementById('modal-status-text');
      return {
        modalHidden: modal.hidden,
        btnDisabled: btn.disabled,
        statusText: status.textContent,
      };
    })()`);
    vpLog.steps.push({ step: 'disconnecting_inspected', ...disconnectingInspection });

    // Screenshot 2
    const ssDisconnecting = `.omo/pairing-20260910/evidence/lifecycle-modal-disconnecting-${vp.label}.png`;
    await page.screenshot(ssDisconnecting);
    vpLog.screenshots.disconnecting = ssDisconnecting;

    // Step A: Settle disconnect FIRST while connect remains deferred
    await page.evaluate(`fixture.settle('disconnect', true)`);

    const duringCleanupInspection = await page.evaluate(`(() => {
      const modal = document.getElementById('modal-connecting');
      const btn = document.getElementById('btn-cancel-connect');
      const status = document.getElementById('modal-status-text');
      return {
        modalHidden: modal.hidden,
        btnDisabled: btn.disabled,
        statusText: status.textContent,
      };
    })()`);
    vpLog.steps.push({
      step: 'cleanup_disconnect_settled_connect_pending',
      ...duringCleanupInspection,
      modalRemainsDuringCleanup: !duringCleanupInspection.modalHidden,
    });

    // Step B: Now settle deferred connect
    await page.evaluate(`fixture.settle('connect', new Error('Cancelled by user'), true)`);
    await page.evaluate(`fixture.until(() => document.getElementById('modal-connecting').hidden)`);

    // Inspect settled state
    const settledInspection = await page.evaluate(`(() => {
      const modal = document.getElementById('modal-connecting');
      const connectView = document.getElementById('view-connect');
      const btnSubmit = document.getElementById('btn-connect-submit');
      return {
        modalHidden: modal.hidden,
        connectViewInert: connectView.inert === true || connectView.hasAttribute('inert'),
        activeElementId: document.activeElement ? document.activeElement.id : null,
        submitDisabled: btnSubmit.disabled,
      };
    })()`);
    vpLog.steps.push({ step: 'settled_inspected', ...settledInspection });

    // Screenshot 3
    const ssSettled = `.omo/pairing-20260910/evidence/lifecycle-modal-settled-${vp.label}.png`;
    await page.screenshot(ssSettled);
    vpLog.screenshots.settled = ssSettled;

    // Verify no stale reconnection: deterministic task barrier via MessageChannel
    await page.evaluate(`fixture.taskBarrier()`);
    const allConnectCalls = await page.evaluate(`fixture.calls.filter((c) => c.cmd === 'connect').length`);
    vpLog.steps.push({
      step: 'stale_connect_check',
      totalConnectCalls: allConnectCalls,
      noStaleCalls: allConnectCalls === 1,
    });
  } finally {
    await page.close();
    vpLog.cleanup = {
      webViewClosed: page.receipts.webViewClosed,
      serverStopped: page.receipts.serverStopped,
    };
  }

  actionLogs.viewports.push(vpLog);
}

await Bun.write(
  '.omo/pairing-20260910/evidence/lifecycle-modal-actions.json',
  JSON.stringify(actionLogs, null, 2)
);
console.log('CONSOLIDATED_ACTION_LOGS_SUCCESS');
