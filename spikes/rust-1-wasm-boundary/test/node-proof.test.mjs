// SPDX-License-Identifier: GPL-3.0-or-later
// RUST-1 Node 24 proof: the release wrlforge-wasm artifact, loaded through the
// facade. Expected values are hand-derived from Unicode encodings and the WD1.2
// contract, not produced by either implementation.
//
//   node spikes/rust-1-wasm-boundary/build.mjs
//   node --test spikes/rust-1-wasm-boundary/test/
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadEngine, loadGlue } from '../loaders/node.mjs';
import { createTextEngine } from '../../../crates/wrlforge-wasm/js/wrlforge-text.mjs';

const PKG = join(dirname(fileURLToPath(import.meta.url)), '..', 'out', 'pkg');
const engine = await loadEngine(PKG);
const glue = await loadGlue(PKG);

const code = (fn) => {
  try { fn(); } catch (e) { return e.code; }
  return 'NO-ERROR';
};
const err = (fn) => {
  try { fn(); } catch (e) { return e; }
  assert.fail('expected a refusal');
};
// Exact UTF-16 identity, unit by unit (stronger than === for diagnostics).
const units = (s) => Array.from({ length: s.length }, (_, i) => s.charCodeAt(i));
const sameText = (a, b) => assert.deepEqual(units(a), units(b));

const HIGH = '\uD83D';
const LOW = '\uDE00';
const SAMPLES = {
  ascii: '#VRML V2.0 utf8\nShape {}\n',
  bmp: 'DEF café Shape { } # €～',
  astral: 'DEF \u{1F600}x Group {}',
  pairs: '\u{1F600}\u{1F600}\u{10FFFF}\u{10000}',
  bom: '﻿#VRML V2.0 utf8\n',
  midBom: 'a﻿b',
  crlf: 'a\r\nb\r\n',
  cr: 'a\rb\r',
  mixed: 'a\nb\r\nc\rd',
  nul: 'a\0b\0',
  replacement: 'real � stays',
  empty: '',
};

test('module initializes, is the release build, and reports no negative controls', () => {
  assert.match(engine.info, /^wrlforge-wasm 0\.0\.0 \(RUST-1; .*negative-controls=\[\]\)$/);
  assert.equal(engine.poisoned, false);
});

test('repeated initialization is idempotent; a fresh instance is independent', async () => {
  const again = glue.initSync({ module: new Uint8Array(0) }); // ignored: already initialized
  // Exports objects have a null prototype; identity is the meaningful check.
  assert.equal(typeof again, 'object');
  assert.equal(glue.initSync({ module: new Uint8Array(0) }), again);
  const e2 = createTextEngine(glue);
  assert.equal(e2.applyEdits('ab', [{ from: 1, to: 1, insert: 'x' }]), 'axb');
  const fresh = await loadEngine(PKG, 'second-instance');
  assert.equal(fresh.applyEdits('ab', [{ from: 0, to: 1, insert: '' }]), 'b');
  // A session from one engine is foreign to every other engine.
  const s = engine.openSession('x');
  const other = e2.openSession('x');
  assert.equal(code(() => other.update(s.current(), 'y')), 'ESESSIONFOREIGN');
  assert.equal(code(() => fresh.openSession('x').propose(s.current(), [])), 'ESESSIONFOREIGN');
});

test('valid text of every class round-trips exactly through a session', () => {
  for (const [name, text] of Object.entries(SAMPLES)) {
    const s = engine.openSession(text);
    const snap = s.current();
    sameText(snap.text(), text);
    assert.equal(snap.length, text.length, name);
    assert.equal(engine.checkText(text), true, name);
    s.dispose();
  }
});

test('a leading BOM survives Rust -> JS (TextDecoder ignoreBOM) and counts as one unit', () => {
  const s = engine.openSession(SAMPLES.bom);
  const t = s.current().text();
  assert.equal(t.charCodeAt(0), 0xfeff);
  assert.equal(s.current().toUtf8(1), 3);
  assert.equal(engine.applyEdits(SAMPLES.bom, []).charCodeAt(0), 0xfeff);
  assert.equal(engine.applyEdits(SAMPLES.bom, [{ from: 1, to: 1, insert: '' }]).charCodeAt(0), 0xfeff);
});

