// SPDX-License-Identifier: GPL-3.0-or-later
// RUST-1 renderer proof. Runs inside an isolated Electron BrowserWindow with
// contextIsolation: true, nodeIntegration: false, no preload, and the
// production editor CSP. Reports one JSON line on the console; main.cjs relays
// it. No IPC, no Node, no privileged scheme.
import { createTextEngine } from '../../../crates/wrlforge-wasm/js/wrlforge-text.mjs';
import { runBench } from '../perf/bench-core.mjs';

const PKG = new URL('../out/pkg/', import.meta.url);
const result = {
  page: location.pathname.split('/').pop(),
  env: {},
  evalBlocked: null,
  cspViolations: [],
  loaders: {},
  checks: [],
  perf: null,
  fatal: null,
};
document.addEventListener('securitypolicyviolation', (e) => {
  result.cspViolations.push({ directive: e.violatedDirective, blockedURI: e.blockedURI, sample: e.sample });
});

const check = (name, fn) => {
  try {
    const v = fn();
    result.checks.push({ name, ok: v === true, detail: v === true ? undefined : String(v) });
  } catch (e) {
    result.checks.push({ name, ok: false, detail: `${e.code || e.name}: ${e.message}` });
  }
};
const code = (fn) => { try { fn(); return 'NO-ERROR'; } catch (e) { return e.code; } };
const units = (s) => Array.from({ length: s.length }, (_, i) => s.charCodeAt(i)).join(',');

async function tryLoader(name, fn) {
  const t0 = performance.now();
  try {
    const glue = await fn();
    result.loaders[name] = { ok: true, ms: +(performance.now() - t0).toFixed(3), info: glue.engine_info() };
    return glue;
  } catch (e) {
    result.loaders[name] = { ok: false, error: `${e.name}: ${e.message}` };
    return null;
  }
}

