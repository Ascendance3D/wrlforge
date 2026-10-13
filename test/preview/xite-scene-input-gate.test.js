'use strict';
// LINUX-EDIT-INPUT-SAFETY: the pick adapter's scene input gate. X_ITE's
// PointingDevice calls browser.buttonPressEvent / motionNotifyEvent from its
// own canvas listeners; while the editor owns the pointer the gate answers
// "no sensor" without calling X_ITE, so no authored sensor activates and the
// press falls through to the viewer. Driven with fakes whose sensor entry
// points live on the PROTOTYPE, as X_ITE's do.

const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const { createFakeX3D, createFakeBrowser } = require('./_fake-xite');

const ADAPTER = path.join(__dirname, '..', '..', 'src', 'preview', 'xite-pick-adapter.js');
const { createXitePickAdapter } = require(ADAPTER);

// One X_ITE-like browser: a sensor under the pointer, a call log.
function sensorBrowser({ version = '16.5.0', omit = null } = {}) {
  const log = [];
  const proto = {
    buttonPressEvent(x, y) { log.push(['press', x, y]); return true; },
    buttonReleaseEvent() { log.push(['release']); },
    motionNotifyEvent(x, y) { log.push(['motion', x, y]); return true; },
  };
  if (omit) delete proto[omit];
  const X3D = createFakeX3D();
  const browser = Object.setPrototypeOf(createFakeBrowser(X3D, { version }), proto);
  return { X3D, browser, log, proto };
}

const own = (o, k) => Object.prototype.hasOwnProperty.call(o, k);

test('a host that never asks keeps X_ITE behavior: nothing is wrapped (Electron)', () => {
  const { X3D, browser, log } = sensorBrowser({ version: '15.1.10' });
  createXitePickAdapter({ X3D, browser });
  assert.equal(own(browser, 'buttonPressEvent'), false);
  assert.equal(own(browser, 'motionNotifyEvent'), false);
  assert.equal(browser.buttonPressEvent(1, 2), true);
  assert.deepEqual(log, [['press', 1, 2]]);
});

test('the first sceneInput() installs the gate CLOSED: press and motion never reach X_ITE', () => {
  const { X3D, browser, log } = sensorBrowser();
  const a = createXitePickAdapter({ X3D, browser });
  assert.equal(a.sceneInput(), 'edit');
  assert.equal(browser.buttonPressEvent(5, 6), false, 'no sensor: the viewer gets the press');
  assert.equal(browser.motionNotifyEvent(5, 6), false, 'no hover, default cursor');
  assert.deepEqual(log, [], 'X_ITE saw neither event');
  // Release is never gated (nothing was activated while closed).
  browser.buttonReleaseEvent();
  assert.deepEqual(log, [['release']]);
});

test('Interact opens the gate; leaving it closes again; only a strict true opens', () => {
  const { X3D, browser, log } = sensorBrowser();
  const a = createXitePickAdapter({ X3D, browser });
  assert.equal(a.sceneInput(true), 'interact');
  assert.equal(browser.buttonPressEvent(1, 1), true);
  assert.equal(browser.motionNotifyEvent(2, 2), true);
  assert.deepEqual(log, [['press', 1, 1], ['motion', 2, 2]]);
  assert.equal(a.sceneInput(false), 'edit');
  assert.equal(browser.buttonPressEvent(3, 3), false);
  for (const v of ['true', 1, {}, null]) {
    a.sceneInput(true);
    assert.equal(a.sceneInput(v), 'edit', `sceneInput(${JSON.stringify(v)}) closes`);
  }
  assert.equal(a.sceneInput(), 'edit', 'a query does not change the state');
  assert.equal(log.length, 2);
});

test('the gate wraps this ONE browser: another browser on the page is untouched', () => {
  const one = sensorBrowser();
  const two = sensorBrowser();
  createXitePickAdapter({ X3D: one.X3D, browser: one.browser }).sceneInput(false);
  createXitePickAdapter({ X3D: two.X3D, browser: two.browser });
  assert.equal(one.browser.buttonPressEvent(0, 0), false);
  assert.equal(two.browser.buttonPressEvent(0, 0), true);
  assert.equal(own(one.proto, 'buttonPressEvent'), true, 'the prototype method is not replaced');
});

test('dispose() removes the wrappers; a disposed adapter reports unavailable', () => {
  const { X3D, browser, log } = sensorBrowser();
  const a = createXitePickAdapter({ X3D, browser });
  a.sceneInput(false);
  a.dispose();
  assert.equal(own(browser, 'buttonPressEvent'), false);
  assert.equal(own(browser, 'motionNotifyEvent'), false);
  assert.equal(browser.buttonPressEvent(1, 1), true);
  assert.deepEqual(log, [['press', 1, 1]]);
  assert.match(a.sceneInput(true), /^unavailable:/);
  // A disposed adapter that never installed never installs later.
  const b = sensorBrowser();
  const a2 = createXitePickAdapter({ X3D: b.X3D, browser: b.browser });
  a2.dispose();
  assert.match(a2.sceneInput(false), /^unavailable:/);
  assert.equal(own(b.browser, 'buttonPressEvent'), false);
});

