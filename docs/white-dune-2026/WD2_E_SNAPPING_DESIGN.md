# Draft — proposal for owner review; not approved

## Introduction
This document outlines the design proposal for WD2-E-F Snapping (#56) for WRL Forge. It addresses translation grid snapping, scale step snapping, and integrates with the existing rotation snapping mechanics.

## 1. Current State
* **Angle Snapping (Rotate):** Verified in UI code. Holding `Shift` during a rotation drag snaps the accumulated angle to 15° steps (`SNAP = std::f64::consts::PI / 12.0`). Handled in `apps/desktop-tauri/ui/src/gizmo.rs:687` (at the audited commit). The snapped value is passed down into the IPC backend.
* **Translation Snapping:** Currently missing. There is no grid snapping feature implemented in `plan_translate` or `apps/desktop-tauri/protocol/src/gizmo.rs` `axis_param`.
* **Scale Snapping:** Currently missing. No discrete step snapping for scale factors.
* **UI State vs Document State:** The rotation snap is entirely calculated in `ui/src/gizmo.rs` within `drag_to`. This means snapping is correctly treated as view/UI state and does not leak into the document representation or `wrlforge_vrml` crates until commit.

## 2. Translate Grid and Scale Step Snapping
**User Model:**
* Snapping will activate when the user holds the `Shift` modifier key, matching the existing rotation behavior.
* **Translation Snapping:** Snaps movement to increments of standard grid sizes (e.g., 0.1, 0.5, 1.0 meters). Since the manipulator operates in parent coordinates (which is world space for top-level transforms, or parent-local space for nested Transforms), snapping constraints apply to these axes. Note: `doc_translate_target` in `apps/desktop-tauri/ui/src/gizmo.rs:5` currently requires the Transform to be a top-level node, meaning snapping currently only applies in world space.
* **Scale Snapping:** Snaps the scaling factor to increments (e.g., 0.1, 0.5 steps). This is applied to the calculated uniform or local-axis factor before composition.

## 3. Where Snapping Lives (Architecture)
Snapping must remain strictly as UI state.
* **UI application:** In `apps/desktop-tauri/ui/src/gizmo.rs`, inside `drag_to`, the `Shift` key parameter will dictate whether the resulting value (for translation distance or scale factor) is snapped *before* it is assigned to `d.value`.
* **Planners unchanged:** `plan_translate` (`crates/wrlforge-vrml/src/manipulate.rs:867`), `plan_scale` (`crates/wrlforge-vrml/src/manipulate.rs:1210`), and `plan_rotate` (`crates/wrlforge-vrml/src/manipulate.rs:1104`) will remain completely ignorant of snapping. They will continue to receive the precise numeric value requested by the UI via IPC (`p::gizmo`), validate its finiteness, and compute the minimal UTF-16 span patch required.

## 4. Number Formatting
* Snapped values are sent to the backend planners. `plan_translate` and `plan_scale` use `field_edit::plan_field_edit` to generate exact UTF-16 edits.
* **Precision and Step Rule:** The decimal formatting relies on the screen precision metric (`decimals_for(units_per_px)` in `apps/desktop-tauri/protocol/src/gizmo.rs:743`). However, for snapped values, the rule is: a snapped value must be written with exactly the decimals the step needs (e.g. max(step decimals, minimal exact repr)), and must never be rounded off-grid due to zoom level.
* **Preservation:** Unchanged components keep their exact authored lexemes (`plan_translate_with` and `plan_scale_with` logic). Only the manipulated component's token changes, preserving the user's authored number style on untouched components.

## 5. Fail-Closed Cases
* **IS-Bound Fields:** Uneditable fields fail closed. Verified at `crates/wrlforge-vrml/src/field_edit.rs:58` (`IS_BOUND = "field-is-is-bound"`) and the refusal at `:664`. Because `plan_translate` and `plan_scale` route through `plan_field_edit`, they reach this gate and safely refuse.
* **DEF/USE Shared Nodes:** USEd nodes refuse edits. Shared PROTO definitions require explicit confirmation (`ui::confirm_shared` in `apps/desktop-tauri/ui/src/gizmo.rs:1411`).
* **Non-finite input:** Handled safely. E.g., `axis_param` in `protocol/src/gizmo.rs:327` returns `Err(refusal::NOT_FINITE)`. Planners like `plan_rotate` also check `!delta.is_finite()` and return `reason::VALUE_NOT_FINITE` (`crates/wrlforge-vrml/src/manipulate.rs:1127`).
* **Rotations near singularities:** `axis_param` protects against parallel rays (`denom < MIN_SIN2`).
* In all these cases, the fail-closed logic lives in the backend Rust protocol and manipulators. Snapping in the UI will not bypass these safety checks because the snapped values still flow through the exact same IPC validation pipeline.

## 6. Undo Transaction Model
* **One drag = one edit.** The architecture enforces this. Mid-drag updates only affect temporary rendering (`ipc::gizmo_set_field`).
* Upon pointer release, `finish_drag` calls the commit logic which dispatches a single IPC command (`doc_translate` etc. in `apps/desktop-tauri/ui/src/gizmo.rs:1402`) that modifies `CORE.revision` (`apps/desktop-tauri/ui/src/editor.rs:29`) exactly once. Snapping just changes the float value submitted in that single transaction.

## 7. Test Plan
* **Planner Unit Tests:** Add pure Rust tests in `crates/wrlforge-vrml/src/manipulate/tests.rs` (or similar) supplying known "snapped" floating-point values to `plan_translate` and `plan_scale` to verify exact UTF-16 span replacement strings.
* **Formatting Test Case:** Add a specific planner test case verifying that snapped values are written exactly (e.g., 0.1 step always outputs "0.1" or "0.100" as minimally exact, ignoring screen zoom precision truncations that would place it off-grid).
* **UI Smoke Tests:** Extend `apps/desktop-tauri/ui/src/smoke/gizmo.rs` and `apps/desktop-tauri/ui/src/smoke/rotscale.rs` to simulate pointer drags with `Shift` held, asserting that the resulting IPC requests contain correctly quantized values for translation and scale, matching the 15° rotation test.

## 8. Open Questions for the Owner
1. **Configurable Grid Sizes:** Should the grid snap size (0.1, 0.5, 1.0) be user-configurable in the UI, or hardcoded for beta?
   * *Recommendation:* Hardcode standard steps (e.g., 0.1 for translation, 0.1 for scale) for beta to reduce settings surface area.
2. **Translation Snapping Origin:** Does translation snap relative to the object's original position, or to absolute world coordinate grid lines?
   * *Recommendation:* Snap relative to the object's start-of-drag position (delta snapping) to avoid unexpected large jumps for off-grid objects.
3. **Modifier Conflict:** Is `Shift` already used for anything else during translate/scale drags?
   * *Recommendation:* No current conflicts found during pointer drags (Shift is only used for rotation snapping, `ev.shift_key()` in `apps/desktop-tauri/ui/src/gizmo.rs:1371`).

## 9. Out of Scope
* Multi-selection transformation (#57).
* Hierarchy changes (WD2-F).
