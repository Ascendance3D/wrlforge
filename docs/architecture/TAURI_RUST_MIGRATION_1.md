# TAURI-RUST-MIGRATION-1 — tracking

Lane: `feature/tauri-rust-migration-1`, stacked on PR #130
(`architecture/rust-1-wasm-boundary`, head
`63f668038c4c80ae4f853e9a7076021cba15c6ba`). The uncommitted RUST-1A1
corrections to PR #130 are NOT part of this branch; they await their own
decision. Nothing in this lane depends on them.

The Electron application is **unchanged** and stays the behavior reference
and recovery path. The new application lives in `apps/desktop-tauri/` and does
not load Electron, Node.js, `main.js`, `preload.js` or `renderer/`.

## Status key

| status | meaning |
|---|---|
| MIGRATED | the Tauri app's behavior is owned by Rust and tested; the JS module is no longer used by the Tauri app (the Electron app still uses it) |
| PARTIAL | a Rust replacement exists for part of the behavior |
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
| `src/editor/language.js` + CodeMirror 6 | `wrlforge-vrml::highlight` + `protocol::syntax` + `<textarea>` with `ui/src/syntax.rs` color layer | PARTIAL | UI-SYNTAX-1: parser-derived syntax colors and diagnostic underlines from the same parse and revision as the Diagnostics panel. Highlight parity with `language.js`: 65/65 committed fixtures, 328/328 approved real items. No gutter, no folding, no outline pane. CodeMirror is NOT used. |
| `src/editor/recovery-store.js`, `recovery-controller.js` | — | PENDING | No crash recovery. |
| `src/editor/ui-state.js` (zoom, themes) | `protocol::theme` + `ui/src/theme.rs` + `ui/static/themes.css` | PARTIAL | UI-THEME-1: three built-in Tokyo Night themes (`tokyo-night` default, `tokyo-night-storm`, `tokyo-night-light`), toolbar selector, persisted. No zoom, High Contrast or Follow System yet. |
| `src/editor/command-registry.js`, `panel-registry.js`, `workspace-presets.js`, `src/shell/*` | — | PENDING | Fixed layout; toolbar and shortcuts only. |
| `src/editor/scene-selection.js` | `ui/src/ui.rs` `adopt_selection` (Scene Tree and viewport) | PARTIAL | One selection authority for tree and viewport clicks. No multi-selection. |
| `src/editor/viewport-pick.js` | `wrlforge-vrml::pick` + `service::pick` (`doc_pick`) | MIGRATED | VISUAL-2. Same steps, statuses and reason ids; promotion also covers Cylinder and Cone (VISUAL-1 creates them). |
| `src/preview/xite-pick-adapter.js` | the same file, copied verbatim into `dist/` by `build-ui.sh` | REUSED | VISUAL-2. The one file with private X_ITE picking access; `preview-adapter.js` uses its public surface only. VISUAL-3A1 adds three span-binding methods (`snapshotSpan`, `nodeAt`, `spansOf`) on the same generations; the seven WD2-D methods are unchanged. |
| — (no JS equivalent) | `wrlforge-vrml::manipulate` + `protocol::gizmo` + `ui/src/gizmo.rs` + `ui/static/xite-gizmo-adapter.js` | NEW | VISUAL-3A translation gizmo (Move tool). Top-level Transforms only. Runtime binding by proven provenance (VISUAL-3A1, `wrlforge_vrml::pick::resolve_node`). |
| `src/preview/viewpoint-preserve.js` (DEF / description / index matching) | `ui/src/camera.rs` + `Document::changes_since` + `doc_preview_carry` + the camera-carry part of `xite-gizmo-adapter.js` | REPLACED (stricter) | VISUAL-3A1. Restores the user's camera only onto a PROVEN same viewpoint (provenance span carried through the exact logged changes, or the layer default); never by name, description or index. |
| `src/editor/editor-locator.js` (VSCodium) | — | PENDING | Optional integration. |
| `src/preview/preview-scheduler.js` | `ui/src/ui.rs` (700 ms debounce) | MIGRATED | Skips revisions already on screen. |
| `src/preview/preview-state.js` | adapter keeps the last valid scene | PARTIAL | No explicit state machine. |
| `src/preview/texture-base.js` (relative textures) | `src-tauri/src/resources.rs` | PORTED (TEXTURE-LOCAL-1) | Read-only `wrlres` scheme; per-generation token bound to the Rust session; image files inside the document folder only; 32 MiB cap; static `ImageTexture` URL check → preview warnings. |
| `src/preview/buffer-overlay.js`, `mall-preview-bridge.js`, `world-preview-bridge.js`, `url-policy.js` | — | PENDING | Preview receives text plus a Rust-issued resource base; `Inline`/movie/audio resources are not served; CSP blocks remote origins. |
| `fit-math.js`, `extrusion-bounds.js`, `bbox-traversal.js`, `guides.js`, `viewpoint-preserve.js` | — | PENDING | Mall Fit preview not ported. |
| X_ITE 15.1.10 (renderer) | `ui/static/preview-adapter.js` (JS) | BLOCKED | X_ITE stays as the temporary renderer; a custom Rust renderer is the approved direction but not started (see below and [PRODUCT_VISION](../PRODUCT_VISION.md)). |
| `validator.js`, `src/mall/*` | — | PENDING | Mall Item profile (80 KiB cap, rules, repack) not ported. `safe_save` already has the `max_bytes` ceiling. |
| `src/world-project/*` | — | PENDING | Scanner, asset graph, preview scheme, ZIP bundle not ported (`zip-writer` should use `flate2`). |
| `src/external-proto/*`, `src/proto-resolution/*`, `src/proto-enrichment/*` | — | PENDING | |
| `src/settings/*` | `src-tauri/src/settings.rs` | PARTIAL | `settings.json` in Tauri's app config dir: `{"schemaVersion":1,"themeId":…}` only. No window state. |
| `main.js`, `preload.js` | `src-tauri/src/lib.rs`, Tauri capabilities | PARTIAL | Only the editor-lane IPC exists. |

