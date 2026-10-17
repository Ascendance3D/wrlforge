'use strict';
// Lane A: Mall artifact size truth.
//
// The bug these tests lock out: WRLForge recompressed the decompressed text with
// Node zlib level 9 and presented the result as the size of the file that would
// be uploaded. For a Zopfli-packed shipping artifact those numbers differ by
// thousands of bytes, and the prediction can fail an item that actually fits.
//
// The contract proved below: the artifact on disk is MEASURED, the repack size
// is a separate PREDICTION, and a hard PASS/FAIL exists only when a measured
// artifact has been proven to hold the exact text being validated.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const zlib = require('node:zlib');

const {
  validate,
  predictedRepackSize,
  resolveSizeState,
  MALL_UPLOAD_MAX_BYTES,
} = require('../validator');
const { measureArtifact, mallPayload } = require('../src/mall/artifact-size');

const FX = path.join(__dirname, 'fixtures', 'gzip-encodings');
const PLAIN = path.join(FX, 'twin.plain.wrl');
const SMALL_GZ = path.join(FX, 'twin-small.wrl.gz');
const LARGE_GZ = path.join(FX, 'twin-large.wrl.gz');

// The limit-straddling pair. The original twins prove ENCODING VARIANCE (same
// text, very different legal gzip sizes) but both now sit under the corrected
// 81,920 B ceiling, so they can no longer prove the LIMIT STRADDLE. That proof
// moved to this second committed pair.
const LIMIT_PLAIN = path.join(FX, 'limit-twin.plain.wrl');
const LIMIT_SMALL_GZ = path.join(FX, 'limit-twin-small.wrl.gz');
const LIMIT_LARGE_GZ = path.join(FX, 'limit-twin-large.wrl.gz');

const twinText = () => fs.readFileSync(PLAIN, 'utf8');
const limitTwinText = () => fs.readFileSync(LIMIT_PLAIN, 'utf8');
const sizeCtx = (over = {}) => ({
  artifactBytes: 1000, artifactIsGzip: true, artifactMatchesText: true, ...over,
});

const VALID = [
  '#VRML V2.0 utf8',
  'WorldInfo { title "t" }',
  'Transform { children [ Shape { geometry Box { size 1 1 1 } } ] }',
  '',
].join('\n');

const sizeRow = (r) => r.results.find((c) => c.name.startsWith('Upload size'));

// ---------------------------------------------------------------------------
// The authoritative limit
// ---------------------------------------------------------------------------

test('the Mall upload limit is 80 KiB -- 80 * 1024 = 81,920 bytes, not decimal 80,000', () => {
  assert.equal(MALL_UPLOAD_MAX_BYTES, 80 * 1024);
  assert.equal(MALL_UPLOAD_MAX_BYTES, 81920);
  assert.notEqual(MALL_UPLOAD_MAX_BYTES, 80000);
  // 81,290 was a transposed typo that was briefly promoted into the contract.
  assert.notEqual(MALL_UPLOAD_MAX_BYTES, 81290);
});

test('the limit is inclusive: exactly 81,920 B passes, 81,921 B fails', () => {
  const at = validate(VALID, sizeCtx({ artifactBytes: 81920 }));
  assert.equal(at.sizeStatus, 'pass');
  assert.equal(at.ok, true);
  assert.equal(at.mallReady, true);

  const over = validate(VALID, sizeCtx({ artifactBytes: 81921 }));
  assert.equal(over.sizeStatus, 'fail');
  assert.equal(over.ok, false, 'a measured over-limit artifact is a HARD failure');
  assert.equal(over.mallReady, false);
});

