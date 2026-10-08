#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
// RUST-1A Task B: independent review of every RUST1-PRECISION case.
//
//   node spikes/rust-1-wasm-boundary/build.mjs
//   node spikes/rust-1-wasm-boundary/harness/precision-review.mjs
//
// Re-runs the edit-algebra stage, selects the cases that the framework puts in
// the RUST1-PRECISION bucket (same rule as framework.mjs), and grades each one
// against an EXACT BigInt oracle. The oracle is written here from the WD1.2
// mapping contract; it imports neither src/vrml/edit.js nor the Rust engine,
// and it does not read either answer. Read-only. Writes
// out/precision-review.json (gitignored). Exit 0 only if no case is a genuine
// regression and every Rust mapping success equals the oracle.
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { answerCode, loadRegistry } from './framework.mjs';
import { makeStage } from './stages/edit-algebra.mjs';
import { loadEngine } from '../loaders/node.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const SPIKE = join(HERE, '..');
const ROOT = join(SPIKE, '..', '..');
const OUT = join(SPIKE, 'out');
const MAX = BigInt(Number.MAX_SAFE_INTEGER);

// ---- independent exact oracle (BigInt) --------------------------------------
// Edits are sorted by `from`, a pure insertion before a replacement at the
// same `from`. Returns a BigInt, or the string 'REFUSE' for an input the WD1.2
// contract refuses (overlap / ambiguity). Only edit sets the stage feeds to
// the P/M/R kinds are needed here; anything else throws so it is never graded
// silently.
function oracleMap(offset, edits, after) {
  const es = edits.map((e) => ({ from: BigInt(e.from), to: BigInt(e.to), n: BigInt(e.insert.length) }))
    .sort((a, b) => (a.from < b.from ? -1 : a.from > b.from ? 1 : (a.to === a.from ? -1 : 1)));
  for (let i = 1; i < es.length; i += 1) {
    const p = es[i - 1];
    const q = es[i];
    if (q.from < p.to) return 'REFUSE';
    if (q.from === p.from && p.from === p.to && q.from === q.to) return 'REFUSE';
  }
  const o = BigInt(offset);
  let shift = 0n;
  for (const e of es) {
    if (e.from === e.to) {
      if (o < e.from) return o + shift;
      if (o === e.from) return o + shift + (after ? e.n : 0n);
      shift += e.n;
      continue;
    }
    if (o < e.from) return o + shift;
    if (o < e.to) return e.from + shift + (after ? e.n : 0n);
    shift += e.n - (e.to - e.from);
  }
  return o + shift;
}

function oracle(c) {
  switch (c.kind) {
    case 'M': return [oracleMap(c.off, c.edits, c.aff === 'a')];
    case 'SM': case 'PM': return [oracleMap(c.off, c.edits, c.aff === 'after')];
    case 'R': {
      const r = c.range;
      const from = r.start ? r.start.offset : r.from;
      const to = r.start ? r.end.offset : r.to;
      const o = c.opts || {};
      return [oracleMap(from, c.edits, o.startAffinity === 'after'),
        oracleMap(to, c.edits, (o.endAffinity === undefined ? 'after' : o.endAffinity) === 'after')];
    }
    default: throw new Error(`no oracle for kind ${c.kind}`);
  }
}

const inputsOf = (c) => {
  const v = [];
  if (typeof c.off === 'number') v.push(c.off);
  if (c.range) v.push(c.range.from, c.range.to, c.range.start?.offset, c.range.end?.offset);
  for (const e of c.edits || []) v.push(e.from, e.to);
  return v.filter((x) => typeof x === 'number');
};

// ---- run ---------------------------------------------------------------------
const registry = loadRegistry(join(HERE, 'registry.json'));
const entry = registry.entries.find((e) => e.id === 'RUST1-PRECISION');
const stage = makeStage(ROOT);
const engine = await loadEngine(join(OUT, 'pkg'));
const ctx = { engine, snapshots: new Map() };
const cases = stage.cases(ctx);
cases.forEach((c, i) => { c.id = i; });

