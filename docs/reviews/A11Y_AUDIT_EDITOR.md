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
| Rust Contrast | WCAG 1.4.3 Contrast (Minimum) | Fail | `apps/desktop-tauri/ui/static/themes.css:179` |
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

**4. Secondary Text Contrast Ratio (Tokyo Night Theme) (Moderate)**
- **Affected element:** Secondary Text in Tauri Tokyo Night.
- **Criterion:** WCAG 1.4.3 Contrast (Minimum) 4.5:1.
- **Evidence:** `apps/desktop-tauri/ui/static/themes.css:179` (`--wf-text-disabled: #545c7e;` on `--wf-bg-app: #1a1b26`).
- **Fix:** The contrast ratio of `#545c7e` on `#1a1b26` is 2.76:1, which is below the 4.5:1 AA threshold.

*Contrast Evaluation Data (WCAG 2.x relative-luminance formula)*:
- Electron Dark: text (`#ddd`) on background (`#1b1b1b`) = 12.68:1 (Passes > 4.5:1)
- Electron Dark: primary button text (`#fff`) on bg (`#2d6ae0`) = 4.94:1 (Passes > 4.5:1)
- Electron Dark: primary button focus (`#7db3ff`) on bg (`#1b1b1b`) = 8.01:1 (Passes > 3:1)
- Tauri Tokyo: text (`#a9b1d6`) on background (`#1a1b26`) = 8.10:1 (Passes > 4.5:1)
- Tauri Tokyo: secondary text (`#565f89`) on background (`#1a1b26`) = 2.76:1 (Fails < 4.5:1)
- Tauri High Contrast: text (`#ffffff`) on background (`#000000`) = 21.00:1 (Passes > 4.5:1)
- Tauri High Contrast: primary button text (`#ffffff`) on bg (`#005fcc`) = 5.98:1 (Passes > 4.5:1)
- Tauri High Contrast: focus (`#00e0ff`) on bg (`#000000`) = 13.11:1 (Passes > 3:1)

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