test('unpaired surrogates are refused before conversion, never replaced', () => {
  const cases = [
    ['lone high', `a${HIGH}b`, 1],
    ['lone low', `a${LOW}b`, 1],
    ['high at end', `ab${HIGH}`, 2],
    ['low at start', `${LOW}ab`, 0],
    ['reversed pair', `${LOW}${HIGH}`, 0],
    ['after a real FFFD', `�${HIGH}`, 1],
  ];
  for (const [name, text, unit] of cases) {
    const e = err(() => engine.openSession(text));
    assert.equal(e.code, 'EENCODING', name);
    assert.equal(e.unit, unit, name);
    assert.equal(e.field, 'text', name);
    assert.equal(code(() => engine.checkText(text)), 'EENCODING', name);
    assert.equal(code(() => engine.applyEdits(text, [])), 'EENCODING', name);
  }
  // In an insert: the caller index is reported.
  const e = err(() => engine.applyEdits('ab', [{ from: 0, to: 0, insert: 'ok' }, { from: 2, to: 2, insert: LOW }]));
  assert.deepEqual([e.code, e.index, e.field, e.unit], ['EENCODING', 1, 'insert', 0]);
  assert.equal(code(() => engine.mapOffset(0, [{ from: 0, to: 0, insert: HIGH }])), 'EENCODING');
});

test('a legitimate U+FFFD is ordinary text, not corruption', () => {
  const text = SAMPLES.replacement;
  sameText(engine.applyEdits(text, [{ from: 0, to: 4, insert: '�' }]), '� � stays');
  sameText(engine.openSession('�').current().text(), '�');
});

test('zero silent substitution: no output ever gains a U+FFFD it was not given', () => {
  const pool = ['', 'a', '�', '\u{1F600}', HIGH, LOW, '\r\n', '﻿', '\0'];
  let accepted = 0;
  let refused = 0;
  for (const a of pool) for (const b of pool) for (const c of pool) {
    const text = a + b + c;
    let out;
    try { out = engine.openSession(text).current().text(); } catch (e) {
      assert.equal(e.code, 'EENCODING');
      assert.equal(text.isWellFormed(), false, 'refused only when ill-formed');
      refused += 1;
      continue;
    }
    assert.equal(text.isWellFormed(), true, 'accepted only when well-formed');
    sameText(out, text);
    accepted += 1;
  }
  assert.ok(accepted > 0 && refused > 0);
});

test('offset conversion: valid boundaries round-trip; interiors and overflow are refused', () => {
  const s = engine.openSession('a\u{1F600}é€\r\nb');
  const snap = s.current();
  const expected = [[0, 0], [1, 1], [3, 5], [4, 7], [5, 10], [6, 11], [7, 12], [8, 13]];
  for (const [u, b] of expected) {
    assert.equal(snap.toUtf8(u), b, `utf16 ${u}`);
    assert.equal(snap.fromUtf8(b), u, `utf8 ${b}`);
  }
  assert.equal(code(() => snap.toUtf8(2)), 'EOFFSETSURROGATE');
  assert.equal(code(() => snap.fromUtf8(2)), 'EOFFSETBYTE');
  assert.equal(code(() => snap.toUtf8(9)), 'EOFFSETBOUNDS');
  for (const big of [2 ** 31, 2 ** 32, 2 ** 32 + 1, Number.MAX_SAFE_INTEGER, 2 ** 64, 2 ** 70]) {
    assert.equal(code(() => snap.toUtf8(big)), 'EOFFSETBOUNDS', String(big));
    assert.equal(code(() => snap.fromUtf8(big)), 'EOFFSETBOUNDS', String(big));
  }
  for (const bad of [-1, 1.5, NaN, Infinity, '1', null]) {
    assert.equal(code(() => snap.toUtf8(bad)), 'EARGUMENT', String(bad));
  }
  assert.deepEqual({ ...snap.validateSpan(1, 3) }, { from: 1, to: 3, utf8From: 1, utf8To: 5 });
  assert.equal(code(() => snap.validateSpan(3, 1)), 'ESPANINVERTED');
  assert.equal(code(() => snap.validateSpan(2, 3)), 'EOFFSETSURROGATE');
});

