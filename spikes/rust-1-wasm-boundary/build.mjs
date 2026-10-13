#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
// RUST-1 reproducible build of the wrlforge-wasm artifact (and, with
// --negative-controls, one mutant artifact per negative-control feature).
//
//   node spikes/rust-1-wasm-boundary/build.mjs [--negative-controls]
//
// Output (gitignored): out/pkg/ (release) and out/neg/<name>/pkg/ (mutants).
// Mutants use their own --target-dir so they can never overwrite the release
// artifact. The release artifact must report negative-controls=[] -- asserted.
// Tools: the pinned toolchain in crates/rust-toolchain.toml and wasm-bindgen-cli
// 0.2.129 installed under crates/target/tools (see README.md).
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, '..', '..');
const CRATES = join(ROOT, 'crates');
const BINDGEN = join(CRATES, 'target', 'tools', 'bin', 'wasm-bindgen');
const OUT = join(HERE, 'out');
const NEG = ['utf8-tiebreak', 'no-ambiguity', 'no-boundary', 'lossy-utf16'];

const sha = (p) => createHash('sha256').update(readFileSync(p)).digest('hex');
const run = (cmd, args) => execFileSync(cmd, args, { cwd: CRATES, stdio: ['ignore', 'pipe', 'inherit'] }).toString();

if (!existsSync(BINDGEN)) {
  console.error(`missing ${relative(ROOT, BINDGEN)}; install: cargo install wasm-bindgen-cli --version 0.2.129 --locked --root crates/target/tools`);
  process.exit(2);
}

function build(name, features, targetDir) {
  const args = ['build', '--locked', '-p', 'wrlforge-wasm', '--release', '--target', 'wasm32-unknown-unknown', '--target-dir', targetDir];
  if (features.length) args.push('--features', features.join(','));
  run('cargo', args);
  const wasm = join(targetDir, 'wasm32-unknown-unknown', 'release', 'wrlforge_wasm.wasm');
  const pkg = name === 'release' ? join(OUT, 'pkg') : join(OUT, 'neg', name, 'pkg');
  rmSync(pkg, { recursive: true, force: true });
  mkdirSync(pkg, { recursive: true });
  execFileSync(BINDGEN, ['--target', 'web', '--out-dir', pkg, wasm], { stdio: 'inherit' });
  // The glue is an ES module with a .js name; the repository package.json is
  // CommonJS. A scoped marker makes Node load it as ESM. (Browsers use
  // <script type="module"> and ignore this file.)
  writeFileSync(join(pkg, 'package.json'), '{ "type": "module" }\n');
  return {
    name,
    features,
    cargoWasm: { path: relative(ROOT, wasm), sha256: sha(wasm), bytes: readFileSync(wasm).length },
    bindgenWasm: { path: relative(ROOT, join(pkg, 'wrlforge_wasm_bg.wasm')), sha256: sha(join(pkg, 'wrlforge_wasm_bg.wasm')), bytes: readFileSync(join(pkg, 'wrlforge_wasm_bg.wasm')).length },
    glue: { path: relative(ROOT, join(pkg, 'wrlforge_wasm.js')), sha256: sha(join(pkg, 'wrlforge_wasm.js')) },
  };
}

const artifacts = [];
if (process.argv.includes('--negative-controls')) {
  for (const n of NEG) artifacts.push(build(n, [`negative-control-${n}`], join(CRATES, 'target', `neg-${n}`)));
}
// Release last, in the default target dir.
artifacts.push(build('release', [], join(CRATES, 'target')));

const versions = {
  rustc: run('rustc', ['--version']).trim(),
  cargo: run('cargo', ['--version']).trim(),
  wasmBindgen: execFileSync(BINDGEN, ['--version']).toString().trim(),
  node: process.version,
};
const manifest = { versions, artifacts };
writeFileSync(join(OUT, 'build-manifest.json'), JSON.stringify(manifest, null, 2) + '\n');
console.log(JSON.stringify(manifest, null, 2));
