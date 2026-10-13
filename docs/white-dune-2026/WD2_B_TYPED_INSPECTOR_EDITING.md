# WD2-B — Typed Inspector Field Editing (as built)

**Lane:** WD2-B · **Status:** **IMPLEMENTED — AWAITING INDEPENDENT QA.**
Not closed. Uncommitted candidate on `feature/wd2-b-typed-inspector-editing`
from `origin/main` `4e8fd35`.

## 1. Scope

The first visual mutation. The WD2-A Inspector becomes write-capable for
**existing, explicitly authored** field values on **built-in VRML97 nodes**,
for nine schema types. Nothing else becomes editable.

```
VISUAL INSPECTOR ACTION
 → VERIFIED SOURCE-TEXT PATCH (WD1.2 + WD1.4 receipt)
 → ONE CODEMIRROR TRANSACTION (same buffer, one history event)
 → REPARSE (the normal analysis)
 → STABLE NODE RE-SELECTION (WD1.4 Tier 1 only)
 → LIVE X_ITE PREVIEW UPDATE (Phase 7C unsaved-buffer path)
 → NORMAL SAVE / UNDO / REDO
```

The canonical document remains **the exact source text**. No AST serializer,
no node or document regeneration, no second buffer, no second parser, schema,
selection store, undo system or preview channel.

## 2. Data flow (production)

```
Inspector DOM (renderer/scene-inspector.js)
  Enter / Apply → deps.applyField(item, field, components)      [typed values only]
renderer/editor.js applyInspectorField
  → WRLForgeSceneBridge.inspectorEdit.prepareInspectorApply      [src/editor/inspector-edit.js]
      → sceneTree.astNodeForItem(tree, itemId)                   [exact AST object, same build]
      → vrml/field-edit.js planFieldEdit
          gates (session, parse, node, schema, field, value shape)
          validate + encode each CHANGED component
          edit.replaceSpan(componentTokenRange, newToken)        [WD1.2]
          edit.validateEdits + edit.applyEdits → newText          [WD1.2]
          documentTransaction.verifyTransaction → receipt         [WD1.4, fail closed]
          round-trip: parse(newText), resolveTransactionAnchor,   [WD1.4 Tier 1]
                      same field index/name, intended tokens
      ← { status:'ready', edits, oldText, newText, receipt } | refused
  → S.handle.applyVerifiedEdits({ oldText, edits, newText })     [editor-view.js]
      refuses unless the buffer is exactly oldText and editable
      ONE view.dispatch: changes = edits, isolateHistory('full'), userEvent 'input.inspector'
      post-condition: doc === newText (else undo + refuse)
      → updateListener: onChange (dirty, recovery snapshot, EP().onEdit → X_ITE)
      → change chain composed (chainChanges)
  → S.pendingApply = { oldText, newText, receipt }
  → S.handle.reanalyzeNow()                                      [the ONE analysis]
      onAnalysis(a: { …, parseResult, text, transaction })
        scope graph → scene tree → findings → presentation       [WD2-A order, unchanged]
        reanchorAfterAnalysis
          createParseSession(a.text, a.parseResult)
          inspectorEdit.reanchorSelection({ previous, next, selectedId,
                                            chain: a.transaction, pendingApply })
            receipt = the Apply receipt if it is exactly prev→next,
                      else verifyTransaction(chain) — else none → LOST
            Node:  createTransactionAnchor(prevSession, astNode)
                   resolveTransactionAnchor(anchor, nextSession, receipt)
                   itemForAstNode(nextTree, resolved.node)
          → setSelection(newId) | clearSelection() + visible notice
        render() → scene tree row + Inspector refresh (focus restored to the field)
```

The Inspector never computes an offset (`test/editor/inspector-edit.test.js`
source-scans it). The analysis is not doubled: `reanalyzeNow()` bumps the
version, so the debounced analysis the dispatch scheduled is dropped as stale.
The F1 order (scope graph → USE resolver → scene tree → findings) is
untouched; WD2-B only appends the re-anchor after it.

## 3. Editable types

Type authority is `src/vrml/node-schema.js` — never the lexical shape.

