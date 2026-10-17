'use strict';
// Lane B B2 -- Mall Repack write safety.
//
// Every test here is about the file on disk, not about a message. The four
// defects B2 closes were all "the real artifact was already destroyed by the
// time WRLForge said anything", so each case asserts the bytes and the SHA of
// the destination, not merely the returned payload.
//
// No Electron. `repackMall` composes safeSave + measureArtifact + validate, so
// the entire write discipline is reachable from node:test.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const zlib = require('node:zlib');
const crypto = require('node:crypto');

const { repackMall, SAVE_ERROR_MESSAGE } = require('../../src/mall/repack');
const io = require('../../src/editor/file-io');
const { FORMAT } = require('../../src/editor/wrl-document');
const { MALL_UPLOAD_MAX_BYTES } = require('../../validator');

const WRL = '#VRML V2.0 utf8\nWorldInfo { title "t" }\nShape {}\n';

const sha = (buf) => crypto.createHash('sha256').update(buf).digest('hex');

function tmpDir(tag) {
  return fs.mkdtempSync(path.join(os.tmpdir(), `wrlforge-b2-${tag}-`));
}

// Gzip bytes carrying foreign-encoder header metadata (OS byte forced to 0x03).
// Preservation may only be decided by decompressed-text identity, never by the
// header or by a length that happens to match.
function foreignGzip(text) {
  const buf = zlib.gzipSync(Buffer.from(text, 'utf8'), { level: 9 });
  buf[9] = 0x03;
  return buf;
}

const tempsIn = (dir) => fs.readdirSync(dir).filter((f) => f.includes('wrlforge-tmp'));
const backupsIn = (dir) => fs.readdirSync(dir).filter((f) => f.includes('.bak-'));

// --- 1. unchanged gzip preservation -----------------------------------------

test('B2: an unchanged gzip Mall artifact is preserved byte-for-byte, with no backup and no temp', () => {
  const dir = tmpDir('preserve');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, foreignGzip(WRL));

  const before = fs.readFileSync(p);
  const beforeSha = sha(before);
  const beforeMtime = fs.statSync(p).mtimeMs;

  const res = repackMall({ mallPath: p, text: WRL, asGzip: true });

  assert.equal(res.saved, true);
  assert.equal(res.preserved, true, 'the artifact was preserved, not rewritten');
  assert.equal(res.writtenBytes, 0, 'a preserved repack writes no bytes');
  assert.equal(res.backup, null, 'no write means no backup (owner policy)');
  assert.equal(res.errorCode, null);

  const after = fs.readFileSync(p);
  assert.equal(sha(after), beforeSha, 'artifact SHA unchanged');
  assert.ok(Buffer.compare(after, before) === 0, 'artifact bytes unchanged');
  assert.equal(fs.statSync(p).mtimeMs, beforeMtime, 'mtime unchanged');
  assert.deepEqual(tempsIn(dir), [], 'no temp file was created');
  assert.deepEqual(backupsIn(dir), [], 'no backup was created');

  // Lane A still has the final word, measured off the untouched artifact.
  assert.equal(res.sizeAuthority, 'measured');
  assert.equal(res.sizeStatus, 'pass');
  assert.equal(res.artifactBytes, before.length,
    'the reported upload size is the REAL artifact, not a candidate');
});

// --- 10. save-error reporting ------------------------------------------------

test('B2: safeSave EEXTERNAL gives the specific message and zero counts', () => {
  const p = path.join(tmpDir('err-eexternal'), 'item.wrl');
  const res = repackMall({ mallPath: p, text: WRL, asGzip: true }, {
    safeSave: () => {
      const e = new Error('simulated eexternal');
      e.code = 'EEXTERNAL';
      throw e;
    },
  });

  assert.equal(res.saved, false);
  assert.equal(res.errorCode, 'EEXTERNAL');
  assert.equal(res.message, SAVE_ERROR_MESSAGE.EEXTERNAL);
  assert.equal(res.writtenBytes, 0);
  assert.equal(res.backup, null);
  assert.equal(res.candidateBytes, null);
  assert.equal(res.maxBytes, null);
  assert.equal(res.overBytes, null);
});

