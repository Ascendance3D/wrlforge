// SPDX-License-Identifier: GPL-3.0-or-later
// RUST-1A Task A: the UTF-16 gate under a hostile or broken
// String.prototype.isWellFormed. Exercises the facade AND every raw export that
// receives a JavaScript string.
//
// Required result: ZERO silent substitutions. Every call either refuses with a
// code, or returns text whose UTF-16 code units equal the well-formed input.
//
// The expected truth is computed by `wellFormed()` below, an independent
// code-unit scan that never calls String.prototype.isWellFormed (the method
// under attack).
//
//   node spikes/rust-1-wasm-boundary/build.mjs
//   node --test spikes/rust-1-wasm-boundary/test/*.test.mjs
//   RUST1A_PKG=spikes/rust-1-wasm-boundary/out/neg/lossy-utf16/pkg node --test <this file>   (must FAIL)
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadGlue } from '../loaders/node.mjs';
import { createTextEngine } from '../../../crates/wrlforge-wasm/js/wrlforge-text.mjs';

// RUST1A_PKG points the sweep at another artifact (a negative control) to prove
// the test detects it; the default is the release artifact.
const PKG = process.env.RUST1A_PKG || join(dirname(fileURLToPath(import.meta.url)), '..', 'out', 'pkg');
const glue = await loadGlue(PKG);
const engine = createTextEngine(glue);

const HIGH = '\uD83D';
const LOW = '\uDE00';
const FFFD = '�';
const EMPTY = new Float64Array(0);

// Independent oracle: well-formed UTF-16 by code-unit scan.
function wellFormed(s) {
  for (let i = 0; i < s.length; i += 1) {
    const c = s.charCodeAt(i);
    if (c >= 0xd800 && c <= 0xdbff) {
      const d = i + 1 < s.length ? s.charCodeAt(i + 1) : -1;
      if (!(d >= 0xdc00 && d <= 0xdfff)) return false;
      i += 1;
    } else if (c >= 0xdc00 && c <= 0xdfff) {
      return false;
    }
  }
  return true;
}
const units = (s) => Array.from({ length: s.length }, (_, i) => s.charCodeAt(i));

const NATIVE = Object.getOwnPropertyDescriptor(String.prototype, 'isWellFormed');
// Run `fn` with String.prototype.isWellFormed replaced (or deleted when
// `impl` is null). Always restores the native method.
function withMethod(impl, fn) {
  if (impl === null) delete String.prototype.isWellFormed;
  else Object.defineProperty(String.prototype, 'isWellFormed', { ...NATIVE, value: impl });
  try { return fn(); } finally { Object.defineProperty(String.prototype, 'isWellFormed', NATIVE); }
}

const MODES = {
  native: NATIVE.value,
  missing: null,
  alwaysTrue() { return true; },
  alwaysFalse() { return false; },
  throws() { throw new TypeError('hostile isWellFormed'); },
  returnsOne() { return 1; },
  returnsTwo() { return 2; },
  returnsString() { return 'yes'; },
  returnsObject() { return {}; },
  returnsUndefined() { return undefined; },
  inverted() { return !NATIVE.value.call(this); },
};

const ALLOWED_REFUSALS = new Set(['EENCODING', 'EENGINE']);

const MALFORMED = [
  `a${HIGH}b`, `a${LOW}b`, `ab${HIGH}`, `${LOW}ab`, `${LOW}${HIGH}`,
  // Task A case 9: a genuine U+FFFD next to an unpaired surrogate.
  `${FFFD}${HIGH}`, `${HIGH}${FFFD}`, `x${FFFD}y${LOW}z${FFFD}`,
];
const VALID = [
  '', 'ascii', 'café €', `real ${FFFD} stays`, FFFD,
  // Task A case 10: astral characters and surrogate pairs.
  '\u{1F600}', '\u{1F600}\u{10FFFF}\u{10000}', `DEF \u{1F600}x Group {}${FFFD}`,
  '﻿#VRML V2.0 utf8\r\n', 'a\rb\r\nc\n',
];

// Every entry point that receives a JS string. Each returns the strings the
// engine handed back (to compare against the input), or throws.
function entryPoints(text) {
  const ins = [text];
  return {
    'facade.checkText': () => { engine.checkText(text); return []; },
    'facade.openSession': () => { const s = engine.openSession(text); try { return [s.current().text()]; } finally { s.dispose(); } },
    'facade.update': () => {
      const s = engine.openSession('base');
      try { const n = s.update(s.current(), text); return [n.text()]; } finally { s.dispose(); }
    },
    'facade.applyEdits(text)': () => [engine.applyEdits(text, [])],
    'facade.applyEdits(insert)': () => [engine.applyEdits('', [{ from: 0, to: 0, insert: text }])],
    'facade.mapOffset(insert)': () => { engine.mapOffset(0, [{ from: 0, to: 0, insert: text }]); return []; },
    'facade.mapRange(insert)': () => { engine.mapRange({ from: 0, to: 0 }, [{ from: 0, to: 0, insert: text }]); return []; },
    'facade.propose(insert)': () => {
      const s = engine.openSession('');
      try { return [s.propose(s.current(), [{ from: 0, to: 0, insert: text }])]; } finally { s.dispose(); }
    },
    'facade.verifyTransaction(after)': () => {
      const s = engine.openSession('');
      // A true transaction: '' + insert(text) == text. Checks insert AND after.
      try { s.verifyTransaction(s.current(), [{ from: 0, to: 0, insert: text }], text); return []; } finally { s.dispose(); }
    },
    'raw.check_text': () => { glue.check_text(text); return []; },
    'raw.apply_edits(text)': () => [glue.apply_edits(text, EMPTY, EMPTY, [])],
    'raw.apply_edits(insert)': () => [glue.apply_edits('', new Float64Array([0]), new Float64Array([0]), ins)],
    'raw.map_offset(insert)': () => { glue.map_offset(0, new Float64Array([0]), new Float64Array([0]), ins, false); return []; },
    'raw.map_range(insert)': () => { glue.map_range(0, 0, new Float64Array([0]), new Float64Array([0]), ins, false, true); return []; },
    'raw.RawSession(text)': () => {
      const r = new glue.RawSession(text);
      try { return [r.text(r.serial(), r.revision())]; } finally { r.free(); }
    },
    'raw.replace(text)': () => {
      const r = new glue.RawSession('base');
      try { const rev = r.replace(r.serial(), r.revision(), text); return [r.text(r.serial(), rev)]; } finally { r.free(); }
    },
    'raw.propose(insert)': () => {
      const r = new glue.RawSession('');
      try { return [r.propose(r.serial(), r.revision(), new Float64Array([0]), new Float64Array([0]), ins)]; } finally { r.free(); }
    },
    'raw.verify(after)': () => {
      const r = new glue.RawSession('');
      try { r.verify(r.serial(), r.revision(), new Float64Array([0]), new Float64Array([0]), ins, text); return []; } finally { r.free(); }
    },
  };
}