## JavaScript still in the Tauri application

| item | why | removal path |
|---|---|---|
| X_ITE 15.1.10 (`vendor/x_ite`, MIT) | It is the temporary 3D renderer and is written in JavaScript. | The owner has approved a future custom Rust VRML97/X3D renderer as the direction (see [PRODUCT_VISION](../PRODUCT_VISION.md)); X_ITE remains the temporary renderer until that lane is approved and built. Renderer implementation has not started and needs its own lane. |
| `preview-adapter.js` (~150 lines, handwritten) | The narrow bridge from Rust/Wasm to X_ITE: load, generation retire, pick snapshot (plain JSON), read-only pixel probes. | Goes away with X_ITE. |
| `xite-pick-adapter.js` (WD2-D, copied at build time) | Parse provenance and hit snapshots need private X_ITE 15.1.10 surfaces. | Goes away with X_ITE; a Rust renderer reports hits directly. |
| `xite-gizmo-adapter.js` (VISUAL-3A / 3A1, ~290 lines, handwritten) | The gizmo needs X_ITE's camera matrices (private), a temporary rendered translation (public SAI) on the PROVEN runtime node, and the bound viewpoint's user offsets (private) for the camera carry. Plain data out. | Goes away with X_ITE; a Rust renderer owns its camera. |
| `boot.js` (2 lines, handwritten) | Module bootstrap that loads the wasm-bindgen output. | Could be generated; trivial. |
| `wrlforge_ui.js` (generated by wasm-bindgen) | Required Wasm glue. | Not application logic. |
| Tauri `withGlobalTauri` injected script | Tauri's own IPC bridge. | Part of Tauri. |

CodeMirror, esbuild, the Electron runtime and all `node_modules` application
code are **not** used by the Tauri application.

## Notes and findings

