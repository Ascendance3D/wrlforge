# WD2-D-INLINE Viewport Selection and Opening of Inline Content

**Status:** Draft — proposal for owner review; not approved

Lane: [#118 WD2-D-INLINE](https://github.com/Ascendance3D/wrlforge/issues/118).
Parent: [#28 WD2-D](https://github.com/Ascendance3D/wrlforge/issues/28).

This is a design-only document. It authorizes no implementation; the lane
starts only on an explicit owner GO. Names marked **(proposed)** do not exist in
the code today. Every other function, file, status and reason cited below exists
on `main` at the time of writing.

## 1. Objective

Define when a viewport click on an object rendered by an `Inline` node may
offer to **open the Inline's own document** in the editor, and when it must
refuse.

Today WD2-D refuses every such hit with `REFUSED_EXTERNAL`
(`hit-belongs-to-another-document`), and a World session editing a nested file
refuses with `preview-root-is-another-document`
(`src/editor/viewport-pick.js`, `WD2_D_XITE_PICKING_CONTRACT.md` §USE, Inline
and PROTO). This design keeps both refusals as the default and adds one narrow,
provenance-bearing path in front of the first one.

The outcome of that path is never a selection. It is an offer to open another
document. No Inline child is ever selected, highlighted, or shown in the
Inspector of the containing document.

## 2. Invariants

- **Exact source text is the document.** A click on Inline content must bind
  to the Inline target document's own exact text, never to the containing
  document's text.
- **No cross-document mapping.** Inline children are never mapped into the
  parent document's source spans, Scene Tree, or Inspector (`WD2_D_XITE_PICKING_CONTRACT.md`
  §USE, Inline and PROTO; issue #118 Out of scope).
- **One document authority.** The editor keeps exactly one open document
  session (`src/editor/session.js`). Opening the Inline target replaces the
  session through the existing open path; it does not create a second editable
  document.
- **Fail closed. `WRONG_SOURCE_SELECTIONS = 0`.** If any proof step below is
  missing, stale, or ambiguous, the result is a refusal. There is no fallback
  by URL text, file name, node type, position, or similarity.
- **Main owns every path.** The renderer never resolves, reads, or authorizes
  a filesystem path. No new renderer file reader is introduced.
- **No remote fetching.** Remote Inline URLs are never fetched, opened, or
  followed.

## 3. Scope

In scope:

- World context only, and only when the preview root is the edited document
  (`St.editedIsPrimary` in `renderer/editor-preview.js`).
- Hits whose Shape lives in exactly one Inline scene that is a direct child of
  the edited document.

Out of scope (keep the current refusal):

- Mall (single-file) context. Inline hits stay `REFUSED_EXTERNAL`.
- A World session editing a nested file (`preview-root-is-another-document`).
- Inline nested inside Inline (see §5.3, ambiguity).
- Selecting or editing any node inside the Inline from the parent document.
- Cross-file structural edits (WD2-F).
- The Rust/Tauri host. Its pick proof (`wrlforge_vrml::pick::resolve_node`) is
  separate and needs its own binding design.

## 4. Exact source identity

The design binds one click to five facts. All five must be proven. Any one
missing refuses.

| # | Fact | How it is proven | Refusal if not proven |
|---|---|---|---|
| I1 | The **parent preview generation** is the displayed one and equals the editor buffer | The existing `resolvePick` stale check: `gen.text === analysis.text === currentText` and `currentCheck === null` | `REFUSED_STALE` (existing) |
| I2 | The hit Shape's execution context is **one Inline scene**, and that scene was created by **one Inline node of the parent generation** | The adapter reports, as plain data, the Inline node that owns the hit's `X3DScene` and that node's single recorded occurrence `[start,end)` in the parent generation text **(proposed adapter field)** | `REFUSED_EXTERNAL` `inline-owner-unproven` **(proposed reason)** |
| I3 | That parent occurrence joins **exactly one** AST `Inline` node of the current analysis, with **one** occurrence and **one** live parent, and the Inline node is not inside a PROTO body | The same exact-span join and uniqueness rules `resolvePick` already applies (steps 7–9), applied to the Inline node instead of the Shape | `REFUSED_AMBIGUOUS` / `UNSUPPORTED` with the existing reasons |
| I4 | The Inline's loaded scene URL maps to **one** World Project asset-graph WRL node, reached from the edited document by **one** graph reference | Main resolves the URL (§6). The renderer supplies the URL string only as evidence; main decides | `REFUSED_EXTERNAL` with a §7 reason |
| I5 | The target document's **current text is the text the preview rendered** | Main compares a content digest of the text it served to X_ITE for that target in this preview load with a digest of the text the editor would open (§4.1) | `REFUSED_EXTERNAL` `inline-target-changed` **(proposed reason)** |

### 4.1 Target content identity (I5)

A URL and an authorized path identify a file, not its content. If the file
changes on disk after the preview loaded it, a path check alone would still
pass and the editor would open different text than the user clicked.

Proposed binding:

1. Every Inline WRL request in the World preview is already served by main
   through `resolveWorldRequest` (`src/world-project/preview-source.js`), which
   reads the file and serves gzip-decompressed text for WRL nodes.
2. **(proposed)** For each World preview load, main records a **served-text
   ledger**: canonical absolute path → SHA-256 of the exact decompressed text it
   served. The ledger is replaced on every new preview load, so an entry can
   only describe the preview currently on screen.
3. If the same path is served twice in one load with different digests, the
   ledger marks that path ambiguous. Selection of it refuses.
4. On an open request, main reads the target through the same reader the
   editor open path uses, digests its decompressed text, and requires equality
   with the ledger entry. Mismatch, a missing entry, or an ambiguous entry
   refuses with `inline-target-changed`.

The ledger stores digests only, never text, and never leaves main.

Owner decision D2 (§10) covers whether the ledger is keyed to the preview load
by resetting it, or by adding a load identifier to the served URL.

### 4.2 Parent → child relationship

The Inline node proven in I3 has an authored `url` MFString. The graph
(`src/world-project/asset-graph.js`) resolves each reference relative to the
referencing file's own directory and records the parent of each WRL node.

The relation is proven only when all of these hold:

- The Inline node's `url` has exactly **one** entry that the graph classifies
  as a local reference. Several local candidates refuse with
  `inline-url-ambiguous` **(proposed reason)**, because X_ITE picks the first
  loadable candidate and the editor cannot prove which one rendered.
- That entry resolves to the same canonical path as the URL the loaded scene
  reports (I4).
- The graph records that path as a readable WRL node whose parent is the edited
  document.

### 4.3 The target keeps its own authority

The opened document is a fresh editor session of the target file, with its own
text, parse, scene tree, generations, and undo history. Nothing from the parent
session — spans, scene-tree ids, selection, undo — carries over. The parent
document is not modified.

## 5. Path through `resolvePick`

### 5.1 Current order (unchanged)

`resolvePick` in `src/editor/viewport-pick.js` runs, in order:

1. Orchestrator pre-refusal by `snapshot.outcome`: `disabled`, `stale`,
   `external` (this carries `preview-root-is-another-document`).
2. Parent generation stale check (I1).
3. `no-hit`, then non-`hit` outcomes.
4. Sensor conflict (`REFUSED_SENSOR_CONFLICT`).
5. `snapshot.ctxKind === 'external-scene'` → `REFUSED_EXTERNAL`
   `hit-belongs-to-another-document`.
6. Onward: PROTO body, syntax errors, climb, join, promotion.

### 5.2 Proposed insertion point

One new branch at step 5, and only there:

- If `snapshot.ctxKind === 'external-scene'` **and** the snapshot carries a
  complete Inline provenance block **(proposed field `snapshot.inline`)**, run
  the Inline proof (I2, I3). On success, return a new non-selection status
  **(proposed `INLINE_OPEN_CANDIDATE`)** that carries:
  - the parent generation id,
  - the proven parent `Inline` node span and type,
  - the Inline's loaded scene URL as reported, for main to resolve.
  It carries **no** `source` span for the hit and **no** `sceneTreeItemId`.
- Otherwise, return exactly what step 5 returns today: `REFUSED_EXTERNAL`
  `hit-belongs-to-another-document`.

Preserved behavior:

- Steps 1–4 run before the new branch. A stale parent, a World session on a
  nested file, a disabled adapter, and a sensor under the pointer refuse
  exactly as today.
- The in-climb check (`g.ctxKind === 'external-scene'` in step 7) is
  unchanged.
- Every external scene without a complete Inline provenance block — including
  Script `createVrmlFromString` scenes and any context the adapter cannot
  attribute to one Inline node — keeps `REFUSED_EXTERNAL`.

Because `INLINE_OPEN_CANDIDATE` is not `PROVEN`, the existing selection code
cannot select from it. It counts as neither a correct nor a wrong selection;
`WRONG_SOURCE_SELECTIONS` stays 0.

### 5.3 Ambiguity

The adapter must refuse to emit `snapshot.inline` (so step 5 refuses as today)
when:

- the Inline scene's owning Inline node has more than one live parent, or is
  reached through a `USE`;
- the hit Shape's context chain crosses more than one external scene (Inline
  inside Inline);
- the owning Inline node has no recorded occurrence in the parent generation,
  or more than one;
- the scene's URL cannot be read through a surface the adapter has proven on
  the pinned X_ITE builds.

### 5.4 Adapter surface

`src/preview/xite-pick-adapter.js` is the only module that may touch X_ITE.
Reading an Inline scene's owner node and loaded URL needs X_ITE surfaces that
the WD2-D contract's private-API inventory (P1–P12) does not list today.
**(proposed)** Each one is added as a new inventory row with its own structural
check and probe. If any row fails on a pinned X_ITE build, `snapshot.inline` is
never emitted and Inline hits keep `REFUSED_EXTERNAL`.

## 6. Authorization

Main decides every path, with the code that already exists:

1. The renderer sends only `{ sessionId, parent generation id, parent Inline
   span, reported scene URL }` **(proposed IPC payload)**. It sends no
   filesystem path.
2. Main checks the session and generation are still current (else
   `inline-target-changed` or the existing stale refusal).
3. Main classifies the URL with `classifyReference` in
   `src/world-project/path-policy.js`. Only `CATEGORY.LOCAL` continues.
4. Main resolves the URL against the **edited document's own directory** — the
   same rule the asset graph and the `wrlworld://` scheme use — and requires an
   exact match with a readable graph WRL node whose parent is the edited
   document (§4.2). It does **not** pass the raw URL to
   `authorizeWorldReference`, because that function resolves relative
   references against the project root, not the referring file.
5. Main passes the graph node's canonical path to the existing
   `editorController.openWorldReference` (`editor:openWorldReference`), which
   runs `authorizeWorldReference` in `src/editor/path-authorizer.js`:
   lexical root containment, membership in the graph WRL set
   (`worldWrlSet()` in `main.js`), and realpath (symlink) containment.
6. Main verifies I5 (§4.1) before the session is replaced.

Refused before any file is opened:

| Condition | Proposed reason |
|---|---|
| Remote URL (`http:`, `https:`, protocol-relative, other schemes) | `inline-target-remote` |
| Not a readable graph WRL node, or no longer on disk | `inline-target-missing` |
| Absolute path, traversal, outside the root, symlink escape, not in the graph WRL set | `inline-target-unauthorized` |
| Parent generation or target text no longer matches the preview | `inline-target-changed` |
| Several local URL candidates, several graph edges, nested Inline, unattributable scene | `inline-url-ambiguous` / `inline-owner-unproven` |

All of these use the existing status `REFUSED_EXTERNAL`; only the reason
differs. This keeps the status set and the existing UX text table stable.
The user-visible text for each reason is part of owner decision D3.

## 7. User experience

1. On `INLINE_OPEN_CANDIDATE`, the editor shows the existing in-DOM modal
   (`renderer/editor.js`, no native dialogs) with the prompt:

   **Open chair.wrl in the editor?**

   The name shown is the graph node's project-relative path, returned by main
   after authorization step 4. The renderer never derives it from the URL.
2. **Cancel** does nothing. The parent document, buffer, selection, and undo
   history are untouched.
3. **Open** runs the existing unsaved-changes guard (`confirmUnsaved` in
   `renderer/editor.js`) before the session is replaced. If the user cancels
   there, nothing changes. Recovery-store behavior is unchanged.
4. The click never moves the Scene Tree selection or the Inspector in the
   parent document.
5. Any refusal shows its one-line text in the existing status line and does
   nothing else.

## 8. Acceptance cases

Deterministic fixtures, one World Project each. "No change" means: the parent
buffer, selection, Scene Tree, Inspector, undo stack, and open session are
byte-for-byte and id-for-id identical before and after the click.

| # | Case | Fixture | Expected |
|---|---|---|---|
| 1 | Authorized local Inline | `main.wrl`: `Inline { url "chair.wrl" }`; `chair.wrl` in the graph | `INLINE_OPEN_CANDIDATE`; prompt "Open chair.wrl in the editor?"; on Open (after the unsaved guard) the session is `chair.wrl` with its exact disk text; on Cancel, no change |
| 2 | Stale source generation | Case 1, then (a) edit `main.wrl` without a preview refresh, and (b) separately, change `chair.wrl` on disk after the preview loaded | (a) `REFUSED_STALE` (existing reasons, e.g. `source-changed-since-preview`); (b) `REFUSED_EXTERNAL` `inline-target-changed`; no change |
| 3 | Remote target | `Inline { url "http://example.com/remote.wrl" }` | WRL Forge fetches and opens nothing. Any external-scene hit refuses `REFUSED_EXTERNAL`; an open request refuses `inline-target-remote`; no change |
| 4 | Missing target | `Inline { url "missing.wrl" }`, file absent | No scene loads. A target deleted after preview load refuses `inline-target-missing`; no change |
| 5 | Path traversal | `Inline { url "../../outside.wrl" }`, and a symlink inside the root pointing outside | Not served by `resolveWorldRequest`; any open attempt refuses `inline-target-unauthorized`; no change |
| 6 | Nested Inline ambiguity | `main.wrl` → `a.wrl` → `b.wrl`; click a Shape in `b.wrl`. Also `Inline { url ["chair.wrl" "chair2.wrl"] }`; also a `USE`d Inline | `REFUSED_EXTERNAL` (`hit-belongs-to-another-document` or `inline-url-ambiguous`); no change |
| 7 | Cross-document source leakage | Case 1 fixture; `chair.wrl` text is a byte prefix/offset collision of a `main.wrl` node span | No span in `main.wrl` is highlighted; the Inspector and Scene Tree of `main.wrl` do not change at any point, including after Cancel |
| 8 | Existing generic external refusal | Mall context with an Inline; World session editing `a.wrl` (preview root is `main.wrl`); an external scene with no Inline provenance block | Exactly today's results: `REFUSED_EXTERNAL` `hit-belongs-to-another-document` or `preview-root-is-another-document`; no change |

Pass requirement for every case: `WRONG_SOURCE_SELECTIONS = 0`.

## 9. Test plan (for the future implementation lane)

- `test/editor/viewport-pick.test.js`: the step-5 branch with and without
  `snapshot.inline`; every §5.3 ambiguity; the existing external refusals
  unchanged (case 8); `INLINE_OPEN_CANDIDATE` never carries `source` or
  `sceneTreeItemId`.
- Adapter tests with fakes for each new inventory row, including the
  "row fails → no `snapshot.inline`" path.
- `src/world-project` tests for the served-text ledger: reset per load,
  ambiguous double-serve, digest mismatch.
- `test/editor/path-authorizer.test.js` and editor-controller tests for the
  main-side resolution against the referring document, cases 3–5.
- Visual QA on Linux (primary), then macOS 15+, for cases 1, 2, 6 and 7.

## 10. Open owner decisions

None of these are decided by this document.

- **D1.** Confirm the proposed status name `INLINE_OPEN_CANDIDATE` and the
  choice to reuse `REFUSED_EXTERNAL` with new reasons instead of new
  `REFUSED_EXTERNAL_*` statuses.
- **D2.** How the served-text ledger is bound to one preview load: reset on
  every load (simplest) or a load identifier in the `wrlworld://` URL.
- **D3.** The user-visible text for each new refusal reason.
- **D4.** Whether nested Inline (Inline inside Inline) is ever in scope, and
  if so its separate proof.
- **D5.** Whether, after opening the target, the editor offers a way back to
  the parent document, and how that interacts with the unsaved guard.
- **D6.** Whether and when the Rust/Tauri host receives an equivalent design.
