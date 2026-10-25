'use strict';
// Phase 7C2 -- the Mall live-preview bridge: authorization, path-rejection,
// version/generation ordering, saved-version fallback, deterministic cleanup, and
// the debounce/coalescing model the editor drives. All pure (injected fakes); no
// Electron, no real disk, no temp file is ever written.

const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const {
  createMallPreviewBridge, MALL_PROFILE, HARD_MAX_BYTES,
} = require('../../src/preview/mall-preview-bridge');
const { fileDirUrl } = require('../../src/preview/texture-base');
const { createPreviewScheduler } = require('../../src/preview/preview-scheduler');

const SRC = '/proj/mall/item.wrl';

// Build a bridge over injected fakes. `resolvePath` is identity so tests are
// path-shape independent (cross-platform: no OS path.resolve drive-letter drift).
function makeBridge(over = {}) {
  const session = {
    open: true, sessionId: 1, context: 'mall', sourcePath: SRC, ...(over.session || {}),
  };
  const bridge = createMallPreviewBridge({
    overlay: over.overlay,
    describeSession: over.describeSession || (() => ({ ...session })),
    getAuthorizedMallSource: over.getAuthorizedMallSource || (() => SRC),
    scanRemoteUrls: over.scanRemoteUrls || ((t) => (/https?:\/\//.test(String(t)) ? ['http://blocked/x.png'] : [])),
    readSaved: over.readSaved || (() => ({ text: '#VRML V2.0 utf8\n# on disk\n', wasGzipped: false })),
    baseUrlFor: over.baseUrlFor, // default = real fileDirUrl
    resolvePath: over.resolvePath || ((p) => String(p)),
  });
  return { bridge, session };
}

// ---- authorization: the renderer never chooses a path ----------------------

test('load authorizes an open Mall session and returns the overlay-owned text', () => {
  const { bridge } = makeBridge();
  const res = bridge.load({ sessionId: 1, text: '#VRML V2.0 utf8\nShape {}\n', bufferVersion: 1 });
  assert.equal(res.ok, true);
  assert.equal(res.text, '#VRML V2.0 utf8\nShape {}\n');
  assert.equal(res.sourcePath, SRC);
  assert.equal(res.wasGzipped, false);
  assert.equal(typeof res.generation, 'number');
});

test('base URL is the SOURCE directory so relative textures resolve as on disk', () => {
  const { bridge } = makeBridge();
  const res = bridge.load({ sessionId: 1, text: '#VRML V2.0 utf8\n', bufferVersion: 1 });
  assert.equal(res.baseURL, fileDirUrl(SRC)); // directory of item.wrl, file:// with trailing slash
});

test('the renderer cannot widen the source: held path must equal the authorized Mall item', () => {
  const { bridge } = makeBridge({ getAuthorizedMallSource: () => '/proj/mall/OTHER.wrl' });
  const res = bridge.load({ sessionId: 1, text: '#VRML V2.0 utf8\n', bufferVersion: 1 });
  assert.equal(res.ok, false);
  assert.equal(res.reason, 'source-mismatch');
});

test('a stale sessionId is rejected (renderer missed a reopen)', () => {
  const { bridge } = makeBridge();
  const res = bridge.load({ sessionId: 999, text: '#VRML V2.0 utf8\n', bufferVersion: 1 });
  assert.equal(res.ok, false);
  assert.equal(res.reason, 'stale-session');
});

test('a non-Mall document is rejected (World live preview is a later lane)', () => {
  const { bridge } = makeBridge({ session: { open: true, sessionId: 1, context: 'world', sourcePath: SRC } });
  const res = bridge.load({ sessionId: 1, text: '#VRML V2.0 utf8\n', bufferVersion: 1 });
  assert.equal(res.ok, false);
  assert.equal(res.reason, 'not-mall');
});

test('no open document is rejected', () => {
  const { bridge } = makeBridge({ describeSession: () => ({ open: false }) });
  const res = bridge.load({ sessionId: 1, text: '#VRML V2.0 utf8\n', bufferVersion: 1 });
  assert.equal(res.ok, false);
  assert.equal(res.reason, 'no-document');
});

// ---- overlay registration + version/generation ordering --------------------

test('the buffer is registered in the overlay from editor text', () => {
  const { bridge } = makeBridge();
  bridge.load({ sessionId: 1, text: '#VRML V2.0 utf8\nShape {}\n', bufferVersion: 3 });
  const d = bridge.describe(1);
  assert.equal(d.hasOverlay, true);
  assert.equal(d.bufferVersion, 3);
  assert.equal(d.profile, MALL_PROFILE);
});

test('buffer-version monotonicity: an older version is refused, the newer stays', () => {
  const { bridge } = makeBridge();
  assert.equal(bridge.load({ sessionId: 1, text: 'a', bufferVersion: 2 }).ok, true);
  const older = bridge.load({ sessionId: 1, text: 'b', bufferVersion: 1 });
  assert.equal(older.ok, false);
  assert.equal(older.reason, 'stale-version');
  assert.equal(bridge.describe(1).bufferVersion, 2); // unchanged
});

test('an EQUAL-version re-render (manual Update, no edit) succeeds with a fresh generation', () => {
  const { bridge } = makeBridge();
  const first = bridge.load({ sessionId: 1, text: 'same-text', bufferVersion: 4 });
  assert.equal(first.ok, true);
  // Manual Update with the SAME version + same bytes: idempotent re-render, not stale.
  const again = bridge.load({ sessionId: 1, text: 'same-text', bufferVersion: 4 });
  assert.equal(again.ok, true);
  assert.equal(again.bufferVersion, 4);
  assert.ok(again.generation > first.generation, 'a re-render is a new generation');
});

test('generation monotonicity: each load issues a strictly newer generation', () => {
  const { bridge } = makeBridge();
  const g1 = bridge.load({ sessionId: 1, text: 'a', bufferVersion: 1 }).generation;
  const g2 = bridge.load({ sessionId: 1, text: 'aa', bufferVersion: 2 }).generation;
  assert.ok(g2 > g1);
});

test('a stale generation cannot be accepted after a newer one begins', () => {
  const { bridge } = makeBridge();
  const g1 = bridge.load({ sessionId: 1, text: 'a', bufferVersion: 1 }).generation;
  bridge.load({ sessionId: 1, text: 'aa', bufferVersion: 2 }); // begins g2
  const stale = bridge.accept({ sessionId: 1, generation: g1 });
  assert.equal(stale.ok, false);
  const fresh = bridge.accept({ sessionId: 1, generation: g1 + 1 });
  assert.equal(fresh.ok, true);
});

// ---- overlay error paths ---------------------------------------------------

test('load returns unauthorized when the overlay rejects the authorization proof', () => {
  const err = new Error();
  err.code = 'ENOAUTH';
  const { bridge } = makeBridge({ overlay: { register: () => { throw err; } } });
  const res = bridge.load({ sessionId: 1, text: 'a', bufferVersion: 1 });
  assert.equal(res.ok, false);
  assert.equal(res.reason, 'unauthorized');
});

test('load returns register-failed for unknown overlay registration errors', () => {
  const err = new Error();
  err.code = 'EUNKNOWN';
  const { bridge } = makeBridge({ overlay: { register: () => { throw err; } } });
  const res = bridge.load({ sessionId: 1, text: 'a', bufferVersion: 1 });
  assert.equal(res.ok, false);
  assert.equal(res.reason, 'register-failed');
  assert.equal(res.code, 'EUNKNOWN');
});

test('load returns resolve-failed if the overlay resolution fails', () => {
  const { bridge } = makeBridge({
    overlay: {
      register: () => ({ sizeTier: 'manual' }),
      beginGeneration: () => 1,
      resolve: () => ({ status: 'stale' }),
      invalidateSession: () => {},
    },
  });
  const res = bridge.load({ sessionId: 1, text: 'a', bufferVersion: 1 });
  assert.equal(res.ok, false);
  assert.equal(res.reason, 'resolve-failed');
  assert.equal(res.status, 'stale');
});

// ---- size thresholds -------------------------------------------------------

test('a buffer over the 8 MiB hard maximum is refused and NOT registered (no truncation)', () => {
  const { bridge } = makeBridge();
  const huge = 'x'.repeat(HARD_MAX_BYTES + 10);
  const res = bridge.load({ sessionId: 1, text: huge, bufferVersion: 1 });
  assert.equal(res.ok, false);
  assert.equal(res.reason, 'too-large');
  assert.equal(res.sizeTier, 'refused');
  assert.equal(bridge.describe(1).hasOverlay, false); // stored nothing
});

test('a buffer between 1 and 8 MiB registers but reports the manual size tier', () => {
  const { bridge } = makeBridge();
  const big = 'x'.repeat(2 * 1024 * 1024); // 2 MiB
  const res = bridge.load({ sessionId: 1, text: big, bufferVersion: 1 });
  assert.equal(res.ok, true);
  assert.equal(res.sizeTier, 'manual');
});

// ---- saved-version fallback ------------------------------------------------

test('saved() renders the on-disk source and never touches the overlay', () => {
  let reads = 0;
  const { bridge } = makeBridge({ readSaved: () => { reads += 1; return { text: '#VRML V2.0 utf8\n# disk\n', wasGzipped: true }; } });
  const res = bridge.saved({ sessionId: 1 });
  assert.equal(res.ok, true);
  assert.equal(res.saved, true);
  assert.equal(res.wasGzipped, true);
  assert.match(res.text, /# disk/);
  assert.equal(reads, 1);
  assert.equal(bridge.describe(1).hasOverlay, false); // saved() registered nothing
});

test('remote URLs surface as advisory but the bridge fetches nothing', () => {
  const { bridge } = makeBridge();
  const res = bridge.load({ sessionId: 1, text: '#VRML V2.0 utf8\nImageTexture { url "http://evil/x.png" }\n', bufferVersion: 1 });
  assert.equal(res.ok, true);
  assert.deepEqual(res.remoteUrls, ['http://blocked/x.png']);
});

// ---- deterministic cleanup -------------------------------------------------

test('invalidateSession leaves zero overlays and zero active generations', () => {
  const { bridge } = makeBridge();
  bridge.load({ sessionId: 1, text: 'a', bufferVersion: 1 });
  assert.equal(bridge.leak().size, 1);
  bridge.invalidateSession(1);
  const leak = bridge.leak();
  assert.equal(leak.size, 0);
  assert.equal(leak.activeGenerations, 0);
});

test('a load for a NEW session invalidates the previous one (document switch)', () => {
  const session = { open: true, sessionId: 1, context: 'mall', sourcePath: SRC };
  const { bridge } = makeBridge({ describeSession: () => ({ ...session }) });
  bridge.load({ sessionId: 1, text: 'a', bufferVersion: 1 });
  session.sessionId = 2; // the editor opened a different Mall item
  bridge.load({ sessionId: 2, text: 'b', bufferVersion: 1 });
  assert.equal(bridge.describe(1).hasOverlay, false); // old session dropped
  assert.equal(bridge.describe(2).hasOverlay, true);
  assert.equal(bridge.leak().size, 1); // only the current session holds an overlay
});

test('invalidateDocument keeps the session open but drops the overlay', () => {
  const { bridge } = makeBridge();
  bridge.load({ sessionId: 1, text: 'a', bufferVersion: 1 });
  bridge.invalidateDocument(1);
  const d = bridge.describe(1);
  assert.equal(d.open, true);
  assert.equal(d.hasOverlay, false);
});

// ---- structural guarantees: no fs, no mutation, no temp file ---------------

test('the bridge module imports no filesystem or Electron capability', () => {
  const src = fs.readFileSync(path.join(__dirname, '../../src/preview/mall-preview-bridge.js'), 'utf8');
  assert.equal(/require\(['"]fs['"]\)/.test(src), false);
  assert.equal(/require\(['"]electron['"]\)/.test(src), false);
  assert.equal(/writeFile|writeFileSync|createWriteStream/.test(src), false);
});

test('load does not mutate the caller-provided text', () => {
  const { bridge } = makeBridge();
  const original = '#VRML V2.0 utf8\nShape {}\n';
  const copy = original.slice();
  bridge.load({ sessionId: 1, text: original, bufferVersion: 1 });
  assert.equal(original, copy);
});

// ---- the debounce / coalescing model the editor drives ---------------------

test('auto-refresh fires exactly at the 700 ms debounce, not before', () => {
  const s = createPreviewScheduler({ debounceMs: 700 });
  s.requestAuto('s1', { bufferVersion: 1, byteLength: 100, at: 0 });
  assert.equal(s.poll('s1', 699).fire, false);
  const fired = s.poll('s1', 700);
  assert.equal(fired.fire, true);
  assert.equal(fired.bufferVersion, 1);
});

test('rapid edits coalesce into the newest version and slide the due time', () => {
  const s = createPreviewScheduler({ debounceMs: 700 });
  s.requestAuto('s1', { bufferVersion: 1, byteLength: 10, at: 0 });
  s.requestAuto('s1', { bufferVersion: 2, byteLength: 10, at: 100 });
  s.requestAuto('s1', { bufferVersion: 5, byteLength: 10, at: 200 });
  assert.equal(s.poll('s1', 800).fire, false);       // last edit was at 200 -> due 900
  const fired = s.poll('s1', 900);
  assert.equal(fired.fire, true);
  assert.equal(fired.bufferVersion, 5);              // newest only
  assert.equal(s.pendingCount(), 0);                 // consumed
});

test('manual Update bypasses the debounce and supersedes any pending auto', () => {
  const s = createPreviewScheduler({ debounceMs: 700 });
  s.requestAuto('s1', { bufferVersion: 1, byteLength: 10, at: 0 });
  const m = s.requestManual('s1', { bufferVersion: 2, at: 50 });
  assert.equal(m.immediate, true);
  const fired = s.poll('s1', 50);
  assert.equal(fired.fire, true);
  assert.equal(fired.kind, 'manual');
  assert.equal(fired.bufferVersion, 2);
});

test('a buffer over 1 MiB declines auto-refresh but manual Update still schedules', () => {
  const s = createPreviewScheduler({ debounceMs: 700, autoMaxBytes: 1024 * 1024 });
  const auto = s.requestAuto('s1', { bufferVersion: 1, byteLength: 1024 * 1024 + 1, at: 0 });
  assert.equal(auto.scheduled, false);
  assert.equal(auto.reason, 'manual-only');
  const manual = s.requestManual('s1', { bufferVersion: 1, at: 0 });
  assert.equal(manual.scheduled, true);
});

test('cancel clears a pending auto (document switch / session close)', () => {
  const s = createPreviewScheduler({ debounceMs: 700 });
  s.requestAuto('s1', { bufferVersion: 1, byteLength: 10, at: 0 });
  s.cancel('s1');
  assert.equal(s.poll('s1', 1000).fire, false);
});
