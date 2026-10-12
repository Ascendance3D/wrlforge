// SPDX-License-Identifier: GPL-3.0-or-later
// PR #130 review: the RUST1-PRECISION check `unsafeIntegerInvolved` must not
// use the JavaScript answer it grades as evidence. Unsafe inputs are read from
// the case; an unsafe result is proven by the independent BigInt oracle.
//   node --test spikes/rust-1-wasm-boundary/test/precision-check.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadRegistry, runStage } from '../harness/framework.mjs';
import { makeStage } from '../harness/stages/edit-algebra.mjs';

const SPIKE = join(dirname(fileURLToPath(import.meta.url)), '..');
const ROOT = join(SPIKE, '..', '..');
const stage = makeStage(ROOT);
const check = stage.checks.unsafeIntegerInvolved;
const M = Number.MAX_SAFE_INTEGER;
const REFUSED = 'ERR EOFFSETPRECISION - -';
const ins = (from, text) => ({ from, to: from, insert: text });
const pm = (off, edits, aff = 'before') => ({ kind: 'PM', group: 't', off, aff, edits });
const range = (from, to, edits, opts = {}) => ({ kind: 'R', group: 't', range: { from, to }, edits, opts });

test('safe inputs, unsafe exact output: involved, whatever JavaScript printed', () => {
  const cases = [
    pm(M - 1, [ins(0, 'ab')]), // M + 1
    pm(M, [ins(0, 'x')]), // M + 1
    pm(M - 3, [{ from: M - 4, to: M - 4, insert: 'abcd' }], 'before'), // M + 1
    range(0, M - 1, [ins(0, 'ab')]), // [2, M + 1]
    { kind: 'R', group: 't', range: { start: { offset: M }, end: { offset: M } }, edits: [ins(0, 'z')], opts: {} },
    { kind: 'M', group: 't', off: M, aff: 'a', edits: [ins(0, 'q'), ins(1, 'r')] },
  ];
  for (const c of cases) {
    for (const js of ['OKN 9007199254740992', 'OKN 1', 'OKR 0 1', 'ERR EEDITSHAPE 0 -', 'garbage']) {
      assert.equal(check(c, js, REFUSED), true, `${JSON.stringify(c)} js=${js}`);
    }
  }
});

test('unsafe input offsets: involved, without consulting any answer', () => {
  for (const c of [
    pm(2 ** 53, []), pm(2 ** 64, [ins(0, 'ab')]), pm(Number.MAX_VALUE, []),
    pm(0, [{ from: 2 ** 60, to: 2 ** 60, insert: '' }]), pm(0, [{ from: 1, to: 2 ** 53 + 2, insert: 'q' }]),
    range(0, 2 ** 53, []), range(2 ** 70, 2 ** 70, []),
    { kind: 'R', group: 't', range: { start: { offset: 0 }, end: { offset: 2 ** 53 } }, edits: [], opts: {} },
    { kind: 'SM', group: 't', off: 2 ** 53, aff: 'before', edits: [] },
  ]) assert.equal(check(c, 'OKN 0', REFUSED), true, JSON.stringify(c));
});

test('valid safe integer cases are NOT involved, even when the JavaScript answer claims an unsafe value', () => {
  for (const c of [
    pm(5, [ins(0, 'x')]), pm(0, []), pm(M - 2, [ins(0, 'a')]), pm(M, []), pm(M, [{ from: 1, to: 3, insert: '' }]),
    range(0, M, []), range(3, 4, [{ from: 1, to: 3, insert: '' }]),
    { kind: 'M', group: 't', off: 2, aff: 'b', edits: [ins(0, 'q'), ins(1, 'r')] },
  ]) {
    for (const js of ['OKN 9007199254740993', `OKR 0 ${2 ** 60}`, 'OKN 1e+300', 'OKN 6']) {
      assert.equal(check(c, js, REFUSED), false, `${JSON.stringify(c)} js=${js}`);
    }
  }
});

test('not a well-formed mapping (shape, overlap, ambiguity, other kinds): NOT involved', () => {
  for (const c of [
    pm('1', []), pm(1.5, []), pm(-1, []), pm(null, []), pm(1, 7), pm(1, [null]), pm(1, [{ from: 2, to: 1, insert: '' }]),
    pm(1, [{ from: 0, to: 1 }]), pm(1, [ins(0, 'a'), ins(0, 'b')]), pm(1, [{ from: 0, to: 3, insert: '' }, { from: 2, to: 4, insert: '' }]),
    range(2, 1, []), { kind: 'R', group: 't', range: null, edits: [], opts: {} },
    { kind: 'R', group: 't', range: { start: { offset: 0 } }, edits: [], opts: {} },
    { kind: 'A', group: 't', text: 'abc', edits: [] }, { kind: 'PA', group: 't', text: 'abc', edits: [{ from: 0, to: 1, insert: 'z' }] },
  ]) assert.equal(check(c, 'OKN 9007199254740993', REFUSED), false, JSON.stringify(c));
});

// A candidate that refuses a safe-in, safe-out mapping must stay a regression
// through the real framework and registry, even if the baseline answer lies.
test('a genuine candidate regression is never classified as RUST1-PRECISION', () => {
  const registry = loadRegistry(join(SPIKE, 'harness', 'registry.json'));
  const cases = [
    { ...pm(5, [ins(0, 'ab')]), js: 'OKN 9007199254740993' }, // lying baseline, safe truth 7
    { ...pm(5, [ins(0, 'ab')]), js: 'OKN 7' },
    { ...range(1, 2, [ins(0, 'ab')]), js: 'OKR 3 9007199254740993' },
    { ...pm(M - 1, [ins(0, 'ab')]), js: 'OKN 9007199254740992' }, // genuine precision case
    { ...pm(2 ** 53, []), js: 'OKN 9007199254740992' }, // genuine precision case
  ];
  const scripted = {
    id: 'edit-algebra', status: 'implemented', compares: [],
    cases: () => cases, serialize: (c) => JSON.stringify(c), baseline: (c) => c.js, candidate: () => REFUSED,
    checks: stage.checks,
  };
  const s = runStage(scripted, {}, registry);
  assert.deepEqual(s.tally, { exact: 0, approved: 0, pending: 2, regression: 3 });
  assert.deepEqual(s.byEntry['RUST1-PRECISION'], 2);
  assert.deepEqual(s.regressions.map((r) => r.id), [0, 1, 2]);
});