* **Themes (UI-THEME-1).** `themes.css` holds every palette value; `style.css`
  reads only semantic `--wf-*` tokens. `REQUIRED_TOKENS` in
  `protocol/src/theme.rs` is the contract: each theme must define exactly that
  set, `style.css` may hold no raw color, and listed text / control pairs must
  meet WCAG AA (`cargo test -p wrlforge-desktop-protocol`). `--wf-syntax-*`
  color the source editor (UI-SYNTAX-1); `--wf-axis-*` and `--wf-gizmo-*`
  color the VISUAL-3A translation gizmo. Switching sets `<html data-theme>` in the same
  event turn and persists through `theme_set`; it never touches the document,
  history, selection, Inspector or X_ITE scene. The body stays hidden until the
  persisted theme is applied (1.5 s fallback), so there is no flash of the
  wrong theme; the CSS fallback is Tokyo Night. Settings writes are temp +
  fsync + rename; corrupt, unknown, wrong-version or oversized files fall back
  to Tokyo Night with a visible notice. Smoke: `./smoke.sh --headless --theme
  <final-id> [--theme-expect id] [--theme-notice] [--theme-save-fails]`; it
  always uses a temporary `--config-dir`.

* **Syntax highlighting (UI-SYNTAX-1).** `wrlforge_vrml::highlight` ports the
  classification of `src/editor/language.js`: lexical classes from the
  tokenizer, identifier roles (node type, field, DEF name, USE / ROUTE
  reference, field type) from exact AST ranges; an identifier the tree does
  not place stays plain. `Service::analyze` derives the spans from the SAME
  parse as the Scene Tree and diagnostics, maps them through `ViewMap`, and
  sends them `[gap, len, class]`-encoded with `session`, `revision`,
  `view_hash` and `view_len`. The UI applies them only when all four match the
  text on screen. The `<textarea>` stays the only editing control (glyphs
  transparent, caret and selection native); an `aria-hidden`,
  `pointer-events: none` layer behind it paints the same text with identical
  metrics, follows scrolling by transform, and is built from whole-line block
  chunks (≤ 64 lines / ~8K units) so an edit relays out one chunk. Only chunks
  near the viewport carry color spans (budget 16,000 elements). Between an
  edit and the next analysis, spans touching the edit are dropped and later
  spans move over identical characters. A theme change is CSS only. Smoke
  runs 21 syntax steps per file; `WRLFORGE_SMOKE_ALIGN_HOLD_MS` adds
  screenshot pauses that paint the textarea's own glyphs over the layer.
  Parity: `spikes/tauri-rust-migration-1/highlight-parity.sh <dir>`.
  Per-keystroke cost is dominated by the plain `<textarea>` itself on large
  files (see the UI-SYNTAX-1 report); the color layer adds 2–25 ms.

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

### UI-EDITOR-2 — revision-bound selection and the input path

- **A Scene Tree item id is a source span of ONE revision** (`node-<from>-<to>`),
  so it is never used without that revision. A tree click or Enter is
  refused while the rendered tree, the held analysis and the document
  revision differ, or while an edit is in flight; the tree shows
  "updating…" (`data-stale`, `aria-busy`) and is re-analyzed at once.
  Diagnostics and Inspector links obey the same rule.
- **The selection moves only through Rust's exact change mapping**
  (`EditRequest.item` → `EditOutcome::Applied.item`, `map_span` over
  `Document::last_changes`), for typing as well as undo / redo; an
  unprovable selection is cleared, never re-pointed.
- `doc_inspect` takes the revision and answers `Stale` for any other one.
  The UI applies an Inspector reply only for the newest request, the same
  selection and its revision; an older analysis never replaces a newer one.
- `doc_analyze` / `doc_inspect` are async and parse a copy of one revision
  outside the session lock, so edits never queue behind a parse.
- Measured (release, Xvfb): on large files the keystroke cost is the
  WebKitGTK textarea's own relayout of its whole value (cyclone 332 K units
  ≈ 188 ms, buswagon 726 K ≈ 555 ms; unchanged by any CSS tried). WRL Forge's
  own work per keystroke is 11 ms and 57 ms. Going lower needs a
  non-textarea or windowed editing surface — an owner decision.

### VISUAL-1 — first visual creation workflow