test('B2: safeSave unknown error falls back to ESAVE and a generic message', () => {
  const p = path.join(tmpDir('err-unknown'), 'item.wrl');
  const res = repackMall({ mallPath: p, text: WRL, asGzip: true }, {
    safeSave: () => {
      throw new Error('disk full');
    },
  });

  assert.equal(res.saved, false);
  assert.equal(res.errorCode, 'ESAVE');
  assert.equal(res.message, 'Not saved — disk full');
  assert.equal(res.writtenBytes, 0);
  assert.equal(res.backup, null);
});

test('B2: safeSave EVERIFY gives the exact EVERIFY message text', () => {
  const p = path.join(tmpDir('err-everify'), 'item.wrl');
  const res = repackMall({ mallPath: p, text: WRL, asGzip: true }, {
    safeSave: () => {
      const e = new Error('simulated everify');
      e.code = 'EVERIFY';
      throw e;
    },
  });

  assert.equal(res.saved, false);
  assert.equal(res.errorCode, 'EVERIFY');
  assert.equal(res.message, SAVE_ERROR_MESSAGE.EVERIFY);
});

test('B2: real-filesystem unchanged-gzip no-op has specific message, real save has null message, neither breaks backup expectations', () => {
  const dir = tmpDir('err-messages');
  const p = path.join(dir, 'item.wrl');

  // Real save (initial write)
  const res1 = repackMall({ mallPath: p, text: WRL, asGzip: true });
  assert.equal(res1.saved, true);
  assert.equal(res1.message, null, 'a real save has no success message text');
  // Initial save from scratch has no backup because there was no prior file

  // Create a backup manually so the next real save overwrite has something to back up
  fs.writeFileSync(p, foreignGzip(WRL));
  const beforeBackups = backupsIn(dir).length;

  // Unchanged gzip no-op
  const res2 = repackMall({ mallPath: p, text: WRL, asGzip: true });
  assert.equal(res2.saved, true);
  assert.equal(res2.preserved, true);
  assert.equal(res2.message, 'Already saved — the existing gzip artifact already matches this text, so nothing was rewritten.');
  assert.equal(res2.backup, null, 'no write means no backup returned');
  assert.equal(backupsIn(dir).length, beforeBackups, 'Mall behavior must not change: no write means no backup on disk');

  // Overwrite with change
  const res3 = repackMall({ mallPath: p, text: WRL + '# modified\n', asGzip: true });
  assert.equal(res3.saved, true);
  assert.equal(res3.preserved, false);
  assert.equal(res3.message, null);
  assert.ok(res3.backup, 'a real overwrite creates a backup');
  assert.equal(backupsIn(dir).length, beforeBackups + 1, 'backup file was written');
});

// --- 2. over-limit refusal ---------------------------------------------------

// A deterministic, cross-platform way to exercise the ceiling without shipping
// an 80 KiB fixture: inject a zlib whose gzip output is padded past the limit.
// The bytes still decode correctly, so the ONLY thing that refuses the save is
// the size guard -- which is exactly what this test must isolate.
function oversizeZlib(overBy) {
  return {
    gzipSync: (buf, opts) => {
      const real = zlib.gzipSync(buf, opts);
      // A gzip member followed by trailing filler: gunzip still yields the
      // original bytes, so the candidate VERIFIES and is refused on size alone.
      return Buffer.concat([real, Buffer.alloc(MALL_UPLOAD_MAX_BYTES + overBy - real.length, 0)]);
    },
    gunzipSync: zlib.gunzipSync,
  };
}

