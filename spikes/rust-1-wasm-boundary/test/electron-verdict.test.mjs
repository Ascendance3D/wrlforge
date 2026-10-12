// SPDX-License-Identifier: GPL-3.0-or-later
// PR #130 review: the Electron proof verdict (electron/verdict.cjs), without
// Electron. The production page must refuse eval with an EvalError; the
// negative control must be non-fatal and report all three loaders blocked.
//   node --test spikes/rust-1-wasm-boundary/test/electron-verdict.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { LOADERS, productionVerdict, negativeControlVerdict, evaluateProof } = require('../electron/verdict.cjs');

const goodProd = () => ({
  id: 'production-csp',
  fatal: null,
  evalBlocked: 'EvalError',
  loaders: {
    fetchThenInitSync: { ok: true }, glueDefaultAsync: { ok: true }, embeddedBytesAsync: { ok: true },
  },
  checks: [{ name: 'a', ok: true }, { name: 'b', ok: true }],
});
const blocked = { ok: false, error: 'CompileError: WebAssembly.Module(): Refused to compile' };
const goodNeg = () => ({
  id: 'no-wasm-eval-csp',
  fatal: null,
  evalBlocked: 'EvalError',
  loaders: { fetchThenInitSync: blocked, glueDefaultAsync: blocked, embeddedBytesAsync: blocked },
  checks: [],
});

test('the expected loader names are the three proof.mjs loaders', () => {
  assert.deepEqual([...LOADERS], ['fetchThenInitSync', 'glueDefaultAsync', 'embeddedBytesAsync']);
});

test('a real passing pair passes', () => {
  assert.deepEqual(evaluateProof([goodProd(), goodNeg()]),
    { prodOk: true, negBlocked: true, reasons: { production: [], negative: [] } });
});

test('production: a successful eval fails the proof', () => {
  for (const evalBlocked of [false, null, undefined, 'TypeError', 'SyntaxError', true, 'evalerror']) {
    const r = productionVerdict({ ...goodProd(), evalBlocked });
    assert.equal(r.ok, false, `evalBlocked=${String(evalBlocked)}`);
    assert.match(r.reasons.join(';'), /EvalError/);
  }
});

test('production: existing requirements still hold', () => {
  assert.equal(productionVerdict({ ...goodProd(), fatal: 'TypeError: x' }).ok, false);
  assert.equal(productionVerdict({ ...goodProd(), checks: [] }).ok, false);
  assert.equal(productionVerdict({ ...goodProd(), checks: [{ name: 'a', ok: true }, { name: 'b', ok: false }] }).ok, false);
  const none = goodProd();
  for (const k of LOADERS) none.loaders[k] = blocked;
  assert.equal(productionVerdict(none).ok, false);
  assert.equal(productionVerdict({ ...goodProd(), loaders: {} }).ok, false);
  assert.equal(productionVerdict(undefined).ok, false);
});

test('negative control: an empty loader result fails', () => {
  assert.equal(negativeControlVerdict({ ...goodNeg(), loaders: {} }).ok, false);
  assert.equal(negativeControlVerdict({ ...goodNeg(), loaders: undefined }).ok, false);
});

test('negative control: each missing loader fails', () => {
  for (const name of LOADERS) {
    const neg = goodNeg();
    delete neg.loaders[name];
    const r = negativeControlVerdict(neg);
    assert.equal(r.ok, false, name);
    assert.deepEqual(r.reasons, [`loader ${name} did not report`]);
  }
});

test('negative control: any loader that was not blocked fails', () => {
  for (const name of LOADERS) {
    for (const rec of [{ ok: true }, { ok: undefined }, { ok: 'false' }, null, 'blocked']) {
      const neg = goodNeg();
      neg.loaders[name] = rec;
      assert.equal(negativeControlVerdict(neg).ok, false, `${name} ${JSON.stringify(rec)}`);
    }
  }
});

test('negative control: an unrelated fatal renderer error fails, even with every loader blocked', () => {
  const r = negativeControlVerdict({ ...goodNeg(), fatal: 'TypeError: cannot read properties of null' });
  assert.equal(r.ok, false);
  assert.match(r.reasons[0], /fatal renderer error/);
  assert.equal(negativeControlVerdict({ ...goodNeg(), fatal: 'boom', loaders: {} }).ok, false);
});

test('negative control: an unexpected extra loader fails', () => {
  const neg = goodNeg();
  neg.loaders.somethingElse = blocked;
  assert.equal(negativeControlVerdict(neg).ok, false);
});

test('a missing page result fails both halves independently', () => {
  assert.deepEqual([evaluateProof([goodNeg()]).prodOk, evaluateProof([goodNeg()]).negBlocked], [false, true]);
  assert.deepEqual([evaluateProof([goodProd()]).prodOk, evaluateProof([goodProd()]).negBlocked], [true, false]);
  assert.deepEqual([evaluateProof([]).prodOk, evaluateProof(undefined).negBlocked], [false, false]);
});

test('run.cjs decides through verdict.cjs, not an inline expression', async () => {
  const { readFileSync } = await import('node:fs');
  const src = readFileSync(new URL('../electron/run.cjs', import.meta.url), 'utf8');
  assert.match(src, /require\('\.\/verdict\.cjs'\)/);
  assert.match(src, /= evaluateProof\(results\)/);
  assert.doesNotMatch(src, /\.every\(\(l\) => !l\.ok\)/, 'the vacuous negative-control expression is gone');
  assert.match(src, /process\.exit\(prodOk && negBlocked \? 0 : 1\)/);
});
