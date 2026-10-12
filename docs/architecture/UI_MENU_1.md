# UI-MENU-1 — Desktop menus and command integration

Lane: UI-MENU-1. Target: the Rust/Tauri 2 application with the Leptos/Wasm UI
(`apps/desktop-tauri`), WRL Forge 1.4.5 (Beta).

Base: `c2fd566263b5f4d4cc9465d45db2778f4a2872b4` (UI-DOCK-1, Draft PR #142).
This lane implements the menu contract of `APP_SHELL_0.md` §5.

X_ITE 16.5.0 stays the default renderer. The native (wgpu) viewport stays a
hidden test feature. No Electron file changed.

## 1. QA preflight: the UI-DOCK-1 test totals

The UI-DOCK-1 coding agent reported File 47/58 and Inspector 18/58. The
independent QA agent reported File 50/58 and Inspector 11/58.

**Cause:** the two agents used different fixture sets. Each set has 58 files.
The difference is fixture selection. It is not a regression, and it is not a
timing or classification difference.

| set | source | files |
|---|---|---|
| `fx58` | the coding agent's flat folder (`/tmp/xite2/fx58`, outside the repository; includes 16 Flux Studio files) | 58 |
| `fxqa` | the QA agent's set: repository `test/fixtures/**/*.wrl|*.wrz` minus the 4 `parts`/`sub` include files | 58 |

Method: two new clean worktrees at APP-SHELL-0 `86bcba9` and UI-DOCK-1
`c2fd566`, both built from source (release Wasm, debug desktop). Each build
ran both sets through `smoke.sh --headless` and `--inspector` (Files 8
parallel runs, Inspector 6, 180 s per file). A run is **PASS** when it exits 0
with `"pass": true`. It is **TIMEOUT** when it exits 124 or writes no report.
All other runs are **FAIL**.

| batch | APP-SHELL-0 pass / fail / timeout | UI-DOCK-1 pass / fail / timeout | per-file diffs | failing-step diffs |
|---|---|---|---|---|
| `fx58` Files | 47 / 11 / 0 | 47 / 11 / 0 | 0 | 0 |
| `fx58` Inspector | 18 / 38 / 2 | 18 / 38 / 2 | 0 | 0 |
| `fxqa` Files | 50 / 8 / 0 | 50 / 8 / 0 | 0 | 0 |
| `fxqa` Inspector | 11 / 45 / 2 | 11 / 45 / 2 | 0 | 0 |

- Both reported totals reproduce exactly: `fx58` gives 47 and 18, `fxqa` gives
  50 and 11.
- On the files both sets share, every verdict is the same.
- The timeouts are the large gzip-twin files. They time out on both builds:
  `gz__twin-large`, `gz__twin-small`, `gzip-encodings__limit-twin.plain`,
  `gzip-encodings__twin.plain`.
- The `fx58` set is not in the repository, so a reviewer cannot rebuild it from
  a checkout. Later gates run both sets.

## 2. Menu architecture

**Decision:** draw the menus in the window, from the registry, on every
platform. On macOS, add the standard application menu too.

- Enabled and checked states live in the Wasm UI (`command::context()`). A
  native menu needs a second copy of that state in the backend, and the menu
  and toolbar could then disagree.
- On Linux the native test viewport uses GTK split panes. A native GTK menu
  bar changes the window geometry under them.
- The smoke harness drives the window with real X input. An in-window menu is
  testable with that harness; a native GTK menu is not.

Parts:

| part | file |
|---|---|
| Menu table (order, groups, mnemonics) | `protocol/src/command.rs` `MENUS`, `MenuEntry`, `MenuDef` |
| Menu bar component and window listeners | `ui/src/menu.rs` |
| Dispatch, context, checked state, shortcut handler | `ui/src/command.rs` |
| Edit → Cut / Copy / Paste / Select All, focus record | `ui/src/clipboard.rs` |
| macOS application menu | `src-tauri/src/app_menu.rs` |
| Quit | `src-tauri/src/commands.rs` `app_quit` |

The menu module defines no labels, handlers or state. Every item reads the
command's spec. Every activation calls `command::run`, which the toolbar
also calls.

## 3. Command IDs

The registry now holds 56 commands (APP-SHELL-0 had 9 and UI-DOCK-1 added 11,
for 20 before this lane; this lane adds 36). The table lists the new ones.
Keys are stable.

| menu | key | label | requires | action |
|---|---|---|---|---|
| File | `app.quit` | Quit (Ctrl+Q / ⌘Q) | — | Backend `app_quit` |
| Edit | `edit.cut` | Cut | Source focus + selection | `SourceEdit` |
| Edit | `edit.copy` | Copy | text focus + selection | UiOnly |
| Edit | `edit.paste` | Paste | Source focus | `SourceEdit` |
| Edit | `edit.selectAll` | Select All | text focus | UiOnly |
| Edit | `edit.find`, `edit.replace`, `edit.goToLine`, `edit.deleteNode`, `edit.duplicateNode` | reserved | — | `Planned` |
| View | `view.theme.tokyoNight`, `view.theme.tokyoNightStorm`, `view.theme.tokyoNightLight` | the 3 registered themes | — | Backend `theme_set` |
| View | `view.livePreview` | Live Preview (checked) | — | UiOnly |
| View | `view.updatePreview` | Update Preview | document | Backend `doc_preview_source` |
| View | `view.zoomIn`, `view.zoomOut`, `view.zoomReset` | reserved | — | `Planned` |
| Create | `create.box`, `create.sphere`, `create.cylinder`, `create.cone`, `create.extrusion` | primitives | document | Backend `doc_create` |
| Tools | `tool.select`, `tool.move`, `tool.rotate`, `tool.scale` | viewport tools (checked) | document + X_ITE viewport | UiOnly |
| Tools | `tool.extrusionOutline` | Extrusion Outline Editor… | an editable Extrusion `crossSection` in the Inspector | UiOnly |
| World | `world.mall.validate`, `world.mall.repack`, `world.project.open`, `world.project.scan`, `world.project.preview`, `world.project.package` | Mall Item / World Project | — | `Planned("Not migrated to the Rust application.")` |
| Help | `help.shortcuts`, `help.licenses` | reserved | — | `Planned` |

The theme commands match `protocol::theme::THEMES` (3 themes, not the 5 in the
old plan). A test fails if a theme has no command, or a command has no theme.

### New action types

- `Action::SourceEdit` is for menu Cut and Paste. The change is made in the
  Source textarea widget. Then `editor::on_input` runs, which is the function a
  typed key reaches. It sends one `doc_edit` with a revision check and a view
  hash check, and it resyncs on any refusal. A test requires
  `mutates_document = true` and a Source requirement for every `SourceEdit`
  command.
- `Action::Planned(reason)` is shown, but never runs.
- The APP-SHELL-0 test `UiOnly ⇒ !mutates_document` is unchanged.

`document.execCommand` is not used, because its `input` event is not
guaranteed. The `setRangeText` + `on_input` path does not depend on any event.
The smoke run proves it: Paste and Cut change the revision, and the Rust text
equals the editor text.

## 4. Command-to-handler map

Every arm calls a function that existed before this lane, or one moved
unchanged out of a toolbar closure.

| command | handler |
|---|---|
| File (New, Open, Save, Save As, Reload, Close) | `ui::new_world`, `ui::open`, `ui::save`, `ui::reload`, `ui::close` (unchanged) |
| Quit | `app_quit` → `WebviewWindow::close` → `CloseRequested` → `CloseGate` → `confirm_close` |
| Undo / Redo | `editor::history` (unchanged) |
| Cut / Copy / Paste / Select All | `clipboard::cut`, `copy`, `paste`, `select_all` |
| Panels, focus, presets, reset | `dock::show`, `hide`, `focus`, `set_preset`, `reset` (unchanged) |
| Theme | `theme::select` (unchanged) |
| Live Preview | toggles `ui().preview_enabled` (the checkbox's signal) |
| Update Preview | `ui::preview(true)` (unchanged) |
| Create | `ui::create(prim)` (unchanged) |
| Select / Move / Rotate / Scale | `panels::toggle_select`, `panels::toggle_gizmo` (moved from the button closures) |
| Extrusion Outline Editor | clicks the Inspector's own `#btn-outline` (the one place that holds the outline data) |
| About | `about::show` (unchanged) |

### Toolbar cross-reference

| toolbar control | command |
|---|---|
| New, Open, Save, Save As, Close, Undo, Redo, About | `file.*`, `edit.undo`, `edit.redo`, `help.about` (unchanged) |
| Create ▾ items | `create.*` (previously a direct `ui::create` call) |
| Layout ▾ items | `view.*` (unchanged) |
| Theme picker | `view.theme.*` (previously a direct `theme::select` call) |
| Viewport Live checkbox, Update | `view.livePreview`, `view.updatePreview` (previously direct) |
| Viewport Select, Move, Rotate, Scale | `tool.*` (previously closures; now `data-command` + registry title and enabled state) |

The toolbar layout is unchanged. Every toolbar control and menu item reads the
same enabled and checked state, so the two cannot disagree.

## 5. Enabled, disabled and checked rules

`CommandId::availability(ctx)` gives the menu state. `CommandId::refusal(ctx)`
is the run-time check. A refusal is always also shown as disabled (a test
checks every state combination). Every disabled state has a reason, and the
item's `title` shows it.

| requirement | disabled reason |
|---|---|
| Document | No document is open |
| UndoHistory / RedoHistory | Nothing to undo / Nothing to redo (display only; Rust decides at run time) |
| PanelShown | The panel is not shown |
| SourceFocus | Works in the Source editor (in other text fields use the keyboard shortcut) |
| SourceSelection | Select text in the Source editor first |
| TextSelection | Select text first |
| TextFocus | Works in a text field or the Source editor |
| ViewportTool | The viewport tools need the X_ITE viewport |
| ExtrusionOutline | Select an Extrusion with an editable crossSection first |
| Planned (World) | Not migrated to the Rust application. |
| Planned (other) | Planned for a future feature lane. |

The checked state comes from the existing signals only:

- Tools: `pick_mode`, `move_mode`, `rotate_mode`, `scale_mode`.
- Theme: `theme().id`.
- Live Preview: `preview_enabled`.
- Panels: `dock::shown()`.
- Preset: the layout's preset.

No menu holds its own copy of any of these.

**Focus record:** a menu takes the keyboard focus while it is open. The Edit
commands therefore act on the control that had the focus before
(`clipboard::LAST`).

- A pointer press on the bar does not move the focus (`mousedown`
  `preventDefault`).
- Focus that moves to nothing clears the record. So a menu never edits a
  control the user left.
- Escape, or the activation of an item, gives the focus back.

## 6. Keyboard behavior

**One shortcut handler.** `command::on_window_keydown` runs on the window, in
the bubble phase. It replaces the Source-only `on:keydown` (APP-SHELL-0 §2.4
defect). It resolves keys only through `protocol::command::from_shortcut`, so
there is no second shortcut table.

| situation | behavior |
|---|---|
| Source focused | All registry shortcuts run. Ctrl+X / C / V / A stay native (input → `doc_edit`). |
| Another text field (Inspector, extrusion table) | Only New, Open, Save, Save As and Quit run. Ctrl+Z / Y / A / X / C / V stay the field's own. |
| Scene Tree, viewport, dock tabs, other controls | All registry shortcuts run. |
| IME composition (`isComposing`, keyCode 229, `Process`) | Nothing runs. |
| Ctrl+Alt or AltGraph (AltGr) | Nothing runs; the key types text. |
| Focus inside a dialog (About, recovery choice, outline editor) | Nothing runs; no menu access. |
| Gizmo, dock or outline drag | Nothing runs; no menu access. The drags keep Escape. |
| A menu bar button has the focus, or a menu is open | The menu handles only its own navigation keys. A registry shortcut closes the menu, gives the focus back, then runs once. The text-field rule uses the control the menu took the focus from. |

Each key press runs at most one command. The smoke run counts `command::run`
calls.

Menu bar keys (Linux and Windows):

| key | effect |
|---|---|
| F10, or Alt pressed and released alone | Focus the bar (File). Again: close and restore the focus. |
| Alt+F / E / V / C / T / W / H | Open that menu. |
| Left / Right, Home / End (on the bar) | Move between menus (wrapping). |
| Down / Enter / Space (on the bar) | Open the menu on its first item; Up opens it on its last item. |
| Down / Up, Home / End (in a menu) | Move between items (wrapping). |
| Left / Right (in a menu) | Open the previous or next menu. |
| Enter / Space | Activate the item. A disabled item stays open and shows its reason. |
| Escape | Close the menu to its bar button; Escape again restores the earlier focus. |
| Tab | Close the menu and restore the focus. |

Shortcut text comes from `Shortcut::label(platform)`: `Ctrl+…` on Linux and
Windows, `⌘…` on macOS. The clipboard items show their native keys
(`native_keys`, label only; the window handler never dispatches them).

## 7. Platform differences

| platform | menus | status |
|---|---|---|
| Linux x64 | In-window menu bar. F10 / Alt / Alt+letter. | Tested (this lane's gates, Xvfb, real X input). |
| Windows x64 | In-window menu bar. F10 / Alt / Alt+letter. AltGr arrives as Ctrl+Alt and is ignored. | **Not tested.** No Windows system was used. |
| macOS 15+ (Apple Silicon) | In-window menu bar without About and Quit (`MAC_APP_MENU`), plus the native application menu: WRL Forge → About, Hide, Hide Others, Show All, Quit, and Edit → Cut / Copy / Paste / Select All (platform items). F10 and the Alt keys are not used. | **Untested and not compile-verified.** No macOS Rust target is installed. The menu builder compiles on Linux, because it has no `cfg`. The 3 installing lines are `cfg(target_os = "macos")` and were not compiled. |

On macOS, About and Quit in the application menu send the command key to the
UI (`menu-command` event). The UI runs it through `command::run`, so macOS
Quit also ends in `app_quit` → CloseGate. The UI listener was missing in the
first commit of this lane; §13 records the fix.

## 8. Accessibility

- `role=menubar` with 7 `menuitem` buttons (`aria-haspopup=menu`,
  `aria-expanded`, `aria-controls`, `aria-keyshortcuts=Alt+X`).
- Underlined mnemonic letter.
- A roving tab stop: only File is in the Tab order.
- Popups are `role=menu`. Items are `menuitem`, `menuitemcheckbox` (tools,
  panels, Live Preview) or `menuitemradio` (theme, preset), with
  `aria-checked`.
- Disabled items use `aria-disabled=true`. They stay focusable, and their title
  gives the reason.
- Focus is always visible (`:focus-visible` ring). Colors use only `--wf-*`
  tokens; disabled text uses `--wf-text-disabled`.

## 9. Source safety

- Menu-only actions (open, close, navigation, layout, theme, tools, Live
  Preview, disabled items) change no revision, text, history or selection. The
  smoke run checks every one against the `DocState` snapshot (revision, dirty
  flag, view hash, textarea hash, Source selection, Scene Tree selection,
  Inspector, preview loads). It also checks that no panel or canvas object is
  recreated (object probes).
- Cut and Paste reach Rust only through `doc_edit`. A text field other than
  Source is never edited from the menu.
- A Cut or Paste edits only if the Source state that it captured before the
  clipboard call is unchanged after the call (§13).
- Quit uses the existing CloseGate only. Cancel keeps the application, the
  document and the recovery records.
- `WRONG_SOURCE_SELECTIONS = 0`: the menu run checks real viewport picks
  before and after the menu work. The pick and dock suites also run.

## 10. Dock focus fixes (PR #142 review)

The owner folded two UI-DOCK-1 review findings (Copilot, `dock.rs`) into this
lane, because they are on the focus path that the menus use.

| finding | fix | proof |
|---|---|---|
| A successful drop re-keys the tab strips and destroys the focused tab, so the focus falls to the body (line 582). | After a drop, the moved tab takes the focus. | Dock main: "after a drop the moved tab (Inspector) has the keyboard focus". |
| Hiding a panel from its focused tab removes the strip, so the focus falls to the body (line 629). | If the hidden panel's group held the focused tab, the focus goes to the tab that is now active in that group. If the group is gone, it goes to the Source tab. View-menu hides are covered too: the menu gives the focus back to the tab before the command runs. | Dock main: the Source fallback (Delete on a single-tab group). Menu run: the same-group hand-off by Delete and by View → Diagnostics (Modeling preset). |

The third finding (the edge-drop guide does not match the final rectangle,
line 492) is not in this lane. It stays with PR #142.

## 11. Test results

Gate on `c488c80` (the menu code, before the dock focus fixes): a new clean
worktree, the script `/tmp/uimenu1/verify.sh`, Xvfb per GUI run, 8 parallel
runs for Files and 6 for Inspector.

| check | result |
|---|---|
| Release Wasm build (`build-ui.sh`) | pass (X_ITE 16.5.0 verified) |
| Desktop backend build | pass; `--version` = `WRL Forge 1.4.5 (Beta)` |
| `cargo test --workspace` (apps) | 202 pass, 0 fail (UI-DOCK-1: 193; +9 registry tests) |
| `cargo test --workspace` (crates) | 193 pass, 0 fail |
| Menu (`--menu`) | pass (48 steps at `c488c80`; 50 after the focus fixes) |
| Dock (main, restore, damaged) | pass |
| Pick (37 fixtures, 152 real clicks) | pass |
| Move, Rotate/Scale, Create, Extrusion, Texture, Recovery | pass |
| Settings / theme (`--theme tokyo-night-light`) | pass |
| Native self-test | pass (46 checks) |
| Files `fx58` | 47 / 11 / 0 (pass / fail / timeout); 0 per-file and 0 failing-step differences from UI-DOCK-1 |
| Inspector `fx58` | 18 / 38 / 2; 0 differences |
| Files `fxqa` | 50 / 8 / 0; 0 differences |
| Inspector `fxqa` | 11 / 45 / 2; 0 differences |

The gate on the final commit is reported in the Draft PR and the lane report.

`WRONG_SOURCE_SELECTIONS = 0`: the menu run (real picks before and after the
menu work), the dock run and the pick oracle all report 0.

The menu run passed 3 of 3 times while it ran beside two other GUI runs.

## 12. Known limits

- Windows and macOS runtime checks are pending. The macOS application menu
  is not compile-verified (see §7).
- WebKitGTK refuses `navigator.clipboard.readText()` outside a user gesture.
  The menu commands run inside the click or key gesture and work. A test
  script cannot read the clipboard directly, so the smoke run proves Copy and
  Cut by pasting the text back from the menu.
- A real IME is not available under Xvfb. The IME rule is proven with
  synthetic `keydown` events (`isComposing`, `Process`). AltGr is proven with
  real `ctrl+alt` keys; the Xvfb keymap has no AltGr key.
- Find, Replace, Go to Line, Delete Node, Duplicate Node, Zoom, Keyboard
  Shortcuts, Licenses and every World item stay disabled.
- The toolbar arrangement is unchanged. The menu bar adds one row above it.
- `cargo fmt --check` already fails on the base. This lane formats its own
  files and hunks only.

## 13. Follow-up: clipboard safety and native menu events

### Clipboard safety

Before, Cut and Paste waited for the clipboard and then edited the Source
selection that was current at that time. If the user moved the selection,
typed, or opened another document during the wait, the late answer edited the
wrong place.

Now `clipboard.rs` takes a `Ticket` before the clipboard call:

- the Source textarea element (the focus record must still be Source);
- the document session, revision and editor generation (the generation
  changes on every acknowledged edit, undo, resync and document replacement);
- the selection (UTF-16 offsets) and the full widget text.

A ticket is taken only when no source edit is in flight and the widget text
equals the text Rust acknowledged. A pending edit is waited for first
(`editor::idle`, bounded). After the clipboard answers, the ticket is taken
again and compared field by field. If any field changed, the operation is
refused: the widget does not change, no `doc_edit` is sent, and no undo entry
is made. If the ticket holds, the edit goes through `editor::on_input`, the
existing revision-checked `doc_edit` path, as before. Rust stays the only
document authority. There is no second history or save path.

A stale Cut has already written its text to the clipboard. It does not
remove the text from Source.

### Test-only clipboard delay

`clipboard::set_test_hold` delays every clipboard answer by a fixed time, or
fails it as a canceled request. Only the menu smoke run sets it. Production
never sets it, and the clipboard API and its permissions are unchanged.

### Native menu events

Before, the backend emitted `menu-command` for the macOS About and Quit items,
but no UI code listened to it. The items did nothing.

Now:

- `protocol::command::from_native_menu` accepts a key only if it names a
  command in `MAC_APP_MENU`. Both sides use it: `app_menu::forward` in the
  backend, and `menu::listen_native` in the UI.
- `menu::listen_native` subscribes once for the page life (a flag prevents a
  second registration; layout and menu changes do not call it). It uses the
  same `window.__TAURI__.event.listen` API as the native viewport events. The
  capability stays `core:default`.
- A valid key runs through `command::run`. There is no second dispatcher.
  Quit therefore ends in `app_quit` → CloseGate.
- An invalid key runs nothing and is counted as refused.

The smoke command `smoke_menu_native` (menu runs only) drives
`app_menu::forward`, or emits an unchecked key to prove the UI check.

### New menu smoke steps

| step | proves |
|---|---|
| Delayed Cut, selection moved | no text, revision or undo change; counted stale |
| Delayed Paste, selection moved | the same |
| Delayed Paste, user typed | only the typed edit stays |
| Delayed Cut and Paste, document replaced | the new document stays clean, with no undo entry |
| Canceled clipboard (Cut and Paste) | nothing changes; the message says why |
| Rapid Cut, Cut, Paste | the first Cut is one edit (one Undo restores it); the other two are stale |
| Paste during a pending source edit | it waits, then pastes once; two undo entries |
| After the races | Rust text == editor text == saved document |
| Native About event | runs About once through `command::run` |
| Native About after a layout change | still runs once (one listener) |
| Native events outside the app menu | refused by the backend and by the UI; nothing runs |
| Native Quit event | asks through the CloseGate; Cancel keeps document and recovery records |

### Platform status for this follow-up

- Linux x64: tested (see the gate below).
- Windows x64: not tested. The clipboard code has no platform branch.
- macOS 15+ (Apple Silicon): not tested, not compile-verified. No macOS Rust
  target is installed. The event path (`forward` → event → UI listener →
  `command::run`) is tested on Linux. The NSMenu installation and its click
  events need a check on a Mac.


### Follow-up gate

Gate on `d19744f` (the follow-up code): a new clean worktree, the script
`/tmp/uimenu1/verify.sh`, Xvfb per GUI run. Total time: 791 s.

| check | result |
|---|---|
| Release Wasm build (`build-ui.sh`) | pass |
| Desktop backend build | pass; `--version` = `WRL Forge 1.4.5 (Beta)` |
| `cargo test --workspace` (apps) | 203 pass, 0 fail (+1 registry test) |
| `cargo test --workspace` (crates) | 193 pass, 0 fail |
| Menu (`--menu`) | pass, 62 of 62 steps (14 new); `WRONG_SOURCE_SELECTIONS = 0` |
| Dock (main, restore, damaged) | pass |
| Pick (37 fixtures, 152 real clicks) | pass |
| Move, Rotate/Scale, Create, Extrusion, Texture, Recovery | pass |
| Settings / theme | pass |
| Native self-test | pass (46 checks) |
| Files `fx58` / `fxqa` | 47/11/0 and 50/8/0; 0 differences from UI-DOCK-1 |
| Inspector `fx58` / `fxqa` | 18/38/2 and 11/45/2; 0 differences from UI-DOCK-1 |

The Files and Inspector failures and timeouts are the known baseline. They are
the same files and the same failing steps as on UI-DOCK-1.

## 14. Copilot review #5484767419 (PR #143)

The review has three comments. Comments A and B were fixed in §13. Comment C
was still active.

### Comment C: menu keys stopped every key

**Finding.** `on_bar_button_key()` called `stop_propagation()` for every key.
`on_list_key()` did the same for unknown keys. Keys that the menu did not use
never reached the window listener.

**Effect, reproduced on `a59f50c`** (new smoke steps, real X keys under Xvfb):

| case | result on `a59f50c` |
|---|---|
| F10, then F10 again | focus stays on `menu-file`; the toggle fails |
| Alt, then Alt again | focus stays on `menu-file`; the toggle fails |
| F10 or Alt alone in an open menu | the menu stays open |
| Alt+V on a focused bar button | View does not open |
| Ctrl+S with the File button focused | 0 command runs; nothing is saved |
| Ctrl+S in an open File menu | 0 command runs; the menu stays open |
| Keys from the bar that reach the window | none |

**Root cause.** The handlers had no handled or unhandled result. They stopped
the event at the end of every call. In addition, `shortcut_blocked()` refused
every shortcut while a menu was open.

**Correction.**

- `menu.rs`: `bar_button_key()` and `list_key()` return `true` only when the
  menu uses the key. `consume()` then calls `prevent_default()` and
  `stop_propagation()`. Other keys bubble to the one window listener.
- The menu handlers do not use a key with Ctrl, Alt or Meta, or a key during
  IME composition (`foreign_key()`). Shift+Tab stays a menu key.
- `close(true)` with no recorded control removes the focus from the bar. A
  second F10 or Alt therefore always leaves the bar.
- `command.rs`: `shortcut_blocked()` no longer refuses shortcuts because a
  menu is open. With the focus in the bar, `on_window_keydown()` applies the
  text-field rule to the recorded control. It then closes the menu with focus
  restoration and runs the command once.

There is no second shortcut listener and no second command table. The
AltGr, IME, dialog and drag rules are unchanged.

### Expected shortcut behavior

| state | registry shortcut | F10 / Alt alone | Alt+letter |
|---|---|---|---|
| Menu closed | runs once (the §6 rules) | focuses the bar | opens the menu |
| A bar button has the focus | focus returns to the earlier control; runs once | leaves the bar; focus returns | opens the menu |
| A menu item has the focus | the menu closes; focus returns; runs once | the menu closes; focus returns | opens that menu |

If the earlier control is an Inspector or other text field, only New, Open,
Save, Save As and Quit run. Undo and Redo do nothing and the menu stays as it
is. Closing a menu changes no document, no source selection and no selected
node. Ctrl+Q always goes through `app_quit` and the CloseGate.

### New menu smoke steps (18)

| step | proves |
|---|---|
| F10, F10 | bar, then Source with the same selection |
| Alt, Alt | the same |
| F10 or Alt in an open menu | the menu closes; Source and its selection return |
| F10, Alt+V, Escape, Escape | View opens from a bar button; no document, node or selection change |
| Window reach probe | F10, Alt and Alt+V reach the window listener; Down and Escape do not |
| The five steps above after View → Modeling | the same results in another dock layout |
| Ctrl+S, File button focused | 1 run; the file holds the document; focus and selection return |
| Ctrl+S, File menu open | the menu closes; 1 run; focus and selection return |
| Ctrl+Z from a bar button, Ctrl+Shift+Z from the Edit menu | 1 undo and 1 redo; Rust text == editor text |
| Shortcut count | each shortcut reaches the window once and runs once; Save leaves the selected node unchanged |
| Ctrl+Z / Ctrl+Shift+Z from an Inspector field through the menu | 0 runs; no revision change; the field keeps its text and the focus |
| AltGr and IME keys in the menu | 0 runs; no change (IME keys are synthetic) |
| Ctrl+S in an open menu after View → Modeling | the menu closes; 1 run; focus and selection return |
| Ctrl+Q from a bar button and from the File menu | 2 runs; the CloseGate asks twice; Cancel keeps the unsaved document, selection and recovery records |

The Move drag step now also presses F10: the bar does not take the focus.

The window reach probe is a test-only capture and bubble listener pair. It
records keys and handles none.

Results with the correction: 80 of 80 steps pass (was 62), 3 of 3 times in
3 parallel runs. On `a59f50c` the same suite fails 7 of the new steps.

### Comments A and B: still fixed

- **A (Cut / Paste race, `clipboard.rs`).** The `Ticket` check (§13) is in
  place. A stale or failed answer changes no widget text and sends no
  `doc_edit`, so there is no undo entry. All clipboard race steps pass.
- **B (native `menu-command`, `app_menu.rs`).** `app_menu::forward` and
  `menu::listen_native` both use `from_native_menu`. The listener registers
  once (`NATIVE_LISTENING`), and `lib.rs` is its only caller. The 4 native
  event steps pass, including the step after a layout change and the Quit
  step through the CloseGate.

### Limits

- Windows and macOS are not tested. The macOS native menu runtime check
  stays pending (§13).
- Xvfb has no IME and no AltGr key. IME keys are synthetic `keydown` events;
  AltGr is the real Ctrl+Alt chord.
- WebKitGTK under Xvfb does not undo typed text in an Inspector `<input>`
  with Ctrl+Z, also without a menu. The step records this. It proves that no
  document Undo runs and that the field keeps its text.
- The `fx58` Files and Inspector fixture set was in `/tmp` and is gone. The
  gate for this change compares `fxqa` only (see the PR).
