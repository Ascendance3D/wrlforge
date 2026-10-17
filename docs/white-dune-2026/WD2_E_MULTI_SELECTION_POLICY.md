# WD2-E-G Multi-Selection Policy Proposal

**Status:** Draft — proposal for owner review; not approved

This document proposes the policy and architectural design for enabling multi-selection in WRL Forge, satisfying issue #57 (WD2-E-G). This is a design-only document; no production code changes are included.

## 1. Current Single-Selection Model

WRL Forge's selection architecture currently strictly enforces a single-item selection model:
* **The Selection Authority:** As dictated by `docs/ui/UI0_COMMAND_WORKSPACE_ARCHITECTURE.md`, there is one and only one selection authority: `sceneSelection` (or its UI state equivalent). In the native Tauri UI, this is manifested as `pub selected: RwSignal<Option<Selected>>` (`apps/desktop-tauri/ui/src/ui.rs:26`).
* **Agreement:** The Scene Tree, viewport, and Inspector all observe and interact with this single signal. Viewport picks translate to `ui::adopt_selection`, updating the single item (`apps/desktop-tauri/ui/src/ui.rs:171`).
* **Gizmo constraints:** Manipulators target a single node, relying on `ui().selected` (`docs/reviews/WD2_E_GAP_MATRIX.md`).

## 2. Selection Gestures

To move to a multi-selection model while maintaining a single selection authority:
* **Ctrl/Cmd+Click:** Toggles the target node's inclusion in the selection set.
* **Shift+Click (Range Selection):** In the Scene Tree, selects all nodes between the previously selected node and the target node based on scene tree order. (In the viewport, Shift+Click is traditionally reserved for other tasks like camera manipulation or snapping, so range selection will be restricted to the Scene Tree).
* **Marquee Selection (Out of Scope for initial implementation):** Box-selecting multiple nodes in the 3D viewport introduces complexity regarding occlusion and coordinate transformation and is deferred to a future phase.
* **One Selection Authority:** The `RwSignal<Option<Selected>>` will change to `RwSignal<Vec<Selected>>` (or similar set structure). The *authority* remains single and centralized in `ui.rs`, but the *payload* becomes a set.

## 3. Identity Rules for a Set

A multi-selection set must adhere to WRL Forge's strict identity and provenance rules (`WD.md`):
* **Independent Proof:** Every member of the selection set must be proven independently. When resolving a selection across revisions, each item is independently verified.
* **Fail-Closed:** If even a single member of a saved/restored selection set cannot be proven (e.g., its source span is invalid), the entire selection restoration fails-closed, dropping the unprovable items (or clearing the selection entirely, pending owner decision - see Open Questions). We do not permit partially valid identity sets.
* **DEF/USE Duplicates:** A selection set contains specific instantiations (source spans). Selecting a `USE` node selects that specific `USE` statement, not all instances.
* **PROTO Bodies:** Selecting a node inside a PROTO body selects the definition. Similar to single selection, edits here require `confirm_shared` consent as they affect all instances (`crates/wrlforge-vrml/src/manipulate.rs:21`).
* **Nested Members:** If a user selects a parent `Transform` and its child `Shape`, both exist in the set. However, transform operations (like translation) must strictly filter this set to only apply to the top-most selected ancestors to avoid double-transformation.

## 4. Operations on a Set

* **Translate, Rotate, Scale:**
  * **Pivot:** The transform pivot defaults to the centroid of the selected objects' bounding boxes in world space.
  * **Local vs. World:** Transformations apply in world space relative to the centroid, but must be mathematically projected back into each object's local coordinate space before creating the source span edits.
  * **Differing Parents:** If selected nodes have different parents, their local spaces differ. The single world-space transformation delta must be individually mapped to each node's local space.
* **Delete:** Deleting a selection set issues a single edit transaction that deletes all proven members. Source order must be respected (deleting from bottom to top of the document avoids invalidating subsequent spans during the plan). `Separator` nodes holding structural children will have their entire sub-trees removed recursively.
* **Duplicate:** Duplicating a set duplicates each member. `DEF` renaming rules apply to each item individually to ensure no naming collisions. When duplicating a `Separator`, the entire hierarchical block is deep-copied.
* **Inspector:**
  * The Inspector displays fields common to all selected node types.
  * If a common field has the same value across all selected nodes, that value is shown.
  * If a common field has differing values, it shows an indeterminate state (e.g., "Mixed Values" or a blank input).
  * Editing a mixed-value field applies the new single value to all selected nodes.
  * Read-only cases (such as structurally required fields that can't be set safely in batch) are disabled in the multi-select view.

## 5. Source Edits

WRL Forge's "source-first" architecture mandates exact UTF-16 span edits (`docs/reviews/WD2_E_GAP_MATRIX.md:15`).
* For a multi-selection operation, one user action (e.g., releasing the translation gizmo) produces `N` independent span patches.
* These `N` patches are applied as **one single transaction** to the source text.
* The system must verify that no two spans in the transaction overlap.
* No AST is regenerated; patches are strictly text replacements.

## 6. Undo

* **One Transaction:** A multi-item manipulation (e.g., a drag gesture) constitutes exactly one undo transaction, incrementing the revision by exactly 1 (`docs/reviews/WD2_E_GAP_MATRIX.md:149`).
* **Undo/Redo State:** Upon undoing, the selection state reverts to the exact set of nodes (by their proven spans in the prior revision) that were selected before the operation. Redo restores the post-operation selection set.

## 7. Test Plan

* **Planner Unit Tests:** New unit tests in `crates/wrlforge-vrml/src/manipulate/tests.rs` and `crates/wrlforge-vrml/src/structure_edit.rs` will verify multi-item plan generation.
  * *Exact Output Text:* Assert that a multi-node translation produces the exact expected UTF-16 string with all unchanged formatting preserved.
  * *Refusal Reasons:* Assert that `plan_translate` refuses out-of-bounds, overlapping spans, or unprovable identities with the correct `reason` code.
* **Smoke-Test Outlines:**
  * Update `--smoke-move` and `--smoke-rotscale` (`apps/desktop-tauri/ui/src/smoke/`) to execute selection of multiple items, perform a manipulation, and verify all targets moved.

## 8. Open Questions for the Owner

1. **Fail-Closed Granularity:** If 2 out of 3 selected nodes fail to prove identity upon undo/redo, should the selection set gracefully degrade to the 1 valid node, or completely clear to 0 nodes?
   * *Recommended Default:* Completely clear to 0 nodes (Strict Fail-Closed) to prevent unnoticed silent failures of context.
2. **Inspector Type Heterogeneity:** If a `Transform` and a `Shape` are selected, should the Inspector show nothing, or show fields common to the underlying `Node` trait (if any)?
   * *Recommended Default:* Show nothing. Mixed-type editing is rarely well-defined in raw VRML without specific UI affordances.

## 9. Out of Scope

* **Snapping (#56):** Grid and scale snapping remain independent efforts and are not bundled with multi-selection.
* **Hierarchy Changes (WD2-F):** Grouping, ungrouping, and reparenting multiple selected nodes are deferred to phase WD2-F.
