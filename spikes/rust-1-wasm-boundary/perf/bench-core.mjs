// SPDX-License-Identifier: GPL-3.0-or-later
// RUST-1 boundary benchmark core. Environment-neutral: Node and the Electron
// renderer both call `runBench`. Every Rust timing goes THROUGH the facade and
// the wasm-bindgen glue, so it includes UTF-16 validation, the JS->wasm copy
// and conversion, and the wasm->JS result copy. Nothing here is a
// Rust-internal number.
//
// inputs: [{ name, text }]; jsEdit: optional src/vrml/edit.js (Node only) for
// the one equivalent JavaScript operation (applyEdits).

export const WARMUP = 20;
export const ITERATIONS = 100;

function stats(samples) {
  const s = [...samples].sort((a, b) => a - b);
  const q = (p) => s[Math.min(s.length - 1, Math.ceil(p * s.length) - 1)];
  const r = (v) => Math.round(v * 1000) / 1000;
  return { median: r(q(0.5)), p95: r(q(0.95)), max: r(s[s.length - 1]), n: s.length };
}

export function time(fn, { warmup = WARMUP, iterations = ITERATIONS } = {}) {
  for (let i = 0; i < warmup; i += 1) fn();
  const out = [];
  for (let i = 0; i < iterations; i += 1) {
    const t0 = performance.now();
    fn();
    out.push(performance.now() - t0);
  }
  return stats(out);
}

export function runBench({ engine, glue, wasmExports, inputs, jsEdit, iterations = ITERATIONS, warmup = WARMUP }) {
  const opts = { warmup, iterations };
  const rows = [];
  for (const { name, text } of inputs) {
    const mid = Math.floor(text.length / 2);
    // Keep the edit on an ASCII boundary so it is valid for both implementations.
    let at = mid;
    while (at < text.length && text.charCodeAt(at) > 0x7f) at += 1;
    const edits = [{ from: at, to: Math.min(text.length, at + 5), insert: 'Group' }];
    const row = { name, utf16Units: text.length, utf8Bytes: new TextEncoder().encode(text).length, ms: {} };

    row.ms.jsIsWellFormed = time(() => text.isWellFormed(), opts);
    // Gate + JS->wasm copy + Rust String (no index), result discarded.
    row.ms.rustCheckText = time(() => glue.check_text(text), opts);
    // Same check via js-sys JsString::is_valid_utf16 (one charCodeAt per unit).
    const unitIters = text.length > 500000 ? { warmup: 2, iterations: 10 } : opts;
    row.ms.rustValidateByUnitsProbe = time(() => glue.probe_is_valid_utf16_by_units(text), unitIters);
    // Session open = gate + copy + Rust String + line index; then dispose.
    row.ms.openSessionAndDispose = time(() => engine.openSession(text).dispose(), opts);

    const session = engine.openSession(text);
    const snap = session.current();
    row.ms.offsetToUtf8Mid = time(() => snap.toUtf8(at), opts);
    row.ms.offsetToUtf8End = time(() => snap.toUtf8(text.length), opts);
    row.ms.lineColEnd = time(() => snap.lineCol(text.length), opts);
    // Stateless edit: full text in, full text out.
    row.ms.rustApplyEditsStateless = time(() => engine.applyEdits(text, edits), opts);
    // Session-resident edit: only the edit in, full proposed text out.
    row.ms.rustProposeResident = time(() => session.propose(snap, edits), opts);
    // Pure Rust->JS transfer of the whole text.
    row.ms.rustTextOut = time(() => snap.text(), opts);
    if (jsEdit) row.ms.jsApplyEdits = time(() => jsEdit.applyEdits(text, edits), opts);

    // Exactness of what was measured.
    const r = engine.applyEdits(text, edits);
    row.resultMatchesJs = jsEdit ? r === jsEdit.applyEdits(text, edits) : null;
    row.textOutExact = snap.text() === text;
    if (wasmExports) row.wasmMemoryBytesWithSessionOpen = wasmExports.memory.buffer.byteLength;
    session.dispose();
    rows.push(row);
  }
  return { warmup, iterations, rows };
}
