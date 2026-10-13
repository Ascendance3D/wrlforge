'use strict';
// SHELL-0 module-loading spike -- runner. Evidence only; never shipped.
//
//   node spikes/shell-0-module-loading/build.js
//   node spikes/shell-0-module-loading/run.js [--rounds=15]
//
// ONE Electron process (spike main.js), driven by VisualQaRunner under the
// visual-QA lock and the workspace guard, loads every spike page, then
// alternates the three loading strategies of one synthetic 42-file graph
// (classic defer tags / native ESM / one esbuild entry bundle) for N rounds.
// Writes out/RESULTS.json (gitignored) and prints a summary.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn } = require('child_process');
const repoRoot = path.join(__dirname, '..', '..');
const { VisualQaRunner } = require(path.join(repoRoot, 'qa', 'visual-qa', 'runner'));
const { acquire } = require(path.join(repoRoot, 'qa', 'visual-qa', 'lock'));
const { guardWindowsWorkspace } = require(path.join(repoRoot, 'qa', 'visual-qa', 'workspace-guard'));
const { parseArgs } = require(path.join(repoRoot, 'qa', 'visual-qa', 'cli'));

const HERE = __dirname;
const OUT = path.join(HERE, 'out');
const APP = path.join(HERE, 'app');
const ASAR = path.join(OUT, 'spike.asar');

function stats(values) {
  const v = values.filter(Number.isFinite).sort((a, b) => a - b);
  if (!v.length) return null;
  const q = (p) => { const i = (v.length - 1) * p; const lo = Math.floor(i); return v[lo] + (v[Math.ceil(i)] - v[lo]) * (i - lo); };
  const r = (x) => Math.round(x * 10) / 10;
  return { n: v.length, median: r(q(0.5)), p10: r(q(0.1)), p90: r(q(0.9)), min: r(v[0]), max: r(v[v.length - 1]) };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const rounds = Number(args.flags.rounds || 15);
  guardWindowsWorkspace({ cwd: repoRoot, label: 'shell-0-module-spike' });
  if (!fs.existsSync(path.join(OUT, 'BUILD.json'))) {
    console.error('run build.js first');
    process.exit(2);
  }
  if (process.platform !== 'win32' && !process.env.DISPLAY && !process.env.WAYLAND_DISPLAY) {
    console.error('module spike: no DISPLAY/WAYLAND_DISPLAY -- refusing to launch Electron headless-blind.');
    process.exit(2);
  }
  const sandboxed = !args.flags['no-sandbox'];
  const userData = fs.mkdtempSync(path.join(os.tmpdir(), 'wrlforge-shell0-esm-'));
  const jobs = [];
  const feature = (id, file, timeoutMs) => jobs.push({ id, file, timeoutMs });
  feature('esm-basic', path.join(APP, 'esm-basic.html'));
  feature('esm-strict-self', path.join(APP, 'esm-strict-self.html'));
  feature('esm-inline', path.join(APP, 'esm-inline.html'), 1200);
  feature('esm-missing', path.join(APP, 'esm-missing.html'), 1200);
  feature('esm-throw', path.join(APP, 'esm-throw.html'), 1200);
  feature('esm-bare', path.join(APP, 'esm-bare.html'), 1200);
  feature('esm-cjs-side-effect', path.join(APP, 'esm-cjs-side-effect.html'));
  feature('esm-cjs-named', path.join(APP, 'esm-cjs-named.html'), 1200);
  feature('bundle-basic', path.join(APP, 'bundle-basic.html'));
  feature('asar-esm-basic', path.join(ASAR, 'app', 'esm-basic.html'));
  feature('asar-bundle-basic', path.join(ASAR, 'app', 'bundle-basic.html'));
  const strategies = ['classic', 'esm', 'bundle'];
  for (let r = 0; r < rounds; r++) {
    // rotate the order each round so no strategy always runs first
    for (let k = 0; k < strategies.length; k++) {
      const s = strategies[(r + k) % strategies.length];
      jobs.push({ id: `many-${s}-${r}`, file: path.join(OUT, 'many', `${s}.html`), timeoutMs: 5000 });
    }
  }

  const runner = new VisualQaRunner({
    spawn: () => spawn(require('electron'), [path.join(HERE, 'main.js'), ...(sandboxed ? [] : ['--no-sandbox']), `--user-data-dir=${userData}`], {
      cwd: repoRoot, stdio: ['pipe', 'pipe', 'ignore'],
    }),
    maxLaunches: 1, retriesPerLaunch: 0, readyTimeoutMs: 30000, captureTimeoutMs: 30000,
    log: () => {},
  });
  const release = acquire();
  let results;
  try { results = await runner.run(jobs); } finally {
    release();
    fs.rmSync(userData, { recursive: true, force: true });
  }
  const byId = Object.fromEntries(results.map((r) => [r.id, r]));
  const timing = {};
  for (const s of strategies) {
    const runs = results.filter((r) => r.id.startsWith(`many-${s}-`));
    timing[s] = {
      allCompleted: runs.every((r) => r.result && r.result.done),
      modulesSeen: runs[0] && runs[0].result && runs[0].result.data.modules,
      toDoneMs: stats(runs.map((r) => r.result && r.result.t)),
      toDoneMsExcludingFirst: stats(runs.slice(1).map((r) => r.result && r.result.t)),
      toLoadEndMs: stats(runs.map((r) => r.nav && r.nav.loadEnd)),
    };
  }
  const features = {};
  for (const j of jobs.filter((x) => !x.id.startsWith('many-'))) {
    const r = byId[j.id];
    features[j.id] = { done: !!(r.result && r.result.done), data: r.result && r.result.data, order: r.result && r.result.order,
      errors: r.result && r.result.errors, violations: r.result && r.result.violations, console: r.console, loadError: r.loadError };
  }
  const report = {
    lane: 'SHELL-0 module-loading spike',
    electron: require('electron/package.json').version,
    platform: `${process.platform} ${os.release()}`,
    sandboxed,
    rounds,
    build: JSON.parse(fs.readFileSync(path.join(OUT, 'BUILD.json'), 'utf8')),
    features,
    timing,
  };
  fs.writeFileSync(path.join(OUT, 'RESULTS.json'), JSON.stringify(report, null, 2) + '\n');
  process.stdout.write(JSON.stringify({ features: Object.fromEntries(Object.entries(features).map(([k, v]) => [k, { done: v.done, errors: v.errors && v.errors.length, violations: v.violations && v.violations.length }])), timing }, null, 2) + '\n');
}

main().catch((err) => { console.error(err && err.stack || err); process.exit(1); });
