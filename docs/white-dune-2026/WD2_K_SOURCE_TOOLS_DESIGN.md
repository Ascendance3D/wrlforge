# WD2-K — Source Tools Design

**Status:** Draft — proposal for owner review; not approved. Merging this document does not authorize implementation; each step in §9 starts on its own explicit owner GO.

**Lane:** WD2-K (Source Tools), issue #92
**Goal:** Power-user source tooling in the native editor.
**Deliverable:** This design document covering source-patch shape, failure modes, undo, and test plan.

## 1. Overview and Scope

This lane designs advanced source tools for the native WRL Forge editor. It relies on the lossless document core (`WD1_LOSSLESS_DOCUMENT_CORE_PLAN.md`), the `src/vrml` parser, and the WD1.5 scope graph (`src/vrml/scope-graph.js`).

The exact text buffer is the only document authority. Every tool below is a read of a projection derived from that text, or a verified span edit of it. No tool creates a second editable model, writes identifiers into the source, or regenerates text from an AST.

**In scope (design only):**
*   Symbol search (DEF names, PROTO/EXTERNPROTO declarations)
*   Go-to-definition and find references
*   Structural selection (expand/shrink by `src/vrml` parse boundaries)
*   Command palette integration (via the UI-0 command registry)
*   Script string handling for navigation and selection (§6). Script syntax assistance beyond that is a separately approved later step (§9, step 5).

**Explicitly out of scope:**
*   **Script execution:** No VRML Script evaluation. The protected-preview CSP is unchanged.
*   **Second grammar / language server:** `src/vrml` remains the sole grammar authority. No external language server, no additional parser, and no CodeMirror language package (for example a JavaScript grammar) is introduced by this lane.
*   **Rename refactoring:** Deferred to a future lane.
*   **Filesystem access:** The tools operate on the editor buffer and its `src/vrml` parse only. They do not read files, so filesystem authorization contracts are unchanged.

## 2. Dependencies (existing code)

1.  **UI-0 command registry** — `src/editor/command-registry.js` (`docs/ui/UI0_COMMAND_WORKSPACE_ARCHITECTURE.md`). The palette lists and dispatches only registered commands; no second dispatch mechanism.
2.  **WD1.5 scope graph** — `buildScopeGraph`, `resolve`, `referencesTo`, `symbolFor`, `referenceFor`, `defIsUniqueInScope`, and the `STATUS` / `REASON` constants exported from `src/vrml/scope-graph.js`.
3.  **Parse sessions and verified transactions** — `createParseSession` and `verifyTransaction` in `src/vrml/document-transaction.js`, over the WD1.2 span algebra in `src/vrml/edit.js` (UTF-16 code-unit offsets, half-open ranges).
4.  **Editor dispatch** — `applyVerifiedEdits({ oldText, edits, newText, userEvent })` in `src/editor/browser/editor-view.js`. It refuses with `buffer-changed` unless the buffer still equals `oldText` and dispatches the whole edit set as one isolated CodeMirror history event.
5.  **Highlighting** — `analyze()` in `src/editor/language.js` produces highlight spans from `src/vrml` tokens. The editor has no CodeMirror grammar of its own.

## 3. Navigation and Ambiguity

The rule for every navigation action (go-to-definition, find references, symbol search): **never point confidently at the wrong node.**

1.  **Scope graph is the reference authority.** Every lookup goes through `resolve(graph, reference)` on a graph built from the current parse session. The tools never match DEF names by text search, and never re-implement scoping.
2.  **Navigate only on `STATUS.RESOLVED` with a symbol.** The command layer moves the cursor only when the resolution's `status` is `STATUS.RESOLVED` and it carries a `symbol` with a `declRange`. Every other status refuses navigation. The cursor does not move.
3.  **Defined results.** The command layer maps each resolution to exactly one of these results (names proposed):

    | Scope-graph result | Command result | User-facing behavior |
    |---|---|---|
    | `RESOLVED` with `symbol` | `navigate` | Select the DEF name span (`declRange`) and scroll it into view. |
    | `RESOLVED`, reason `NODE_TYPE_IS_BUILTIN` | `refuse:builtin` | "Built-in node type — no source definition." |
    | `AMBIGUOUS` (for example `DUPLICATE_DEF_IN_SCOPE`) | `ambiguous` | List every candidate from `evidence` in source order. Nothing is preselected; the user picks one explicitly. |
    | `INVALID` (`MISSING_NAME`, `SELF_REFERENTIAL_USE`, `USE_BEFORE_DEF`, …) | `refuse:invalid` | "Cannot navigate: <reason message>." `USE_BEFORE_DEF` may list the later declarations as information, not as a jump target. |
    | `UNRESOLVED` | `refuse:unresolved` | "No visible definition." |
    | `UNSUPPORTED` | `refuse:unsupported` | "Not supported." |
    | `RECOVERED` | `refuse:incomplete` | "Document structure incomplete." |
    | Any status not listed above | `refuse:unknown` | Refuse. New statuses never default to navigation. |

