# WD2-C0 — X_ITE viewport picking → exact-source identity (spike)

Research spike. **Not wired into the product.** Report:
[`docs/white-dune-2026/WD2_C0_XITE_PICKING_SPIKE.md`](../../docs/white-dune-2026/WD2_C0_XITE_PICKING_SPIKE.md).

## Run

```bash
# from the repository root, after npm ci
node spikes/wd2-c0-xite-picking/probe.js          # Electron (xvfb-run + SwiftShader) + grading
node spikes/wd2-c0-xite-picking/probe.js --grade-only   # re-grade existing out/raw-*.json
node --test spikes/wd2-c0-xite-picking/spike.test.js
```

Requires `xvfb-run` (Linux). Each Electron launch gets a fresh
`--user-data-dir` under the OS temp dir, deleted afterwards; `~/.config/wrl-forge`
is never touched. Output (`out/`, gitignored, regenerable):

| file | content |
|---|---|
| `out/PICKING_MATRIX.json` | every graded click: truth, runtime hit summary, mapping status, WRONG flag; reload, edit, coordinate, sensor, anchor and perf sections |
| `out/raw-main.json`, `out/raw-dpr2.json` | raw harness records (harness labels only, no absolute paths) |
| `out/edit-job.json` | the source-edit test's before/after text and oracle spans |
| `out/P*.wrl` | the generated fixtures, for inspection |

## Layout

| path | role |
|---|---|
| `fixtures.js` | fixture builder **and oracle**: composes VRML by concatenation and records each occurrence's exact span as it writes it. Requires neither `mapping.js` nor `src/` (tested). |
| `grade.js` | compares a mapping result with the oracle (span equality only). |
| `mapping.js` | the **candidate mapping** under test: hit → exact source occurrence or refusal. Pure. |
| `browser/harness.js` | X_ITE side: parse-time provenance hook, per-generation `WeakMap`, `touch`/`getHit`, runtime parent graph. |
| `browser/index.html` | spike page (local X_ITE, strict CSP, no remote origin). |
| `electron/main.js` | driver: `contextIsolation` on, `nodeIntegration` off, sandbox, no preload, no IPC; read-only `wd2c0:` scheme. |
| `probe.js` | runner + grader → `out/PICKING_MATRIX.json`. |
| `spike.test.js` | independence, no-fallback source scans, synthetic refusal cases, measured-matrix gate. |

Fixture truth is never derived from X_ITE or the WRL Forge parser. No
identifier, comment or node is added to any VRML text for picking's sake.
