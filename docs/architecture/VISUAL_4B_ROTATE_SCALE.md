# VISUAL-4B — Direct Rotate and Scale Tools

Lane: `feature/visual-4b-rotate-scale`, stacked on PR #135
(`feature/visual-4a-extrusion`, head `a5cc0bbccc380a5c26af93aed08c965129b4be6c`).
Application: the Tauri desktop app (`apps/desktop-tauri`). The Electron app
is unchanged.

Renderer decision: X_ITE remains the default renderer. The Rust (wgpu)
renderer remains experimental and is disabled by default. This lane uses
X_ITE for temporary feedback only; Rust writes every change.

## Feature contract

| capability | where | contract |
|---|---|---|
| Rotate tool | viewport toolbar **Rotate** (`aria-pressed`) | Select, Move, Rotate and Scale exclude each other. Three rings, one per PARENT (world) axis, are drawn around the pivot at a constant screen radius (70 px). A drag on a ring turns the object about that axis. Shift snaps to 15 degree steps. |
| Scale tool | viewport toolbar **Scale** | Three axis handles along the object's own scale axes (`R · SR · e_i`, from the pivot) and one uniform square at the pivot. An axis drag multiplies ONE scale component; the uniform drag multiplies all three by the same factor. |
| Live feedback | X_ITE | During a drag, the bound Transform's rendered `rotation` or `scale` is overridden through the public SAI (`xite-gizmo-adapter.js` `setField`). The source is not touched. Escape, a lost pointer, focus loss, a revision, selection, document or preview-generation change cancels and restores the rendered value. |
| Commit | `doc_rotate`, `doc_scale` | One request on release. Rust re-proves the target, plans the edit, and applies ONE `apply_source_transaction` (one undo step). A result equal to the current value after rounding is `Unchanged` (no revision). |
| Missing field | gizmo status **Write rotation** / **Write scale** | A Transform without an authored `rotation` (or `scale`) is refused with a reason (`rotation-not-explicitly-authored`, `scale-not-explicitly-authored`). The tool never writes the field by itself. The button writes the VRML97 default (`0 0 1 0` / `1 1 1`) through the VISUAL-4A `doc_add_field` workflow, as one undo step, and only on the user's request. |

## Geometry conventions

* **Pivot.** Every rotation and scale leaves the Transform's local `center`
  point fixed. In the parent frame that point is `T + C` (ISO/IEC 14772-1
  6.52: `P = T + C + R·SR·S·SR⁻¹·(p − C)`, so `p = C` maps to `T + C`).
  Rings and scale handles are drawn there. `translation` and `center` are
  never changed.
* **Rotation axes.** The rings are the PARENT frame's X, Y and Z axes. Only
  top-level Transforms are accepted, so the parent frame is the world. The
  new value is `rotation := Rot(axis, δ) · rotation`, composed as
  quaternions. No Euler angles are used.
* **Rotation drag.** The pointer ray (camera frozen at the press) meets the
  ring's plane. The angle of that point about the axis is measured in the
  ring's right-handed basis (`X: (Y, Z)`, `Y: (Z, X)`, `Z: (X, Y)`). Steps
  are unwrapped, so a drag past ±180° keeps counting. A ray within about 3°
  of the plane (`ring-edge-on-to-view`), a point behind the camera or a
  point within 15% of the radius from the pivot (`pointer-too-close-to-pivot`)
  is refused; a release at such a position cancels. A ring seen within about
  7° of edge-on is drawn disabled.
* **Scale axes.** Handle `i` points along `R · SR · e_i`: the direction the
  `i`-th scale component really stretches, including `scaleOrientation`. The
  factor is the pointer's parameter on that line divided by the parameter at
  the press (the press must be at least 30% of the handle length from the
  pivot). A factor below 0.001 (collapse or flip) is refused. The uniform
  drag is screen-vertical only: up enlarges, ×2 per 90 px. It is
  independent of the camera because a uniform scale has no direction.
* **Camera.** Every drag uses the camera snapshot from the press. Viewport
  size, canvas rectangle and field of view enter through the real view and
  projection matrices (`protocol::gizmo`). The VISUAL-3A1 camera carry keeps
  the user's view across the reload after each commit.

