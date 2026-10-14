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
    // X_ITE's concrete node registry; no keyboard sensor component loaded.
    getConcreteNodes() { return new Map([['Transform', {}]]); },
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

// ---- LINUX-KEY-INPUT-SAFETY: the keyboard sensor gate -----------------------
// X_ITE's KeyDeviceSensor context calls keydown(event) / keyup(event) on each
// enabled KeySensor / StringSensor (prototype methods), and both call
// event.preventDefault() for every key, so the application's shortcut
// handler skipped the key. The component (and its registry entries and
// browser methods) arrives with the first scene that uses it.

const KEY_PROTO = {
  keydown(ev) { ev.preventDefault(); this.log.push(['down', this.name, ev.code]); },
  keyup(ev) { ev.preventDefault(); this.log.push(['up', this.name, ev.code]); },
};
function keyNode(name, log) { return Object.assign(Object.create(KEY_PROTO), { name, log }); }
function keyEvent(code) {
  return { code, key: code, prevented: false, preventDefault() { this.prevented = true; } };
}
// Load the KeyDeviceSensor component into a sensorBrowser (as X_ITE does).
function loadKeyComponent(b, ...names) {
  const nodes = new Set(names.map((n) => keyNode(n, b.log)));
  b.proto.getConcreteNodes = () => new Map([['Transform', {}], ['KeySensor', {}], ['StringSensor', {}]]);
  b.proto.getKeyDeviceSensorNodes = () => nodes;
  // X_ITE's own dispatch: every enabled sensor gets the event.
  b.press = (code) => { const ev = keyEvent(code); for (const n of nodes) n.keydown(ev); return ev; };
  b.release = (code) => { const ev = keyEvent(code); for (const n of nodes) n.keyup(ev); return ev; };
  return nodes;
}

test('keys: a closed gate keeps KeySensor and StringSensor quiet and the key unprevented', () => {
  const b = sensorBrowser();
  const a = createXitePickAdapter({ X3D: b.X3D, browser: b.browser });
  assert.equal(a.sceneInput(false), 'edit', 'no keyboard component: nothing to gate');
  loadKeyComponent(b, 'KS', 'SS');
  assert.equal(a.sceneInput(), 'edit', 'the newly loaded sensors are gated by the next check');
  for (const code of ['KeyZ', 'Delete', 'Escape', 'KeyF']) {
    const ev = b.press(code);
    b.release(code);
    assert.equal(ev.prevented, false, `${code} goes on to the application`);
  }
  assert.deepEqual(b.log, [], 'no sensor saw a key');
});

test('keys: Interact delivers keys; leaving it closes them again', () => {
  const b = sensorBrowser();
  const a = createXitePickAdapter({ X3D: b.X3D, browser: b.browser });
  loadKeyComponent(b, 'KS');
  assert.equal(a.sceneInput(true), 'interact');
  assert.equal(b.press('KeyA').prevented, true);
  b.release('KeyA');
  assert.deepEqual(b.log, [['down', 'KS', 'KeyA'], ['up', 'KS', 'KeyA']]);
  assert.equal(a.sceneInput(false), 'edit');
  assert.equal(b.press('KeyB').prevented, false, 'a late key after Interact ends is not delivered');
  assert.equal(b.log.length, 2);
});

test('keys: a key held across a mode switch never reports a release it did not press', () => {
  const b = sensorBrowser();
  const a = createXitePickAdapter({ X3D: b.X3D, browser: b.browser });
  loadKeyComponent(b, 'KS');
  a.sceneInput(false);
  b.press('ShiftLeft'); // pressed while editing
  a.sceneInput(true);
  b.release('ShiftLeft'); // released in Interact
  assert.deepEqual(b.log, [], 'no keyRelease for a key the world never saw pressed');
  b.press('KeyQ'); // pressed in Interact, then Interact ends while held
  a.sceneInput(false);
  a.sceneInput(true);
  b.release('KeyQ');
  assert.deepEqual(b.log, [['down', 'KS', 'KeyQ']], 'the release after a close/reopen is not delivered');
  b.press('KeyR'); b.press('KeyR'); // key repeat
  b.release('KeyR');
  // An auto-repeat of a key pressed while editing is not a press the world saw.
  a.sceneInput(false);
  b.press('KeyT');
  a.sceneInput(true);
  const rep = keyEvent('KeyT');
  rep.repeat = true;
  for (const n of b.browser.getKeyDeviceSensorNodes()) n.keydown(rep);
  b.release('KeyT');
  assert.equal(b.log.length, 4, 'neither the repeat nor the release of KeyT reached the sensor');
  assert.equal(rep.prevented, false);
  assert.deepEqual(b.log.slice(1), [['down', 'KS', 'KeyR'], ['down', 'KS', 'KeyR'], ['up', 'KS', 'KeyR']]);
});

