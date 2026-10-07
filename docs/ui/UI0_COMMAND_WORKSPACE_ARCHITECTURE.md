# UI-0 — Command and Workspace Architecture

## 1. Status

**OWNER-ACCEPTED FOR IMPLEMENTATION AFTER MERGE.** The owner accepted this
architecture on **2026-10-07**, with one packaging correction (D10, §25). Every
owner decision D1–D10 is final and recorded in §25. This document is research and
design only.

| | |
|---|---|
| Lane | UI-0-R [#34](https://github.com/Ascendance3D/wrlforge/issues/34) under UI-0 [#33](https://github.com/Ascendance3D/wrlforge/issues/33) |
| Baseline | `origin/main` `c6b460fcc767779621445a6a4510d145a4b1e4ed` |
| Owner acceptance | 2026-10-07 — architecture accepted; D1–D9 accepted; D10 revised (§25) |
| Implementation | **none started.** Three tracked packages (§21), each starting only on its own owner GO: UI-0-I [#35](https://github.com/Ascendance3D/wrlforge/issues/35), UI-0-I2 [#121](https://github.com/Ascendance3D/wrlforge/issues/121), UI-0-M [#122](https://github.com/Ascendance3D/wrlforge/issues/122) |
| Product code changed | none |
| Dependencies added | none |

Every `file:line` below refers to that baseline. Line numbers are evidence for
review, not a stable API. #35 must re-verify them before it edits anything.

This document lives in `docs/ui/` because no existing location fits.
`docs/white-dune-2026/` holds the WD document-core and authoring lanes. UI-0 is
a separate track that WD2-E and later lanes depend on. UI-C0 and UI-1 should add
their documents here too.

## 2. Scope

**In scope:**
- one Command Registry
- command enabled and checked state
- keyboard invocation
- toolbar and menu references to commands
- one Panel Registry
- workspace presets: Code, Model and Play
- one workspace authority
- the meaning of Reset Workspace
- UI-state persistence
- migration and test plans for #35

**Out of scope, owned elsewhere:**

| Topic | Owner |
|---|---|
| docking engine selection, Dockview | UI-C0 [#38](https://github.com/Ascendance3D/wrlforge/issues/38) |
| production dock layout, popouts, restoring a dock layout | UI-1 [#43](https://github.com/Ascendance3D/wrlforge/issues/43) |
| transform gizmos, snapping | WD2-E [#50](https://github.com/Ascendance3D/wrlforge/issues/50) |
| hierarchy authoring | WD2-F [#60](https://github.com/Ascendance3D/wrlforge/issues/60) |
| command palette UX | WD2-K [#91](https://github.com/Ascendance3D/wrlforge/issues/91) |
| animation and PROTO UI | WD2-I [#81](https://github.com/Ascendance3D/wrlforge/issues/81), WD2-J [#86](https://github.com/Ascendance3D/wrlforge/issues/86) |

**Page scope.** UI-0 covers the native editor page only: `renderer/editor.html`,
`renderer/editor.js` and the modules that page loads. That is where Code, Model
and Play exist.

The Mall page (`renderer/index.html` / `renderer.js`) and the World page
(`renderer/world.html` / `world.js`) keep their current handlers in #35. For
example, the Mall page's Ctrl+R Repack and Ctrl+E handler is at
`renderer/renderer.js:275`. Those pages can adopt the registry in a later lane.
The registry is page-agnostic, so nothing in it prevents that.

## 3. Current UI inventory

### 3.1 Shape of the editor page

- **Script stack.** `renderer/editor.html` loads plain classic scripts in a fixed
  order. `test/editor/script-load-order.test.js` checks that order, and it
  enforces module-unique top-level names.
- **Pure UI state.** `src/editor/ui-state.js` is loaded as `WrlEditorUI`. It
  holds the pure models:
  - `toolbarModel` (`:208`)
  - `resolveShortcut` (`:257`)
  - `previewLayoutModel` (`:84`)
  - `initialWorkspaceMode` (`:292`)
  - zoom and theme
- **`editor.js`** owns document-page state in a module-level object `S`
  (`renderer/editor.js:79-123`), the central `render()` (`:188`), every File/Edit
  action and the workspace mode.
- **`editor-preview.js`** owns preview state in `St`: the layout, the split, the
  scheduler, and arming/disarming picking. It exposes these as
  `window.wrlEditorPreview`.
- **`model-workspace.js`** binds the Model bar and the Object panel through
  injected `deps`. It owns no document data and no selection.
- **`sceneSelection`** (`renderer/editor.js:127`,
  `src/editor/scene-selection.js`) is the one selection authority. Its API is
  `getSelection`, `setSelection`, `clearSelection`, `subscribe` and
  `listenerCount`.
- **CodeMirror** is the one undo authority (`src/editor/browser/editor-view.js:209`,
  `history()`). Inspector and Model edits arrive as single isolated transactions
  (`:336`).
- **Preferences** go to renderer `localStorage` through `WrlPreferences`
  (`renderer/preferences.js`, `src/settings/preferences.js`). There is no
  preference IPC.
- **Main process.**
  - It owns every file path.
  - It builds **no application menu**: `main.js` never imports `Menu`, so
    Electron's default menu applies.
  - There is **no main→renderer push channel**: `preload.js` has no
    `ipcRenderer.on`.
  - All IPC is `ipcRenderer.invoke` → `ipcMain.handle`.

### 3.2 Command inventory

Column key:
- **Doc** = edits source text.
- **IPC** = calls `window.vrmlpad.*`.
- **M/P** = safe in Model / safe in Play under the proposal in §15.
- Enabled state "T" = from `UI.toolbarModel` through `render()`
  (`renderer/editor.js:198-207`). In `toolbarModel`, `active = open && !saving`.

#### File

| Action (id) | Handler | Entry points | Key | Enabled | Checked | Doc | IPC | M/P |
|---|---|---|---|---|---|---|---|---|
| ← Back (`backBtn`) | `doBack` `editor.js:474` | button | — | always | — | no | `setText`, `recoveryRecordDirty`, `goto` | ✓/✓ |
| Save (`saveBtn`) | `doSave` `:362` | button `:499`, window keydown `:530` | Mod+S | T: `active && dirty`; self-guards `:363` | — | no | `editor.save` | ✓/✓ |
| Save As… (`saveAsBtn`) | `doSaveAs` `:402` | button `:500`, keydown `:531`, conflict modal `:392` | Mod+Shift+S | T: `active` | — | no | `editor.saveAs` | ✓/✓ |
| Reload (`reloadBtn`) | `doReload(force)` `:420` | button `:501`, conflict modal `:397` (force) | — | T | — | **replaces buffer** | `editor.reload` | ✓/✗ |
| External editor (`externalBtn`) | `doExternal` `:450` | button `:507` | — | T | — | no | `openInExternal` | ✓/✓ |
| Close (`closeBtn`) | `doClose` `:463` | button `:508`, keydown `:533` | Mod+W | T: `open && !saving` | — | no | `close`, `goto` | ✓/✓ |

#### Edit

| Action | Handler | Entry points | Key | Enabled | Doc | M/P |
|---|---|---|---|---|---|---|
| Undo (`undoBtn`) | `S.handle.undo()` → `editor-view.js:352` | button `:502`; CodeMirror `historyKeymap` | Mod+Z (CM) | T | **yes** | ✓/✗ |
| Redo (`redoBtn`) | `S.handle.redo()` `:353` | button `:503`; CM | Mod+Y, Mod+Shift+Z (CM) | T | **yes** | ✓/✗ |
| Find (`findBtn`) | `S.handle.openSearch()` `:354` | button `:504`; CM `searchKeymap` | Mod+F (CM) | T | no | ✓/✗ |
| Replace (`replaceBtn`) | the same `openSearch()` | button `:505` | — | T | no | ✓/✗ |
| Go to line… (`gotoBtn`) | `doGotoLine` `:437` | button `:506`, keydown `:532` | Mod+G | T | no (caret) | ✓/✗ |

#### View / Accessibility

| Action | Handler | Entry points | Key | Enabled | Checked | Notes |
|---|---|---|---|---|---|---|
| Zoom + / − / Reset | `applyZoom(...)` `editor.js:49` | toolbar `:510-512`, keydown `:534-536`, Preferences dialog `preferences.js:383-385` via `WrlPreferences.set('zoom')` → subscriber | Mod+= / Mod++, Mod+−, Mod+0 | always | — | Writes the `zoom` preference |
| Theme (`themeSelect`) | change listener `editor.js:551` | select, Preferences dialog | — | always | value | A value picker, not a command (§7.4) |
| High Contrast | `setHighContrastEnabled` (`src/settings/preferences.js:184`) via the dialog checkbox handler | Preferences dialog checkbox | — | always | `theme === 'contrast'` | Not on the toolbar |
| Preferences (`prefsBtn`) | `WrlPreferences.show` `editor.js:519` | button | — | always | — | Modal with a focus trap and Escape |

#### Workspace

| Action | Handler | Entry points | Enabled | Checked | Notes |
|---|---|---|---|---|---|
| Model (`modeModelBtn`) | `deps.setMode('model')` `model-workspace.js:81` → `setWorkspaceMode(m, true)` `editor.js:871` | button | always | `aria-pressed` `model-workspace.js:63` | Persists |
| Code (`modeCodeBtn`) | `deps.setMode('code')` `:82` | button | always | `:64` | Persists |
| Show/Hide Source (`sourceToggleBtn`) | `deps.setSourceOpen` `:83` → `S.sourceOpen` + `applyWorkspace` | button | hidden unless Model `:66` | `aria-pressed` `:67` | Also forced open by `noteDamage` `editor.js:773` |

#### Preview

| Action | Handler | Entry points | Key | Enabled | Checked |
|---|---|---|---|---|---|
| Update (`previewUpdateBtn`) | `manualUpdate` `editor-preview.js:221` | button `:535`, keydown `editor.js:537` | Mod+Enter | always; guards `St.active` | — |
| Show saved version | `showSaved` `:330` | button `:539` | — | always; guards | `displaySaved` chip |
| Maximize/Restore (`previewMaxBtn`) | `toggleMaximize` `:141` | button `:537`, keydown `editor.js:538` | Mod+Shift+Enter | always | `aria-pressed` `:129` |
| Layout (`previewLayoutSelect`) | `setLayout` `:140` | select `:543`, Preferences dialog | — | always | value |
| Find new files (`previewFindNewBtn`) | `findNewFiles` `:361` | button `:541` | — | disabled while in flight `:364/:367` | — |

#### Model (authoring)

| Action | Handler | Enabled | Doc |
|---|---|---|---|
| Add Box / Add Sphere | `addObject` `editor.js:910` → `dispatchModelPlan` `:884` | `!!S.handle` (`model-workspace.js:70-71`) | yes, as one CM transaction |
| Duplicate | `duplicateSelected` `:916` | a node is selected (`:75`) | yes |
| Delete | `deleteSelected` `:923` | a node is selected (`:76`) | yes |

#### Selection / Validation

- **No selection commands exist today.** Nothing is bound to "clear selection",
  Escape or Delete.
- Selection changes come from:
  - Scene Tree rows (`scene-tree.js:138`, and the keyboard handler at `:230`);
  - a proven viewport pick (`editor.js:836`);
  - analysis re-anchoring.
- **Validation** has no commands. Diagnostics and Advisories are panels whose
  rows navigate the source (`editor.js:267-299`).
- **Neither area gets an invented command in #35.**

#### Widget-local interactions (deliberately not commands)

These act on a focused widget instance or take per-field parameters. They are
not application commands. They stay local to their widget:
- Scene Tree arrow, Home, End, Enter and Space (`scene-tree.js:230`)
- Inspector and Object-panel field commit (Enter, Escape, Apply):
  `scene-inspector.js:280`, `model-workspace.js:190`
- Outline and diagnostic row activation
- the split divider's arrow keys (`editor-preview.js:173`)
- modal buttons
- the recovery prompt
- the Mall Fit radios and guides (`preview.js:404-406`)
- World Viewpoint, Navigation and Reset View (`world-preview.js:415-417`)

The last two groups are shared with the Mall and World pages.

**Totals:** **30 existing editor-page commands** migrate. **2 new commands** are
`workspace.play` and `workspace.reset`. The full mapping is in §7.

## 4. Problems found (architecture debt)

1. **Each logical action has two or three hand-written entry paths.**
   - Save, Save As, Go to line, Close, Zoom×3, Preview Update and Maximize each
     have a button listener (`editor.js:499-512`, `editor-preview.js:535-543`)
     *and* a separate `if/else` arm in the window keydown (`editor.js:526-539`).
   - Zoom has a third path through the Preferences dialog.
   - The paths agree today only because they happen to call the same function.
2. **Enabled state is computed in three uncoordinated places.**
   - `toolbarModel` → `render()` covers 10 buttons.
   - `paintBar()` covers 4 Model-bar buttons (`model-workspace.js:69-76`).
   - Ad-hoc writes cover the rest (`editor-preview.js:364`, `preview.js:293`).
   - The keyboard path ignores all of these and relies on per-handler guards
     (`doSave` `:363`, `manualUpdate` `:222`).
3. **Checked state is written by four different modules.** Model/Code in
   `model-workspace.js:63-64`, Source in `:67`, Maximize in
   `editor-preview.js:129`, High Contrast in `preferences.js`.
4. **The window keydown has no input, modal or `defaultPrevented` checks**
   (`editor.js:526`). Ctrl+S still fires behind the Go-to-line modal.
5. **There are CodeMirror key collisions.** I checked these against the installed
   `@codemirror/search` and `@codemirror/commands`:
   - `searchKeymap` binds **Mod-g → findNext** with `preventDefault: true`. The
     app's window listener also maps Mod+G → `doGotoLine`.
   - `defaultKeymap` binds **Mod-Enter → insertBlankLine**. The app maps
     Mod+Enter → preview Update.
   - CodeMirror calls `preventDefault` but does not stop propagation, so **both
     probably fire when the source has focus.** If so, Ctrl+Enter in the source
     edits the document *and* updates the preview.
   - This is not yet confirmed at runtime. #35 must record the actual current
     behaviour as a baseline (§23, Q-K3) before it migrates anything.
6. **Shortcut text is copied into markup.** For example `title="Increase size (Ctrl +)"`
   at `editor.html:431` and `title="Update the preview (Ctrl+Enter)"` at `:476`.
   There is also a read-only shortcut table in `preferences.js:212-222`. Nothing
   ties any of these to the real bindings.
7. **Workspace mode values are defined twice.** `UI_WORKSPACE_MODES`
   (`ui-state.js:287`) and `PREF_WORKSPACE_MODES` (`src/settings/preferences.js:48`).
   `setWorkspaceMode` also coerces any other value to `'code'` (`editor.js:872`).
   Adding Play needs all three changed together.
8. **The workspace switch does not refresh the Model bar.** `setWorkspaceMode`
   → `applyWorkspace` never repaints it. Callers have to call
   `modelWorkspace.refresh()` themselves (`model-workspace.js:81-82`) or rely on
   `render()`.
9. **The Preferences dialog has dead or partial appliers.**
   - `applyTheme`, `applyZoom` and `applyPreviewLayout`
     (`renderer/preferences.js:83/96/113`) are exported but never called.
   - `applyZoom` calls a `__wrlEditor.setFontSize` that does not exist.
   - Changing the preview layout in the dialog is persisted but **not applied
     live**. `editor.js`'s subscriber handles theme and zoom only, and
     `editor-preview.js` never subscribes.
10. **Electron's default menu is live and nobody has checked how it interacts
    with the app's shortcuts.** On Linux and Windows the default View menu
    carries zoom roles (Ctrl+0/+/−) and Reload (Ctrl+R). The app binds the same
    zoom keys. Whether the renderer's `preventDefault` suppresses Chromium page
    zoom has not been verified (§9.6, risk R3).
11. **The Model bar shows in Code.** Add/Duplicate/Delete work from Code today,
    because only the Source toggle hides. This is current behaviour, and #35
    preserves it (§13).

## 5. Architecture invariants

These carry forward unchanged and bind #35:

- **Source authority.** The exact source text is the document (`WD.md` §2). No
  registry, preset or panel record owns or caches source, AST, scene model,
  selection or undo history.
- **Selection.** `sceneSelection` remains the only selection store. No workspace
  or panel gets its own.
- **Undo.** CodeMirror `history()` remains the only undo authority. Commands that
  edit source call the existing source-safe paths:
  - `S.handle.undo/redo`
  - `dispatchModelPlan` → `applyVerifiedEdits`
  - `applyInspectorField`
- **Workspace changes never touch source.** A workspace or panel change must leave
  `currentText()` and CodeMirror `historyDepth()` unchanged.
- **Rendering.** X_ITE only. UI-0 adds no `preventDefault`/`stopPropagation` on
  the viewport, so the WD2-D contract stays as it is.
- **Security.** Strict CSP, `contextIsolation: true` and `nodeIntegration: false`
  (`main.js:303-307`) stay. So do the invoke-only IPC model and the rule that the
  renderer has no filesystem access.
- **No framework.** Plain classic scripts with the existing dual export (CommonJS
  when `module` exists, `window.*` otherwise). No runtime dependency.

## 6. Command Registry

### 6.1 Placement

| Module | Kind | Loaded as |
|---|---|---|
| `src/editor/command-registry.js` | pure: no DOM, no Electron | `window.WrlCommandRegistry` / CommonJS |
| `renderer/command-bindings.js` | DOM binder for toolbar controls and the keydown dispatcher | `window.WrlCommandBindings` |

The pure module is placed beside `ui-state.js` and `scene-selection.js`, which
follow the same pattern. Both new scripts go into `editor.html` before
`editor.js`, and into `EDITOR_PAGE_SCRIPTS` in `script-load-order.test.js`.

### 6.2 Command record (input to `register`)

```js
{
  id: 'file.save',          // required; see §7
  label: 'Save',            // required; the command's user-facing name
  area: 'file',             // required; must equal the id's first segment
  run(ctx) { ... },         // required; may return a Promise
  enabled() { return ... }, // optional; default: always enabled
  checked() { return ... }, // optional; present ⇒ the command is a toggle/radio
  keys: ['Mod+S'],          // optional; one or more shortcut strings (§9)
  keyOwner: 'app',          // optional; 'app' (default) | 'editor' (§9.3)
}
```

Each field is there for a reason found in the inventory:

| Field | Why it exists |
|---|---|
| `enabled` | 17 hand-written `.disabled` writes (§4.2) |
| `checked` | four separate pressed-state writers (§4.3) |
| `keys`, more than one | zoom already accepts `=`, `+` and `add` (`ui-state.js:265`) |
| `keyOwner` | undo, redo and find are bound by CodeMirror, not the app (§9.3) |

Fields deliberately **left out**, because no evidence needs them yet:
- **`icon`.** Every button is text today. UI-1 can add it.
- **`when` context expressions.** `enabled()` covers everything.
- **Arguments or parameter schemas.** Value pickers stay controls (§7.4).
- **Priority or ordering.** That belongs to the surfaces.
- **A `busy` lock.** The handlers already guard re-entry, e.g. `S.saving`.

`ctx` is `{ source: 'toolbar' | 'keyboard' | 'menu' | 'api' }`. It is diagnostic
only. **A handler must not branch on `source`.** That rule is the guarantee that
every entry point behaves the same.

### 6.3 Registry surface

```js
const reg = WrlCommandRegistry.createCommandRegistry();

reg.register(record)      // → dispose(); throws ECOMMAND_DUPLICATE / ECOMMAND_INVALID
reg.has(id)               // → boolean
reg.get(id)               // → frozen descriptor {id,label,area,keys,keyOwner,isToggle} | null
reg.list()                // → descriptors in registration order (palette input, §20)
reg.isEnabled(id)         // → boolean; unknown id throws ECOMMAND_UNKNOWN
reg.isChecked(id)         // → boolean | null (null = not a toggle)
reg.execute(id, ctx)      // → {ok:true, value} | {ok:false, reason:'disabled'}
                          //   | Promise of the same; unknown id throws ECOMMAND_UNKNOWN
reg.keyBindings()         // → [{key, id}] normalized, for the dispatcher and conflict tests
reg.subscribe(fn)         // → unsubscribe(); fn() is called after invalidate()
reg.invalidate()          // ask surfaces to re-read enabled/checked
reg.dispose()             // drop all commands and listeners (page teardown, tests)
```

- `get` returns a frozen descriptor that **does not** include `run`. A surface can
  never call a handler directly.
- `execute` evaluates `enabled()` immediately before `run`.

### 6.4 Failure policy

- **Unknown id: throw.** `ECOMMAND_UNKNOWN` is a programming error, never a user
  state.
- **Disabled: refuse, do not throw.** `{ok:false, reason:'disabled'}`, and `run`
  is never called. The refusal is silent, the same as clicking a disabled button
  today.
- **Handler throws or rejects.** The exception reaches the caller unchanged.
  `execute` mutates no registry state before or after `run`, so a throw cannot
  corrupt the registry. The dispatcher and binder do not catch it either. It
  surfaces through `window.onerror` / `unhandledrejection` as it does from today's
  listeners. Existing handlers keep their own user-facing `try/catch`
  (`doExternal` `:451`, `doSave`).
- **A listener throws.** It is isolated per listener, the same rule as
  `scene-selection.js`. One faulty surface must not stop the others repainting.
  The error is logged with `console.error` rather than swallowed silently.
- **Register after dispose.** Throws `ECOMMAND_DISPOSED`.

### 6.5 Instance and ownership

- `editor.js` creates **one** registry per page load. It registers commands whose
  `run`, `enabled` and `checked` close over the existing state: `S`, `EP()`,
  `sceneSelection` and `WrlPreferences`.
- The registry **holds behaviour references, not state**. Every `enabled()` and
  `checked()` reads the authoritative state when called.
- The registry is exposed read-only on the existing QA hook object
  (`window.__wrlEditor.commands`, i.e. `execute` / `isEnabled` / `isChecked` /
  `list`) for Electron QA. This adds no new global surface for product code.

## 7. Command IDs

### 7.1 Naming rules

1. The form is `<area>.<name>[.<variant>]`. Each segment is lowerCamel ASCII
   `[a-z][a-zA-Z0-9]*`. The full pattern is
   `/^[a-z][a-zA-Z0-9]*(\.[a-z][a-zA-Z0-9]*){1,2}$/`.
2. The area is one of `file edit view workspace preview selection model panel`.
   It must equal `record.area`.
3. The name reuses the current vocabulary. `resolveShortcut` already returns
   `save`, `saveAs`, `gotoLine`, `close`, `zoomIn`, `zoomOut`, `zoomReset`,
   `previewUpdate` and `previewMaximize` (`ui-state.js:257-271`). Those become the
   name segments.
4. Ids are **stable API** once #35 lands. Labels may change; ids may not.
5. An id is a string *in the renderer*. It is never a function name, an IPC
   channel or a path (§19).

### 7.2 Table

| id | label | from | keys (owner) | pure migration? |
|---|---|---|---|---|
| `file.back` | Back to … (dynamic) | `doBack` | — | yes |
| `file.save` | Save | `doSave` | Mod+S | yes |
| `file.saveAs` | Save As… | `doSaveAs` | Mod+Shift+S | yes |
| `file.reload` | Reload | `doReload(false)` | — | yes |
| `file.openExternal` | External editor | `doExternal` | — | yes |
| `file.close` | Close | `doClose` | Mod+W | yes |
| `edit.undo` | Undo | `handle.undo` | Mod+Z (editor) | yes |
| `edit.redo` | Redo | `handle.redo` | Mod+Y, Mod+Shift+Z (editor) | yes |
| `edit.find` | Find | `handle.openSearch` | Mod+F (editor) | yes |
| `edit.replace` | Replace | `handle.openSearch` | — | yes (same handler as today) |
| `edit.gotoLine` | Go to line… | `doGotoLine` | Mod+G (app; see §9.4) | yes |
| `view.zoomIn` | Increase size | `applyZoom(step +1)` | Mod+=, Mod++, Mod+Add | yes |
| `view.zoomOut` | Decrease size | `applyZoom(step −1)` | Mod+-, Mod+_, Mod+Subtract | yes |
| `view.zoomReset` | Reset size | `applyZoom(DEFAULT)` | Mod+0 | yes |
| `view.highContrast` | High Contrast | `setHighContrastEnabled` | — | yes (checked) |
| `view.preferences` | Preferences | `WrlPreferences.show` | — | yes |
| `workspace.code` | Code | `setWorkspaceMode('code', true)` | — | yes (checked) |
| `workspace.model` | Model | `setWorkspaceMode('model', true)` | — | yes (checked) |
| `workspace.toggleSource` | Show/Hide Source | `setSourceOpen(!open)` | — | yes (checked) |
| `workspace.play` | Play | — | — | **new behaviour** (§15) |
| `workspace.reset` | Reset Workspace | — | — | **new behaviour** (§16.3) |
| `preview.update` | Update | `manualUpdate` | Mod+Enter | yes |
| `preview.showSaved` | Show saved version | `showSaved` | — | yes |
| `preview.toggleMaximize` | Maximize | `toggleMaximize` | Mod+Shift+Enter | yes (checked) |
| `preview.layout.split` | Split | `setLayout('split')` | — | yes (radio-checked) |
| `preview.layout.previewMax` | Preview maximized | `setLayout('preview-max')` | — | yes (radio-checked) |
| `preview.layout.editorOnly` | Editor only | `setLayout('editor-only')` | — | yes (radio-checked) |
| `preview.findNewFiles` | Find new files | `findNewFiles` | — | yes |
| `model.addBox` | Box | `addObject('Box')` | — | yes |
| `model.addSphere` | Sphere | `addObject('Sphere')` | — | yes |
| `model.duplicate` | Duplicate | `duplicateSelected(sel)` | — | yes |
| `model.delete` | Delete | `deleteSelected(sel)` | — | yes |

30 existing commands and 2 new.

`selection.*` and `panel.*` are reserved areas with no commands in #35:
- WD2-E / WD2-F will add `selection.*`.
- `panel.<id>.focus` commands are owner-approved (D6) and registered by UI-0-I2 [#121](https://github.com/Ascendance3D/wrlforge/issues/121)
  (§11.4), with no default keys.

### 7.3 Model-bar result messages

The Model handlers return `{ok, message}`. Today `act()` writes that message to
`#modelStatus` (`model-workspace.js:84-87`). That moves into the command's `run`,
which calls `modelWorkspace.setStatus` with the same text. **The message must not
depend on `ctx.source`.** A keyboard-invoked Add Box must report exactly what a
click reports.

### 7.4 Value pickers stay controls

The Theme `<select>` (5 themes) and `previewLayoutSelect` are value pickers.
- The layout select is bound to the three `preview.layout.*` radio commands. It
  shows the one whose `checked()` is true, and runs that command on change.
- The theme select stays a `WrlPreferences` control in #35. Theme is a preference
  value, not an action. `view.highContrast` covers the one theme toggle that
  accessibility needs.

## 8. Enabled and checked state

### 8.1 Enabled

Enabled state is **derived on read**, from the state that already exists. No
command carries a stored boolean.

| Group | `enabled()` |
|---|---|
| file.save … file.close, edit.* | `UI.toolbarModel({open, dirty, saving})[key].enabled`. `toolbarModel` stays the pure source of truth, and its existing tests keep their meaning. |
| file.back, view.*, preview.update/showSaved/maximize/layout | `true`, matching today |
| preview.findNewFiles | `!EP().isRescanning()`. Today the in-flight state exists **only as the button's `.disabled`** (`editor-preview.js:364/:367`). #35 moves it into one `St` boolean behind a public read accessor, so that the DOM is no longer the state. |
| model.addBox/addSphere | `!!S.handle` (matches `model-workspace.js:70`) **and** `preset.visualAuthoring` |
| model.duplicate/delete | the selection is a Node (matches `:75`) **and** `preset.visualAuthoring` |
| workspace.code/model | `true` |
| workspace.toggleSource | `S.workspaceMode === 'model'`. The button's `hidden` rule stays in `paintBar`, because visibility is a layout concern. |

`preset.visualAuthoring` is the workspace gate (§12.3). It is `true` in Code and
Model, so the migration changes nothing. It is `false` in Play. WD2-E transform
commands will use the same gate (§20.3).

### 8.2 Checked

`checked()` reads the authoritative state directly. No mirror booleans:

| Command | `checked()` reads |
|---|---|
| workspace.code / model / play | `S.workspaceMode === '<mode>'` |
| workspace.toggleSource | `S.sourceOpen` |
| preview.toggleMaximize | `EP()._state().layout === 'preview-max'` |
| preview.layout.* | `EP()._state().layout === '<layout>'` |
| view.highContrast | `WrlPreferences.get('theme') === 'contrast'` |

#35 must replace the private `_state()` with a small public read accessor on
`wrlEditorPreview` (`getLayout()`, `isRescanning()`; the latter backed by the new `St` flag). Product code must not depend
on a QA-only underscore method.

## 9. Keyboard architecture

### 9.1 One dispatcher

`WrlCommandBindings.installKeyboard(reg, { target: window })` adds **one**
keydown listener. It replaces the `if/else` chain at `editor.js:526-539`. The
flow:

```text
keydown → normalize(e) → reg.keyBindings() lookup → (suppression policy §9.5)
        → e.preventDefault() → reg.execute(id, {source:'keyboard'})
```

**A claimed shortcut is consumed even when its command is disabled.** That
matches today: the window listener calls `preventDefault` whenever
`resolveShortcut` matches, and `doSave` self-guards. It stops a disabled Mod+S
from falling through to browser behaviour.

### 9.2 Shortcut grammar and normalization

- **Syntax.** `[Mod+][Ctrl+][Alt+][Shift+]<key>`. `<key>` is a lowercased
  `KeyboardEvent.key`, or a named key: `Enter`, `Escape`, `Delete`, `Add`,
  `Subtract`, `F1`–`F12`.
- **`Mod`** matches `ctrlKey || metaKey` on **every** platform. That is exactly
  today's `ctrlOrMeta` (`editor.js:527`), so Linux and Windows behaviour is
  preserved.
- **Display.** `Mod` is shown as "Ctrl" on Linux and Windows and "⌘" on macOS.
  The platform comes from `navigator.userAgentData?.platform || navigator.platform`.
  This is display only and does not affect matching.
- **Shift is significant only for letters and named keys** (`S`, `G`, `W`,
  `Enter`). For digits and punctuation it is ignored, because the keyboard layout
  needs Shift to produce them. This rule reproduces `resolveShortcut` exactly.
  For example, `Mod+0` fires with or without Shift, and `Mod+Shift+G` does not
  fire `edit.gotoLine`.
- **`e.repeat` is not filtered.** Today's listener does not filter it either.
- **Parity check.** A #35 test feeds the full `resolveShortcut` input space
  through both resolvers and asserts the same command for every input. After
  that, `resolveShortcut` can be retired or reduced to a wrapper.

### 9.3 Shortcut ownership

Each binding has exactly one owner:

- **`keyOwner: 'app'`.** The registry dispatcher runs it.
- **`keyOwner: 'editor'`.** CodeMirror's keymap runs it: undo, redo and find. The
  registry records the key **for display, conflict tests and the future palette
  only**. The dispatcher **ignores** it, and nothing calls `run` twice. The
  toolbar button still invokes `edit.undo`, whose `run` calls `S.handle.undo()`,
  the same CodeMirror `undo(view)` command (`editor-view.js:352`).

Rules:
1. No two `app` bindings may share a normalized key. This is checked at register
   time and throws `ECOMMAND_KEY_CONFLICT`.
2. An `app` key that collides with a **known CodeMirror binding** must appear in
   the conflict table (§9.4) with an explicit resolution. A test reads
   `editor-view.js`'s keymap list together with the table and fails on any
   collision that is not in the table.
3. UI-0 adds **no new default shortcuts**. In particular, Code, Model and Play get
   no workspace shortcuts (owner decision D4, §25).

### 9.4 CodeMirror conflict policy

| Key | CodeMirror | App | Current (to be confirmed by Q-K3) | Proposed |
|---|---|---|---|---|
| Mod+G | `findNext` (`searchKeymap`, preventDefault) | `edit.gotoLine` | probably both run | **app wins** (D2, accepted) |
| Mod+Enter | `insertBlankLine` (`defaultKeymap`) | `preview.update` | probably both: blank line + preview update | **app wins** (D2, accepted) |
| Mod+Z / Y / Shift+Z / F | history and search | — (`keyOwner:'editor'`) | CM only | unchanged |

There are two consistent resolutions:
- **Capture-phase precedence (accepted, D2).** The dispatcher listens in the
  capture phase. For an `app` binding that the conflict table marks
  `app-wins`, it calls `stopPropagation` so CodeMirror never sees the key. The
  result:
  - Ctrl+Enter always updates the preview and never inserts a line.
  - Ctrl+G always opens Go to line.
  - Find-next stays on Enter / F3 inside the search panel.

  This is a behaviour change, owner-approved as D2 and implemented by #121.
- **Bubble phase with a `defaultPrevented` yield.** The app skips any event that
  CodeMirror already handled. In that case Ctrl+Enter inside the source only
  inserts a line, and Ctrl+G only does find-next. That is also a behaviour change,
  in the opposite direction.

**Owner decision D2 (accepted):** capture-phase precedence. The WRL Forge
command wins where the conflict table marks the shortcut application-owned;
CodeMirror-owned keys (undo, redo, find) stay `keyOwner:'editor'`. The final
behaviour must be proven by runtime tests.

Pure migration (bubble phase, no yield) keeps today's double-fire. #35 implements
pure migration only and does **not** apply D2. The D2 resolution is an
intentional behaviour change implemented by UI-0-I2 [#121](https://github.com/Ascendance3D/wrlforge/issues/121), after the #35 Wave 0 baseline.

### 9.5 Suppression policy

| Situation | `Mod` shortcuts (all current shortcuts) | Bare-key shortcuts (none today; e.g. future WD2-E W/E/R, Delete) |
|---|---|---|
| focus in `input`/`textarea`/`select`/`contenteditable` | **fire.** This preserves today's behaviour: Ctrl+S saves from an Inspector field. | **suppressed** |
| focus in CodeMirror | fire (subject to §9.4) | **suppressed** |
| Preferences dialog open | fire; the dialog traps only Tab and Escape (preferences.js:549) | suppressed |
| `showModal` / recovery prompt open | fire in #35 (today's behaviour, preserved by pure migration). **Suppressed by UI-0-I2 [#121](https://github.com/Ascendance3D/wrlforge/issues/121)** (D3, accepted) unless a dialog action explicitly owns the key | suppressed |
| Play workspace, focus in viewport | fire unless the command is disabled by preset | suppressed: authored world interaction wins (§15) |

The bare-key rule is the guard future authoring shortcuts need. Bare keys belong
to text entry and to the world (X3D `KeySensor` / `StringSensor` in Play). Only
`Mod` shortcuts can be global.

### 9.6 Browser and Electron reserved keys

- On Linux and Windows, Electron's default menu (§3.1) has accelerators for:
  Reload Ctrl+R, Force Reload Ctrl+Shift+R, Toggle DevTools Ctrl+Shift+I, the zoom
  roles Ctrl+0/Ctrl+Plus/Ctrl+−, Quit, Full screen F11, and the edit roles.
- Rule: **#35 registers no new app shortcut on any of those keys.** The existing
  Ctrl+0/+/− overlap stays as it is.
- Electron QA case Q-K4 must verify that app zoom does not also trigger Chromium
  page zoom. If it does, that is a pre-existing defect. It goes to the menu
  decision (§10.3); #35 does not quietly fix it.

## 10. Toolbar and menu integration

### 10.1 Toolbar binding

Markup keeps its buttons, but they lose their behaviour wiring:

```html
<button id="saveBtn" data-command="file.save" disabled>Save</button>
```

`WrlCommandBindings.bindControls(reg, root)` runs once after `editor.js` has
registered its commands. For each `[data-command]` element it does the following:

| Concern | Binding |
|---|---|
| click | `reg.execute(id, {source:'toolbar'})` and nothing else |
| enabled | `el.disabled = !reg.isEnabled(id)` on every registry notification |
| checked | if `isToggle`, `aria-pressed = String(reg.isChecked(id))` |
| label | the existing text and `aria-label` are **kept** (pure migration). New controls with no text get `label`. |
| tooltip / shortcut display | the binder generates `title` from `label` plus the shortcut display: "Increase size (Ctrl+=)". This replaces the hand-copied shortcut text in `title`. It is a minor visible change (D5, accepted), implemented by #121; #35 keeps the hand-written titles. |
| `aria-keyshortcuts` | set from `keys`. **New a11y behaviour** (D5, accepted), implemented by #121. |
| unknown `data-command` | throws at bind time. A missing command is a build error, not a dead button. |

- **One click path per button.** The binder adds the only click listener. #35
  deletes the direct `addEventListener('click', …)` calls at `editor.js:499-512`,
  `model-workspace.js:81-91` and `editor-preview.js:535-543` for every migrated
  control. A source-scan test (§22) asserts they are gone.
- **What stays outside the binder.** Element *visibility* stays where it is now:
  `paintBar` sets the Source toggle's `hidden`, and `render()` sets the Back
  label. That is layout, not command state. `paintBar` stops writing `.disabled`
  and `aria-pressed`.
- **Repaint triggers.** `reg.invalidate()` is called in a small, fixed set of
  places:
  - at the end of `render()` (`editor.js:188`), the existing central refresh
  - in `applyWorkspace()`
  - in `applyLayout()` (`editor-preview.js:111`)
  - in the `sceneSelection` subscription
  - in the `WrlPreferences` subscriber
  - around `findNewFiles` in-flight

  No other code writes command state.

### 10.2 Menu: the current state

There is **no application menu**. Today no menu path exists, so nothing is
duplicated there. Electron's default menu is installed implicitly, and its roles
act on the webContents directly. They do not reach the app's commands.

### 10.3 Menu: the design (D1 accepted; built by UI-0-M)

When a menu is built, the menu bridge follows these rules:

1. **Main owns a static template.** Each item is a fixed command id from a frozen
   list in main, for example `{ label: 'Save', commandId: 'file.save',
   accelerator: 'CmdOrCtrl+S' }`. **The accelerator is display only**
   (`registerAccelerator: false`), so the key still reaches the renderer
   dispatcher and runs exactly once.
2. **Main → renderer, one channel, ids only.** Clicking an item calls
   `webContents.send('ui:command', id)`. Main never sends arguments.
3. **The preload exposes a narrow subscription.**
   `window.vrmlpad.ui.onCommand(cb) → unsubscribe`. It validates `typeof id ===
   'string'` against the id pattern (§7.1) before calling `cb`. This is the first
   `ipcRenderer.on` in the codebase. It must remove its listener on unsubscribe
   and page unload.
4. **The renderer revalidates.** `reg.has(id)`, then `reg.execute(id,
   {source:'menu'})`. Unknown ids are logged and dropped. A disabled command
   refuses exactly as it does from the toolbar.
5. **No renderer → main execution path.** The renderer never asks main to "run" a
   command id. Commands that need main keep calling their existing, specific
   `window.vrmlpad.editor.*` methods.
6. **Menu enabled/checked state** needs renderer → main state sync, for example a
   debounced `ui:commandState` carrying `{id: {enabled, checked}}`. That is a
   second channel. The alternative is that menu items stay always-enabled and
   refuse on execute. UI-0-M [#122](https://github.com/Ascendance3D/wrlforge/issues/122) designs and implements this synchronization as
   required, within the constraints above.

**#35 builds no menu and adds no IPC.** The menu bridge is deferred from the
registry-foundation implementation to its own UI-0 implementation sub-lane,
UI-0-M [#122](https://github.com/Ascendance3D/wrlforge/issues/122) (D1, accepted). It remains **required before UI-0 #33 closes**. The reason
for the separate lane is that it is the only part of UI-0 that crosses the
main / preload / IPC security boundary. Until it lands, the default Electron menu
stays exactly as it is.

## 11. Panel Registry

### 11.1 Purpose and limits

The Panel Registry **describes** panels: identity, title, how to focus them,
whether they are currently visible, and which ones can be shown or hidden.
- It is not a layout engine.
- It stores no geometry, order, tabs or sizes. Those belong to UI-1.
- It owns no document data. Panels keep reading `S`, `sceneSelection` and the
  analysis as they do now.

### 11.2 Panel record

```js
{
  id: 'sceneTree',                // lowerCamel, stable
  title: 'Scene',                 // visible heading
  element: () => HTMLElement,     // the panel's root; looked up lazily
  focus() { ... },                // optional; default: focus first focusable in element()
  canToggle: false,               // true only where a show/hide mechanism exists today
  show() {}, hide() {},           // required iff canToggle
}
```

`isVisible(id)` is **derived, never stored**. It is true when the element is
rendered: `element().getClientRects().length > 0`. The CSS and the workspace and
layout classes stay the single authority for visibility.

### 11.3 Registry surface

```js
const panels = WrlPanelRegistry.createPanelRegistry();
panels.register(record)   // → dispose(); throws EPANEL_DUPLICATE / EPANEL_INVALID
panels.has(id) / panels.get(id) / panels.list()
panels.isVisible(id)      // → boolean; unknown id throws EPANEL_UNKNOWN
panels.focus(id)          // → {ok:true} | {ok:false, reason:'hidden'}; unknown throws
panels.show(id) / hide(id)// → {ok:false, reason:'not-toggleable'} unless canToggle
panels.dispose()
```

`focus` on a hidden panel **refuses**. It does not open the panel as a side
effect. Opening is a workspace or layout decision.

The pure module is `src/editor/panel-registry.js` (`window.WrlPanelRegistry`).

### 11.4 Panel identities (input to UI-C0 / UI-1)

| id | title | element | canToggle today | Visibility today |
|---|---|---|---|---|
| `source` | Source | `#editorCol` | **yes**, in Model (`S.sourceOpen`) | Code: shown unless `layout-preview-max`. Model: only with `source-open`. |
| `preview` | Preview | `section.preview-col` | via layout (`editor-only` hides it) | Model: always |
| `object` | Object | `section.object-section` / `#objectProps` | no | with the sidebar |
| `outline` | Outline | `section.outline` / `#outlineList` | no | sidebar; hidden in Model |
| `sceneTree` | Scene | `#sceneTree` | no | sidebar |
| `inspector` | Inspector | `#sceneInspector` | no | sidebar |
| `diagnostics` | Diagnostics | `#diagList` section | no | sidebar |
| `advisories` | Advisories | `#advList` section | no | sidebar |
| `console` | — | — | — | **reserved id**, not registered. No console panel exists. |

- Everything in the sidebar shows when the layout is `split`, or in Model. It is
  hidden by `layout-preview-max` (`editor.html:126`), and Model forces it on
  (`:262`).
- "Validation" in UI-0 means the `diagnostics` and `advisories` panels.
- Panel commands (D6, accepted; implemented by UI-0-I2 [#121](https://github.com/Ascendance3D/wrlforge/issues/121)) are:
  - `panel.<id>.focus` for every panel
  - `panel.<id>.toggle` only where `canToggle`

  `workspace.toggleSource` stays the toggle for `source`; it is not duplicated.

### 11.5 Panel state rule

Panel visibility, focus and (later, in UI-1) geometry are UI state:
- They are never written into WRL/VRML source.
- They are never read during analysis.
- They never touch undo history or `sceneSelection`.

A test asserts that `currentText()` and `historyDepth()` are unchanged across
every panel and workspace operation (§22).

## 12. Workspace authority

### 12.1 Decision: keep `S.workspaceMode` + `setWorkspaceMode` as the one authority, wrapped by commands

**Evidence:**
- **There is already one writer.** Every mode change goes through
  `setWorkspaceMode` (`editor.js:871`). Its callers are the Model and Code
  buttons through `deps.setMode` (`:949`), cold open (`:1106`) and recovery
  restore (`:1076`). No surface keeps its own copy:
  - the toolbar reads it through `deps.getMode`
  - picking is driven from `applyWorkspace`
  - the CSS reads the classes on `#editorMain`
- **The side effects are tied into `editor.js` closures.** `applyWorkspace`
  (`:785`) arms picking with `viewportPickHandlers`, uses `EP()`, and coordinates
  with `noteDamage`, `S.sourceOpen` and `clearPickStatus`. Moving the authority
  into a new controller module would mean injecting all of that. It would be a
  large refactor inside what is meant to be a behaviour-preserving migration,
  and nothing breaks today because it hasn't been done.
- **What actually needs fixing is the policy, not the ownership.** Today the
  rules are expressed as scattered `=== 'model'` checks (`:775`, `:787`, `:820`,
  `:858`), and the mode sets are duplicated (§4.7).

### 12.2 What #35 changes

1. **`setWorkspaceMode(mode, persist)` stays the only writer.**
   - It is reached from `workspace.*` commands (persist = true) and from document
     open (persist = false).
   - After #35, `deps.setMode` in `model-workspace.js` is deleted. The buttons are
     `data-command` bound.
2. **`applyWorkspace()` reads a preset (§12.3) instead of testing `=== 'model'`.**
   It ends with `modelWorkspace.refresh()` and `reg.invalidate()`. That removes
   problem §4.8, because a switch repaints every surface from the one authority.
3. **The mode list has one definition.** `WORKSPACE_PRESETS` keys. A test asserts
   that `ui-state.js` `WORKSPACE_MODES` and `src/settings/preferences.js`
   `PREF_WORKSPACE_MODES` are equal to the **persistable** subset (§17).

**Rejected:**
- a separate workspace controller module. As above, it adds injection without
  removing a real duplicate.
- per-surface mode flags.
- deriving the mode from CSS classes. That would make the DOM the authority.

### 12.3 Workspace presets

The preset table is a pure frozen table in `src/editor/workspace-presets.js`. It
holds semantics only:

```js
WORKSPACE_PRESETS = {
  code:  { label: 'Code',  composition: 'source-primary', viewportPicking: false,
           visualAuthoring: true,  sourceEditing: true,  persistable: true },
  model: { label: 'Model', composition: 'visual-primary', viewportPicking: true,
           visualAuthoring: true,  sourceEditing: true,  persistable: true },
  play:  { label: 'Play',  composition: 'visual-primary', viewportPicking: false,
           visualAuthoring: false, sourceEditing: false, persistable: false },
}
```

| Field | Meaning |
|---|---|
| `composition` | A **semantic** name that UI-1 maps to a dock layout. Today it maps to the `workspace-model` CSS class (`visual-primary`) or no class (`source-primary`). |
| `viewportPicking` | The argument `applyWorkspace` passes to `EP().armPicking`. The WD2-D O2 rule becomes data. |
| `visualAuthoring` | Gates `model.*` and future WD2-E/F commands (§8.1). |
| `sourceEditing` | Gates source-mutating commands that are not themselves authoring: `edit.undo/redo/replace`, `file.reload`. In Play the source pane is not shown and the document must not change underneath a running world (§15). |
| `persistable` | Whether the mode may be the remembered startup mode (§17). |

Authored interaction is **not** a preset field. X_ITE sensors and Anchors are
active in every workspace, as they are today (§15.3).

### 12.4 Name: "Code"

Keep **Code**. It is the shipped toolbar label (`editor.html:425`), the
persisted value (`'code'`) and the WD2-C/WD2-D vocabulary. The pane inside it is
called "Source" (`sourceToggleBtn`). Renaming either would break persisted
preferences and the docs for no gain.

## 13. Code workspace (pure migration, current behaviour verified)

- The source is primary. `#editorMain` has no `workspace-model` class. The
  preview layout is `split`, `preview-max` or `editor-only`, remembered through
  `previewLayout`.
- **Viewport picking is inert.** There is no listener, no `touch()`, and no
  provenance request (`applyWorkspace` → `armPicking(false)`, `editor.js:795`;
  `editor-preview.js:443`).
- **Authored sensors and Anchors are active.** X_ITE receives every pointer event
  (WD2-D contract: no `preventDefault`).
- CodeMirror keeps its full keymap. No app binding takes a CodeMirror key beyond
  the two pre-existing collisions (§9.4).
- Scene Tree, Inspector and Object panel work wherever the sidebar is visible,
  through `sceneSelection`. Inspector Apply edits through `applyInspectorField`.
- **The Model bar is visible and Add/Duplicate/Delete work in Code**
  (`visualAuthoring: true`). That is current behaviour, and #35 preserves it.
  Whether to hide authoring tools in Code is a UI-1 composition question, not a
  UI-0 migration.

## 14. Model workspace (pure migration of WD2-C/WD2-D)

- Visual-primary composition. The class is `workspace-model`, with these CSS
  effects (`editor.html:258-278`):
  - the preview is large
  - the sidebar is forced visible
  - the outline is hidden
  - Source is collapsed into a toggle (`source-open`)
- Viewport picking is **armed** when compatibility is proven
  (`viewportPicking: true`).
  - A proven pick calls `sceneSelection.setSelection`.
  - Ambiguous, external, sensor-conflict, stale and unsupported picks **fail
    closed**. The selection is unchanged and the reason appears on
    `#modelStatus` (WD2-D contract).
- The **source caret does not follow** a viewport selection.
- Authored sensors and Anchors still fire under **Policy C**. A pick on them
  refuses with `REFUSED_SENSOR_CONFLICT`. UI-0 keeps that (D7).
- Entering Model with the layout `editor-only` and nothing displayed renders once
  (`editor.js:793`). Damaged documents open Source once (`noteDamage`).
- Future WD2-E transform commands register with
  `enabled: () => preset.visualAuthoring && <selection rules>`, and with
  `preset.viewportPicking` for viewport handles. UI-0 defines no transforms.

## 15. Play workspace (new behaviour, defined here, implemented by UI-0-I2)

### 15.1 Purpose

Play is deferred from #35, not from UI-0. It is not part of the
behaviour-preserving foundation; it is required UI-0 work implemented by UI-0-I2 [#121](https://github.com/Ascendance3D/wrlforge/issues/121)
before independent QA #36.

Play lets the author **use** the world as a visitor would. The primary rule:
**authored world interaction wins.**

### 15.2 Semantics

| Concern | Play |
|---|---|
| Composition | `visual-primary`: the same canvas as Model. The panel set is fixed by D8 (accepted, below). |
| Viewport editor picking | **off.** `armPicking(false)`: no listener, no `touch()`, no provenance parse, exactly as in Code. Picking status lines are cleared. |
| Authored sensors / Anchor | active and unobstructed. UI-0 adds no pointer handling on the canvas. |
| Visual authoring | **disabled:** `model.*` and future WD2-E/F commands. `enabled()` returns false, buttons show disabled, and keys are refused. |
| Source mutation | **disabled:** `edit.undo/redo/replace` and `file.reload` (`sourceEditing: false`). The Source pane is not shown. `edit.find` and `edit.gotoLine` are also disabled, because they act on a pane that is not visible. |
| Selection | `sceneSelection` is **kept and untouched.** The Scene Tree (if shown) still displays and changes it, but **viewport clicks never change it**. |
| Save / Save As / Close / Back / Preview Update / View / Workspace | enabled, as in Model |
| Bare-key shortcuts | suppressed whenever the viewport has focus, so world `KeySensor` input wins (§9.5) |
| Inspector / Object-panel edits | **Hidden** (D8, accepted). No edit surface is visible, and no read-only panel code is written. |

**Play composition (D8, accepted):** entering Play changes neither the document
nor the existing selection. The large preview, plus the Scene Tree
(read-only navigation, selection visible), plus Diagnostics. Inspector, Object,
Outline and Source are hidden. Under today's CSS that is `workspace-model` plus a
new `workspace-play` modifier that hides those sections.

### 15.3 What Play adds

**Code already behaves like Play inside the viewport.** Picking is inert and
sensors fire. Play therefore adds:
- a visual-primary composition with picking off
- authoring and source mutation disabled
- the bare-key policy

Play does **not** change X_ITE's behaviour, and needs no new X_ITE private API.

The opposite move belongs to WD2-E, not UI-0: suppressing sensor dispatch *in
Model* (spike §15 Policy D), so that Model clicks never trigger authored
behaviour. UI-0 keeps Policy C in Model (D7).

## 16. Workspace transitions

### 16.1 Matrix

All transitions go through `setWorkspaceMode` → `applyWorkspace`. **None of them
touch source text, the CodeMirror history or `sceneSelection`.**

| From → To | Changes | Stays |
|---|---|---|
| Code → Model | add `workspace-model`; `armPicking(true)` (re-renders once with provenance if the current scene was rendered disarmed, `editor-preview.js:443`); render once if `editor-only` and nothing displayed | text, history, selection, `sourceOpen`, the preview layout preference, the authored world state *unless* the provenance re-render reloads it |
| Model → Code | remove `workspace-model`; `armPicking(false)` (retires the pick map, no re-render); `clearPickStatus()` | text, history, selection, the displayed scene |
| Model → Play | add `workspace-play`; `armPicking(false)`; `clearPickStatus()`; disable authoring commands; **no preview reload** (the running world continues) | text, history, selection, the displayed scene and its world state |
| Play → Model | remove `workspace-play`; `armPicking(true)`. **Re-arming re-renders with provenance**, which resets the authored world state (current WD2-D behaviour, `renderedArmed`) | text, history, selection |
| Code ↔ Play | the union of the rows above | the same |

**Pending edits.** Play disables source mutation but does not freeze the preview
scheduler. An edit that was debounced before entering Play still renders. That
is expected: the world always reflects the current text.

**Restart.** "Restart the world" in Play is the existing `preview.update`.
Mod+Enter is enabled in Play. No new command is needed.

### 16.2 Focus on a workspace switch

- **Pure migration.** A toolbar click keeps focus on the clicked button, as today.
- **New, as part of Play.** Entering Play moves focus to the preview canvas, so
  keyboard input reaches the world. Leaving Play moves focus to the button that
  invoked the switch. If the switch came from the keyboard, focus goes to the
  Source editor in Code, or to the Model bar's first enabled control in Model.
  This mirrors the cold-open rule at `editor.js:1109`.

### 16.3 Reset Workspace (`workspace.reset`)

**Owned by UI-0 (semantic intent):** return the *current* workspace's
composition to its preset defaults. Today that means:
- `S.sourceOpen = false` in Model
- preview layout `split`
- split fraction 0.5, which writes the `previewLayout` and `previewSplit`
  preferences as user changes do today

Reset does **not**:
- change the workspace mode
- touch selection, source, history, zoom or theme

**Owned by UI-1:** the implementation becomes "restore the default dock layout
for this workspace preset". The command id and its meaning stay the same; only
the body of `run` changes.

## 17. Persistence

The existing mechanism is reused. Nothing new is added: `WrlPreferences` →
`localStorage` (`src/settings/preferences.js`).

| State | Persisted? | Where | Change |
|---|---|---|---|
| last workspace | yes, **code/model only** | `wrlforge.editor.workspaceMode` | **Play is never persisted** (`persistable: false`). Choosing Play writes nothing, so the remembered value stays the last authoring workspace. A document always opens in Code or Model, through the unchanged `initialWorkspaceMode` (D9). |
| preview layout and split | yes (existing) | `previewLayout`, `previewSplit` | none |
| theme, zoom, high contrast | yes (existing) | `theme`, `zoom`, `lastNonContrastTheme` | none |
| `sourceOpen` | **no**, per document as today | `S.sourceOpen` | none |
| panel visibility | **no** (derived, §11.2) | — | UI-1 owns layout persistence |
| command enabled/checked | **never** (derived) | — | — |

No UI state goes into the document, `settings.json`, the recovery store or main.

## 18. Accessibility

- **Mouse is never required.** Every command is reachable by keyboard: through a
  registered shortcut, or through a `data-command` control that is focusable,
  Tab-reachable and invoked with Enter or Space via `registry.execute`. A #35
  test enumerates the registry and asserts that each command has at least one of
  these (the acceptance mapping, §26).
- **Disabled and pressed state** stay native: `disabled`, `aria-pressed`. Screen
  readers get the same semantics, now from one source.
- **`aria-keyshortcuts`** advertises bindings (D5).
- **Focus ownership:**
  - Panel focus goes through `panels.focus(id)`, which refuses on hidden panels.
  - Workspace switches follow §16.2.
  - Modal focus traps are unchanged.
  - The registry itself never moves focus.
- **Zoom.** `view.zoom*` call the existing `applyZoom`, so the font compartment
  and the `--wrl-ui-scale` rem layer are unchanged.
- **High Contrast.** `view.highContrast` wraps `setHighContrastEnabled` + `WrlPreferences.set`.
- **Focus visibility.** No new CSS on focus. #36 QA checks focus rings at maximum
  zoom in High Contrast.

## 19. Security

- **No change to:**
  - the CSP (`editor.html:17-28`)
  - `contextIsolation: true` / `nodeIntegration: false` (`main.js:303-307`)
  - the preload surface
  - the IPC channels
  - the `app:goto` whitelist (`main.js:128`, `:1273`)
- **#35 adds no IPC.** The registry, panel registry and presets are renderer-local
  classic scripts, served from `'self'` under the existing `script-src`.
- **Command ids are not executable messages.**
  - `execute` looks up a closure registered by first-party code.
  - An unregistered id throws.
  - There is no `eval`, no dynamic `require` and no method lookup by name.
  - No command id crosses to main.
  - Commands that need main call their existing, specific bridge methods with the
    same arguments as today.
- **The menu bridge (D1, UI-0-M [#122](https://github.com/Ascendance3D/wrlforge/issues/122))** is main → renderer, ids only, checked against a
  pattern and an allow-list on both sides. There is no reverse "run this id"
  channel, so it cannot become a string-to-main execution path.
- **Play adds no capability.** Authored Scripts stay governed by the existing
  preview protections and SEC-SCRIPT-1 (#75). Play does not loosen the preview
  network guard (`main.js:221`) or the `wrlworld:` allow-list.

## 20. UI-C0 / UI-1 / WD2-E / WD2-K boundaries

### 20.1 UI-C0 (#38) receives
- The stable **panel ids** and titles (§11.4), and the reserved `console`.
- The **panel record contract**: element, focus, and derived visibility. The dock
  engine hosts `element()`; it does not own panel data.
- **Workspace semantics**: three presets with semantic `composition` names, and
  the rule that presets are not layouts (§12.3).
- **Reset intent** (§16.3): the dock engine must be able to "restore the default
  for preset X".
- A constraint: the engine must work with plain DOM elements, under the strict
  CSP, without a framework.

### 20.2 UI-1 (#43) receives
- The **command registry**. Dock tabs, panel menus and close buttons invoke
  `panel.*` and `workspace.*` commands. They never call handlers directly.
- The **panel registry**. UI-1 adds geometry and persistence alongside it, for
  example `panels.setHost(id, container)`. It does not change ownership.
- `composition` → layout mapping, and `workspace.reset`'s `run` body.

### 20.3 WD2-E (#50) receives
- The **`visualAuthoring`** and **`viewportPicking`** preset gates (§12.3).
- The **bare-key suppression policy** (§9.5), which is needed for W/E/R-style tool
  keys.
- The `selection.*` area, reserved for it.
- The Model sensor policy question (Policy C vs Policy D, D7).

### 20.4 WD2-K (#91) receives
- `reg.list()`, `get(id)`, `isEnabled(id)`, `execute(id, {source:'palette'})` and
  the shortcut display. The registry has no knowledge of any palette.

## 21. Implementation packaging and migration plan

Every stage keeps `npm run check` green. UI-0 implementation is tracked in
**three packages**, each a native sub-issue of #33, each starting only on its own
owner GO. The separation between them is mandatory.

### 21.1 Package A — behaviour-preserving migration: UI-0-I [#35](https://github.com/Ascendance3D/wrlforge/issues/35)

#35 owns **Waves 0–4 only**. It is a behaviour-preserving foundation and
migration lane. Existing behaviour stays unchanged except where absolutely
required to make the single command path function.

| Wave | Content | Kind |
|---|---|---|
| **0** | Measure and lock down current behaviour, before any code: Mod+G collision, Mod+Enter collision, default Electron menu zoom interaction, and the existing shortcut table (Q-K3, Q-K4). No behaviour change. | evidence |
| **1** | `command-registry.js`, `panel-registry.js` and `workspace-presets.js` (where pure) as pure modules, with unit tests. Script tags and the load-order list updated. **No callers.** | pure (additive) |
| **2** | Register File, Edit, View and Preview commands in `editor.js`. Add `command-bindings.js` (toolbar binder). Toolbar and preview buttons get `data-command`. The keydown chain is replaced by the keyboard dispatcher in bubble phase, with no yield (today's semantics). The direct click listeners are deleted. Parity test against `resolveShortcut`. **No D2/D3 behaviour correction in this wave.** | pure |
| **3** | Model bar command migration and `workspace.code/model/toggleSource` commands; workspace preset integration. `paintBar` keeps visibility only. `applyWorkspace` reads the presets and ends with `refresh` + `invalidate`. Mode-list single-source test. **No Play behaviour.** | pure |
| **4** | Register the panels from §11.4 (descriptive). No new UI. | pure (additive) |

### 21.2 Package B — intentional behaviour change: UI-0-I2 [#121](https://github.com/Ascendance3D/wrlforge/issues/121)

Blocked by #35. Implements the owner-approved behaviour that is intentionally
**not** pure migration:

- D2 application-owned Mod+G / Mod+Enter collision resolution
- D3 application shortcuts blocked behind dialogs
- D5 generated tooltip shortcut text and `aria-keyshortcuts`
- D6 `panel.*.focus` commands (no default keys)
- semantic `workspace.reset` (§16.3)
- the Play workspace (§15): Model ↔ Play transitions, Play command gating, Play
  focus behaviour (§16.2), D8 composition, D9 never-restored-at-startup
- D7 Model sensor Policy C regression coverage

Out of scope: the menu bridge, docking, transform tools, authoring features.
Constraints: source text unchanged by workspace transitions; CodeMirror remains
undo authority; `sceneSelection` remains selection authority; X_ITE remains the
renderer; no new runtime dependency; CSP unchanged; no new IPC.

### 21.3 Package C — main-process integration: UI-0-M [#122](https://github.com/Ascendance3D/wrlforge/issues/122)

Blocked by #35. Risk: **High** (main / preload / IPC boundary). Implements the
menu bridge exactly as §10.3 and §19 define it: a fixed main-process template of
application-defined command ids; one narrow one-way `ui:command` channel main →
renderer; a preload subscription API; renderer-side registered-id validation;
menu enabled/checked synchronization as required. Main never executes arbitrary
command strings. `contextIsolation`, `nodeIntegration` and the CSP are unchanged;
no filesystem exposure; the IPC model changes only by this reviewed channel.

### 21.4 Sequencing and the parent completion rule

```
UI-0-R #34 (this document) → #35 → { #121 , #122 } → UI-0-Q #36 → UI-0-C #37
```

- #121 and #122 are natively **blocked by #35**.
- Independent QA #36 is natively **blocked by #35, #121 and #122**. It must not
  run after #35 alone; it tests the architecture plus all required UI-0
  implementation (§23).
- Closeout #37 is natively **blocked by #36**.

**UI-0 #33 cannot close after #35 alone.** It completes only after:
1. UI-0-R #34 (this architecture) is merged;
2. #35 foundation is complete;
3. UI-0-I2 [#121](https://github.com/Ascendance3D/wrlforge/issues/121) is complete;
4. UI-0-M [#122](https://github.com/Ascendance3D/wrlforge/issues/122) is complete;
5. #36 independent QA is accepted;
6. #37 closeout is complete.

This is required because #33 promises toolbar, menu and keyboard command
consistency and the Model / Play modes. The menu bridge and Play are deferred
**from #35**, not from UI-0.

## 22. Test plan (#35, #121, #122)

The registry, key, binding and panel-registry suites below land with #35. The
Play, `workspace.reset`, D2/D3/D5/D6 and D9 assertions land with #121, and
menu-bridge tests (preload subscription, registered-id validation) with #122.

New files go under directories already listed in `scripts/run-tests.js` `DIRS`
(`test/editor`, `test/renderer`, `test/settings`). No runner change is needed.

**`test/editor/command-registry.test.js`**
- registration and the frozen descriptor (no `run` exposed)
- duplicate id and invalid id (pattern, area mismatch) → throws
- unknown id → `isEnabled`, `isChecked` and `execute` throw `ECOMMAND_UNKNOWN`
- execute: an enabled command runs once and returns the value
- a disabled command does not run and returns `{ok:false, reason:'disabled'}`
- `enabled()` is evaluated at execute time, not cached
- async: a resolved value and a rejection both propagate
- after a handler throws, the registry still lists, executes and notifies normally
- `checked`: `null` for non-toggles; it tracks the backing state with no stored
  copy
- subscription: `invalidate` notifies; `unsubscribe` stops it; a throwing
  listener is isolated
- dispose: listeners are cleared, and `register` throws afterwards

**`test/editor/command-keys.test.js`**
- grammar parse and normalize
- `Mod` matches ctrl and meta
- the Shift rule for letters and Enter vs digits and punctuation
- **exhaustive parity with `UI.resolveShortcut`**
- `app`/`app` key conflict → throws
- `keyOwner:'editor'` keys are never dispatched
- the CodeMirror conflict table covers every collision with the keymaps listed at
  `editor-view.js:222`
- display strings for linux, win32 and darwin

**`test/renderer/command-bindings-runtime.test.js`** (stub DOM, as in the
existing `*-runtime` tests)
- click → `execute(source:'toolbar')`
- keydown → `execute(source:'keyboard')`, and `preventDefault` even when disabled
- a disabled command refuses through every path
- `disabled` and `aria-pressed` repaint on `invalidate`
- an unknown `data-command` throws at bind time
- suppression: a bare key is suppressed in input and CodeMirror; `Mod` keys fire

**`test/renderer/editor-commands-runtime.test.js`**
- every `[data-command]` in `editor.html` resolves to a registered command
- **source scan:** no `addEventListener('click'` remains on migrated element ids
  in `editor.js`, `model-workspace.js` or `editor-preview.js`
- **keyboard coverage:** every registered command has `keys` or a focusable bound
  control (§26)
- the toolbar enabled state equals `toolbarModel` across the open, dirty and
  saving matrix

**`test/editor/panel-registry.test.js`**
- registration and duplicates
- unknown id throws
- visibility derived from the element
- `focus` on a hidden panel refuses
- `show`/`hide` on a non-toggleable panel refuses
- dispose

**`test/editor/workspace-presets.test.js`** and additions to
`test/renderer/model-workspace-runtime.test.js`
- presets are frozen
- the persistable subset equals `WORKSPACE_MODES` and `PREF_WORKSPACE_MODES`
- `armPicking` gets `preset.viewportPicking` for code, model and play
- Code is inert and Model is armed; in Play picking is inert and `model.*` is
  disabled
- **`currentText()` and `historyDepth()` are identical before and after** every
  transition, and after `workspace.reset`
- `sceneSelection` is unchanged across transitions
- Play is never written to preferences

**Updates:** `test/editor/script-load-order.test.js` (new scripts) and
`test/editor/ui-state.test.js` (if `resolveShortcut` becomes a wrapper).

## 23. Electron QA plan (#36, final combined UI-0 candidate)

UI-0-Q #36 independently verifies the **final combined UI-0 candidate**: this
design plus #35, #121 and #122 together. It does not run against the design
document alone, nor after #35 alone. Wave 0 baseline captures (Q-K3, Q-K4) are
taken by #35 and are the comparison point. Required coverage: registry behaviour,
toolbar, keyboard, application menu, enabled/disabled state, checked state, panel
registry, Code, Model and Play workspaces, Model → Play → Model, source byte
identity through workspace transitions, undo-depth preservation, `sceneSelection`
preservation, WD2-D picking only in Model, TouchSensor / Anchor in Play, command
collision resolution, dialog shortcut suppression, tooltip / `aria-keyshortcuts`,
panel focus, semantic Reset Workspace, High Contrast, maximum zoom, focus
behaviour, and security of the menu bridge.

Routing follows `CLAUDE.md`. All captures use `VisualQaRunner`. **The QA tool and
reviewer are the owner's choice.** This document picks none. Evidence goes in
`qa/ui0-commands/`, following the repository's existing `qa/phase-*` pattern.

| Case | Checks |
|---|---|
| Q-K1 | Each shortcut command invoked by keyboard does the same thing as its toolbar button: Save, Save As, Go to line, Close, Zoom ×3, Update, Maximize |
| Q-K2 | Disabled refusal. Ctrl+S on a clean document does nothing, and no browser default fires |
| Q-K3 | **Baseline (Wave 0) and after:** Ctrl+G and Ctrl+Enter with the source focused, showing what CodeMirror does and what the app does |
| Q-K4 | Ctrl+0/+/− change app zoom only, and the Chromium page zoom factor stays at 1 (default-menu interaction) |
| Q-T1 | Toolbar invocation of every `data-command` control; enabled/disabled screenshots in the no-document, clean, dirty and saving states |
| Q-T2 | Checked state: Code/Model/Play pressed, Maximize pressed, layout select in sync, High Contrast |
| Q-M1 | Menu invocation (#122): the menu item gives the same result as the toolbar and keyboard; a disabled item refuses |
| Q-M2 | Menu bridge security (#122): only registered ids execute; an unknown id is dropped; no renderer → main execution path; `contextIsolation`, `nodeIntegration` and CSP unchanged |
| Q-K5 | Collision resolution (#121, D2): Ctrl+G opens Go to line only; Ctrl+Enter updates the preview only; CodeMirror undo/redo/find unchanged |
| Q-K6 | Dialog suppression (#121, D3): with a modal or the recovery prompt open, application shortcuts do not fire |
| Q-A3 | Tooltip shortcut text and `aria-keyshortcuts` generated from the registry (#121, D5); `panel.*.focus` focuses a visible panel and never opens a hidden one (#121, D6) |
| Q-W1 | Code: picking inert (no pick status on click); a TouchSensor fixture fires |
| Q-W2 | Model: a proven pick selects; a DEF/USE pick refuses; the caret does not move (WD2-D regression) |
| Q-W3 | Model → Play → Model: the source bytes and the undo depth are identical, and the selection survives |
| Q-W4 | Play: a TouchSensor fixture toggles; an Anchor follows; a viewport click never changes the selection; Add/Delete are disabled; Ctrl+Z is refused |
| Q-W5 | Reset Workspace restores the preset defaults and leaves the mode, selection and source unchanged |
| Q-A1 | Keyboard-only walk: Tab reaches every command control, and focus is correct after each workspace switch (§16.2) |
| Q-A2 | High Contrast at maximum zoom (`ZOOM_MAX`): focus rings and disabled/pressed states legible (regression vs `qa/phase-7c-vision`) |

Linux first. Windows and macOS real-pointer coverage follows #119.

## 24. Risks

| # | Risk | Mitigation |
|---|---|---|
| R1 | Migration silently changes a keyboard path; for example the CodeMirror double-fire is "fixed" by accident when the listener phase changes | Wave 0 baseline; bubble phase with no yield in Waves 2–4; D2 applied only in #121 |
| R2 | Shared preview controls (`preview.js`, `world-preview.js`) are bound to the registry and break the Mall or World page | Only editor-page-owned elements get `data-command`; the shared preview controls stay widget-local (§3.2) |
| R3 | The default Electron menu zoom roles double-apply zoom | Q-K4; the fix goes to the menu bridge lane #122 |
| R4 | Re-arming picking on Play → Model resets the authored world state, which may surprise users | Documented (§16.1); it is current WD2-D behaviour, not new |
| R5 | Over-generalising the registry: a `when` language, a plugin model | The field list in §6.2 is evidence-gated; #36 QA should reject unjustified fields |
| R6 | New scripts collide in the shared classic-script scope | Module-unique names; the `script-load-order` test |
| R7 | `_state()` remains a product dependency | §8.2 requires public read accessors |
| R8 | Play's suppression of bare keys relies on focus being in the canvas | §16.2 moves focus on entry; Q-A1 verifies it |

## 25. Owner decisions (final)

All ten decisions were resolved by the owner on **2026-10-07**. None is open.
The only future owner authorization is permission to **start** an
already-defined implementation lane (#35, #121, #122), then #36 and #37.

| # | Decision | Final disposition |
|---|---|---|
| **D1** | Application menu | **ACCEPTED WITH PACKAGING RULE.** Not implemented in #35. The menu bridge is deferred from the registry-foundation implementation to its own UI-0 implementation sub-lane, UI-0-M [#122](https://github.com/Ascendance3D/wrlforge/issues/122), because it crosses the main / preload / IPC security boundary. It remains required for UI-0 completion. |
| **D2** | CodeMirror collisions on Mod+G and Mod+Enter | **ACCEPTED.** The WRL Forge application command wins where the architecture marks the shortcut application-owned. CodeMirror-owned shortcuts (undo, redo, find) are preserved through `keyOwner:'editor'`. The final behaviour must be proven with runtime tests. Implemented by #121. |
| **D3** | Application shortcuts behind dialogs | **ACCEPTED.** Application shortcuts do not execute behind an open modal/dialog unless a specific dialog action explicitly owns the key. Implemented by #121. |
| **D4** | Workspace shortcuts | **ACCEPTED.** No default keyboard shortcuts for Code / Model / Play in UI-0. The commands exist and remain keyboard-invocable through normal focusable UI controls. |
| **D5** | Shortcut labels | **ACCEPTED.** Tooltip shortcut text and `aria-keyshortcuts` are generated from the Command Registry. Implemented by #121. |
| **D6** | Panel focus commands | **ACCEPTED.** `panel.*.focus` commands with no default shortcuts. A focus command does not implicitly open a hidden panel. Implemented by #121. |
| **D7** | Model sensor policy | **ACCEPTED.** Keep Policy C: authored sensors and Anchors continue to win under the pointer; WD2-D source-proven editor picking refuses on those interactions. Regression coverage in #121. |
| **D8** | Play layout | **ACCEPTED.** Play uses the visual workspace with Scene Tree and Diagnostics available; editing panels are hidden by the Play preset. Entering Play changes neither the document nor the existing selection. Implemented by #121. |
| **D9** | Remember Play | **ACCEPTED.** Play is never restored as the startup workspace; startup may restore Code or Model only. Implemented by #121. |
| **D10** | Lane packaging | **REVISED BY OWNER.** Three tracked implementation packages (§21): **A** — #35, Waves 0–4, behaviour-preserving foundation and migration; **B** — UI-0-I2 [#121](https://github.com/Ascendance3D/wrlforge/issues/121), intentional behaviour change (D2, D3, D5, D6, `workspace.reset`, Play, D7 coverage, D8, D9); **C** — UI-0-M [#122](https://github.com/Ascendance3D/wrlforge/issues/122), main-process integration (menu bridge). #121 and #122 are blocked by #35. #36 is blocked by #35, #121 and #122. #37 is blocked by #36. UI-0 #33 closes only under the §21.4 completion rule. |

## 26. Acceptance mapping (parent #33)

| #33 criterion | How UI-0 proves it |
|---|---|
| Architecture doc approved by owner | This document: owner-accepted 2026-10-07 with D1–D10 final (§25), merged through the UI-0-R #34 PR |
| Registry foundation lands with tests | Wave 1 modules plus `command-registry.test.js`, `command-keys.test.js`, `panel-registry.test.js` and `workspace-presets.test.js`, green in `npm run check` and on the three-platform CI |
| Existing actions migrated without behaviour change | Waves 2–4 are pure. Evidence: (1) `resolveShortcut` parity test; (2) the toolbar enabled-state matrix equals `toolbarModel`; (3) a source scan shows no direct click listeners on migrated ids; (4) Wave 0 baseline vs post-migration Electron QA (Q-K1–Q-K4, Q-T1, Q-W1, Q-W2) are identical; (5) the existing WD2 runtime and visual suites are unchanged and green |
| Keyboard invocation covers every registered command | The `editor-commands-runtime` coverage test: every `reg.list()` entry has an `app` or `editor` key, or a bound focusable `[data-command]` control invoked through `execute`. Plus Q-A1, a keyboard-only Electron walk over every command control. Under D4 "none", coverage comes from focusable bound controls; there is no shortcut for every command. If the owner reads the criterion as "a shortcut for every command", that changes D4. |
| Toolbar, menu and keyboard command consistency | Toolbar and keyboard through #35 (pure migration) and #121 (D2, D3, D5); menu through UI-0-M [#122](https://github.com/Ascendance3D/wrlforge/issues/122). Verified together by #36 (§23) |
| Model / Play modes | Model through #35 (pure migration of WD2-C/WD2-D); Play through UI-0-I2 [#121](https://github.com/Ascendance3D/wrlforge/issues/121). Verified together by #36 (§23) |

#33 closes only under the completion rule in §21.4.
