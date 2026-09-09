import test from 'node:test';
import assert from 'node:assert/strict';
import { createInputQueue } from '../input-queue.js';

function createDeferred() {
  let resolve;
  let reject;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

test('preserves execution order for serialized input commands', async () => {
  const invoked = [];
  const invoke1Started = createDeferred();
  const gate1 = createDeferred();
  const invoke2Started = createDeferred();

  const mockInvoke = async (cmd, args) => {
    invoked.push({ cmd, args, start: true });
    if (cmd === 'first') {
      invoke1Started.resolve();
      await gate1.promise;
    }
    if (cmd === 'second') {
      invoke2Started.resolve();
    }
    invoked.push({ cmd, args, end: true });
    return true;
  };

  const queue = createInputQueue({
    invoke: mockInvoke,
    isCurrent: () => true,
    getGeneration: () => 1
  });

  const p1 = queue.enqueue('first', { id: 1 });
  const p2 = queue.enqueue('second', { id: 2 });

  await invoke1Started.promise;
  assert.equal(invoked.length, 1);
  assert.equal(invoked[0].cmd, 'first');
  assert.equal(invoked[0].start, true);

  gate1.resolve();
  await invoke2Started.promise;
  await Promise.all([p1, p2]);

  assert.equal(invoked.length, 4);
  assert.equal(invoked[0].cmd, 'first');
  assert.equal(invoked[0].start, true);
  assert.equal(invoked[1].cmd, 'first');
  assert.equal(invoked[1].end, true);
  assert.equal(invoked[2].cmd, 'second');
  assert.equal(invoked[2].start, true);
  assert.equal(invoked[3].cmd, 'second');
  assert.equal(invoked[3].end, true);
});

test('drops queued commands when generation changes before invocation', async () => {
  const invoked = [];
  const invoke1Started = createDeferred();
  const gate1 = createDeferred();
  let currentGen = 1;

  const mockInvoke = async (cmd, args) => {
    invoked.push({ cmd, args });
    if (cmd === 'first') {
      invoke1Started.resolve();
      await gate1.promise;
    }
    return true;
  };

  const queue = createInputQueue({
    invoke: mockInvoke,
    isCurrent: (gen) => gen === currentGen,
    getGeneration: () => currentGen
  });

  const p1 = queue.enqueue('first', { id: 1 });
  const p2 = queue.enqueue('second', { id: 2 });

  await invoke1Started.promise;
  assert.equal(invoked.length, 1);
  assert.equal(invoked[0].cmd, 'first');

  currentGen = 2;
  gate1.resolve();

  const [res1, res2] = await Promise.all([p1, p2]);
  assert.equal(res1.dropped, false);
  assert.equal(res2.dropped, true);
  assert.equal(invoked.length, 1);
  assert.equal(invoked[0].cmd, 'first');
});

test('captures generation at enqueue time rather than invocation time', async () => {
  const invoked = [];
  const invoke1Started = createDeferred();
  const gate1 = createDeferred();
  let activeGen = 1;

  const mockInvoke = async (cmd, args) => {
    invoked.push({ cmd, args });
    if (cmd === 'first') {
      invoke1Started.resolve();
      await gate1.promise;
    }
    return true;
  };

  const queue = createInputQueue({
    invoke: mockInvoke,
    isCurrent: (gen) => gen === activeGen,
    getGeneration: () => activeGen
  });

  const p1 = queue.enqueue('first', { id: 1 });
  await invoke1Started.promise;

  const p2 = queue.enqueue('stale_second', { id: 2 });

  activeGen = 2;
  const p3 = queue.enqueue('fresh_third', { id: 3 });

  gate1.resolve();
  const [r1, r2, r3] = await Promise.all([p1, p2, p3]);

  assert.equal(r1.dropped, false);
  assert.equal(r2.dropped, true);
  assert.equal(r3.dropped, false);

  assert.equal(invoked.length, 2);
  assert.equal(invoked[0].cmd, 'first');
  assert.equal(invoked[1].cmd, 'fresh_third');
});

test('handles invoke rejection without breaking subsequent queue processing', async () => {
  const invoked = [];
  const mockInvoke = async (cmd) => {
    invoked.push(cmd);
    if (cmd === 'failing') {
      throw new Error('command failed');
    }
    return true;
  };

  const queue = createInputQueue({
    invoke: mockInvoke,
    isCurrent: () => true,
    getGeneration: () => 1
  });

  const r1 = await queue.enqueue('failing', {});
  assert.equal(r1.dropped, false);
  assert.equal(Boolean(r1.error), true);

  const r2 = await queue.enqueue('succeeding', {});
  assert.equal(r2.dropped, false);
  assert.equal(r2.result, true);

  assert.deepEqual(invoked, ['failing', 'succeeding']);
});