const rows = [];
let allPrecisionRefusals = 0;
// Every Rust SUCCESS on a mapping case is graded too: an answer that JS and
// Rust share is "exact" to the framework even if both are wrong.
const rustSuccess = { graded: 0, exact: 0, inexact: [], oracleRefused: 0 };
for (const c of cases) {
  const js = stage.baseline(c);
  const rs = stage.candidate(c, ctx);
  if (answerCode(rs) === 'EOFFSETPRECISION') allPrecisionRefusals += 1;
  if (['M', 'SM', 'PM', 'R'].includes(c.kind) && /^OK[NR] /.test(rs)) {
    const t = oracle(c);
    const vals = rs.split(' ').slice(1).map(Number);
    rustSuccess.graded += 1;
    if (t.includes('REFUSE')) rustSuccess.oracleRefused += 1;
    else if (vals.every((v, i) => BigInt(v) === t[i])) rustSuccess.exact += 1;
    else if (rustSuccess.inexact.push({ id: c.id, kind: c.kind, rs, truth: t.map(String) }) > 20) rustSuccess.inexact.length = 20;
  }
  if (js === rs) continue;
  if (answerCode(rs) !== entry.candidateCode || !stage.checks[entry.independentCheck](c, js, rs)) continue;

  const ins = inputsOf(c);
  const inputUnsafe = ins.some((x) => !Number.isSafeInteger(x));
  const truth = oracle(c);
  const refused = truth.includes('REFUSE');
  const truthSafe = !refused && truth.every((t) => t >= 0n && t <= MAX);
  const jsOk = /^OK[NR] /.test(js);
  const jsVals = jsOk ? js.split(' ').slice(1).map(Number) : null;
  // A JS answer is "exact" when every value equals the BigInt truth.
  const jsExact = jsOk && !refused && jsVals.every((v, i) => Number.isInteger(v) && BigInt(v) === truth[i]);
  let cls;
  if (!jsOk) cls = 'X-js-error'; // JS refused differently: taxonomy question
  else if (inputUnsafe && jsExact) cls = 'P1-unsafe-input-js-exact';
  else if (inputUnsafe) cls = 'P2-unsafe-input-js-wrong';
  else if (!truthSafe && jsExact) cls = 'P3-safe-input-unsafe-result-js-exact';
  else if (!truthSafe) cls = 'P4-safe-input-unsafe-result-js-wrong';
  else cls = 'G-genuine-regression'; // safe in, safe exact out, Rust refused
  rows.push({
    id: c.id, kind: c.kind, group: c.group, cls,
    inputs: ins, maxInput: Math.max(...ins),
    js, rust: rs, truth: truth.map(String),
  });
}

const byClass = {};
for (const r of rows) {
  const b = (byClass[r.cls] ||= { count: 0, kinds: {}, groups: {}, example: r });
  b.count += 1;
  b.kinds[r.kind] = (b.kinds[r.kind] || 0) + 1;
  b.groups[r.group] = (b.groups[r.group] || 0) + 1;
}
const minMaxInput = Math.min(...rows.map((r) => r.maxInput));
const summary = {
  registrySha256: registry.sha256,
  entryStatus: entry.ownerApproval.status,
  precisionCases: rows.length,
  allPrecisionRefusalsInRun: allPrecisionRefusals,
  byClass,
  smallestLargestInput: minMaxInput,
  smallestLargestInputLog2: Math.log2(minMaxInput),
  anyInputBelow2pow30: rows.some((r) => r.inputs.every((x) => x < 2 ** 30)),
  rustSuccess: { ...rustSuccess, inexact: rustSuccess.inexact.length },
  genuineRegressions: rows.filter((r) => r.cls === 'G-genuine-regression').length,
  taxonomyChanges: rows.filter((r) => r.cls === 'X-js-error').length,
};
mkdirSync(OUT, { recursive: true });
writeFileSync(join(OUT, 'precision-review.json'), JSON.stringify({ summary, rows }, null, 2) + '\n');
console.log(JSON.stringify(summary, (k, v) => (k === 'example' ? { kind: v.kind, inputs: v.inputs, js: v.js, rust: v.rust, truth: v.truth } : v), 2));
process.exit(summary.genuineRegressions === 0 && rustSuccess.inexact.length === 0 ? 0 : 1);
