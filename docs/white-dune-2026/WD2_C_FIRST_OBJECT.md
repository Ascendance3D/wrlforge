# WD2-C — First Object (as built)

**Lane:** WD2-C · **Status:** **IMPLEMENTED — AWAITING OWNER REVIEW / INDEPENDENT QA.**
Not closed. Uncommitted candidate on `feature/wd2-c-first-object`, built on the
uncommitted WD2-B candidate (reproduced byte-for-byte from the frozen
`wd2-b-typed-inspector` worktree) over `origin/main` `4e8fd35`.

## 1. Goal

The first lane judged by **beginner usability**: someone who does not know
VRML can create a Box, move, rotate, resize and colour it, duplicate it, delete
it, undo any of that, add a Sphere and save a valid VRML97 file — without
opening Source. An expert can open Source at any moment and see exactly the
text each action wrote, and nothing else changed.

Contract inputs (research only, nothing copied): the Sunrize + Spazz3D audit
(`.worktrees/wrlforge/sunrize-spazz3d-audit/docs/visual-authoring/`), in
particular `SPAZZ3D_BEGINNER_UX_CONTRACT.md`. **No external source was opened
for reuse, copied, adapted or ported in this lane**; no provenance entry is
required.

## 2. Architecture (unchanged spine)

```
visual action (Add / Duplicate / Delete / property Apply / colour pick)
 → exact source edit plan          src/vrml/structure-edit.js | field-edit.js
 → verified transaction            WD1.2 validateEdits/applyEdits + WD1.4 verifyTransaction
 → ONE CodeMirror transaction      editor-view applyVerifiedEdits (isolateHistory 'full', userEvent 'input.model')
 → reparse                         the normal analysis (reanalyzeNow)
 → selection                       inserted node (resolveInsertedNode) | cleared (Delete) | WD1.4 Tier 1
 → X_ITE preview                   the existing Phase 7C unsaved-buffer onEdit path
 → normal Save / Undo / Redo
```

No serializer, no X_ITE scene serialization, no second buffer, no second
undo stack, no second selection store, no second schema, no new preview
channel, no direct X_ITE mutation.

### Modules

| module | responsibility |
|---|---|
| `src/vrml/node-templates.js` | Text of a **new** anonymous simple object. Requires only the schema; never reads an AST or existing text (not a serializer). |
| `src/vrml/structure-edit.js` | Exact-span planners: `planInsertObject`, `planDuplicateNode`, `planDeleteNode`, `planFieldInsert`, plus `resolveInsertedNode`. Pure. |
| `src/vrml/simple-object.js` | Beginner facade: recognises `Transform → Shape → (Appearance → Material) + Box/Sphere` and maps Position/Rotation/Size\|Radius/Color to the real nodes + schema fields. A view, no state. |
| `src/vrml/field-edit.js` (WD2-B, extended) | Now also exports `encodeFieldValue` (the one value validator/encoder, reused for absent fields), `nodeEditGate`, `hasBlockingSyntaxError`. |
| `src/editor/first-object.js` | Renderer glue (pure): `prepare{Add,Duplicate,Delete,PropertySet}`, `objectForItem`, `displayLabels`, `selectInserted`, `refusalText`. Published on `WRLForgeSceneBridge.firstObject`. |
| `renderer/model-workspace.js` | DOM binding: Model bar + Object panel. Computes no offset, builds no VRML. |
| `src/settings/preferences.js` | `workspaceMode` (`code` default \| `model`) added to the existing preferences model (same storage, same `WrlPreferences`). |
| `src/editor/ui-state.js` | `isEmptyScene` / `initialWorkspaceMode` (pure, text-level). |

## 3. Generated VRML

```vrml
#VRML V2.0 utf8

Transform {
  children [
    Shape {
      appearance Appearance {
        material Material {
        }
      }
      geometry Box {
      }
    }
  ]
}
```

Sphere is identical with `geometry Sphere`. Two-space indentation, the
document's own line ending, no trailing/leading line ending in the template.

- **No automatic DEF**, no `WorldInfo`/metadata/comment markers, no proprietary
  fields: generated objects are anonymous. Editor identity comes from the
  verified insertion receipt, never from names written into the user's file.
- **No default-valued fields.** `translation`, `rotation`, `size`, `radius`,
  `diffuseColor` are authored only when the user changes them.
- Every node and field is checked against the WD1.3 schema at module load
  (`isVRML97Node`, `isFieldAllowed(..., 'vrml97')`).
- **Origin placement**: new objects sit at `0 0 0` (no click-to-place, no
  projected placement). Status: *"Box created at origin."*

## 4. The structural-edit contract

Every planner first runs the document gate (session is the editor's exact
current text; no parse cap; **no blocking syntax error** — WD.md §8 rule 3),
then builds WD1.2 edits, and returns a plan only after **all** of:

