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
| `src/vrml/tokenizer.js` | `wrlforge-vrml::tokenizer` | MIGRATED | Trivia arrays not kept (no consumer). Positions are `u32`. A leading U+FEFF is signature trivia (Migration-2; deliberate difference, see notes). |
| `src/vrml/parser.js`, `ast.js`, `diagnostics.js` (syntax codes) | `wrlforge-vrml::{parser, ast, diagnostics}` | MIGRATED | Parity: 673/673 files identical (65 fixtures + 608 `new-items/item-categories`). |
| `src/vrml/scene-tree.js` | `wrlforge-vrml::scene::build_scene_tree` | PARTIAL | Same inclusion rules and id format; no PROTO-instance flags, no read-only map API, flat USE scope only. |
| inspector read side (`presentation.js`, `interface-query.js`) | `wrlforge-vrml::scene::inspect` + `field_edit::inspect_node_fields` | PARTIAL | Node items show schema-typed field descriptors; no messages catalog. |
| `src/vrml/analyze.js` (VRML040–044) | — | PENDING | Advisory semantic diagnostics not ported. |
| `src/vrml/symbols.js`, `scope-graph.js` (WD1.5) | — | PENDING | Largest remaining port (~4,600 lines). |
| `src/vrml/edit.js` (WD1.2) | `wrlforge-text::edit` (RUST-1) | MIGRATED | Used by `wrlforge-document` for every edit. |
| `src/vrml/source-map.js` | `wrlforge-text` offsets + `ViewMap` | PARTIAL | Offset↔token lookup not exposed. |
| `src/vrml/node-identity.js`, `document-transaction.js` (WD1.4) | span proof in `field_edit` + `wrlforge-document::{apply_source_transaction, map_span}` | PARTIAL | Edits require exactly one node at the exact span of the current revision, re-proved after re-parse. Selections survive undo/redo only through exact change mapping. No Tier-2 DEF identity. |
| `src/vrml/node-schema.js` | `wrlforge-vrml::node_schema` | MIGRATED | Generated field-for-field from the committed JS schema (`scripts/build-rust-node-schema.js`, `--check`); `scripts/check-rust-node-schema-parity.js` proves value equality (54 nodes, 544 fields, 10 classes). |
| `src/vrml/field-edit.js` (WD2-B) | `wrlforge-vrml::field_edit` | MIGRATED | Same nine types, reason ids, lexical gates and round-trip proof. Stricter: nodes in PROTO/EXTERNPROTO/interface scope refuse; SFString line breaks refuse. |
| `structure-edit.js`, `inspector-edit.js`, `first-object.js`, `simple-object.js`, `node-templates.js` | — | PENDING | No structural editing yet. |
| `compatibility.js`, `semantic-findings.js`, `messages.js`, `containment.js`, `proto-*.js`, `asset-refs.js` | — | PENDING | |
| `src/editor/wrl-document.js` | `wrlforge-document::Document` | MIGRATED | One canonical buffer; dirty, revision, undo/redo are Rust-owned. |
| `src/editor/file-io.js` | `src-tauri/src/files.rs` | MIGRATED | Same 7-step order. Stricter: refuses invalid UTF-8; exact byte stamp instead of SHA-1. |
| `src/files/vrml-file.js` (`isGzip`), `backups.js`, `src/preview/wrl-source.js` | `files.rs` | MIGRATED | `editPathFor` (Mall `.edit.wrl`) not ported. |
| `src/editor/session.js`, `session-store.js`, `editor-controller.js`, `path-authorizer.js` | `src-tauri/src/service.rs` + `commands.rs` | PARTIAL | Rust-owned sessions; paths only from native dialogs or the launch argument. No multi-document UI. No World-graph authorization. |
| `src/editor/language.js` + CodeMirror 6 | `<textarea>` + `ui/src/editor.rs` | PARTIAL | No syntax highlighting, no gutter, no folding. CodeMirror is NOT used. |
| `src/editor/recovery-store.js`, `recovery-controller.js` | — | PENDING | No crash recovery. |
| `src/editor/ui-state.js` (zoom, themes) | `protocol::theme` + `ui/src/theme.rs` + `ui/static/themes.css` | PARTIAL | UI-THEME-1: three built-in Tokyo Night themes (`tokyo-night` default, `tokyo-night-storm`, `tokyo-night-light`), toolbar selector, persisted. No zoom, High Contrast or Follow System yet. |
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
| `src/settings/*` | `src-tauri/src/settings.rs` | PARTIAL | `settings.json` in Tauri's app config dir: `{"schemaVersion":1,"themeId":…}` only. No window state. |
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

* **Themes (UI-THEME-1).** `themes.css` holds every palette value; `style.css`
  reads only semantic `--wf-*` tokens. `REQUIRED_TOKENS` in
  `protocol/src/theme.rs` is the contract: each theme must define exactly that
  set, `style.css` may hold no raw colour, and listed text / control pairs must
  meet WCAG AA (`cargo test -p wrlforge-desktop-protocol`). `--wf-syntax-*`
  and `--wf-axis-*` are reserved and unused: there is no syntax highlighting
  and no transform overlay yet. Switching sets `<html data-theme>` in the same
  event turn and persists through `theme_set`; it never touches the document,
  history, selection, Inspector or X_ITE scene. The body stays hidden until the
  persisted theme is applied (1.5 s fallback), so there is no flash of the
  wrong theme; the CSS fallback is Tokyo Night. Settings writes are temp +
  fsync + rename; corrupt, unknown, wrong-version or oversized files fall back
  to Tokyo Night with a visible notice. Smoke: `./smoke.sh --headless --theme
  <final-id> [--theme-expect id] [--theme-notice] [--theme-save-fails]`; it
  always uses a temporary `--config-dir`.

* **BOM (fixed in Rust, Migration-2).** The JS tokenizer reads a leading
  U+FEFF as an identifier (`VRML001` + `VRML020` and a bogus node), which makes
  every BOM file read-only for field editing. The Rust tokenizer now skips it as
  signature trivia while every span still counts it, so BOM files parse cleanly
  and stay byte-exact. The JS tokenizer is unchanged. The preview still strips a
  leading BOM only from the text it hands to X_ITE.
* **Writable Inspector (Migration-2).** `doc_edit_field` names the node by its
  Scene Tree id and the revision the Inspector was built from. Rust refuses a
  stale revision, re-finds exactly one node at that span, plans token-span
  edits, re-parses to prove the same node and value, and applies the set as one
  undo step whose result must equal the planned text. The UI sends raw text and
  adopts only Rust's reply. Verified headless (Xvfb) on
  `new-items/item-categories/decorative/velvet-thornwing.wrl` copies in gzip,
  LF, BOM + CRLF and lone-CR forms: `./smoke.sh --headless --inspector`.
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
