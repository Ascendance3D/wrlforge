'use strict';
// Lane B B2 -- Mall Repack path authority (WRLFORGE_LANE_B_B2_PATH_AUTHORITY_CORRECTION).
//
// The corrected defect: the `mall:repack` IPC handler resolved its paths as
//
//     const editPath   = session ? session.editFile : editFile;   // renderer value
//     const targetPath = session ? session.mallPath : mallPath;   // renderer value
//
// so with no open session the RENDERER named both the file that was read and
// the real artifact that was backed up and overwritten. That is a path-authority
// violation: main must own every Mall write path at all times.
//
// These tests are behavioural, not regex-only. `handler()` below is the handler
// body verbatim -- the same `activeRepackPaths` helper, the same read, the same
// `repackMall` call -- with `fs` injected so every read and write attempt can be
// recorded. A source assertion at the end pins main.js to that same shape so the
// replica cannot drift away from the thing it stands for.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const zlib = require('node:zlib');

const { repackMall } = require('../../src/mall/repack');
const { activeRepackPaths } = require('../../src/mall/repack-paths');

const SAFE_TEXT = '#VRML V2.0 utf8\nWorldInfo { title "safe" }\nShape {}\n';
const EVIL_TEXT = '#VRML V2.0 utf8\nWorldInfo { title "evil" }\nShape {}\n';

function tmpDir(tag) {
  return fs.mkdtempSync(path.join(os.tmpdir(), `wrlforge-b2-auth-${tag}-`));
}

// Records every path fs is asked to touch, so "was EVIL_EDIT read?" and "was
// EVIL_TARGET written?" are answered by observation rather than by inference
// from the result payload.
function recordingFs(log) {
  const wrap = (name, real, kind) => (...args) => {
    log.push({ kind, path: String(args[0]) });
    return real(...args);
  };
  const mockFs = Object.assign(Object.create(fs), {
    readFileSync: wrap('readFileSync', fs.readFileSync.bind(fs), 'read'),
    writeFileSync: wrap('writeFileSync', fs.writeFileSync.bind(fs), 'write'),
    openSync: wrap('openSync', fs.openSync.bind(fs), 'open'),
    renameSync: wrap('renameSync', fs.renameSync.bind(fs), 'rename'),
    copyFileSync: wrap('copyFileSync', fs.copyFileSync.bind(fs), 'copy'),
  });
  const mockPromises = Object.create(fs.promises);
  mockPromises.readFile = wrap('readFile', fs.promises.readFile.bind(fs.promises), 'read');
  Object.defineProperty(mockFs, 'promises', { value: mockPromises, writable: true, configurable: true });
  return mockFs;
}

// The `mall:repack` handler body. Only `asGzip` comes from the renderer
// request; `mallPath`/`editFile` are accepted on the wire (preload compat) and
// ignored. Session paths are the only paths.
async function handler(session, request, fsImpl) {
  const { mallPath, editFile } = activeRepackPaths(session);
  const text = await fsImpl.promises.readFile(editFile, 'utf8');
  return repackMall({ mallPath, text, asGzip: request.asGzip }, { fs: fsImpl });
}

// --- 1. live session: renderer paths cannot redirect the write ---------------

test('B2 path authority: renderer-supplied paths cannot redirect a live Repack', async () => {
  const dir = tmpDir('spoof');
  const SAFE_TARGET = path.join(dir, 'safe.wrl');
  const SAFE_EDIT = path.join(dir, 'safe.edit.wrl');
  const EVIL_TARGET = path.join(dir, 'evil.wrl');
  const EVIL_EDIT = path.join(dir, 'evil.edit.wrl');

  fs.writeFileSync(SAFE_TARGET, zlib.gzipSync(Buffer.from('#VRML V2.0 utf8\nWorldInfo { title "old" }\n', 'utf8')));
  fs.writeFileSync(SAFE_EDIT, SAFE_TEXT, 'utf8');
  fs.writeFileSync(EVIL_TARGET, 'DO NOT TOUCH\n', 'utf8');
  fs.writeFileSync(EVIL_EDIT, EVIL_TEXT, 'utf8');
  const evilTargetBefore = fs.readFileSync(EVIL_TARGET);

  const log = [];
  const session = { mallPath: SAFE_TARGET, editFile: SAFE_EDIT };
  const res = await handler(session, { mallPath: EVIL_TARGET, editFile: EVIL_EDIT, asGzip: true }, recordingFs(log));

  assert.equal(res.saved, true, 'the legitimate session save still succeeds');

  // The session's edit file is what was read.
  const reads = log.filter((e) => e.kind === 'read').map((e) => e.path);
  assert.ok(reads.includes(SAFE_EDIT), 'the session edit file was read');
  assert.ok(!reads.includes(EVIL_EDIT), 'the renderer-supplied edit file was NOT read');

  // The session's mall file is the only write target. Temp/backup siblings of
  // SAFE_TARGET are part of the safe-write discipline; nothing may name an
  // evil path.
  const touched = log.filter((e) => e.kind !== 'read').map((e) => e.path);
  assert.ok(touched.length > 0, 'the recording fs observed the real write path');
  for (const p of touched) {
    assert.ok(!p.startsWith(EVIL_TARGET) && !p.startsWith(EVIL_EDIT),
      `no write may name a renderer-supplied path, got ${p}`);
    assert.ok(path.basename(p).startsWith('safe.') || path.basename(p).includes('wrlforge-tmp'),
      `write target must belong to the session artifact, got ${p}`);
  }

  // On disk: SAFE_TARGET now holds the session text; EVIL_TARGET is byte-identical.
  assert.equal(zlib.gunzipSync(fs.readFileSync(SAFE_TARGET)).toString('utf8'), SAFE_TEXT);
  assert.deepEqual(fs.readFileSync(EVIL_TARGET), evilTargetBefore, 'the spoofed target was not written');
  assert.equal(fs.readFileSync(EVIL_EDIT, 'utf8'), EVIL_TEXT, 'the spoofed edit file was not modified');
});