1. `validateEdits` + `applyEdits` derive the new text;
2. `verifyTransaction` proves the edit set is exactly old → new;
3. the new text reparses with no blocking syntax error and no cap;
4. **token stream preserved**: the new text's tokens are exactly the old tokens
   before each edit + the insert's own tokens + the old tokens after, and the
   old text splits into whole tokens at every cut — no fusion, split or
   swallowed neighbour;
5. the operation's post-condition (below).

A refusal carries a stable reason id and no text/edits; nothing is mutated
first and checked later.

| action | bytes that may change | post-condition |
|---|---|---|
| **Add** | one insertion at the end of the root scene: separator (a line ending if the file lacks a final one, plus one blank line unless one is already there) + object + line ending. Empty file: one insertion of header + blank line + object. Whitespace-only file: header inserted at offset 0 + object at the end. | exactly one new `Transform` at the planned span, in root context |
| **Position / Rotation / Size / Radius / Color, field authored** | only the changed component tokens (WD2-B `planFieldEdit`) | WD2-B round-trip |
| **… field absent** | one insertion inside the node body: if `{` ends its line, a new line `indent + name value + EOL` right after it (indent of the first non-blank body line, or the closing brace's indent + one unit — tab if it uses tabs, else two spaces); otherwise ` name value` right after `{` (plus a space when the next character is not whitespace) | WD1.4 Tier 1 resolves the same node; it authors the field exactly once, editable, with exactly the intended tokens |
| **Duplicate** | one insertion: the node's **exact source bytes**; on the next line with the same indentation when the node occupies its lines alone, else ` ` + copy right after it on the same line | exactly one new node of the same type at the planned span, in the same kind of parent (root, or the same MFNode field of the same owner type) |
| **Delete** | the node's exact span plus the indentation and line ending of the line(s) it occupies alone (last line without a final newline: the *preceding* line ending instead); inline: the span plus the space run after it when a space precedes it | reparses clean; token stream preserved |

The value of an absent field is validated and spelled by `encodeFieldValue`
(the WD2-B encoders and schema bounds; the user's spelling is written verbatim,
nothing is clamped). A value equal to the schema default is `unchanged` —
nothing to author.

### Ownership and refusals (`STRUCTURE_REASON`)

Duplicate / Delete apply only to a node that is a **root statement** or an item
of a **schema-proven MFNode field** of a built-in, non-PROTO-named,
singly-authored owner field. Comments outside the owned span are never touched;
comments inside the span belong to the node (copied by Duplicate, removed by
Delete).

| reason | when |
|---|---|
| `document-has-syntax-errors` | any blocking syntax error (all actions) |
| `document-has-no-vrml97-header` / `document-header-not-vrml97` | Add into headerless content / a non-`#VRML V2.0 utf8` header — never silently repaired |
| `node-is-inside-a-proto` | any structural edit or absent-field insert in a PROTO body |
| `node-is-not-in-a-node-list` | an SFNode value (`geometry Box {}`), an un-bracketed MF single value, an interface default |
| `parent-field-not-a-provable-mfnode` | owner is a PROTO instance / vendor node / duplicated field / `IS`-bound |
| `contains-def-name` (names listed) | Duplicate of a span holding any `DEF` — no automatic renaming |
| `contains-use` | Duplicate of a span holding a `USE` |
| `contains-route-or-proto` | Duplicate of a span holding `ROUTE` / `PROTO` / `EXTERNPROTO` |
| `node-is-referenced` (names listed) | Delete when a `USE` or `ROUTE` **outside** the span names a `DEF` inside it (name-based and document-wide on purpose: conservative across scopes and duplicate names) |
| `field-already-authored` | absent-field insert on an authored field (planPropertySet routes to `planFieldEdit` instead) |
| `surrounding-tokens-changed`, `round-trip-verification-failed`, `transaction-rejected` | a verification step failed — shown as *"…its exact source location cannot be proved."* |

Every reason has one plain sentence (`first-object.js REFUSAL_TEXT`, test
asserts coverage), e.g. *"Cannot duplicate this object because it contains a
DEF name that would conflict (Lamp)."*, *"Cannot delete this object because
other nodes reference it (Lamp)."*

### Insertion location

**Root-only in WD2-C.** The end of a syntax-error-free document is always root
scope, so the target never has to be guessed; insertion into a selected
container is deferred. Never inside a USE, a damaged document, or a PROTO body.

## 5. Identity and selection

- **After Add / Duplicate**: `resolveInsertedNode` — resolved only when the
  receipt was minted by `verifyTransaction`, is bound to the analysed text, the
  planned span lies **wholly inside text the receipt inserted**, and exactly one
  node of the expected type has exactly that range. Never "the newest Box",
  never a name, never a position guess. Unproven → selection cleared with a
  notice.
- **After Delete**: the selection (the deleted node) is cleared.
- **Everything else** (property edits, typing, Undo, Redo): the WD2-B
  re-anchor through WD1.4 Tier 1 — unchanged.
- **Display label** "Box"/"Sphere" for a recognised object in the scene tree,
  with the real `Transform` label kept beside it. Display only; never written,
  never identity.

Evidence: twins (delete first / delete second / duplicate second), insertion
before/after identical siblings and next to a byte-identical generated object,
lost identity clears; adversarial sweep over 25 nodes × {Duplicate, Delete,
absent-field insert}: 22 ready plans, 53 stable refusals, **509 proven
re-anchors checked against an independent byte oracle, 30 lost, 0 wrong**.

## 6. Beginner property model

| object | label | node · field · type | default shown |
|---|---|---|---|
| Box | Position | Transform · `translation` · SFVec3f | 0 0 0 |
| Box | Rotation | Transform · `rotation` · SFRotation (axis X/Y/Z + angle in radians) | 0 0 1 0 |
| Box | Size | Box · `size` · SFVec3f (Width/Height/Depth, each > 0) | 2 2 2 |
| Box | Color | Material · `diffuseColor` · SFColor (native colour picker) | 0.8 0.8 0.8 |
| Sphere | Position, Rotation | as Box | |
| Sphere | **Radius** | Sphere · `radius` · SFFloat (> 0) — not "Size" | 1 |
| Sphere | Color | as Box | |

Values are read from the same `inspectNodeFields` descriptors the technical
Inspector shows (they cannot disagree); absent fields show the schema's
`defaultText`. The technical name + type is always visible as secondary text
(`translation · SFVec3f`). Rotation stays VRML axis-angle (no Euler subsystem);
the panel says *"A quarter turn is 1.5708."* Colour: `<input type="color">`
commits on `change` only (never on every `input` while dragging), as 8-bit
channels spelled with ≤ 3 decimals (`#ff8000` → `1 0.502 0`; every 8-bit value
round-trips). Colour is read-only (stated) when the object has no Material or a
`USE`d one.

## 7. Model workspace

- **New/empty document** (every line blank or a comment: empty, whitespace-only,
  header-only) opens in **Model** — not persisted.
- **Existing document** opens in the **remembered** mode (`workspaceMode`,
  default `code`, i.e. the pre-WD2-C behaviour). Clicking Model/Code persists
  it through the existing `WrlPreferences`.
- **Model layout**: the X_ITE preview is the work area; Add / Duplicate /
  Delete / "Selected: …" sit in the always-visible Model bar; the sidebar keeps
  Object panel, scene tree, Inspector, diagnostics; the Outline (a source
  navigation aid) is hidden in Model only. **Source** is collapsed, one click
  away (Show/Hide Source, `aria-pressed`, `aria-controls`); it is the same
  CodeMirror view, never a second document. Model also forces the preview
  visible when the remembered preview layout is "Editor only".
- **Damaged document in Model**: Source opens automatically once per damaged
  state and the status says, in words, that visual editing is paused until the
  syntax errors are fixed; every visual action refuses.
- **Code mode**: unchanged layout, plus the Model bar and Object panel.

## 8. Undo / Redo

One visible action = one `view.dispatch` with `isolateHistory.of('full')`
(`userEvent 'input.model'`) = one CodeMirror undo entry — including the
two-edit header + object Add into a whitespace-only file and three-component
Size applies. Real-history tests: Add, every property (insert and patch),
Duplicate, Delete each +1 `undoDepth`; one Undo restores the exact prior text,
one Redo the exact edited text; typing before/after an Add stays a separate
entry. Real Electron confirms the same depths through the toolbar buttons.

## 9. Live preview evidence (real Electron, `qa/wd2-c-first-object/RESULTS.json`)

Read-only walk of the live X_ITE scene after each step: Box created (1
Transform, Box size 2 2 2 at origin); Position → translation 3 0 0 and world
bounds X ∈ [2, 4]; Size → Box 2 1 0.5; Color → Material 1 0 0; Duplicate → 2
Boxes; Undo → 1; Redo → 2; Delete → 1; Undo → 2; Sphere radius 2. Screenshots
`01`–`14`.

## 10. Beginner walkthrough (measured)

| step | actions (no Source, no VRML terms needed) |
|---|---|
| Add Box | 1 click (Add **Box**); the Box is selected, Object panel open |
| Position | click X (value pre-selected), type `3`, Enter |
| Size | click Width, type, Tab, type, Tab, type, Enter |
| Rotation | click Angle, type `1.5708`, Enter |
| Color | click the swatch, pick a colour, close the picker |
| Duplicate | 1 click; the copy is selected |
| Delete | 1 click |
| Save | 1 click (or Ctrl+S) |

Friction removed during the lane: numeric fields select their value on focus
(clicking "0" and typing "3" gives 3, not "03"); the stale "created at origin"
line clears once the panel reports "Applied.".

## 11. Accessibility

All controls are native buttons/inputs reachable by Tab. Add buttons are named
"Add Box"/"Add Sphere" (visible text included); Model/Code and Show Source carry
`aria-pressed`; each property is a `role="group"` labelled by its beginner name;
each input is labelled by property + component (`Position X`) and described by
the technical type and its status line (`role="status"`, `aria-live`). Enter
commits, Escape restores, invalid input sets `aria-invalid` with an
"Invalid: …" sentence. Status and refusals are text, never colour alone. Sizes
are `rem`, so `--wrl-ui-scale` zoom applies; High Contrast + 140 % capture
`14-contrast-zoom-model.png`. Keyboard focus starts on Add Box in a new
document.

## 12. Security

Renderer-only. No new IPC channel, no path in any renderer message, no fs in
the renderer, no CSP / `contextIsolation` / `nodeIntegration` / preload change,
no network, no new dependency. `main.js` is untouched by WD2-C. New
`__wrlEditor` QA hooks drive the page's own DOM and read the live X_ITE scene
read-only; no buffer text is exposed. The QA orchestrator runs Electron with an
isolated `--user-data-dir` under the OS temp dir.

## 13. Performance

Planner median times (Node, generated objects with an authored translation):

| objects (bytes) | analyse | planFieldEdit | planFieldInsert | planInsertObject | planDuplicate | planDelete |
|---|---|---|---|---|---|---|
| 10 (1.8 KB) | 0.34 ms | 0.24 | 0.64 | 0.70 | 0.36 | 0.29 |
| 100 (17 KB) | 2.3 | 1.2 | 5.5 | 6.3 | 3.3 | 2.9 |
| 1000 (174 KB) | 16 | 11 | 53 | 51 | 50 | 49 |

A structural plan costs ≈ 3× one analysis (round-trip reparse + two-sided
token-stream proof), once per click — never per keystroke, never per colour
drag. No change to the typing path.

## 14. Known separate issues (not fixed here)

- **CRLF**: the native CodeMirror buffer normalises CRLF to LF on load/save
  (pre-existing, recorded in WD2-B). The WD2-C edit model is line-ending aware
  and tested on CRLF text at the model level; it does not make the editor issue
  worse.
- No "New document" command exists; a new document is an empty `.wrl` opened
  through the existing Open flows (a File ▸ New needs its own main-process
  save-target design).

## 15. Deferred (not implemented)

Viewport picking / click-to-select (WD2-C0/WD2-D), click-to-place, transform
gizmos / snapping / pivots (WD2-E), hierarchy drag-and-drop / group / ungroup /
reparenting / multi-selection (WD2-F), insertion into a selected container,
material presets / swatches, textures, ROUTE graph, animation, PROTO editing,
Cylinder/Cone/Text primitives, automatic DEF naming or renaming, a "Name" field.

## 16. Tests

| file | tests | what |
|---|---|---|
| `test/vrml/structure-edit.test.js` | 33 | creation (empty / whitespace / header-only / existing / no final newline / CRLF / ROUTE tail / refusals), inserted-node identity + forgery, absent fields (all five, spelling, defaults, bounds, single-line / comment / tab / CRLF / comma bodies, twins), Duplicate + Delete (exact bytes, comments, inline, EOF, CRLF, every refusal), twins, adversarial sweep (0 wrong), purity, determinism |
| `test/vrml/node-templates.test.js` | 8 | VRML97 validity, no DEF/metadata/defaults, determinism, CRLF, error codes, not-a-serializer scan |
| `test/vrml/simple-object.test.js` | 9 | Box/Sphere mappings, Inspector agreement, authored vs absent routing, missing/shared Material, strict recognition, damaged doc, architecture |
| `test/editor/first-object.test.js` | 11 | real CodeMirror history: one-action-one-undo, exact Undo/Redo, selection after Add/Duplicate/Delete, twins, typing isolation, refusal text |
| `test/renderer/model-workspace-runtime.test.js` | 9 | DOM: bar states, text status, labels/ARIA, Enter/Escape/invalid, colour commit on change only, read-only reasons, no re-render wipe, select-on-focus, colour round-trip |
| `test/editor/ui-state.test.js` (+2), `test/settings/preferences.test.js` (+2) | | workspace-mode rule and preference |
| `qa/wd2-c-first-object/orchestrate.js` (`npm run qa:wd2c`) | 60 assertions | real Electron (sections 8–10) |
