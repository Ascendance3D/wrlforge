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
  parent document's source spans, Scene Tree, or Inspector
  (`WD2_D_XITE_PICKING_CONTRACT.md` §USE, Inline and PROTO; issue #118 Out of
  scope).
- **One document authority.** The editor keeps exactly one open document
  session (`src/editor/session.js`). Opening the Inline target replaces that
  session only after every proof passes; it never creates a second editable
  document.
- **Fail closed. `WRONG_SOURCE_SELECTIONS = 0`.** If any proof step below is
  missing, stale, or ambiguous, the result is a refusal. There is no fallback
  by URL text, file name, node type, position, or similarity.
- **Main owns every path.** The renderer never resolves, reads, or authorizes
  a filesystem path, and never supplies one. No new renderer file reader is
  introduced.
- **No remote fetching.** Remote Inline URLs are never fetched, opened, or
  followed.

## 3. Scope

In scope:

- World context only, and only when the preview root is the edited document
  (`St.editedIsPrimary` in `renderer/editor-preview.js`).
- Hits whose Shape lives in exactly one Inline scene that is a direct child of
  the edited document, rendered by the editor's live World preview
  (`src/preview/world-preview-bridge.js`).

Out of scope (keep the current refusal):

- Mall (single-file) context. Inline hits stay `REFUSED_EXTERNAL`.
- A World session editing a nested file (`preview-root-is-another-document`).
- Inline nested inside Inline, and `USE`d Inline nodes (§5.3).
- An Inline whose target is the edited document itself (§6.4).
- The World workspace disk preview (`world:previewLoad`). It shares the
  `wrlworld://` handler but has no editor generation, so it never produces an
  open offer.
- Selecting or editing any node inside the Inline from the parent document.
- Cross-file structural edits (WD2-F).
- The Rust/Tauri host. Its pick proof (`wrlforge_vrml::pick::resolve_node`) is
  separate and needs its own binding design (§5.4, D5).

## 4. Exact source identity

The design binds one click to five facts. All five must be proven. Any one
missing refuses with exactly the result listed in §5.3.

| # | Fact | How it is proven |
|---|---|---|
| I1 | The **parent preview generation** is the displayed one and equals the editor buffer | The existing `resolvePick` stale check: `currentCheck === null` and `gen.text === analysis.text === currentText` |
| I2 | The hit Shape's execution context is **one Inline scene**, owned by **one Inline node** that has one recorded occurrence in the parent generation text and one live parent | The adapter reports, as plain data, the owning Inline node's single occurrence `[start,end)` and the scene's loaded URL **(proposed field `snapshot.inline`)**; it omits the field when any part is unproven |
| I3 | That occurrence joins **exactly one** AST `Inline` node of the current analysis, outside any PROTO body, with exactly one local `url` entry | The exact-span join `resolvePick` already uses (steps 7–9), applied to the Inline node |
| I4 | The scene URL and the authored `url` entry name **the same** canonical path (§6.2), which is a readable World Project graph WRL node whose parent is the edited document | Main decides, from its own copies only (§6) |
| I5 | The text the editor opens is **byte-for-byte the text main served** to the clicked, displayed preview generation | Served-text ledger keyed by generation (§4.1) and verify-then-commit open (§6.3) |

### 4.1 Served-text ledger (I5)

A URL and an authorized path identify a file, not its content. A disk change,
or a reload whose new render is not yet displayed, could otherwise make a click
on the old render open text the user never saw.

Facts on `main` this design relies on:

- Every editor World preview generation is started by
  `WorldPreviewBridge` through `this.overlay.beginGeneration(sessionId)`, which
  replaces the bridge's serving context (`_serving`) **before** the new render
  is accepted. The previous scene stays on screen until the renderer accepts
  the new generation (`accept()` → `acceptGeneration`, which refuses replayed
  and stale generations).
- The renderer already receives that id as `provenance.generationId`
  (`renderer/editor-preview.js`), so every pick snapshot's
  `generation.overlayGeneration` names the main-side generation that rendered
  it.
- One `wrlworld://` handler in `main.js` serves both the editor live preview
  (through `worldPreviewBridge.servingContext()`) and the World workspace disk
  preview.

Required binding (mandatory, not an option):

1. **(proposed)** The bridge keeps one ledger **per generation id**. Each Inline
   WRL request served through the bridge's serving context is recorded in the
   ledger of the generation that `_serving` names at serve time, as
   canonical path (§6.2) → SHA-256 of the exact bytes served.