test('B2: an over-limit gzip candidate is refused BEFORE any mutation (ESIZE)', () => {
  const dir = tmpDir('oversize');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, foreignGzip(WRL));
  const beforeSha = sha(fs.readFileSync(p));
  const beforeBytes = fs.readFileSync(p).length;

  // Edited text -> preservation cannot apply, so a candidate is really built.
  const edited = WRL + '# edited\n';
  const res = repackMall({ mallPath: p, text: edited, asGzip: true }, { zlib: oversizeZlib(1) });

  assert.equal(res.saved, false, 'the save was refused');
  assert.equal(res.preserved, false);
  assert.equal(res.errorCode, 'ESIZE');
  assert.equal(res.maxBytes, MALL_UPLOAD_MAX_BYTES, 'the exact 80 KiB limit');
  assert.equal(res.maxBytes, 81920, 'the Mall ceiling is 80 * 1024 = 81,920 B');
  assert.ok(res.candidateBytes > MALL_UPLOAD_MAX_BYTES, 'candidate is over the limit');
  assert.equal(res.overBytes, res.candidateBytes - MALL_UPLOAD_MAX_BYTES,
    'overBytes === candidateBytes - limit');
  assert.equal(res.writtenBytes, 0);
  assert.equal(res.backup, null, 'a refusal creates no backup');
  assert.match(res.message, /was not changed/);

  assert.equal(sha(fs.readFileSync(p)), beforeSha, 'original SHA unchanged');
  assert.equal(fs.readFileSync(p).length, beforeBytes, 'original bytes unchanged');
  assert.deepEqual(tempsIn(dir), [], 'no temp left behind');
  assert.deepEqual(backupsIn(dir), [], 'no backup was created');

  // The refusal does NOT invent a verdict about the surviving artifact: Lane A
  // still measures it, and it is stale relative to the edited text.
  assert.equal(res.sizeStatus, 'stale', 'the surviving artifact no longer matches the edited text');
});

// --- 3. exact-limit boundary -------------------------------------------------

test('B2: a candidate of exactly the limit is ALLOWED (guard is >, not >=)', () => {
  const dir = tmpDir('exact');
  const p = path.join(dir, 'item.wrl');
  const edited = WRL + '# exact\n';

  // Pad the candidate to EXACTLY MALL_UPLOAD_MAX_BYTES.
  const exactZlib = {
    gzipSync: (buf, opts) => {
      const real = zlib.gzipSync(buf, opts);
      return Buffer.concat([real, Buffer.alloc(MALL_UPLOAD_MAX_BYTES - real.length, 0)]);
    },
    gunzipSync: zlib.gunzipSync,
  };

  const res = repackMall({ mallPath: p, text: edited, asGzip: true }, { zlib: exactZlib });

  assert.equal(res.saved, true, 'exactly at the limit must be allowed');
  assert.equal(res.preserved, false);
  assert.equal(MALL_UPLOAD_MAX_BYTES, 81920, 'boundary is 80 * 1024 = 81,920 B');
  assert.equal(res.writtenBytes, MALL_UPLOAD_MAX_BYTES,
    'the candidate really was exactly the limit');
  assert.equal(fs.statSync(p).size, MALL_UPLOAD_MAX_BYTES);

  // One byte more is refused -- same fixture, same code path, opposite verdict.
  const dir2 = tmpDir('exact-plus-one');
  const p2 = path.join(dir2, 'item.wrl');
  const res2 = repackMall({ mallPath: p2, text: edited, asGzip: true }, { zlib: oversizeZlib(1) });
  assert.equal(res2.saved, false);
  assert.equal(res2.errorCode, 'ESIZE');
  assert.equal(res2.candidateBytes, 81921, 'exactly 81,921 B is the first refused size');
  assert.equal(res2.overBytes, 1, 'one byte over is one byte over');
  assert.equal(fs.existsSync(p2), false, 'nothing was created');
});

// --- 4. under-limit real write ----------------------------------------------

test('B2: a changed under-limit gzip candidate saves safely, with a backup of the prior artifact', () => {
  const dir = tmpDir('under');
  const p = path.join(dir, 'item.wrl');
  const priorBytes = foreignGzip(WRL);
  fs.writeFileSync(p, priorBytes);

  const edited = WRL + '# edited under limit\n';
  const res = repackMall({ mallPath: p, text: edited, asGzip: true });

  assert.equal(res.saved, true);
  assert.equal(res.preserved, false, 'the text changed, so a real write happened');
  assert.ok(res.backup, 'a real overwrite creates its normal timestamped backup');
  assert.ok(Buffer.compare(fs.readFileSync(res.backup), priorBytes) === 0,
    'the backup holds the exact prior artifact bytes');

  const now = fs.readFileSync(p);
  assert.equal(now.length, res.writtenBytes, 'writtenBytes equals the real new artifact size');
  assert.equal(now[0], 0x1f, 'new artifact is gzip');
  assert.equal(now[1], 0x8b);
  assert.equal(zlib.gunzipSync(now).toString('utf8'), edited,
    'the new artifact decompresses to exactly the edited text');

  // Lane A measured the file that now exists.
  assert.equal(res.sizeAuthority, 'measured');
  assert.equal(res.sizeStatus, 'pass');
  assert.equal(res.artifactBytes, now.length);
  assert.deepEqual(tempsIn(dir), [], 'no temp left behind');
});

