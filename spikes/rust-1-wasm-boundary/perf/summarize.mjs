// SPDX-License-Identifier: GPL-3.0-or-later
// Print a perf JSON (Node or Electron) as a median / p95 table.
//   node spikes/rust-1-wasm-boundary/perf/summarize.mjs out/perf-node.json
import { readFileSync } from 'node:fs';
const r = JSON.parse(readFileSync(process.argv[2], 'utf8'));
const rows = r.rows || r.perf?.rows;
const keys = Object.keys(rows[0].ms);
console.log(['op', ...rows.map((x) => x.name)].join(' | '));
for (const k of keys) {
  console.log([k, ...rows.map((x) => (x.ms[k] ? `${x.ms[k].median} / ${x.ms[k].p95}` : '-'))].join(' | '));
}
console.log(['resultMatchesJs', ...rows.map((x) => x.resultMatchesJs)].join(' | '));
console.log(['textOutExact', ...rows.map((x) => x.textOutExact)].join(' | '));