2. Requests served by the workspace disk preview are never recorded. A
   generation has no ledger unless the bridge started it.
3. A ledger entry whose bytes came from the unsaved-buffer overlay
   (`resolveWorldRequest` returns `overlay: true`) is marked **overlay**. An
   overlay entry is never openable (§6.4).
4. If one path is recorded twice in one generation with different digests,
   the entry is marked **ambiguous** and is never openable.
5. Ledgers are retained only for the currently accepted generation and any
   later pending one. Accepting a generation drops every older ledger.
6. An open request is valid only when its generation id **equals the
   currently accepted (displayed) generation** for that session. A pending,
   older, replayed, or unknown id refuses. A click on the old render while a
   new one is pending uses the old, still-accepted ledger; once the new render
   is accepted, the old click refuses.
7. A request served for an older scene after `_serving` moved on is recorded
   under the newer generation. If the content differs it becomes ambiguous
   there (rule 4); if it is missing from the clicked generation's ledger the
   open refuses. Either way the result fails closed.

The ledger stores digests only, never text, and never leaves main.

### 4.2 Parent → child relationship

The Inline node proven in I3 has an authored `url` MFString.

- Main reads that entry from **its own registered copy of the clicked
  generation's text** (the copy `overlay.resolve` registers for the
  generation), at the Inline span supplied by the renderer. The span is only
  a locator: main re-parses its own copy, requires exactly one `Inline` node at
  exactly that span, and reads the `url` field itself. No URL string from the
  renderer is used as authority.
- The `url` must have exactly one entry that `classifyReference`
  (`src/world-project/path-policy.js`) classifies as `CATEGORY.LOCAL`. Zero or
  several local entries refuse, because X_ITE loads the first loadable
  candidate and the editor cannot prove which one rendered.
- Main resolves that entry against **the edited document's own directory**,
  the same rule the asset graph and the `wrlworld://` scheme use.
- The graph (`src/world-project/asset-graph.js`) must record that canonical
  path as a readable WRL node whose parent is the edited document.

### 4.3 The target keeps its own authority

The opened document is a fresh editor session of the target file, with its own
text, parse, scene tree, generations, and undo history. Nothing from the parent
session — spans, scene-tree ids, selection, undo — carries over. The parent
document on disk is not modified.

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
  complete `snapshot.inline` block, run the I3 join. On success, return the new
  status **(proposed `INLINE_OPEN_CANDIDATE`)** carrying:
  - the parent generation id,
  - the proven parent `Inline` node span and type.
  It carries **no** hit `source` span, **no** `sceneTreeItemId`, and no URL or
  path.
- Otherwise, return exactly what step 5 returns today.

Preserved behavior:

- Steps 1–4 run before the new branch. A stale parent, a World session on a
  nested file, a disabled adapter, and a sensor under the pointer refuse
  exactly as today.
- The in-climb check (`g.ctxKind === 'external-scene'` in step 7) is
  unchanged.
- Every external scene without a complete `snapshot.inline` block — including
  Script `createVrmlFromString` scenes — keeps `REFUSED_EXTERNAL`
  `hit-belongs-to-another-document`.

### 5.3 One result per refusal sub-case

| Sub-case | Decided by | Result |
|---|---|---|
| Inline inside Inline (chain crosses more than one external scene) | adapter omits `snapshot.inline` | `REFUSED_EXTERNAL` `hit-belongs-to-another-document` |
| Owning Inline node is `USE`d, has more than one live parent, or has zero or several recorded occurrences | adapter omits `snapshot.inline` | `REFUSED_EXTERNAL` `hit-belongs-to-another-document` |
| Scene URL not readable through a proven adapter surface (§5.4) | adapter omits `snapshot.inline` | `REFUSED_EXTERNAL` `hit-belongs-to-another-document` |
| `snapshot.inline` present, but the span does not join exactly one `Inline` AST node, the join type differs, or the node is inside a PROTO body | `resolvePick` | `REFUSED_EXTERNAL` `inline-owner-unproven` **(proposed)** |
| `snapshot.inline` present, but the AST `url` has zero or several local entries | `resolvePick` | `REFUSED_EXTERNAL` `inline-url-ambiguous` **(proposed)** |
| Any main-side refusal after the user confirms | main (§6) | `REFUSED_EXTERNAL` with the §6.5 reason |

### 5.4 New status and its consumers

`INLINE_OPEN_CANDIDATE` **adds one value** to the `STATUS` set. Required
handling of every existing consumer:

- **`STATUS_TEXT` / `refusalText`** (`src/editor/viewport-pick.js`): no entry.
  `refusalText` returns `''` for it. The editor handles it before the generic
  refusal path.