// --- 5. candidate verification failure --------------------------------------

test('B2: a candidate that does not decode to the text is refused (EVERIFY) before mutation', () => {
  const dir = tmpDir('everify');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, foreignGzip(WRL));
  const beforeSha = sha(fs.readFileSync(p));

  // An encoder that compresses DIFFERENT bytes than it was handed.
  const tamperZlib = {
    gzipSync: (_buf, opts) => zlib.gzipSync(Buffer.from('tampered', 'utf8'), opts),
    gunzipSync: zlib.gunzipSync,
  };

  const res = repackMall({ mallPath: p, text: WRL + '# real\n', asGzip: true }, { zlib: tamperZlib });

  assert.equal(res.saved, false);
  assert.equal(res.errorCode, 'EVERIFY');
  assert.equal(res.writtenBytes, 0);
  assert.equal(res.backup, null, 'no backup before the candidate is proven');
  assert.equal(sha(fs.readFileSync(p)), beforeSha, 'original untouched');
  assert.deepEqual(tempsIn(dir), [], 'no completed temp');
  assert.deepEqual(backupsIn(dir), [], 'no backup');
});

test('B2: safeSave verifies the candidate BEFORE creating any temp file', () => {
  const dir = tmpDir('everify-order');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, foreignGzip(WRL));

  const tamperZlib = {
    gzipSync: (_buf, opts) => zlib.gzipSync(Buffer.from('tampered', 'utf8'), opts),
    gunzipSync: zlib.gunzipSync,
  };
  // An fs that fails loudly if anything tries to open a file for writing: the
  // candidate check must reject before the temp path is ever touched.
  const noWriteFs = new Proxy(fs, {
    get(t, k) {
      if (k === 'openSync') return () => { throw new Error('temp was opened before the candidate was verified'); };
      return t[k];
    },
  });

  assert.throws(
    () => io.safeSave({
      filePath: p, text: WRL + '# real\n', format: FORMAT.GZIP, verifyCandidate: true,
    }, { zlib: tamperZlib, fs: noWriteFs }),
    (err) => err.code === 'EVERIFY'
  );
});

test('B2: the size guard refuses before any temp file is opened', () => {
  const dir = tmpDir('esize-order');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, foreignGzip(WRL));

  const noWriteFs = new Proxy(fs, {
    get(t, k) {
      if (k === 'openSync') return () => { throw new Error('temp was opened before the size guard ran'); };
      return t[k];
    },
  });

  assert.throws(
    () => io.safeSave({
      filePath: p, text: WRL + '# big\n', format: FORMAT.GZIP, maxBytes: 4,
    }, { fs: noWriteFs }),
    (err) => err.code === 'ESIZE' && err.maxBytes === 4 && err.overBytes === err.candidateBytes - 4
  );
});

// --- 6. failure safety: write / fsync / read-back / rename -------------------

function faultyFs(overrides) {
  return new Proxy(fs, { get: (t, k) => (k in overrides ? overrides[k] : t[k]) });
}

test('B2: a temp write failure leaves the Mall artifact intact with no backup', () => {
  const dir = tmpDir('writefail');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, foreignGzip(WRL));
  const beforeSha = sha(fs.readFileSync(p));

  const res = repackMall({ mallPath: p, text: WRL + '# x\n', asGzip: true }, {
    fs: faultyFs({ writeSync: () => { throw new Error('simulated disk-full'); } }),
  });

  assert.equal(res.saved, false);
  assert.match(res.message, /disk-full/);
  assert.equal(sha(fs.readFileSync(p)), beforeSha, 'original intact');
  assert.deepEqual(tempsIn(dir), [], 'temp cleaned up');
  assert.deepEqual(backupsIn(dir), [], 'backup never happened -- it comes after verification');
});

test('B2: an fsync failure leaves the Mall artifact intact with no backup', () => {
  const dir = tmpDir('fsyncfail');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, foreignGzip(WRL));
  const beforeSha = sha(fs.readFileSync(p));

  const res = repackMall({ mallPath: p, text: WRL + '# x\n', asGzip: true }, {
    fs: faultyFs({ fsyncSync: () => { throw new Error('simulated fsync failure'); } }),
  });

  assert.equal(res.saved, false);
  assert.equal(sha(fs.readFileSync(p)), beforeSha, 'original intact');
  assert.deepEqual(tempsIn(dir), [], 'temp cleaned up');
  assert.deepEqual(backupsIn(dir), [], 'no backup');
});

