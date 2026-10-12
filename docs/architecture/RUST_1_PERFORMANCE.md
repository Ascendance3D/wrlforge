# RUST-1 — Boundary Performance

Measured, not estimated. Every Rust number goes **through** the facade and the
wasm-bindgen glue: UTF-16 validation, JS→wasm copy and transcoding, the Rust
work, and the wasm→JS copy back. None is a Rust-internal number. **No
application-speed claim is made, and no X_ITE rendering claim is made.**

## 1. Setup

| item | value |
|---|---|
| CPU | AMD Ryzen 9 5900X (12C/24T) |
| OS | Linux 7.0.0-34-generic x86_64 |
| Rust | rustc 1.95.0 (59807616e 2026-04-14), wasm-bindgen 0.2.129 |
| Node | v24.21.0 (V8 13.6) |
| Electron | 41.7.1 (Chromium 146.0.7680.216), renderer main thread |
| wasm build | release: opt-level 3, LTO, codegen-units 1, stripped; SHA-256 `a1e99690…10963` |
| method | `performance.now()`, 20 warm-up + 100 measured iterations per op (the per-unit probe at 1.6 MB: 2 + 10) |
| statistics | median / p95 in milliseconds |
| load | load average 0.9–1.6 during Node runs; desktop session idle |
| scripts | `perf/node-bench.mjs` → `out/perf-node.json`; renderer via `electron/run.cjs` → `out/electron-proof.json` |

Inputs (the profiles of `qa/phase-7b-native-editor/perf.js`):

| input | UTF-16 units | UTF-8 bytes |
|---|---|---|
| ~6 KB `test/fixtures/world/valid70/world.wrl` | 6,929 | 6,929 |
| ~327 KB `test/fixtures/oversized.wrl` | 326,887 | 326,887 |
| ~1.6 MB `oversized.wrl` × 5 (in memory) | 1,634,435 | 1,634,435 |
| ~1.6 MB two-byte variant (one `😀` comment per copy) | 1,634,460 | 1,634,470 |

The committed inputs are pure ASCII. V8 stores them as one-byte strings, so
`isWellFormed` is effectively free for them. The two-byte variant measures the
gate's real worst case.

## 2. Initialization (Node)

| op | median |
|---|---|
| cold `initSync` from bytes (compile + instantiate + glue finalize) | 0.18 ms |
| `new WebAssembly.Module` (65,892 B) | 0.03 ms |
| instantiate + finalize | 0.15 ms |

Electron renderer, end-to-end including the module `import()` of the glue: fetch
+ `initSync` 7.2 ms; glue default async 4.4 ms; embedded bytes (base64 decode)
14.5 ms. These are one-time costs.

## 3. Node 24 (median / p95, ms)

| op | 6 KB | 327 KB | 1.6 MB | 1.6 MB two-byte |
|---|---|---|---|---|
| JS `isWellFormed` (native) | 0 / 0 | 0 / 0 | 0 / 0 | 0.71 / 0.72 |
| `check_text` (gate + copy + Rust `String`) | 0.013 / 0.016 | 0.50 / 0.57 | 3.31 / 3.67 | 2.48 / 2.74 |
| probe: `JsString::is_valid_utf16` (per-unit calls) | 0.133 / 0.138 | 5.32 / 6.30 | 15.6 / 16.3 | 15.1 / 16.1 |
| `openSession` + `dispose` (adds line index) | 0.019 / 0.020 | 0.84 / 1.11 | 4.88 / 5.03 | 4.09 / 4.23 |
| `snapshot.toUtf8(mid)` | 0.001 | 0.001 | 0.001 | 0.000 |
| `snapshot.lineCol(end)` | 0.001 | 0.001 | 0.001 | 0.001 |
| Rust `applyEdits` stateless (text in + out) | 0.032 / 0.046 | 1.37 / 1.71 | 7.56 / 8.27 | 6.33 / 7.84 |
| Rust `propose` (text resident; result out) | 0.022 / 0.030 | 0.87 / 1.20 | 3.65 / 3.85 | 3.77 / 5.23 |
| `snapshot.text()` (wasm → JS only) | 0.005 / 0.011 | 0.14 / 0.44 | 0.15 / 0.34 | 0.30 / 1.71 |
| **JS `edit.js` `applyEdits` (equivalent op)** | **0.004 / 0.007** | **0.13 / 0.44** | **0.69 / 0.75** | **1.32 / 1.38** |

