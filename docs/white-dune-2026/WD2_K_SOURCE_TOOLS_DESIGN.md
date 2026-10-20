# WD2-K — Source Tools Design

**Status:** Draft — proposal for owner review; not approved.

**Lane:** WD2-K (Source Tools)
**Goal:** Power-user source tooling in the native editor.
**Deliverable:** This design document covering source-patch shape, failure modes, undo, and test plan.

## 1. Overview and Scope

This lane designs advanced source tools for the native WRL Forge editor, leaning heavily on the lossless document core, the `src/vrml` parser, and the WD1.5 scope graph.

**In scope:**
*   Symbol search
*   Go-to-definition and navigation
*   Command palette integration (via UI-0)
*   DEF/USE references mapping
*   Structural selection (selecting nodes and fields in source by their boundaries)
*   Script editing UX (syntax highlighting within CDATA/string blocks where safely detectable)

**Explicitly Out of Scope:**
*   **Script execution:** No VRML Script node evaluation is added here.
*   **Second grammar/Language Server:** `src/vrml` remains the sole grammar authority. No external language servers or duplicate parsers are introduced.
*   **Rename refactoring:** Deferred to a future lane to keep navigation and discovery focused and safe.

## 2. Dependencies

1.  **UI-0 Command Registry:** The Command palette must route through the `UI-0` shared command registry. It must not invent a secondary dispatch mechanism.
2.  **WD1.5 Scope Graph:** DEF/USE references, go-to-definition, and symbol search must consume the `scope-graph.js` model built in WD1.5.
3.  **`src/vrml` Authority:** Structural selection and all token/AST boundaries strictly rely on the existing `src/vrml` parse results.

## 3. Design: Navigation and Ambiguity

The core mandate for all navigation actions (Go-to-definition, Find References) is: **Never confidently point at the wrong node.**

1.  **Resolution via Scope Graph:** All reference lookups query the WD1.5 scope graph.
2.  **Handling Ambiguity:** If the scope graph returns an `ambiguous` result (e.g., duplicate DEF names in the same scope, or unresolvable PROTO bindings), the UI *must* explicitly represent this ambiguity to the user (e.g., a "Multiple definitions found" list or an "Ambiguous reference" warning toast) rather than guessing the "closest" match.
3.  **Fail-Closed Identity:** If node identity cannot be proven (e.g., due to document recovery states or unresolved graph nodes), the action must fail closed. It must refuse to navigate rather than navigating to a confidently wrong location.

## 4. Design: Source Patches and Undo

While rename refactoring is out of scope, structural selection and Script UX may involve targeted edits (e.g., surrounding a node, or formatting a script block).

1.  **Exact-Source-Text Contracts:** Any edit performed via source tools must use span-anchored text patching (`src/vrml/edit.js`). Text outside the targeted edit span is never touched.
2.  **Minimal Source Patches:** Where edits occur, they must be the minimal possible string replacement, preserving surrounding whitespace, CRLF, and comments.
3.  **CodeMirror Undo Integration:** All text modifications must be dispatched as CodeMirror `Transaction` objects so that the native CodeMirror undo history manages them seamlessly. UI state is strictly separate from document state; do not store UI selection states into the document, source text, analysis, or CodeMirror undo history.

## 5. Design: Command Palette and UX

1.  **Command Palette:** A modal UI component that lists available actions. It exclusively lists commands registered in the UI-0 command registry.
2.  **Structural Selection:** Expand/shrink selection based on the `src/vrml` AST boundaries (e.g., expand from field value -> field -> node -> parent node).
3.  **Script Editing UX:** Enhance the editor to provide better syntax highlighting or structural assistance inside `Script` node `url` blocks (specifically `javascript:` or `vrmlscript:` protocols), but relying purely on CodeMirror language nesting, not a heavy LSP.

## 6. Failure Modes (Fail-Closed Cases)

The following states must be gracefully handled (fail-closed, no crash, no wrong answers):

1.  **Parse Failure/Recovery:** Document in `document-parse-incomplete` or `scope-recovered` state. Navigation commands disable or report "Document structure incomplete".
2.  **Ambiguous Resolution:** As defined in §3, duplicate DEFs or unresolved references result in an ambiguous UI presentation, not a silent fallback.
3.  **Cross-File Boundary Limit:** Go-to-definition on an `EXTERNPROTO` USE or external file reference does not attempt to open a new tab/file in this lane (unless already supported by base architecture). It reports "Defined externally".
4.  **Out-of-Bounds Selection:** Structural selection at the root of the document or on whitespace gracefully halts expansion.

## 7. Test Plan

**Independent QA is required for this lane.**

**Focused Tests (Unit/Integration in Node):**
1.  **Ambiguity tests:** Querying the scope graph for a duplicate DEF returns the ambiguous state, and the command layer correctly maps this to an ambiguous UI response.
2.  **Selection tests:** Structural selection on 5+ specific node/field nesting scenarios expands to the exact expected text spans.
3.  **Command Registry tests:** The command palette provider returns *only* commands that exist in the UI-0 registry.
4.  **Undo tests:** Triggering a source tool edit and reversing it via the editor model restores the exact original byte stream.

**Visual QA / Electron Tests:**
1.  **Palette UX:** Verify the Command Palette opens, filters correctly by typing, and executes a UI-0 command via keyboard (e.g., Arrow Down, Enter).
2.  **Navigation UX:** Click "Go to Definition" on a `USE` node; verify cursor moves and editor scrolls to the exact `DEF` span.
3.  **Ambiguity UX:** Trigger "Go to Definition" on an ambiguous `USE`; verify a list or explicit warning is shown, and the cursor does *not* silently jump to the first occurrence.
4.  **Script UX:** Verify a `Script` node containing a `javascript:` string block renders with appropriate styling (if applicable) and structural selection respects the string boundaries.

## 8. Unresolved Decisions and Limits

*   **Owner Review:** Design is pending owner review.
*   **Independent QA:** Not yet completed.
*   **Script Nesting Depth:** How deep CodeMirror should attempt to parse nested languages inside Script URL arrays.
*   **No File System Changes:** The tools operate entirely within the editor buffer memory and the `src/vrml` parse. They do not access the filesystem directly, preserving filesystem authorization contracts.
