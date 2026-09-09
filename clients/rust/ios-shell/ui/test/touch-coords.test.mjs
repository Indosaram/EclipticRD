import test from 'node:test';
import assert from 'node:assert/strict';
import {
  calculateAspectFit,
  normalizeTouchCoordinates,
  mapTouchPhase,
  createTouchTracker
} from '../touch-coords.js';

test('calculateAspectFit calculates correct pillarbox in wide container', () => {
  const fit = calculateAspectFit(1000, 500, 500, 500);
  assert.equal(fit.displayWidth, 500);
  assert.equal(fit.displayHeight, 500);
  assert.equal(fit.offsetX, 250);
  assert.equal(fit.offsetY, 0);
});

test('calculateAspectFit calculates correct letterbox in tall container', () => {
  const fit = calculateAspectFit(500, 1000, 500, 500);
  assert.equal(fit.displayWidth, 500);
  assert.equal(fit.displayHeight, 500);
  assert.equal(fit.offsetX, 0);
  assert.equal(fit.offsetY, 250);
});

test('calculateAspectFit handles zero or invalid dimensions gracefully', () => {
  const fit = calculateAspectFit(0, 500, 1920, 1080);
  assert.equal(fit.displayWidth, 0);
  assert.equal(fit.displayHeight, 0);
  assert.equal(fit.offsetX, 0);
  assert.equal(fit.offsetY, 0);
});

test('normalizeTouchCoordinates normalizes coordinates accurately', () => {
  const fit = { displayWidth: 400, displayHeight: 200, offsetX: 50, offsetY: 25 };
  const rect = { left: 10, top: 20 };

  const normCenter = normalizeTouchCoordinates(10 + 50 + 200, 20 + 25 + 100, fit, rect);
  assert.equal(normCenter.x, 0.5);
  assert.equal(normCenter.y, 0.5);

  const normTopLeft = normalizeTouchCoordinates(60, 45, fit, rect);
  assert.equal(normTopLeft.x, 0.0);
  assert.equal(normTopLeft.y, 0.0);

  const normBottomRight = normalizeTouchCoordinates(60 + 400, 45 + 200, fit, rect);
  assert.equal(normBottomRight.x, 1.0);
  assert.equal(normBottomRight.y, 1.0);
});

test('normalizeTouchCoordinates clamps out-of-bounds coordinates to 0..1', () => {
  const fit = { displayWidth: 400, displayHeight: 200, offsetX: 50, offsetY: 25 };
  const rect = { left: 0, top: 0 };

  const clampedLow = normalizeTouchCoordinates(10, 10, fit, rect);
  assert.equal(clampedLow.x, 0.0);
  assert.equal(clampedLow.y, 0.0);

  const clampedHigh = normalizeTouchCoordinates(600, 400, fit, rect);
  assert.equal(clampedHigh.x, 1.0);
  assert.equal(clampedHigh.y, 1.0);
});

test('mapTouchPhase maps browser touch/pointer events to contract phases', () => {
  assert.equal(mapTouchPhase('pointerdown'), 'began');
  assert.equal(mapTouchPhase('touchstart'), 'began');
  assert.equal(mapTouchPhase('pointermove'), 'moved');
  assert.equal(mapTouchPhase('touchmove'), 'moved');
  assert.equal(mapTouchPhase('pointerup'), 'ended');
  assert.equal(mapTouchPhase('touchend'), 'ended');
  assert.equal(mapTouchPhase('pointercancel'), 'cancelled');
  assert.equal(mapTouchPhase('touchcancel'), 'cancelled');
  assert.equal(mapTouchPhase('unknown'), 'cancelled');
});

test('createTouchTracker tracks and cancels touches', () => {
  const tracker = createTouchTracker();
  assert.equal(tracker.getActiveCount(), 0);

  tracker.addTouch(1, 0.2, 0.3);
  tracker.addTouch(2, 0.7, 0.8);
  assert.equal(tracker.getActiveCount(), 2);
  assert.equal(tracker.hasTouch(1), true);
  assert.equal(tracker.hasTouch(2), true);

  tracker.updateTouch(1, 0.25, 0.35);
  tracker.removeTouch(2);
  assert.equal(tracker.getActiveCount(), 1);
  assert.equal(tracker.hasTouch(2), false);

  const cancelled = tracker.cancelAll();
  assert.equal(cancelled.length, 1);
  assert.equal(cancelled[0].id, 1);
  assert.equal(cancelled[0].x, 0.25);
  assert.equal(cancelled[0].y, 0.35);
  assert.equal(cancelled[0].phase, 'cancelled');
  assert.equal(tracker.getActiveCount(), 0);
});