| type | control | patch unit | validation |
|---|---|---|---|
| SFBool | checkbox | the `TRUE`/`FALSE` token | boolean |
| SFInt32 | 1 input (`inputmode=numeric`) | the scalar token | A.3 `int32` lexeme (dec/hex), integer, 32-bit range (ISO 5.6), schema numeric bounds |
| SFFloat | 1 input | the scalar token | A.3 `float` lexeme, finite, schema numeric bounds |
| SFTime | 1 input | the scalar token | as SFFloat |
| SFVec2f | X Y | each component token independently | as SFFloat, per component |
| SFVec3f | X Y Z | each component token independently | as SFFloat, per component |
| SFColor | R G B | each component token independently | as SFFloat; schema `[0,1]` enforced per component |
| SFRotation | X Y Z Angle | each component token independently | as SFFloat; the schema carries only a `PER_COMPONENT_RANGE` note → nothing enforced |
| SFString | text input | the string token only | `encodeString`: escapes `\` and `"` (the tokenizer's only two escapes), refuses CR, proven by re-tokenizing |

Numeric input is checked against the VRML97 lexical shape **and** the real
tokenizer (exactly one valid NUMBER token spelling itself). Rejected: empty,
`NaN`, `Infinity`, `-Infinity`, `1e999` (non-finite), `1x`, `1 2`, `0x10` in a
float field, fractions in SFInt32. Nothing is clamped. Schema `constraints` are
read exactly as their header requires: numeric `min`/`max` enforced with their
inclusive flags; symbolic bounds (`pi/2`, `infinity`) and `note` categories are
not converted into numbers; `null` is never "unrestricted". The user's own
spelling is written verbatim (`1e3` stays `1e3`); an unchanged component is
not patched.

## 4. Refusal cases (read-only, stable reason ids)

| case | reason |
|---|---|
| Document root, USE, ROUTE, PROTO decl, EXTERNPROTO decl | `not-a-node-instance` |
| node from another parse | `node-not-in-parse-session` |
| session text ≠ editor text (stale selection) | `parse-session-is-stale` |
| parse hit a node/depth cap | `document-parse-incomplete` |
| any syntax error except the header line (VRML001/002) | `document-has-syntax-errors` |
| incomplete node | `node-incomplete` |
| unknown / vendor node type | `node-type-not-standard-vrml97` |
| node type spelled like any PROTO/EXTERNPROTO in the document | `node-type-is-a-proto-name` |
| absent / default field | `field-not-explicitly-authored` (absent fields are not listed) |
| field authored twice on one node | `field-authored-more-than-once` |
| not in the schema | `field-not-in-schema` |
| X3D-only field | `field-is-x3d-only` (no VRML97 type shown) |
| eventIn / eventOut | `field-is-not-a-stored-field` |
| `IS`-bound | `field-is-is-bound` |
| MF*, SFNode, MFNode, SFImage | `field-type-not-editable-in-wd2b` |
| value shape ≠ type (arity, kind), or no exact span | `field-value-shape-does-not-match-type` |
| lexically invalid token / hex in a float field | `field-value-token-invalid` |
| transaction verification failure | `transaction-rejected` |
| patch that does not round-trip (e.g. `1-2` → `12`) | `round-trip-verification-failed` |

A syntax error anywhere blocks the whole document because parser recovery
moves boundaries (WD.md §8 rule 3): a damaged document can attribute a field
to the wrong node. Refusal never produces edits or text.

## 5. Transaction gate

`planFieldEdit` captures `session.text` (which must equal the editor's current
text), builds the WD1.2 edits, derives `newText` with `applyEdits`, and
verifies `{oldText, edits, newText}` with `verifyTransaction`. A rejection
returns `transaction-rejected` and nothing is dispatched. The receipt is kept
(`S.pendingApply`) and is the one used to re-anchor when the next analysis is
exactly that transaction. `applyVerifiedEdits` additionally refuses a buffer
that moved on (`buffer-changed`) or a read-only editor.

## 6. Node identity / re-selection

Only WD1.4: a Tier 1 transaction anchor resolved through a verified receipt.
For any other change (typing, Undo, Redo, QA `setText`), the editor view hands
the analysis the CodeMirror change sets composed since the previous analysis
(`chainBase` + `chainChanges`, converted by `inspectorEdit.changesToEdits`);
the renderer **verifies** them before use. `setDoc` (reload/open/recovery)
breaks the chain → nothing provable → selection lost.

