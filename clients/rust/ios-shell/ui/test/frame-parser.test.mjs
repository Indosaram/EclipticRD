import test from 'node:test';
import assert from 'node:assert/strict';
import { parseNv12Frame } from '../frame-parser.js';

test('parseNv12Frame parses valid 4x2 NV12 frame buffer', () => {
  const width = 4;
  const height = 2;
  const sequence = 42n;
  const yLength = width * height;
  const uvLength = width * Math.ceil(height / 2);
  const totalLength = 16 + yLength + uvLength;

  const buffer = new ArrayBuffer(totalLength);
  const view = new DataView(buffer);
  view.setUint32(0, width, true);
  view.setUint32(4, height, true);
  view.setBigUint64(8, sequence, true);

  const u8 = new Uint8Array(buffer);
  for (let i = 0; i < yLength; i++) {
    u8[16 + i] = 0x80;
  }
  for (let i = 0; i < uvLength; i++) {
    u8[16 + yLength + i] = 0x40;
  }

  const parsed = parseNv12Frame(buffer);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.width, 4);
  assert.equal(parsed.height, 2);
  assert.equal(parsed.sequence, 42n);
  assert.equal(parsed.yLength, 8);
  assert.equal(parsed.uvLength, 4);
  assert.equal(parsed.yOffset, 16);
  assert.equal(parsed.uvOffset, 24);
  assert.equal(parsed.yData.length, 8);
  assert.equal(parsed.uvData.length, 4);
  assert.equal(parsed.yData[0], 0x80);
  assert.equal(parsed.uvData[0], 0x40);
});

test('parseNv12Frame rejects buffer shorter than 16-byte header', () => {
  const buffer = new ArrayBuffer(12);
  const parsed = parseNv12Frame(buffer);
  assert.equal(parsed.ok, false);
  assert.match(parsed.error, /shorter than 16-byte header/);
});

test('parseNv12Frame rejects truncated frame data', () => {
  const buffer = new ArrayBuffer(20);
  const view = new DataView(buffer);
  view.setUint32(0, 1920, true);
  view.setUint32(4, 1080, true);
  view.setBigUint64(8, 1n, true);

  const parsed = parseNv12Frame(buffer);
  assert.equal(parsed.ok, false);
  assert.match(parsed.error, /Buffer truncated/);
});

test('parseNv12Frame rejects zero dimensions', () => {
  const buffer = new ArrayBuffer(16);
  const view = new DataView(buffer);
  view.setUint32(0, 0, true);
  view.setUint32(4, 100, true);
  view.setBigUint64(8, 1n, true);

  const parsed = parseNv12Frame(buffer);
  assert.equal(parsed.ok, false);
  assert.match(parsed.error, /Invalid zero dimensions/);
});

test('parseNv12Frame accepts Uint8Array slices', () => {
  const total = 16 + 4 + 2;
  const bigBuffer = new ArrayBuffer(total + 32);
  const slice = new Uint8Array(bigBuffer, 16, total);
  const view = new DataView(slice.buffer, slice.byteOffset, slice.byteLength);
  view.setUint32(0, 2, true);
  view.setUint32(4, 2, true);
  view.setBigUint64(8, 100n, true);

  const parsed = parseNv12Frame(slice);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.width, 2);
  assert.equal(parsed.height, 2);
  assert.equal(parsed.sequence, 100n);
});