// Outcome of one call: a refusal code, or the returned strings.
function attempt(fn) {
  try { return { out: fn() }; } catch (e) { return { code: e && e.code, error: e }; }
}

// The safety property, independent of the method's behaviour:
//   malformed input  -> refused with a code (never accepted, never replaced)
//   well-formed input -> accepted with exact units, or refused with a code
// It records every violation; the test asserts there are none.
function sweep(modeName, impl) {
  const violations = [];
  let calls = 0;
  for (const text of [...MALFORMED, ...VALID]) {
    const ok = wellFormed(text);
    for (const [where, fn] of Object.entries(entryPoints(text))) {
      // Sessions are opened with valid base text BEFORE the method changes, so
      // case 8 (replacement after valid text was accepted) is also covered.
      const r = withMethod(impl, () => attempt(fn));
      calls += 1;
      if ('out' in r) {
        // Every returned string is the input itself (empty base, one insert).
        if (!ok) violations.push({ modeName, where, text: units(text), accepted: r.out.map(units) });
        else for (const o of r.out) if (units(o).join() !== units(text).join()) violations.push({ modeName, where, text: units(text), got: units(o) });
      } else if (!ALLOWED_REFUSALS.has(r.code)) {
        violations.push({ modeName, where, text: units(text), uncoded: String(r.error) });
      }
    }
  }
  return { violations, calls };
}

for (const [modeName, impl] of Object.entries(MODES)) {
  test(`zero silent substitution with isWellFormed = ${modeName}`, () => {
    const { violations, calls } = sweep(modeName, impl);
    assert.ok(calls > 0);
    assert.deepEqual(violations.slice(0, 5), [], `${violations.length} violation(s) of ${calls} calls`);
  });
}

test('native method: malformed refused as EENCODING with the first unpaired unit; valid accepted exactly', () => {
  for (const text of MALFORMED) {
    for (const [where, fn] of Object.entries(entryPoints(text))) {
      const r = attempt(fn);
      assert.equal(r.code, 'EENCODING', `${where} ${units(text)}`);
    }
  }
  for (const text of VALID) {
    for (const [where, fn] of Object.entries(entryPoints(text))) {
      const r = attempt(fn);
      assert.ok('out' in r, `${where} ${units(text)}: ${r.code}`);
    }
  }
});

test('case 2: method missing before engine creation -> engine refuses to start', () => {
  withMethod(null, () => assert.equal(attempt(() => createTextEngine(glue)).code, 'EENGINE'));
});

test('case 3: method deleted after engine creation -> fail closed, never converted', () => {
  const s = engine.openSession('kept');
  withMethod(null, () => {
    assert.ok(ALLOWED_REFUSALS.has(attempt(() => s.update(s.current(), `x${HIGH}`)).code));
    assert.ok(ALLOWED_REFUSALS.has(attempt(() => glue.check_text(`x${LOW}`)).code));
  });
  assert.equal(s.current().text(), 'kept');
  s.dispose();
});

test('case 8: method replaced after valid text was accepted -> session text never replaced', () => {
  const s = engine.openSession('DEF a Shape {}');
  const before = s.current();
  for (const impl of [MODES.alwaysTrue, MODES.returnsOne, MODES.returnsString]) {
    const r = withMethod(impl, () => attempt(() => s.update(s.current(), `DEF ${HIGH} Shape {}`)));
    assert.ok(ALLOWED_REFUSALS.has(r.code), `update accepted malformed text under ${impl.name}`);
  }
  assert.equal(s.current().revision, before.revision, 'no new revision was issued');
  assert.deepEqual(units(s.current().text()), units('DEF a Shape {}'));
  s.dispose();
});

test('case 5: a method that always says false refuses valid text with EENGINE, not a false EENCODING', () => {
  withMethod(MODES.alwaysFalse, () => {
    for (const text of ['abc', `real ${FFFD}`, '\u{1F600}']) {
      const r = attempt(() => engine.checkText(text));
      assert.equal(r.code, 'EENGINE', `valid ${units(text)} must not be reported as malformed`);
    }
  });
});
