#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
// RUST-1 differential runner.
//
//   node spikes/rust-1-wasm-boundary/build.mjs --negative-controls
//   node spikes/rust-1-wasm-boundary/harness/run.mjs [--negative-controls]
//
// Exit 0 only if the release artifact has 0 regressions, the RUST-0 subset
// reproduces RUST-0's input and baseline digests, and (with
// --negative-controls) every mutant artifact is detected (> 0 regressions).
// Writes out/differential-summary.json (gitignored).
import { mkdirSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadRegistry, runStage } from './framework.mjs';
import { makeStage } from './stages/edit-algebra.mjs';
import { PLANNED_STAGES } from './stages/planned.mjs';
import { loadEngine } from '../loaders/node.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const SPIKE = join(HERE, '..');
const ROOT = join(SPIKE, '..', '..');
const OUT = join(SPIKE, 'out');
const NEG = ['utf8-tiebreak', 'no-ambiguity', 'no-boundary', 'lossy-utf16'];

const registry = loadRegistry(join(HERE, 'registry.json'));
const stage = makeStage(ROOT);

async function runWith(pkgDir, fresh) {
  const engine = await loadEngine(pkgDir, fresh);
  const t0 = performance.now();
  const summary = runStage(stage, { engine, snapshots: new Map() }, registry);
  return { engineInfo: engine.info, poisoned: engine.poisoned, seconds: +((performance.now() - t0) / 1000).toFixed(1), ...summary };
}

const release = await runWith(join(OUT, 'pkg'), 'release');
const result = {
  node: process.version,
  plannedStages: PLANNED_STAGES.map((s) => ({ id: s.id, lane: s.lane, status: s.status })),
  release,
};
let ok = release.tally.regression === 0
  && release.rust0Equivalence.inputMatchesRust0 && release.rust0Equivalence.baselineMatchesRust0
  && /negative-controls=\[\]/.test(release.engineInfo);

if (process.argv.includes('--negative-controls')) {
  result.negativeControls = {};
  for (const n of NEG) {
    const dir = join(OUT, 'neg', n, 'pkg');
    if (!existsSync(dir)) { console.error(`missing ${dir}; run build.mjs --negative-controls`); process.exit(2); }
    const s = await runWith(dir, `neg-${n}`);
    const detected = s.tally.regression > 0;
    result.negativeControls[n] = { engineInfo: s.engineInfo, regressions: s.tally.regression, detected, candidateSha256: s.candidateSha256, firstRegression: s.regressions[0] };
    ok = ok && detected;
  }
}

mkdirSync(OUT, { recursive: true });
writeFileSync(join(OUT, 'differential-summary.json'), JSON.stringify(result, null, 2) + '\n');
const brief = { ...result, release: { ...release, regressions: release.regressions.length } };
if (brief.negativeControls) for (const v of Object.values(brief.negativeControls)) delete v.firstRegression;
console.log(JSON.stringify(brief, null, 2));
process.exit(ok ? 0 : 1);
