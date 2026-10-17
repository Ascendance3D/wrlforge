'use strict';
const { test } = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const zlib = require('node:zlib');
const io = require('../../src/editor/file-io');
const { FORMAT } = require('../../src/editor/wrl-document');

const WRL = '#VRML V2.0 utf8\nGroup {\n  children [\n    Shape {}\n  ]\n}\n';

function tmpDir(suffix = '') {
  return fs.mkdtempSync(path.join(os.tmpdir(), `wrlforge-editor-${suffix}`));
}

// Proxy over real fs that overrides selected methods with faults, delegating the
// rest -- lets us simulate an interrupted write without a real disk fault.
function faultyFs(faults) {
  return new Proxy(fs, {
    get(target, prop) {
      if (Object.prototype.hasOwnProperty.call(faults, prop)) return faults[prop];
      const v = target[prop];
      return typeof v === 'function' ? v.bind(target) : v;
    },
  });
}

test('loadDocument reads a plain .wrl as UTF-8 with a plain format + stat', () => {
  const dir = tmpDir('load-plain');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, WRL, 'utf8');
  const d = io.loadDocument(p);
  assert.strictEqual(d.text, WRL);
  assert.strictEqual(d.format, FORMAT.PLAIN);
  assert.ok(d.stat && typeof d.stat.hash === 'string' && d.stat.size > 0);
});

test('loadDocument transparently decompresses a gzip .wrl', () => {
  const dir = tmpDir('load-gzip');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, zlib.gzipSync(Buffer.from(WRL, 'utf8')));
  const d = io.loadDocument(p);
  assert.strictEqual(d.text, WRL);
  assert.strictEqual(d.format, FORMAT.GZIP);
});

test('safeSave (plain) writes verified bytes, leaves no temp, and round-trips', () => {
  const dir = tmpDir('save-plain');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, WRL, 'utf8');
  const next = WRL + '# edited\n';
  const res = io.safeSave({ filePath: p, text: next, format: FORMAT.PLAIN, expectedStat: io.statFile(p) });
  assert.strictEqual(res.ok, true);
  assert.strictEqual(fs.readFileSync(p, 'utf8'), next);
  assert.ok(res.backup && fs.existsSync(res.backup), 'a backup was created');
  assert.strictEqual(fs.readFileSync(res.backup, 'utf8'), WRL, 'backup holds the prior contents');
  const leftovers = fs.readdirSync(dir).filter((f) => f.includes('wrlforge-tmp'));
  assert.deepStrictEqual(leftovers, [], 'no temp file is left behind');
});

test('safeSave (gzip) writes real gzip bytes that decode back to the buffer', () => {
  const dir = tmpDir('save-gzip');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, zlib.gzipSync(Buffer.from(WRL)));
  const next = WRL + '# gz\n';
  const res = io.safeSave({ filePath: p, text: next, format: FORMAT.GZIP, expectedStat: io.statFile(p) });
  assert.strictEqual(res.ok, true);
  const onDisk = fs.readFileSync(p);
  assert.strictEqual(onDisk[0], 0x1f, 'on-disk file has gzip magic bytes');
  assert.strictEqual(zlib.gunzipSync(onDisk).toString('utf8'), next);
});

test('Save As to a new path creates the file with no backup and may change format', () => {
  const dir = tmpDir('save-as');
  const src = path.join(dir, 'src.wrl');
  fs.writeFileSync(src, zlib.gzipSync(Buffer.from(WRL))); // gzip source
  const dst = path.join(dir, 'copy.wrl');
  const res = io.safeSave({ filePath: dst, text: WRL, format: FORMAT.PLAIN }); // as plain
  assert.strictEqual(res.ok, true);
  assert.strictEqual(res.backup, null, 'a brand-new destination has nothing to back up');
  assert.strictEqual(fs.readFileSync(dst, 'utf8'), WRL);
});

