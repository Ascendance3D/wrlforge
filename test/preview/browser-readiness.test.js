'use strict';
// Regression coverage for the bounded X_ITE readiness gate
// (src/preview/browser-readiness.js).
//
// The defect being locked down: on a Windows FIRST launch from a fresh
// application directory, X3D() resolves while the WebGL2 context X_ITE captured
// at construction is already lost (isContextLost() === true,
// getParameter(MAX_TEXTURE_SIZE) === null, getError() === CONTEXT_LOST_WEBGL).
// Parsing against that context throws from inside X_ITE's VRML parser, which
// rewrites the failure as "Unexpected end of file" and blames a valid document.
//
// Every case here runs on INJECTED time -- no real sleeps, no Electron, no WebGL.

const test = require('node:test');
const assert = require('node:assert/strict');
const R = require('../../src/preview/browser-readiness');

// ---- fake clock ------------------------------------------------------------
// now() only advances when sleep() is awaited, so a "10 second" timeout costs
// nothing and the ordering of checks is exactly reproducible.
function fakeClock() {
  let t = 0;
  return {
    now: () => t,
    sleep: async (ms) => { t += ms; },
    advance: (ms) => { t += ms; },
    get time() { return t; },
  };
}

// A stub X_ITE browser whose WebGL state is scripted per call.
function stubBrowser(states) {
  const queue = Array.isArray(states) ? states.slice() : [states];
  let current = queue.shift();
  const b = {
    reads: 0,
    setState(s) { current = s; },
    getContext() {
      if (current === 'no-context') return null;
      return {
        MAX_TEXTURE_SIZE: 0x0d33,
        isContextLost: () => current.lost === true,
        getParameter: () => current.maxTextureSize,
        getError: () => (current.lost === true ? R.CONTEXT_LOST_WEBGL : 0),
      };
    },
  };
  Object.defineProperty(b, '_state', { get: () => current });
  return b;
}

const LIVE = { lost: false, maxTextureSize: 16384 };
const DEAD = { lost: true, maxTextureSize: null };

// ---- predicate -------------------------------------------------------------

test('predicate: a live context with a positive MAX_TEXTURE_SIZE is usable', () => {
  assert.equal(R.isBrowserUsable(stubBrowser(LIVE)), true);
});

test('predicate: the proved failure state (lost + null max texture size) is NOT usable', () => {
  const b = stubBrowser(DEAD);
  const state = R.readGlState(b);
  assert.equal(state.lost, true);
  assert.equal(state.maxTextureSize, null);
  assert.equal(state.contextLostError, true);
  assert.equal(R.isGlStateUsable(state), false);
  assert.equal(R.isBrowserUsable(b), false);
});

test('predicate: MAX_TEXTURE_SIZE is queried with the real WebGL enum', () => {
  // getParameter here answers ONLY for the correct pname, so a gate that passed
  // the wrong constant would read null and call a healthy context unusable.
  const withEnum = { getContext: () => ({
    MAX_TEXTURE_SIZE: 0x0d33,
    isContextLost: () => false,
    getParameter: (pname) => (pname === 0x0d33 ? 4096 : null),
    getError: () => 0,
  }) };
  assert.equal(R.readGlState(withEnum).maxTextureSize, 4096);
  assert.equal(R.isBrowserUsable(withEnum), true);

  // A context object that does not expose the enum as a property still works:
  // the module falls back to the spec constant.
  const withoutEnum = { getContext: () => ({
    isContextLost: () => false,
    getParameter: (pname) => (pname === 0x0d33 ? 2048 : null),
    getError: () => 0,
  }) };
  assert.equal(R.readGlState(withoutEnum).maxTextureSize, 2048);
  assert.equal(R.isBrowserUsable(withoutEnum), true);
});

test('predicate: a browser with no context at all is not usable', () => {
  assert.equal(R.isBrowserUsable(stubBrowser('no-context')), false);
  assert.equal(R.isBrowserUsable(null), false);
  assert.equal(R.isBrowserUsable({}), false);
});

test('predicate: exceptions during state interrogation do not crash the gate', () => {
  // 1. getContext() throws
  const throwGetContext = { getContext: () => { throw new Error('getContext boom'); } };
  const s1 = R.readGlState(throwGetContext);
  assert.equal(s1.hasBrowser, true);
  assert.equal(s1.hasContext, false);
  assert.match(s1.error, /getContext boom/);
  assert.equal(R.isGlStateUsable(s1), false);

  // 2. gl methods throw
  const throwGl = { getContext: () => ({
    isContextLost: () => { throw new Error('lost boom'); },
    getParameter: () => { throw new Error('param boom'); },
    getError: () => { throw new Error('error boom'); },
  }) };
  const s2 = R.readGlState(throwGl);
  assert.equal(s2.hasContext, true);
  assert.equal(s2.lost, null, 'lost defaults to null on exception');
  assert.match(s2.error, /param boom/, 'getParameter exception is captured');
  assert.equal(s2.contextLostError, false, 'getError exception is swallowed and does not set contextLostError');
  assert.equal(R.isGlStateUsable(s2), false);
});