- **New World** (`new_document`) makes a session with **no path**: the
  canonical buffer is `#VRML V2.0 utf8\n`, nothing is written and no
  temporary file exists. If the shown document is dirty (Rust's own flag),
  Rust asks with a native confirmation; Cancel leaves it unchanged. Save on an
  untitled world runs Save As; a verified Save As assigns the path, and later
  saves are the normal backup-first, conflict-checked save.
- **Create** (`doc_create`) takes only a closed `Primitive` choice (Box,
  Sphere, Cylinder, Cone) and the base revision. `wrlforge_vrml::create`
  generates a fixed `DEF <Type>_<n> Transform { translation rotation scale
  children [ Shape { Appearance { Material { diffuseColor } } geometry } ] }`.
  It refuses a non-`V2.0 utf8` document, any blocking syntax error, any
  recovered node and a capped parse, so the end of the text is provably top
  level (never inside a node or PROTO). The insert is a pure append in the
  document's dominant line ending; the DEF name is one no identifier token in
  the file spells. The result is re-parsed: old top-level spans unchanged and
  exactly the intended structure at the planned span, or nothing changes. It
  is applied as one `apply_source_transaction` (one undo step); the reply
  names the new Transform's Scene Tree id in the new revision.
- The UI selects that id, shows it in the Scene Tree, Inspector and source
  editor, and loads the preview at once (no 700 ms debounce for a Create).
- Smoke: `./smoke.sh --headless --create` drives New World → Create Box →
  Inspector translation and color → Undo → Redo → New World (Cancel) → Save
  As → Close → Reopen → Create Sphere / Cone / Cylinder → Save → three themes
  → New World (Discard) through the real controls; Rust answers the dialogs
  from its plan and checks the saved file byte for byte, the backup and the
  confirmation count. A read-only pixel probe (`wrlforgePreview.coverage`)
  proves each object is drawn. Release, Xvfb: click → frame drawn
  ≈ 110–230 ms.
- Limits: every object is created at the origin, so a new object can be
  hidden inside an earlier one of the same size (a Cone inside a Cylinder).
  Top-level insertion only; no child insertion into a selected Group yet.

### VISUAL-2 — source-proven viewport picking

- **Select** (viewport toolbar, `aria-pressed`) arms an explicit selection
  mode. A primary-button press + release within 4 px requests a pick; a drag
  never does, and nothing happens on pointer movement. Capture-phase,
  passive listeners only observe the pointer: camera navigation, authored
  sensors and Anchors still receive every event. The pick runs one task
  after the release, because the capture listener runs before X_ITE's own
  release handler (its viewer still counts the button as down, and
  `touch()` answers false). Real X input found this; dispatched DOM events
  did not.
- **Provenance.** Each preview load parses through the WD2-D adapter's
  `parseWithProvenance` (temporary `VRMLParser.prototype.nodeStatement`
  wrapper, owned by one coordinator, restored in `finally`, probe-verified
  before trust). Nothing is written into the source or the preview text.
- **Generation.** A preview generation is (session, revision,
  `preview_hash`, load number). It is retired when a preview load starts,
  when the document changes, when a load fails (the last valid scene stays on
  screen but can never be picked), when a document opens or closes, and when
  the viewport's X_ITE browser is replaced (`pagehide` disposes the adapter).
- **Authority.** The adapter returns plain data only (labels, types,
  context kinds, provenance spans, sensor types). `doc_pick` refuses unless
  the document holds exactly the rendered text (revision and hash), then
  `wrlforge_vrml::pick::resolve` joins every runtime link to exactly one AST
  node by exact span (BOM-shifted), checks type and containment chain by
  identity, refuses USE / several parents / PROTO bodies / Inline scenes /
  sensors and Anchors / syntax errors, and promotes a Shape only to its own
  simple-object Transform. The UI refuses a reply that arrives after the
  generation, document or revision moved, then selects through the same
  `adopt_selection` the Scene Tree uses. Selection never moves the caret,
  edits the source, marks dirty, adds undo, reloads or saves.
- **Smoke.** `./smoke.sh --headless --pick` (needs `node` for the oracle plan,
  `xdotool` for real input). Per theme: New World → Box, Sphere, Cylinder,
  Cone placed by the Inspector → click each in the real viewport → Scene Tree
  + Inspector + unchanged source → empty click → Transform and Material edits
  reach the viewport → stale, late and last-valid-scene clicks refused →
  Undo / Redo / Save As. Then the WD2-C0 oracle matrix (P1–P22 plus CRLF,
  CR, BOM, Unicode forms). Real X pointer clicks are used only inside the
  harness's own Xvfb (environment marker, exact screen geometry, a window of
  this process holding focus); otherwise events are dispatched in-window.