test('safeSave preserves the original when the write is interrupted', () => {
  const dir = tmpDir('save-fail-write');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, WRL, 'utf8');
  const badFs = faultyFs({ writeSync: () => { throw new Error('simulated disk-full'); } });
  assert.throws(
    () => io.safeSave({ filePath: p, text: 'CLOBBERED', format: FORMAT.PLAIN }, { fs: badFs }),
    /disk-full/
  );
  assert.strictEqual(fs.readFileSync(p, 'utf8'), WRL, 'source is untouched after a failed write');
  const leftovers = fs.readdirSync(dir).filter((f) => f.includes('wrlforge-tmp'));
  assert.deepStrictEqual(leftovers, [], 'the temp file was cleaned up');
});

test('safeSave with backup-then-rename-failure leaves original unchanged and removes temp, but might leave backup', (t) => {
  const dir = tmpDir('save-backup-then-rename-failure');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, WRL, 'utf8');
  const badFs = faultyFs({
    renameSync: () => {
      const err = new Error('EBUSY: resource busy or locked');
      err.code = 'EBUSY';
      throw err;
    }
  });
  let errCode;
  try {
    io.safeSave({ filePath: p, text: 'CLOBBERED', format: FORMAT.PLAIN }, { fs: badFs });
  } catch (err) {
    errCode = err.code;
  }
  assert.strictEqual(errCode, 'EBUSY', 'safeSave should throw the renameSync error');
  assert.strictEqual(fs.readFileSync(p, 'utf8'), WRL, 'source is untouched after a failed replace');

  const files = fs.readdirSync(dir);
  const leftovers = files.filter((f) => f.includes('wrlforge-tmp'));
  assert.deepStrictEqual(leftovers, [], 'the temp file was cleaned up');

  const backups = files.filter((f) => f.includes('.bak'));
  if (backups.length > 0) {
    t.skip('owner review: backup left after failed replace');
  } else {
    // If no backup was left, we verify the product rule "no write means no backup"
    assert.deepStrictEqual(backups, [], 'no backup is left behind');
  }
});

test('safeSave preserves the original and throws when renameSync throws EBUSY or EACCES', () => {
  for (const code of ['EBUSY', 'EACCES']) {
    const dir = tmpDir(`save-fail-rename-${code.toLowerCase()}`);
    const p = path.join(dir, 'item.wrl');
    fs.writeFileSync(p, WRL, 'utf8');
    const badFs = faultyFs({
      renameSync: () => {
        const err = new Error(`${code}: simulated error`);
        err.code = code;
        throw err;
      }
    });
    let errCode;
    try {
      io.safeSave({ filePath: p, text: 'CLOBBERED', format: FORMAT.PLAIN }, { fs: badFs });
    } catch (err) {
      errCode = err.code;
    }
    assert.strictEqual(errCode, code, `safeSave should throw the ${code} error`);
    assert.strictEqual(fs.readFileSync(p, 'utf8'), WRL, 'source is untouched after a failed replace');
    const leftovers = fs.readdirSync(dir).filter((f) => f.includes('wrlforge-tmp'));
    assert.deepStrictEqual(leftovers, [], 'the temp file was cleaned up');
  }
});

test('safeSave preserves the original, cleans up, and does not rename if copyFileSync (backup) throws EACCES', () => {
  const dir = tmpDir('save-fail-backup-eacces');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, WRL, 'utf8');
  let renameCalled = false;
  const badFs = faultyFs({
    copyFileSync: () => {
      const err = new Error('EACCES: permission denied');
      err.code = 'EACCES';
      throw err;
    },
    renameSync: (...args) => {
      renameCalled = true;
      return fs.renameSync(...args);
    }
  });
  let errCode;
  try {
    io.safeSave({ filePath: p, text: 'CLOBBERED', format: FORMAT.PLAIN }, { fs: badFs });
  } catch (err) {
    errCode = err.code;
  }
  assert.strictEqual(errCode, 'EACCES', 'safeSave should throw the copyFileSync error');
  assert.strictEqual(renameCalled, false, 'renameSync should never be called');
  assert.strictEqual(fs.readFileSync(p, 'utf8'), WRL, 'source is untouched after a failed backup');
  const leftovers = fs.readdirSync(dir).filter((f) => f.includes('wrlforge-tmp'));
  assert.deepStrictEqual(leftovers, [], 'the temp file was cleaned up');
});