async function main() {
  result.env = {
    userAgent: navigator.userAgent,
    protocol: location.protocol,
    typeofRequire: typeof globalThis.require,
    typeofProcess: typeof globalThis.process,
    typeofModule: typeof globalThis.module,
    hasIsWellFormed: typeof String.prototype.isWellFormed === 'function',
    webAssembly: typeof WebAssembly,
  };
  try {
    // Must be refused: the production CSP has no 'unsafe-eval'.
    (0, eval)('1');
    result.evalBlocked = false;
  } catch (e) {
    result.evalBlocked = `${e.name}`;
  }

  // L1: fetch the .wasm from file:, synchronous initSync on the main thread.
  const g1 = await tryLoader('fetchThenInitSync', async () => {
    const g = await import(new URL('wrlforge_wasm.js?l1', PKG));
    const bytes = await (await fetch(new URL('wrlforge_wasm_bg.wasm', PKG))).arrayBuffer();
    g.initSync({ module: bytes });
    return g;
  });
  // L2: the glue's own default async init (fetch + instantiate[Streaming]).
  const g2 = await tryLoader('glueDefaultAsync', async () => {
    const g = await import(new URL('wrlforge_wasm.js?l2', PKG));
    await g.default();
    return g;
  });
  // L3: bytes embedded in a JS module (the esbuild `binary` loader shape the
  // production bundle would use) -- no fetch, no file read at runtime.
  const g3 = await tryLoader('embeddedBytesAsync', async () => {
    const { WASM_BYTES } = await import('../out/electron/wasm-bytes.mjs');
    const g = await import(new URL('wrlforge_wasm.js?l3', PKG));
    await g.default({ module_or_path: WASM_BYTES });
    return g;
  });

  const glue = g3 || g1 || g2;
  if (!glue) return;
  const engine = createTextEngine(glue);

  check('release artifact (no negative controls)', () => /negative-controls=\[\]/.test(engine.info) || engine.info);
  check('all three loaders give the same engine identity', () =>
    [g1, g2, g3].every((g) => g && g.engine_info() === engine.info) || JSON.stringify(result.loaders));
  check('repeated initSync is idempotent', () => {
    const a = glue.initSync({ module: new Uint8Array(0) });
    return a === glue.initSync({ module: new Uint8Array(0) });
  });
  check('valid text classes round-trip exactly', () => {
    for (const t of ['#VRML V2.0 utf8\n', 'café €～', 'a\u{1F600}b\u{10FFFF}', '﻿#V', 'a\r\nb\rc\n', 'a\0b', '�', '']) {
      const out = engine.openSession(t).current().text();
      if (units(out) !== units(t)) return `mismatch for ${JSON.stringify(t)}`;
    }
    return true;
  });
  check('lone high / lone low / reversed pair are refused (EENCODING), not replaced', () => {
    const got = ['a\uD83Db', 'a\uDE00b', '\uDE00\uD83D'].map((t) => code(() => engine.openSession(t)));
    return got.every((c) => c === 'EENCODING') || got.join(',');
  });
  check('zero silent substitution over 11^3 combinations', () => {
    const pool = ['', 'a', '�', '\u{1F600}', '\uD83D', '\uDE00', '\r\n', '\r', '﻿', '\0', 'é'];
    for (const a of pool) for (const b of pool) for (const c of pool) {
      const t = a + b + c;
      let out;
      try { out = engine.applyEdits(t, []); } catch (e) {
        if (e.code !== 'EENCODING' || t.isWellFormed()) return `unexpected ${e.code} for ${units(t)}`;
        continue;
      }
      if (!t.isWellFormed() || units(out) !== units(t)) return `substitution for ${units(t)}`;
    }
    return true;
  });
  check('offset conversion and boundary refusal', () => {
    const s = engine.openSession('a\u{1F600}é').current();
    const ok = s.toUtf8(3) === 5 && s.fromUtf8(5) === 3 && s.toUtf8(4) === 7;
    return (ok && code(() => s.toUtf8(2)) === 'EOFFSETSURROGATE' && code(() => s.toUtf8(2 ** 40)) === 'EOFFSETBOUNDS') || 'conversion';
  });
  check('patch application preserves CRLF, lone CR and comments', () =>
    engine.applyEdits('# c\r\nShape {}\r', [{ from: 5, to: 10, insert: 'Group' }]) === '# c\r\nGroup {}\r');
  check('error propagation keeps code and caller indexes', () => {
    try { engine.applyEdits('abc', [{ from: 1, to: 1, insert: 'x' }, { from: 1, to: 1, insert: 'y' }]); } catch (e) {
      return (e instanceof Error && e.code === 'EEDITAMBIGUOUS' && e.index === 1 && e.otherIndex === 0) || `${e.code} ${e.index} ${e.otherIndex}`;
    }
    return 'no error';
  });
  check('surrogate-interior edit refused (EEDITBOUNDARY)', () =>
    code(() => engine.applyEdits('a\u{1F600}', [{ from: 2, to: 2, insert: 'x' }])) === 'EEDITBOUNDARY');
  check('session create / update / stale / foreign / disposed', () => {
    const a = engine.openSession('one');
    const b = engine.openSession('one');
    const a0 = a.current();
    const a1 = a.update(a0, 'two');
    const results = [
      a1.text() === 'two',
      code(() => a0.text()) === 'ESESSIONSTALE',
      code(() => a.propose(b.current(), [])) === 'ESESSIONFOREIGN',
      code(() => a.update({ revision: a1.revision }, 'x')) === 'ESESSIONFOREIGN',
      Reflect.ownKeys(a).length === 0,
    ];
    a.dispose();
    results.push(code(() => a1.text()) === 'ESESSIONDISPOSED', b.current().text() === 'one');
    return results.every(Boolean) || results.join(',');
  });
  check('transaction verify uses full text', () => {
    const s = engine.openSession('ab\r\n');
    const base = s.current();
    return s.verifyTransaction(base, [{ from: 0, to: 1, insert: 'A' }], 'Ab\r\n').verified === true
      && code(() => s.verifyTransaction(base, [{ from: 0, to: 1, insert: 'A' }], 'Ab\n\r')) === 'EVERIFYMISMATCH';
  });

  // RUST-1A: the gate must not rest on a replaceable isWellFormed.
  check('RUST-1A: hostile isWellFormed cannot cause a substitution (facade + raw)', () => {
    const native = Object.getOwnPropertyDescriptor(String.prototype, 'isWellFormed');
    const bad = [];
    try {
      for (const lie of [() => true, () => 1, function inverted() { return !native.value.call(this); }]) {
        Object.defineProperty(String.prototype, 'isWellFormed', { ...native, value: lie });
        for (const t of ['a\uD83Db', '\uDE00', '�\uD83D', 'x�y\uDC00']) {
          for (const [where, fn] of [['openSession', () => engine.openSession(t).current().text()],
            ['applyEdits', () => engine.applyEdits('', [{ from: 0, to: 0, insert: t }])],
            ['raw.apply_edits', () => glue.apply_edits(t, new Float64Array(0), new Float64Array(0), [])]]) {
            // Malformed text must be refused with a code. Under `inverted` the
            // valid base text '' is itself refused first (EENGINE): also safe.
            const c = code(fn);
            const ok = lie.name === 'inverted' ? (c === 'EENCODING' || c === 'EENGINE') : c === 'EENCODING';
            if (!ok) bad.push(`${lie.name || 'lie'} ${where} ${units(t)} -> ${c}`);
          }
        }
        // A genuine U+FFFD is still ordinary text under a method that says true.
        if (lie.name !== 'inverted' && units(engine.applyEdits('a�', [])) !== units('a�')) bad.push('genuine FFFD');
      }
    } finally {
      Object.defineProperty(String.prototype, 'isWellFormed', native);
    }
    return bad.length === 0 || bad.slice(0, 4).join('; ');
  });

  // Performance, in the renderer main thread (where the editor runs today).
  const { INPUTS } = await import('../out/electron/perf-inputs.mjs');
  const wasmExports = glue.initSync({ module: new Uint8Array(0) });
  const t0 = performance.now();
  result.perf = runBench({ engine, glue, wasmExports, inputs: INPUTS });
  result.perf.totalSeconds = +((performance.now() - t0) / 1000).toFixed(1);
  if (performance.memory) {
    result.perf.jsHeapMB = +(performance.memory.usedJSHeapSize / 1048576).toFixed(1);
  }
}

window.addEventListener('error', (e) => { result.fatal = `${e.message}`; });
main()
  .catch((e) => { result.fatal = `${e.name}: ${e.message}`; })
  .finally(() => {
    document.getElementById('out').textContent = JSON.stringify(result, null, 2);
    console.log(`RUST1_RESULT ${JSON.stringify(result)}`);
  });
