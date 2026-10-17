# Draft — proposal for owner review; not approved

## Accessibility Audit Report: Editor UI

**Scope:**
1. The Electron editor (`renderer/*.html`, `renderer/command-bindings.js`, `src/editor/command-registry.js`).
2. The Rust/Tauri UI (`apps/desktop-tauri/ui/src/`).

**Method:** Static review against WCAG 2.2 AA and WAI-ARIA 1.2 / APG patterns.

### Summary Table

| Area | Criterion | Status | Evidence `path:line` |
|---|---|---|---|
| Electron Toolbar | APG Keyboard Navigation | Fail | `renderer/editor.html:418`, `449` |
| Rust Toolbar | APG Keyboard Navigation | Fail | `apps/desktop-tauri/ui/src/lib.rs:190` |
| Electron Tree | APG Tree Widget | Fail | `renderer/scene-tree.js:96` |
| Rust Tree | APG Tree Widget | Fail | `apps/desktop-tauri/ui/src/panels.rs:63` |
| Electron Dialog | Modal Focus Trap | Unverified | `renderer/editor.html:616` |
| Rust Dialog | Modal Focus Trap | Unverified | `apps/desktop-tauri/ui/src/about.rs:144` |
| Electron Contrast | WCAG 1.4.3 Contrast (Minimum) | Pass | `renderer/editor.html:97`, `100` |
| Rust Contrast | WCAG 1.4.3 Contrast (Minimum) | Partial | `apps/desktop-tauri/ui/static/themes.css:305`, `309` (Tokyo Night Light muted/placeholder on `--wf-bg-app`) |
| Electron Zoom | WCAG 1.4.4 Resize Text | Pass | `renderer/editor.html:434-438` |
| Rust Zoom | WCAG 1.4.4 Resize Text | Unverified | `apps/desktop-tauri/ui/src/` |
| Command ARIA | ARIA `aria-keyshortcuts` | Pass | `renderer/command-bindings.js:42` |
| ARIA Names | Icon-only Controls | Pass | `renderer/editor.html:435`, `apps/desktop-tauri/ui/src/findbar.rs:438` |
| Tooltip Use | `title` Misuse | Fail | `renderer/editor.html:435` |
| State ARIA | `aria-pressed` / `aria-expanded` | Pass | `renderer/command-bindings.js:59`, `apps/desktop-tauri/ui/src/menu.rs:399` |
| Live Regions | `aria-live` Status | Pass | `renderer/editor.html:445`, `apps/desktop-tauri/ui/src/dock.rs:1065` |
| Rust Menu | APG Menubar | Pass | `apps/desktop-tauri/ui/src/menu.rs:387` |

### Findings List

**1. Toolbar Keyboard Navigation (Moderate)**
- **Affected element:** `<div class="toolbar" role="toolbar">` and `<div class="model-bar" role="toolbar">` (Electron), `<header class="toolbar" role="toolbar">` (Rust).
- **Criterion:** ARIA Authoring Practices Guide (APG) Toolbar pattern.
- **Evidence:** `renderer/editor.html:418`, `449`, `apps/desktop-tauri/ui/src/lib.rs:190`. Search for `keydown` and `ArrowLeft`/`ArrowRight` shows handlers exist for dock splitters (`dock.rs:642`) and the menubar (`menu.rs:209`) but no specific arrow-key roving focus management exists for the `role="toolbar"` containers.
- **Fix:** A `role="toolbar"` must have a single tab stop, and its focusable child controls should be navigated using the arrow keys.

**2. Tree Widget `aria-expanded` State (Moderate)**
- **Affected element:** Scene tree nodes `role="treeitem"`.
- **Criterion:** APG Tree View pattern.
- **Evidence:** `renderer/scene-tree.js:96`, `apps/desktop-tauri/ui/src/panels.rs:63`.
- **Fix:** Tree items that have children must have `aria-expanded="true"` or `aria-expanded="false"`. Electron manually sets this, but Rust's Leptos UI is missing it for expandable nodes. Both need full keyboard navigation (Right/Left arrows to expand/collapse). `grep "ArrowLeft" apps/desktop-tauri/ui/src/panels.rs` yields no results.

**3. Tooltip `title` Misuse (Minor)**
- **Affected element:** Icon-only controls.
- **Criterion:** WAI-ARIA 1.2 / APG best practices.
- **Evidence:** `renderer/editor.html:435` `<button id="zoomOutBtn" data-command="view.zoomOut" class="secondary" title="Decrease size (Ctrl -)" aria-label="Decrease size">−</button>`.
- **Fix:** The `title` attribute is often read inconsistently by screen readers. A combination of `aria-label` for the base name and `aria-description` or a custom tooltip for the keyboard shortcut is preferred.

