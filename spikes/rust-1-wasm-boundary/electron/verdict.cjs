// SPDX-License-Identifier: GPL-3.0-or-later
// RUST-1 Electron proof verdict. Pure: page results in, pass/fail + reasons
// out. run.cjs owns the launch; this file owns the decision, so the decision
// is testable without Electron (test/electron-verdict.test.mjs).
//
// PR #130 review fixes:
//   * the production page passes only if eval is refused with an EvalError;
//   * the negative control passes only if it is non-fatal AND reports every
//     expected loader AND every one of them was blocked. An empty or partial
//     loader record proves nothing, so it fails.
'use strict';

const LOADERS = Object.freeze(['fetchThenInitSync', 'glueDefaultAsync', 'embeddedBytesAsync']);

const isObject = (v) => !!v && typeof v === 'object' && !Array.isArray(v);

function productionVerdict(prod) {
  const reasons = [];
  if (!isObject(prod)) return { ok: false, reasons: ['no production-csp result'] };
  if (prod.fatal) reasons.push(`fatal renderer error: ${prod.fatal}`);
  if (prod.evalBlocked !== 'EvalError') reasons.push(`eval was not refused with EvalError (evalBlocked=${JSON.stringify(prod.evalBlocked)})`);
  const loaders = isObject(prod.loaders) ? Object.values(prod.loaders) : [];
  if (!loaders.some((l) => isObject(l) && l.ok === true)) reasons.push('no loader instantiated the wasm');
  const checks = Array.isArray(prod.checks) ? prod.checks : [];
  if (checks.length === 0) reasons.push('no checks ran');
  for (const c of checks) if (!isObject(c) || c.ok !== true) reasons.push(`check failed: ${isObject(c) ? c.name : String(c)}`);
  return { ok: reasons.length === 0, reasons };
}

function negativeControlVerdict(neg) {
  const reasons = [];
  if (!isObject(neg)) return { ok: false, reasons: ['no no-wasm-eval-csp result'] };
  if (neg.fatal) reasons.push(`fatal renderer error: ${neg.fatal}`);
  const loaders = isObject(neg.loaders) ? neg.loaders : {};
  for (const name of LOADERS) {
    const l = loaders[name];
    if (!isObject(l)) reasons.push(`loader ${name} did not report`);
    else if (l.ok !== false) reasons.push(`loader ${name} was not blocked`);
  }
  for (const name of Object.keys(loaders)) if (!LOADERS.includes(name)) reasons.push(`unexpected loader ${name}`);
  return { ok: reasons.length === 0, reasons };
}

// `results` is VisualQaRunner's array of page results.
function evaluateProof(results) {
  const list = Array.isArray(results) ? results : [];
  const prod = productionVerdict(list.find((r) => isObject(r) && r.id === 'production-csp'));
  const neg = negativeControlVerdict(list.find((r) => isObject(r) && r.id === 'no-wasm-eval-csp'));
  return { prodOk: prod.ok, negBlocked: neg.ok, reasons: { production: prod.reasons, negative: neg.reasons } };
}

module.exports = { LOADERS, productionVerdict, negativeControlVerdict, evaluateProof };