## Source rules (`crates/wrlforge-vrml/src/manipulate.rs`)

* **Target gate** (`manip_target(src, from, to, tool)`). The VISUAL-3A gate,
  per tool: exactly one standard `Transform` at the exact span, top-level,
  editable source, a unique DEF that is never USEd and never a ROUTE
  destination, and the tool's own field authored exactly once
  (`translation` for Move, `rotation` for Rotate, `scale` for Scale). The
  other frame fields must be readable or absent (ISO defaults). Scale also
  refuses an authored component ≤ 0 (`scale-not-positive`: VRML97 requires
  `scale > 0`, and a mirrored frame cannot be scaled safely). A nested
  Transform is refused (`transform-is-not-top-level`).
* **Runtime binding.** Unchanged from VISUAL-3A1: the preview locates the
  runtime node by the exact provenance span; `doc_translate_prove` (now with
  a `tool`) proves its runtime chain. A secondary assertion was added: the
  bound node's RENDERED rotation and scale must equal the source's
  (`preview-rotation-differs-from-source`, `preview-scale-differs-from-source`).
  Disagreement refuses; agreement proves nothing.
* **Rotation write** (`plan_rotate`). If the authored axis is already
  parallel to the ring axis (the other two components exactly 0), only the
  angle token changes (`angle ± δ`, never wrapped, axis lexemes untouched).
  Otherwise the composed value is written (unit axis to 6 places, angle in
  [0, π] to 4 places), and every component whose value does not change keeps
  its exact lexeme. Proof: the written text must reproduce the intended
  rotation matrix within 5·10⁻⁴ per element (`result-not-representable`), its
  axis must not be zero (`result-would-be-singular`), and only the rotation
  field's tokens may change (`edit-changes-another-axis`).
* **Scale write** (`plan_scale`). The factor must be finite and > 0
  (`scale-factor-invalid`). Each new value is rounded to four significant
  digits (at most 6 places). A value that rounds to 0 is refused
  (`result-would-be-singular`); the field-edit gate also enforces the schema
  bound `scale > 0`. An axis drag re-sends the other two components as their
  exact lexemes; only the changed tokens are written.
* **Transaction.** Both go through `field_edit::plan_field_edit` (re-parse,
  same node, same field, intended values) and `apply_source_transaction`,
  which requires the planned text byte for byte. A stale base revision is
  refused before planning.
* **Explicit field authoring** (`mf_edit::plan_add_field`). Extended with
  `AddValue::Tuple` for SFVec2f, SFVec3f and SFRotation (zero rotation axis
  and out-of-range values refused).
* The UI previews with `protocol::gizmo::rotate_about`; Rust commits with
  `manipulate::rotate_about`. A desktop test holds the two equal to 10⁻¹².

## Tests

| test | result |
|---|---|
| `cargo test -p wrlforge-vrml` (109; new: 9 rotate/scale planner tests, 1 tuple add-field test) | pass |
| `cargo test -p wrlforge-desktop-protocol` (32; new: 6 ring / scale-axis math tests; the theme test now passes) | pass |
| `cargo test -p wrl-forge-desktop` (60; new: `rotate_scale_undo_redo_save_reopen_is_exact`, `rotate_scale_refusals_change_nothing`, `ui_preview_rotation_equals_the_committed_rule`) | pass |
| `./smoke.sh --headless --rotscale` (71 steps, REAL X input, Xvfb) | pass |
| `./smoke.sh --headless --move` (VISUAL-3A / 3A1 regression) | 156 / 156 pass |
| `./smoke.sh --headless --create` (VISUAL-1 regression) | 51 / 51 pass |
| `./smoke.sh --headless --pick` (VISUAL-2 regression) | 127 / 127 pass |
| `./smoke.sh --headless --extrusion` (VISUAL-4A regression, no research copies) | 58 / 58 pass |
| `./smoke.sh --headless --inspector` (a /tmp copy of `velvet-thornwing.wrl`) | 61 / 61 pass |
| `--smoke-texture` (TEXTURE-LOCAL-1 regression) | 27 / 27 pass |

