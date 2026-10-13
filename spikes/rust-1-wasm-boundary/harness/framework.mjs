// SPDX-License-Identifier: GPL-3.0-or-later
// RUST-1 reusable differential framework.
//
// One shape for every migration stage (RUST-2..RUST-5): identical inputs go to
// the JavaScript baseline and to the Rust candidate; each answer is a canonical
// string; the pair is classified as
//
//   exact      -- byte-identical canonical answers
//   approved   -- differs, AND a registry entry for this stage names the
//                 candidate's error code, AND that entry's INDEPENDENT check
//                 (written in the stage, never trusting either answer) holds,
//                 AND the entry's owner status is approved-experimental
//   pending    -- same as approved, but the entry is proposed-pending-owner
//   regression -- anything else (harness exits non-zero)
//
// Determinism: cases are generated in a fixed order with sequential ids; no
// PRNG, no clock in any digest; SHA-256 over canonical input / baseline /
// candidate streams; the registry's own SHA-256 is recorded so a widened
// registry is always visible.
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';

export const sha256 = (s) => createHash('sha256').update(s).digest('hex');

const REQUIRED = ['id', 'stage', 'candidateCode', 'inputCondition', 'independentCheck', 'justification', 'ownerApproval'];
const STATUSES = new Set(['approved-experimental', 'proposed-pending-owner']);

/** Load and validate the registry. Throws on any malformed entry. */
export function loadRegistry(path) {
  const raw = readFileSync(path, 'utf8');
  const reg = JSON.parse(raw);
  const ids = new Set();
  for (const e of reg.entries) {
    for (const k of REQUIRED) if (!(k in e)) throw new Error(`registry entry ${e.id}: missing ${k}`);
    if (ids.has(e.id)) throw new Error(`registry: duplicate id ${e.id}`);
    ids.add(e.id);
    if (/^D[0-9]$/.test(e.id)) throw new Error(`registry: id ${e.id} collides with an owner-decision id`);
    if (!STATUSES.has(e.ownerApproval.status)) throw new Error(`registry entry ${e.id}: bad status`);
    if (e.ownerApproval.productionChangeApproved !== false) {
      throw new Error(`registry entry ${e.id}: a harness entry can never approve a production change`);
    }
  }
  return { entries: reg.entries, sha256: sha256(raw) };
}

/**
 * The stage adapter contract. A stage module exports an object:
 *
 *   id            string, matches registry `stage`
 *   status        'implemented' | 'planned'
 *   description   string
 *   compares      string[]  -- what must match exactly (documentation + summary)
 *   cases(ctx)    -> Case[] in a fixed order; Case = { kind, group, ... }
 *   serialize(c)  -> string, the canonical input line (feeds inputSha256)
 *   baseline(c)   -> string, the JavaScript answer in canonical form
 *   candidate(c, ctx) -> string, the Rust answer in the same canonical form
 *   checks        { [name]: (case, baselineAnswer, candidateAnswer) => boolean }
 *   extra(cases, ctx, streams) -> object merged into the summary (optional)
 *
 * Canonical answers start with `OK` or `ERR <CODE>`; everything after is
 * compared exactly. Answers never contain object addresses or map order.
 */
export const answerCode = (a) => (a.startsWith('ERR ') ? a.split(' ')[1] : 'OK');

export function runStage(stage, ctx, registry) {
  if (stage.status !== 'implemented') {
    const e = new Error(`stage ${stage.id} is ${stage.status}; it has no cases yet`);
    e.code = 'ESTAGEPLANNED';
    throw e;
  }
  const entries = registry.entries.filter((e) => e.stage === stage.id);
  for (const e of entries) {
    if (typeof stage.checks[e.independentCheck] !== 'function') {
      throw new Error(`registry entry ${e.id}: stage ${stage.id} has no check ${e.independentCheck}`);
    }
  }
  const cases = stage.cases(ctx);
  cases.forEach((c, i) => { c.id = i; });

  const inputs = [];
  const baselines = [];
  const candidates = [];
  const classes = []; // per case: 'exact' | registry id | 'regression'
  const tally = { exact: 0, approved: 0, pending: 0, regression: 0 };
  const byEntry = Object.fromEntries(entries.map((e) => [e.id, 0]));
  const byKind = {};
  const regressions = [];

  for (const c of cases) {
    byKind[c.kind] = (byKind[c.kind] || 0) + 1;
    const line = stage.serialize(c);
    const js = stage.baseline(c);
    const rs = stage.candidate(c, ctx);
    inputs.push(line);
    baselines.push(js);
    candidates.push(rs);
    if (js === rs) { tally.exact += 1; classes.push('exact'); continue; }
    const code = answerCode(rs);
    const hit = entries.find((e) => e.candidateCode === code && stage.checks[e.independentCheck](c, js, rs));
    if (hit) {
      byEntry[hit.id] += 1;
      classes.push(hit.id);
      if (hit.ownerApproval.status === 'approved-experimental') tally.approved += 1;
      else tally.pending += 1;
      continue;
    }
    tally.regression += 1;
    classes.push('regression');
    if (regressions.length < 25) regressions.push({ id: c.id, kind: c.kind, group: c.group, input: line.slice(0, 240), js, rust: rs });
  }

  return {
    stage: stage.id,
    compares: stage.compares,
    cases: cases.length,
    byKind,
    tally,
    byEntry,
    registrySha256: registry.sha256,
    registryEntries: entries.map((e) => `${e.id}:${e.ownerApproval.status}`),
    inputSha256: sha256(inputs.join('\n')),
    baselineSha256: sha256(baselines.join('\n')),
    candidateSha256: sha256(candidates.join('\n')),
    ...(stage.extra ? stage.extra(cases, ctx, { inputs, baselines, candidates, classes }) : {}),
    regressions,
  };
}