test('line index: LF, CRLF, lone CR and mixed endings; UTF-16 columns', () => {
  const s = engine.openSession(SAMPLES.mixed);
  const snap = s.current();
  assert.deepEqual({ ...snap.lineInfo() }, { lines: 4, lf: 1, crlf: 1, cr: 1, mixed: true });
  assert.deepEqual({ ...snap.lineCol(5) }, { line: 3, column: 1 });
  assert.deepEqual({ ...snap.lineCol(4) }, { line: 2, column: 3 }); // between CR and LF
  assert.equal(snap.offsetAt(4, 2), 8);
  assert.equal(code(() => snap.offsetAt(5, 1)), 'ELINE');
  assert.equal(code(() => snap.offsetAt(1, 3)), 'ECOLUMN');
  assert.equal(code(() => snap.offsetAt(0, 1)), 'ELINE');
  const a = engine.openSession('DEF \u{1F600}x Group').current();
  assert.deepEqual({ ...a.lineCol(8) }, { line: 1, column: 9 });
  assert.equal(code(() => a.lineCol(5)), 'EOFFSETSURROGATE');
  assert.deepEqual({ ...engine.openSession('a\r\nb\r\n').current().lineInfo() }, { lines: 3, lf: 0, crlf: 2, cr: 0, mixed: false });
  const e = engine.openSession('').current();
  assert.deepEqual({ ...e.lineCol(0) }, { line: 1, column: 1 });
  assert.equal(e.length, 0);
});

test('edits: exact output, line endings and comments preserved, conflicts refused', () => {
  const t = '#VRML V2.0 utf8\r\n# keep me\rShape { } \n';
  sameText(engine.applyEdits(t, [{ from: 27, to: 32, insert: 'Group' }]), '#VRML V2.0 utf8\r\n# keep me\rGroup { } \n');
  sameText(engine.applyEdits(t, []), t);
  sameText(engine.applyEdits('', []), '');
  sameText(engine.applyEdits('', [{ from: 0, to: 0, insert: '' }]), '');
  sameText(engine.applyEdits('ab', [{ from: 1, to: 1, insert: '' }]), 'ab');
  const amb = err(() => engine.applyEdits('abc', [{ from: 1, to: 1, insert: 'x' }, { from: 1, to: 1, insert: 'y' }]));
  assert.deepEqual([amb.code, amb.index, amb.otherIndex], ['EEDITAMBIGUOUS', 1, 0]);
  const empty = err(() => engine.applyEdits('abc', [{ from: 1, to: 1, insert: '' }, { from: 1, to: 1, insert: '' }]));
  assert.equal(empty.code, 'EEDITAMBIGUOUS');
  const ov = err(() => engine.applyEdits('abcd', [{ from: 0, to: 2, insert: '' }, { from: 1, to: 3, insert: '' }]));
  assert.deepEqual([ov.code, ov.index, ov.otherIndex], ['EEDITOVERLAP', 1, 0]);
  const b = err(() => engine.applyEdits('a\u{1F600}b', [{ from: 2, to: 2, insert: 'x' }]));
  assert.deepEqual([b.code, b.index], ['EEDITBOUNDARY', 0]);
  assert.equal(code(() => engine.applyEdits('ab', [{ from: 0, to: 2 ** 53, insert: '' }])), 'EEDITBOUNDS');
  assert.equal(code(() => engine.applyEdits('ab', [{ from: 2 ** 60, to: 2 ** 61, insert: '' }])), 'EEDITBOUNDS');
});

test('shape errors match edit.js codes and caller indexes', () => {
  const cases = [
    [() => engine.applyEdits(1, []), 'EEDITSHAPE', undefined],
    [() => engine.applyEdits('a', null), 'EEDITSHAPE', undefined],
    [() => engine.applyEdits('a', [[]]), 'EEDITSHAPE', 0],
    [() => engine.applyEdits('a', [{ from: 0, to: 0, insert: '', x: 1 }]), 'EEDITSHAPE', 0],
    [() => engine.applyEdits('a', [{ from: 0, to: 0, insert: '' }, { from: 1.5, to: 2, insert: '' }]), 'EEDITSHAPE', 1],
    [() => engine.applyEdits('a', [{ from: -1, to: 0, insert: '' }]), 'EEDITSHAPE', 0],
    [() => engine.applyEdits('a', [{ from: NaN, to: 0, insert: '' }]), 'EEDITSHAPE', 0],
    [() => engine.applyEdits('a', [{ from: 0, to: Infinity, insert: '' }]), 'EEDITSHAPE', 0],
    [() => engine.applyEdits('a', [{ from: 1, to: 0, insert: '' }]), 'EEDITSHAPE', 0],
    [() => engine.applyEdits('a', [{ from: 0, to: 0, insert: 1 }]), 'EEDITSHAPE', 0],
    [() => engine.mapOffset(0, [], 'sideways'), 'EEDITAFFINITY', undefined],
    [() => engine.mapRange({ from: 2, to: 1 }, []), 'EEDITRANGE', undefined],
    [() => engine.mapRange({ start: { offset: 1 } }, []), 'EEDITRANGE', undefined],
  ];
  for (const [fn, c, index] of cases) {
    const e = err(fn);
    assert.equal(e.code, c, String(fn));
    assert.equal(e.index ?? undefined, index, String(fn));
  }
});