test('B2: a temp read-back failure leaves the Mall artifact intact with no backup', () => {
  const dir = tmpDir('readbackfail');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, foreignGzip(WRL));
  const beforeSha = sha(fs.readFileSync(p));

  const res = repackMall({ mallPath: p, text: WRL + '# x\n', asGzip: true }, {
    fs: faultyFs({
      readFileSync: (f, ...rest) => {
        if (String(f).includes('wrlforge-tmp')) throw new Error('simulated temp read failure');
        return fs.readFileSync(f, ...rest);
      },
    }),
  });

  assert.equal(res.saved, false);
  assert.equal(sha(fs.readFileSync(p)), beforeSha, 'original intact');
  assert.deepEqual(tempsIn(dir), [], 'temp cleaned up');
  assert.deepEqual(backupsIn(dir), [], 'no backup');
});

test('B2: a temp that decodes to the wrong text is refused after the read-back, original intact', () => {
  const dir = tmpDir('tempverify');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, foreignGzip(WRL));
  const beforeSha = sha(fs.readFileSync(p));
  const edited = WRL + '# x\n';

  // The in-memory candidate is correct; the bytes that come BACK off the temp
  // are not. This is the second, independent verification -- it proves the disk,
  // where the first proved the encoder.
  const res = repackMall({ mallPath: p, text: edited, asGzip: true }, {
    fs: faultyFs({
      readFileSync: (f, ...rest) => {
        if (String(f).includes('wrlforge-tmp')) return zlib.gzipSync(Buffer.from('not the text', 'utf8'));
        return fs.readFileSync(f, ...rest);
      },
    }),
  });

  assert.equal(res.saved, false);
  assert.equal(res.errorCode, 'EVERIFY', 'the post-temp check still exists and still fires');
  assert.equal(sha(fs.readFileSync(p)), beforeSha, 'original intact');
  assert.deepEqual(tempsIn(dir), [], 'temp cleaned up');
  assert.deepEqual(backupsIn(dir), [], 'backup comes AFTER verification, so none was taken');
});

test('B2: a rename failure leaves the destination intact and the temp cleaned; the backup already exists', () => {
  const dir = tmpDir('renamefail');
  const p = path.join(dir, 'item.wrl');
  const prior = foreignGzip(WRL);
  fs.writeFileSync(p, prior);
  const beforeSha = sha(prior);

  const res = repackMall({ mallPath: p, text: WRL + '# x\n', asGzip: true }, {
    fs: faultyFs({ renameSync: () => { throw new Error('simulated rename failure'); } }),
  });

  assert.equal(res.saved, false);
  assert.equal(sha(fs.readFileSync(p)), beforeSha, 'destination intact -- the swap never happened');
  assert.deepEqual(tempsIn(dir), [], 'temp cleaned up');

  // Honest reporting: the backup is created immediately before the rename, so a
  // rename failure DOES leave a timestamped backup behind. That is not a
  // rollback failure -- the backup is a copy, the original is untouched -- but
  // claiming "nothing was created" would be false.
  const backups = backupsIn(dir);
  assert.equal(backups.length, 1, 'the pre-rename backup remains on disk');
  assert.ok(Buffer.compare(fs.readFileSync(path.join(dir, backups[0])), prior) === 0,
    'and it is an exact copy of the still-present original');
});

// --- 7. plain output ---------------------------------------------------------

test('B2: plain output writes safely, backs up, and gets no gzip size guard or preservation', () => {
  const dir = tmpDir('plain');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, 'old plain text\n', 'utf8');

  const res = repackMall({ mallPath: p, text: WRL, asGzip: false });

  assert.equal(res.saved, true);
  assert.equal(res.preserved, false, 'plain output never takes the preservation shortcut');
  assert.ok(res.backup, 'an overwrite still creates a backup');
  assert.equal(fs.readFileSync(res.backup, 'utf8'), 'old plain text\n');
  assert.equal(fs.readFileSync(p, 'utf8'), WRL, 'plain text written verbatim');
  assert.equal(res.writtenBytes, Buffer.byteLength(WRL, 'utf8'));

  // No false Mall PASS: a plain file is not an upload artifact.
  assert.equal(res.sizeStatus, 'unknown');
  assert.equal(res.sizeReason, 'no-gzip-artifact');
  assert.equal(res.artifactBytes, null, 'a plain file is never reported as an upload artifact');
  assert.equal(res.mallReady, false);
});

