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
| Electron Contrast | WCAG 1.4.3 Contrast (Minimum) | Fail | `renderer/editor.html:97`, `100` |
| Electron Zoom | WCAG 1.4.4 Resize Text | Pass | `renderer/editor.html:434-438` |
| Command ARIA | ARIA `aria-keyshortcuts` | Pass | `renderer/command-bindings.js:42` |

### Findings List

**1. Toolbar Keyboard Navigation (Moderate)**
- **Affected element:** `<div class="toolbar" role="toolbar">` and `<div class="model-bar" role="toolbar">` (Electron), `<header class="toolbar" role="toolbar">` (Rust).
- **Criterion:** ARIA Authoring Practices Guide (APG) Toolbar pattern.
- **Evidence:** `renderer/editor.html:418`, `449`, `apps/desktop-tauri/ui/src/lib.rs:190`.
- **Fix:** A `role="toolbar"` must have a single tab stop, and its focusable child controls should be navigated using the arrow keys. Static markup uses `tabindex="0"` for the default but the scripts do not appear to handle arrow key navigation.

**2. Tree Widget `aria-expanded` State (Moderate)**
- **Affected element:** Scene tree nodes `role="treeitem"`.
- **Criterion:** APG Tree View pattern.
- **Evidence:** `renderer/scene-tree.js:96`, `apps/desktop-tauri/ui/src/panels.rs:63`.
- **Fix:** Tree items that have children must have `aria-expanded="true"` or `aria-expanded="false"`. Electron manually sets this, but Rust's Leptos UI is missing it for expandable nodes. Both need full keyboard navigation (Right/Left arrows to expand/collapse).

**3. Primary Button Contrast Ratio (Moderate)**
- **Affected element:** Primary `<button>`.
- **Criterion:** WCAG 1.4.3 Contrast (Minimum) 4.5:1.
- **Evidence:** `renderer/editor.html:97` (`background: #2d6ae0`, `color: #fff`).
- **Fix:** The contrast ratio of `#fff` on `#2d6ae0` is approximately 4.45:1, which is just under the 4.5:1 AA threshold. The background blue or the text color needs to be adjusted.

**4. Modal Focus Management (Unverified)**
- **Affected element:** `.modal-backdrop` and `.about-dialog` `role="dialog"`.
- **Criterion:** WCAG 2.4.3 Focus Order.
- **Evidence:** `renderer/editor.html:616`, `apps/desktop-tauri/ui/src/about.rs:144`.
- **Fix:** Cannot be fully verified statically. Dialogs must trap focus within them while open and return focus to the triggering element when closed.

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