test('no MAX_GZIP constant survives, and the gate reads the one named limit', () => {
  const validatorSrc = fs.readFileSync(path.join(__dirname, '..', 'validator.js'), 'utf8');
  const rendererSrc = fs.readFileSync(path.join(__dirname, '..', 'renderer', 'renderer.js'), 'utf8');

  // `MAX_GZIP_BYTES` and the renderer's duplicate `MAX_GZIP` were two separate
  // competing limits. Both must be gone, not merely corrected in one place --
  // the point is ONE named constant, whatever its value.
  assert.equal(/\bMAX_GZIP(_BYTES)?\b/.test(validatorSrc), false);
  assert.equal(/\bMAX_GZIP(_BYTES)?\b/.test(rendererSrc), false);

  // Executable code (comments stripped) must compare against the named constant.
  const code = validatorSrc.replace(/\/\/[^\n]*/g, '');
  assert.match(code, /bytes <= MALL_UPLOAD_MAX_BYTES/);
  assert.equal(/\b(81290|80000)\b/.test(code), false, 'no competing Mall limit literal');

  // The renderer must not hard-code the limit at all -- it reads it from the payload.
  assert.equal(/\b(81920|81290|80000)\b/.test(rendererSrc), false,
    'the renderer must take the limit from mallUploadMaxBytes, not duplicate it');
});

// ---------------------------------------------------------------------------
// Three separate numbers
// ---------------------------------------------------------------------------

test('textBytes, artifactBytes and predictedRepackBytes are three distinct facts', () => {
  const text = twinText();
  const artifactBytes = fs.statSync(SMALL_GZ).size;
  const r = validate(text, { artifactBytes, artifactIsGzip: true, artifactMatchesText: true });

  assert.equal(r.textBytes, Buffer.byteLength(text, 'utf8'));
  assert.equal(r.artifactBytes, artifactBytes);
  assert.equal(r.predictedRepackBytes, predictedRepackSize(text));

  // All three genuinely differ here, so a collapsed field would be caught.
  assert.notEqual(r.textBytes, r.artifactBytes);
  assert.notEqual(r.textBytes, r.predictedRepackBytes);
});

test('no ambiguous gzipBytes / rawBytes field survives in the validator contract', () => {
  const r = validate(VALID, sizeCtx());
  assert.equal(Object.hasOwn(r, 'gzipBytes'), false, 'gzipBytes meant two things and must be gone');
  assert.equal(Object.hasOwn(r, 'rawBytes'), false, 'rawBytes was text bytes and is now textBytes');
  assert.equal(Object.hasOwn(r, 'textBytes'), true);
  assert.equal(Object.hasOwn(r, 'artifactBytes'), true);
  assert.equal(Object.hasOwn(r, 'predictedRepackBytes'), true);
});

test('predictedRepackSize is the zlib level-9 re-encode, and only advisory', () => {
  const text = twinText();
  assert.equal(predictedRepackSize(text), zlib.gzipSync(Buffer.from(text, 'utf8'), { level: 9 }).length);
});

// ---------------------------------------------------------------------------
// Same text, different legal gzip artifacts (the core proof)
// ---------------------------------------------------------------------------

test('the committed twins decompress to identical bytes at different artifact sizes', () => {
  const plain = fs.readFileSync(PLAIN);
  const small = fs.readFileSync(SMALL_GZ);
  const large = fs.readFileSync(LARGE_GZ);

  assert.equal(Buffer.compare(zlib.gunzipSync(small), plain), 0);
  assert.equal(Buffer.compare(zlib.gunzipSync(large), plain), 0);
  assert.notEqual(small.length, large.length,
    'same text must be able to produce different legal gzip sizes');
});

// (A) ENCODING VARIANCE. The original twins are 451 B and 81,781 B of the same
// text. Under the corrected 80 KiB ceiling BOTH pass -- so this pair proves the
// variance, and nothing about the limit. It is not an over-limit artifact.
test('the original twins are both UNDER the corrected limit -- variance, not a straddle', () => {
  const text = twinText();
  const small = validate(text, measureArtifact(SMALL_GZ, text));
  const large = validate(text, measureArtifact(LARGE_GZ, text));

  assert.equal(small.artifactBytes, 451);
  assert.equal(large.artifactBytes, 81781);
  assert.ok(small.artifactBytes <= MALL_UPLOAD_MAX_BYTES);
  assert.ok(large.artifactBytes <= MALL_UPLOAD_MAX_BYTES,
    '81,781 B is under 81,920 B and must not be treated as over-limit');
  assert.equal(small.sizeStatus, 'pass');
  assert.equal(large.sizeStatus, 'pass');

  // A ~181x size difference from identical text is the point of the pair.
  assert.equal(small.textBytes, large.textBytes);
  assert.equal(small.predictedRepackBytes, large.predictedRepackBytes);
  assert.notEqual(small.artifactBytes, large.artifactBytes);
  assert.equal(small.sizeAuthority, 'measured');
  assert.equal(large.sizeAuthority, 'measured');
});