- Node items: Tier 1 (containment guard, exact mapped span, type/DEF/parent/
  field match). `ambiguous` is treated as lost.
- Document: unique by construction → the new root.
- USE / ROUTE / PROTO / EXTERNPROTO: kept only when the text is byte-identical;
  otherwise lost (WD1.4 covers node instances only).

**Behaviour change vs WD2-A:** WD2-A kept a selection whenever an item with the
same `kind-start-end` id existed after a reparse — offset retention, which
could confidently select a different node after a whole-document replacement.
That rule is removed; every analysis now re-anchors through identity or loses
the selection with the visible notice *"Selection cleared: the previously
selected item could not be proven to be the same item after this change."*

Evidence: twin-sibling tests (second stays second, first stays first, in pure
tests **and** in real Electron), a sweep applying every editable field of every
node of a nested/DEF/PROTO document (≥ 8 proven re-anchors, **0 wrong**),
boundary-crossing replacement → lost, reload → lost, forged chain → lost.

## 7. CodeMirror undo / redo

One Apply = one `view.dispatch` carrying all component changes with
`isolateHistory.of('full')`, so it neither joins adjacent typing nor absorbs
later typing. Tested with real `@codemirror/state` + `@codemirror/commands`:
`undoDepth` 1 per Apply; one Undo restores the exact pre-edit text, one Redo
the exact post-edit text; a multi-component Apply is reversed by one Undo.
The toolbar Undo/Redo buttons call the same commands (`handle.undo/redo`).
No semantic undo stack exists.

## 8. Live preview

No new channel. The dispatch fires the existing `onChange` → `EP().onEdit()` →
Phase 7C debounced unsaved-buffer refresh. Real-Electron evidence
(`qa/wd2-b-typed-inspector/RESULTS.json`): the X_ITE authoritative world
bounds (`computeSceneBBox` over the live X_ITE scene) of the primary fixture
are X ∈ [−1, 1] before, **[2, 4] after** Apply X = 3, back to [−1, 1] after
Undo, [2, 4] after Redo; screenshots `01`–`04`. Camera/viewpoint behaviour is
the existing 7C path, unchanged.

## 9. Losslessness evidence

`test/vrml/field-edit.test.js` compares old and new text in lockstep over the
canonical edits for every success case (every byte outside the spans equal,
each span exactly its insert), covering A comments between components,
B unusual whitespace, C commas, D a CRLF document (CRLF count preserved, no
bare LF), E exponent spellings, F comments around the field, G hyphenated DEF
+ ROUTE-in-MFNode + vendor node elsewhere, H several SFVec3f fields on one
node, I identical twins, J nested Transform, K DEF node, L built-in node in a
PROTO body (and its IS-bound sibling refused).

Note: the CodeMirror buffer itself normalises CRLF to LF on load (pre-existing
behaviour of the native editor, unrelated to WD2-B; see Findings in the QA
handoff). WD2-B patches are byte-lossless relative to the buffer they edit.

## 10. White Dune influence and provenance

UX concepts only, from the already-recorded audit
(`WD_OSS_A1_IMPLEMENTATION_ARCHITECTURE_AUDIT.md`, matrix rows FI-01..FI-04):
one selected node, its typed fields together, per-type controls next to the
field name. WRL Forge uses generic schema-driven controls rather than per-field
dialog classes. **No White Dune source code was opened, copied, adapted,
translated or ported in this lane**; no provenance entry is required.

## 11. Accessibility

Each field is a `role="group"` labelled by its name + type. Each input has a
`<label for>` (X/Y/Z, R/G/B, X/Y/Z/Angle, Value, TRUE/FALSE) and an accessible
name of *field + component + type* (e.g. "translation X SFVec3f") via
`aria-labelledby` of visible text, and `aria-describedby` → its message
region (`role="status"`, `aria-live="polite"`). Keyboard: Tab reaches every
control; **Enter** in any control (or on Apply) commits the whole field;
**Escape** restores the document value and keeps focus; Apply/Cancel are
buttons. Invalid input sets `aria-invalid="true"` on the offending component,
writes "Invalid: …", moves focus there and keeps the typed text. State is
always text ("editable", "read-only", "Invalid:", "Applied.", "No change."),
never colour alone. Sizes are `rem`, so `--wrl-ui-scale` zoom applies;
High Contrast + 140 % capture `10-contrast-zoom-inspector.png`. The WD2-A tree
(roving tabindex) is untouched. Re-renders with an unchanged tree/findings are
skipped, so status refreshes never wipe typed text; after Apply, focus returns
to the edited component.

