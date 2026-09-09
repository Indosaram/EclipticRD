import test from 'node:test';
import assert from 'node:assert/strict';
import {
  validateConnectRequest,
  createConnectionManager
} from '../connection-state.js';

test('validateConnectRequest validates required host and optional 8-digit PIN', () => {
  const empty = validateConnectRequest({ host: '' });
  assert.equal(empty.ok, false);
  assert.equal(empty.field, 'host');

  const validWithoutPin = validateConnectRequest({ host: '192.168.1.10' });
  assert.equal(validWithoutPin.ok, true);
  assert.equal(validWithoutPin.args.host, '192.168.1.10');
  assert.equal(validWithoutPin.args.pin, null);

  const validWithPin = validateConnectRequest({ host: '192.168.1.10', pin: '12345678' });
  assert.equal(validWithPin.ok, true);
  assert.equal(validWithPin.args.pin, '12345678');

  const invalidPinShort = validateConnectRequest({ host: '192.168.1.10', pin: '1234' });
  assert.equal(invalidPinShort.ok, false);
  assert.equal(invalidPinShort.field, 'pin');

  const invalidPinLetters = validateConnectRequest({ host: '192.168.1.10', pin: 'abcdefgh' });
  assert.equal(invalidPinLetters.ok, false);
  assert.equal(invalidPinLetters.field, 'pin');
});

test('createConnectionManager transitions state from idle to waiting-video to streaming', async () => {
  let connectCalledWith = null;
  const mockInvoke = async (cmd, args) => {
    if (cmd === 'connect') {
      connectCalledWith = args;
      return { state: 'ready' };
    }
    if (cmd === 'disconnect') {
      return true;
    }
    return null;
  };

  const manager = createConnectionManager({ invoke: mockInvoke, hasNative: true });
  assert.equal(manager.getState(), 'idle');

  const ok = await manager.connect({ host: '10.0.0.1', pin: '87654321' });
  assert.equal(ok, true);
  assert.equal(connectCalledWith.host, '10.0.0.1');
  assert.equal(connectCalledWith.pin, '87654321');
  assert.equal(manager.getState(), 'waiting-video');

  const currentGen = manager.getGeneration();
  manager.markFrameRendered(currentGen);
  assert.equal(manager.getState(), 'streaming');

  await manager.disconnect();
  assert.equal(manager.getState(), 'idle');
});

test('createConnectionManager ignores stale frame tokens from prior generation', async () => {
  const manager = createConnectionManager({ invoke: async () => ({}), hasNative: true });
  await manager.connect({ host: '10.0.0.1' });
  assert.equal(manager.getState(), 'waiting-video');

  const oldGen = manager.getGeneration();
  await manager.disconnect();
  assert.equal(manager.getState(), 'idle');

  manager.markFrameRendered(oldGen);
  assert.equal(manager.getState(), 'idle');
});

test('createConnectionManager reports error when native bridge is missing', async () => {
  const manager = createConnectionManager({ invoke: null, hasNative: false });
  const ok = await manager.connect({ host: '10.0.0.1' });
  assert.equal(ok, false);
  assert.equal(manager.getState(), 'error');
  assert.match(manager.snapshot().lastError, /Mobile connection controls are unavailable/);
});