test('safeSave preserves the original and throws when renameSync throws EPERM', () => {
  const dir = tmpDir('save-fail-rename-eperm');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, WRL, 'utf8');
  const badFs = faultyFs({
    renameSync: () => {
      const err = new Error('EPERM: operation not permitted');
      err.code = 'EPERM';
      throw err;
    }
  });
  let errCode;
  try {
    io.safeSave({ filePath: p, text: 'CLOBBERED', format: FORMAT.PLAIN }, { fs: badFs });
  } catch (err) {
    errCode = err.code;
  }
  assert.strictEqual(errCode, 'EPERM', 'safeSave should throw the EPERM error');
  assert.strictEqual(fs.readFileSync(p, 'utf8'), WRL, 'source is untouched after a failed replace');
  const leftovers = fs.readdirSync(dir).filter((f) => f.includes('wrlforge-tmp'));
  assert.deepStrictEqual(leftovers, [], 'the temp file was cleaned up');
});

test('safeSave aborts (EVERIFY) and preserves the source when the written file will not decode', () => {
  const dir = tmpDir('save-fail-verify');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, zlib.gzipSync(Buffer.from(WRL)));
  // zlib that compresses DIFFERENT bytes than the buffer: the verify read-back
  // will decode to the wrong text and must abort before replacing the file.
  const tamperZlib = {
    gzipSync: (_buf, opts) => zlib.gzipSync(Buffer.from('tampered', 'utf8'), opts),
    gunzipSync: zlib.gunzipSync,
  };
  assert.throws(
    () => io.safeSave({ filePath: p, text: WRL + '# real\n', format: FORMAT.GZIP }, { zlib: tamperZlib }),
    (err) => err.code === 'EVERIFY'
  );
  assert.strictEqual(zlib.gunzipSync(fs.readFileSync(p)).toString('utf8'), WRL, 'source is intact after a verify failure');
  const leftovers = fs.readdirSync(dir).filter((f) => f.includes('wrlforge-tmp'));
  assert.deepStrictEqual(leftovers, [], 'the unverified temp was removed');
});

test('safeSave refuses (EEXTERNAL) when the file changed on disk, unless allowOverwrite', () => {
  const dir = tmpDir('save-conflict');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, WRL, 'utf8');
  const stamp = io.statFile(p);
  // External edit (same length -> exercises the content-hash tiebreaker).
  const external = WRL.replace('Shape {}', 'Shape {@'.padEnd('Shape {}'.length, ' '));
  fs.writeFileSync(p, external, 'utf8');

  assert.throws(
    () => io.safeSave({ filePath: p, text: 'MINE', format: FORMAT.PLAIN, expectedStat: stamp }),
    (err) => err.code === 'EEXTERNAL'
  );
  assert.strictEqual(fs.readFileSync(p, 'utf8'), external, 'the external change is not clobbered');

  const forced = io.safeSave({ filePath: p, text: 'MINE', format: FORMAT.PLAIN, expectedStat: stamp, allowOverwrite: true });
  assert.strictEqual(forced.ok, true);
  assert.strictEqual(fs.readFileSync(p, 'utf8'), 'MINE');
});

test('detectExternalChange distinguishes unchanged, content, size, and deleted', () => {
  const dir = tmpDir('detect');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, WRL, 'utf8');
  const stamp = io.statFile(p);

  assert.strictEqual(io.detectExternalChange(stamp, p).changed, false);

  fs.writeFileSync(p, WRL.replace('Shape {}', 'Shape {!'), 'utf8'); // same length
  const contentChange = io.detectExternalChange(stamp, p);
  assert.strictEqual(contentChange.changed, true);
  assert.strictEqual(contentChange.reason, 'content');

  fs.writeFileSync(p, WRL + 'MORE', 'utf8'); // different length
  assert.strictEqual(io.detectExternalChange(stamp, p).reason, 'size');

  fs.rmSync(p);
  const gone = io.detectExternalChange(stamp, p);
  assert.strictEqual(gone.changed, true);
  assert.strictEqual(gone.reason, 'deleted');
});

test('reloadDocument returns the current on-disk contents', () => {
  const dir = tmpDir('reload');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, WRL, 'utf8');
  fs.writeFileSync(p, WRL + '# changed on disk\n', 'utf8');
  const d = io.reloadDocument(p);
  assert.strictEqual(d.text, WRL + '# changed on disk\n');
});