4.  **No first-match fallback.** When there are several candidates, the tool never jumps to the first, closest, or type-matching one. Choosing from the ambiguity list is a separate, explicit user action.
5.  **Stale analysis.** Navigation uses the scope graph of the parse session for the current buffer. If the buffer changed after that session was created, the tool refuses (`refuse:stale`) or waits for the next analysis. It never maps a stale `declRange` onto newer text.
6.  **PROTO scopes.** Resolution respects PROTO boundaries exactly as the scope graph does (`DEF_NOT_VISIBLE_ACROSS_PROTO_BOUNDARY` is a refusal, not a cross-scope jump).
7.  **External definitions.** Go-to-definition on an `EXTERNPROTO` instance stops at the `EXTERNPROTO` declaration in this document. It does not open or fetch the external file and reports "Defined externally."

## 4. Source Patches and Undo

Rename is out of scope, but structural selection and later steps may produce targeted edits (for example, deleting a selected node span).

1.  **Span-anchored edits only.** Edits are WD1.2 edit sets (`src/vrml/edit.js`) against the exact text of a parse session. Text outside each edit span is never touched.
2.  **Stale-span guard.** Before dispatch, the planner checks that the current buffer equals the session text (the same gate as `documentGate` in `src/vrml/structure-edit.js`). It then builds a receipt with `verifyTransaction({ oldText, edits, newText })`. The editor dispatch re-checks the buffer (`applyVerifiedEdits` refuses with `buffer-changed`). On any mismatch the edit is refused and nothing is applied; the user can retry after the next analysis.
3.  **Minimal patches.** Each edit is the smallest replacement that achieves the action. Surrounding whitespace, comments, and the document's LF or CRLF convention are preserved. Invalid, overlapping, or ambiguous spans are refused by the edit algebra and the whole set is rejected — no partial application.
4.  **One action, one undo step.** Each user-visible action is dispatched through `applyVerifiedEdits` as one CodeMirror transaction with `isolateHistory`, so one Undo restores the exact previous text and one Redo reapplies it. A proposed new `userEvent` value for source tools (for example `input.source`) needs owner approval because that list is closed today.
5.  **UI state is not document state.** Selection, ambiguity lists, and palette state are not written to the source, the analysis, or the CodeMirror undo history. Pure selection changes do not create undo steps.

## 5. Structural Selection

Structural selection changes the selection only. It does not edit text.

1.  **Expansion order.** From the cursor or current selection, each "expand" step selects the smallest enclosing `src/vrml` parse range that strictly contains the current selection, in this order:
    1.  **Value** — the token or value range (number, string, `TRUE`, a single node, a `[ … ]` list).
    2.  **Field** — field name plus its value.
    3.  **Node** — the node including its `DEF name` prefix, or the `USE name` statement.
    4.  **Parent node** — the next enclosing node, then repeat steps 2–4 upward.
    "Shrink" walks the same stack back down. It never invents a range that did not appear on the expansion stack.
2.  **Strings.** A string literal is one value. Expansion inside a string first selects the whole string, including quotes. It never selects part of a string and part of the surrounding source. Text inside a string is not parsed further (§6).
3.  **Comments.** A cursor inside a `#` comment selects the comment. The next step expands to the enclosing node. A comment is never split. Comments adjacent to a node are not added to the node range.
4.  **Root and whitespace.** At the top-level scene, expansion stops; another expand step leaves the selection unchanged. On whitespace between statements, the first step selects the nearest enclosing node, or does nothing at the root.
5.  **Incomplete source.** If the parse is truncated, depth-capped, has blocking syntax errors, or the enclosing node is marked `incomplete`, structural selection does not expand past the last complete range. It shows "Document structure incomplete." It never guesses a closing brace.
6.  **Units.** All ranges use UTF-16 code-unit offsets from the tokenizer, the same units CodeMirror uses, so non-ASCII and non-BMP text select correctly.

## 6. Script Strings

`src/vrml` tokenizes each `url` entry of a Script node as one VRML string token. This lane keeps that as the only structure for Script content.

