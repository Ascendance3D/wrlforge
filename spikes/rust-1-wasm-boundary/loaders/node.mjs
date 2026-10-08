// SPDX-License-Identifier: GPL-3.0-or-later
// Node loader for the RUST-1 artifact (wasm-bindgen --target web glue).
// Reads the .wasm bytes itself and initializes synchronously; the glue's
// fetch()-based default path is never used in Node.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createTextEngine } from '../../../crates/wrlforge-wasm/js/wrlforge-text.mjs';

// `pkgDir` defaults to the release artifact. A distinct query string gives a
// FRESH glue module instance (and so a fresh wasm instance) per `fresh` value.
export async function loadGlue(pkgDir, fresh) {
  const url = pathToFileURL(join(pkgDir, 'wrlforge_wasm.js')).href + (fresh ? `?fresh=${fresh}` : '');
  const glue = await import(url);
  const bytes = readFileSync(join(pkgDir, 'wrlforge_wasm_bg.wasm'));
  glue.initSync({ module: bytes });
  return glue;
}

export async function loadEngine(pkgDir, fresh) {
  return createTextEngine(await loadGlue(pkgDir, fresh));
}
