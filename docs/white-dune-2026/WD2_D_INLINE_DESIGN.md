# WD2-D-INLINE Viewport Selection and Opening of Inline Content

Lane: [#118 WD2-D-INLINE](https://github.com/Ascendance3D/wrlforge/issues/118).
Parent: [#28 WD2-D](https://github.com/Ascendance3D/wrlforge/issues/28).

## Current status

**DRAFT — proposal for owner review; not approved**

## Objective

Define the exact rules and proven authorization path for when a user clicks on an object inside an `Inline` node in the WRL Forge viewport.

Instead of an immediate refusal (the current WD2-D behavior: `REFUSED_EXTERNAL`, "hit-belongs-to-another-document"), the goal is to securely map the click to the target's source file and allow the user to open that target file *if* it is authorized by the World Project constraints.

## Architecture and Scope Rules

*   **Exact Source Text:** The click provenance must resolve to the *Inline target document's* exact source text, not the containing document's text.
*   **No Cross-Document Leakage:** The target document's tree must *not* be grafted into the current source document's Scene Tree or Inspector. Each editor session handles exactly one root `wrl` document.
*   **Fail Closed (WRONG_SOURCE_SELECTIONS = 0):** As with WD2-D, we must never select an incorrect or ambiguous node. If the file cannot be securely verified or opened, it must refuse selection.
*   **One Authority:** The existing World Project loader/authorization (`src/world-project/preview-source.js` / `project-loader.js`) and Editor workspace IPC (`world:openPrimaryInEditor`, etc.) must remain the only ways to open files. We do not invent a new side-channel renderer file-reader.
*   **No Remote Targets:** Only local files recognized by the asset graph can be selected and opened. Remote, ambiguous, or missing files are explicitly refused.

## Path from Viewport Hit to Authorized Target File

When `xite-pick-adapter.js` reports a hit where `ctxKind === 'external-scene'`:

1.  **Hit Provenance:**
    X_ITE's pick report includes the `Inline` shape. We trace up its parent chain. When the chain crosses `external-scene`, X_ITE (and our parse-time wrapper) retains the `url` from the `Inline` node that instantiated this scene.
2.  **Asset Graph Verification:**
    The viewport sends the target `url` back to the WRL Forge UI layer. The UI compares this target against the active World Project's asset graph.
3.  **Authorization Check (`preview-source.js`):**
    If the target URL is a local file inside the project root, and present in the `wrlNodes` allow-list, it is an **authorized local target**.
4.  **Opening the File:**
    Instead of selecting the node in the *current* Scene Tree (which violates single-document rules), WRL Forge dispatches an action to open the authorized local target in a new or replacement editor workspace (via standard IPC like `editor:openAuthorizedNested` or equivalent).
5.  **Refusal:**
    If the `url` is remote, missing, malformed, or outside the World Project root, WRL Forge must emit an explicit refusal and not open the file.

## Explicit Refusal States

We define the following explicit refusal states for `Inline` content (extending `src/editor/viewport-pick.js`):

| Condition | Internal Reason Code | User-Visible Text |
| :--- | :--- | :--- |
| Target is a remote URL (`http://`, etc.) | `REFUSED_EXTERNAL_REMOTE` | "This object belongs to a remote Inline file and cannot be opened locally." |
| Target file is missing from disk | `REFUSED_EXTERNAL_MISSING` | "This object belongs to an Inline file that cannot be found on disk." |
| Target file is outside the Project Root | `REFUSED_EXTERNAL_UNAUTHORIZED` | "This object belongs to an Inline file outside the authorized World Project." |
| Stale Preview / Out of sync graph | `REFUSED_EXTERNAL_STALE` | "The preview is out of date; refresh to select or open Inline files." |
| Multiple nested Inline ambiguous paths | `REFUSED_EXTERNAL_AMBIGUOUS` | "This object's exact source file cannot be unambiguously determined." |

## Concrete Acceptance and QA Cases

To maintain `WRONG_SOURCE_SELECTIONS = 0`, the implementation must pass the following QA scenarios:

*   **Case 1 (Authorized Local Target):**
    Clicking a Shape inside `Inline { url "chair.wrl" }` (where `chair.wrl` is in the asset graph) must surface an option or immediately trigger opening `chair.wrl` in the editor.
*   **Case 2 (Remote Target):**
    Clicking a Shape inside `Inline { url "http://example.com/remote.wrl" }` must yield `REFUSED_EXTERNAL_REMOTE` and do nothing else.
*   **Case 3 (Missing Target):**
    Clicking a Shape inside `Inline { url "missing.wrl" }` (where `missing.wrl` does not exist in the asset graph) must yield `REFUSED_EXTERNAL_MISSING`.
*   **Case 4 (Cross-document Leakage Prevention):**
    Clicking a Shape inside an `Inline` must *never* highlight a text span or populate the Inspector in the *parent* document's UI.
*   **Case 5 (World Project Confinement):**
    Clicking a Shape inside `Inline { url "../../../etc/passwd.wrl" }` must be blocked by the `path-policy.js` / `preview-source.js` containment rules and yield `REFUSED_EXTERNAL_UNAUTHORIZED`.

## Dependencies and Limits

*   **Dependencies:** `src/world-project/asset-graph.js` (for validating paths), `src/editor/viewport-pick.js` (for resolving hits).
*   **Limits:** Multi-selection across files is impossible. Editing an object inside an `Inline` without explicitly opening that `Inline` file is out of scope.
*   **Unresolved Decisions:**
    *   Should clicking an authorized `Inline` *immediately* navigate the editor away from the parent file, or surface a UI prompt ("Open chair.wrl?") first? (Recommendation: surface a prompt to prevent accidental navigation and loss of context).

## Test Plan

*   Unit tests in `test/editor/viewport-pick.test.js` to assert the new `REFUSED_EXTERNAL_*` states.
*   Visual QA: Ensure the correct refusal message appears in the UI status bar or popup when remote/missing files are clicked.
