import test from 'node:test';
import assert from 'node:assert/strict';
import {
  KEY_CODES,
  MODIFIER_BITS,
  mapNamedKeyToCode,
  createModifierTracker,
  validateKeyPayload
} from '../keyboard-mapper.js';

test('mapNamedKeyToCode maps known keys to wire key codes', () => {
  assert.equal(mapNamedKeyToCode('Escape'), 0x35);
  assert.equal(mapNamedKeyToCode('Tab'), 0x30);
  assert.equal(mapNamedKeyToCode('ArrowUp'), 0x7e);
  assert.equal(mapNamedKeyToCode('ArrowDown'), 0x7d);
  assert.equal(mapNamedKeyToCode('ArrowLeft'), 0x7b);
  assert.equal(mapNamedKeyToCode('ArrowRight'), 0x7c);
  assert.equal(mapNamedKeyToCode('Return'), 0x24);
  assert.equal(mapNamedKeyToCode('Backspace'), 0x33);
  assert.equal(mapNamedKeyToCode('Space'), 0x31);
  assert.equal(mapNamedKeyToCode('NonExistent'), null);
});

test('createModifierTracker toggles and clears modifier bitflags', () => {
  const tracker = createModifierTracker();
  assert.equal(tracker.get(), 0);

  tracker.toggle(MODIFIER_BITS.SHIFT);
  assert.equal(tracker.get(), 1);
  assert.equal(tracker.has(MODIFIER_BITS.SHIFT), true);
  assert.equal(tracker.has(MODIFIER_BITS.CONTROL), false);

  tracker.toggle(MODIFIER_BITS.CONTROL);
  assert.equal(tracker.get(), 3);
  assert.equal(tracker.has(MODIFIER_BITS.SHIFT), true);
  assert.equal(tracker.has(MODIFIER_BITS.CONTROL), true);

  tracker.toggle(MODIFIER_BITS.SHIFT);
  assert.equal(tracker.get(), 2);
  assert.equal(tracker.has(MODIFIER_BITS.SHIFT), false);
  assert.equal(tracker.has(MODIFIER_BITS.CONTROL), true);

  tracker.reset();
  assert.equal(tracker.get(), 0);
});

test('validateKeyPayload validates well-formed send_key payloads', () => {
  const valid = validateKeyPayload({ keyCode: 0x35, down: true, modifiers: 1 });
  assert.equal(valid.ok, true);
  assert.equal(valid.payload.keyCode, 53);
  assert.equal(valid.payload.down, true);
  assert.equal(valid.payload.modifiers, 1);
});

test('validateKeyPayload rejects invalid key payload types', () => {
  assert.equal(validateKeyPayload(null).ok, false);
  assert.equal(validateKeyPayload({ keyCode: -1, down: true }).ok, false);
  assert.equal(validateKeyPayload({ keyCode: 0x35, down: 'true' }).ok, false);
  assert.equal(validateKeyPayload({ keyCode: 0x35, down: true, modifiers: -5 }).ok, false);
});
