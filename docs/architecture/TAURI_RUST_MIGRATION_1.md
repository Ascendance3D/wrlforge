# TAURI-RUST-MIGRATION-1 — tracking

Lane: `feature/tauri-rust-migration-1`, stacked on PR #130
(`architecture/rust-1-wasm-boundary`, head
`63f668038c4c80ae4f853e9a7076021cba15c6ba`). The uncommitted RUST-1A1
corrections to PR #130 are NOT part of this branch; they await their own
decision. Nothing in this lane depends on them.

The Electron application is **unchanged** and stays the behaviour reference
and recovery path. The new application lives in `apps/desktop-tauri/` and does
not load Electron, Node.js, `main.js`, `preload.js` or `renderer/`.

## Status key

| status | meaning |
|---|---|
| MIGRATED | the Tauri app's behaviour is owned by Rust and tested; the JS module is no longer used by the Tauri app (the Electron app still uses it) |
| PARTIAL | a Rust replacement exists for part of the behaviour |
| PENDING | no Rust replacement yet; the Tauri app lacks the feature |
| BLOCKED | needs an owner decision before work can proceed |

## Layout

| path | what |
|---|---|
| `crates/wrlforge-text` | RUST-1 text core (UTF-16 offsets, WD1.2 edit algebra). Reused unchanged. |
| `crates/wrlforge-vrml` | NEW. Tokenizer, parser, scene-tree/inspector read projections. std-only. |
| `crates/wrlforge-document` | NEW. Canonical buffer, revisions, undo/redo, editor-view mapping. Depends only on `wrlforge-text`. |
| `apps/desktop-tauri/protocol` | NEW. Serde IPC DTOs shared by backend and UI. |
| `apps/desktop-tauri/src-tauri` | NEW. Tauri 2 backend: file service, session service, commands, smoke harness. |
| `apps/desktop-tauri/ui` | NEW. Leptos 0.8 (CSR) UI compiled to Wasm, plus static shell. |
| `spikes/tauri-rust-migration-1` | NEW. Dev-time JS-vs-Rust parser parity harness (Node is the oracle only). |

`apps/desktop-tauri` is its own Cargo workspace on purpose, so the Tauri and
Leptos trees never re-resolve `crates/Cargo.lock` (RUST-1 pins
`wasm-bindgen =0.2.129`). The two new pure crates joined the `crates/`
workspace; `crates/Cargo.lock` gained only their two path entries.

## Module map