test('B2: a plain save is NOT blocked by the Mall upload limit', () => {
  const dir = tmpDir('plain-big');
  const p = path.join(dir, 'item.wrl');
  // Text far larger than the gzip upload ceiling.
  const big = WRL + '#' + 'a'.repeat(MALL_UPLOAD_MAX_BYTES + 100) + '\n';

  const res = repackMall({ mallPath: p, text: big, asGzip: false });

  assert.equal(res.saved, true, 'the gzip upload ceiling must not gate a plain write');
  assert.equal(res.errorCode, null);
  assert.equal(fs.statSync(p).size, Buffer.byteLength(big, 'utf8'));
});

// --- 8. preservation runs before the size guard ------------------------------

test('B2: an unchanged artifact is preserved even when re-encoding it WOULD exceed the limit', () => {
  const dir = tmpDir('preserve-beats-size');
  const p = path.join(dir, 'item.wrl');
  const prior = foreignGzip(WRL);
  fs.writeFileSync(p, prior);
  const beforeSha = sha(prior);

  // This is the Ragnum shape in miniature: the existing artifact is small and
  // legal, but this runtime's encoder would produce something over the limit.
  // Preservation must win, because no write is needed at all.
  const res = repackMall({ mallPath: p, text: WRL, asGzip: true }, { zlib: oversizeZlib(1) });

  assert.equal(res.saved, true);
  assert.equal(res.preserved, true, 'the existing artifact was preserved, not judged on a candidate');
  assert.equal(res.errorCode, null, 'ESIZE must never apply to a file that needs no write');
  assert.equal(sha(fs.readFileSync(p)), beforeSha);
  assert.equal(res.sizeStatus, 'pass', 'the real artifact still passes');
});

// --- 9. Lane A stays authoritative -------------------------------------------

test('B2: the reported upload size is the measured artifact, never the candidate', () => {
  const dir = tmpDir('lane-a');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, foreignGzip(WRL));

  const res = repackMall({ mallPath: p, text: WRL, asGzip: true });
  assert.equal(res.candidateBytes, null, 'a successful save reports no candidate count');
  assert.equal(res.artifactBytes, fs.statSync(p).size,
    'artifactBytes is the real file size on disk');
  assert.equal(res.sizeAuthority, 'measured');
  assert.equal(res.mallUploadMaxBytes, MALL_UPLOAD_MAX_BYTES,
    'the one authoritative limit rides along in the payload');
});

test('B2: the Mall limit is not duplicated as a literal in production repack code', () => {
  const src = fs.readFileSync(path.join(__dirname, '..', '..', 'src', 'mall', 'repack.js'), 'utf8');
  for (const bad of ['80000', '81920', '80 * 1024']) {
    assert.ok(!src.includes(bad), `repack.js must not hard-code ${bad}`);
  }
  assert.ok(src.includes('MALL_UPLOAD_MAX_BYTES'), 'it uses the exported constant');
  const fio = fs.readFileSync(path.join(__dirname, '..', '..', 'src', 'editor', 'file-io.js'), 'utf8');
  assert.ok(!/require\(['"][^'"]*validator['"]\)/.test(fio),
    'file-io.js must stay profile-neutral and never import validator.js');
});

test('B2: main.js no longer writes the Mall artifact directly', () => {
  const src = fs.readFileSync(path.join(__dirname, '..', '..', 'main.js'), 'utf8');
  // Comment-blind: the header note ABOVE the handler names the old call in
  // order to say it is gone, and a source scan that cannot tell code from
  // prose would fail on its own documentation.
  const code = src.split('\n').filter((l) => !/^\s*\/\//.test(l)).join('\n');
  assert.ok(!code.includes('fs.writeFileSync(mallPath'),
    'the direct mall write is gone');
  assert.ok(!code.includes('copyFileSync(mallPath'),
    'the handler no longer runs its own backup-before-write');
  assert.ok(code.includes("require('./src/mall/repack')"),
    'mall:repack routes through the safe helper');
});