- **`renderer/editor.js` pick handler**: today every non-`PROVEN`,
  non-`NO_HIT` result falls through to `showPickStatus(refusalText(res))`. The
  implementation adds one explicit branch for `INLINE_OPEN_CANDIDATE` that
  opens the §7 prompt and never calls `sceneSelection.setSelection`.
- **QA counters** (`qa/wd2-d-picking/orchestrate.js`, `WRONG_SOURCE_SELECTIONS`):
  the status is tallied as a **non-selection**. It can never count as a wrong
  selection because it carries no `sceneTreeItemId` and no `source`.
- **Rust/Tauri pick vocabulary** (`wrlforge_vrml::pick::Status`,
  `apps/desktop-tauri/protocol`): unchanged in this lane. The Tauri host keeps
  returning `REFUSED_EXTERNAL` for every Inline hit; adding the status there is
  owner decision D5.

### 5.5 Adapter surface

`src/preview/xite-pick-adapter.js` is the only module that may touch X_ITE.
Reading an Inline scene's owning node and loaded URL needs X_ITE surfaces that
the WD2-D contract's private-API inventory does not list today. **(proposed)**
Each one is added as a new inventory row with its own structural check and
probe. If any row fails on a pinned X_ITE build, `snapshot.inline` is never
emitted and Inline hits keep `REFUSED_EXTERNAL`.

## 6. Authorization and open

### 6.1 Request

The renderer sends exactly `{ sessionId, generationId, inlineSpan: { start,
end } }` over **one new IPC channel (proposed `editor:openInlineTarget`)**. It
takes no path, no URL, and no file name. The reported scene URL from
`snapshot.inline` is sent only as evidence `{ sceneUrl }` and is never used as
authority except as described in §6.2.

### 6.2 Main-side resolution

1. Session and generation: `sessionId` must be the live editor session and
   `generationId` the currently accepted generation (§4.1 rule 6). Else
   `inline-target-changed`.
2. Authored entry: read from main's own registered copy (§4.2). Else
   `inline-owner-unproven` or `inline-url-ambiguous`.
3. Classification: `classifyReference` must return `CATEGORY.LOCAL`. Remote
   categories refuse `inline-target-remote`; absolute, traversal, malformed and
   inline-script categories refuse `inline-target-unauthorized`.
4. Scene URL normalization. Main maps `sceneUrl` with the existing
   `requestToAbsPath` (`src/world-project/preview-source.js`) and first refuses,
   with `inline-target-unauthorized`, any URL that:
   - is not exactly scheme `wrlworld:` and authority `project` (this rejects
     `file:`, `http(s):` and every other scheme);
   - has a `?query` or `#fragment`;
   - contains a backslash, raw or percent-encoded (`%5C`);
   - contains a percent-encoded `/` (`%2F`), `.` (`%2E`), NUL (`%00`), or any
     encoding that fails `decodeURIComponent`;
   - has an empty, `.` or `..` path segment.
5. **Canonical absolute path.** One key is used everywhere — the ledger, the
   graph match, `allowedWrl.has` in `authorizeWorldReference`, and the
   `resolveWorldRequest` allow-list: `path.resolve(...)` of the resolved path,
   **exact case, no case folding, no `realpath`** on the key. This is the form
   `worldWrlSet()` in `main.js` and `requestToAbsPath` already produce. A case
   mismatch is a different key and refuses. Symlink escapes are still caught
   separately by `realpathInside` inside `authorizeWorldReference`.
6. Agreement: the canonical path from the authored entry (step 2) must equal
   the canonical path from `sceneUrl` (step 4). Else `inline-owner-unproven`.
7. Graph: the path must be a readable graph WRL node whose parent is the
   edited document. Else `inline-target-missing`.
8. Ledger: the clicked generation's ledger must hold a non-ambiguous,
   non-overlay digest for that path. Missing or ambiguous refuses
   `inline-target-changed`; overlay refuses `inline-target-is-document`.
9. Authorization: `authorizeWorldReference` (`src/editor/path-authorizer.js`)
   with the canonical path: lexical root containment, `allowedWrl` membership,
   realpath containment. Any failure refuses `inline-target-unauthorized`.

Main does **not** pass the raw authored URL to `authorizeWorldReference`,
because that function resolves relative references against the project root,
not the referring file.

### 6.3 Verify-then-commit open