**4. Muted and Placeholder Text Contrast, Tokyo Night Light (Moderate)**
- **Affected element:** Muted and placeholder text in the Tauri `tokyo-night-light` theme, wherever it is drawn directly on the app background (`--wf-bg-app`).
- **Criterion:** WCAG 1.4.3 Contrast (Minimum), 4.5:1 for normal text.
- **Evidence:** `apps/desktop-tauri/ui/static/themes.css:292` (`--wf-bg-app: #c1c2c7`), `:305` (`--wf-text-muted: #4f5266`), `:309` (`--wf-text-placeholder: #5a5d6f`).
- **Fix:** On `--wf-bg-app`, muted text measures 4.32:1 and placeholder text 3.66:1, both below 4.5:1. On the editor surface (`--wf-surface-editor: #e6e7ed`) they pass, at 6.23:1 and 5.27:1. Darken the two tokens for this theme, or confirm at runtime that muted and placeholder text never sit directly on `--wf-bg-app`. Which surfaces carry these tokens was not traced statically, so this is **Partial** rather than Fail.
- **Not a finding:** `--wf-text-disabled` (e.g. `#545c7e`, 2.23–2.90:1 in the dark themes) is used for inactive controls. WCAG 1.4.3 exempts text in inactive user-interface components.

*Contrast evaluation data (WCAG 2.x relative-luminance formula; every value read from the cited line):*

| Theme (`apps/desktop-tauri/ui/static/themes.css`) | Token | Foreground | Background | Ratio | Result (4.5:1 text) |
|---|---|---|---|---|---|
| `tokyo-night` (`:26`) | text-primary `:41` | `#c0caf5` | bg-app `#101014` `:30` | 11.76:1 | Pass |
| `tokyo-night` | text-secondary `:43` | `#a9b1d6` | bg-app `#101014` | 8.99:1 | Pass |
| `tokyo-night` | text-muted / placeholder `:44`, `:48` | `#8089b3` | bg-app `#101014` | 5.56:1 | Pass |
| `tokyo-night` | text-muted | `#8089b3` | surface-editor `#1a1b26` | 5.00:1 | Pass |
| `tokyo-night-storm` (`:164`) | text-secondary `:179` | `#a9b1d6` | bg-app `#1b1e2e` `:167` | 7.82:1 | Pass |
| `tokyo-night-storm` | text-muted / placeholder `:180`, `:184` | `#8a94c2` | bg-app `#1b1e2e` | 5.58:1 | Pass |
| `tokyo-night-storm` | text-muted | `#8a94c2` | surface-editor `#24283b` | 4.92:1 | Pass |
| `tokyo-night-light` (`:289`) | text-primary `:302` | `#343b59` | bg-app `#c1c2c7` `:292` | 6.16:1 | Pass |
| `tokyo-night-light` | text-muted `:305` | `#4f5266` | bg-app `#c1c2c7` | **4.32:1** | **Fail** |
| `tokyo-night-light` | text-placeholder `:309` | `#5a5d6f` | bg-app `#c1c2c7` | **3.66:1** | **Fail** |
| `tokyo-night-light` | text-muted / placeholder | `#4f5266` / `#5a5d6f` | surface-editor `#e6e7ed` | 6.23:1 / 5.27:1 | Pass |
| Electron default dark (`renderer/editor.html`) | body text | `#ddd` | `#1b1b1b` | 12.68:1 | Pass |
| Electron default dark | primary button `:97` | `#fff` | `#2d6ae0` | 4.94:1 | Pass |
| Electron default dark | focus outline | `#7db3ff` | `#1b1b1b` | 8.01:1 | Pass (3:1 non-text) |

The Tauri UI defines only the three themes above, so it has no high-contrast theme. The Electron editor offers `dark`, `light`, `terminal`, `tokyo` and `contrast` (`src/settings/preferences.js:46`). Only the default dark palette was computed here; the other four Electron themes are **Unverified**.

**5. Modal Focus Management (Unverified)**
- **Affected element:** `.modal-backdrop` and `.about-dialog` `role="dialog"`.
- **Criterion:** WCAG 2.4.3 Focus Order.
- **Evidence:** `renderer/editor.html:616`, `apps/desktop-tauri/ui/src/about.rs:144`.
- **Fix:** Cannot be fully verified statically. Dialogs must trap focus within them while open and return focus to the triggering element when closed.

**6. Tauri UI Zoom (Unverified)**
- **Affected element:** Entire Tauri application.
- **Criterion:** WCAG 1.4.4 Resize Text.
- **Evidence:** `apps/desktop-tauri/ui/src/`.
- **Fix:** Cannot verify UI scaling statically without executing the application layout to verify text clipping or scaling limits.

### Not testable statically
The following criteria require a manual screen-reader and keyboard pass on Linux (e.g., Orca):
1. **Focus Trapping:** Open the Preferences, About, and Conflict dialogs. Press `Tab` repeatedly to ensure focus loops within the dialog. Close the dialog and ensure focus returns to the original button.
2. **Keyboard Roving Tabindex:** Focus a toolbar or the scene tree. Ensure arrow keys properly navigate through the items without `Tab`.
3. **Live Regions:** Trigger errors and previews. Use Orca to ensure `aria-live="polite"` reads out the status changes reliably.

### Coverage Note
**Files read:**
- `renderer/editor.html`
- `renderer/index.html`
- `renderer/world.html`
- `renderer/scene-tree.js`
- `renderer/command-bindings.js`
- `src/editor/command-registry.js`
- `apps/desktop-tauri/ui/src/*.rs` (Dock, Menu, Panels, Lib, About, Theme)
- `docs/ui/UI0_COMMAND_WORKSPACE_ARCHITECTURE.md`

**Files not read:**
- Deep X_ITE renderer internals.
- Main process IPC handlers.
- End-to-end tests without DOM focus evaluation.