// (B) LIMIT STRADDLE. Same text, two legal gzip encodings, one on each side of
// the real 81,920 B ceiling.
test('two legal encodings of the SAME text straddle the limit: small PASSes, large FAILs', () => {
  const text = limitTwinText();
  const small = validate(text, measureArtifact(LIMIT_SMALL_GZ, text));
  const large = validate(text, measureArtifact(LIMIT_LARGE_GZ, text));

  assert.equal(small.sizeStatus, 'pass');
  assert.equal(large.sizeStatus, 'fail');
  assert.ok(small.artifactBytes <= MALL_UPLOAD_MAX_BYTES);
  assert.ok(large.artifactBytes > MALL_UPLOAD_MAX_BYTES);

  // Identical text => identical text-derived numbers. ONLY the measurement moved.
  assert.equal(small.textBytes, large.textBytes);
  assert.equal(small.predictedRepackBytes, large.predictedRepackBytes);
  assert.equal(small.sizeAuthority, 'measured');
  assert.equal(large.sizeAuthority, 'measured');
});

test('the prediction cannot override the measurement in either direction', () => {
  const text = limitTwinText();
  const large = validate(text, measureArtifact(LIMIT_LARGE_GZ, text));
  // The predicted repack is tiny and would "pass"; the real artifact does not.
  assert.ok(large.predictedRepackBytes <= MALL_UPLOAD_MAX_BYTES,
    'precondition: the prediction alone would have passed');
  assert.equal(large.sizeStatus, 'fail', 'the measured artifact decides, not the prediction');
  assert.equal(large.ok, false);
});

// ---------------------------------------------------------------------------
// State matrix
// ---------------------------------------------------------------------------

test('state 1 -- current gzip artifact: measured authority and a hard verdict', () => {
  const text = twinText();
  const r = validate(text, measureArtifact(SMALL_GZ, text));
  assert.deepEqual(
    { a: r.artifactMatchesText, b: r.sizeAuthority, c: r.sizeStatus, d: r.sizeReason },
    { a: true, b: 'measured', c: 'pass', d: 'measured' }
  );
  assert.equal(sizeRow(r).severity, 'hard');
  assert.equal(sizeRow(r).status, 'pass');
});

test('state 2 -- stale artifact: status stale, no authority, no hard verdict', () => {
  const edited = twinText() + '# an edit that has never been packed\n';
  const r = validate(edited, measureArtifact(SMALL_GZ, edited));

  assert.equal(r.artifactMatchesText, false);
  assert.equal(r.sizeStatus, 'stale');
  assert.equal(r.sizeAuthority, 'none');
  assert.equal(r.mallReady, false);
  // The stale file's byte count is preserved as a fact...
  assert.equal(r.artifactBytes, fs.statSync(SMALL_GZ).size);
  // ...but must not drive the verdict for the edited buffer.
  assert.equal(sizeRow(r).severity, 'info');
  assert.equal(sizeRow(r).pass, null, 'never a PASS and never a hard FAIL while stale');
  assert.equal(sizeRow(r).status, 'stale');
});

test('state 3 -- plain source, no gzip artifact: status unknown, nothing invented', () => {
  const text = twinText();
  const ctx = measureArtifact(PLAIN, text);
  assert.deepEqual(ctx, { artifactBytes: null, artifactIsGzip: false, artifactMatchesText: null });

  const r = validate(text, ctx);
  assert.equal(r.artifactBytes, null);
  assert.equal(r.artifactMatchesText, null);
  assert.equal(r.sizeStatus, 'unknown');
  assert.equal(r.sizeReason, 'no-gzip-artifact');
  assert.equal(r.mallReady, false);
  assert.equal(sizeRow(r).severity, 'info');
  assert.equal(sizeRow(r).pass, null);
});