Today `openWorldReference` → `_open` → `session.open` reads the file and
replaces the session in one step (`src/editor/editor-controller.js`,
`src/editor/session.js`). Hashing a separate read before that call would leave
a check-then-use gap: the file or a symlink could change between the two reads.

**(proposed)** The Inline open path therefore splits the open:

1. **Load** the target once through the existing `loadDocument` reader
   (gzip-transparent), without touching `session.doc`.
2. **Verify** the SHA-256 of exactly the loaded text against the ledger digest
   from §6.2 step 8, and re-run `realpathInside` on the path.
3. **Commit** — create the new document from those same loaded bytes and
   replace the session — only when step 2 matches.

On any mismatch or error, nothing is committed: the current session, its
buffer, unsaved work, recovery record, selection and undo history are
untouched, and the renderer receives a refusal. The renderer replaces its
buffer only on a successful response.

### 6.4 Self-Inline

An Inline whose canonical target is the edited document itself (`main.wrl`
inlining `main.wrl`) refuses `inline-target-is-document`, whether or not the
served bytes came from the unsaved-buffer overlay. Opening it would only
reload the same document from disk and discard the buffer the preview showed.

### 6.5 Refusal reasons

All main-side refusals use the existing status `REFUSED_EXTERNAL` with one of
these **(proposed)** reasons. The user-visible text for each is owner decision
D3.

| Reason | Condition |
|---|---|
| `inline-target-remote` | Remote URL category |
| `inline-target-missing` | Not a readable graph WRL node with the edited document as parent, or gone from disk |
| `inline-target-unauthorized` | Unusual URL form (§6.2 step 4), absolute path, traversal, outside the root, not in `allowedWrl`, symlink escape |
| `inline-target-changed` | Generation not the accepted one, ledger entry missing or ambiguous, digest mismatch at commit |
| `inline-owner-unproven` | Main cannot find exactly one `Inline` at the span in its own copy, or authored and scene paths disagree |
| `inline-url-ambiguous` | Zero or several local `url` entries |
| `inline-target-is-document` | Self-Inline, or overlay-served target |

### 6.6 Security posture

- CSP, the `wrlworld://` scheme, the scheme's allow-list and confinement, and
  the network posture are **unchanged**. Nothing new is fetched.
- IPC gains exactly **one** new channel, `editor:openInlineTarget`
  **(proposed)**, taking no path argument. No existing channel changes.
- `authorizeWorldReference`, `requestToAbsPath` and `resolveWorldRequest` are
  reused as-is; their checks are not relaxed.

## 7. User experience

1. On `INLINE_OPEN_CANDIDATE`, the editor asks main to run §6.2 steps 1–8
   (a dry run that opens nothing) and receives the target's project-relative
   name. It then shows the existing in-DOM modal (`renderer/editor.js`, no
   native dialogs):

   **Open chair.wrl in the editor?**

   The renderer never derives the name from a URL.
2. **Cancel** does nothing. The parent document, buffer, selection, and undo
   history are untouched.
3. **Open** runs the existing unsaved-changes guard (`confirmUnsaved` in
   `renderer/editor.js`), then sends the open request. §6.2 runs again in full
   before §6.3, so nothing from the dry run is trusted. If the user cancels the
   guard, or the open refuses, nothing changes.
4. The click never moves the Scene Tree selection or the Inspector in the
   parent document.
5. Any refusal shows its one-line text in the existing status line and does
   nothing else.

## 8. Acceptance cases

Deterministic fixtures, one World Project each. "No change" means: the parent
buffer, selection, Scene Tree, Inspector, undo stack, recovery record and open
session are byte-for-byte and id-for-id identical before and after the click.