- Limits: USE occurrences are refused permanently under X_ITE 15.1.10 (no
  per-instance hit path). Inline content is not served by the Tauri preview,
  so the GUI shows NO_HIT there; the resolver's `REFUSED_EXTERNAL` is unit
  tested. The harness does not bind non-default Viewpoints (P22 side
  camera clicks are skipped).

### VISUAL-3A — direct 3D translation gizmo

- **Move** (viewport toolbar, `aria-pressed`) is the second viewport tool;
  Select and Move exclude each other. In Move mode a click still selects
  (the VISUAL-2 path) and a drag on empty space still navigates the camera.
  A move starts ONLY with a press on a drawn, usable axis handle.
- **What may move** (`doc_translate_target`, `wrlforge_vrml::manipulate`):
  the selected item must be a standard `Transform` that is a TOP-LEVEL
  statement (its parent frame is the world, so world axes are its
  translation axes) with an explicit, editable SFVec3f `translation`. A DEF
  name is optional; if present it must be defined exactly once in the file,
  never USEd and never a ROUTE destination. Everything else is refused with
  a reason id and a message (`transform-is-not-top-level`,
  `def-name-not-unique`, `transform-is-used-elsewhere`,
  `transform-is-a-route-destination`, `translation-not-explicitly-authored`,
  …); the Inspector stays the exact-value path. A Shape is never guessed into
  a Transform here: the selection is what the pick or the tree proved.
- **Camera and math** (`protocol::gizmo`, pure Rust, unit tested): the
  adapter reports the active viewpoint's view and projection matrices, the
  layer viewport and the canvas rectangle as plain numbers. Handles are
  placed at the Transform's local origin (`T + C + R·SR·S·SR⁻¹·(−C)`) at a
  constant 90 px screen length; a handle whose axis points within ~9° of the
  view direction is drawn disabled. The drag value is the closest point on
  the axis line to the pointer ray (camera frozen at the press), so camera
  angle, viewport size and field of view are all accounted for; a ray within
  ~3° of the axis, a point behind the camera or any non-finite value is
  refused, and a release at such a position cancels. Precision follows the
  screen: `decimals_for(units per px)` (≤ 6 places, no exponent, no `-0`).
- **Live feedback** (`xite-gizmo-adapter.js`): the rendered Transform is
  bound by PROVEN PROVENANCE (VISUAL-3A1, below) -- never by root order,
  name, value or position. During a drag its
  translation field is overridden through the public SAI; the document is
  not touched. The override belongs to the generation: retire / reload /
  close / pagehide restore it. Cancel restores it; a committed release
  releases the binding WITHOUT restoring, so the object stays where it was
  dropped until the new revision's scene replaces it.
- **Commit** (`doc_translate`): one request on release with the session,
  base revision, item, axis and value. Rust re-proves the target, formats the
  token, and plans through `field_edit::plan_field_edit` with the other two
  components re-sent as their exact lexemes (so only one token can change;
  `edit-changes-another-axis` otherwise), then applies ONE
  `apply_source_transaction` (one undo step). A value that rounds to the
  current one is `Unchanged` (no revision); a stale revision is refused. The
  UI selects the returned item at the new revision, re-inspects, and reloads
  the preview at once (no debounce).
- **Cancel** paths: Escape, `pointercancel`, `lostpointercapture`, window
  blur, document open / close / replace, preview reload or generation
  retire, selection change, revision change (any edit), Move turned off.
  Every frame re-checks that the drag still belongs to the same session,
  revision, item and preview generation.
- **Theme.** `--wf-axis-x/y/z` over a `--wf-gizmo-halo` stroke (≥ 3:1 in
  every theme, so the colors read over any rendered background);
  `--wf-gizmo-active` marks the dragged handle, `--wf-gizmo-disabled` +
  dashes + 45% opacity + no pointer input mark a disabled one.
