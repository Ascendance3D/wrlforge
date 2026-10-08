#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
// RUST-1 Node 24 boundary benchmark. Writes out/perf-node.json.
//   node --expose-gc spikes/rust-1-wasm-boundary/perf/node-bench.mjs
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { runBench, time } from './bench-core.mjs';
import { loadInputs } from './inputs.mjs';
import { createTextEngine } from '../../../crates/wrlforge-wasm/js/wrlforge-text.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, '..', '..', '..');
const PKG = join(HERE, '..', 'out', 'pkg');
const require = createRequire(import.meta.url);
const jsEdit = require(join(ROOT, 'src/vrml/edit.js'));
const bytes = readFileSync(join(PKG, 'wrlforge_wasm_bg.wasm'));
const glueUrl = pathToFileURL(join(PKG, 'wrlforge_wasm.js')).href;

// Initialization: compile + instantiate + glue finalize, on fresh glue
// instances (the dynamic import itself is excluded from the timing).
const initSamples = { compile: [], instantiateViaInitSync: [] };
for (let i = 0; i < 25; i += 1) {
  const g = await import(`${glueUrl}?init=${i}`);
  let t0 = performance.now();
  const mod = new WebAssembly.Module(bytes);
  initSamples.compile.push(performance.now() - t0);
  t0 = performance.now();
  g.initSync({ module: mod });
  initSamples.instantiateViaInitSync.push(performance.now() - t0);
}
const g0 = await import(`${glueUrl}?cold`);
let t0 = performance.now();
g0.initSync({ module: bytes });
const coldInitFromBytesMs = performance.now() - t0;
const med = (a) => [...a].sort((x, y) => x - y)[Math.floor(a.length / 2)];

const glue = await import(`${glueUrl}?bench`);
const wasmExports = glue.initSync({ module: bytes });
const engine = createTextEngine(glue);
const inputs = loadInputs(ROOT);

// Memory: hold one session per input and measure.
globalThis.gc?.();
const before = process.memoryUsage();
const wasmBefore = wasmExports.memory.buffer.byteLength;
const held = inputs.map(({ text }) => engine.openSession(text));
globalThis.gc?.();
const during = process.memoryUsage();
const wasmDuring = wasmExports.memory.buffer.byteLength;
held.forEach((s) => s.dispose());
globalThis.gc?.();
const after = process.memoryUsage();
const wasmAfter = wasmExports.memory.buffer.byteLength;
const mb = (n) => Math.round((n / 1048576) * 10) / 10;

const bench = runBench({ engine, glue, wasmExports, inputs, jsEdit });
const manifest = JSON.parse(readFileSync(join(HERE, '..', 'out', 'build-manifest.json'), 'utf8'));
const result = {
  host: {
    cpu: os.cpus()[0].model, cores: os.cpus().length, os: `${os.type()} ${os.release()} ${os.arch()}`,
    node: process.version, v8: process.versions.v8, rustc: manifest.versions.rustc, wasmBindgen: manifest.versions.wasmBindgen,
    electronPinned: JSON.parse(readFileSync(join(ROOT, 'node_modules/electron/package.json'), 'utf8')).version,
    wasmProfile: 'release (opt-level 3, lto, codegen-units 1, strip)',
    wasmSha256: manifest.artifacts.find((a) => a.name === 'release').bindgenWasm.sha256,
    gcExposed: typeof globalThis.gc === 'function',
    loadavg: os.loadavg().map((v) => +v.toFixed(2)),
  },
  init: {
    wasmBytes: bytes.length,
    coldInitSyncFromBytesMs: +coldInitFromBytesMs.toFixed(3),
    compileMedianMs: +med(initSamples.compile).toFixed(3),
    instantiateMedianMs: +med(initSamples.instantiateViaInitSync).toFixed(3),
    samples: initSamples.compile.length,
  },
  memory: {
    note: 'one session held per input (all four inputs at once, ~3.6 MB of text)',
    wasmLinearMemoryMB: { before: mb(wasmBefore), during: mb(wasmDuring), afterDispose: mb(wasmAfter) },
    rssMB: { before: mb(before.rss), during: mb(during.rss), after: mb(after.rss) },
    heapUsedMB: { before: mb(before.heapUsed), during: mb(during.heapUsed), after: mb(after.heapUsed) },
    arrayBuffersMB: { before: mb(before.arrayBuffers), during: mb(during.arrayBuffers), after: mb(after.arrayBuffers) },
  },
  ...bench,
  frameBudgetMs: 16.7,
  analyzeDebounceMs: 250,
};
writeFileSync(join(HERE, '..', 'out', 'perf-node.json'), JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify(result, null, 2));
