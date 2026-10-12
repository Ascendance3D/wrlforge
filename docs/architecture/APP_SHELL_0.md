# APP-SHELL-0 — Version 1.4.5 Beta and workspace foundation

Lane: APP-SHELL-0. Target: the Rust/Tauri 2 application with the Leptos/Wasm UI
(`apps/desktop-tauri`).

Base: `c23646b57190f34bda85d8df3721d2babdd03ed5` (XITE-16-UPGRADE-2, Draft PR #140).
This lane stacks on that branch.

This lane sets the version and builds one command path for File and Edit. It
also writes the docking and menu contracts. It does not build the docking
system or the menu bar.

## 1. Version policy

| item | value |
|---|---|
| Rust/Tauri application version | **1.4.5** |
| Release channel | **Beta** |
| Identity string | `WRL Forge 1.4.5 (Beta)` |
| Application identifier | `org.ascendance3d.wrlforge.tauri` (unchanged) |
| Electron application (root `package.json`) | **1.4.0**, historical, unchanged |
| Reserved | **1.5.0** = the complete Rust release on Linux, Windows and macOS |
| Release tag | none created |

### 1.1 Where the version lives

| location | before | after |
|---|---|---|
| `apps/desktop-tauri/Cargo.toml` `[workspace.package] version` | 0.1.0 | 1.4.5 |
| `apps/desktop-tauri/src-tauri/tauri.conf.json` `version` | 0.1.0 | 1.4.5 |
| `apps/desktop-tauri/Cargo.lock` (5 workspace members) | 0.1.0 | 1.4.5 |
| `crates/Cargo.toml` `[workspace.package] version` | 0.0.0 | 0.0.0 (unchanged) |
| root `package.json` (Electron) | 1.4.0 | 1.4.0 (unchanged) |

The `crates/` workspace holds the shared core libraries. They are not the
application, and they are not published, so their version stays `0.0.0`.

The workspace version is the single source. `protocol::command::APP_VERSION`
reads it with `env!("CARGO_PKG_VERSION")`, so the backend and the Wasm UI
compile the same value.

### 1.2 Where the version is shown

| surface | text |
|---|---|
| Toolbar identity (`#app-identity`) | `WRL Forge 1.4.5 (Beta)` |
| Window title, no document | `WRL Forge 1.4.5 (Beta)` |
| Window title, document | `● name.wrl — WRL Forge 1.4.5 (Beta)` |
| Help → About (toolbar **About** button) | identity, version, package version, identifier, renderer, Tauri version, license |
| `wrl-forge --version` | `WRL Forge 1.4.5 (Beta)` (no window opens) |
| Linux package names | `WRL Forge_1.4.5_amd64.deb` / `.AppImage` (from `tauri.conf.json`) |

About compares three values: the backend version, the package version that
Tauri read from `tauri.conf.json`, and the UI build version. A difference
shows a warning line, and the warning is never hidden.

### 1.3 Guards

`src-tauri/tests/app_shell.rs` makes these checks:

- The Cargo version, the protocol constant and `tauri.conf.json` are equal, and the value is `1.4.5`.
- A version of 1.5 or later fails the test, because 1.5.0 needs its own release lane.
- The channel is `Beta`.
- The identifier is unchanged, and the Electron `package.json` stays `1.4.0`.
- `xite/PIN` names X_ITE `16.5.0`.

## 2. Command inventory

Status key:

- **registry**: the command runs through the APP-SHELL-0 registry.
- **direct**: implemented, but the command does not use the registry yet.
- **native**: the WebView text widget does the work.
- **missing**: not implemented in the Rust app.

The "backend" column names the Rust IPC commands. "UI" means that no IPC
call occurs.

### 2.1 File and Edit (registry)

| ID | name | shortcut | enabled when | backend | needs | error / refusal | status |
|---|---|---|---|---|---|---|---|
| `file.newWorld` | New World | Ctrl+N (Shift ignored) | always | `new_document`, `close_document` | — | Rust asks with a native dialog when the document is dirty. Cancel leaves the document unchanged. | registry |
| `file.open` | Open… | Ctrl+O (Shift ignored) | always | `open_document`, `close_document` | — | Refused while dirty ("Save or undo your changes…"). A dialog Cancel changes nothing. A bad file gives "Could not open: …". | registry |
| `file.save` | Save | Ctrl+S | document open | `doc_save`; `doc_save_as` for an untitled world | document | A conflict shows the banner and writes nothing. "Not saved: …" on failure. | registry |
| `file.saveAs` | Save As… | Ctrl+Shift+S | document open | `doc_save_as` | document | Cancel writes nothing. | registry |
| `file.close` | Close | — | document open | `close_document` | document | Refused while dirty. | registry |
| `file.reload` | Reload from Disk | — (conflict banner) | document open | `doc_reload` | document | Discards the buffer by the user's explicit choice. | registry |
| `edit.undo` | Undo | Ctrl+Z; native `historyUndo` | button: undo history exists | `doc_undo` | document | Rust answers "Nothing to undo". | registry |
| `edit.redo` | Redo | Ctrl+Y, Ctrl+Shift+Z; native `historyRedo` | button: redo history exists | `doc_redo` | document | Rust answers "Nothing to redo". | registry |
| `help.about` | About WRL Forge | — | always | `app_info` (read-only) | — | "Could not read the version: …" | registry (new) |

### 2.2 Other implemented actions (direct, not in the registry yet)

| proposed ID | name | shortcut | enabled when | backend | needs | refusal | status |
|---|---|---|---|---|---|---|---|
| `file.dismissConflict` | Dismiss (conflict banner) | — | banner shown | UI | — | — | direct |
| `app.quit` | Window close | OS | always | Rust `CloseGate` | — | A native confirmation appears for dirty sessions. | direct (Rust) |
| `edit.cut` / `edit.copy` / `edit.paste` / `edit.selectAll` | text clipboard | Ctrl+X / C / V / A | Source focused | `doc_edit` (cut, paste) | document | Rust verifies the edit. | native |
| `view.theme` | Theme | — (select) | always | `theme_get`, `theme_set` | — | A save failure shows a notice. | direct |
| `view.livePreview` | Live | — (toggle) | always | UI | — | — | direct |
| `view.updatePreview` | Update | — | always | `doc_preview_source` | — | Preview status text | direct |
| `view.resetNativeCamera` | Reset view | Home (native pane) | native viewport shown (hidden test feature) | `native_reset_camera` | — | — | direct |
| `create.box` … `create.extrusion` | Create ▾ Box, Sphere, Cylinder, Cone, Extrusion | menu keys (arrows, Home, End, Esc) | document open | `doc_create` | document, base revision | "Not created — …" | direct |
| `tools.select` | Select | — | document open | `doc_pick` | document | Pick refusal text | direct |
| `tools.move` | Move | — | document open | `doc_translate_target`, `doc_translate_prove`, `doc_translate` | selected top-level Transform | Gizmo refusal text | direct |
| `tools.rotate` | Rotate | Shift = 15° snap | document open | `doc_rotate` | selected Transform | Gizmo refusal text | direct |
| `tools.scale` | Scale | — | document open | `doc_scale` | selected Transform | Gizmo refusal text | direct |
| `tools.cancelDrag` | Cancel drag | Esc | during a drag | UI | drag active | — | direct |
| `tools.writeMissingField` | Write rotation / scale | — | the field is absent | `doc_add_field` | selection | Rust refusal | direct |
| `inspector.apply` | Apply (field) | Enter in a field | selection, current revision | `doc_edit_field`, `doc_edit_list`, `doc_add_field` | selection | "Not applied — …" | direct |
| `tree.select` | Scene Tree item | Arrow keys, Enter, Space | analysis current | UI (revision check) | analysis | Stale reason | direct |
| `extrusion.outline*` | Outline editor: open, set, add, insert, delete, apply, cancel | Arrows, Delete, Backspace | Extrusion selected | `doc_edit_list` (apply) | selection | Rust refusal | direct |
| `recovery.restore` / `.discard` / `.later` | Recovery dialog | — | records exist | `recovery_restore`, `recovery_discard` | — | Damaged-record text | direct |

### 2.3 Missing commands

| proposed ID | name | menu | note |
|---|---|---|---|
| `edit.find`, `edit.replace` | Find / Replace | Edit | Not implemented. Electron uses CodeMirror search. |
| `edit.goToLine` | Go to Line | Edit | Not implemented. |
| `edit.deleteNode`, `edit.duplicateNode` | Delete / Duplicate node | Edit | Needs source-proven node edits. |
| `view.zoomIn/Out/Reset` | Zoom | View | Electron has Feature A zoom. Rust does not. |
| `view.panel.*`, `view.resetLayout`, `view.preset.*` | Panels and presets | View | UI-DOCK-1 |
| `view.fullScreen` | Full Screen | View | Not implemented. |
| `world.mall.*` | Mall Item: validate, repack | World | Electron only. Not migrated. |
| `world.project.*` | World Project: open, scan, preview, package | World | Electron only. Not migrated. |
| `help.docs`, `help.shortcuts`, `help.licenses` | Help items | Help | Not implemented. |

### 2.4 Defects found by the inventory (not changed)

- The shortcuts work only while the Source textarea has focus. The handler is on the textarea. A menu lane must add a window-level handler.
- On macOS, the handler accepts Ctrl and Command for every shortcut, which is the behavior before the registry. The labels already show `⌘`. A macOS lane must decide whether Ctrl alone stays accepted.
- Alt is not examined, so Ctrl+Alt+S saves. The registry keeps this behavior. AltGr layouts need a review before a change.
- After Close, the window keeps the last document title. This is the pre-existing behavior.
- At a 1440-pixel window width, the viewport header clips **Rotate** and **Scale**, because the header has no overflow rule. UI-DOCK-1 must fix this (see §3.6).

## 3. Command registry

### 3.1 Shape

`apps/desktop-tauri/protocol/src/command.rs` holds the registry. It contains data only, and both the backend and the UI compile it.

- `CommandId`: a typed enum with one entry for each command.
- `CommandSpec`: the stable string ID, the label, the menu, the shortcuts, the requirement, the action, the `mutates_document` flag and the hint.
- `Requires`: `Nothing`, `Document`, `UndoHistory` or `RedoHistory`.
- `Action`: `Backend(&[ipc names])` or `UiOnly`.
- `ALLOWED_BACKEND`: the only IPC commands that a registry action can name.
- `from_shortcut(key, primary, shift)`: one table for every key press.
- `Shortcut::label(platform)`: `Ctrl+Shift+S` on Linux and Windows, `⇧⌘S` on macOS.

`apps/desktop-tauri/ui/src/command.rs` holds the one UI path:

- `run(id)`: refuses a document command when no document is open. It then calls the function that the toolbar called before this lane.
- `on_keydown`: the Source shortcuts.
- `CommandButton`: a toolbar button. It gets its label, tooltip, `disabled` state and `data-command` from the registry.

### 3.2 Wired paths

All of these paths now go through `command::run`:

- the toolbar buttons New World, Open, Save, Save As, Close, Undo, Redo and About;
- the Source keyboard shortcuts;
- the native textarea `historyUndo` / `historyRedo`;
- the conflict banner buttons **Reload from disk** and **Save mine As…**.

### 3.3 Behavior kept

- Each command calls the same function as before: `ui::new_world`, `ui::open`, `ui::save`, `ui::close`, `ui::reload` and `editor::history`.
- The button `disabled` rules are the same as before. A test checks them.
- The shortcut table is equal to the old handler, key by key. A test checks this.
- Undo and Redo are not refused in the UI. The UI can be behind Rust for a just-typed edit, so Rust answers "Nothing to undo".

The one visible change: a document command with no document open now shows
"<Command> is not available: No document is open." Before this lane it did nothing.

### 3.4 Source-edit safety

No registry action edits text. Each action calls a Rust command that holds the document authority:

- `doc_undo` and `doc_redo` use the Rust history.
- `doc_save` and `doc_save_as` use the safe-write path.
- `new_document`, `open_document`, `close_document` and `doc_reload` own the sessions.

`ALLOWED_BACKEND` contains no `doc_edit*` and no `doc_add_field`, and a test checks this. Another test parses `generate_handler!` and proves that each allowed name is a registered Tauri command. The registry adds no new route into the document.

### 3.5 Rules for the next lanes

- Add a command to the registry first. A menu item or a button without a registry entry is not permitted.
- A `missing` command does not get a menu item, or it gets a disabled item with a reason (§5). It never gets an item that does nothing.
- Each new backend name must be added to `ALLOWED_BACKEND` on purpose. A document-mutating command must call a Rust command that verifies the edit.

### 3.6 Panel toolbars

The current viewport header uses one row and clips buttons. Each panel header in UI-DOCK-1 must wrap or collapse into an overflow (`…`) control. A command must never become hidden without a way to reach it.

## 4. Docking contract

This section is a contract. It is not implemented in this lane.

### 4.1 Panels

| panel ID | title | minimum size (CSS px) | can close | notes |
|---|---|---|---|---|
| `panel.sceneTree` | Scene Tree | 200 × 160 | yes | |
| `panel.viewport` | 3D Viewport | 320 × 240 | no | It hosts the X_ITE `<x3d-canvas>` or the native surface. |
| `panel.source` | Source | 320 × 200 | no | It holds the only editing textarea. |
| `panel.inspector` | Inspector | 260 × 160 | yes | |
| `panel.diagnostics` | Diagnostics | 260 × 100 | yes | |

Panel IDs are stable strings, and persisted layouts use only these IDs. An unknown ID in a saved layout is dropped. The tree is then normalized and validated as a whole (§4.2.1). If the result is not valid, the preset is used instead and a notice appears.

The Viewport and the Source cannot close. A Source panel must exist because it holds keyboard focus for editing. A Viewport panel must exist so that the gizmo and pick state stay valid.

### 4.2 Layout tree

```
Layout   = { version: 1, preset: PresetId, root: Node }
Node     = Split | TabGroup
Split    = { axis: "row" | "column", children: [Node], sizes: [f32] }  // sizes sum to 1.0
TabGroup = { panels: [PanelId], active: PanelId }
```

- A split has at least two children, and each `sizes` value is a fraction of the parent.
- A size never makes a panel smaller than its minimum. On a window resize, the layout reduces the largest neighbor first.
- A tab group has at least one panel. Each **visible** panel appears exactly once in the tree.
- A hidden (closed) panel is not in the tree. **View → Panels** puts it back in its preset position.
- The Viewport and the Source are always visible, so they are always in the tree.

*Correction (STACK-REVIEW-1, PR #141 review r4242710866):* the first version
said that each panel appears exactly once. A closed Scene Tree, Inspector or
Diagnostics panel is not in the tree, so the rule applies to visible panels.

#### 4.2.1 Loading a saved layout (as built in UI-DOCK-1)

*Added in STACK-REVIEW-1 (PR #141 review r4242710854).* `Layout::from_wire`
(`apps/desktop-tauri/protocol/src/layout.rs`) reads a saved layout in this
order. Each step is deterministic.

1. **Refuse the shape as written.** A wrong version, an unknown preset, an
   unknown split axis, more than 6 levels or 24 nodes, a split with fewer
   than two children, a wrong size count, a size that is not finite or is
   outside 0.02–1.0, sizes that do not sum to 1 (±0.001), an empty tab
   group, an `active` that is not in its group, or a known panel twice: the
   layout is refused. Nothing is repaired.
2. **Drop unknown panel IDs.** An unknown ID is removed from its tab group
   and named in the returned list. If the group's `active` was unknown, the
   first remaining panel of the group becomes active.
3. **Remove empty groups.** A group with no known panel left is removed,
   with its size entry.
4. **Collapse splits.** A split with no child left is removed. A split with
   one child left is replaced by that child. A child split with the same
   axis as its parent is merged into the parent (its sizes are multiplied
   by its share).
5. **Renormalize sizes.** The remaining sizes are scaled to sum to 1, each
   is raised to at least 0.03, and they are scaled to 1 again.
6. **Validate the result as a whole.** The step-1 rules run again, and the
   Viewport and the Source must both be present. An empty tree counts as a
   missing Source.

If any step refuses, the UI uses the `preset.source` layout and shows a
notice with the reason (`dock::load`). A bad layout never blocks startup.
Sizes are only proportions: minimum panel sizes are applied when the tree
is solved (`layout::solve`), not when it is loaded.

### 4.3 Persistence

- The layout is stored in the application settings (`settings.json` in the Tauri config directory) under a new key with its own schema version.
- The layout is **never** written into the VRML document, a sidecar file or a recovery record.
- An invalid or newer-version layout falls back to the preset, and a notice appears. A bad layout never blocks startup.
- Smoke runs use `--config-dir` in `/tmp`, as now.

### 4.4 Reset

- **View → Reset Layout** restores the active preset.
- Reset changes only the layout. The document, the selection, the undo history and the preview stay unchanged.

### 4.5 Presets

| preset | layout |
|---|---|
| `preset.modeling` | Scene Tree (left) · Viewport (center, largest) · Inspector over Diagnostics (right) · Source as a tab beside Diagnostics |
| `preset.source` | Scene Tree (left, narrow) · Source (center, largest) · Diagnostics (below Source) · Viewport over Inspector (right) — the current layout |

The default preset is `preset.source`, because it matches the 1.4.5 layout.

### 4.6 State that a layout change must protect

| state | rule |
|---|---|
| Source selection and caret | Keep the textarea element across moves (re-parent it, do not re-create it). Keep the selection range, the scroll position and the syntax-layer registration. |
| Document revision, dirty flag and undo history | A layout change never calls an edit, a save or a history command. |
| X_ITE provenance | A move must not reload the scene. If the canvas is re-created, the preview generation and the `wrlres` token change. The pick and gizmo bindings then retire (`pick::retire`, `gizmo::reset`) and are bound again from Rust, never guessed. |
| Active manipulation tool | A layout change during a drag cancels the drag ("layout changed") with no source edit, as Escape does now. The tool mode (Select, Move, Rotate, Scale) stays on. |
| Native viewport (hidden) | Report the new pane rectangle to Rust after each change, so that the child surface follows. |
| Inspector edit in progress | An unapplied field value is kept, or the user is warned. It is never applied silently. |

## 5. Menu architecture

This section is a contract. It is not implemented in this lane.

### 5.1 Rules

- Each menu item maps to one registry `CommandId`.
- A `missing` command is either left out or shown **disabled** with the text "(not available yet)" and a tooltip reason. It is never an active item that does nothing.
- Mall and World features stay disabled with "Not migrated to the Rust app" until their lanes ship.
- The accelerator text comes from `Shortcut::label(platform)`: Ctrl on Linux and Windows, ⌘ on macOS.
- Keyboard access: F10 or Alt focuses the menu bar on Linux and Windows. Arrow keys move, Enter runs, and Esc closes. Each top-level menu has a mnemonic (`_File`, `_Edit`, `_View`, `_Create`, `_Tools`, `_World`, `_Help`).
- On macOS, the system menu bar holds the application menu: About, Settings, Hide and Quit. In that case Help contains no About.
- The implementation (native Tauri menu or WebView menu) is a decision for the menu lane. Both must dispatch through `command::run`.

### 5.2 Menu map

| menu | item | command ID | status |
|---|---|---|---|
| File | New World | `file.newWorld` | registry |
| File | Open… | `file.open` | registry |
| File | Save | `file.save` | registry |
| File | Save As… | `file.saveAs` | registry |
| File | Reload from Disk | `file.reload` | registry |
| File | Close | `file.close` | registry |
| File | Quit | `app.quit` | direct (window close + CloseGate) — *as built in UI-MENU-1: registered, see below* |
| Edit | Undo | `edit.undo` | registry |
| Edit | Redo | `edit.redo` | registry |
| Edit | Cut / Copy / Paste / Select All | `edit.cut` … `edit.selectAll` | native → register |
| Edit | Find… / Replace… / Go to Line… | `edit.find`, `edit.replace`, `edit.goToLine` | missing (disabled) |
| Edit | Delete Node / Duplicate Node | `edit.deleteNode`, `edit.duplicateNode` | missing (disabled) |
| View | Theme ▸ (5 themes) | `view.theme` | direct → register — *as built: three commands, see below* |
| View | Live Preview | `view.livePreview` | direct → register |
| View | Update Preview | `view.updatePreview` | direct → register |
| View | Panels ▸ Scene Tree / Inspector / Diagnostics | `view.panel.*` | UI-DOCK-1 |
| View | Layout ▸ Modeling / Source / Reset | `view.preset.*`, `view.resetLayout` | UI-DOCK-1 |
| View | Zoom In / Out / Reset | `view.zoom*` | missing (disabled) |
| Create | Box / Sphere / Cylinder / Cone / Extrusion | `create.*` | direct → register |
| Tools | Select / Move / Rotate / Scale | `tools.*` | direct → register |
| Tools | Extrusion Outline Editor | `extrusion.outline` | direct → register |
| World | Mall Item ▸ Validate / Repack | `world.mall.*` | missing (disabled, "Not migrated") |
| World | World Project ▸ Open / Scan / Preview / Package | `world.project.*` | missing (disabled, "Not migrated") |
| Help | Keyboard Shortcuts | `help.shortcuts` | missing (disabled) |
| Help | Licenses | `help.licenses` | missing (disabled) |
| Help | About WRL Forge | `help.about` | registry |

*As built (STACK-REVIEW-1, PR #141 reviews r4242710880 and r4242710895).*
The two rows above were planning text. UI-MENU-1 implemented them as
follows; the registry in `apps/desktop-tauri/protocol/src/command.rs` is the
authority.

- **Quit** is the registered command `app.quit` (`CommandId::AppQuit`,
  File menu, Ctrl+Q / ⌘Q). It runs through `command::run`, which calls the
  `app_quit` backend command. The close goes through the same CloseGate as
  the title-bar button, so unsaved work is asked about and Cancel keeps the
  application. On macOS, Quit and About are in the native application menu
  (`MAC_APP_MENU`). Their `menu-command` event is validated with
  `command::from_native_menu` (protocol) and run through `command::run`. No menu item
  bypasses the registry.
- **Theme** has one command per theme, so `command::run(CommandId)` needs
  no parameter. There are three themes, not five:

  | command ID | `CommandId` | theme |
  |---|---|---|
  | `view.theme.tokyoNight` | `ViewThemeTokyoNight` | `tokyo-night` |
  | `view.theme.tokyoNightStorm` | `ViewThemeTokyoNightStorm` | `tokyo-night-storm` |
  | `view.theme.tokyoNightLight` | `ViewThemeTokyoNightLight` | `tokyo-night-light` |

  The checked item comes from `Check::Theme`.

"direct → register" means that the item exists, but it must move into the registry before it gets a menu item.

## 6. Tests and limits

All tests ran on Linux x64 (Xvfb software rendering) against code commit
`931070a`. The final commit adds only this document.

### 6.1 Results

| check | result | base `c23646b` |
|---|---|---|
| `cargo test --workspace` (`apps/desktop-tauri`) | 166 pass, 0 fail (includes 7 registry and 5 version-policy tests) | — |
| `cargo test --workspace` (`crates/`) | 193 pass, 0 fail | code unchanged |
| release Wasm UI build (`./build-ui.sh`) | OK; X_ITE 16.5.0 verified against `xite/PIN` | same |
| `wrl-forge --version` | `WRL Forge 1.4.5 (Beta)` | — |
| window title (xdotool, Xvfb) | `WRL Forge 1.4.5 (Beta)`; `title.wrl — WRL Forge 1.4.5 (Beta)` | `… — WRL Forge` |
| file smoke, 58 fixtures | 47 pass, the 11 known invalid inputs fail; **each per-file verdict is identical to the base** | 47 |
| Inspector smoke, 58 fixtures | 18 pass; **each per-file verdict is identical to the base** | 18 |
| APP-SHELL-0 smoke steps (inside the file and Inspector runs) | 672 run, 672 pass, 0 fail | — |
| create | 51/51 | 51/51 |
| extrusion | 58/58 | 58/58 |
| move | 156/156 | 156/156 |
| rotate / scale | 71/71 | 71/71 |
| pick | pass; 37 fixtures, 152 clicks, `WRONG_SOURCE_SELECTIONS = 0` | same |
| texture and resources | 46/46 | 46/46 |
| recovery (SIGKILL, Restore, Later) | crash phase OK (4 records); restore 34/34; later 15/15 | same |

The new smoke steps in each file run check six things:

- Ctrl+Z then Ctrl+Y restores the edit through the registry.
- The toolbar shows the identity.
- Each File, Edit and About button has the correct `data-command`.
- `app_info` reports 1.4.5, Beta, package 1.4.5, X_ITE 16.5.0 and the unchanged identifier.
- About shows the identity and X_ITE 16.5.0, with no version mismatch.
- About closes.

The existing steps continue to prove the File and Edit document actions through the new path:

- The Save button writes through Rust.
- Ctrl+Z and Ctrl+Shift+Z work.
- The create, move, extrusion and rotate/scale suites click New, Open, Save, Save As, Close, Undo and Redo.

### 6.2 Defect found and fixed during the lane

The first smoke run failed at the About step in all 58 files. The About state signal was created lazily inside the dialog's own render closure. Leptos disposed that signal when the closure ran again, which is exactly when About opens. Now the signal is created in `start()` before the UI mounts, as `recovery::init()` does. After this fix, all 672 APP-SHELL-0 steps pass.

### 6.3 Limits

- **Windows x64: not tested.** No native Windows system was used.
- **macOS 15+ Apple Silicon: not tested.** No macOS system was available. The `⌘` labels are covered by unit tests only.
- No Linux package was built in this lane. The package name comes from `tauri.conf.json` and will be `1.4.5`.
- `cargo fmt --check` already fails on the base (260 hunks in files this lane did not change). The new files are formatted.
- The shortcuts stay focus-scoped to Source (§2.4).
- The X_ITE renderer, the CSP, the capabilities and `resources.rs` are unchanged. X_ITE 16.5.0 stays the default, and the native viewport stays a hidden test feature.

## 7. Next lane: UI-DOCK-1 (exact scope)

**In scope:**

1. Build the layout model of §4.2 as pure Rust in `protocol`, with unit tests: validation, minimum sizes, unknown IDs, and fallback.
2. Build the dock host in the Leptos UI: splits with drag handles, and tab groups. Use the five panel IDs and the two presets of §4.5.
3. Persist the layout in `settings.json` (§4.3). Add **Reset Layout** and the **Panels** and **Layout** commands to the registry, with UI buttons in place of a menu.
4. Meet every rule in §4.6. Add smoke steps that move each panel during these states: a Source selection, a live X_ITE scene with a pick binding, and an active Move drag. The steps prove no reload, no source edit and a cancelled drag.
5. Fix the panel-header overflow (§3.6).
6. Test on Linux with the full smoke matrix. Record Windows and macOS as untested if no native system is available.

**Out of scope:** the menu bar (a later MENU-1 lane), floating or undocked windows, multiple windows, new commands, Mall or World features, and any renderer change. X_ITE 16.5.0 stays the default.