- **Smoke.** `./smoke.sh --headless --move` (needs `xdotool`): every drag
  is REAL X input in the harness's own Xvfb. Tokyo Night: New World → Box →
  real-click select → Move → X+, Y−, zero-distance, Escape, release outside
  the viewport (+ Undo), source edit during a drag, real camera orbit, Z+ and
  X− from oblique views, Undo / Redo of every drag, a Sphere, picking after
  moves, all three themes with the gizmo shown, Save As → Close → Open.
  Storm and Light: Box X− and Y+. Then three Rust-written fixtures
  (BOM + CRLF + comments + Unicode with a nested Transform that must be
  refused; lone CR + Unicode; LF + `0.0` lexemes): select, drag, save; Rust
  checks each file on disk differs by exactly one translation token and has
  one backup holding the original bytes. Mid-drag, pixel probes of the real
  frame prove the object is drawn at the moved position while the source,
  revision, dirty flag and history are unchanged.
- **Measured** (release, Xvfb software GL, 12 drags / ~130 moves per run):
  pointer event → temporary translation applied median 14 ms / p95 25 ms
  (the update runs on the next animation frame); move handler and gizmo
  redraw below the 1 ms `performance.now()` resolution of WebKitGTK;
  `doc_translate` round trip ≤ 1 ms; release → Inspector of the new
  revision 18–21 / 24–27 ms; release → new preview generation drawn and the
  gizmo re-bound 71–72 / 86 ms.
- **Limits.** Top-level Transforms only (nested ones need the parent frame);
  world axes only; one object; no snapping; handles are pointer-only (Move
  and Escape are keyboard-operable, exact values go through the Inspector).
  A committed move reloads the preview; VISUAL-3A1 keeps the user's camera
  across that reload (below). Objects whose DEF is reused, USEd or ROUTEd
  are refused rather than guessed. Over the approved `cars/` + `misc/`
  items (247 files, read-only scan), the gate accepts 301 of 308 top-level
  Transforms; 5 have no authored `translation` and 2 are ROUTE
  destinations.

### VISUAL-3A1 — proven runtime binding and camera preservation

Owner decision: a root-node index does not prove which rendered object a
Transform is (two objects may share name, type, values and appearance), and
the camera must survive every Move.

- **Binding.** `doc_translate_target` returns the Transform's exact UTF-16
  span in the PREVIEW text (`preview_from` / `preview_to`: canonical minus a
  leading BOM). The UI asks the pick adapter (`gizmoLocate` →
  `xite-pick-adapter.snapshotSpan`) which runtime node X_ITE's own parser
  recorded for exactly that span in the generation on screen -- the SAME
  parser-hook provenance VISUAL-2 picking uses (one coordinator, no second
  hook, no prototype left modified). The adapter refuses unless exactly one
  runtime node carries the span; it returns the node's runtime chain as plain
  data. Rust (`doc_translate_prove` → `wrlforge_vrml::pick::resolve_node`,
  the VISUAL-2 chain proof factored out of `resolve`) requires: the session,
  revision and preview hash of the generation equal the document now; the
  item is still a movable target; every runtime link has exactly ONE source
  occurrence and ONE live parent, joins exactly ONE AST node of the same
  type at both span ends, and the runtime chain equals the source
  containment chain up to the scene; the located node is exactly the
  authored node and is top-level. Only then does `gizmoBind` hand the node
  (from `nodeAt`, same generation, same span) to `xite-gizmo-adapter.bind`.
  The root index, the DEF name and the rendered translation remain as
  SECONDARY assertions -- any disagreement refuses, agreement proves
  nothing -- and every temporary translation re-proves the binding (same
  generation, same span, same object). A failed proof disables the handles
  with a reason; the Inspector stays usable; nothing moves and no source
  changes. Runtime objects never cross to Rust.
