'use strict';
// PR #150 review (finding 2): the Tauri preview host (ui/static/
// preview-adapter.js) tells Rust ONCE whenever a verified scene input gate
// becomes unavailable -- found at a keydown, keyup, keypress, beforeinput,
// input or composition event, a scene reload or a new adapter -- so Rust
// ends Interact. Not again while the same failure lasts; the gate comes back
// in edit, never Interact; the notice is sent after the current script
// (microtask), and Rust's reply (a read of the gate) causes no new notice.
// Driven in a VM with a small fake DOM and a fake pick adapter whose gate
// state the test sets.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const HOST = path.join(__dirname, '..', '..', 'apps', 'desktop-tauri', 'ui', 'static', 'preview-adapter.js');
const SOURCE = fs.readFileSync(HOST, 'utf8');
const EVENT = 'wrlforge-scene-input-blocked';

function host() {
  const listeners = {};
  const sent = [];
  const queue = [];
  const gate = { broken: null, adapters: [], disposed: 0 };
  const doc = { activeElement: null };
  const stage = {
    hasAttribute() { return true; },
    setAttribute() {},
    focus() { doc.activeElement = stage; },
    dispatchEvent() { return true; },
  };
  const browser = () => ({
    baseURL: '',
    replaceWorld: async () => {},
    createX3DFromString: async () => ({ rootNodes: [] }),
  });
  const canvas = { browser: browser(), parentElement: stage };
  canvas.contains = (t) => t === canvas;
  doc.getElementById = (id) => (id === 'viewport' ? canvas : null);
  const window = {
    WeakRef,
    X3D: {},
    addEventListener(t, f) { (listeners[t] = listeners[t] || []).push(f); },
    dispatchEvent(e) { sent.push(e); for (const f of listeners[e.type] || []) f(e); return true; },
    WrlXitePickAdapter: {
      createXitePickAdapter() {
        const a = {
          sceneInput(open) { return gate.broken ? `unavailable:${gate.broken}` : open === true ? 'interact' : 'edit'; },
          dispose() { gate.disposed++; },
          retire() {}, abort() {}, activate() { return false; },
        };
        gate.adapters.push(a);
        return a;
      },
    },
  };
  class CustomEvent { constructor(type, init) { this.type = type; this.detail = init && init.detail; } }
  class KeyboardEvent { constructor(type, init) { Object.assign(this, init, { type }); } }
  const sandbox = {
    window, document: doc, CustomEvent, KeyboardEvent, performance,
    queueMicrotask: (f) => queue.push(f),
  };
  vm.runInNewContext(SOURCE, sandbox, { filename: HOST });
  const api = window.wrlforgePreview;
  const flush = () => { while (queue.length) queue.shift()(); };
  // One event at the canvas, through the window capture listeners.
  const fire = (type, extra) => {
    const ev = Object.assign({
      type, target: canvas, cancelable: true, repeat: false, stopped: false, prevented: false,
      composedPath() { return [canvas]; },
      stopImmediatePropagation() { this.stopped = true; },
      preventDefault() { this.prevented = true; },
    }, extra || {});
    for (const f of listeners[type] || []) { if (ev.stopped) break; f(ev); }
    return ev;
  };
  const lost = () => sent.filter((e) => e.type === EVENT).map((e) => e.detail);
  return { api, gate, canvas, browser, fire, flush, lost, queue, listeners };
}

const TYPES = ['keydown', 'keyup', 'keypress', 'beforeinput', 'input', 'compositionstart', 'compositionupdate', 'compositionend'];

for (const type of TYPES) {
  test(`gate lost at ${type} in Interact: blocked, Rust told once, never again while it lasts, back in edit only`, () => {
    const h = host();
    assert.equal(h.api.sceneInput(true), 'interact');
    h.api.sceneInputFaultOn(type, `fault-at-${type}`);
    const ev = h.fire(type);
    assert.equal(ev.stopped, true, `${type} does not reach the canvas`);
    assert.deepEqual(h.lost(), [], 'not sent inside the event (no re-entry into Rust)');
    h.flush();
    assert.deepEqual(h.lost(), [`unavailable:fault-at-${type}`]);
    // The same failure lasts: every event type, a repeat, a Rust read.
    for (const t of TYPES) h.fire(t);
    h.fire('keydown', { repeat: true });
    h.api.sceneInput();
    h.flush();
    assert.equal(h.lost().length, 1, 'one notice per failure');
    assert.equal(h.api.sceneGateLost(), 1);
    // Recovery: closed (edit), never Interact by itself; no notice.
    assert.equal(h.api.sceneInputFault(''), 'edit');
    assert.equal(h.api.sceneInput(), 'edit');
    h.fire('keyup');
    h.flush();
    assert.equal(h.lost().length, 1);
  });
}

test('a real gate failure (foreign wrapper) found at keyup is reported once', () => {
  const h = host();
  assert.equal(h.api.sceneInput(true), 'interact');
  h.gate.broken = 'key-sensor-entry-already-wrapped';
  h.fire('keyup');
  h.fire('keyup');
  h.flush();
  assert.deepEqual(h.lost(), ['unavailable:key-sensor-entry-already-wrapped']);
  h.gate.broken = null;
  assert.equal(h.api.sceneInput(), 'edit', 'Interact was dropped: it is never re-entered');
  // A NEW failure after a passing check is a new change: one more notice.
  h.gate.broken = 'key-sensor-gate-not-verified';
  h.fire('compositionstart');
  h.flush();
  assert.deepEqual(h.lost(), ['unavailable:key-sensor-entry-already-wrapped', 'unavailable:key-sensor-gate-not-verified']);
});

test('the gate lost at a scene reload is reported once', async () => {
  const h = host();
  assert.equal(h.api.sceneInput(true), 'interact');
  h.gate.broken = 'key-sensor-gate-not-verified';
  await h.api.load('#VRML V2.0 utf8\n', null, '');
  h.flush();
  await h.api.load('#VRML V2.0 utf8\n', null, '');
  h.flush();
  assert.deepEqual(h.lost(), ['unavailable:key-sensor-gate-not-verified']);
});

test('the gate lost with a new adapter (replaced X_ITE browser) is reported once', () => {
  const h = host();
  assert.equal(h.api.sceneInput(true), 'interact');
  h.canvas.browser = h.browser();
  h.gate.broken = 'pointer-sensor-entry-already-wrapped';
  assert.match(h.api.sceneInput(), /^unavailable:/);
  h.flush();
  assert.equal(h.gate.disposed, 1, 'the old adapter is disposed');
  assert.equal(h.gate.adapters.length, 2);
  assert.deepEqual(h.lost(), ['unavailable:pointer-sensor-entry-already-wrapped']);
});

test('no loop: a reply that reads (or sets edit on) the gate sends nothing more', () => {
  const h = host();
  // Rust's reply: read the gate, then leave Interact (a closed request).
  h.listeners[EVENT] = [() => { h.api.sceneInput(); h.api.sceneInput(false); }];
  assert.equal(h.api.sceneInput(true), 'interact');
  h.api.sceneInputFault('smoke');
  h.flush();
  assert.equal(h.queue.length, 0);
  assert.equal(h.lost().length, 1);
});

test('no notice while the gate was never verified (no adapter yet), and none in a plain edit/Interact switch', () => {
  const h = host();
  h.canvas.browser = null;
  assert.match(h.api.sceneInput(), /^unavailable:/);
  h.canvas.browser = h.browser();
  assert.equal(h.api.sceneInput(true), 'interact');
  assert.equal(h.api.sceneInput(false), 'edit');
  h.flush();
  assert.deepEqual(h.lost(), []);
});