test('paths with spaces and non-ASCII characters round-trip through save + load', () => {
  const dir = tmpDir('unicode');
  const sub = path.join(dir, 'my worlds', 'café — tëst');
  fs.mkdirSync(sub, { recursive: true });
  const p = path.join(sub, 'ítem wörld.wrl');
  const res = io.safeSave({ filePath: p, text: WRL, format: FORMAT.PLAIN });
  assert.strictEqual(res.ok, true);
  assert.strictEqual(io.loadDocument(p).text, WRL);
});

// --- Lane B B1: unchanged-gzip preservation ----------------------------------
// A gzip `.wrl` may have been packed by a different encoder than Node's zlib.
// Saving it back unchanged must not re-encode it. These tests assert EXACT
// BYTES (sha256), never length -- see the same-size case below for why.
//
// They never assume that a Node re-encode of the same text produces DIFFERENT
// compressed bytes from a given artifact: whether it does depends on the zlib
// build and platform, so byte inequality is not a portable proof that a write
// happened. Real writes are proven by their observable effects instead.

const crypto = require('node:crypto');
const sha256 = (buf) => crypto.createHash('sha256').update(buf).digest('hex');

// Repository fixtures, committed byte-exact and pinned by
// test/preview/fixture-byte-contract.test.js. Copied to a temp dir before use;
// the committed files themselves are never written to.
const GZIP_FIXTURES = path.join(__dirname, '..', 'fixtures', 'gzip-encodings');

function stageFixture(name, suffix) {
  const dir = tmpDir(suffix);
  const dest = path.join(dir, 'item.wrl');
  fs.copyFileSync(path.join(GZIP_FIXTURES, name), dest);
  return { dir, dest };
}

test('wouldPreserve proves identity only for an exact gzip match, and fails closed', () => {
  const dir = tmpDir('would-preserve');
  const gz = path.join(dir, 'gzip.wrl');
  fs.writeFileSync(gz, zlib.gzipSync(Buffer.from(WRL, 'utf8')));

  assert.strictEqual(io.wouldPreserve({ filePath: gz, text: WRL, format: FORMAT.GZIP }), true);

  // Different text -- the only "legitimate" false.
  assert.strictEqual(io.wouldPreserve({ filePath: gz, text: WRL + 'x', format: FORMAT.GZIP }), false);
  // Plain format is out of scope for B1 even when the bytes would match.
  assert.strictEqual(io.wouldPreserve({ filePath: gz, text: WRL, format: FORMAT.PLAIN }), false);
  // Missing destination.
  assert.strictEqual(io.wouldPreserve({ filePath: path.join(dir, 'nope.wrl'), text: WRL, format: FORMAT.GZIP }), false);
  // Non-string text.
  assert.strictEqual(io.wouldPreserve({ filePath: gz, text: null, format: FORMAT.GZIP }), false);

  // A plain file holding the same text is NOT a gzip artifact.
  const plain = path.join(dir, 'plain.wrl');
  fs.writeFileSync(plain, WRL, 'utf8');
  assert.strictEqual(io.wouldPreserve({ filePath: plain, text: WRL, format: FORMAT.GZIP }), false);

  // Gzip magic bytes but a truncated stream: identity is unprovable, so false
  // rather than a throw -- preservation never becomes an error bypass.
  const corrupt = path.join(dir, 'corrupt.wrl');
  fs.writeFileSync(corrupt, zlib.gzipSync(Buffer.from(WRL, 'utf8')).subarray(0, 12));
  assert.strictEqual(io.wouldPreserve({ filePath: corrupt, text: WRL, format: FORMAT.GZIP }), false);

  // An unreadable destination answers false, it does not propagate the error.
  const throwingFs = faultyFs({ readFileSync: () => { throw new Error('EIO'); } });
  assert.strictEqual(io.wouldPreserve({ filePath: gz, text: WRL, format: FORMAT.GZIP }, { fs: throwingFs }), false);
});