test('fails closed and visible: a missing entry point or a foreign wrapper is unavailable, never replaced', () => {
  for (const k of ['buttonPressEvent', 'motionNotifyEvent', 'buttonReleaseEvent']) {
    const { X3D, browser } = sensorBrowser({ omit: k });
    const a = createXitePickAdapter({ X3D, browser });
    assert.equal(a.sceneInput(false), 'unavailable:pointer-sensor-entry-missing', k);
    assert.equal(a.sceneInput(true), 'unavailable:pointer-sensor-entry-missing', k);
  }
  const { X3D, browser } = sensorBrowser();
  const foreign = () => 'foreign';
  browser.motionNotifyEvent = foreign;
  const a = createXitePickAdapter({ X3D, browser });
  assert.equal(a.sceneInput(false), 'unavailable:pointer-sensor-entry-already-wrapped');
  assert.equal(browser.motionNotifyEvent, foreign);
  assert.equal(own(browser, 'buttonPressEvent'), false, 'nothing half-installed');
});

test('a dispose never removes a wrapper it does not own', () => {
  const { X3D, browser } = sensorBrowser();
  const a = createXitePickAdapter({ X3D, browser });
  a.sceneInput(false);
  const later = () => 'later';
  browser.buttonPressEvent = later;
  a.dispose();
  assert.equal(browser.buttonPressEvent, later);
  assert.equal(own(browser, 'motionNotifyEvent'), false);
});

// ---- PR #149 review: the gate is verified on every call and fails closed ----

test('a wrapper replaced after install is unavailable, the gate closes, and it never reopens on its own', () => {
  const { X3D, browser, log } = sensorBrowser();
  const a = createXitePickAdapter({ X3D, browser });
  assert.equal(a.sceneInput(true), 'interact');
  const foreign = function () { return 'foreign'; };
  browser.buttonPressEvent = foreign;
  assert.equal(a.sceneInput(), 'unavailable:pointer-sensor-entry-already-wrapped');
  assert.equal(a.sceneInput(true), 'unavailable:pointer-sensor-entry-already-wrapped', 'Interact cannot open it');
  assert.equal(browser.buttonPressEvent, foreign, 'a foreign wrapper is never replaced');
  // The other entry point is still this adapter's wrapper, and it is CLOSED.
  assert.equal(browser.motionNotifyEvent(1, 1), false);
  assert.deepEqual(log, []);
});

test('recovery only through a passing check: the gate comes back CLOSED (edit), never in Interact', () => {
  const { X3D, browser, log } = sensorBrowser();
  const a = createXitePickAdapter({ X3D, browser });
  assert.equal(a.sceneInput(true), 'interact');
  browser.buttonPressEvent = () => 'foreign';
  assert.match(a.sceneInput(), /^unavailable:/);
  delete browser.buttonPressEvent; // the foreign wrapper goes away
  assert.equal(a.sceneInput(), 'edit', 'reinstalled, verified, closed');
  assert.equal(browser.buttonPressEvent(2, 2), false);
  assert.equal(browser.motionNotifyEvent(2, 2), false);
  assert.deepEqual(log, []);
  assert.equal(a.sceneInput(true), 'interact');
  assert.equal(browser.buttonPressEvent(3, 3), true);
  assert.deepEqual(log, [['press', 3, 3]]);
});

test('an entry point that appears later recovers on the next call', () => {
  const { X3D, browser, proto } = sensorBrowser({ omit: 'motionNotifyEvent' });
  const a = createXitePickAdapter({ X3D, browser });
  assert.equal(a.sceneInput(false), 'unavailable:pointer-sensor-entry-missing');
  proto.motionNotifyEvent = () => true;
  assert.equal(a.sceneInput(), 'edit');
  assert.equal(browser.motionNotifyEvent(0, 0), false);
});

test('an install error is unavailable, not a crash', () => {
  const { X3D, browser } = sensorBrowser();
  const a = createXitePickAdapter({ X3D, browser });
  Object.defineProperty(browser, 'motionNotifyEvent', {
    configurable: true,
    get() { return function () { return true; }; },
    set() { throw new Error('read-only'); },
  });
  // The getter makes it an own, foreign entry point: refused before any write.
  assert.equal(a.sceneInput(false), 'unavailable:pointer-sensor-entry-already-wrapped');
  delete browser.motionNotifyEvent;
  Object.preventExtensions(browser);
  assert.equal(a.sceneInput(false), 'unavailable:pointer-sensor-gate-install-error');
});
