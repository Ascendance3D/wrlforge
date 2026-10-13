// SPDX-License-Identifier: GPL-3.0-or-later
// PR #130 review: harness/precision-review.mjs must exit non-zero when Rust
// reports a mapping SUCCESS on an edit set the independent oracle refuses.
// Unit cases use a scripted stage; the end-to-end case runs the real command
// on the no-ambiguity negative-control build (a real Rust artifact that maps
// sets the contract refuses).
//   node spikes/rust-1-wasm-boundary/build.mjs --negative-controls
//   node --test spikes/rust-1-wasm-boundary/test/precision-review-gate.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { review, summarize, exitCode } from '../harness/precision-review.mjs';

const SPIKE = join(dirname(fileURLToPath(import.meta.url)), '..');
const SCRIPT = join(SPIKE, 'harness', 'precision-review.mjs');
const ENTRY = {
  id: 'RUST1-PRECISION', candidateCode: 'EOFFSETPRECISION', independentCheck: 'unsafeIntegerInvolved',
  ownerApproval: { status: 'proposed-pending-owner' },
};
const REGISTRY = { sha256: 'test' };

// A scripted stage: each case carries the two answers it should produce.
const scripted = (check = () => true) => ({
  baseline: (c) => c.js,
  candidate: (c) => c.rs,
  checks: { unsafeIntegerInvolved: check },
});
const gate = (cases, check) => {
  cases.forEach((c, i) => { c.id = i; });
  const summary = summarize(review(cases, scripted(check), {}, ENTRY), REGISTRY, ENTRY);
  return { summary, code: exitCode(summary) };
};
const ins = (from, text) => ({ from, to: from, insert: text });

test('a clean run exits 0', () => {
  const { summary, code } = gate([
    { kind: 'PM', group: 'g', off: 5, aff: 'before', edits: [ins(0, 'ab')], js: 'OKN 7', rs: 'OKN 7' },
  ]);
  assert.deepEqual(summary.rustSuccess, { graded: 1, exact: 1, inexact: 0, oracleRefused: 0 });
  assert.equal(code, 0);
});

test('Rust success where the oracle refuses (ambiguous double insert) exits non-zero', () => {
  const { summary, code } = gate([
    { kind: 'PM', group: 'g', off: 0, aff: 'before', edits: [ins(0, 'a'), ins(0, 'b')], js: 'ERR EEDITAMBIGUOUS 1 0', rs: 'OKN 0' },
  ]);
  assert.equal(summary.rustSuccess.oracleRefused, 1);
  assert.equal(summary.genuineRegressions, 0);
  assert.equal(summary.rustSuccess.inexact, 0);
  assert.equal(code, 1);
});

test('Rust success where the oracle refuses (overlap), even when JavaScript agrees, exits non-zero', () => {
  const { summary, code } = gate([
    { kind: 'R', group: 'g', range: { from: 0, to: 1 }, opts: {}, edits: [{ from: 0, to: 3, insert: '' }, { from: 2, to: 4, insert: '' }],
      js: 'OKR 0 0', rs: 'OKR 0 0' },
  ]);
  assert.equal(summary.rustSuccess.oracleRefused, 1);
  assert.equal(code, 1);
});

test('the existing gates still hold: an inexact success and a genuine regression each exit non-zero', () => {
  const inexact = gate([{ kind: 'PM', group: 'g', off: 5, aff: 'before', edits: [ins(0, 'ab')], js: 'OKN 7', rs: 'OKN 8' }]);
  assert.equal(inexact.summary.rustSuccess.inexact, 1);
  assert.equal(inexact.code, 1);
  // Safe input, safe exact truth (7), Rust refused for precision.
  const genuine = gate([{ kind: 'PM', group: 'g', off: 5, aff: 'before', edits: [ins(0, 'ab')], js: 'OKN 7', rs: 'ERR EOFFSETPRECISION - -' }]);
  assert.equal(genuine.summary.genuineRegressions, 1);
  assert.equal(genuine.code, 1);
});

test('exitCode reads every gate field', () => {
  const base = { genuineRegressions: 0, rustSuccess: { inexact: 0, oracleRefused: 0 } };
  assert.equal(exitCode(base), 0);
  assert.equal(exitCode({ ...base, genuineRegressions: 1 }), 1);
  assert.equal(exitCode({ ...base, rustSuccess: { inexact: 1, oracleRefused: 0 } }), 1);
  assert.equal(exitCode({ ...base, rustSuccess: { inexact: 0, oracleRefused: 1 } }), 1);
});

const run = (pkg) => spawnSync(process.execPath, [SCRIPT], {
  env: { ...process.env, ...(pkg ? { RUST1A_PKG: pkg } : {}) }, encoding: 'utf8', timeout: 120000, maxBuffer: 64 << 20,
});

test('end to end: the real command exits non-zero on the no-ambiguity negative control', () => {
  const pkg = join(SPIKE, 'out', 'neg', 'no-ambiguity', 'pkg');
  assert.ok(existsSync(join(pkg, 'wrlforge_wasm_bg.wasm')), 'build first: build.mjs --negative-controls');
  const r = run(pkg);
  assert.equal(r.status, 1, r.stderr);
  const s = JSON.parse(r.stdout);
  assert.ok(s.rustSuccess.oracleRefused > 0, JSON.stringify(s.rustSuccess));
  assert.equal(s.rustSuccess.inexact, 0);
  assert.equal(s.genuineRegressions, 0, 'the mutant fails ONLY through the oracle-refused gate');
});

test('end to end: the real command exits 0 on the release build', () => {
  const r = run(null);
  assert.equal(r.status, 0, r.stderr);
  const s = JSON.parse(r.stdout);
  assert.deepEqual([s.rustSuccess.oracleRefused, s.rustSuccess.inexact, s.genuineRegressions], [0, 0, 0]);
});