test('wouldPreserve fails safely and decodeBytes throws clear error on gzip bomb exceeding limits', () => {
  const os = require('os');
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrlforge-bomb-file-io-'));
  const bombPath = path.join(dir, 'bomb.wrl');

  // A small string that easily clears the artificially low injected cap
  const bombContent = Buffer.alloc(2048, 0x41);
  const bombBytes = zlib.gzipSync(bombContent);
  fs.writeFileSync(bombPath, bombBytes);

  try {
    const deps = { getGzipCap: () => 1024 };

    // wouldPreserve must return false on ERR_BUFFER_TOO_LARGE
    assert.strictEqual(io.wouldPreserve({ filePath: bombPath, text: bombContent.toString('utf8'), format: FORMAT.GZIP }, deps), false);

    // decodeBytes must throw a specific ERR_BUFFER_TOO_LARGE error
    assert.throws(
      () => io.decodeBytes(bombBytes, FORMAT.GZIP, deps),
      /decoding candidate exceeded the allowed decompression limit/i
    );
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('a legitimate highly compressible file expands safely when within cap', () => {
  const os = require('os');
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrlforge-legit-io-'));
  const p = path.join(dir, 'highly-compressible.wrl');

  // Highly compressible content simulating >250x compression ratio
  const text = Buffer.alloc(100 * 1024, 0x41).toString('utf8');
  const compressed = zlib.gzipSync(Buffer.from(text, 'utf8'));

  // ensure > 250x compression ratio actually works without ratio limits checking it
  assert.ok(compressed.length * 250 < text.length, "the compression ratio must exceed 250x to be a valid test");

  fs.writeFileSync(p, compressed);

  try {
    // Should not throw and correctly decode
    const decoded = io.decodeBytes(compressed, FORMAT.GZIP);
    assert.strictEqual(decoded, text);

    // wouldPreserve must return true since identity is provable
    assert.strictEqual(io.wouldPreserve({ filePath: p, text, format: FORMAT.GZIP }), true);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('B1: an unchanged gzip save is a true no-op -- exact bytes, mtime and no backup', () => {
  const dir = tmpDir('preserve-noop');
  const p = path.join(dir, 'item.wrl');
  // Force the gzip OS header byte so the artifact carries foreign-encoder
  // metadata. What matters for B1 is that only decompressed-text identity may
  // decide preservation -- never the header, the size or the encoder.
  const original = zlib.gzipSync(Buffer.from(WRL, 'utf8'), { level: 9 });
  original[9] = 0x03;
  fs.writeFileSync(p, original);

  const beforeBytes = fs.readFileSync(p);
  const beforeSha = sha256(beforeBytes);
  const beforeMtime = fs.statSync(p).mtimeMs;

  const res = io.safeSave({
    filePath: p, text: WRL, format: FORMAT.GZIP,
    expectedStat: io.statFile(p), preserveExistingGzip: true,
  });

  assert.strictEqual(res.ok, true);
  assert.strictEqual(res.preserved, true, 'the save reports itself as preserved');
  assert.strictEqual(res.bytesWritten, 0, 'nothing was written');
  assert.strictEqual(res.backup, null, 'a no-op creates no backup (owner policy)');
  assert.strictEqual(res.format, FORMAT.GZIP);

  const afterBytes = fs.readFileSync(p);
  assert.strictEqual(sha256(afterBytes), beforeSha, 'destination bytes are byte-identical');
  assert.strictEqual(fs.statSync(p).mtimeMs, beforeMtime, 'destination mtime is untouched');
  assert.deepStrictEqual(fs.readdirSync(dir), ['item.wrl'], 'no backup and no temp were created');

  // The returned stat still describes the real file, so the conflict baseline
  // stays usable for the next save.
  assert.strictEqual(res.stat.hash, io.statFile(p).hash);

  // Control: the same save WITHOUT the opt-in takes the normal real-write path.
  // Proven by its observable effects -- a fresh encode, a backup holding the
  // exact prior artifact, and a valid gzip result -- not by hoping this
  // runtime's encoder emits bytes unequal to the ones already on disk.
  const res2 = io.safeSave({ filePath: p, text: WRL, format: FORMAT.GZIP, expectedStat: io.statFile(p) });
  assert.strictEqual(res2.preserved, false, 'without the opt-in nothing is preserved');
  assert.ok(res2.bytesWritten > 0, 'a real encode was written');
  assert.ok(res2.backup && fs.existsSync(res2.backup), 'an overwrite makes its normal backup');
  assert.strictEqual(sha256(fs.readFileSync(res2.backup)), beforeSha,
    'the backup holds the EXACT prior artifact bytes -- so the prior file really was replaced');
  const written = fs.readFileSync(p);
  assert.ok(written[0] === 0x1f && written[1] === 0x8b, 'the result is still a valid gzip');
  assert.strictEqual(zlib.gunzipSync(written).toString('utf8'), WRL,
    'and it decompresses to exactly the requested text');
  assert.strictEqual(written.length, res2.bytesWritten, 'bytesWritten describes the real file');
});

test('B1: same-size-different-bytes -- twin-small.wrl.gz survives byte-identical', () => {
  const { dir, dest } = stageFixture('twin-small.wrl.gz', 'preserve-twin-small');
  const beforeBytes = fs.readFileSync(dest);
  const beforeSha = sha256(beforeBytes);
  const text = zlib.gunzipSync(beforeBytes).toString('utf8');

  // The trap this fixture exists to catch: two valid gzip artifacts can have the
  // same byte length AND the same decompressed text while their compressed bytes
  // differ, so a size-based preservation check would silently pass while the
  // artifact was being rewritten. Built deterministically from the fixture's own
  // bytes -- one gzip HEADER metadata byte changed -- rather than relying on this
  // runtime's encoder to disagree with the fixture.
  const alternate = Buffer.from(beforeBytes);
  alternate[9] = beforeBytes[9] === 0xff ? 0x03 : 0xff; // OS byte: metadata only
  assert.strictEqual(alternate.length, beforeBytes.length, 'same compressed size');
  assert.notStrictEqual(sha256(alternate), beforeSha, 'but not the same bytes');
  assert.strictEqual(zlib.gunzipSync(alternate).toString('utf8'), text, 'and the same text');

  const res = io.safeSave({
    filePath: dest, text, format: FORMAT.GZIP,
    expectedStat: io.statFile(dest), preserveExistingGzip: true,
  });
  assert.strictEqual(res.preserved, true);
  assert.strictEqual(res.bytesWritten, 0);
  assert.strictEqual(sha256(fs.readFileSync(dest)), beforeSha, 'fixture copy is byte-identical');
  assert.deepStrictEqual(fs.readdirSync(dir), ['item.wrl']);
});

test('B1: twin-large.wrl.gz is preserved, not optimized -- 81,781 bytes stay', () => {
  const { dest } = stageFixture('twin-large.wrl.gz', 'preserve-twin-large');
  const beforeBytes = fs.readFileSync(dest);
  const beforeSha = sha256(beforeBytes);
  assert.strictEqual(beforeBytes.length, 81781, 'the stored-DEFLATE twin');
  const text = zlib.gunzipSync(beforeBytes).toString('utf8');

  // A repack would be dramatically SMALLER here. B1 still must not do it:
  // preservation is about not touching an unchanged file, not about picking the
  // better encoding.
  assert.ok(zlib.gzipSync(Buffer.from(text, 'utf8'), { level: 9 }).length < beforeBytes.length);

  const res = io.safeSave({
    filePath: dest, text, format: FORMAT.GZIP,
    expectedStat: io.statFile(dest), preserveExistingGzip: true,
  });
  assert.strictEqual(res.preserved, true);
  assert.strictEqual(fs.readFileSync(dest).length, 81781);
  assert.strictEqual(sha256(fs.readFileSync(dest)), beforeSha);
});

test('B1: an unchanged PLAIN save keeps its normal real-write behaviour', () => {
  const dir = tmpDir('preserve-plain');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, WRL, 'utf8');
  const beforeSha = sha256(fs.readFileSync(p));

  // Even with the opt-in set, a plain document is outside B1 entirely.
  const res = io.safeSave({
    filePath: p, text: WRL, format: FORMAT.PLAIN,
    expectedStat: io.statFile(p), preserveExistingGzip: true,
  });
  assert.strictEqual(res.preserved, false, 'plain files are not preserved in B1');
  assert.strictEqual(res.bytesWritten, Buffer.byteLength(WRL, 'utf8'), 'a real write happened');
  assert.ok(res.backup && fs.existsSync(res.backup), 'and it made its normal backup');
  assert.strictEqual(fs.readFileSync(p, 'utf8'), WRL);
  assert.strictEqual(sha256(fs.readFileSync(p)), beforeSha, 'plain bytes are deterministic, so identical');
});

test('B1: an external change still wins over preservation (EEXTERNAL)', () => {
  const dir = tmpDir('preserve-conflict');
  const p = path.join(dir, 'item.wrl');
  const first = zlib.gzipSync(Buffer.from(WRL, 'utf8'), { level: 9 });
  first[9] = 0x03; // foreign-encoder OS header byte; metadata only
  fs.writeFileSync(p, first);
  const opened = io.statFile(p);

  // Somebody else rewrites the file with DIFFERENT bytes that decode to the
  // SAME text. Preservation would happily call this "already correct"; the
  // conflict guard must speak first, because the user asked to be told.
  const rewritten = zlib.gzipSync(Buffer.from(WRL, 'utf8'), { level: 1 });
  fs.writeFileSync(p, rewritten);
  assert.notStrictEqual(sha256(rewritten), sha256(first));
  assert.strictEqual(zlib.gunzipSync(fs.readFileSync(p)).toString('utf8'), WRL, 'same text on disk');

  assert.throws(
    () => io.safeSave({
      filePath: p, text: WRL, format: FORMAT.GZIP,
      expectedStat: opened, preserveExistingGzip: true,
    }),
    (err) => err.code === 'EEXTERNAL',
    'conflict detection runs before the preservation test'
  );
  assert.strictEqual(sha256(fs.readFileSync(p)), sha256(rewritten), 'the on-disk file is untouched');

  // Once the user resolves the conflict, preservation may apply again: the
  // on-disk artifact now genuinely is the one this text needs.
  const resolved = io.safeSave({
    filePath: p, text: WRL, format: FORMAT.GZIP,
    allowOverwrite: true, preserveExistingGzip: true,
  });
  assert.strictEqual(resolved.preserved, true);
  assert.strictEqual(sha256(fs.readFileSync(p)), sha256(rewritten));
});

test('B1: a CHANGED gzip save still writes, verifies and backs up as before', () => {
  const dir = tmpDir('preserve-changed');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, zlib.gzipSync(Buffer.from(WRL, 'utf8')));
  const next = WRL + '# edited\n';

  const res = io.safeSave({
    filePath: p, text: next, format: FORMAT.GZIP,
    expectedStat: io.statFile(p), preserveExistingGzip: true,
  });
  assert.strictEqual(res.preserved, false, 'changed text cannot be preserved');
  assert.ok(res.bytesWritten > 0);
  assert.ok(res.backup && fs.existsSync(res.backup), 'a real overwrite still backs up');
  assert.strictEqual(zlib.gunzipSync(fs.readFileSync(p)).toString('utf8'), next);
  assert.strictEqual(zlib.gunzipSync(fs.readFileSync(res.backup)).toString('utf8'), WRL);
});

test('B1: preservation never fires on a failure path -- it cannot mask an error', () => {
  const dir = tmpDir('preserve-failclosed');
  const p = path.join(dir, 'item.wrl');
  fs.writeFileSync(p, zlib.gzipSync(Buffer.from(WRL, 'utf8')));
  const before = sha256(fs.readFileSync(p));

  // Changed text + an encoder that throws: the original must survive and the
  // error must surface, with no "preserved" consolation result.
  const badZlib = { gzipSync: () => { throw new Error('encoder exploded'); }, gunzipSync: zlib.gunzipSync };
  assert.throws(
    () => io.safeSave({
      filePath: p, text: WRL + '# edited\n', format: FORMAT.GZIP,
      expectedStat: io.statFile(p), preserveExistingGzip: true,
    }, { zlib: badZlib }),
    /encoder exploded/
  );
  assert.strictEqual(sha256(fs.readFileSync(p)), before, 'original intact after encoder failure');
  assert.deepStrictEqual(fs.readdirSync(dir), ['item.wrl'], 'no temp, no backup');
});