test('keys: fails closed when the keyboard entry point is missing or a sensor method is replaced', () => {
  const b = sensorBrowser();
  const a = createXitePickAdapter({ X3D: b.X3D, browser: b.browser });
  const nodes = loadKeyComponent(b, 'KS');
  delete b.proto.getKeyDeviceSensorNodes;
  assert.equal(a.sceneInput(true), 'unavailable:key-sensor-entry-missing', 'component loaded, entry missing');
  b.proto.getKeyDeviceSensorNodes = () => nodes;
  assert.equal(a.sceneInput(), 'edit', 'recovers CLOSED');
  assert.equal(a.sceneInput(true), 'interact');
  const [ks] = nodes;
  const foreign = function () { return 'foreign'; };
  ks.keydown = foreign;
  assert.equal(a.sceneInput(), 'unavailable:key-sensor-entry-already-wrapped');
  assert.equal(a.sceneInput(true), 'unavailable:key-sensor-entry-already-wrapped', 'Interact cannot open it');
  assert.equal(ks.keydown, foreign, 'a foreign wrapper is never replaced');
  b.release('KeyA');
  assert.deepEqual(b.log, [], 'the remaining wrapper is closed');
  delete ks.keydown;
  delete ks.keyup;
  assert.equal(a.sceneInput(), 'edit', 'rewrapped, verified, closed');
});

test('keys: a sensor enabled after the check is gated by the next one; a missing registry fails closed', () => {
  const b = sensorBrowser();
  const a = createXitePickAdapter({ X3D: b.X3D, browser: b.browser });
  const nodes = loadKeyComponent(b, 'KS');
  assert.equal(a.sceneInput(false), 'edit');
  const late = keyNode('LATE', b.log);
  nodes.add(late);
  assert.equal(a.sceneInput(false), 'edit');
  assert.equal(own(late, 'keydown'), true, 'wrapped');
  b.press('KeyA');
  assert.deepEqual(b.log, []);
  delete b.proto.getConcreteNodes;
  assert.equal(a.sceneInput(true), 'unavailable:key-sensor-registry-missing');
  b.proto.getConcreteNodes = () => { throw new Error('boom'); };
  assert.equal(a.sceneInput(true), 'unavailable:key-sensor-registry-missing');
});

test('keys: an install error is unavailable; dispose removes only its own key wrappers', () => {
  const b = sensorBrowser();
  const a = createXitePickAdapter({ X3D: b.X3D, browser: b.browser });
  const nodes = loadKeyComponent(b, 'KS', 'SS');
  const [ks, ss] = nodes;
  Object.preventExtensions(ss);
  assert.equal(a.sceneInput(false), 'unavailable:key-sensor-gate-install-error');
  const b2 = sensorBrowser();
  const a2 = createXitePickAdapter({ X3D: b2.X3D, browser: b2.browser });
  const [k2, s2] = loadKeyComponent(b2, 'KS', 'SS');
  a2.sceneInput(false);
  const later = () => 'later';
  s2.keyup = later;
  a2.dispose();
  assert.equal(own(k2, 'keydown'), false);
  assert.equal(own(k2, 'keyup'), false);
  assert.equal(s2.keyup, later, 'a foreign wrapper stays');
  assert.equal(own(ks, 'keydown') && own(ss, 'keydown'), false);
});

// ---- PR #150 review: dispose() cleans DISABLED sensors too -----------------
// X_ITE lists only the ENABLED keyboard sensors (a disabled or dead sensor
// leaves getKeyDeviceSensorNodes()); the adapter keeps its own list of every
// node it wrapped.

const enable = (nodes, n, on) => (on ? nodes.add(n) : nodes.delete(n));
const clean = (n) => !own(n, 'keydown') && !own(n, 'keyup') && n.keydown === KEY_PROTO.keydown && n.keyup === KEY_PROTO.keyup;

test('keys (#150): a sensor disabled before dispose is cleaned; re-enabled, a new adapter gates it (no entry-already-wrapped)', () => {
  const b = sensorBrowser();
  const a = createXitePickAdapter({ X3D: b.X3D, browser: b.browser });
  const nodes = loadKeyComponent(b, 'KS', 'SS');
  const [ks, ss] = nodes;
  assert.equal(a.sceneInput(true), 'interact');
  b.press('KeyA'); // held in Interact
  enable(nodes, ss, false);
  a.dispose();
  assert.equal(clean(ks) && clean(ss), true, 'both sensors are clean, the disabled one included');
  enable(nodes, ss, true);
  const a2 = createXitePickAdapter({ X3D: b.X3D, browser: b.browser });
  assert.equal(a2.sceneInput(false), 'edit', 'no key-sensor-entry-already-wrapped');
  b.log.length = 0;
  b.press('KeyZ');
  b.release('KeyA');
  assert.deepEqual(b.log, [], 'edit: no key, and no release of the key held under the old adapter');
  assert.equal(a2.sceneInput(true), 'interact');
  b.press('KeyB');
  b.release('KeyB');
  assert.deepEqual(b.log, [['down', 'KS', 'KeyB'], ['down', 'SS', 'KeyB'], ['up', 'KS', 'KeyB'], ['up', 'SS', 'KeyB']]);
  assert.equal(a2.sceneInput(false), 'edit');
});

