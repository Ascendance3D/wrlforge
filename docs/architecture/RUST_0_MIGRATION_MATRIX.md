# RUST-0 — Migration Feasibility Matrix

Companion to `RUST_0_CORE_FEASIBILITY.md`. Base `afb4158`. Evidence is from the
live repository on 2026-10-08. Classes:

- **A** strong Rust candidate: deterministic core logic, clear benefit.
- **B** possible: benefit exists, more evidence needed.
- **C** keep JavaScript: browser or renderer-adjacent, no reason to move.
- **D** defer: too risky, or depends on an unresolved decision.

"Runs in" follows real execution paths, not file names. `src/vrml/*` reaches
the renderer only through the esbuild bundle (`editor-view.js:20,368-374`).

## 1. Document core — `src/vrml/` (27 files, 22,514 LOC, JS CommonJS, no privileges)

| module(s) | responsibility | public interface | callers | runs in | tests | invariants | complexity | class | stage |
|---|---|---|---|---|---|---|---|---|---|
| `edit.js` (594) | WD1.2 span-patch algebra | `applyEdits`, `validateEdits`, `mapOffset`, `mapRange`, `createEdit`, `replaceSpan`, `insertAt`, `removeSpan`, `EDIT_ERROR` | `document-transaction`, `field-edit`, `structure-edit`, WD2 modules | both | `test/vrml/edit*.test.js` | UTF-16 half-open; same-offset inserts refused; insertion-first canonical order; caller-index errors | low | **A** — proven by spike | RUST-2 |
| `tokenizer.js` (349) | lossless tokens + trivia | `tokenize`, `TT`, `KEYWORDS` | `parser`, `language.js`, spike harness | both | `test/vrml/tokenizer*`, `line-endings`, `round-trip` | UTF-16 offsets; column in UTF-16 units; CRLF/CR = one line; multiline strings | low–med | **A** | RUST-3 |
| `parser.js`, `ast.js`, `diagnostics.js`, `messages.js` (~1.4k) | structural parse, recovery, limits | `parse(text)` → `{ast, diagnostics, …}`, `DEFAULT_LIMITS` | `index.js`, everything downstream | both | `test/vrml/parser*`, `fixtures`, corpus 7A1 | recovery tree shape; Blaxxun leniency as recovery; `maxDepth 256`, `maxNodes 100000` | med | **B** → A after RUST-3 differential | RUST-3 |
| `source-map.js` (343) | offset → token/node | `createSourceMap` | WD2, identity | both | `test/vrml/source-map*` | lazy, opt-in; half-open UTF-16 | low | **A** | RUST-3 |
| `node-schema.js` (6,680, generated) | VRML97/X3D schema facts | `nodeSchema`, `isFieldAllowed` | field-edit, semantics, templates | both | `--check` + schema tests | 312 ISO decls, 232 X3D-only fields never in VRML97 export | low (data) | **A** — regenerate as Rust data from the same two inputs | RUST-4 |
| `symbols.js`, `scope-graph.js` (4.6k) | WD1.5 DEF/USE, PROTO, IS, ROUTE | `buildScopeGraph`, resolvers, `REFERENCE_KIND` | WD2 pipeline, findings | both | WD1.5 oracles, corpus sweeps (245,540 ROUTEs / 23,246 IS) | 4.8.4 disjointness; recovered scope withholds all answers; no ranking; EXTERNPROTO asymmetry | **high** | **B** | RUST-4 |
| `node-identity.js`, `document-transaction.js` (1.1k) | WD1.4 Tier 1/2 identity, verified receipts | `createCurrentSelection`, `resolve*Anchor`, `verifyTransaction` | WD2 selection, inspector, picking | renderer (bundled) + tests | `test/vrml/node-identity*` incl. absence scans | never a confidently wrong node; object-identity sessions; NUL-separated scope keys | **high** | **B** — needs §4.4 handle design proven | RUST-5 |
| `containment.js`, `interface-query.js`, `proto-target.js`, `proto-agreement.js`, `compatibility.js`, `semantic-findings.js` (~3.8k) | containment, interfaces, PROTO checks, findings | various | WD2, proto-* | both | `test/vrml/*` | §9 compatibility is a profile, never language | high | **B** | RUST-4 |
| `field-edit.js`, `structure-edit.js`, `node-templates.js`, `simple-object.js` (~1.7k) | WD2 typed field and structure edits → edit sets | `planFieldEdit`, `planInsertObject`, `planDuplicateNode`, `planDeleteNode`, templates | `inspector-edit`, `first-object` | renderer (bundled) | `test/vrml/field-edit`, `structure-edit` (33), `node-templates` (8) | output is an edit set, never a serializer; line-ending aware | med | **B** | RUST-5 |
| `scene-tree.js`, `presentation.js` (~1.3k) | derived scene tree and labels | `buildSceneTree`, `itemById`, `astNodeForItem` | renderer scene tree/inspector | renderer (bundled) | `test/vrml/scene-tree*` | projection only; item ids are not identity | med | **B** (projection boundary design first) | RUST-5 |
| `analyze.js`, `asset-refs.js`, `index.js` | flat advisory index, URL extraction, facade | `analyze`, `extractAssetRefs`, `classifyAssetRefs`, module facade | `language.js`, world-project | both | yes | `analyze` is non-authoritative (VRML040–044 advisory) | low | **B** — retire `analyze` rather than port it once P4 lands | RUST-4 |

## 2. Main-process services