// ---- bounded wait ----------------------------------------------------------

test('already-ready browser resolves immediately, without sleeping or recreating', async () => {
  const clock = fakeClock();
  const browser = stubBrowser(LIVE);
  let recreated = 0;
  const res = await R.acquireReadyBrowser({
    awaitX3D: async () => {},
    getBrowser: () => browser,
    recreateBrowser: () => { recreated += 1; return browser; },
    sleep: clock.sleep,
    now: clock.now,
  });
  assert.equal(res.browser, browser);
  assert.equal(res.recreates, 0);
  assert.equal(recreated, 0);
  assert.equal(clock.time, 0, 'no time should pass when the browser is already ready');
});

test('browser that becomes ready after several checks resolves as soon as it does', async () => {
  const clock = fakeClock();
  const browser = stubBrowser({ lost: false, maxTextureSize: null }); // present, not yet reporting
  let checks = 0;
  const res = await R.acquireReadyBrowser({
    awaitX3D: async () => {},
    getBrowser: () => { checks += 1; if (checks === 4) browser.setState(LIVE); return browser; },
    recreateBrowser: () => { throw new Error('must not recreate a context that is not lost'); },
    sleep: clock.sleep,
    now: clock.now,
    pollMs: 10,
  });
  assert.equal(res.browser, browser);
  assert.equal(res.recreates, 0);
  assert.equal(checks, 4, 'resolves on the first check where the predicate holds');
  assert.ok(clock.time > 0 && clock.time < 100, `bounded short wait, got ${clock.time}ms`);
});

test('a LOST context is recovered by replacing the canvas, not by waiting it out', async () => {
  const clock = fakeClock();
  const dead = stubBrowser(DEAD);
  const fresh = stubBrowser(LIVE);
  let current = dead;
  let recreated = 0;
  const res = await R.acquireReadyBrowser({
    awaitX3D: async () => {},
    getBrowser: () => current,
    recreateBrowser: () => { recreated += 1; current = fresh; return fresh; },
    sleep: clock.sleep,
    now: clock.now,
    pollMs: 10,
    recreateDelayMs: 20,
    timeoutMs: 5000,
  });
  assert.equal(recreated, 1, 'exactly one replacement was needed');
  assert.equal(res.browser, fresh);
  assert.equal(res.recreates, 1);
  assert.ok(clock.time < 5000, 'returned well before the deadline');
});

test('timeout rejects with a PreviewInitError, bounded and with no infinite loop', async () => {
  const clock = fakeClock();
  const dead = stubBrowser(DEAD);
  let recreated = 0;
  await assert.rejects(
    () => R.acquireReadyBrowser({
      awaitX3D: async () => {},
      getBrowser: () => dead,
      recreateBrowser: () => { recreated += 1; return dead; },
      sleep: clock.sleep,
      now: clock.now,
      pollMs: 25,
      recreateDelayMs: 50,
      maxRecreates: 3,
      timeoutMs: 1000,
    }),
    (err) => {
      assert.ok(err instanceof R.PreviewInitError);
      assert.equal(err.code, 'preview-init');
      assert.match(err.message, /graphics context is not available/i);
      assert.equal(err.detail.recreates, 3, 'recreate attempts are bounded');
      assert.ok(err.detail.waitedMs <= 1000 + 25, 'the wait honoured its deadline');
      return true;
    },
  );
  assert.equal(recreated, 3, 'never retries past maxRecreates');
  assert.ok(clock.time <= 1000 + 25, `bounded total wait, got ${clock.time}ms`);
});

test('X3D() rejection is reported as an initialization failure, not a parse failure', async () => {
  const clock = fakeClock();
  await assert.rejects(
    () => R.acquireReadyBrowser({
      awaitX3D: async () => { throw new Error("Couldn't create WebGL context."); },
      getBrowser: () => stubBrowser(LIVE),
      recreateBrowser: () => null,
      sleep: clock.sleep,
      now: clock.now,
    }),
    (err) => {
      assert.ok(err instanceof R.PreviewInitError);
      assert.equal(err.detail.phase, 'X3D');
      assert.match(err.message, /engine failed to start/i);
      return true;
    },
  );
});