| # | Case | Fixture | Expected |
|---|---|---|---|
| 1 | Authorized local Inline | `main.wrl`: `Inline { url "chair.wrl" }`; `chair.wrl` in the graph | `INLINE_OPEN_CANDIDATE`; prompt "Open chair.wrl in the editor?"; on Open (after the unsaved guard) the session is `chair.wrl` with exactly the served text; on Cancel, no change |
| 2a | Stale parent | Case 1, then edit `main.wrl` without a preview refresh | `REFUSED_STALE` (existing reason); no change |
| 2b | Target changed on disk | Case 1, then change `chair.wrl` on disk after the preview loaded, then Open | `REFUSED_EXTERNAL` `inline-target-changed`; no change |
| 2c | Change during an in-progress reload | Case 1; change `chair.wrl`; start a reload; click the still-displayed old render before the new one is accepted, then Open after it is accepted | Before acceptance: the old ledger is used and the digest mismatch refuses `inline-target-changed`. After acceptance: the old generation id refuses `inline-target-changed`. No change |
| 2d | Symlink swapped between check and open | `chair.wrl` is a symlink inside the root; between the dry run and Open, repoint it outside the root, and separately to another in-root file with different text | Outside: `inline-target-unauthorized`. Other in-root file: `inline-target-changed` at commit. No change |
| 3 | Remote target | `Inline { url "http://example.com/remote.wrl" }` | WRL Forge fetches and opens nothing. A hit refuses `REFUSED_EXTERNAL` `hit-belongs-to-another-document` (no scene, no `snapshot.inline`); a crafted open request refuses `inline-target-remote`; no change |
| 4 | Missing target | `Inline { url "missing.wrl" }`, file absent; and `chair.wrl` deleted after the preview loaded | Absent: no scene loads, no offer. Deleted: `inline-target-missing`; no change |
| 5 | Path traversal | `Inline { url "../../outside.wrl" }`; a crafted `sceneUrl` with `%2F`, `%5C`, `?q`, `#f`, `file:` | Not served by `resolveWorldRequest`; every crafted open request refuses `inline-target-unauthorized`; no change |
| 6a | Nested Inline | `main.wrl` → `a.wrl` → `b.wrl`; click a Shape in `b.wrl` | `REFUSED_EXTERNAL` `hit-belongs-to-another-document`; no change |
| 6b | `USE`d Inline | `DEF I Inline { url "chair.wrl" }` and `USE I`; click either instance | `REFUSED_EXTERNAL` `hit-belongs-to-another-document`; no change |
| 6c | Several local candidates | `Inline { url [ "chair.wrl" "chair2.wrl" ] }` | `REFUSED_EXTERNAL` `inline-url-ambiguous`; no change |
| 7 | Cross-document source leakage | Case 1 fixture; `chair.wrl` text has node spans whose offsets coincide with node spans in `main.wrl` | No span in `main.wrl` is highlighted; the Inspector and Scene Tree of `main.wrl` do not change at any point, including after Cancel |
| 8 | Existing generic external refusal | Mall context with an Inline; World session editing `a.wrl` (preview root is `main.wrl`); a Script-created external scene; the workspace disk preview | Exactly today's results: `REFUSED_EXTERNAL` `hit-belongs-to-another-document` or `preview-root-is-another-document` as today; no offer; no change |
| 9 | Self-Inline | `main.wrl`: `Inline { url "main.wrl" }`, with unsaved edits in the buffer | `REFUSED_EXTERNAL` `inline-target-is-document`; no change |

Pass requirement for every case: `WRONG_SOURCE_SELECTIONS = 0`.

## 9. Test plan (for the future implementation lane)

- `test/editor/viewport-pick.test.js`: the step-5 branch with and without
  `snapshot.inline`; every §5.3 row; the existing external refusals unchanged
  (case 8); `INLINE_OPEN_CANDIDATE` never carries `source` or
  `sceneTreeItemId`.
- Adapter tests with fakes for each new inventory row, including the
  "row fails → no `snapshot.inline`" path.
- Bridge tests for the per-generation ledger: keyed by generation, overlay and
  ambiguous marking, workspace-preview requests not recorded, old ledgers
  dropped on accept, pending and older generations refused (case 2c).
- `test/editor/path-authorizer.test.js` and editor-controller tests for §6.2
  normalization and the canonical key (cases 3–5), and for verify-then-commit
  leaving the session untouched on mismatch (cases 2b, 2d).
- QA counter test: `INLINE_OPEN_CANDIDATE` tallied as non-selection.
- Visual QA on Linux (primary), then macOS 15+, for cases 1, 2c, 6 and 7.

## 10. Open owner decisions

None of these are decided by this document.

- **D1.** Confirm the proposed status name `INLINE_OPEN_CANDIDATE`, the channel
  name `editor:openInlineTarget`, and reusing `REFUSED_EXTERNAL` with new
  reasons instead of new `REFUSED_EXTERNAL_*` statuses.
- **D2.** Whether verify-then-commit (§6.3) is a new method on the existing
  session/controller or a separate Inline-only open path. Either form must keep
  the single-read, commit-on-match rule.
- **D3.** The user-visible text for each new refusal reason.
- **D4.** Whether nested Inline (Inline inside Inline) is ever in scope, and
  if so its separate proof.
- **D5.** Whether and when the Rust/Tauri host receives an equivalent design
  and the new status.
- **D6.** Whether, after opening the target, the editor offers a way back to
  the parent document, and how that interacts with the unsaved guard.