| subsystem | responsibility | interface | callers | privileges | tests | invariants | complexity | class | reason |
|---|---|---|---|---|---|---|---|---|---|
| `src/world-project/` pure: `asset-graph`, `url-fields`, `path-policy`, `profile`, `project-stats`, `image-size`, `package-plan`, `zip-writer` | World analysis, deterministic bundle | `buildPackagePlan`, `buildManifest`, `summarize` | `main.js:15-25` `world:*` | none (pure, injected fs) | `test/world-project` (148), `world-recon` (17) | no texture cap; read-only; byte-stable ZIP; remote never fetched | med | **A** (pure parts) | deterministic, golden-testable |
| `world-project/` I/O: `project-loader`, `session`, `bundle-builder`, `preview-source` | fs scanning, confined writes | `ProjectSession` | `main.js` | fs read/realpath; one write outside project | same | write only outside project; refuse overwrite; re-hash | med | **D** | trust boundary stays in JS (principle 1) |
| `externproto-deps.js`, `src/external-proto/`, `proto-resolution/`, `proto-enrichment/` | EXTERNPROTO retrieval, graph, enrichment | resolvers, status enums | `package-plan`, each other | fs read, realpath (`retrieval.js:99,245`) | 138 + 95 + 68 | containment; no network; status enums | med | **B** | pure parts move with `src/vrml`; retrieval I/O stays JS |
| `src/mall/` (`artifact-size`, `repack`, `repack-paths`), `src/files/` | Mall artifact truth, repack | `measureArtifact`, `repackMall` | `main.js` `mall:*` | fs via `safeSave`, zlib | `test/mall` (23) + size-truth tests | unchanged gzip preserved; 81,920 B refusal; measure real bytes | low | **D** | deflate bytes are encoder-specific; no benefit |
| `validator.js` (304) | Mall rules | `validate(text,sizeInfo)` | `main.js`, tests | zlib (predicted size) | `test/validator.test.js` | must mirror `../new-items/CLAUDE.md` | low | **A** for rules only | pure; keep the zlib size prediction in JS |
| `src/editor/file-io.js`, `path-authorizer.js`, `session*.js`, `recovery-*.js`, `wrl-document.js`, `editor-controller.js`, `mall-edit-flow.js`, `editor-locator.js` | safe save, authorization, sessions, recovery, external editor | `EditorController`, `safeSave`, `authorizeWorldReference` | `main.js:26-30` + `editor:*` IPC | heavy fs, rename, realpath, `child_process` | `test/editor` (289) | verify → backup → atomic rename; format round-trip; no renderer write path | med | **D** | security-critical, reviewed, I/O-bound; Rust adds risk, not value |
| `src/preview/` main side: `wrl-source`, `url-policy`, `texture-base`, `buffer-overlay`, `*-preview-bridge` | preview sources, URL policy, overlay | `readWrlSource`, `isBlockedPreviewUrl`, bridges | `main.js:9-14,31-32` | fs read, gunzip, realpath | `test/preview` (230, shared) | remote blocked; overlay byte-substitution only; proof-gated | med | **B** (`url-policy` pure) / **D** (bridges) | bridges are authorization code |
| `src/settings/`, `src/app/` | settings, window state, file-open arg | `loadSettings` etc. | `main.js` | userData JSON | `test/settings` (47) | read-only settings | low | **D** | trivial, Electron-coupled |
| `main.js` (1,573), `preload.js` (86) | lifecycle, 43 IPC handlers, QA capture server, bridge | `window.vrmlpad` | — | all | many | contextIsolation; narrow IPC | high | **C** | Electron glue; Rust is called from here, never replaces it |

## 3. Renderer

| subsystem | responsibility | runs in | class | reason |
|---|---|---|---|---|
| `src/editor/browser/editor-view.js`, `language.js` | CodeMirror 6 host, highlighting, diagnostics | renderer (bundle) | **C** | CodeMirror is JS; `language.js` becomes a caller of the wasm facade in RUST-3 |
| `inspector-edit.js`, `first-object.js`, `viewport-pick.js`, `scene-selection.js` | WD2 UI glue, selection authority | renderer | **C** | DOM and selection; keep, call Rust for analysis only |
| `command-registry.js`, `panel-registry.js`, `workspace-presets.js`, `ui-state.js`, `src/settings/preferences.js` | commands, panels, zoom, prefs | renderer | **C** | presentation state |
| `src/preview/` renderer side (`bbox-traversal`, `extrusion-bounds`, `fit-math`, `guides`, `preview-scheduler`, `preview-state`, `viewpoint-preserve`, `browser-readiness`) | Mall fit, guides, scheduling | renderer | **C** | reads X_ITE runtime state; `fit-math`/`extrusion-bounds` are pure but tiny |
| `src/preview/xite-pick-adapter.js` | the only X_ITE private-surface module | renderer | **C** | renderer-adjacent by design (WD2-D) |
| `renderer/*.js`, `*.html` | pages, orchestration (`editor.js` 1,448) | renderer | **C** | Shell-2 decomposition first |
| `src/shell/` (1,325) | SHELL-0 contracts | not wired | **D** | design still moving; not performance-relevant |

## 4. Summary

| class | subsystems |
|---|---|
| **A** | `edit.js`; tokenizer; `source-map.js`; `node-schema.js` data; World pure analysis; `validator.js` rules |
| **B** | parser/AST/diagnostics; scope graph + symbols; identity + transactions; WD2 transforms; scene-tree projection; proto-* pure parts; `url-policy` |
| **C** | `main.js`, `preload.js`, all renderer UI, CodeMirror host, X_ITE adapters, renderer preview math |
| **D** | file I/O + authorization + sessions + recovery; Mall deflate/repack; settings/app; `src/shell` |

The **A** set is small on purpose. Most of the value sits in **B**, and **B**
becomes **A** only stage by stage, after its differential gate passes.
