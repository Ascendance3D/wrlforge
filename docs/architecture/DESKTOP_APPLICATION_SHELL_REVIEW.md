# APP-ARCH-0 — Desktop Application Shell Architecture

| | |
|---|---|
| Lane | APP-ARCH-0, research and design only |
| Baseline | `origin/main` `58656fe33cf7d0bd806b0c4c4a7f43f499f7749b` (after PR #126) |
| Status | **Owner-approved architecture direction (2026-10-07).** O1–O6 are recorded as decided in §29. This document authorizes a direction, not an implementation: every lane in §24 and §30 starts only on its own owner GO. |
| Product code changed | none |
| Dependencies added | none |
| Repository rules changed | none. `CLAUDE.md`, `AGENTS.md` and `WD.md` are unchanged by this document (§12). |
| Lanes started | none. SHELL-0, #121, #122, #38, #43 and #50 are untouched. |

Every `file:line` below refers to the baseline. Line numbers are evidence for
review, not a stable API.

**Location.** This document lives in a new `docs/architecture/` directory. It is
not in `docs/ui/` because its scope is wider than UI: it covers the main
process, the preload, all three renderer pages, profiles, the module system and
packaging. `docs/ui/` stays the home for UI-0, UI-C0 and UI-1. The future
shell-contract documents should live here.

**Authorities read first:** `WD.md`, `CLAUDE.md`,
`docs/ui/UI0_COMMAND_WORKSPACE_ARCHITECTURE.md`,
`docs/NATIVE_EDITOR_ARCHITECTURE.md`, and issues #33, #38, #43, #121 and #122.

---

## 1. Executive decision

**The architecture problem is not Electron, and not the lack of a UI
framework.** Five measured problems sit in the structure around them:

1. **Three separate web pages, one window.** Every profile switch is a full
   `loadFile` reload (`main.js:134-140`). Each reload re-parses X_ITE (1.3 MB)
   and, on the editor page, the 1.4 MB CodeMirror bundle and 27 classic
   scripts. Renderer state survives a switch only where main happens to hold it.
2. **No lifecycle contract.**
   - Renderer modules register listeners and subscriptions and then discard
     the unsubscribe handles.
   - No module can be mounted or disposed independently of a page reload.
3. **One central orchestrator.**
   - `renderer/editor.js` (1,448 lines) owns the document session, every
     command registration (`:516-587`), every panel registration (`:591-608`),
     the analysis pipeline, picking glue, Model dispatch, recovery throttling,
     zoom and theme, and about 234 lines of QA hooks (`:1208-1441`).
   - Every future tool would be added here.
4. **The module system is implicit global order.**
   - Three hand-maintained `<script>` lists, of 11, 8 and 27 tags.
   - About 30 `window.*` globals with three naming conventions.
   - Dependencies are expressed only through tag order.
5. **`main.js` is a 1,573-line file.**
   - All 43 IPC handlers are inline.
   - About 610 lines (`:339-948`) are the QA capture server.
   - All state is module-level singletons.

**Decisions (owner-approved 2026-10-07 unless marked otherwise):**

| Question | Decision |
|---|---|
| Electron (O1) | **KEEP WITH MAJOR SHELL REFACTOR.** WRL Forge remains an Electron application. Replacing the shell is not a migration path. The refactor is staged and in place, not a rewrite. |
| Framework (O2) | **KEEP FRAMEWORK-FREE.** No React, Vue, Svelte or other general UI framework. First-party module boundaries plus an explicit contribution/disposable lifecycle contract. |
| Bundling (O3) | **Approved in principle:** the existing esbuild devDependency becomes the one bundler for first-party renderer entrypoints. **Not active yet**: it takes effect only after an approved implementation lane updates the repository rules (§12). |
| Persistent shell (O4) | **One persistent desktop application shell is the target**, reached in stages. The improved multi-page state (Shell-3) is a waypoint, not the final architecture. |
| Path authority (O5) | A future separate security lane, provisionally **SEC-SHELL-0 — Renderer-Supplied Path Authority Audit and Confinement** (§21). No issue is created by this document. |
| #121 (O6) | **`#121 SHOULD WAIT FOR SHELL FOUNDATION`**, where shell foundation means **SHELL-0 only** (§25) |
| Profiles | **Document profiles that contribute capabilities**, not pages. Each profile's rules stay in its own modules (§14). |
| Next lane | **SHELL-0 — Shell Contracts and Measurement** (§24, §30) |
| #122 | Plan fits; proceeds after SHELL-0 defines the command/menu boundary, with three amendments (§26) |
| UI-C0 / UI-1 / WD2-E | UI-C0 stays a separate spike, run when the owner authorizes it. UI-1 follows Shell-2. WD2-E follows its shell/workspace prerequisites (§27). |

The document invariants of `WD.md` and the security invariants are preserved
by every stage. Two **pre-existing findings** are promoted to required work:
- three IPC handlers take a path from the renderer, an **authority-boundary
  finding** that needs its own audit (§7.8, §21);
- closing the window with unsaved edits does not ask first (§6, §24).

---

## 2. Current application map

```text
Electron main (main.js, 1,573 lines, global singletons)
├─ one BrowserWindow (createWindow :291-337)
├─ wrlworld: privileged scheme (:39-42, handler :233)
├─ network guard on defaultSession (:221-225)
├─ single-instance lock, argv / second-instance / open-file → Mall page (:71-185)
├─ window-state persistence (:254-289)
├─ QA capture server, active only under env vars (:339-948)
├─ EditorController + RecoveryController factories (:950-1023)
├─ IPC: mall:* (5), preview:load, shell:revealInFolder, app:goto,
│       world:* (12), editor:* (12), editor:preview* (6), editor:recovery* (5)
└─ src/ services: editor-controller, session, file-io, path-authorizer,
   recovery-*, world-project/*, mall/*, preview bridges, validator.js

preload.js (86 lines)
└─ window.vrmlpad = { 8 flat mall/app methods, .world{12}, .editor{23} }
   invoke-only; no ipcRenderer.on; no event exposure

renderer (one window, three pages; full reload on every switch)
├─ index.html  Mall Item      (11 scripts, renderer.js + preview.js)
├─ world.html  World Project  (8 scripts, world.js + world-preview.js + world-packaging.js)
└─ editor.html Native editor  (27 scripts + the CodeMirror bundle;
                               loads BOTH preview.js and world-preview.js)
   shared on every page: preferences pair, recovery-prompt, x_ite
```

**How it is held together today.**
- **Main is the real application.** It holds the one Mall session
  (`currentSession`, `:56`), the one World session (`worldSession`, `:62`), the
  one editor document (`editorController`, `:80`) and the current page
  (`currentPage`, `:129`).
- **Each renderer page is a view over that state** that re-creates itself on
  load. The editor rehydrates through `editor:describe`. World rehydrates
  through `world:describe`. **Mall cannot rehydrate**, because there is no
  `mall:describe`.
- **Cross-page continuity** is carried by:
  - main-process state;
  - `localStorage` (preferences);
  - `sessionStorage` (`wrlforge.nav.returnFocusId`);
  - the editor's `editor:setText` push on Back
    (`docs/NATIVE_EDITOR_ARCHITECTURE.md:362-364`).

## 3. Current main / preload / renderer boundaries

| Boundary | Fact | Evidence |
|---|---|---|
| Isolation | `contextIsolation: true`, `nodeIntegration: false` | `main.js:303-307` |
| Sandbox | Not set. The Electron 41 default for a renderer is sandboxed, but this is implicit. | `main.js:303-307`; `node_modules/electron` 41.7.1 |
| CSP | `<meta>` per page, copy-pasted three times with drift: `wrlworld:` and `connect-src` differ | `index.html:11`, `world.html:12`, `editor.html:17-35` |
| Navigation guard | No `will-navigate` handler and no `setWindowOpenHandler`. Safe today only because the renderer never supplies a URL. | rg over `main.js` |
| Network | `onBeforeRequest` blocks http/https/ws/ftp | `main.js:221-225` |
| IPC direction | Renderer→main `invoke` only (43 `ipcMain.handle`). Main→renderer only through `executeJavaScript` for desktop open (`:152`) and QA hooks. | `preload.js`; `main.js` |
| Path authority | Editor, World, repack and preview: main-owned with a session id and authorization. **Exceptions:** `mall:openPath` (`:1131`), `mall:check` (`:1228`) and `shell:revealInFolder` (`:1262`) accept a renderer-supplied path. | §7.8 |
| Preferences | Renderer `localStorage` only; no preference IPC. `settings.json` is read by main for `editorCommand` only. | `src/settings/preferences.js`; `main.js:1164` |
| Menu | `Menu` is never imported. The Electron default menu is live (View → Reload, zoom roles, DevTools). | `main.js`; UI-0 §4.10 |

## 4. Current renderer / page map

| | Mall `index.html` | World `world.html` | Editor `editor.html` |
|---|---|---|---|
| Purpose | open/validate/repack a Mall item; Fit preview | read-only world asset resolver, preview, packaging | the native editor (Code/Model) for any profile |
| HTML / inline CSS | 237 / ~90 lines | 273 / ~105 lines | 625 / ~340 lines |
| Main scripts | `renderer.js` 358, `preview.js` 427 | `world.js` 441, `world-preview.js` 445, `world-packaging.js` 183 | `editor.js` 1,448, `editor-preview.js` 585, `scene-inspector.js` 566, `scene-tree.js` 280, `model-workspace.js` 239, `command-bindings.js` 94 |
| State owner | `let state`, `let pollTimer` (`renderer.js:25-26`) | `let current`, `let filter` (`world.js:31-32`); main `worldSession` | `const S` (`editor.js:82`), `St` (`editor-preview.js:61`); main `editorController` |
| Lifecycle | top-level body; `mall:check` every 3 s (`renderer.js:178-190`), never torn down | async IIFE: recovery prompt → `world:describe` → rescan | `DOMContentLoaded` → `init()` (`editor.js:1083`) |
| Commands | 10 direct `addEventListener` | 11 direct `addEventListener` | Command Registry (about 38 commands) plus 8 direct listeners |
| IPC | `vrmlpad.*` flat, `editor.openMall`, `goto` | `vrmlpad.world.*`, `editor.openWorld*`, `goto` | `vrmlpad.editor.*` (23 methods), `goto` |
| Navigation in/out | initial page; `goto('world')`; `editor.openMall()` then `goto('editor')` | `editor.openWorld*()` then `goto('editor')`; `goto('mall')` | Back/Close: `setText`/`close`, then `goto(origin)` (`editor.js:456-487`) |
| State after returning | **lost** (main still holds `currentSession`; no describe) | **rehydrated** via `world:describe` | rehydrated via `editor:describe` |
| QA hook | `__wrlForgeApplyOpen` | `__wrlForgeApplyWorld`, `__wrlForgeResetWorld` | `__wrlEditor` |

**Shared modules:**
- `preferences.js` + `src/settings/preferences.js`
- `recovery-prompt.js`
- `x_ite`
- the `src/preview/*` math

**Duplicated code:**
- the `restoreReturnFocus` IIFE, in `renderer.js` and `world.js`;
- the `prefsBtn` wiring, ×3;
- header and toolbar markup, ×3;
- inline `<style>` blocks, ×3. There is no shared stylesheet and no `<link>`.
- the CSP, ×3;
- three preview controllers. `editor-preview.js` orchestrates the other two by
  context.

**How far WRL Forge acts as separate web pages:**
- *Fully separate:* chrome, CSS, CSP, command wiring and state lifetime.
- *Shared:* only main-process state, preferences, the recovery prompt and the
  pure `src/` modules.
- The editor page alone is already structured like an application, with a
  registry, panels and workspaces. It also already embeds both profile preview
  engines (`editor.html` has two `<x3d-canvas>` elements, `:156`).

## 5. Current state authorities

| State | Class | Owner | Notes |
|---|---|---|---|
| Source text | document | CodeMirror `EditorState` (`editor-view.js:226`) | canonical (`WD.md` §2) |
| Undo history | document | CodeMirror `history()` (`editor-view.js:209`) | `setDoc` resets it (`:286`) |
| Baseline | document | `S.baseline` (`editor.js:89`); main `EditorSession` | dirty is derived (`:137`) |
| Path, format, gzip, authorization | session | **main** `EditorSession` / `EditorController` | renderer mirrors in `S` (`:86-88`) |
| `sessionId` | session | main (monotonic) and `S.sessionId` | stale ids are rejected (`editor-controller.js:186`) |
| Profile / context | session | `S.context`; CodeMirror `profile` fixed at mount (`:1182`) | the profile expression is duplicated at `:153`, `:1154` and `:1182` |
| Save lifecycle | session | `S.saving`, `S.saveState` | |
| Analysis products | derived | `S.sceneTree`, `S.findings`, `S.analysisSession`, `S.diagnostics`, `S.outline`… | rebuilt per analysis (`:654-747`) |
| Selection | interaction | `sceneSelection` (`editor.js:130`) | **one authority**; written by tree, proven pick, re-anchor, Model ops |
| Workspace mode | workspace UI | `S.workspaceMode`; pref `workspaceMode`; class `workspace-model` | one writer (`setWorkspaceMode`, `:957`) |
| Source open, damaged | workspace UI | `S.sourceOpen`, `S.damaged` + `.source-open` class | |
| Panel visibility | panel | derived from geometry (`panel-registry.js`) | by design (UI-0 §11.2) |
| Preview layout / split | preference + panel | `St.layout`, `St.split`; prefs; three `layout-*` classes; the select value | four copies |
| Zoom, theme | preference | `WrlPreferences`; `S.zoom`; the CodeMirror handle | loop avoided only by a "no-op if unchanged" guard (`:1105-1111`) |
| Preview state machine | preview | `St.sm` (`editor-preview.js:67`); last-valid scene in X_ITE controllers and main overlay | |
| Pick state | interaction | `St.pick*`, `S.lastViewportPick`, `S.pickStatus*` | |
| Recovery | session | renderer throttle (`editor.js:146-163`, 1.5 s); main `RecoveryController` debounce | |
| Mall session | session | main `currentSession` + renderer `state` | renderer copy lost on navigation |
| World session | session | main `worldSession` + renderer `current` | renderer copy rebuilt from main |

**Duplicated or poorly owned state (evidence):**
1. Preview layout is held in four places, and the Preferences dialog **pushes**
   into the preview by direct call (`preferences.js:115`). The preview never
   subscribes.
2. Zoom and theme are cross-written between `S`, preferences and the
   CodeMirror handle.
3. The DOM is read back as truth:
   - `clearPickStatus` reads `els.modelStatus.textContent` (`editor.js:897`);
   - `modalVisible` reads `classList.contains('show')`.
4. Session metadata is copied from `describe()` in two places (`:1147-1155`,
   `:1176-1182`).
5. Mall session state exists in main and in the renderer, but only the renderer
   copy drives the UI, and it dies on navigation.
6. `currentPage` in main can drift from the real page. It is updated only by
   `gotoPage`, not by a renderer self-reload.

None of these duplicates canonical document data. **The `WD.md` invariants hold
today.** The debt is in UI, session and lifecycle state.

## 6. Current lifecycle

```text
launch
  whenReady (main.js:1025) → network guard, wrlworld protocol, controllers
  → createWindow → loadFile index.html (:337)  [Mall page always first]
  → Mall page: recovery prompt (unawaited, renderer.js:357)

open profile (Mall)
  mall:open → dialog → openMallFile: writes .edit.wrl, sets currentSession
  → renderer state = {mallPath, editFile}; 3 s mall:check poll starts

open document → enter editor
  editor.openMall → EditorController opens session (sessionId++)
  → app:goto('editor') → loadFile editor.html          [FULL RELOAD]
     re-parse x_ite 1.3 MB + bundle 1.4 MB + 27 scripts
  → init: register commands/panels → prefs → views → recovery prompt
     → editor:describe{includeText} → mountEditor → EP().start (mall/world only)

switch Code/Model
  setWorkspaceMode → applyWorkspace: classes, armPicking(preset),
  maybe one render for provenance, modelWorkspace.refresh, registry.invalidate
  (no reload; source/history/selection unchanged)

preview reload
  CodeMirror change → EP().onEdit → 700 ms scheduler (manual only > 1 MiB)
  → editor:previewLoad(sessionId, text, version) → main overlay → X_ITE

save
  doSave → editor:save(sessionId, text) → safeSave (verify, backup, atomic
  rename, or preserve) → S.baseline = text; main clears recovery

close
  confirmUnsaved → EP().stop → flush recovery → editor:close
  → app:goto(origin) → loadFile index.html / world.html  [FULL RELOAD]
  → Mall: renderer state null, page is blank although main still holds
    currentSession; World: rehydrates from world:describe
```

**Lifecycle findings:**
- **Page reloads.** There are two full reloads per edit round-trip, one into
  the editor and one out. The startup cost is unmeasured. The repository has no
  startup or process-memory figure; only a 45.2 MB stable renderer heap for the
  editor page (`docs/ACCESSIBILITY_PERFORMANCE.md:225-228`).
- **Lost state.** The Mall item is lost on return. Preview camera, scroll
  positions, panel focus and the Mall poll are lost on every switch.
- **Repeated initialization.** Every page re-runs preferences, the recovery
  prompt, X_ITE startup and theme/zoom application.
- **Implicit global state.**
  - About 30 `window.*` globals.
  - `editor.js` reads 11 of them at load (`:9-22`).
  - `editor.js` and `editor-preview.js` reach each other through globals
    (`EP()`) and a DOM `CustomEvent` (`wrl-editor-preview-state`).
- **Cleanup risks.**
  - `installKeyboard` and `bindControls` unsubscribes are discarded
    (`wireCommands`, `:610`).
  - The `WrlPreferences.subscribe` (`:1094`) and both
    `sceneSelection.subscribe` calls (`:618`, `:955`) are never undone.
  - This is harmless only because a page reload is the sole teardown. **A
    persistent shell makes every one of these a leak.**
- **Window close with unsaved edits (required work, §24).**
  - Nothing prompts. No `beforeunload` sets `returnValue`, and main's `close`
    does not `preventDefault`.
  - The unsaved buffer survives only through the recovery snapshot, which
    covers the last 1.5 s throttle window, and is restored through the
    recovery prompt.
  - **A recovery snapshot is not a close workflow.** Recovery exists for
    abnormal termination. A normal close of a dirty document must ask first.
  - The `will-prevent-unload` flush at `main.js:320` appears never to fire,
    because no page cancels unload. This is inferred from code, not
    runtime-verified.

## 7. Architecture problems with evidence

| # | Problem | Evidence | Consequence for the product target |
|---|---|---|---|
| 7.1 | Multi-page shell with full reloads | `gotoPage` → `loadFile` (`main.js:134-140`); three HTML pages | Every profile switch drops renderer state; a menu bridge must re-subscribe per page; docking cannot span profiles |
| 7.2 | Central registration in `editor.js` | `registerCommands` `:516-587` (about 38 commands), `registerPanels` `:591-608`; handlers close over `S`, `EP()` and `do*` | Every tool in §18 (Move, Box, Extrusion, ROUTE…) edits one file |
| 7.3 | No mount/dispose contract | discarded unsubscribes (§6); `St._dividerCleanup` only on `stop()` | A persistent shell or a docking engine that detaches panels leaks listeners |
| 7.4 | Implicit module graph | 3 script lists (11/8/27), about 30 globals, the `script-load-order.test.js` 28-entry list | Adding a tool means adding a tag in the right position; naming collisions are caught only by a test |
| 7.5 | Copy-pasted chrome | three CSP metas with drift, three inline style blocks (~535 lines), three headers | A visual or security change must be made three times |
| 7.6 | `main.js` mixes product and harness | about 610 of 1,573 lines are the capture server (`:339-948`); 43 inline handlers | #122 will add the menu to the same file; reviewing the IPC surface means reading the QA harness |
| 7.7 | Desktop behaviors missing | no application menu; no close prompt for unsaved edits; no drag-and-drop; no shared status or notification surface | §8 criteria; the close prompt is required before Shell-4 becomes the default (§24) |
| 7.8 | Renderer-supplied path authority (security) | `mall:openPath` (`:1131`, exposed in preload, **no renderer caller found**) passes its argument to `openMallFile`; `mall:check(editFile)` reads the file the renderer names (`:1228-1229`); `shell:revealInFolder(filePath)` passes its argument to `shell.showItemInFolder` (`:1262`) | Conflicts with the main-process path-ownership boundary. Pre-existing, independent of the shell, and **requires its own audit** (SEC-SHELL-0, §21). Exploitability is **not** asserted here. |
| 7.9 | Implicit hardening | `sandbox` unset; no explicit navigation or window-open policy | Correct today by default and by construction; must become explicit before more surfaces land (§21) |

**Not a problem (evidence against):**
- The **pure `src/` layer** is well-factored. It is Electron-free and
  injectable, and node-tested (137 test files in directories, plus 12
  top-level).
- The **editor document core** already meets `WD.md`. The **Command and Panel
  Registries** are the right primitives.
- **Performance.** The analysis median is 216 ms against a 250 ms gate, and the
  renderer heap is stable over 30 cycles.

## 8. Desktop-application criteria

"Real desktop application" here means behavior, not toolkit. Status is measured
at the baseline.

| # | Criterion | Measurable target | Today |
|---|---|---|---|
| C1 | Persistent application shell | profile or workspace switch performs **no** page load; renderer state survives | **Missing** (full reload) |
| C2 | Application menu | platform menu whose items execute registered commands; no stray default roles | **Missing** (default menu) |
| C3 | Command system | every user action in every profile is a registered command | **Partial** (editor page only) |
| C4 | Toolbar system | toolbars are `data-command` bound; tools contribute buttons without editing a central file | **Partial** (binder exists; contributions central) |
| C5 | Panel lifecycle | panels mount, dispose, focus and report visibility through one contract | **Partial** (descriptive registry; no mount/dispose) |
| C6 | Workspace lifecycle | Code/Model/Play transitions preserve source, history and selection | **Partial** (Code/Model; Play is #121) |
| C7 | Document/session lifecycle | open, rehydrate, save, close and crash-restore for every profile | **Partial** (Mall cannot rehydrate) |
| C8 | Keyboard and focus | one dispatcher; no collisions; dialogs block app keys; focus restored on switches | **Partial** (#121 D2/D3) |
| C9 | Accessibility | full keyboard reach, zoom, High Contrast, ARIA state from one source | **Mostly met** (Feature A, UI-0 bindings) |
| C10 | Status and notifications | one status bar and one notification channel across profiles | **Missing** (per-page markup) |
| C11 | Window persistence | bounds, maximized state, multi-display safety | **Met** (`main.js:254-289`) |
| C12 | Close safety | closing a dirty document or the window asks first and offers Save, Discard and Cancel; recovery covers abnormal termination only | **Missing** (recovery only) |
| C13 | File drag and drop | dropping a `.wrl`/`.wrz` opens it through main authorization | **Missing** |
| C14 | OS file open / single instance | argv, second-instance, `open-file` | **Met** (`main.js:71-185`) |
| C15 | Recovery | crash and unclean-exit restore | **Met** (snapshot, crash reload, prompt) |
| C16 | Testability | pure logic node-tested; renderer modules testable without page order | **Partial** (vm co-load harness tied to tag order) |
| C17 | Packaging | signed or unsigned artifacts for Linux, Windows and macOS | **Met** (AppImage/tar, NSIS/MSI/portable, dmg/zip) |
| C18 | Cross-platform | CI on three OSes; native Windows QA | **Met** (ci.yml matrix; `docs/WINDOWS_QA_RUNBOOK.md`) |

## 9. Electron assessment

**Decision (O1, owner-approved): KEEP WITH MAJOR SHELL REFACTOR.** "Major"
refers to the staged refactor of the renderer and main-process structure in
§24. Electron itself is kept, and replacing it is not a migration path. The
assessment below is the evidence for that decision.

| Factor | Evidence | Weight |
|---|---|---|
| X_ITE | It is a WebGL browser engine, proven in bundled Chromium on all three OSes, including a 72-texture world in 847 ms. A system webview changes the GL stack per OS. | decisive |
| CodeMirror | Chromium-tested, bundled, accessible | strong |
| Security model | contextIsolation + preload bridge + `wrlworld:` privileged scheme + `onBeforeRequest` guard are all Electron APIs already reviewed | strong |
| Main-process code | `src/` services (safe-write, gzip, World scanner, parser shared by main and renderer) are Node. Electron lets main and renderer share one JS implementation. | strong |
| QA infrastructure | `VisualQaRunner`, the capture server, `sendInputEvent` and `capturePage` are Electron-specific; about 1,015 harness lines plus the evidence history | strong |
| Packaging | electron-builder across three OSes with notarization, already in release CI | strong |
| Memory and startup | 45 MB renderer heap measured; no startup figure exists. No measured problem justifies migration. | neutral; measured in SHELL-0 |
| Cost | Electron 41 upgrade cadence; about 31 MB of x_ite dist plus the Chromium runtime | accepted |

**Option D, replacing the shell (Tauri or a native toolkit):**
- **X_ITE** would run in WebKitGTK on Linux (the primary platform),
  WKWebView on macOS and WebView2 on Windows. That gives three GL and
  WebGL2 behaviors and no proven X_ITE support on WebKitGTK.
- A **native toolkit** cannot host X_ITE or CodeMirror at all without
  embedding a browser, which is Electron again.
- **IPC**: all 43 handlers would be re-implemented (Rust) or moved into a Node
  sidecar, which duplicates the runtime Tauri was chosen to drop.
- **Code**: the parser and document core shared by main and renderer would
  have to be split across languages.
- **QA**: the capture harness would be rewritten from scratch.
- **Packaging**: new signing and notarization pipelines.

Option D therefore scores lowest on almost every criterion (§10). Its
evaluation is recorded so the question does not need to be reopened without new
evidence; it is not a candidate path.

## 10. Renderer architecture option comparison

**Scale.** Each criterion is scored 1 (poor) to 5 (best) for *this*
codebase, not in general. Higher is always better, so on risk and cost rows 5
means lowest risk or cost. All eleven criteria are weighted equally; a
weighting check follows the table.

**Option definitions.**
- **A:** today's plain modules, plus the §11 contribution/disposable contract
  and esbuild entry bundles (O3).
- **B:** custom elements as the component unit, using shadow DOM where they
  want encapsulation.
- **C1–C3:** an established framework renders the application chrome and
  panels, hosting CodeMirror, X_ITE and the dock engine as imperative islands.
- **D:** a different desktop shell (Tauri, or a native toolkit).

| Criterion | A | B | C1 React | C2 Svelte | C3 Vue | D |
|---|---|---|---|---|---|---|
| Migration risk | **5** | 4 | 2 | 2 | 2 | 1 |
| Long-term maintainability | 4 | 4 | 4 | 4 | 4 | 2 |
| UI scalability (many tools) | 4 | 4 | **5** | **5** | **5** | 3 |
| CodeMirror integration | **5** | 4 | 3 | 3 | 3 | 2 |
| X_ITE integration | **5** | 4 | 3 | 3 | 3 | 1 |
| Accessibility | 4 | 3 | 4 | 4 | 4 | 2 |
| Testability | 4 | 3 | 3 | 3 | 3 | 1 |
| CSP / security | **5** | **5** | 4 | **5** | 3 | 3 |
| Bundle / runtime cost | **5** | **5** | 3 | 4 | 3 | 4 |
| Cross-platform packaging | **5** | **5** | **5** | **5** | **5** | 2 |
| Developer complexity | 4 | 3 | 3 | 3 | 3 | 1 |
| **Total (of 55)** | **50** | **44** | **39** | **41** | **38** | **22** |

**Evidence behind each row.**

| Criterion | Evidence and score notes |
|---|---|
| Migration risk | **A** moves one module at a time and keeps every existing test passing (§24). **B** needs each view re-shaped into an element class, but still incrementally. **C1–C3** need a framework root and a new render model per page, and the bundler on day one. They also replace the vm co-load test model. **D** rewrites 43 IPC handlers, the capture harness and packaging. |
| Maintainability | **A–C** all tie at 4: each gives a clear module unit once adopted. A framework's ecosystem churn (major-version migrations) roughly offsets its conventions. **D** splits one JavaScript codebase across two languages. |
| UI scalability | This is the **only row a framework wins.** Declarative rendering scales best for many small form-like panels (Inspector, contextual tool editors). **A** and **B** reach 4 through the `tools` / `contextualPanels` contributions (§18–19), not through declarative rendering. |
| CodeMirror | **A** drives the view imperatively, as `editor-view.js` does today. **B** works only if CodeMirror stays out of shadow roots. **C** needs a ref/effect wrapper and must never let the framework re-render the editor's DOM. **D** puts CodeMirror on three different system webviews. |
| X_ITE | `<x3d-canvas>` is already a custom element that owns its DOM and its WebGL context. **A** hosts it as-is. **B** composes with it naturally. **C** must treat it as an opaque island and guard against re-mounts, which would re-create the GL context. **D** would run it on WebKitGTK, where it is unproven, on the primary Linux platform. |
| Accessibility | **A** and **C** use native elements and global CSS, so the High Contrast and zoom stylesheets and cross-tree `aria-labelledby` work unchanged. **B** scores 3 because shadow DOM blocks global theme CSS and cross-root ARIA ids. Restricted to light DOM, B would score 4, but then it loses its encapsulation benefit and becomes A with permanent global registration (see the note below). |
| Testability | **A** keeps `node:test` with stub-DOM vm contexts (`test/renderer`, 9 files). **B** needs custom-element registry stubs. **C** realistically needs jsdom plus a testing library (new devDependencies). **D** has no Electron harness. |
| CSP / security | All but C3 and D keep today's `script-src 'self' file:` policy. **C1** is fine once bundled, but adds a runtime dependency to audit. **C3** needs `unsafe-eval` for in-DOM templates unless every template is precompiled. **D** replaces a reviewed security model with a new one. |
| Bundle / runtime cost | **A** and **B** add zero runtime bytes. **C2** compiles to a small runtime. **C1** (about 45 KB gzipped for React + ReactDOM) and **C3** add a runtime dependency, which also requires an exception to the X_ITE-only runtime rule. **D** ships smaller binaries, which is its one real advantage. |
| Packaging | **A–C** keep electron-builder and the existing three-platform release jobs unchanged. **D** needs new signing and notarization pipelines. |
| Developer complexity | **A** is the current idiom plus one contract. **B** adds custom-element lifecycle rules. **C** adds JSX or a template compiler, a reactivity model and a bundler-first workflow. **D** adds a second language and toolchain. |

**Why the order comes out as it does.**
- **A over B (50 vs 44).** B ties or trails A on every row:
  - it is −1 on migration risk, CodeMirror, X_ITE, accessibility,
    testability and developer complexity;
  - it gains nothing on scalability.

  Even scored in light DOM (accessibility 4), B totals 45. Its one structural
  benefit, native lifecycle callbacks, is delivered in A by the explicit
  contract (§11). The explicit contract also avoids permanent global element
  registration, which conflicts with dispose.
- **B over the frameworks (44 vs 38–41).** The frameworks each win
  scalability and accessibility by +1, but lose:
  - −2 on migration risk;
  - −1 on CodeMirror and X_ITE;
  - on runtime cost (C1, C3);
  - on CSP (C3).

  These are the criteria that dominate this product, because its heaviest
  surfaces are imperative islands that a framework must work around rather
  than render.
- **D last (22).** D is the lowest or tied-lowest on every row except
  bundle/runtime cost. X_ITE on WebKitGTK alone is an unbounded risk on the
  primary platform.

**Weighting check (no score changed).** The ranking is not an artefact of
equal weights. Tripling the weight of UI scalability, the frameworks' strongest
row, gives:

| Option | A | B | Svelte | React | Vue | D |
|---|---|---|---|---|---|---|
| Total | **58** | 52 | 51 | 49 | 48 | 28 |

A remains first, and the frameworks remain below B. Scores were re-checked for
this revision against the evidence notes above. **None changed.** The only
correction is to wording: React's runtime size is stated as gzipped.

**Why a framework buys little here.**
- **The surfaces that dominate the product are imperative and own their DOM:**
  the CodeMirror view, the X_ITE canvas (and soon its gizmos, which are
  scene-graph nodes rather than DOM), and a docking engine. UI-C0's candidate,
  Dockview, is framework-neutral.
- **The views a framework would help with are small.** The Inspector, Scene
  Tree and Object panel total about 1,085 lines today, and they render from
  pure planners (`inspector-edit.js`, `first-object.js`).
- **The real debts (§7) are lifecycle, registration and module graph.** A
  framework addresses none of them by itself. React, for example, would still
  need a contribution model for tools and still need the same document-session
  boundary.
- **Every framework option forces the bundler question** and rewrites the vm
  co-load test harness.

**Web Components.**
- *Plus:* native lifecycle callbacks.
- *Minus:* shadow DOM conflicts with the global High Contrast and zoom
  stylesheet, and with cross-tree `aria-labelledby`.
- *Minus:* custom-element registration is global and permanent, which fights
  dispose.
- *Verdict:* allowed later as an implementation detail of a single widget in
  light DOM. Not the architecture.

## 11. Framework decision

**Decision (O2, owner-approved): KEEP FRAMEWORK-FREE.** No React, Vue, Svelte
or other general UI framework is adopted.

The renderer uses first-party module boundaries and one explicit
**contribution and disposable contract**: plain modules, specified in
SHELL-0, roughly 100 lines plus tests. The sketch below shows its intended
shape; SHELL-0 fixes the actual API.

```js
// a feature module (e.g. features/model-primitives.js)
export function contribute(app) {          // app = shell services (§16)
  const d = app.disposables();             // collects every unsubscribe
  d.add(app.commands.register({ id: 'model.addBox', ... }));
  d.add(app.tools.register({ id: 'tool.box', commandId: 'model.addBox', group: 'create' }));
  d.add(app.panels.register({ id: 'object', mount(host) {...}, dispose() {...} }));
  return d;                                // shell disposes on profile/document teardown
}
```

**Evidence that would justify reopening this decision.** If after Shell-2 the
contextual tool panels (§18) show repeated hand-written diff/re-render code
across three or more panels, the owner may reopen the question with that code
as evidence. Popularity is not evidence. Until then the decision stands.

## 12. Module / bundling decision

**Current state:**
- Classic `<script defer>` everywhere; zero `type="module"`.
- Shared `src/` files use a dual export: CommonJS when `module` exists,
  `window.*` otherwise.
- esbuild bundles **only** `src/editor/browser/editor-view.js` → an IIFE
  (`package.json:17`). It is not minified, has no sourcemap, is gitignored and
  is rebuilt by `prestart`, `pretest:visual` and the dist scripts.

**Decision (O3, owner-approved in principle):** the existing esbuild
devDependency becomes the one approved bundler for first-party renderer
entrypoints.

**Architecture approval is not the current repository policy.** The two are
kept apart:

| | APP-ARCH-0 architecture approval (this document) | Current repository implementation policy |
|---|---|---|
| What it says | esbuild is the approved direction for first-party renderer entrypoints | `CLAUDE.md`: "Renderer UI is plain HTML/CSS/JS (plus the editor's esbuild bundle); no framework or bundler without separate approval." |
| Effect today | authorizes the **direction** only | **binding, unchanged.** Only the editor's existing `editor-view.js` bundle is permitted. |
| How it changes | — | A later approved implementation lane, expected to be SHELL-0 (bundling confirmation) or Shell-2 (first adoption), updates the applicable repository instructions **before** any new renderer entrypoint is bundled. |

This document changes no repository instruction file. Until that later lane
lands, no new renderer entrypoint may use esbuild.

The rule text that later lane is expected to adopt, subject to the SHELL-0
module-loading spike:

> Renderer UI is plain HTML/CSS/JS without a UI framework. **esbuild** (already
> an approved devDependency) is the one bundler, and may bundle first-party
> renderer entrypoints into local files under `renderer/`. No other bundler, no
> CDN, no runtime dependency added by bundling, and no transform that requires
> loosening the CSP. Shared `src/` modules stay CommonJS so main and `node:test`
> keep requiring them directly.

**Why esbuild entry bundles rather than native ESM:**
- **Tooling.** Main is CommonJS (`"type":"commonjs"`), and the pure modules
  are shared with main. esbuild consumes CommonJS directly, so nothing in `src/`
  changes format.
- **Native ESM over `file://`** in Electron 41 is **unverified** here: module
  MIME type, the null-origin CORS checks, and the CSP `script-src 'self' file:`
  interaction. SHELL-0 spikes it in `spikes/`. If it works cleanly it is an
  acceptable alternative with zero build step, and SHELL-0 records which of
  the two the repository rule should name. The contribution contract does not
  depend on which one wins.
- **Ordering.** Either option deletes the three hand-maintained script lists
  and the `window.*` namespace for migrated code.
- **Source maps.** Allowed as local files. A sourcemap needs no CSP change.
- **Tests.**
  - Pure modules: unchanged.
  - `script-load-order.test.js` shrinks as pages migrate. It is replaced by a
    test that each entry bundle builds and that the bundle has no `require` of
    a Node built-in.
  - The vm stub-DOM runtime tests keep working by loading the built bundle, or
    the module, into the vm.
- **Packaging.** Bundles are already built in release CI; more entries add no
  pipeline step.

**Keep:** the X_ITE classic script tag (vendor runtime, loaded once per
document) and the no-runtime-dependency rule.

## 13. Persistent-shell decision

**Decision (O4, owner-approved): YES.** One persistent desktop application
shell is the target architecture. It is reached in stages (§24). It is not a
new page and not a rewrite.

The shell grows out of `editor.html`, which already has:
- the Command and Panel Registries and workspace presets;
- both preview engines (`preview.js` and `world-preview.js` with two
  `<x3d-canvas>`);
- the recovery prompt;
- preferences;
- the most mature lifecycle.

```text
Application (one renderer document)
├─ Menu (main template → ui:command → registry)            #122
├─ Main toolbar (data-command, contributed tools)          C4
├─ Workspace selector (Code / Model / Play)                UI-0, #121
├─ Document/session area (one DocumentSession at a time)   §15
├─ Panel host (today: CSS composition; later: dock)        UI-1
├─ Status bar + notification service                       C10
└─ Dialog service (modal stack; owns D3 key blocking)      #121 D3
Profiles contribute: commands, panels, tools, validator, preview adapter
```

**Practical, because:**
- Main already behaves as one application; its state is not per-page.
- The editor page already proves that Mall and World preview can coexist.
- World already rehydrates from main.

**What makes it non-trivial:**
- the Mall page has no `mall:describe` (§7);
- the Mall 3 s poll must become a profile service with explicit stop;
- every discarded unsubscribe (§6) must become a disposable *before* pages
  merge.

**Not the final architecture:** the improved multi-page state. Shell-3
(shared chrome across pages) is a **waypoint**, not an end state. It satisfies
C3, C5, C10 and C16, but not C1, and it keeps the per-page menu
re-subscription cost.

**Required before the shell becomes the production default:** the
dirty-document close guard (C12, §24 Shell-4 gate).

## 14. Profile architecture

Recommended: **profiles are document profiles that contribute capabilities to
the shell**, selected by the open document's `context`
(`'mall' | 'world' | 'generic'`) plus any profile the user explicitly enables.

| Profile | Contributes | Never contributes to others |
|---|---|---|
| Mall Item | `validator.js` (Mall-only), size contract, repack command, Fit preview adapter, Mall checks panel | 80 KiB cap, `WorldInfo`, forbidden-node and texture rules |
| World Project | project scan service, asset/issue panels, packaging commands, world preview adapter (`wrlworld:`) | Mall rules; packaging is World-only |
| Generic VRML97 | parser diagnostics and advisories only | no Cybertown validation; **no preview** (`EP().start` is mall/world only today, `editor.js:1193`) |

**Generic VRML97 preview is not authorized by any shell stage.** The shell can
host a preview adapter for any profile, but Generic preview remains a
separately approved product lane. No Shell stage may register a Generic preview
adapter.

- **Rule containment is structural.** A profile is a module that registers
  commands and panels with `when: profile === 'mall'`-style enablement
  evaluated by the registry. It is not a set of `if (context === 'mall')`
  branches inside shared code. A source-scan test can assert that no shared
  module imports `validator.js`.
- **Separate pages → shell routes.** The project-level views (World
  project browser, Mall item checks) become **profile workspaces** inside the
  shell. They are reached through commands and need no page load.
- **Main keeps one service module per profile.**
  - Mall IPC, World IPC and editor IPC become `src/main/ipc-mall.js`,
    `ipc-world.js` and `ipc-editor.js`.
  - The channel names and the `window.vrmlpad` bridge are unchanged (CLAUDE.md
    naming rule).

## 15. Document / session architecture

No second canonical model. The split is by **ownership**, not by copying.

**These `WD.md` conclusions are unchanged by every shell stage:**
- the exact source text stays canonical;
- visual actions produce exact source patches;
- CodeMirror remains the undo authority;
- `sceneSelection` remains the selection authority;
- there is no second editable scene graph and no whole-document serializer;
- X_ITE remains the renderer;
- ambiguous identity fails closed.

Layout, panel, workspace and docking state are **UI state**. They never become
canonical document state, are never written to source, and never enter
analysis or undo history.

| Concern | Owner | Lifetime |
|---|---|---|
| File path, format, gzip, authorization, `sessionId`, baseline on disk | **main** `EditorController` / `EditorSession` | until close |
| Source text + undo | **CodeMirror** state inside the renderer `DocumentSession` | until close |
| Derived analysis (scene tree, scope graph, diagnostics) | `DocumentSession`, rebuilt per analysis | disposable |
| Selection | `sceneSelection`, owned by `DocumentSession` | per document |
| Preview | a `PreviewSession` owned by `DocumentSession`, choosing the profile adapter | per document |
| Profile | `DocumentSession.profile`, read-only after open | per document |
| Workspace mode, `sourceOpen`, layout | **shell** workspace service; never in the document | per window |
| Preferences | `WrlPreferences` (`localStorage`) | per user |
| Recovery | main `RecoveryController`, fed by `DocumentSession` | until save, close or Start Fresh |

`DocumentSession` is a **renderer object that wraps what `S` holds today**:
- the CodeMirror handle, baseline, `sessionId`, profile and analysis products;
- `sceneSelection` and the preview;
- `open(describe)` and `dispose()`.

It owns no new data. It is the boundary that lets commands receive
`app.document` instead of closing over `S`. Main stays the single authority
for paths. One document at a time stays the product rule; keying by `sessionId`
keeps multi-document possible later without designing it now.

## 16. Service boundaries

**Plain modules, no DI container.** The shell creates each service once and
passes an `app` object to contributions.

| Service | Exists today as | Shell responsibility |
|---|---|---|
| `commands` | `src/editor/command-registry.js` | unchanged API; contributions register |
| `keyboard` | `command-bindings.js` `installKeyboard` | one dispatcher; consults `dialogs` (D3) |
| `panels` | `src/editor/panel-registry.js` | adds `mount(host)` / `dispose()` to the record (§19) |
| `workspaces` | `setWorkspaceMode` / `applyWorkspace` + `workspace-presets.js` | one module; owns mode, `sourceOpen`, reset; emits changes |
| `tools` | — (new) | tool records → toolbars and contextual areas (§18) |
| `document` | `S` + init/save/close in `editor.js` | `DocumentSession` (§15) |
| `preview` | `editor-preview.js` (`St`) | `PreviewSession` + profile adapters; subscribes to prefs |
| `files` | `window.vrmlpad.editor.*` | thin renderer façade over the existing IPC; no path ever originates here |
| `preferences` | `WrlPreferences` | unchanged; the only UI persistence |
| `validation` | `language.js` + profile validators | profile-contributed |
| `notifications` / `status` | per-page markup, `showMsg` | one status bar and one toast/announce service (also `aria-live`) |
| `dialogs` | `showModal`, recovery prompt, Preferences | modal stack; exposes `isModalOpen()` for D3 |
| `close guard` (main + renderer) | — (recovery snapshot only) | dirty-document close: Save, Discard or Cancel (§24) |
| `menu` (main) | — | `src/main/app-menu.js` (#122) |

## 17. Command / panel / workspace integration

UI-0's architecture **carries forward unchanged**: ids, command records, the
enabled/checked pull model, `keyOwner`, presets as semantics and not layouts,
the panel ids in §11.4, and the persistence table. The shell changes only
**where registration happens**:
- today all of it happens in `editor.js` `registerCommands` / `registerPanels`;
- each feature module registers its own commands and panels with the same
  registry instance, returning disposables. New feature code does this from
  SHELL-0 onward (#121 first); Shell-2 moves the existing registrations out of
  `editor.js`.

Two additions to the UI-0 records, both additive and both specified in
SHELL-0:
1. **Panel `mount(host)` / `dispose()`** (optional). `element()` stays for
   CSS-composition hosting; `mount` is what a dock host calls.
2. **Command `profiles`** (optional array). An unlisted command is
   profile-neutral. The registry folds it into `isEnabled`, so profile
   containment is data rather than branches.

## 18. Modeling-tool extensibility

A future tool is **one feature module**, plus a markup-free toolbar entry:

```js
app.tools.register({
  id: 'tool.move', commandId: 'transform.move', group: 'transform',
  icon: 'move', appliesTo: (sel) => sel.kind === 'node' && sel.type === 'Transform',
});
```

- **Toolbars render from `tools.list(group)`.** The data-command binder
  already paints enabled and checked state, so a new tool needs **no edit to
  `editor.html` or `editor.js`.**
- **Each tool's source effect still goes through the existing source-safe
  paths:** a pure planner (like `first-object.js` / `inspector-edit.js`) →
  `applyVerifiedEdits` → one CodeMirror transaction.
- The §24 example list maps to groups as follows. The groups `select`,
  `transform`, `create`, `appearance`, `hierarchy`, `behavior` and `animation`
  hold, in order:
  - Select;
  - Move, Rotate, Scale;
  - Box, Sphere, Cone, Cylinder, Extrusion, IndexedFaceSet;
  - Material, Texture;
  - Group, Ungroup;
  - ROUTE;
  - Keyframe.
- **Viewport-interactive tools** (gizmos, WD2-E) additionally register a
  `viewportHandler` gated by `preset.viewportPicking`. They keep Policy C (D7)
  unless WD2-E changes it.

## 19. Contextual tool UI and docking boundary

**Contextual tool UI.** Add a `contextualPanels` contribution:
`{ id, title, appliesTo(selectionInfo, analysis), mount(host, ctx), dispose() }`.
- The shell evaluates `appliesTo` on each `sceneSelection` change and on each
  analysis.
- It mounts the matching editors into one **Context** host, and disposes the
  ones that no longer apply.
- Examples: Transform controls, Extrusion `spine`/`crossSection`, IFS geometry
  tools, Material.
- Editors read the node through `DocumentSession` analysis and write only
  through planners → `applyVerifiedEdits`.
- An unproven identity disables the editor. It never guesses (`WD.md` §7).
- The current Object panel (`model-workspace.js`) is the first instance.

**Docking boundary (what UI-C0 #38 receives; the engine is not chosen here).**
The shell exposes to any dock engine:
- **Registration:** `panels.list()` with `id`, `title`, `mount(host)` and
  `dispose()`. The engine creates host elements and calls `mount`; it never
  reaches into panel internals.
- **Lifecycle:** `mount` on first show, `dispose` on close.
  - Hidden-but-docked panels stay mounted. CodeMirror and X_ITE must not be
    re-created on a tab switch.
  - The engine must emit a resize notification so CodeMirror
    `requestMeasure` and the X_ITE canvas resize can run.
- **Focus:** `panels.focus(id)` stays the only focus path (D6); the engine
  reports the active panel to the shell for `panel.*` command state.
- **Presets:** `composition` (`source-primary` / `visual-primary`) maps to one
  default layout per preset. `workspace.reset` asks the engine to restore that
  default.
- **Persistence:** layout JSON goes through `WrlPreferences` under a
  per-preset key. It holds panel ids and geometry only, **never document
  data**. An unknown id in stored JSON is dropped (missing-panel recovery).
- **Constraint:** the engine works with plain DOM under the strict CSP, and
  does not require a UI framework. This is unchanged from UI-0 §20.1.

## 20. Application-menu boundary

#122's plan (UI-0 §10.3) **still fits**. #122 proceeds once SHELL-0 has
defined the command and menu integration boundary: how a contribution exposes
a command to the menu, and where the menu module and its subscription live.
SHELL-0 defines that boundary; it does not build the menu.

Three amendments should be added to #122 before it starts:

1. **Placement.** The template and its handlers go in a new
   `src/main/app-menu.js`, not inline in `main.js`. This follows §7.6 and keeps
   the security review to one file.
2. **Page and profile awareness.** Until Shell-4, the registry exists only on
   the editor page.
   - The bridge must tolerate a page with no registry: drop the id and log it.
   - Main should mark editor-only items disabled when `currentPage !==
     'editor'`, rather than relying on the renderer.
   - The preload `onCommand` subscription must be re-installed per page load.
     That is already in §10.3(3); it now has a concrete reason.
3. **Default roles.** Replacing the default menu removes View → Reload,
   Force Reload and the zoom roles. **Reload must not be re-added as a
   role**: a reload silently drops the unsaved editor buffer to the recovery
   snapshot (§6). DevTools should be dev-build only. Q-K4 already covers zoom.

The shell does **not** change the one-way, ids-only, allow-listed design, or
the enabled-state synchronization question that #122 owns.

## 21. Security impact

**Unchanged by every stage:**
- `contextIsolation: true` and `nodeIntegration: false`;
- the strict CSP. One source of truth per page; Shell-3 adds a test that
  asserts identical policy, rather than three drifting copies;
- no renderer filesystem authority;
- invoke-only IPC, except #122's reviewed `ui:command`;
- `wrlworld:` allow-listing;
- the network guard;
- no remote runtime dependency. esbuild bundling emits local files only.

**Electron hardening in Shell-1b.** This is a **separate reviewable unit with
its own QA gate** from the main-process structural refactor (§24). It changes
no intended behavior.
- **Explicit `sandbox: true`** in `webPreferences`, verified by the existing
  smoke probe.
- **An explicit navigation and window-open policy**, rather than a blanket
  deny:
  - `will-navigate` and `setWindowOpenHandler` consult one allow-list in main.
  - Today the allow-list is the `APP_PAGES` `loadFile` targets only.
    Everything else is denied and logged.
  - The policy is the single place where a **future, explicitly approved**
    runtime behavior is added, for example an approved Anchor or Play-mode
    navigation policy. This document does not define that behavior. It only
    requires that navigation becomes explicit and confined, and that the
    policy does not structurally preclude such a later approval.

**Pre-existing authority-boundary finding: SEC-SHELL-0** (O5,
owner-approved as a future separate lane; no issue is created by this
document).

Provisional lane name: **SEC-SHELL-0 — Renderer-Supplied Path Authority Audit
and Confinement.**

Three IPC handlers act on a path supplied by the renderer:

| Handler | Evidence | What the handler does with the path |
|---|---|---|
| `mall:openPath` | `main.js:1131`; exposed as `window.vrmlpad.openMallPath` (`preload.js:6`); **no renderer caller found** | passes it to `openMallFile`, which opens the file as the Mall item and writes its `.edit.wrl` working copy |
| `mall:check` | `main.js:1228-1229` | `fs.readFileSync(editFile, 'utf8')` on the renderer-named file, then validates it |
| `shell:revealInFolder` | `main.js:1262` | `shell.showItemInFolder(filePath)` |

**Why this matters.**
- These handlers conflict with the intended main-process ownership boundary:
  main owns every path, and the renderer names none (`CLAUDE.md`, Security
  and file safety).
- Every other path-bearing flow already conforms:
  - editor save resolves against the held session;
  - World references go through `authorizeWorldReference`;
  - `mall:repack` ignores renderer paths (`main.js:1255-1259`).

**What this document does not claim.** It does **not** assert that the
handlers are exploitable. The renderer runs only first-party code under a
strict CSP with context isolation, and no exploit path has been demonstrated.

**Why it is not a cleanup item.** Path authority is a security boundary. The
finding needs its own audit to:
- establish the actual exposure;
- check for further handlers of the same shape;
- define the confinement, for example resolving against main's
  `currentSession`.

**This document changes none of the three handlers.** SEC-SHELL-0 is
independent of the shell stages and can be scheduled whenever the owner
authorizes it.

**A persistent shell reduces the attack surface rather than growing it.**
There is one page and one CSP, and no navigation once loaded.

## 22. Accessibility impact

- **Feature A is preserved.** The zoom font compartment and the
  `--wrl-ui-scale` rem layer are page-level today. In the shell they become
  app-level and apply once.
- **Focus restoration across "pages"** today uses `sessionStorage` plus a
  duplicated retry IIFE. In the shell it becomes a direct `focus()` after the
  route change, which removes timing retries.
- **`aria-live` announcements** for status and notifications become one
  service (C10). Today status changes are per-page and inconsistent.
- **The dialog service** gives D3 one modal stack. Today modality is read from
  DOM classes.
- **Risk.** Moving Mall/World UI into the shell must keep every existing label
  and landmark. Each migration stage runs the accessibility runtime tests and
  `qa:vision` (§23).

## 23. Testing impact

**Kept unchanged:**
- all pure-module suites (`test/vrml`, `test/editor` pure files, `test/preview`,
  `test/world-project`, `test/settings`);
- `VisualQaRunner`, lock, transport and evidence code;
- CI matrix and release jobs.

**Changes:**

| Area | Change |
|---|---|
| `script-load-order.test.js` | Shrinks as pages adopt entry bundles. It is replaced per entry by "bundle builds; no Node built-in reachable; no duplicate registration". |
| vm stub-DOM runtime tests (9 files in `test/renderer`) | Load the module or bundle instead of the ordered tag list; assertions unchanged |
| New: contribution tests | Every feature module disposes everything it registered (`listenerCount()` returns to baseline) |
| New: shell route tests | Profile switch performs no navigation; Mall rehydrates; `currentText()`/`historyDepth()`/selection unchanged |
| Capture server (`main.js:339-948`) | Moves to `qa/visual-qa/capture-server.js` (Shell-1a), still env-gated. `gotoPage`-based jobs change to shell-route jobs in Shell-4. |

**Regression gates per stage (all stages):**
- `npm run check` green on three platforms;
- `test:visual` smoke;
- UI-0 Q-cases;
- WD2-C/WD2-D suites (`qa:wd2b`, `qa:wd2c`, picking);
- `qa:vision`;
- source byte identity and undo depth across workspace transitions;
- Mall size contract (`docs/MALL_SIZE_CONTRACT.md`) on repack;
- World packaging determinism.

Stage-specific gates are in §24.

## 24. Migration plan

No big-bang rewrite. Each stage ships alone, leaves the app usable, and starts
only on its own owner GO. Feature lanes are placed where their prerequisites
exist.

```text
APP-ARCH-0        architecture decision (this document)
  │
SHELL-0           contracts + measurements + module-loading spike
  │
  ├─ #121 UI-0-I2  workspace behavior against SHELL-0 contracts
  └─ #122 UI-0-M   application menu against the SHELL-0 command/menu boundary
  │
Shell-1           main-process decomposition
                  + separately gated Electron hardening
  │
Shell-2           editor module decomposition + contribution/disposable adoption
  │
Shell-3           shared application chrome across the current pages
  │
Shell-4           persistent application shell migration
                  (dirty-document close guard required before it becomes default)
  │
UI-1 / Shell-5    production configurable docked workspace (#43)

Independent:  UI-C0 #38 docking-engine research spike (when authorized)
              SEC-SHELL-0 path-authority audit (when authorized)
WD2-E #50:    after its shell/workspace prerequisites (§27)
```

### 24.1 SHELL-0 — Shell Contracts and Measurement

The **shell foundation**. A narrow lane that defines contracts and takes
measurements. It does **not**:
- implement the persistent shell;
- move any profile page into one shell;
- add docking;
- change product behavior.

| Scope item | Output |
|---|---|
| Application contribution contract | `contribute(app)` shape and the `app` service object (§11, §16) |
| Disposable lifecycle contract | disposable collection, dispose ordering, leak test (`listenerCount()` returns to baseline) |
| Document-session boundary | `DocumentSession` ownership and API surface (§15) |
| Application-service boundary | which services exist, what each owns, how contributions reach them (§16) |
| Command contribution boundary | how a feature module registers commands and returns disposables; optional `profiles` field (§17) |
| Panel contribution boundary | `mount(host)` / `dispose()` added to the panel record (§17, §19) |
| Tool contribution boundary | tool record and toolbar rendering from `tools.list(group)` (§18) |
| Contextual-panel contribution boundary | `appliesTo` / `mount` / `dispose` evaluated on selection and analysis (§19) |
| Menu integration boundary | how commands are exposed to #122's menu; where the menu module and its renderer subscription live (§20) |
| Startup baseline | cold start to first interactive frame, per page |
| Page-switch baseline | Mall → editor → Mall and World → editor → World |
| Memory baseline | process RSS and renderer heap per page, and after a switch soak |
| Native ES-module `file://` spike | in `spikes/`; MIME, origin and CSP behavior in Electron 41 |
| Bundling decision confirmation | esbuild entry bundles vs native ESM, recorded with spike evidence, plus the exact repository-rule text a later lane adopts (§12) |

Contract modules land as pure modules with unit tests and no callers, the same
pattern UI-0-I Wave 1 used. Measurements go under `qa/`.

**Gates:**
- `npm run check` on three platforms;
- contract unit tests;
- no product behavior change (the visual smoke test passes unchanged).

### 24.2 Later stages

| Stage | Content | Behavior change | Stage-specific gates |
|---|---|---|---|
| **Shell-1a** — main-process structural refactor | Move the capture server to `qa/visual-qa/`; IPC groups to `src/main/ipc-*.js`; window and lifecycle to `src/main/window.js`. Pure module movement. | none | IPC inventory test (43 channels, same names, same handlers); all visual QA unchanged; `qa:windows` |
| **Shell-1b** — Electron security hardening | Explicit `sandbox: true`; the explicit navigation and window-open policy (§21) | none intended | sandbox smoke probe; navigation-policy tests (allowed `loadFile` targets pass, everything else is denied and logged); preview, `wrlworld:` and recovery flows unchanged |
| **Shell-2** — editor decomposition | Split `editor.js` into `DocumentSession`, workspace service, dialog service and feature contributions (file, edit, view, preview, model, scene views); adopt the disposable contract; keep all ids and markup; adopt entry bundling for the editor page only once the repository rule has been updated (§12) | none | UI-0 registry and binding suites; #121 suites; WD2 runtime suites; dispose tests; source byte, undo-depth and selection identity |
| **Shell-3** — shared chrome | One stylesheet, one CSP source, shared header/status/notification/preferences/recovery modules for all three pages; Mall and World commands registered through the registry | small, visible (consistent chrome) | `qa:mall-preview`, `qa:world-preview`, packaging; accessibility runtime; CSP-identity test |
| **Shell-4** — persistent shell | Add a read-only `mall:describe` from `currentSession`; World, then Mall, become profile workspaces inside the shell; `app:goto` becomes an in-shell route; **the dirty-document close guard** (§24.3); the old pages stay loadable as a fallback for one release, then are removed in a later lane | profile switch without reload; Mall survives a round-trip; close asks when dirty | full Mall workflow (open, validate, repack, size contract); World scan, preview, package; desktop open; recovery prompt; close-guard matrix (§24.3); capture-server shell-route jobs |
| **UI-1 / Shell-5** — docking (#43) | The dock engine chosen by UI-C0 hosts `panels.mount`; presets map to default layouts | as UI-1 defines | UI-1 acceptance |

**Shell-1 is two reviewable units.** Shell-1a and Shell-1b may share a lane,
but they must be separate commits with separate QA gates, and 1a must land
first. A security regression must be diagnosable independently of a
module-movement regression.

C13 (drag and drop) is a small independent desktop lane that fits after
Shell-1a, once the window module exists.

### 24.3 Dirty-document close guard

**Required before the persistent shell becomes the production default.** It
may land earlier, as its own lane after Shell-1a. It is not implemented by this
document.

The shell must distinguish four outcomes:

| Outcome | Trigger | Result |
|---|---|---|
| **Successful save** | user chooses Save; the save succeeds | the document closes; the recovery record is cleared, as today on save |
| **Discard** | user chooses Discard | the document closes without writing; the recovery record is cleared |
| **Cancel close** | user chooses Cancel, **or Save fails or hits a conflict** | the window and document stay open, unchanged; a failed save never falls through to closing |
| **Recovery after abnormal termination** | crash, kill or power loss: no prompt was possible | the existing snapshot and recovery prompt; unchanged |

**Design constraints:**
- Main must intercept window `close` and ask the renderer, because the
  renderer alone cannot stop a native close. The question to the renderer is
  "is the current document dirty?"; main never receives document text this
  way.
- The guard reuses the existing save path (`editor:save`, `safeSave`). It adds
  no new write path.
- The same guard covers the app quit, the window close and, in the shell, a
  profile switch that would close the document.

## 25. #121 impact

**`#121 SHOULD WAIT FOR SHELL FOUNDATION`**

**Shell foundation = SHELL-0 only.** #121 does **not** wait for Shell-1
through Shell-4.

**Evidence.**
- #121 is the next lane that adds new commands, panel commands, a dialog
  key-blocking rule, a reset path and a new workspace (D2, D3, D5, D6,
  `workspace.reset`, Play, D7, D9).
- Built today, it would have to invent the contribution, disposable, dialog
  and workspace-service contracts that SHELL-0 is chartered to define, or add
  all of that to `editor.js` (§7.2).
- Either outcome creates a second contract that SHELL-0 or Shell-2 must later
  reconcile. Waiting for SHELL-0 costs one narrow lane and removes that
  rework.
- #121 needs no stage after SHELL-0. Its behavior is renderer-local and does
  not depend on main decomposition, shared chrome or the persistent shell.

**After SHELL-0, #121 proceeds against its contracts:**
1. New commands live in feature modules that use the contribution contract:
   - Play and `workspace.reset` in a workspace-commands module;
   - `panel.*.focus` in a panel-commands module.

   They are not added to `editor.js`.
2. Every new subscription returns a retained disposable.
3. D3 dialog blocking asks one `isModalOpen()` (the dialog-service boundary),
   not per-dialog `classList.contains('show')` checks.
4. The D8 hidden-panel set is recorded on the Play preset as data (e.g.
   `hiddenPanels`). The `workspace-play` CSS is driven from that data, so UI-1
   maps it to a layout without re-deriving it from CSS.
5. `workspace.reset` calls the preview through its public API (`setLayout`, a
   split setter). It never writes `St` or preference keys directly, so UI-1
   replaces only the body.

#121's existing constraints are unchanged:
- source text unchanged by workspace transitions;
- CodeMirror undo authority and `sceneSelection` authority;
- X_ITE;
- no new runtime dependency;
- CSP unchanged;
- no new IPC.

## 26. #122 impact

**The plan fits** (UI-0 §10.3). #122 proceeds **after SHELL-0** has defined
the command/menu integration boundary (§24.1, §20).

It also takes the three §20 amendments:
1. place the menu in `src/main/app-menu.js`;
2. page/profile-aware disabled items, and a per-page subscription until the
   persistent shell exists;
3. replace the default menu roles without re-adding Reload.

#122 does **not** wait for Shell-1. If Shell-1a lands first, #122 places its
module beside the extracted `ipc-*` modules.

The one-way, ids-only, allow-listed design and the UI-0 completion rule
(#36 after #35, #121 and #122) are unchanged.

## 27. UI-C0 / UI-1 / WD2-E impact

**UI-C0 #38** stays the **docking-engine research spike**.
- It is not part of the shell migration. It runs in `spikes/` when the owner
  explicitly authorizes it.
- Its evaluation adds the §19 requirements:
  - `mount`/`dispose` hosting;
  - hidden tabs stay mounted (no CodeMirror or X_ITE re-creation);
  - a resize notification;
  - per-preset layout through `WrlPreferences`;
  - no document data in layout JSON.
- Consuming a dock engine through esbuild depends on the §12 rule update.
- The dependency itself still needs owner approval.

**UI-1 #43** is the production docked workspace.
- It runs **after Shell-2**, and after UI-C0's recommendation is accepted.
- Without Shell-2, UI-1 would have to decompose `editor.js` and invent the
  panel lifecycle inside a docking lane. With Shell-2, UI-1 hosts existing
  `mount`able panels.
- UI-1 is the Shell-5 position in the sequence, but it remains its own lane
  with its own acceptance. UI-C0 and UI-1 are not merged into the shell
  stages.

**WD2-E #50** (transform authoring) does not begin before its
shell/workspace foundation exists:
- SHELL-0's tool and contextual-panel contribution boundaries;
- #121's workspace behavior: the Play preset and the `visualAuthoring` /
  `viewportPicking` gates in use;
- Shell-2, so its tools register as feature modules rather than in
  `editor.js`.

## 28. Risks

| # | Risk | Mitigation |
|---|---|---|
| S1 | Decomposition silently changes keyboard or command paths | Shell-2 is pure; UI-0 and #121 parity suites and Q-cases are its gates |
| S2 | Persistent shell leaks listeners or X_ITE contexts across profile switches | dispose tests; SHELL-0 memory baseline vs a 30-switch soak in Shell-4 |
| S3 | Two X_ITE browsers live in one document after Mall and World merge | already the case on `editor.html` today; Shell-4 keeps engines lazy and disposes the inactive one |
| S4 | Bundling changes load timing (defer vs IIFE) | adopted on one page in Shell-2, only after the rule update, with the visual suite as gate |
| S5 | Mall rehydration (`mall:describe`) widens IPC | read-only, from `currentSession`, returns no new capability; reviewed in Shell-4 and consistent with SEC-SHELL-0 |
| S6 | Capture-server relocation breaks the Windows file transport | `transport.js` untouched; `qa:windows` gate in Shell-1a |
| S7 | Over-engineering the contribution contract (plugin system, `when` language) | SHELL-0 limits it to `contribute(app)` + disposables + the record fields in §17–19; UI-0 R5 applies |
| S8 | SHELL-0 grows into an implementation lane and delays #121 | the §24.1 scope table is the boundary; persistent shell, page moves and docking are explicitly excluded |
| S9 | Hardening and refactor regressions become indistinguishable | Shell-1a and Shell-1b are separate commits with separate gates, 1a first |
| S10 | The persistent shell ships with only recovery protecting unsaved edits | the close guard is a Shell-4 gate (§24.3) |
| S11 | The navigation policy blocks a later approved Anchor or Play behavior | one allow-list in main, extended only by an approved lane (§21) |

## 29. Owner decisions

### 29.1 Approved (2026-10-07)

| # | Decision | Disposition |
|---|---|---|
| O1 | Electron | **KEEP WITH MAJOR SHELL REFACTOR.** WRL Forge remains an Electron application; no replacement shell is a migration path (§9). |
| O2 | Renderer framework | **KEEP FRAMEWORK-FREE.** No general UI framework; first-party module boundaries and explicit lifecycle contracts (§11). |
| O3 | Bundling | **APPROVED IN PRINCIPLE.** esbuild is the approved bundler for first-party renderer entrypoints. It becomes active only after an approved implementation lane updates the repository rules; this document changes none (§12). |
| O4 | Persistent shell | **APPROVED.** One persistent desktop application shell is the target; the improved multi-page state is a waypoint; migration stays staged (§13, §24). |
| O5 | Path authority | **APPROVED** as a future separate security lane, provisionally **SEC-SHELL-0 — Renderer-Supplied Path Authority Audit and Confinement**. No issue created yet (§21). |
| O6 | #121 sequencing | **`#121 SHOULD WAIT FOR SHELL FOUNDATION`**; shell foundation = SHELL-0 only (§25). |

### 29.2 Owner decisions required

**No architecture decision remains unresolved.** What remains are
authorizations of already-defined work, each on its own owner GO:
- start SHELL-0;
- create the SEC-SHELL-0 issue and schedule it;
- after SHELL-0, start #121 and #122;
- authorize the UI-C0 spike.

Detail decisions that belong to a lane are made inside it. SHELL-0 settles the
contract APIs and esbuild vs native ESM; UI-C0 settles the dock engine.

## 30. Recommended next lane structure

```text
APP-ARCH-0 (this document, owner-approved direction)
│
├─ SHELL-0  Shell Contracts and Measurement        ← next lane
│    ├─ #121 UI-0-I2  after SHELL-0
│    └─ #122 UI-0-M   after SHELL-0's command/menu boundary
│         └─ #36 UI-0-Q → #37 UI-0-C (UI-0 completion rule unchanged)
│
├─ Shell-1a main structural refactor → Shell-1b Electron hardening
├─ Shell-2  editor decomposition                    ← UI-1 and WD2-E wait for this
├─ Shell-3  shared chrome
├─ Shell-4  persistent shell (+ close guard before default)
└─ UI-1 #43 / Shell-5 docked workspace (after Shell-2 and UI-C0)

Independent, each on its own GO:
  UI-C0 #38   docking-engine spike
  SEC-SHELL-0 renderer-supplied path authority audit and confinement
```

**Recommended next lane: SHELL-0 — Shell Contracts and Measurement** (§24.1).
It is a narrow foundation lane with no product behavior change. It defines:
- the contribution, disposable, document-session and service contracts;
- the command, panel, tool, contextual-panel and menu boundaries.

It also measures startup, page-switch and memory baselines, and runs the
native ES-module spike that confirms the bundling decision.
