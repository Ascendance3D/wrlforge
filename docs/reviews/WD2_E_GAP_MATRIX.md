# WD2-E Transform Authoring & Snapping — Gap Matrix & Implementation Review

**Target Issue:** [#50 [WD2-E] Transform authoring and snapping](https://github.com/Ascendance3D/wrlforge/issues/50)
**Sub-Issues:** [#51–#59](https://github.com/Ascendance3D/wrlforge/issues?q=is%3Aissue+%5BWD2-E%5D)
**Date:** April 2026
**Status:** Audit Completed

---

## Executive Summary

This document reviews **WD2-E Transform Authoring and Snapping** (Issue #50 and sub-issues #51–#59) by comparing the issue requirements against the current Rust/Tauri implementation and automated test suites.

WRL Forge follows a **source-first** architecture where direct manipulation in the 3D viewport produces minimal, exact UTF-16 span edits to the underlying VRML97 source text. Edits never regenerate ASTs or bake transformation matrices into second document models.

### Summary Verdict
- **Core Manipulators (Translate, Rotate, Scale):** Fully implemented and verified in Rust and Tauri UI.
- **Inspector ↔ Viewport Synchronization:** Fully implemented and verified in live drags and post-commit inspection updates.
- **Undo / Redo:** Fully implemented and verified (one drag gesture = exactly one undo transaction).
- **Snapping:** **Partial**. 15° rotation angle snapping with the `Shift` key is implemented and verified. Discrete translation grid snapping and scale snapping are **missing**.
- **Multi-Selection:** **Missing**. Selection state (`ui().selected`) and manipulator targets are strictly single-item.
- **Independent QA & Closeout:** **Not Tested / Pending**. Automated GUI smoke tests pass, but formal independent QA verdict and closeout docs remain open.

---

## Gap Matrix Summary

| Sub-Issue ID | Title / Requirement Area | Status | Primary Code Path(s) | Test Evidence & Proof |
| :--- | :--- | :--- | :--- | :--- |
| **#50** | **[WD2-E] Parent Parent Tracking & ACs** | **Partial** | Entire `wrlforge-vrml` & Tauri UI stack | AC audit below |
| **#51** | **[WD2-E-A] Manipulator Architecture** | **Verified** | `crates/wrlforge-vrml/src/manipulate.rs`<br>`apps/desktop-tauri/protocol/src/gizmo.rs`<br>`apps/desktop-tauri/ui/src/gizmo.rs` | `cargo test -p wrlforge-vrml`<br>`cargo test -p wrlforge-desktop-protocol`<br>`ui/src/smoke/gizmo.rs` |
| **#52** | **[WD2-E-B] Translate** | **Verified** | `wrlforge_vrml::manipulate::plan_translate`<br>`p::gizmo::axis_param`<br>`ui/src/gizmo.rs` (`Grip::Axis`) | `crates/wrlforge-vrml/src/manipulate/tests.rs`<br>`apps/desktop-tauri/ui/src/smoke/gizmo.rs` |
| **#53** | **[WD2-E-C] Rotate** | **Verified** | `wrlforge_vrml::manipulate::plan_rotate`<br>`p::gizmo::ring_angle`<br>`ui/src/gizmo.rs` (`Grip::Ring`) | `crates/wrlforge-vrml/src/manipulate/tests.rs`<br>`apps/desktop-tauri/ui/src/smoke/rotscale.rs` |
| **#54** | **[WD2-E-D] Scale** | **Verified** | `wrlforge_vrml::manipulate::plan_scale`<br>`p::gizmo::scale_factor`<br>`ui/src/gizmo.rs` (`Grip::Stretch`, `Grip::Uniform`) | `crates/wrlforge-vrml/src/manipulate/tests.rs`<br>`apps/desktop-tauri/ui/src/smoke/rotscale.rs` |
| **#55** | **[WD2-E-E] Inspector & Viewport Sync** | **Verified** | `apps/desktop-tauri/ui/src/gizmo.rs`<br>`apps/desktop-tauri/ui/src/panels.rs`<br>`apps/desktop-tauri/ui/src/editor.rs` | `apps/desktop-tauri/ui/src/smoke/gizmo.rs`<br>`apps/desktop-tauri/ui/src/smoke/rotscale.rs` |
| **#56** | **[WD2-E-F] Snapping** | **Partial** | `apps/desktop-tauri/ui/src/gizmo.rs` (`SNAP = PI / 12.0`) | Angle snapping verified in `ui/src/smoke/rotscale.rs`. Grid & scale snapping missing. |
| **#57** | **[WD2-E-G] Multi-Selection Policy** | **Missing** | `apps/desktop-tauri/ui/src/ui.rs` (`selected: RwSignal<Option<Selected>>`) | Single-selection only. No multi-selection selection data structure or multi-item gizmo policy. |
| **#58** | **[WD2-E-Q] Independent QA** | **Not Tested** | `apps/desktop-tauri/ui/src/smoke/gizmo.rs`<br>`apps/desktop-tauri/ui/src/smoke/rotscale.rs` | Automated smoke suites pass; formal independent QA verdict sign-off is pending closeout. |
| **#59** | **[WD2-E-CLOSE] Closeout & Docs** | **Missing** | `docs/reviews/WD2_E_GAP_MATRIX.md` | In progress (this gap matrix). Issue state reconciliation pending. |

---

## Detailed Requirement Analysis

### 1. Manipulator Architecture (#51) — **Verified**
- **Requirements:** Architecture for viewport direct manipulation without AST regeneration, without hidden sidecar state, and with pure-function mathematical separation.
- **Implementation:**
  - Pure Rust AST parsing and target validation in `crates/wrlforge-vrml/src/manipulate.rs`. Handles parent frame composition (`instance_parent_frame`) for PROTO body transforms (B3 LV2).
  - Pure 3D math & screen projection (camera matrices, ray casting, layout calculations) in `apps/desktop-tauri/protocol/src/gizmo.rs`.
  - Leptos/WASM SVG overlay rendering and pointer event handling in `apps/desktop-tauri/ui/src/gizmo.rs`.
  - Live drag changes viewport preview via `ipc::gizmo_set_field` without modifying the source text mid-drag. Source is modified only on pointer release via `doc_translate`, `doc_rotate`, or `doc_scale`.
- **Evidence:**
  - Unit tests: `crates/wrlforge-vrml/src/manipulate/tests.rs` (199 passed).
  - Protocol tests: `apps/desktop-tauri/protocol/src/gizmo/tests.rs` (128 passed).
  - Smoke tests: `apps/desktop-tauri/ui/src/smoke/gizmo.rs`.

### 2. Translate (#52) — **Verified**
- **Requirements:** Translate along world or local parent axes with on-screen manipulators, writing minimal source patches.
- **Implementation:**
  - `wrlforge_vrml::manipulate::translate_target` inspects target nodes for `Transform` node type, explicit `translation` field, DEF uniqueness, and absence of ROUTE destinations or USE instances.
  - `wrlforge_vrml::manipulate::plan_translate` computes exact component updates and uses `field_edit::plan_field_edit` to generate UTF-16 span replacement edits.
  - `p::gizmo::axis_param` computes constrained parameter along handle line using ray casting.
- **Evidence:**
  - `crates/wrlforge-vrml/src/manipulate/tests.rs`: tests `plan_translate_retains_other_components`, `translate_target_refuses_unauthored_field`.
  - `apps/desktop-tauri/ui/src/smoke/gizmo.rs`: end-to-end real pointer drags along X, Y, Z axes, verifying pixel displacement and exact single-token source edit.

### 3. Rotate (#53) — **Verified**
- **Requirements:** Rotate about parent/world axes using on-screen rings, composed as quaternions, writing exact axis-angle values.
- **Implementation:**
  - `wrlforge_vrml::manipulate::plan_rotate` / `rotate_about` composes `Rot(axis, delta) · old` using quaternions (never Euler angles).
  - Formats axis-angle output (`ANGLE_DECIMALS = 4`, `AXIS_DECIMALS = 6`) and verifies matrix equivalence within `ROTATION_TOLERANCE = 5e-4`.
  - `p::gizmo::ring_layout_in` and `ring_angle_in` project 64-segment SVG rings on screen and evaluate angular motion in right-handed in-plane basis.
- **Evidence:**
  - `crates/wrlforge-vrml/src/manipulate/tests.rs`: tests `plan_rotate_parallel_axis_edits_angle_token_only`, `plan_rotate_oblique_rewrites_quaternion_composition`.
  - `apps/desktop-tauri/ui/src/smoke/rotscale.rs`: real ring dragging smoke tests.

### 4. Scale (#54) — **Verified**
- **Requirements:** Scale along local object axes (`R · SR · e_i`) or uniformly.
- **Implementation:**
  - `wrlforge_vrml::manipulate::plan_scale` multiplies individual or all scale components by `factor`.
  - Refuses non-positive, singular, or out-of-range scales.
  - `p::gizmo::scale_axes` projects scale handles along `R · SR · e_i` scale frame. `Grip::Uniform` scales all three axes simultaneously via center square handle.
- **Evidence:**
  - `crates/wrlforge-vrml/src/manipulate/tests.rs`: tests `plan_scale_single_axis`, `plan_scale_uniform_refuses_zero_or_negative`.
  - `apps/desktop-tauri/ui/src/smoke/rotscale.rs`: real axis stretch and uniform square scale smoke tests.

### 5. Inspector & Viewport Synchronization (#55) — **Verified**
- **Requirements:** Viewport edits update live in preview during drag, update Inspector upon release, and Inspector edits update viewport gizmo handles. Unauthored fields prompt explicit "Write [field]" actions.
- **Implementation:**
  - `apps/desktop-tauri/ui/src/gizmo.rs`: `drag_to` calls `ipc::gizmo_set_field` for live preview updates.
  - On release, `commit` calls backend (`doc_translate` / `doc_rotate` / `doc_scale`), updates `CORE.revision`, calls `ui::inspect()` to refresh the Inspector, and triggers `ui::preview(false)` to re-bind handles.
  - If `translation`, `rotation`, or `scale` is missing on a `Transform`, `manip_target` returns refusal code `[field]-not-explicitly-authored`. UI displays "Write [field]" button (`gz-write-field`), which calls `write_missing_field()` to insert ISO default values as a clean undo transaction.
- **Evidence:**
  - `apps/desktop-tauri/ui/src/smoke/gizmo.rs` & `rotscale.rs`: verified Inspector input values match source translations/rotations/scales post-commit.

### 6. Snapping (#56) — **Partial**
- **Requirements:** Deterministic snapping for translate, rotate, and scale.
- **Implementation Status:**
  - **Angle Snapping (Rotate):** **Verified**. Holding `Shift` during a rotation drag snaps the accumulated angle to 15° steps (`SNAP = std::f64::consts::PI / 12.0` in `apps/desktop-tauri/ui/src/gizmo.rs:697`).
  - **Translation Grid Snapping:** **Missing**. There is currently no grid snapping feature (e.g., 0.1, 0.5, or 1.0 unit grid steps) implemented in `plan_translate` or `p::gizmo::axis_param`.
  - **Scale Snapping:** **Missing**. No discrete step snapping for scale factors.
- **Evidence:**
  - `apps/desktop-tauri/ui/src/smoke/rotscale.rs` tests Shift angle snapping during rotation.

### 7. Multi-Selection Policy (#57) — **Missing**
- **Requirements:** Explicit multi-selection policy, group transform gizmo or multi-item handle behavior.
- **Implementation Status:**
  - The UI state `ui().selected` (`apps/desktop-tauri/ui/src/ui.rs`) is an `RwSignal<Option<Selected>>`, strictly limited to one item.
  - The manipulation backend APIs (`translate_target`, `plan_translate`, etc.) take a single UTF-16 span `[from, to)`.
  - Multi-selection (Shift-selecting multiple scene nodes, multi-item bounding box, or multi-item gizmo transformation) is not supported.
  - *Note:* Shared PROTO instances render secondary outlines (`shared-outline`), but this is for instanced definitions, not multi-selection of distinct scene objects.

### 8. Single Undo Step / Transaction — **Verified**
- **Requirements:** One drag gesture = exactly one undo transaction.
- **Implementation:**
  - Mid-drag pointer movements modify temporary rendered properties in the viewport only (`ipc::gizmo_set_field`).
  - On pointer release (`finish_drag`), `doc_translate` / `doc_rotate` / `doc_scale` issues a single document edit command through `wrlforge_document`, incrementing revision by exactly 1.
  - Pressing `Escape` or losing pointer focus cancels the drag, calling `ipc::gizmo_restore()` and making zero document edits.
- **Evidence:**
  - `crates/wrlforge-vrml/src/manipulate/tests.rs`
  - `apps/desktop-tauri/ui/src/smoke/gizmo.rs`: verifies revision increments by 1 on release and `editor::history(true)` (Undo) restores exact original source text.

### 9. Independent QA (#58) & Closeout (#59) — **Not Tested / In Progress**
- **Requirements:** Independent QA audit verdict and documentation closeout.
- **Implementation Status:**
  - Comprehensive automated in-window smoke tests exist in `ui/src/smoke/gizmo.rs` and `ui/src/smoke/rotscale.rs`.
  - Formal independent QA sign-off verdict remains **Not Tested** until final release closeout review.
  - Documentation closeout (#59) is **In Progress** with the delivery of this gap matrix.

---

## Detailed Acceptance Criteria Audit (Parent Issue #50)

| Acceptance Criterion | Status | Evidence / Notes |
| :--- | :--- | :--- |
| **Translate/rotate/scale write minimal source patches** | **Satisfied** | `field_edit::plan_field_edit` generates minimal span patches touching only changed tokens. |
| **One drag = one undo step** | **Satisfied** | Verified across all tools. Mid-drag uses live preview updates; release commits 1 edit transaction. |
| **Snapping is deterministic** | **Partially Satisfied** | 15° Shift angle snapping is implemented. Grid translation snapping and scale snapping are missing. |
| **USE-shared nodes handled per explicit policy, never silently** | **Satisfied** | USEd nodes are refused with `transform-is-used-elsewhere`. Shared PROTO definitions prompt explicit user consent before editing. |
| **Unsupported or ambiguous identity fails closed** | **Satisfied** | Duplicate DEFs, non-top-level transforms, and unproven runtime nodes fail closed with clear refusal messages. |

---

## Code Locations & Reference Index

### Production Source Files
- `crates/wrlforge-vrml/src/manipulate.rs` — Pure Rust transform manipulation logic, AST inspection, PROTO frame composition.
- `crates/wrlforge-vrml/src/field_edit.rs` — Typed field edit planning and span patch generation.
- `apps/desktop-tauri/protocol/src/gizmo.rs` — Desktop protocol pure math (camera projection, ray casting, axis/ring drags).
- `apps/desktop-tauri/ui/src/gizmo.rs` — Leptos UI overlay, pointer event loop, IPC commit triggers, Shift angle snapping.

### Test Files
- `crates/wrlforge-vrml/src/manipulate/tests.rs` — 28 pure Rust unit tests for translate, rotate, scale, and refusal rules.
- `apps/desktop-tauri/protocol/src/gizmo/tests.rs` — 25 pure Rust unit tests for 3D projection, ray-plane intersection, and angle unwrapping.
- `apps/desktop-tauri/ui/src/smoke/gizmo.rs` — Real pointer GUI smoke tests for Move tool, Undo/Redo, camera persistence, Escape cancel.
- `apps/desktop-tauri/ui/src/smoke/rotscale.rs` — Real pointer GUI smoke tests for Rotate/Scale tools, Shift angle snapping, PROTO body instances.

---

## Recommended Next Steps

1. **Grid Translation Snapping (Sub-issue #56 / WD2-E-F):**
   - Add configurable grid snapping (e.g. 0.1, 0.5, 1.0 unit steps) to `p::gizmo::axis_param` or `ui/src/gizmo.rs` when `Shift` or a Grid Snap toggle is active.
2. **Multi-Selection Architecture (Sub-issue #57 / WD2-E-G):**
   - Define multi-selection state in `ui().selected` (`Vec<Selected>`) and establish a multi-item transform policy (e.g., primary target gizmo vs group centroid gizmo).
3. **QA & Owner Closeout (#58 & #59):**
   - Run official QA verification suite and reconcile issue status on GitHub once grid snapping and multi-selection policies are scheduled or explicitly deferred.