- **Camera carry.** A reload replaces the X_ITE scene, which binds its
  first viewpoint (or the layer default) with zero user offsets. When a
  generation retires, the adapter captures the bound viewpoint's identity:
  the layer default, or an authored viewpoint with exactly one provenance
  occurrence (its span). Before the next load, Rust maps that span through
  the exact logged changes (`Document::changes_since`, a bounded log of
  every revision's `SpanChange`s; `doc_preview_carry`; any change touching
  the span, at offset 0 or outside the log loses it). The user offsets
  (position, orientation, scale, scale orientation, center of rotation,
  field-of-view scale -- what orbit, pan and zoom change) are read at X_ITE's
  SHUTDOWN of the old world, i.e. the newest user action, and written in a
  microtask right after the new world binds (same task, so before X_ITE
  draws a frame of it) -- only onto the layer default again, or onto the ONE
  runtime node the new generation recorded for the mapped span, and only if
  that is the viewpoint X_ITE bound. The new world's own active layer is
  used (`getWorld().getActiveLayer()`): the browser adopts it only at the
  world's `initialized` event. No authored field, no source text, no dirty
  flag, no undo step. Every world that really replaces the scene -- also
  one from a load superseded after its replacement -- carries the camera
  onto its own proven viewpoint, and the capture follows that viewpoint;
  only the newest load reports and consumes it, so an older camera can
  never land over a newer one. Another session never receives a capture. When the proof fails the camera is not touched and the
  preview status reports `view reset (reason)`.
- **Release-build race (found and fixed).** With overlapping reloads, a
  superseded load could still replace the world; it skipped the restore
  and the next load then refused (`viewpoint-changed-before-reload`), so the
  camera reset. Seen 1 time in the first release run; fixed as above.
- **Fixed on the way.** An older, superseded preview load whose
  `replaceWorld` was aborted called `adapter.abort()` in its error path and
  discarded the NEWEST load's pending generation (nothing pickable or
  movable until the next reload). Only the newest load may abort / retire
  now.
- **Tests.** Rust: `pick::resolve_node` (twins with equal translation and
  geometry never bind as each other; DEF + USE, two live parents, nested,
  chain disagreement, PROTO body, no provenance, type disagreement,
  dangling label, off-by-one span, BOM projection); `changes_since` (move,
  undo, redo, reset, bounded log); service (`translate_prove` on twins,
  stale hash, non-unique snapshot, nested runtime chain; `preview_carry`
  through a move, undo, redo, a crossing change, an unknown or non-current
  revision). JS (`test/preview/xite-pick-binding.test.js`, fake X_ITE): twins
  in LF, BOM + CRLF + Unicode and lone-CR forms; DEF + USE; several runtime
  nodes for one span (two distinct nodes from one statement); replaced /
  retired / superseded / pending generations; a missing parser hook. GUI
  (`--move`, real X input): every commit records the view matrix on EVERY
  animation frame from release until the gizmo re-binds and requires
  max |Δ| ≤ 1e-5 plus a `restored` carry; orbit, pan (middle button), zoom
  (wheel), oblique Z and X, repeated moves, Undo / Redo of every drag, a
  forced reload, three overlapping reloads, a failed load then Undo, a
  window resize after and during a drag, document switch (New World opens
  at its own default view), and a twin fixture (two anonymous Transforms,
  same translation, geometry and material, plus an authored Viewpoint):
  a real click selects one, it moves alone (the other stays drawn and its
  source untouched), a real click at the old place selects the OTHER, it
  moves alone, the authored-viewpoint camera survives each reload.
  `WRONG_RUNTIME_MANIPULATIONS = 0`.
- **Measured** (release, Xvfb software GL, 3 runs × 156/156 steps, ms
  median / p95): provenance locate + Rust proof 1 / 2–3 (n 34 per run);
  gizmo bind < 1 (1 ms timer resolution); scene replacement (parse +
  `replaceWorld`) 42–44 / 55–62; camera carry (SHUTDOWN read → restore
  written) 9 / 10–11; release → final preview bound and gizmo re-bound
  71 / 75–87. Every traced commit: max view-matrix Δ 0.

### Known issues

- Two unexplained smoke timeouts on `advenbed2` (UI-SYNTAX-1 matrix runs).
  Five later back-to-back runs passed in about 20 s each. Not investigated
  further; recorded so it is not lost.