The lane prompt's required tests, mapped:

1. Rotate around X, Y and Z: real ring drags after a real orbit. Each
   commit changes only the rotation line; the relative rotation
   `R_new · R_old⁻¹` has the dragged axis (|dot| > 0.999).
2. Scale along X, Y and Z: real handle drags. X and Z in the New World
   run, Y in each fixture (in the New World view after three rotations the
   Y scale axis points at the viewer and is correctly drawn disabled).
3. Uniform scale: all three components × the same factor (1.414 on the
   run).
4. Escape: a Rotate and a Scale drag; the rendered frame returns; the
   release commits nothing.
5. One undo step per drag: 6 drags = 6 undo steps (smoke); 7 in the service
   test.
6. Exact undo and redo: each Undo equals the previous text, each Redo the
   committed text (smoke and service, LF / CRLF / BOM).
7. Save, close, reopen: Save As → Close → Open shows the same source; a real
   click selects Box_1 again; the Scale tool binds the saved rotation and
   scale. Rust checks the saved file equals Create Box with only its
   rotation and scale lines changed.
8. Stale revision and generation: an Undo mid-drag cancels the drag; a
   forced preview reload mid-drag cancels it; nothing commits. Stale
   `doc_rotate` / `doc_scale` / target requests are refused (service).
9. Ambiguous or unsupported targets: a USEd Transform, a nested Transform,
   a missing rotation or scale, a non-positive scale and a non-node item are
   refused (unit, service and smoke).
10. Camera: every commit keeps the view matrix (max Δ 0).
11. Existing translated and rotated objects: fixture `Turn` (translation
    `1 0.5 0`, rotation `0 0 1 0.5`, scale `1 1.5 1`, center `0.25 0 0`)
    binds with pivot `1.25 0.5 0`. A Z drag rewrites only the angle token.
12. Selection: after every commit the same object stays selected and
    inspected; after reopening a real viewport click selects it.
13. Comments, DEF names, unrelated text: three fixtures (LF; BOM + CRLF +
    Unicode; lone CR) with a PROTO, comments, `DEF`/`USE`. Rust checks each
    file on disk changed only the allowed rotation / scale lines (Turn's
    angle token only) and has one backup holding the original bytes.
14. LF, CRLF and BOM: service test and the three fixtures.
15. Move and Extrusion tools: `--move` 156 / 156, `--extrusion` 58 / 58;
    the smoke ends by switching back to Move, which binds again.
16. Native renderer: the smoke checks the wgpu renderer is not requested or
    shown and X_ITE renders the viewport.

## Theme fix

`style.css` used `var(--wf-warning-fg, #e0a030)` for the texture warning.
The raw fallback is removed; every theme defines `--wf-warning-fg`.
`theme::tests::component_styles_use_tokens_not_raw_colors` now passes. No
palette value changed. The new gizmo parts use only existing tokens
(`--wf-axis-*`, `--wf-gizmo-*`).

## Known limits

* Top-level Transforms only; no nested Transform and no multi-selection.
* Rotation is about the parent (world) axes only; there is no local-axes
  mode.
* The uniform scale handle is screen-vertical (direction-free by design).
* Rotation snapping (Shift) is not tested with real input (the harness has
  no held modifier key).
* A Transform whose `scale` has a zero or negative component cannot be
  scaled with the handles (Rotate and Move still work); use the Inspector.
* A general (non-parallel) rotation rewrites the whole rotation value as a
  unit axis and an angle in [0, π]; only components whose value is
  unchanged keep their lexemes.
* "Write rotation" / "Write scale" append the field after the node's last
  statement (VISUAL-4A rule), not in schema order.
* The experimental native renderer has no Rotate / Scale tools.

## Provenance

No code was copied. The handle / drag split is conceptually informed by
White Dune's `Node::getHandle()` / `Node::setHandle()` (already recorded in
`OPEN_SOURCE_PROVENANCE.md` section 4.1). Transform semantics come from
ISO/IEC 14772-1 6.52.