| existing JS module | Rust replacement | status | remaining dependency / gap |
|---|---|---|---|
| `src/vrml/tokenizer.js` | `wrlforge-vrml::tokenizer` | MIGRATED | Trivia arrays not kept (no consumer). Positions are `u32`. |
| `src/vrml/parser.js`, `ast.js`, `diagnostics.js` (syntax codes) | `wrlforge-vrml::{parser, ast, diagnostics}` | MIGRATED | Parity: 673/673 files identical (65 fixtures + 608 `new-items/item-categories`). |
| `src/vrml/scene-tree.js` | `wrlforge-vrml::scene::build_scene_tree` | PARTIAL | Same inclusion rules and id format; no PROTO-instance flags, no read-only map API, flat USE scope only. |
| inspector read side (`presentation.js`, `interface-query.js`) | `wrlforge-vrml::scene::inspect` | PARTIAL | Shows exact source text per field; no schema types, no messages catalog. |
| `src/vrml/analyze.js` (VRML040–044) | — | PENDING | Advisory semantic diagnostics not ported. |
| `src/vrml/symbols.js`, `scope-graph.js` (WD1.5) | — | PENDING | Largest remaining port (~4,600 lines). |
| `src/vrml/edit.js` (WD1.2) | `wrlforge-text::edit` (RUST-1) | MIGRATED | Used by `wrlforge-document` for every edit. |
| `src/vrml/source-map.js` | `wrlforge-text` offsets + `ViewMap` | PARTIAL | Offset↔token lookup not exposed. |
| `src/vrml/node-identity.js`, `document-transaction.js` (WD1.4) | — | PENDING | Selections are by span id only; no Tier-1/Tier-2 identity. |
| `src/vrml/node-schema.js` | — | PENDING | Generate a Rust table from the same two inputs. |
| `field-edit.js`, `structure-edit.js`, `inspector-edit.js`, `first-object.js`, `simple-object.js`, `node-templates.js` | — | PENDING | No visual editing commands yet. |
| `compatibility.js`, `semantic-findings.js`, `messages.js`, `containment.js`, `proto-*.js`, `asset-refs.js` | — | PENDING | |
| `src/editor/wrl-document.js` | `wrlforge-document::Document` | MIGRATED | One canonical buffer; dirty, revision, undo/redo are Rust-owned. |
| `src/editor/file-io.js` | `src-tauri/src/files.rs` | MIGRATED | Same 7-step order. Stricter: refuses invalid UTF-8; exact byte stamp instead of SHA-1. |
| `src/files/vrml-file.js` (`isGzip`), `backups.js`, `src/preview/wrl-source.js` | `files.rs` | MIGRATED | `editPathFor` (Mall `.edit.wrl`) not ported. |
| `src/editor/session.js`, `session-store.js`, `editor-controller.js`, `path-authorizer.js` | `src-tauri/src/service.rs` + `commands.rs` | PARTIAL | Rust-owned sessions; paths only from native dialogs or the launch argument. No multi-document UI. No World-graph authorization. |
| `src/editor/language.js` + CodeMirror 6 | `<textarea>` + `ui/src/editor.rs` | PARTIAL | No syntax highlighting, no gutter, no folding. CodeMirror is NOT used. |
| `src/editor/recovery-store.js`, `recovery-controller.js` | — | PENDING | No crash recovery. |
| `src/editor/ui-state.js` (zoom, themes) | — | PENDING | One dark theme; no zoom or high-contrast theme. |
| `src/editor/command-registry.js`, `panel-registry.js`, `workspace-presets.js`, `src/shell/*` | — | PENDING | Fixed layout; toolbar and shortcuts only. |
| `src/editor/scene-selection.js` | `ui/src/panels.rs` (tree → span select) | PARTIAL | Selection goes one way only (tree → source). |
| `src/editor/viewport-pick.js`, `src/preview/xite-pick-adapter.js` | — | PENDING | No viewport picking. |
| `src/editor/editor-locator.js` (VSCodium) | — | PENDING | Optional integration. |
| `src/preview/preview-scheduler.js` | `ui/src/ui.rs` (700 ms debounce) | MIGRATED | Skips revisions already on screen. |
| `src/preview/preview-state.js` | adapter keeps the last valid scene | PARTIAL | No explicit state machine. |
| `src/preview/buffer-overlay.js`, `mall-preview-bridge.js`, `world-preview-bridge.js`, `texture-base.js`, `url-policy.js` | — | PENDING | Preview receives text only; relative textures are not served; CSP blocks remote origins. |
| `fit-math.js`, `extrusion-bounds.js`, `bbox-traversal.js`, `guides.js`, `viewpoint-preserve.js` | — | PENDING | Mall Fit preview not ported. |
| X_ITE 15.1.10 (renderer) | `ui/static/preview-adapter.js` (JS) | BLOCKED | Renderer replacement needs an owner decision (see below). |
| `validator.js`, `src/mall/*` | — | PENDING | Mall Item profile (80 KiB cap, rules, repack) not ported. `safe_save` already has the `max_bytes` ceiling. |
| `src/world-project/*` | — | PENDING | Scanner, asset graph, preview scheme, ZIP bundle not ported (`zip-writer` should use `flate2`). |
| `src/external-proto/*`, `src/proto-resolution/*`, `src/proto-enrichment/*` | — | PENDING | |
| `src/settings/*` | — | PENDING | No persisted settings or window state. |
| `main.js`, `preload.js` | `src-tauri/src/lib.rs`, Tauri capabilities | PARTIAL | Only the editor-lane IPC exists. |

## JavaScript still in the Tauri application

| item | why | removal path |
|---|---|---|
| X_ITE 15.1.10 (`vendor/x_ite`, MIT) | It is the approved 3D renderer and is written in JavaScript. | Owner decision on a native renderer (FreeWRL, or the MIT `oxideav-vrml` tessellator plus a wgpu viewport). Verify licence, embedding and standards coverage first. |
| `preview-adapter.js` (~50 lines, handwritten) | The narrow bridge from Rust/Wasm to X_ITE. | Goes away with X_ITE. |
| `boot.js` (2 lines, handwritten) | Module bootstrap that loads the wasm-bindgen output. | Could be generated; trivial. |
| `wrlforge_ui.js` (generated by wasm-bindgen) | Required Wasm glue. | Not application logic. |
| Tauri `withGlobalTauri` injected script | Tauri's own IPC bridge. | Part of Tauri. |

CodeMirror, esbuild, the Electron runtime and all `node_modules` application
code are **not** used by the Tauri application.

## Notes and findings

* **BOM parity defect (pre-existing).** A leading U+FEFF is tokenized as an
  identifier by both the JS and the Rust parser (`VRML001` + `VRML020` and a
  bogus node). The Rust port keeps parity; fix both together. The preview strips
  a leading BOM only from the text it hands to X_ITE, because X_ITE rejects it.
* **Editor view.** A `<textarea>` normalizes line breaks, so the UI edits a
  `\n`-only view projection and `wrlforge-document` maps it back to source.
  CRLF, lone CR, mixed endings, BOM and non-ASCII text are preserved exactly
  (unit tests plus the in-window smoke test on a BOM + CRLF + lone-CR file).
* **External-change polling** is stat-first (size + mtime); the save-time
  guard always compares exact bytes.
* **Memory.** On Xvfb, a 2-minute typing session on a 316-line Mall item kept
  the Rust process flat (+0.2 MB) and the WebKit process within ±15 MB of its
  idle level. Parsing the largest `item-categories` file (726 KB) takes 0.03 s
  and peaks at 55 MB RSS.
* **Candidate crates reviewed:** `oxideav-vrml` / `oxideav-x3d` 0.0.1 (MIT).
  Not usable as the document core: no source spans, Latin-1 fallback decode,
  a re-serializing writer, no error recovery. Possibly useful later as a
  tessellator for a native viewport.