1.  **`javascript:` strings** — Treated as one VRML string value. Highlighting stays the existing string highlight. Navigation inside the string does nothing. Structural selection follows §5.2.
2.  **`vrmlscript:` strings** — Same behavior as `javascript:`.
3.  **Other or nested syntax** (`ecmascript:`, CDATA-style content, escaped quotes, embedded VRML-like text) — Same behavior. The content is opaque. No tool interprets it.
4.  **Incomplete Script blocks** (an unterminated string, or a Script node with parse errors) — Handled by §5.5 and §3 (`refuse:incomplete`). Nothing inside the string is highlighted structurally.
5.  **Selection across string boundaries** — Never produced (§5.2).
6.  **Fallback.** Because the above is "opaque string" in every case, no nested-language parser is required. Any future JavaScript-aware assistance needs a separate owner decision on whether a second grammar or a new runtime dependency is acceptable. It is not part of this lane.

## 7. Command Palette

1.  A modal list of commands from `src/editor/command-registry.js`. It shows only registered commands and runs them through the registry's own `run` reference.
2.  Disabled commands (registry `enabled` is false) are shown as disabled and cannot run.
3.  Palette state (query text, highlighted row) is UI state only (§4.5).

## 8. Failure Modes (Fail-Closed Cases)

Each case returns a defined result. None of them crashes or produces a wrong answer.

1.  **Parse incomplete or recovered** — Navigation returns `refuse:incomplete`. Structural selection stops at the last complete range.
2.  **Ambiguous resolution** — `ambiguous` with a candidate list. No silent jump.
3.  **Invalid or unresolved reference** — `refuse:invalid` or `refuse:unresolved`. The cursor does not move.
4.  **Stale session** — `refuse:stale`. Edits refuse with `buffer-changed`.
5.  **External definition** — "Defined externally." No file access or fetching.
6.  **Root or whitespace selection** — Expansion stops with no change.
7.  **Read-only editor** — Edits refuse with `editor-read-only`. Navigation and selection still work.

## 9. Proposed Implementation Order

Each step requires its own owner GO.

1.  Read-only symbol navigation (symbol list from the scope graph).
2.  Go-to-definition and find references (§3).
3.  Structural selection (§5).
4.  Command palette integration (§7).
5.  Script syntax assistance — separately approved; see §6.6.

## 10. Test Plan

**Independent QA is required for this lane.**

**Focused tests (Node, `node:test`):**
1.  **Resolution mapping:** One fixture per row of the §3.3 table — resolved USE; duplicate DEF in one scope (`ambiguous`, cursor unchanged, no candidate preselected); missing name; self-referential USE outside Script (`refuse:invalid`); USE inside a Script referencing its own DEF (`navigate`); USE before DEF; DEF behind a PROTO boundary (`refuse:unresolved`); recovered document (`refuse:incomplete`); built-in node type (`refuse:builtin`); an unknown status value (`refuse:unknown`).
2.  **Stale session:** Navigation and edits computed on one session refuse after an intervening edit to the buffer.
3.  **Structural selection:** At least five nesting fixtures (value → field → node → parent node) with exact expected spans; a cursor in a string; a cursor in a comment; root and whitespace; a truncated document; non-BMP characters before the target span.
4.  **Script strings:** `javascript:` and `vrmlscript:` URLs, an unterminated Script string, and an escaped quote inside a string — selection never crosses the string boundary and no structural highlights appear inside the string.
5.  **Command registry:** The palette provider returns only registry commands and respects `enabled`.
6.  **Edits and undo:** A verified edit preserves all text outside its span (byte-exact, LF and CRLF fixtures); an overlapping or invalid edit set is rejected with no change; one action adds exactly one undo step (`historyDepth()`), and undo restores the exact previous text.

**Visual QA (Electron, Linux first, then macOS 15+):**
1.  **Palette:** Opens, filters by typing, runs a command with the keyboard.
2.  **Navigation:** Go-to-definition on a `USE` selects and scrolls to the exact `DEF` name span.
3.  **Ambiguity:** Go-to-definition on an ambiguous `USE` shows the candidate list; the cursor does not move.
4.  **Refusal:** An invalid or unresolved `USE` shows the refusal message; the cursor does not move.
5.  **Script string:** A Script with a `javascript:` URL keeps the normal string styling, and structural selection selects the whole string.

## 11. Open Owner Decisions

1.  Approve the result names and messages in §3.3, or replace them.
2.  Approve a new `userEvent` value for source-tool edits (§4.4), or reuse `input.model`.
3.  Whether any JavaScript-aware Script assistance (second grammar or new dependency) should ever be considered (§6.6).
4.  Whether structural selection should include leading comments attached to a node (§5.3 currently excludes them).
5.  Owner review of this design; independent QA not yet performed.