## 12. Security

Renderer-only text editing over the already-authorized session. No new IPC
channel, no path in any renderer message, no fs in the renderer, no CSP /
`contextIsolation` / `nodeIntegration` change, no network. `main.js` changes
are confined to the `WRL_FORGE_CAPTURE_SERVER` QA harness (a `steps` list that
calls existing `window.__wrlEditor` hooks by validated name, and optional
console capture). New `__wrlEditor` QA hooks drive the page's own DOM;
`bufferEquals` answers yes/no so no buffer text is exposed.

## 13. Explicit non-goals (unchanged, deferred)

Node insertion/deletion/duplication/reparenting, drag-and-drop/reorder, DEF
rename, USE retarget, field insertion/deletion, MF array editing, SFNode/MFNode
editing, PROTO/EXTERNPROTO/IS editing, ROUTE editing, animation/keyframes,
viewport picking, gizmos, colour picker, texture chooser, Extrusion/IFS
authoring, auto-fix, save-time repair, formatting, AST serializer.

## 14. Tests

| file | tests | what |
|---|---|---|
| `test/vrml/field-edit.test.js` | 48 | 9-type matrix (schema-asserted), A–L losslessness, full refusal matrix, input validation, encoder round-trip, determinism, purity/no-serializer scans, facade surface |
| `test/editor/inspector-edit.test.js` | 16 | real CodeMirror state/history: primary flow, length-changing re-anchor, multi-component = one undo, history isolation, twins, zero-wrong sweep, PROTO body, boundary loss, reload loss, forged chain, non-node kinds, editor-boundary refusals, change-chain verification, architecture scans |
| `test/renderer/editor-wd2b-runtime.test.js` | 8 | Inspector DOM: typed controls + label/ARIA wiring, Enter commit + focus + "Applied.", invalid input, Escape/Cancel, bool/string controls, no wipe on same-tree re-render, read-only kinds/documents, lost-selection notice |
| `test/vrml/scene-tree-links.test.js` | 3 | item ↔ AST links by object identity; cross-parse null; F5/C3 + item key set preserved |
| `qa/wd2-b-typed-inspector/orchestrate.js` | 32 assertions | real Electron (see §8, §15) |

## 15. Files changed

Added: `src/vrml/field-edit.js`, `src/editor/inspector-edit.js`, the four test
files above, `qa/wd2-b-typed-inspector/orchestrate.js` (+ `RESULTS.json`,
`screenshots/`), this document.

Modified: `src/vrml/scene-tree.js` (private item↔AST link table +
`astNodeForItem`/`itemForAstNode`), `src/vrml/index.js` (`vrml.fieldEdit`,
scene-tree links on the facade), `src/editor/browser/editor-view.js` (change
chain, `text`/`transaction` on analysis, `applyVerifiedEdits`, `isReadOnly`,
bridge entries), `renderer/editor.js` (re-anchoring, Apply wiring, QA hooks),
`renderer/scene-inspector.js` (Fields section), `renderer/editor.html` (CSS),
`main.js` (capture-server `steps` + console capture), `package.json`
(`qa:wd2b`), `docs/WRL_FORGE_ROADMAP.md`, `CLAUDE.md`, `WD.md` (status rows).

## 16. QA handoff

- `npm test` / `npm run check` (set `WRL_FORGE_ISO_MIRROR` to un-skip the four
  maintainer-only schema regeneration tests in a worktree).
- `npm run qa:wd2b` (needs a display) → 32/32, 0 console errors/warnings.
- Regression: `qa:mall-preview`, `qa:world-preview`, `qa:vision`, and the
  `test/visual/*.test.js` files run one at a time.
- Suggested probes: the twin and sweep tests; `Transform { translation 1-2 3 }`
  (round-trip refusal); typing in the code editor while a node is selected
  (verified chain keeps it); Reload (selection lost).