test('state 4 -- freshly written artifact is measured, not predicted', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrlforge-size-'));
  try {
    const out = path.join(dir, 'item.wrl');
    const text = twinText();
    // Simulate a repack that writes a DIFFERENT encoding than level 9, so the
    // prediction and the written file cannot accidentally agree.
    const written = zlib.gzipSync(Buffer.from(text, 'utf8'), { level: 1 });
    fs.writeFileSync(out, written);

    const r = validate(text, measureArtifact(out, text));
    assert.equal(r.artifactBytes, written.length);
    assert.equal(r.artifactBytes, fs.statSync(out).size, 'must be the file that exists');
    assert.notEqual(r.artifactBytes, r.predictedRepackBytes, 'the pre-write prediction is not the result');
    assert.equal(r.artifactMatchesText, true);
    assert.equal(r.sizeAuthority, 'measured');
    assert.equal(r.sizeStatus, 'pass');
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('no artifact at all (no path / missing file) degrades to unknown, never to 0', () => {
  for (const p of [null, path.join(os.tmpdir(), 'wrlforge-does-not-exist-12345.wrl')]) {
    const ctx = measureArtifact(p, VALID);
    assert.deepEqual(ctx, { artifactBytes: null, artifactIsGzip: null, artifactMatchesText: null });
    assert.equal(validate(VALID, ctx).sizeStatus, 'unknown');
  }
});

test('validate() with NO size context at all is unknown, not a silent pass', () => {
  const r = validate(VALID);
  assert.equal(r.sizeStatus, 'unknown');
  assert.equal(r.sizeAuthority, 'none');
  assert.equal(r.artifactBytes, null);
  assert.equal(r.mallReady, false);
  assert.equal(r.ok, true, 'structural rules are unaffected by an unverified size');
});

test('a gzip artifact whose identity was never proven is unknown, not measured', () => {
  const r = resolveSizeState({ artifactBytes: 500, artifactIsGzip: true });
  assert.equal(r.sizeStatus, 'unknown');
  assert.equal(r.sizeAuthority, 'none');
  assert.equal(r.sizeReason, 'unverified-artifact');
  assert.equal(r.artifactBytes, 500, 'the measurement is still reported as a fact');
});

test('a corrupt gzip artifact reports real bytes with unprovable identity', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrlforge-corrupt-'));
  try {
    const out = path.join(dir, 'broken.wrl');
    const truncated = zlib.gzipSync(Buffer.from(VALID, 'utf8')).subarray(0, 12);
    fs.writeFileSync(out, truncated);
    const ctx = measureArtifact(out, VALID);
    assert.equal(ctx.artifactBytes, truncated.length);
    assert.equal(ctx.artifactIsGzip, true);
    assert.equal(ctx.artifactMatchesText, null);
    assert.equal(validate(VALID, ctx).sizeStatus, 'unknown');
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('gzip bomb that exceeds max decompression bounds safely degrades to unknown without unbounded allocation', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrlforge-bomb-'));
  try {
    const out = path.join(dir, 'bomb.wrl');
    // A small string that easily clears the artificially low injected cap
    const bombContent = Buffer.alloc(2048, 0x41);
    const bombBytes = zlib.gzipSync(bombContent);
    fs.writeFileSync(out, bombBytes);

    // Attempt measurement with a legitimate document text (bomb decompresses to an unexpected large content).
    const ctx = measureArtifact(out, VALID, { getGzipCap: () => 1024 });

    // measureArtifact catches the ERR_BUFFER_TOO_LARGE from zlib and degrades to unknown/unprovable identity
    assert.equal(ctx.artifactBytes, bombBytes.length);
    assert.equal(ctx.artifactIsGzip, true);
    assert.equal(ctx.artifactMatchesText, null);
    assert.equal(validate(VALID, ctx).sizeStatus, 'unknown');
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

// ---------------------------------------------------------------------------
// ok / mallReady separation
// ---------------------------------------------------------------------------

test('structural validity is not Mall readiness', () => {
  const r = validate(VALID); // structurally fine, nothing packed
  assert.equal(r.ok, true);
  assert.equal(r.mallReady, false, 'an unverified size can never read as upload-ready');
});

test('a structural hard failure keeps mallReady false even with a passing measured size', () => {
  const bad = VALID.replace('#VRML V2.0 utf8', '#VRML V1.0 ascii');
  const r = validate(bad, sizeCtx({ artifactBytes: 100 }));
  assert.equal(r.sizeStatus, 'pass');
  assert.equal(r.ok, false);
  assert.equal(r.mallReady, false);
});

// ---------------------------------------------------------------------------
// The clobber (§17): measured bytes must survive payload assembly
// ---------------------------------------------------------------------------

test('mallPayload refuses to let validator output overwrite a measured file fact', () => {
  const validation = validate(VALID, sizeCtx());
  assert.throws(
    () => mallPayload({ artifactBytes: 72820 }, validation),
    /would overwrite measured file facts: artifactBytes/,
    'the exact shape of the original defect must now throw instead of silently winning'
  );
});

test('mallPayload preserves non-colliding measured facts alongside the validation', () => {
  const validation = validate(VALID, sizeCtx({ artifactBytes: 72820 }));
  const payload = mallPayload({ mallPath: '/x/item.wrl', sourceFileBytes: 72820 }, validation);
  assert.equal(payload.sourceFileBytes, 72820);
  assert.equal(payload.artifactBytes, 72820, 'the measured artifact size must survive the open flow');
  assert.notEqual(payload.artifactBytes, payload.textBytes);
});

test('an open-flow payload never reports text bytes as the artifact size', () => {
  // Reproduces the real Ragnum shape: a 72,820 B gzip holding 335,924 B of text.
  const text = 'x'.repeat(335924);
  const payload = mallPayload(
    { mallPath: '/x/ragnum-red.wrl', sourceFileBytes: 72820 },
    validate(text, { artifactBytes: 72820, artifactIsGzip: true, artifactMatchesText: true })
  );
  assert.equal(payload.artifactBytes, 72820);
  assert.equal(payload.textBytes, 335924);
  assert.equal(payload.sizeStatus, 'pass');
});

// ---------------------------------------------------------------------------
// main.js wiring (source contract)
// ---------------------------------------------------------------------------

test('main.js measures the artifact on every Mall size path and never validates text alone', () => {
  const src = fs.readFileSync(path.join(__dirname, '..', 'main.js'), 'utf8');
  const calls = [...src.matchAll(/\bvalidate\(([^)]*)\)/g)].map((m) => m[1]);
  assert.ok(calls.length >= 2, 'expected the open and check size paths');
  for (const args of calls) {
    assert.ok(/,/.test(args),
      `validate() must be given a size context, got validate(${args})`);
  }
});

// Lane B B2 moved the repack write out of main.js and into src/mall/repack.js,
// so this invariant moved with it. The rule is unchanged and is if anything
// stronger now: repack measures the REAL file, and it does so after the write
// rather than predicting beforehand. It must also never let the pre-write
// candidate count stand in for the measurement.
test('the Mall repack path measures the real artifact, never a pre-write prediction', () => {
  const src = fs.readFileSync(path.join(__dirname, '..', 'src', 'mall', 'repack.js'), 'utf8');
  const code = src.split('\n').filter((l) => !/^\s*\/\//.test(l)).join('\n');

  // Every validate() in the repack path is handed a measured size context.
  const calls = [...code.matchAll(/\bcheck\(([^)]*)\)/g)].map((m) => m[1]);
  assert.ok(calls.length >= 2, 'expected the success and refusal size paths');
  for (const args of calls) {
    assert.match(args, /measure\(/,
      `the repack verdict must come from a measured artifact, got check(${args})`);
  }

  // main.js hands the write to the helper rather than doing it itself.
  const mainSrc = fs.readFileSync(path.join(__dirname, '..', 'main.js'), 'utf8');
  const mainCode = mainSrc.split('\n').filter((l) => !/^\s*\/\//.test(l)).join('\n');
  assert.match(mainCode, /mall:repack[\s\S]{0,600}?repackMall\(/,
    'the repack handler routes through repackMall');
  assert.ok(!mainCode.includes('fs.writeFileSync(mallPath'),
    'main.js no longer writes the Mall artifact directly');
});
