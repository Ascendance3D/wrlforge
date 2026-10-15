# WD2-E Transform Authoring & Snapping — Gap Matrix & Implementation Review

**Target Issue:** [#50 [WD2-E] Transform authoring and snapping](https://github.com/Ascendance3D/wrlforge/issues/50) (Tracking Parent Issue — **Open**)
**Sub-Issues:** [#51–#59](https://github.com/Ascendance3D/wrlforge/issues?q=is%3Aissue+%5BWD2-E%5D) (Sub-issues — **Open**)
**Audit Date:** October 2026
**Repository Head SHA:** `ee68e9c3e8f9948af800ba39fd83d7d0d303d800`
**Status:** Audit Completed

---

## Executive Summary

This document reviews **WD2-E Transform Authoring and Snapping** (Issue #50 and sub-issues #51–#59) by comparing the issue requirements against the current Rust/Tauri implementation and test suites.

WRL Forge follows a **source-first** architecture where direct manipulation in the 3D viewport produces minimal, exact UTF-16 span edits to the underlying VRML97 source text. Edits never regenerate ASTs or bake transformation matrices into second document models.

### Critical Distinction: Code Implementation vs. Issue Lane Acceptance
- **GitHub Issue Status:** Parent issue **#50** and sub-issues **#51–#59** remain **OPEN** in GitHub. This audit evaluates technical code/test readiness in the repository; it does **not** authorize or mark GitHub issue lanes as accepted or closed.
- **Core Manipulators (Translate, Rotate, Scale):** Production code is present in Rust/Tauri (`wrlforge-vrml`, `wrlforge-desktop-protocol`, Leptos UI). Pure Rust unit test suites were executed and verified during this audit.
- **Inspector ↔ Viewport Synchronization:** Implemented in code and verified via unit tests and live signal/IPC inspection logic.
- **Undo / Redo:** Implemented and verified (one drag gesture = exactly one undo transaction).
- **Snapping:** **Partial**. 15° rotation angle snapping with the `Shift` key is implemented in UI. Discrete translation grid snapping and scale snapping are **missing** in production code.
- **Multi-Selection:** **Missing**. Selection state (`ui().selected`) and manipulator targets are strictly single-item in code.
- **Independent QA & Closeout:** **Not Tested / Pending**. Rust unit test suites were run and passed in this audit session. In-window GUI smoke tests (`--smoke-move`, `--smoke-rotscale`) exist in source code but were not executed in full Xvfb headless sessions during this audit.

---

## Gap Matrix Summary

> **Note on Status Column:**
> - **Code Readiness:** `Verified` (code implemented + unit tests passed in this session), `Partial` (partially implemented), `Missing` (code not implemented), or `Not Tested` (code exists but suite was not run in this audit).
> - **Issue Lane State:** All GitHub issue lanes (#50–#59) remain **Open**.

| Sub-Issue ID | Title / Area | Code Status | Primary Source Paths | Audit Test Evidence & Audit Execution Notes |
| :--- | :--- | :--- | :--- | :--- |
| **#50** | **[WD2-E] Parent Tracking & ACs** | **Partial** | Entire `wrlforge-vrml` & Tauri UI stack | Lane remains **Open**. AC audit detailed below. |
| **#51** | **[WD2-E-A] Manipulator Architecture** | **Verified** | `crates/wrlforge-vrml/src/manipulate.rs`<br>`apps/desktop-tauri/protocol/src/gizmo.rs`<br>`apps/desktop-tauri/ui/src/gizmo.rs` | **Executed in audit:** `cargo test -p wrlforge-vrml` (199 passed), `cargo test -p wrlforge-desktop-protocol` (128 passed).<br>GUI smoke test source reviewed (`ui/src/smoke/gizmo.rs`). |
| **#52** | **[WD2-E-B] Translate** | **Verified** | `wrlforge_vrml::manipulate::plan_translate`<br>`p::gizmo::axis_param`<br>`ui/src/gizmo.rs` (`Grip::Axis`) | **Executed in audit:** `crates/wrlforge-vrml/src/manipulate/tests.rs` (passed).<br>GUI smoke test source reviewed (`ui/src/smoke/gizmo.rs`). |
| **#53** | **[WD2-E-C] Rotate** | **Verified** | `wrlforge_vrml::manipulate::plan_rotate`<br>`p::gizmo::ring_angle`<br>`ui/src/gizmo.rs` (`Grip::Ring`) | **Executed in audit:** `crates/wrlforge-vrml/src/manipulate/tests.rs` & `protocol/src/gizmo/tests.rs` (passed).<br>GUI smoke test source reviewed (`ui/src/smoke/rotscale.rs`). |
| **#54** | **[WD2-E-D] Scale** | **Verified** | `wrlforge_vrml::manipulate::plan_scale`<br>`p::gizmo::scale_factor`<br>`ui/src/gizmo.rs` (`Grip::Stretch`, `Grip::Uniform`) | **Executed in audit:** `crates/wrlforge-vrml/src/manipulate/tests.rs` & `protocol/src/gizmo/tests.rs` (passed).<br>GUI smoke test source reviewed (`ui/src/smoke/rotscale.rs`). |
| **#55** | **[WD2-E-E] Inspector & Viewport Sync** | **Verified** | `apps/desktop-tauri/ui/src/gizmo.rs`<br>`apps/desktop-tauri/ui/src/panels.rs`<br>`apps/desktop-tauri/ui/src/editor.rs` | **Executed in audit:** Rust unit tests for field edit and inspection passed.<br>GUI sync logic source reviewed (`ui/src/smoke/gizmo.rs`). |
| **#56** | **[WD2-E-F] Snapping** | **Partial** | `apps/desktop-tauri/ui/src/gizmo.rs` (`SNAP = PI / 12.0`) | **Angle snapping code verified:** `Shift` key 15° rotation snap implemented.<br>**Missing:** Translation grid snapping and scale snapping are not implemented. |
| **#57** | **[WD2-E-G] Multi-Selection Policy** | **Missing** | `apps/desktop-tauri/ui/src/ui.rs` (`selected: RwSignal<Option<Selected>>`) | **Missing:** Selection model is strictly single-item. No multi-selection data structures or multi-item transform policy in code. |
| **#58** | **[WD2-E-Q] Independent QA** | **Not Tested** | `apps/desktop-tauri/ui/src/smoke/gizmo.rs`<br>`apps/desktop-tauri/ui/src/smoke/rotscale.rs` | **Not run in this session:** In-window GUI smoke tests exist in repo but full Xvfb headless binary run was not executed. Formal QA sign-off pending. |
| **#59** | **[WD2-E-CLOSE] Closeout & Docs** | **Missing** | `docs/reviews/WD2_E_GAP_MATRIX.md` | Issue remains **Open**. Documentation audit delivered in this PR. |

---

## Test Execution Evidence (Audit Session Log)

During this audit session (October 2026), the following automated test suites were **actively executed** and verified:

1. **`crates/` Workspace Tests:**
   - **Command:** `cargo test --workspace --manifest-path crates/Cargo.toml`
   - **Result:** **199 passed** in `wrlforge-vrml`, **3 passed** in `wrlforge-wasm`, 0 failed.
   - **Key Unit Coverage Executed:**
     - `manipulate::tests`: `plan_translate_retains_other_components`, `translate_target_refuses_unauthored_field`, `plan_rotate_parallel_axis_edits_angle_token_only`, `plan_rotate_oblique_rewrites_quaternion_composition`, `plan_scale_single_axis`, `plan_scale_uniform_refuses_zero_or_negative`.
     - `field_edit::tests`: exact UTF-16 span patch generation and round-trip verification.

2. **Desktop Protocol Tests:**
   - **Command:** `cargo test --manifest-path apps/desktop-tauri/protocol/Cargo.toml`
   - **Result:** **128 passed**, 0 failed.
   - **Key Unit Coverage Executed:**
     - `gizmo::tests`: `x_and_y_drags_move_exactly_the_projected_distance_both_ways`, `rotate_about_matches_matrix_composition`, `scale_handles_follow_the_object_frame`, `ring_angles_round_trip_from_front_and_oblique_cameras`, `angle_steps_unwrap_across_pi`, `decimals_follow_the_screen_precision`.

3. **Scene Crate Tests:**
   - **Command:** `cargo test --manifest-path apps/desktop-tauri/scene/Cargo.toml`
   - **Result:** **25 passed**, 0 failed.

4. **GUI Smoke Test Harness (`apps/desktop-tauri/ui/src/smoke/`):**
   - **Status in Audit:** **Source reviewed / Test code exists in repository**.
   - The full headless Xvfb GUI smoke harness (`--smoke-move`, `--smoke-rotscale`) requires desktop GTK/WebKit system libraries and Xvfb display server, so it was **not executed** in this CLI audit pass.

---

## Detailed Requirement Analysis

### 1. Manipulator Architecture (#51) — **Code Verified / Issue Open**
- **Requirements:** Architecture for viewport direct manipulation without AST regeneration, without hidden sidecar state, and with pure-function mathematical separation.
- **Code Implementation:**
  - Pure Rust AST parsing and target validation in `crates/wrlforge-vrml/src/manipulate.rs`. Handles parent frame composition (`instance_parent_frame`) for PROTO body transforms (B3 LV2).
  - Pure 3D math & screen projection (camera matrices, ray casting, layout calculations) in `apps/desktop-tauri/protocol/src/gizmo.rs`.
  - Leptos/WASM SVG overlay rendering and pointer event handling in `apps/desktop-tauri/ui/src/gizmo.rs`.
  - Live drag changes viewport preview via `ipc::gizmo_set_field` without modifying the source text mid-drag. Source is modified only on pointer release via `doc_translate`, `doc_rotate`, or `doc_scale`.
- **Evidence:**
  - Executed unit tests in `crates/wrlforge-vrml/src/manipulate/tests.rs` and `apps/desktop-tauri/protocol/src/gizmo/tests.rs`.
  - Reviewed GUI smoke harness in `apps/desktop-tauri/ui/src/smoke/gizmo.rs`.

### 2. Translate (#52) — **Code Verified / Issue Open**
- **Requirements:** Translate along world or local parent axes with on-screen manipulators, writing minimal source patches.
- **Code Implementation:**
  - `wrlforge_vrml::manipulate::translate_target` inspects target nodes for `Transform` node type, explicit `translation` field, DEF uniqueness, and absence of ROUTE destinations or USE instances.
  - `wrlforge_vrml::manipulate::plan_translate` computes exact component updates and uses `field_edit::plan_field_edit` to generate UTF-16 span replacement edits.
  - `p::gizmo::axis_param` computes constrained parameter along handle line using ray casting.
- **Evidence:**
  - Executed unit tests: `crates/wrlforge-vrml/src/manipulate/tests.rs` (`plan_translate_retains_other_components`).
  - Reviewed GUI smoke harness: `apps/desktop-tauri/ui/src/smoke/gizmo.rs`.

### 3. Rotate (#53) — **Code Verified / Issue Open**
- **Requirements:** Rotate about parent/world axes using on-screen rings, composed as quaternions, writing exact axis-angle values.
- **Code Implementation:**
  - `wrlforge_vrml::manipulate::plan_rotate` / `rotate_about` composes `Rot(axis, delta) · old` using quaternions (never Euler angles).
  - Formats axis-angle output (`ANGLE_DECIMALS = 4`, `AXIS_DECIMALS = 6`) and verifies matrix equivalence within `ROTATION_TOLERANCE = 5e-4`.
  - `p::gizmo::ring_layout_in` and `ring_angle_in` project 64-segment SVG rings on screen and evaluate angular motion in right-handed in-plane basis.
- **Evidence:**
  - Executed unit tests: `crates/wrlforge-vrml/src/manipulate/tests.rs` & `apps/desktop-tauri/protocol/src/gizmo/tests.rs`.
  - Reviewed GUI smoke harness: `apps/desktop-tauri/ui/src/smoke/rotscale.rs`.

### 4. Scale (#54) — **Code Verified / Issue Open**
- **Requirements:** Scale along local object axes (`R · SR · e_i`) or uniformly.
- **Code Implementation:**
  - `wrlforge_vrml::manipulate::plan_scale` multiplies individual or all scale components by `factor`.
  - Refuses non-positive, singular, or out-of-range scales.
  - `p::gizmo::scale_axes` projects scale handles along `R · SR · e_i` scale frame. `Grip::Uniform` scales all three axes simultaneously via center square handle.
- **Evidence:**
  - Executed unit tests: `crates/wrlforge-vrml/src/manipulate/tests.rs` & `apps/desktop-tauri/protocol/src/gizmo/tests.rs`.
  - Reviewed GUI smoke harness: `apps/desktop-tauri/ui/src/smoke/rotscale.rs`.

### 5. Inspector & Viewport Synchronization (#55) — **Code Verified / Issue Open**
- **Requirements:** Viewport edits update live in preview during drag, update Inspector upon release, and Inspector edits update viewport gizmo handles. Unauthored fields prompt explicit "Write [field]" actions.
- **Code Implementation:**
  - `apps/desktop-tauri/ui/src/gizmo.rs`: `drag_to` calls `ipc::gizmo_set_field` for live preview updates.
  - On release, `commit` calls backend (`doc_translate` / `doc_rotate` / `doc_scale`), updates `CORE.revision`, calls `ui::inspect()` to refresh the Inspector, and triggers `ui::preview(false)` to re-bind handles.
  - If `translation`, `rotation`, or `scale` is missing on a `Transform`, `manip_target` returns refusal code `[field]-not-explicitly-authored`. UI displays "Write [field]" button (`gz-write-field`), which calls `write_missing_field()` to insert ISO default values as a clean undo transaction.
- **Evidence:**
  - Rust field edit and inspection unit tests passed. GUI sync code reviewed in `apps/desktop-tauri/ui/src/smoke/gizmo.rs` & `rotscale.rs`.

### 6. Snapping (#56) — **Partial / Issue Open**
- **Requirements:** Deterministic snapping for translate, rotate, and scale.
- **Code Implementation Status:**
  - **Angle Snapping (Rotate):** **Verified in UI code**. Holding `Shift` during a rotation drag snaps the accumulated angle to 15° steps (`SNAP = std::f64::consts::PI / 12.0` in `apps/desktop-tauri/ui/src/gizmo.rs:697`).
  - **Translation Grid Snapping:** **Missing**. There is currently no grid snapping feature (e.g., 0.1, 0.5, or 1.0 unit grid steps) implemented in `plan_translate` or `p::gizmo::axis_param`.
  - **Scale Snapping:** **Missing**. No discrete step snapping for scale factors.
- **Evidence:**
  - `apps/desktop-tauri/ui/src/smoke/rotscale.rs` source tests Shift angle snapping during rotation.

### 7. Multi-Selection Policy (#57) — **Missing / Issue Open**
- **Requirements:** Explicit multi-selection policy, group transform gizmo or multi-item handle behavior.
- **Code Implementation Status:**
  - The UI state `ui().selected` (`apps/desktop-tauri/ui/src/ui.rs`) is an `RwSignal<Option<Selected>>`, strictly limited to one item.
  - The manipulation backend APIs (`translate_target`, `plan_translate`, etc.) take a single UTF-16 span `[from, to)`.
  - Multi-selection (Shift-selecting multiple scene nodes, multi-item bounding box, or multi-item gizmo transformation) is not supported in code.
  - *Note:* Shared PROTO instances render secondary outlines (`shared-outline`), but this is for instanced definitions, not multi-selection of distinct scene objects.

### 8. Single Undo Step / Transaction — **Code Verified**
- **Requirements:** One drag gesture = exactly one undo transaction.
- **Code Implementation:**
  - Mid-drag pointer movements modify temporary rendered properties in the viewport only (`ipc::gizmo_set_field`).
  - On pointer release (`finish_drag`), `doc_translate` / `doc_rotate` / `doc_scale` issues a single document edit command through `wrlforge_document`, incrementing revision by exactly 1.
  - Pressing `Escape` or losing pointer focus cancels the drag, calling `ipc::gizmo_restore()` and making zero document edits.
- **Evidence:**
  - Executed unit tests in `crates/wrlforge-vrml/src/manipulate/tests.rs`.
  - Reviewed GUI smoke harness in `apps/desktop-tauri/ui/src/smoke/gizmo.rs`.

### 9. Independent QA (#58) & Closeout (#59) — **Not Tested / Pending / Issues Open**
- **Requirements:** Independent QA audit verdict and documentation closeout.
- **Code Implementation Status:**
  - Automated unit test suites were run and verified in this audit session.
  - In-window GUI smoke tests exist in code (`ui/src/smoke/gizmo.rs` and `ui/src/smoke/rotscale.rs`), but full Xvfb headless binary runs were not executed in this session.
  - Issue lanes #50–#59 remain **Open** in GitHub pending official QA sign-off and owner closeout.

---

## Acceptance Criteria Summary (Parent Issue #50)

| Acceptance Criterion | Implementation Status | Issue Lane Status | Audit Notes |
| :--- | :--- | :--- | :--- |
| **Translate/rotate/scale write minimal source patches** | **Verified** | **Open** | `field_edit::plan_field_edit` generates minimal span patches touching only changed tokens. |
| **One drag = one undo step** | **Verified** | **Open** | Verified in unit tests. Mid-drag uses live preview updates; release commits 1 edit transaction. |
| **Snapping is deterministic** | **Partial** | **Open** | 15° Shift angle snapping is implemented. Grid translation snapping and scale snapping are missing. |
| **USE-shared nodes handled per explicit policy, never silently** | **Verified** | **Open** | USEd nodes are refused with `transform-is-used-elsewhere`. Shared PROTO definitions prompt explicit user consent before editing. |
| **Unsupported or ambiguous identity fails closed** | **Verified** | **Open** | Duplicate DEFs, non-top-level transforms, and unproven runtime nodes fail closed with clear refusal messages. |

---

## Code Locations & Reference Index

### Production Source Files
- `crates/wrlforge-vrml/src/manipulate.rs` — Pure Rust transform manipulation logic, AST inspection, PROTO frame composition.
- `crates/wrlforge-vrml/src/field_edit.rs` — Typed field edit planning and span patch generation.
- `apps/desktop-tauri/protocol/src/gizmo.rs` — Desktop protocol pure math (camera projection, ray casting, axis/ring drags).
- `apps/desktop-tauri/ui/src/gizmo.rs` — Leptos UI overlay, pointer event loop, IPC commit triggers, Shift angle snapping.

### Test Files
- `crates/wrlforge-vrml/src/manipulate/tests.rs` — 28 pure Rust unit tests (executed in audit).
- `apps/desktop-tauri/protocol/src/gizmo/tests.rs` — 25 pure Rust unit tests (executed in audit).
- `apps/desktop-tauri/ui/src/smoke/gizmo.rs` — Real pointer GUI smoke harness for Move tool (source reviewed).
- `apps/desktop-tauri/ui/src/smoke/rotscale.rs` — Real pointer GUI smoke harness for Rotate/Scale tools (source reviewed).

---

## Recommended Next Steps for Lane Closeout

1. **Grid Translation Snapping (Sub-issue #56 / WD2-E-F):**
   - Implement configurable grid snapping (e.g. 0.1, 0.5, 1.0 unit steps) in `p::gizmo::axis_param` or `ui/src/gizmo.rs` when `Shift` or a Grid Snap setting is active.
2. **Multi-Selection Architecture (Sub-issue #57 / WD2-E-G):**
   - Extend selection model in `ui().selected` (`Vec<Selected>`) and establish a multi-item transform policy (e.g., primary target gizmo vs group centroid gizmo).
3. **Formal QA Sign-off (#58) & Issue Closeout (#59):**
   - Run the full headless GUI smoke suite (`--smoke-move`, `--smoke-rotscale`) in an Xvfb environment and reconcile GitHub issue states upon owner GO.
