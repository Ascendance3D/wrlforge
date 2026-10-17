'use strict';
// Tests the main-process WRL source loader: gzip transparency (plain and
// gzipped inputs yield the same text), clear errors on corrupt gzip, and
// non-mutation of the source file. Proving the text "parses in X_ITE" is done
// end-to-end in the Electron fixture runs (see qa/phase-2b0-extrusion-loading);
// node:test has no X_ITE, so it verifies everything up to that boundary.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { readWrlSource } = require('../../src/preview/wrl-source');

const FX = path.join(__dirname, '..', 'fixtures', 'preview');
const md5 = (p) => crypto.createHash('md5').update(fs.readFileSync(p)).digest('hex');

test('plain WRL is returned as-is, flagged not gzipped', () => {
  const { text, wasGzipped } = readWrlSource(path.join(FX, 'gz-plain-twin.wrl'));
  assert.equal(wasGzipped, false);
  assert.match(text, /Box \{ size 3 4 5 \}/);
});

test('gzipped WRL decompresses to text identical to its plain twin', () => {
  const plain = readWrlSource(path.join(FX, 'gz-plain-twin.wrl'));
  const gz = readWrlSource(path.join(FX, 'gz-gzipped-twin.wrl'));
  assert.equal(gz.wasGzipped, true);
  assert.equal(plain.wasGzipped, false);
  assert.equal(gz.text, plain.text, 'decompressed gzip text must match the plain twin exactly');
});

test('corrupt gzip (valid magic bytes, broken body) throws a clear, prefixed error', () => {
  assert.throws(
    () => readWrlSource(path.join(FX, 'gz-corrupt.wrl')),
    /failed to decompress/i,
  );
});

test('gzip bomb expanding past the allowed limit throws a clear limit error without unbounded allocation', () => {
  const zlib = require('zlib');
  const os = require('os');
  const tmpBomb = path.join(os.tmpdir(), 'wrlforge-test-bomb-wrl-source.wrl');
  // Small input that would expand to 64MB if allowed (exceeds ratio cap for small inputs)
  const bombContent = Buffer.alloc(10 * 1024 * 1024, 0x41);
  const bomb = zlib.gzipSync(bombContent);
  fs.writeFileSync(tmpBomb, bomb);

  try {
    assert.throws(
      () => readWrlSource(tmpBomb),
      /exceeds the allowed decompression limit/i,
    );
  } finally {
    fs.unlinkSync(tmpBomb);
  }
});

test('reading a source file does not modify it', () => {
  const p = path.join(FX, 'gz-gzipped-twin.wrl');
  const before = md5(p);
  readWrlSource(p);
  readWrlSource(p);
  assert.equal(md5(p), before, 'source file must be byte-identical after reads');
});
