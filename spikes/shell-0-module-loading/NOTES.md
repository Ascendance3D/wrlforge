# SHELL-0 module-loading spike — native ES modules over `file://` vs esbuild entry bundles

Evidence for `docs/architecture/SHELL_0_CONTRACTS_AND_MEASUREMENTS.md` §16–§17.
Isolated: nothing here is loaded by the product, and no production renderer
script changed module system.

```sh
node spikes/shell-0-module-loading/build.js   # esbuild bundles, synthetic graph, asar -> out/
node spikes/shell-0-module-loading/run.js     # one sandboxed Electron process via VisualQaRunner
```

`out/` (gitignored, regenerable) receives `BUILD.json` and `RESULTS.json`.

## Set-up

- `main.js` opens one hidden `BrowserWindow` with WRL Forge's renderer
  settings: `contextIsolation: true`, `nodeIntegration: false`, Electron's
  default sandbox (the run is **sandboxed**; no `--no-sandbox`), and pages
  loaded with `loadFile`. It speaks the capture-server protocol, so
  `qa/visual-qa/runner.js` drives it (launch cap 1, PID accounting, leak
  check) under the visual-QA lock and the workspace guard.
- Every page carries a `<meta>` CSP with the editor page's `script-src 'self'
  file: 'wasm-unsafe-eval'` (one page uses a strict `script-src 'self'`).
- `app/probe.js` (classic) records order, window- and element-level errors,
  unhandled rejections and CSP violations; main also records console output.
- Timing graph: 42 files, ~320–337 KB, entry → 8 features → 4 leaves each +
  a shared util. Every function is referenced at load, so the bundle cannot
  tree-shake the comparison away. That is close to the editor page's
  first-party load today (27 classic tags, 332 KB). Each strategy loaded 15
  times in rotating order in the same process.

## Results (Electron 41.7.1, Linux 7.0, sandboxed)

| Probe | Result |
|---|---|
| `type="module"` from `file://` | loads and runs |
| Relative static imports, `.mjs`, dynamic `import()`, top-level `await` | all work |
| Origin | `self.origin` / `location.origin` = `"file://"`; `import.meta.url` is the real file URL |
| CSP `script-src 'self' file:` (editor page) | allowed, no violation |
| CSP `script-src 'self'` only | allowed, no violation |
| Inline `<script type="module">` | blocked (`script-src-elem`), console error — same as classic inline |
| Order with classic `defer` | document order: `classic-defer-1`, module, `classic-defer-2` |
| Module scope | top-level names stay module-scoped; no `window` leak |
| Top-level throw | `error` event with file, line, column; console error; stack shows the source file and line |
| Missing import | **only an element-level `error` event on the `<script>`; no console message, no window error** |
| Bare specifier (`import 'x_ite'`) | clear `TypeError: Failed to resolve module specifier` |
| Shared dual-format `src/` module, side-effect import | works (publishes its `window.*` global) |
| Same module imported **by name** | `SyntaxError: … does not provide an export named …` — CommonJS `src/` modules have no ES exports |
| From inside an `asar` (release-like packaging) | ESM graph and bundle both load and run |
| esbuild IIFE bundle (`chrome122`, linked sourcemap) | runs under the same CSP, no violation; `Error.stack` points at the bundle line (the map is applied by DevTools, not by V8) |

| 42-file graph, load → all executed | median | p10–p90 |
|---|---|---|
| classic `defer` tags (today) | 32.4 ms | 31.1–33.6 |
| native ESM | 33.5 ms | 30.8–34.3 |
| **one esbuild entry bundle** | **11.6 ms** | 10.9–13.5 |

| esbuild build (`BUILD.json`) | |
|---|---|
| 42-file graph → 1 file | 15 ms, 337 KB (+661 KB map) |
| Determinism | byte-identical across three builds (SHA-256) |

## Reading

- Native ESM over `file://` is **reliable** in Electron 41 under the product's
  CSP and sandbox. It is a viable module system.
- It does **not** fit WRL Forge's shared code: `src/` is CommonJS so main and
  `node:test` can `require` it, and a module cannot import a CommonJS file by
  name. Native ESM would force `src/` to become dual-format ES modules, or
  keep the `window.*` globals the shell is meant to retire.
- A missing import fails silently in the console, which is a worse developer
  experience than a build error.
- One bundle loads the same graph ~2.9× faster than either 42 separate files
  (about 21 ms saved per page load of an editor-sized graph).

Recommendation recorded in the SHELL-0 document: `ESBUILD_ENTRY_BUNDLES`.