test('a failed readiness attempt does not poison a later retry', async () => {
  const clock = fakeClock();
  const dead = stubBrowser(DEAD);
  const live = stubBrowser(LIVE);
  let current = dead;
  const deps = {
    awaitX3D: async () => {},
    getBrowser: () => current,
    recreateBrowser: () => dead,     // first attempt: recovery also fails
    sleep: clock.sleep,
    now: clock.now,
    pollMs: 25,
    recreateDelayMs: 50,
    maxRecreates: 1,
    timeoutMs: 500,
  };
  await assert.rejects(() => R.acquireReadyBrowser(deps), R.PreviewInitError);

  // Same module, same deps object shape -- a second call starts from scratch and
  // succeeds once the environment recovers. No cached rejection, no false-ready.
  current = live;
  const res = await R.acquireReadyBrowser({ ...deps, recreateBrowser: () => live });
  assert.equal(res.browser, live);
  assert.equal(res.recreates, 0);
});

// ---- ordering: nothing parses before readiness ------------------------------

test('parsing does not start before readiness, and starts exactly after it succeeds', async () => {
  const clock = fakeClock();
  const dead = stubBrowser(DEAD);
  const fresh = stubBrowser(LIVE);
  let current = dead;
  const trace = [];

  // The caller's contract: acquire, THEN parse. The gate must not resolve while
  // the context is dead, so no parse is attempted against it.
  async function loadLikeRenderer() {
    const res = await R.acquireReadyBrowser({
      awaitX3D: async () => { trace.push('X3D'); },
      getBrowser: () => { trace.push('check:' + (current === dead ? 'dead' : 'live')); return current; },
      recreateBrowser: () => { trace.push('recreate'); current = fresh; return fresh; },
      sleep: clock.sleep,
      now: clock.now,
      pollMs: 10,
      recreateDelayMs: 10,
    });
    trace.push('parse');
    return res.browser;
  }

  const used = await loadLikeRenderer();
  assert.equal(used, fresh);
  assert.equal(trace[0], 'X3D');
  assert.equal(trace[trace.length - 1], 'parse');
  assert.equal(trace.indexOf('parse'), trace.length - 1, 'parse happens exactly once, last');
  const parseAt = trace.indexOf('parse');
  const lastDeadAt = trace.lastIndexOf('check:dead');
  assert.ok(lastDeadAt < parseAt, 'no parse was attempted while the context was dead');
  assert.equal(trace[parseAt - 1], 'check:live', 'parse starts immediately after the predicate holds');
});

// ---- canvas replacement ------------------------------------------------------

test('replaceCanvasElement disposes the dead browser and carries attributes over', () => {
  const disposed = [];
  const made = [];
  const oldEl = {
    browser: { dispose: () => disposed.push('old') },
    attributes: [
      { name: 'id', value: 'preview' },
      { name: 'tabindex', value: '0' },
      { name: 'aria-label', value: 'Embedded X_ITE 3D preview' },
    ],
    replaceWith(next) { this.replacedWith = next; },
  };
  const doc = {
    getElementById: (id) => (id === 'preview' ? oldEl : null),
    createElement: (tag) => { const e = { tag, attrs: {}, setAttribute(n, v) { this.attrs[n] = v; } }; made.push(e); return e; },
  };
  const next = R.replaceCanvasElement(doc, 'preview');
  assert.deepEqual(disposed, ['old']);
  assert.equal(next.tag, 'x3d-canvas');
  assert.deepEqual(next.attrs, { id: 'preview', tabindex: '0', 'aria-label': 'Embedded X_ITE 3D preview' });
  assert.equal(oldEl.replacedWith, next);
});

test('replaceCanvasElement survives a browser too broken to dispose', () => {
  const oldEl = {
    browser: { dispose: () => { throw new Error('context already gone'); } },
    attributes: [{ name: 'id', value: 'wpCanvas' }],
    replaceWith(next) { this.replacedWith = next; },
  };
  const doc = {
    getElementById: () => oldEl,
    createElement: (tag) => ({ tag, attrs: {}, setAttribute(n, v) { this.attrs[n] = v; } }),
  };
  const next = R.replaceCanvasElement(doc, 'wpCanvas');
  assert.equal(next.attrs.id, 'wpCanvas');
  assert.equal(oldEl.replacedWith, next);
});

test('replaceCanvasElement returns null when the canvas is absent', () => {
  assert.equal(R.replaceCanvasElement({ getElementById: () => null }, 'nope'), null);
});