Every measured Rust result was compared with the `edit.js` result (equal) and
every `text()` with its input (exact).

## 4. Electron renderer main thread (median / p95, ms)

Chromium coarsens `performance.now()` to about 0.1 ms here, so sub-0.1 ms
values read as 0.

| op | 6 KB | 327 KB | 1.6 MB | 1.6 MB two-byte |
|---|---|---|---|---|
| JS `isWellFormed` | 0 | 0 | 0 | 0.1 / 0.1 |
| `check_text` | 0.1 / 0.2 | 2.4 / 2.5 | 11.8 / 12.5 | 5.2 / 5.7 |
| probe: per-unit validation | 0.1 / 0.2 | 3.2 / 3.4 | 15.7 / 16.3 | 15.8 / 16.3 |
| `openSession` + `dispose` | 0.1 / 0.1 | 2.7 / 3.2 | 13.5 / 14.3 | 6.9 / 7.3 |
| offset conversion / `lineCol` | 0 | 0 | 0 | 0 |
| Rust `applyEdits` stateless | 0.1 / 0.2 | 3.2 / 3.9 | 16.4 / 16.8 | 12.6 / 13.7 |
| Rust `propose` (resident) | 0 / 0.1 | 0.8 / 0.8 | 4.5 / 5.0 | 7.2 / 7.8 |
| `snapshot.text()` | 0 | 0 / 0.1 | 0.9 / 1.0 | 3.5 / 4.3 |

`edit.js` is CommonJS and is not loaded in the isolated renderer page, so no
renderer JS comparison is reported (equivalent-operations rule).

## 5. Memory

Node, all four inputs held as open sessions at once (~3.6 MB of text): wasm
linear memory 1.1 → 12.3 MB; RSS 57.5 → 77.6 MB; V8 heap 7.3 → 12.0 MB. After
`dispose()` the wasm memory stays at 12.3 MB. WebAssembly linear memory never
shrinks; the Rust allocator reuses the freed space. Renderer: wasm memory
1.2 MB (6 KB) → 11.9 MB with the two-byte 1.6 MB session open; JS heap 17.4 MB
after the run. Per open session, the cost is about 1× the UTF-8 text plus the
line table (16 B per line start + 8 B content length). Peak RSS was not
sampled continuously.

## 6. What this means

1. **Rust is slower for the edit itself.** It is 5–11× slower than `edit.js`
   for a stateless edit, because the whole text crosses the boundary twice.
   Offset conversion and line lookup on a resident snapshot are effectively
   free (≤ 1 µs).
2. **The UTF-16 gate is cheap.** It costs 0–0.7 ms at 1.6 MB in Node. The
   per-unit `js-sys` alternative is 1.3–11× slower than the whole native
   check-and-copy path. That confirms the design choice.
3. **UI blocking.** At 1.6 MB, a full-text transfer on the renderer main thread
   (`openSession`, stateless `applyEdits`) takes 7–17 ms. That is close to or
   above one 60 Hz frame (16.7 ms). Doing this on every keystroke would
   compete with typing and X_ITE frames. At 327 KB it takes 2.4–3.9 ms. At
   6 KB it costs nothing measurable. RUST-2 must therefore:
   - keep the text **resident** and send **changes** (CodeMirror transactions)
     rather than whole documents; or
   - run whole-document work off the main thread in a Worker (the CSP already
     has `worker-src 'self' blob:`; not tested in RUST-1); and
   - keep JavaScript `edit.js` as the default for small synchronous edits
     until a measured gain exists.
4. These numbers are not a speed claim and say nothing about parse or analyze
   cost (RUST-3). RUST-0's conclusion stands: correctness and type safety are
   the case for Rust; speed is not.

## 7. Limitations

- One Linux host. macOS 15+ not measured.
- Synthetic 1.6 MB input (a repeated fixture). The private corpus was not
  used.
- Renderer timer resolution is about 0.1 ms.
- The bench runs synchronously in an idle page; it does not measure contention
  with CodeMirror or X_ITE frames.
- Memory snapshots are point samples, not continuous peaks.
