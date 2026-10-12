# UI-DOCK-1 — Magnetic docking and resizable workspace

Lane: UI-DOCK-1. Target: the Rust/Tauri 2 application with the Leptos/Wasm UI
(`apps/desktop-tauri`), WRL Forge 1.4.5 (Beta).

Base: `86bcba93e653295529b0e59bb6e50b02500576b6` (APP-SHELL-0, Draft PR #141).
This lane implements the docking contract of `APP_SHELL_0.md` §4.

X_ITE 16.5.0 stays the default renderer. The native (wgpu) viewport stays a
hidden test feature. No Electron file changed.

## 1. Layout model

`protocol/src/layout.rs` holds the model. It is pure Rust with no UI and no
I/O, and both the backend and the Wasm UI compile it.

```
Layout   = { preset: PresetId, root: Node }
Node     = Split { axis: Row | Column, children: [Node], sizes: [f64] }
         | Tabs  { panels: [PanelId], active: PanelId }
```

| rule | value |
|---|---|
| Panel IDs | `panel.sceneTree`, `panel.viewport`, `panel.source`, `panel.inspector`, `panel.diagnostics` |
| Required panels | Viewport and Source. They cannot close. |
| Each panel | appears at most once. A shown panel appears exactly once. |
| Split | at least 2 children; `sizes` has one ratio per child |
| Ratio | finite, `0.02 ≤ r ≤ 1`, and the sum is `1 ± 0.001` |
| Tab group | at least 1 panel; `active` is one of its panels |
| Depth | at most 6 levels (`MAX_DEPTH`) |
| Nodes | at most 24 splits and tab groups (`MAX_NODES`) |

Validation runs on the whole tree. `Layout::validate` checks a built layout.
`Layout::from_wire` checks untrusted data (a saved file or IPC):

- It checks the tree's shape as written, with no repair.
- It counts depth and nodes on the written tree.
- It drops unknown panel IDs (APP-SHELL-0 §4.1) and reports them.
- It then validates the result as a whole.

Every operation returns a new validated layout or a refusal. An operation
never changes the current layout half-way.

| operation | refusal examples |
|---|---|
| `dock(panel, target)` | panel or anchor not shown; the panel onto itself; a drop that changes nothing |
| `move_tab(panel, index)` | no change |
| `activate(panel)` | panel not shown |
| `hide(panel)` | Viewport or Source; panel not shown |
| `show(panel)` | — (a shown panel is activated) |
| `resize(geometry, path, index, delta)` | unknown divider; non-finite delta |

`approx_eq` compares two layouts with a ratio tolerance. A restore can add
float noise of about 1e-16 to a ratio; that is not a different layout.

## 2. Magnetic docking

A user drags a panel by its tab. The tab strip is the panel header.

1. A press on a tab captures the pointer. A press on a button in the tab
   (the close `×`) never starts a drag.
2. After 6 px of travel (`DRAG_THRESHOLD_PX`) the press becomes a docking
   drag. An active gizmo drag is canceled first.
3. While the pointer moves, the overlay shows:
   - a dashed outline on every tab group's content (the valid drop areas);
   - a five-tile compass on the group under the pointer (left, right, top,
     bottom, center); a tile for an invalid zone is dimmed and dashed;
   - the snap guide: the exact area that the panel will take. An invalid
     target shows the guide in the error style;
   - a label that follows the pointer, and a polite live-region sentence.
4. Release docks the panel only when `Layout::dock` accepts the target.
   The layout does not change before the release.
5. Escape, `pointercancel`, `lostpointercapture` and window blur cancel the
   drag. The layout stays unchanged. The later release does nothing.

Docking runs only on tab strips. The viewport canvas never receives the
docking pointer, so a docking drag cannot start a 3D manipulation. Docking
stays inside the main window. Floating windows are out of scope.

### 2.1 Drop-target geometry

| pointer position | target |
|---|---|
| tab strip of a group | Center, inserted between tabs at the nearest tab center (`insertion_index`) |
| outer 28 % band of a group's content (`EDGE_BAND`) | the nearest edge: Left, Right, Top or Bottom |
| inside that band | Center (add to the tab group) |
| outside every group | no target (release does nothing) |

| zone | result | snap guide |
|---|---|---|
| Left / Right | a Row split around the target group | the left / right half of the group |
| Top / Bottom | a Column split around the target group | the top / bottom half |
| Center | the panel joins the group as the active tab | the group, inset 4 px |
| Center in the panel's own strip | reorder | a 4 px insertion bar |

A target is named by an anchor panel of the target group, because each panel
appears once. A new split of the same axis as its parent merges into the
parent. An empty group is removed, and a split with one child collapses.

## 3. Resizing

Dividers are 6 px wide (`DIVIDER_PX`). `role="separator"`,
`aria-orientation`, and `aria-valuenow` (the left/top share in percent) are
set.

- **Geometry.** `solve(layout, width, height)` computes every rectangle.
  Split sizes are ratios. Minimum sizes are applied when the rectangles are
  computed, so a stored ratio does not change when the window shrinks.
- **Minimums.** The APP-SHELL-0 minimums apply to panel content. A tab group
  needs the largest minimum of its panels plus the 30 px tab strip.

| panel | min width | min height |
|---|---:|---:|
| Scene Tree | 200 | 160 |
| 3D Viewport | 320 | 240 |
| Source | 320 | 200 |
| Inspector | 260 | 160 |
| Diagnostics | 260 | 100 |

- **Shortfall rule.** A part that falls below its minimum is set to the
  minimum, and the other parts shrink in proportion (the largest gives the
  most).
- **Overflow rule.** When the window is smaller than the layout's minimum,
  the dock keeps its minimum size and the workspace scrolls. No panel
  collapses and no size is negative. The Source preset needs 852 × 466 px;
  the 960 × 600 window minimum fits it.
- **Pointer drag.** Every move recomputes from the drag's start layout, so
  clamping never accumulates. A divider stops where either neighbor reaches
  its minimum. The layout is saved on release; a cancel restores the start.
- **Keyboard.** A focused divider moves 16 px with an arrow key (64 px with
  Shift). Home and End go to the limits. Each step is saved.
- **Focus.** Tabs and dividers show the 3 px `--wf-focus-ring` outline.

## 4. Tab groups

- A click on a tab brings it to the front.
- A drag along the strip reorders the tabs. A drag to another strip moves
  the panel into that group.
- Keyboard on a focused tab: Left / Right / Home / End select a tab (focus
  follows), Enter and Space select, Shift+Left / Shift+Right reorder, and
  Delete hides an optional panel.
- An inactive tab keeps its state (§5). The tab strip has `role="tablist"`;
  tabs have `role="tab"`, `aria-selected` and `aria-controls`.

## 5. Panel lifetime strategy

Leptos owns each panel's DOM and its reactive scope. Moving a mounted node to
another parent behind Leptos would break that ownership and its cleanup. This
lane does not move nodes.

- `dock::Workspace` mounts each of the five panel components **once**, in a
  fixed host element (`#dock-panel-<slug>`). The hosts never move in the DOM
  and are never re-created.
- A layout change sets only each host's `left`, `top`, `width`, `height`
  and `data-state`: `active`, `inactive` (a background tab) or `hidden`
  (closed).
- An inactive or hidden host uses `visibility: hidden` and
  `pointer-events: none`. Its layout box remains, so the textarea keeps its
  scroll position and the canvas keeps its size. A closed panel keeps its
  last box.
- Only the tab strips, the dividers and the drag overlay are rendered from
  the tree. They hold no state. Strips are keyed by their panel list and
  dividers by their path, so a live resize does not replace the divider that
  holds the pointer.

Effects of this strategy:

| state | result |
|---|---|
| Source textarea | the same element: value, selection, caret, scroll, `#source-hl` registration |
| X_ITE `<x3d-canvas>` | the same element and browser: no scene reload, the same preview generation, the same pick and gizmo bindings |
| Inspector | an unapplied field value stays in its input; hiding never applies it |
| Document | no layout code calls an edit, save, history or preview command |

The smoke run proves object identity. It marks the textarea and the canvas
objects with a JavaScript property, and the mark is still present after
every move. It also compares the inspection object and the input element
before and after a close and restore.

## 6. Active 3D tools

- `dock::apply` calls `gizmo::cancel("the layout changed")` **before** it
  changes the layout. A docking drag start and a divider press do the same.
- A workspace size change (window resize, banner) cancels first with "the
  workspace was resized".
- Cancel restores the rendered value. It writes no source edit and creates
  no undo step. The tool mode (Select, Move, Rotate, Scale) stays on.
- After a resize, X_ITE resizes its own canvas. The gizmo reads the canvas
  rectangle each frame, and picking uses the canvas rectangle of the click,
  so both follow the new bounds.
- Native viewport (hidden): it is a native split **beside** the WebView
  (NATIVE-RENDER-1 D6), not a rectangle inside the page. A dock change does
  not move it, and GTK / AppKit resize it with the window. No pane rectangle
  is sent to Rust; the §4.6 contract item does not apply to the D6 layout.
  The `--smoke-native` self-test was run as a regression (§12).

## 7. Settings format

The layout is stored in the Tauri `settings.json`, next to the theme:

```json
{
  "schemaVersion": 1,
  "themeId": "tokyo-night",
  "viewport": { "renderer": "native-experimental" },
  "layout": {
    "version": 1,
    "preset": "preset.source",
    "root": {
      "type": "split", "axis": "row", "sizes": [0.18, 0.48, 0.34],
      "children": [
        { "type": "tabs", "panels": ["panel.sceneTree"], "active": "panel.sceneTree" },
        { "type": "split", "axis": "column", "sizes": [0.76, 0.24], "children": [ … ] },
        { "type": "split", "axis": "column", "sizes": [0.56, 0.44], "children": [ … ] }
      ]
    }
  }
}
```

- `layout.version` is the layout schema version (1). It is separate from the
  file's `schemaVersion`.
- The theme and the layout use **one** writer. Both hold the same lock across
  the read-modify-write, each replaces only its own keys, and both use the
  existing temp-file, fsync, read-back and atomic-rename path. Unknown keys
  (for example `viewport.renderer`) are kept.
- A first layout write into a file without `schemaVersion` / `themeId` adds
  them (the current theme), so the theme reader accepts the file.
- The UI sends a layout with `layout_set`. Rust validates it again as a
  whole. A layout with an unknown panel or a bad shape is rejected and
  nothing is written.
- The UI saves after each committed change, with at most one write in
  flight; the last state wins.
- The layout is **never** written into a `.wrl`, a `.wrz`, a sidecar or a
  recovery record. The smoke run scans every other file and checks this.

Recovery at startup (never blocks startup; the theme still loads):

| saved data | result |
|---|---|
| no file, or no `layout` key | the Source preset, no notice |
| not JSON | Source preset + notice |
| `layout` not an object, no valid `version` | Source preset + notice |
| `version` < 1 | Source preset + "too old" notice |
| `version` > 1 | Source preset + "from a newer WRL Forge" notice |
| wrong shape, bad sizes, duplicate or missing required panel, too deep | Source preset + "invalid (reason)" notice |
| unknown panel IDs only | the layout without them + a notice naming them |

The notice shows beside the Layout control (`#layout-notice`). The next
successful save clears it.

## 8. Presets

| preset | arrangement | ratios |
|---|---|---|
| `preset.source` (default) | Scene Tree · (Source over Diagnostics) · (Viewport over Inspector) | row 0.18 / 0.48 / 0.34; columns 0.76 / 0.24 and 0.56 / 0.44 |
| `preset.modeling` | Scene Tree · large Viewport · (Inspector over a [Diagnostics, Source] tab group) | row 0.17 / 0.55 / 0.28; column 0.55 / 0.45 |

- The default is Source, the 1.4.5 arrangement.
- A preset change replaces the layout only. The document, the selection, the
  history and the preview stay unchanged.
- Reset Layout restores the active preset exactly.
- A closed panel returns to its preset place with the preset's share: Scene
  Tree at the far left at full height; Diagnostics below the Source (Source
  preset) or as the first tab beside the Source (Modeling); Inspector below
  the Viewport (Source) or above the Diagnostics / Source group (Modeling).
  Closing any subset and restoring it in any order gives the preset again
  (unit tested: 7 subsets and all 6 orders, both presets).

## 9. Command IDs

All commands are in the APP-SHELL-0 registry, run through `command::run`,
and use the existing enabled-state checks. None has a shortcut, so there is
no second shortcut table.

| ID | label | requires | backend |
|---|---|---|---|
| `view.panel.sceneTree` | Show Scene Tree | — | `layout_set` |
| `view.panel.inspector` | Show Inspector | — | `layout_set` |
| `view.panel.diagnostics` | Show Diagnostics | — | `layout_set` |
| `view.hidePanel.sceneTree` | Hide Scene Tree | panel shown | `layout_set` |
| `view.hidePanel.inspector` | Hide Inspector | panel shown | `layout_set` |
| `view.hidePanel.diagnostics` | Hide Diagnostics | panel shown | `layout_set` |
| `view.focus.viewport` | Focus 3D Viewport | — | `layout_set` |
| `view.focus.source` | Focus Source | — | `layout_set` |
| `view.preset.modeling` | Modeling Layout | — | `layout_set` |
| `view.preset.source` | Source Layout | — | `layout_set` |
| `view.resetLayout` | Reset Layout | — | `layout_set` |

- Every entry has `menu: View` and `mutates_document: false`.
- `layout_set` is the only new name in `ALLOWED_BACKEND`. It is a registered
  Tauri command (the existing `app_shell` test proves this), and it writes
  settings only.
- `Requires::PanelShown` disables a Hide command for a closed panel and
  refuses it at run time with "The panel is not shown".
- "Show" on a shown panel brings it to the front and focuses its tab.
  "Focus Source" focuses the textarea, which keeps its selection.

The toolbar has one real control before the menu bar exists: **Layout: ▾**.
It lists the three panels as check items, Focus 3D Viewport, Focus Source,
the two presets as radio items, and Reset Layout. Each item runs its registry
command and carries `data-command`. The tab `×` button runs the Hide command.

## 10. Accessibility and header controls

- Every tab, divider and menu item is reachable with the keyboard (§3, §4;
  the menu uses the Create-menu keys: arrows, Home, End, Escape, Tab).
- Focus is always visible (`--wf-focus-ring`).
- An active tab shows a top bar and a color change, never color alone.
- The drag overlay has a live-region sentence ("Release to dock Inspector
  right of 3D Viewport.").
- The viewport header now wraps (`APP_SHELL_0.md` §3.6). Select, Move,
  Rotate and Scale form one `role="group"` that wraps as a unit. They keep
  their `aria-pressed` state and their actions are unchanged. The smoke run
  checks that all four are inside the panel and do not overlap at 1440,
  1100 and 960 px windows (a 900 px request gets 960 px: the window
  minimum).
- The toolbar wraps to a second row when the window is narrow, so the page
  never scrolls sideways. The long profile label has a zero flex basis and
  truncates with an ellipsis, so the toolbar keeps one row at 1440 px.
- The canvas minimum is 6rem (was 12rem), so the preview status line stays
  inside the 240 px viewport minimum.
- `style.css` uses only semantic `--wf-*` tokens; the existing theme test
  enforces this.

## 11. Source-safety proof

Before and after each workspace change, the dock smoke compares one record:

- the canonical text hash, revision, dirty flag, can-undo and can-redo
  (from Rust, `doc_snapshot`);
- the textarea text hash, selection range and scroll position;
- the Scene Tree selection and the Inspector inspection;
- the X_ITE preview load count, the preview status text and the pick
  generation;
- the object marks on the textarea and the canvas;
- the syntax layer state.

The record must be identical after: a center drop, a tab click, a strip
reorder, keyboard tab moves, an edge drop, an invalid drop, an Escape cancel,
Reset Layout, three divider drags, keyboard resizing, both presets (twice,
from the menu), and the Source tab in the Modeling preset.

Also checked:

- Real viewport clicks after a tab switch, after a resize and in the
  Modeling preset each select exactly the expected Transform:
  `WRONG_SOURCE_SELECTIONS = 0`.
- A preset change during an active Move drag, and a window resize during an
  active Move drag, cancel the drag. The source, revision and history are
  unchanged, no commit happens, and Move stays on. The redrawn gizmo origin
  matches the projected object within 1 px inside the new viewport.
- After all changes, one Undo returns the exact original text.
- Rust checks that the fixture file on disk is byte-identical and that no
  file except `settings.json` contains layout data.

## 12. GUI test results

Linux x64, Xvfb software rendering (1600 × 1000), a clean detached
verification worktree. Every GUI run had its own `xvfb-run -a` display and
its own `/tmp` directory.

Two complete runs were made. The first ran on the implementation commit
`a93f8ea`. The second ran on the final branch head; it is the gate run. Both
runs gave the results below. The APP-SHELL-0 column is the base `86bcba9`.

| suite | UI-DOCK-1 | APP-SHELL-0 base |
|---|---|---|
| `cargo test --workspace` (apps/desktop-tauri) | 193 pass, 0 fail | 166 pass |
| `cargo test --workspace` (crates) | 193 pass, 0 fail | 193 pass |
| `./build-ui.sh` (release Wasm, X_ITE 16.5.0 verified) | OK | OK |
| backend build (`wrl-forge-desktop`) | OK, `WRL Forge 1.4.5 (Beta)` | OK |
| `--dock` main (real X input) | 48 / 48 | — (new) |
| `--dock` restore (restart) | 7 / 7 | — (new) |
| `--dock` damaged layout | 7 / 7 | — (new) |
| files (58 fixtures) | 47 / 58, identical per file | 47 / 58 |
| `--inspector` (58 fixtures) | 18 / 58, identical per file | 18 / 58 |
| `--pick` (37 fixtures, 152 clicks) | 130 / 130, `WRONG_SOURCE_SELECTIONS = 0` | pass, WRONG 0 |
| `--move` | 156 / 156 | 156 |
| `--rotscale` | 71 / 71 | 71 |
| `--extrusion` | 58 / 58 | 58 |
| `--create` | 51 / 51 | 51 |
| `--recovery` | crash OK (4 records), restore 34 / 34, later 15 / 15 | the same |
| texture matrix | 46 / 46 | 46 |
| native self-test (`native-smoke/harness.py xvfb`) | 46 / 46 | 46 |

- The 11 files that fail in the file matrix are the known invalid inputs
  (malformed VRML, a corrupt gzip). They fail the same steps as on the base,
  file for file. The Inspector matrix also matches the base file for file.
- The new layout model has 19 unit tests; settings has 7 new tests; the
  command registry has 1 new test. They are in the apps count above.
- Windows and macOS were not tested (§13).

## 13. Known limits

- **Windows x64: not tested.** No native Windows system was used.
- **macOS 15+ Apple Silicon: not tested.** No macOS system was used.
- Keyboard docking to **another** group is not implemented. The keyboard can
  reorder tabs, switch tabs, resize, close, restore and change presets; a
  move to another group needs the pointer.
- A drag that ends outside every group docks nothing (it is a refusal, not a
  root-edge drop). Root-edge docking is not offered.
- A layout saved by a newer WRL Forge is replaced by the next layout change
  in this version (the notice says so at startup).
- At narrow windows the toolbar takes two rows, which leaves less height
  for the workspace (the workspace then scrolls if needed).
- The panel's own title row (for example "Scene Tree · 8 items") is kept
  under the tab, because existing smoke checks and status text use it.
- The native viewport does not receive a pane rectangle (§6); it is a native
  sibling of the WebView by design.
- `cargo fmt --check` already fails on the base in files this lane did not
  change. New files are formatted, and changed hunks are formatted.