test('mapOffset / mapRange: exact large values; precision loss is refused', () => {
  const M = Number.MAX_SAFE_INTEGER;
  assert.equal(engine.mapOffset(M, []), M);
  assert.equal(engine.mapOffset(M - 2, [{ from: 0, to: 0, insert: 'ab' }]), M);
  assert.equal(engine.mapOffset(2 ** 40, [{ from: 2 ** 39, to: 2 ** 39 + 5, insert: '' }]), 2 ** 40 - 5);
  assert.equal(code(() => engine.mapOffset(M - 1, [{ from: 0, to: 0, insert: 'ab' }])), 'EOFFSETPRECISION');
  assert.equal(code(() => engine.mapOffset(2 ** 53, [])), 'EOFFSETPRECISION');
  assert.equal(code(() => engine.mapOffset(0, [{ from: 2 ** 60, to: 2 ** 60, insert: '' }])), 'EOFFSETPRECISION');
  assert.equal(engine.mapOffset(2, [{ from: 2, to: 2, insert: '\u{1F600}' }], 'after'), 4);
  assert.deepEqual({ ...engine.mapRange({ from: 2, to: 2 }, [{ from: 2, to: 2, insert: 'xy' }]) }, { from: 2, to: 4 });
  assert.deepEqual({ ...engine.mapRange({ start: { offset: 0 }, end: { offset: 3 } }, [{ from: 1, to: 2, insert: '' }]) }, { from: 0, to: 2 });
  assert.equal(code(() => engine.mapRange({ from: 1, to: 2 }, [{ from: 1, to: 3, insert: 'Q' }],
    { startAffinity: 'after', endAffinity: 'before' })), 'EEDITINVERTED');
});

test('sessions: revision-bound snapshots; stale, foreign, forged and disposed fail closed', () => {
  const a = engine.openSession('one');
  const b = engine.openSession('one');
  const a0 = a.current();
  assert.equal(a.propose(a0, [{ from: 0, to: 3, insert: 'two' }]), 'two');
  assert.equal(a0.text(), 'one', 'a proposal changes nothing');
  const a1 = a.update(a0, 'two');
  assert.ok(a1.revision > a0.revision);
  assert.equal(a1.text(), 'two');
  // stale
  for (const fn of [() => a0.text(), () => a0.length, () => a0.lineCol(0), () => a.update(a0, 'x'),
    () => a.propose(a0, []), () => a.verifyTransaction(a0, [], 'one')]) {
    assert.equal(code(fn), 'ESESSIONSTALE');
  }
  assert.equal(a0.revision < a1.revision, true, 'a stale snapshot still names its revision');
  // foreign: another session's snapshot, a forged object, a prototype clone
  assert.equal(code(() => a.propose(b.current(), [])), 'ESESSIONFOREIGN');
  assert.equal(code(() => a.update(b.current(), 'x')), 'ESESSIONFOREIGN');
  const forged = { revision: a1.revision };
  assert.equal(code(() => a.update(forged, 'x')), 'ESESSIONFOREIGN');
  const clone = Object.create(Object.getPrototypeOf(a1));
  assert.equal(code(() => clone.text()), 'ESESSIONFOREIGN');
  assert.equal(code(() => Object.getPrototypeOf(a).current.call({})), 'ESESSIONFOREIGN');
  assert.equal(code(() => new (Object.getPrototypeOf(a1).constructor)()), 'EARGUMENT');
  // nothing internal is reachable from the facade objects
  assert.deepEqual(Reflect.ownKeys(a), []);
  assert.deepEqual(Reflect.ownKeys(a1), []);
  assert.equal(JSON.stringify(a1), '{}');
  assert.ok(Object.isFrozen(a) && Object.isFrozen(a1));
  // invalid text on update leaves the session unchanged
  assert.equal(code(() => a.update(a1, `x${HIGH}`)), 'EENCODING');
  assert.equal(a.current().revision, a1.revision);
  assert.equal(a1.text(), 'two');
  // disposed
  a.dispose();
  a.dispose(); // idempotent
  assert.equal(a.disposed, true);
  for (const fn of [() => a.current(), () => a1.text(), () => a.update(a1, 'x'), () => a.propose(a1, [])]) {
    assert.equal(code(fn), 'ESESSIONDISPOSED');
  }
  assert.equal(b.current().text(), 'one', 'disposing one session does not touch another');
});