test('keys (#150): after a scene replacement the old (dead) sensors are cleaned by dispose too', () => {
  const b = sensorBrowser();
  const a = createXitePickAdapter({ X3D: b.X3D, browser: b.browser });
  const nodes = loadKeyComponent(b, 'KS', 'SS');
  const old = [...nodes];
  assert.equal(a.sceneInput(false), 'edit');
  // replaceWorld: the old sensors leave the list, the new scene's arrive.
  nodes.clear();
  const ks2 = keyNode('KS2', b.log);
  nodes.add(ks2);
  assert.equal(a.sceneInput(false), 'edit');
  enable(nodes, ks2, false);
  a.dispose();
  assert.equal(old.every(clean) && clean(ks2), true);
  enable(nodes, ks2, true);
  const a2 = createXitePickAdapter({ X3D: b.X3D, browser: b.browser });
  assert.equal(a2.sceneInput(true), 'interact');
  b.press('KeyC');
  assert.deepEqual(b.log, [['down', 'KS2', 'KeyC']]);
});

test('keys (#150): dispose works with the sensor registry and list unavailable', () => {
  const b = sensorBrowser();
  const a = createXitePickAdapter({ X3D: b.X3D, browser: b.browser });
  const nodes = loadKeyComponent(b, 'KS', 'SS');
  assert.equal(a.sceneInput(false), 'edit');
  delete b.proto.getConcreteNodes;
  b.proto.getKeyDeviceSensorNodes = () => { throw new Error('gone'); };
  a.dispose();
  assert.equal([...nodes].every(clean), true);
  assert.equal(a.sceneInput(true), 'unavailable:adapter-disposed');
});

test('keys (#150): a partial install is rolled back at once and dispose leaves nothing', () => {
  const b = sensorBrowser();
  const a = createXitePickAdapter({ X3D: b.X3D, browser: b.browser });
  const nodes = loadKeyComponent(b, 'KS', 'SS');
  const [ks, ss] = nodes;
  // SS takes keydown, then refuses keyup (an inherited accessor): half wrapped.
  const refusing = Object.create(KEY_PROTO);
  Object.defineProperty(refusing, 'keyup', { get() { return KEY_PROTO.keyup; }, set() { throw new Error('refused'); } });
  Object.setPrototypeOf(ss, refusing);
  assert.equal(a.sceneInput(true), 'unavailable:key-sensor-gate-install-error');
  assert.equal(own(ss, 'keydown'), false, 'the keydown half is rolled back');
  assert.equal(own(ks, 'keydown'), true, 'KS (wrapped first) stays gated and closed');
  b.press('KeyA');
  assert.deepEqual(b.log.filter((e) => e[1] === 'KS'), [], 'closed: no key');
  a.dispose();
  assert.equal(clean(ks), true);
  assert.equal(own(ss, 'keydown'), false);
  Object.setPrototypeOf(ss, KEY_PROTO);
  const a2 = createXitePickAdapter({ X3D: b.X3D, browser: b.browser });
  assert.equal(a2.sceneInput(false), 'edit', 'a new adapter installs fully');
});

test('keys (#150): a wrapper replaced by other code is never removed; ours behind it passes keys after dispose', () => {
  const b = sensorBrowser();
  const a = createXitePickAdapter({ X3D: b.X3D, browser: b.browser });
  const nodes = loadKeyComponent(b, 'KS', 'SS');
  const [ks, ss] = nodes;
  assert.equal(a.sceneInput(false), 'edit');
  const ours = ks.keydown;
  const foreign = function (ev) { b.log.push(['foreign', ev.code]); return ours.apply(this, arguments); };
  ks.keydown = foreign;
  enable(nodes, ks, false); // and disabled
  a.dispose();
  assert.equal(ks.keydown, foreign, 'the other component keeps its wrapper');
  assert.equal(own(ks, 'keyup'), false, 'our keyup is removed');
  assert.equal(clean(ss), true);
  b.log.length = 0;
  ks.keydown(keyEvent('KeyQ'));
  assert.deepEqual(b.log, [['foreign', 'KeyQ'], ['down', 'KS', 'KeyQ']], 'the retired wrapper passes the key on');
  enable(nodes, ks, true);
  const a2 = createXitePickAdapter({ X3D: b.X3D, browser: b.browser });
  assert.equal(a2.sceneInput(true), 'unavailable:key-sensor-entry-already-wrapped', 'a foreign wrapper fails closed');
  assert.equal(ks.keydown, foreign);
});
