# VISUAL-4A — Source-Safe Extrusion Authoring

Lane: `feature/visual-4a-extrusion`, stacked on PR #134
(`feature/texture-local-1`, head `9d5f389fab1ec552f420b24ada088e0e6ddfe3a9`).
Application: the Tauri desktop app (`apps/desktop-tauri`). The Electron app
is unchanged.

## Feature contract

| capability | where | contract |
|---|---|---|
| Create Extrusion | toolbar **Create ▾ → Extrusion** | `wrlforge_vrml::create` gains `Primitive::Extrusion`. It uses the existing VISUAL-1 planner and appends `DEF Extrusion_<n> Transform { … Shape { appearance … geometry Extrusion { crossSection … spine … } } }` at top level as one pure insert (one undo step). The outline is a closed, convex hexagon of radius 1 with the ISO default winding. The spine runs from `0 -1 0` to `0 1 0`. No other Extrusion field is written. Box, Sphere, Cylinder and Cone are unchanged. |
| Extrusion Inspector | select an `Extrusion` node | Five sections: cross-section points, spine points, per-point scale, per-point orientation (axis + angle, with a display-only degree column), and caps and geometry flags. Each field shows "authored", or "default … (not written in the source)" with the VRML97 default value. Notes report value counts, closure, winding (x right, z down), unit-length axes and unreadable values. |
| List editing | Inspector tables (any node) | `MFVec2f`, `MFVec3f` and `MFRotation` fields are editable in every node: per-row Apply, Insert after (+), Delete (×), Add at end. Each button sends one operation, which is one undo step. |
| Write a missing field | "Write field with these values", "Write" | An explicit user action adds ONE absent field (list, SFBool or SFFloat) as the node's last statement (`mf_edit::plan_add_field`). Defaults are never written automatically. |
| Outline editor | Inspector **Edit outline…** | A non-modal 2D editor with a grid and axis labels (`+x →`, `+z ↓`). It can select, drag (optionally snapped), set exact x/z, add a point, insert between two points (by button or by a click on a segment), delete, and use the keyboard (arrow keys, Delete). It works on a copy. **Apply** sends the whole operation log as one request, so one undo step. **Cancel** sends nothing. |
| Preview | X_ITE | Unchanged path: every committed edit is a new revision, loaded from the current source (no 700 ms wait for list edits). The last valid scene, generation retirement, picking proof and camera carry from VISUAL-2 / 3A1 apply unchanged. Picking an Extrusion object selects its Transform (`pick::PRIMITIVES` now includes `Extrusion`). |

Rust owns every value. The UI sends raw text, a field index, a tuple count,
the session, the revision and the item id. It never parses, rounds or
validates a number for the document.

## Source safety rules (`crates/wrlforge-vrml/src/mf_edit.rs`)

* **Identity.** The node is named by its exact UTF-16 span in ONE revision. The
  `field_edit::node_gate` gate is reused: exactly one node at that span, no
  blocking syntax error, no parser cap, a standard VRML97 node, not a PROTO
  instance, not inside a PROTO / EXTERNPROTO / interface scope. The field must
  be at the given index with the given name, authored once, not IS-bound, a
  VRML97 `field` / `exposedField` of a list type. The tuple count the UI saw
  must match (`list-count-changed`). The service refuses a stale revision
  before planning.
* **Accepted forms.** `[ ]`, `[ a b, c d … ]` (count is a multiple of the
  tuple size) and one bare tuple. Other forms are read-only with a reason
  (`list-form-not-supported`, `field-value-shape-does-not-match-type`,
  `field-value-token-invalid`).
* **Minimal edits.** A Set replaces only the tokens whose lexeme changes. An
  Insert adds one tuple next to an existing tuple and copies a clean
  separator from the list (whitespace and commas only, so line endings and
  indentation follow the list). A Delete removes the tuple and ONE clean
  neighboring separator. A comment is never removed: if both neighbors hold
  comments, only the tokens go. A tuple with a comment inside it refuses
  deletion. A bare value gets brackets inserted around its untouched tokens.
* **Values.** Each component goes through the field-edit number gate (Annex A
  float lexeme, tokenizer round trip, finite, schema bounds: `scale > 0`).
  The typed lexeme is written exactly. A component equal to its current
  lexeme is kept without re-validation, so an unusual authored value
  survives. A rotation axis of `0 0 0` is refused. Angles and axes are never
  normalized.
* **Extrusion rules (ISO 6.18).** A closed outline (first point = last point)
  refuses deletion of its first or last point, and refuses insertion before
  the first or after the last point (`outline-would-open`). Minimums: 4
  entries for a closed outline, 2 for an open one, 2 spine points, 1 scale or
  orientation value. Moving the closing point is an explicit Set: it is
  allowed and never repaired.
* **One transaction.** A request is a sequence of operations. Each one is
  planned against the text the previous one produced and re-parsed. The
  result is reduced to ONE span edit (the exact differing region), which must
  lie inside the original value and strictly inside the node. The final
  parse must show the same node (same start, type and DEF, outside PROTO
  scope), the same field and exactly the intended lexemes.
  `Document::apply_source_transaction` re-applies it and requires the planned
  text byte for byte.
* **Add field.** Allowed only for an absent VRML97 `field` / `exposedField`
  (SFBool, SFFloat or a list type). The body braces come from TOKENS, never
  from a text search. The new statement is appended after the last body
  statement, with the gap between the last two statements copied (or one
  space). Proof: every earlier statement keeps its span, the new field is
  last, and its value text is exactly the one written.