test('transaction verification compares full text, never a hash', () => {
  const s = engine.openSession('Shape { }\r\n');
  const base = s.current();
  const edits = [{ from: 0, to: 5, insert: 'Group' }];
  assert.deepEqual({ ...s.verifyTransaction(base, edits, 'Group { }\r\n') }, { verified: true, revision: base.revision, length: 11 });
  // Differs only in the final unit (CRLF vs LF + CR order).
  const e = err(() => s.verifyTransaction(base, edits, 'Group { }\n\r'));
  assert.deepEqual([e.code, e.firstDivergence], ['EVERIFYMISMATCH', 9]);
  const e2 = err(() => s.verifyTransaction(base, edits, 'Group { }\r\nX'));
  assert.deepEqual([e2.code, e2.firstDivergence], ['EVERIFYMISMATCH', 11]);
  assert.equal(code(() => s.verifyTransaction(base, edits, `Group { }\r${LOW}`)), 'EENCODING');
});

test('raw layer re-checks numbers it did not mint (defence in depth)', () => {
  const r = new glue.RawSession('abc');
  const other = new glue.RawSession('abc');
  const serial = r.serial();
  const rev = r.revision();
  assert.equal(r.text(serial, rev), 'abc');
  for (const bad of [NaN, -1, 0.5, 2 ** 53, Infinity, 0, other.revision()]) {
    assert.equal(code(() => r.text(serial, bad)), 'ESESSIONREVISION', String(bad));
  }
  assert.equal(code(() => r.text(other.serial(), rev)), 'ESESSIONFOREIGN');
  const rev2 = r.replace(serial, rev, 'abcd');
  assert.equal(code(() => r.text(serial, rev)), 'ESESSIONSTALE');
  assert.equal(r.text(serial, rev2), 'abcd');
  r.dispose();
  assert.equal(code(() => r.text(serial, rev2)), 'ESESSIONDISPOSED');
  r.free();
  other.free();
});

test('engine fails closed without String.prototype.isWellFormed', () => {
  const saved = Object.getOwnPropertyDescriptor(String.prototype, 'isWellFormed');
  delete String.prototype.isWellFormed;
  try {
    assert.equal(code(() => createTextEngine(glue)), 'EENGINE');
    // The raw gate refuses too (it calls the method with `catch`).
    assert.equal(code(() => glue.check_text('abc')), 'EENGINE');
    assert.equal(code(() => glue.apply_edits('abc', new Float64Array(0), new Float64Array(0), [])), 'EENGINE');
  } finally {
    Object.defineProperty(String.prototype, 'isWellFormed', saved);
  }
  assert.equal(engine.checkText('abc'), true);
});

test('an uncoded failure poisons the engine', () => {
  const fake = Object.fromEntries(['RawSession', 'map_offset', 'map_range', 'check_text', 'engine_info']
    .map((n) => [n, () => 'x']));
  fake.apply_edits = () => { throw new WebAssembly.RuntimeError('unreachable'); };
  const e = createTextEngine(fake);
  assert.equal(code(() => e.applyEdits('a', [])), 'EENGINE');
  assert.equal(e.poisoned, true);
  assert.equal(code(() => e.checkText('a')), 'EENGINE');
});

test('missing or uninitialized module is refused', () => {
  assert.equal(code(() => createTextEngine({})), 'EENGINE');
  assert.equal(code(() => createTextEngine(null)), 'EENGINE');
});