// --- 2. no session: the operation is rejected outright -----------------------

test('B2 path authority: Repack with no open session is rejected and touches nothing', async () => {
  const dir = tmpDir('nosession');
  const EVIL_TARGET = path.join(dir, 'evil.wrl');
  const EVIL_EDIT = path.join(dir, 'evil.edit.wrl');
  fs.writeFileSync(EVIL_TARGET, 'DO NOT TOUCH\n', 'utf8');
  fs.writeFileSync(EVIL_EDIT, EVIL_TEXT, 'utf8');
  const before = fs.readdirSync(dir).sort();
  const evilTargetBefore = fs.readFileSync(EVIL_TARGET);

  const log = [];
  await assert.rejects(
    () => handler(null, { mallPath: EVIL_TARGET, editFile: EVIL_EDIT, asGzip: true }, recordingFs(log)),
    (err) => err instanceof Error && err.message === 'No file is open.' && err.code === 'ENOSESSION',
  );

  assert.deepEqual(log, [], 'no filesystem operation was attempted at all');
  assert.deepEqual(fs.readFileSync(EVIL_TARGET), evilTargetBefore, 'nothing was written');
  assert.deepEqual(fs.readdirSync(dir).sort(), before, 'no backup and no temp file was created');
  assert.equal(fs.readdirSync(dir).filter((f) => f.includes('.bak-')).length, 0);
  assert.equal(fs.readdirSync(dir).filter((f) => f.includes('wrlforge-tmp')).length, 0);
});

// A partial session is no session: a half-populated object must not become a
// path source either.
test('B2 path authority: an incomplete session is rejected', () => {
  for (const bad of [undefined, null, {}, { mallPath: '/x/a.wrl' }, { editFile: '/x/a.edit.wrl' }]) {
    assert.throws(() => activeRepackPaths(bad), /No file is open\./);
  }
  const ok = activeRepackPaths({ mallPath: '/x/a.wrl', editFile: '/x/a.edit.wrl' });
  assert.deepEqual(ok, { mallPath: '/x/a.wrl', editFile: '/x/a.edit.wrl' });
});

// --- 3. the shipped handler has the shape these tests stand for --------------

test('B2 path authority: main.js mall:repack has no renderer-path fallback', () => {
  const src = fs.readFileSync(path.join(__dirname, '..', '..', 'main.js'), 'utf8');
  // Comment-blind: the header above the handler quotes the old fallback in
  // order to record that it is gone.
  const code = src.split('\n').filter((l) => !/^\s*\/\//.test(l)).join('\n');
  const m = code.match(/ipcMain\.handle\('mall:repack'[\s\S]*?\n\}\);/);
  assert.ok(m, 'the mall:repack handler is present');
  const body = m[0];
  assert.ok(body.includes('await fs.promises.readFile'), 'reads via async promises API');

  assert.ok(body.includes('activeRepackPaths(currentSession)'),
    'paths come from the main-owned session helper');
  assert.ok(!/session\s*\?/.test(body) && !/currentSession\s*\?/.test(body),
    'no ternary fallback to a renderer path');
  assert.ok(!/\beditFile\s*;/.test(body) && !/:\s*editFile\b/.test(body),
    'the renderer editFile is never used as a read path');
  assert.ok(!/:\s*mallPath\b/.test(body),
    'the renderer mallPath is never used as a write target');
  // Only asGzip is destructured out of the renderer request.
  const destructure = body.match(/async \(_evt, \{([^}]*)\}\)/);
  assert.ok(destructure, 'the handler destructures its request');
  assert.deepEqual(
    destructure[1].split(',').map((s) => s.trim()).filter(Boolean),
    ['asGzip'],
    'only asGzip may come from the renderer',
  );
});