## Tests

| test | result |
|---|---|
| `cargo test -p wrlforge-vrml` (99, of which 14 are new: `mf_edit` 12 and `extrusion` 2) | pass |
| `cargo test -p wrl-forge-desktop` (57; new: `extrusion_create_edit_undo_redo_save_reopen_is_exact`, `extrusion_expectations_…`) | pass |
| `cargo test -p wrlforge-ui extrusion` (3: op compaction, number text, local rule mirror) | pass |
| `./smoke.sh --headless --extrusion --research <copies>` (70 steps, real window, Xvfb) | pass |
| `./smoke.sh --headless --create` (VISUAL-1 regression, 51 steps) | pass |
| `./smoke.sh --headless --pick` (VISUAL-2 regression, 127 steps) | pass |
| `./smoke.sh --headless --move` (VISUAL-3A / 3A1 regression, 156 steps) | pass |
| `./smoke.sh --headless --inspector velvet-thornwing.wrl` (copy; Inspector / Save regression, 61 steps) | pass |
| `--smoke-texture` (TEXTURE-LOCAL-1 regression, 27 steps) | pass |
| `cargo test -p wrlforge-desktop-protocol` | 25 / 26: the one failure is pre-existing on the base (see Known limits) |

The lane prompt's required tests, mapped:

1. Create, undo, redo, save, reopen: service test (LF, CRLF, BOM) and smoke.
   Nine undo steps return to `#VRML V2.0 utf8\n`, eight return to exactly the
   generated text, and redo restores the final text exactly.
2. and 3. One cross-section and one spine coordinate: unit, service and smoke.
   The source must equal the previous source with only that replacement.
4. Scale and orientation: written explicitly, then edited (service and smoke).
5. Add, move and delete outline points: one outline session (Set, snapped
   drag, Add, Insert-between, Delete) with one Apply.
6. Cancel: revision, text, undo and redo flags are unchanged.
7. One undo step per committed action: the smoke counts 9 steps for 9
   actions.
8. LF, CRLF + BOM and lone CR: unit, service and three smoke fixtures saved to
   disk. Rust checks each file equals the original with one coordinate
   changed, and that one backup holds the original bytes.
9. Comments and unrelated fields: fixtures keep `PROTO Keep`, `USE Body` and
   `# diamond ✓`. Unit tests cover comments between and inside points.
10. Stale revision, stale count, off-by-one span, duplicated field: refused.
11. Malformed numbers (`1..5`, `1e`, `NaN`, `1e999`), `scale ≤ 0`, a zero
    axis, the wrong component count, nested brackets, strings, hex and a bare
    run of two tuples: refused or read-only, with no change.
12. DEF/USE and PROTO content: preserved byte for byte (unit and smoke).
    A node in a PROTO body and an IS-bound field are refused.

**Rendering (X_ITE).** Every committed edit loaded the new revision ("rev N ·
loaded"). The created hexagonal prism was drawn (covered pixels 0% → 5.2%),
and Undo of the creation showed the empty world.

**Research fixtures.** Six files from the Spazz3D research (`in/gall.wrl`,
`in/f1_ext.wrl`, `out/ex_c0.txt`, `out/sw_drag1.txt`, `out/nall_swext.txt`,
`out/rv_closed90_ext.txt`) were COPIED into the run's temporary directory and
rendered by X_ITE under the VRML97 rule (1 to 17 Extrusion nodes each). The
Inspector opened on each, nothing was written, and Rust confirmed every copy
byte-identical. The captures stay in the temporary run directory: the
research README allows only findings to enter the repository, so no research
file or capture is committed. This is a visual record only. It makes no
claim of byte-level or visual compatibility with Spazz3D.

## Known limits

* Spine editing is numeric (Inspector table). There are no viewport spine
  handles and no frame display (that is a later lane).
* The outline editor has no translate / mirror / rotate / scale commands, no
  "Reverse order" command, and no undo inside a session (Cancel and reopen).
* "Write field" appends the field after the last statement. It does not move
  it into schema order.
* The editor shows winding but never changes it. A non-convex outline keeps
  `convex` as authored. The user must set `convex FALSE`.
* There is no automatic Spazz3D warning or repair (by owner rule, the
  signature is unproven). Legacy repair is a later, explicit action.
* The experimental native (wgpu) renderer still reports Extrusion as
  unsupported. It was not extended.
* `MFInt32`, `MFFloat`, `MFColor`, `MFString` and `MFNode` remain read-only.
* A pre-existing test failure on the base: `wrlforge-desktop-protocol`
  `theme::tests::component_styles_use_tokens_not_raw_colors` flags
  `var(--wf-warning-fg, #e0a030)` in `style.css` (TEXTURE-LOCAL-1). It is
  unchanged by this lane.

## Provenance

No Spazz3D or Flux Studio program code, text, layout or assets were copied.
The research documents (`spazz3d-re-a3-procedural/*`,
`re-crossapp-1/2026-10-11-flux-studio/WRLFORGE_REQUIREMENTS.md`) informed
only the behavior requirements: one undo step per session, no silent
reordering, closing or clamping, and no placeholder geometry. Field types,
defaults and bounds come from the committed node schema (ISO/IEC 14772-1).
No GPL upstream code was used.
