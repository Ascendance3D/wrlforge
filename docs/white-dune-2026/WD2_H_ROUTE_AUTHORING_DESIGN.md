# WD2-H-R Behavior and ROUTE authoring — design

**Status:** Draft — proposal for owner review; not approved

This document proposes the architecture for behavior and ROUTE authoring in WRL Forge
(issue #71, a sub-issue of the WD2-H lane #70). It is design-only: no production code,
test, or workflow changes are included, and merging it does not authorize
implementation. ROUTE authoring, Script execution and animation-timeline authoring are
**not implemented** by this document or by the PR that adds it. Every module name below
marked *(existing)* was checked against the current tree; every name marked
*(proposed)* does not exist yet and is a placeholder for the implementation lane.

## 1. Goal

Visualize and edit ROUTEs, sensors, TimeSensor and Interpolators, allowing users to see
which nodes are wired together and create/edit interaction wiring without hand-typing
ROUTEs.

## 2. In Scope

- ROUTE visualization
- ROUTE editing (create, delete; rewiring is delete + create in one action — §6.4)
- Sensors
- TimeSensor
- Interpolators
- Interaction wiring

## 3. Out of Scope

- Script execution (SEC-SCRIPT-2). No Script is run to discover, validate, or preview a
  ROUTE.
- Keyframe timeline / animation authoring (WD2-I).
- Script-node authoring UX (deferred to WD2-K).
- ROUTEs inside PROTO bodies and ROUTEs whose endpoints are IS-mapped interface events
  (refused in the first slice — §5.4; widening needs a separate owner decision).
- EXTERNPROTO instances as ROUTE endpoints are shown as they resolve today (usually not
  proven — WD1.5-P2C §11.3) and are never offered as new ROUTE endpoints.

## 4. Architecture Constraints

### 4.1. Source-First Principle

The exact source text is the document (WD.md §2); every edit lands as WD1.2 span
patches. There is no second editable document model, canonical scene graph, or
AST→text regeneration.

### 4.2. No Sidecar State

No hidden source instrumentation, synthetic identifiers, or sidecar semantic state is
permitted. All relationships are derived from the exact parsed source through the
existing scope graph (`src/vrml/scope-graph.js`, *existing*).

### 4.3. Fail-Closed Identity

Unsupported or ambiguous identity fails closed (WD.md §7 hard gate). We must never
offer a "confident wrong node". A ROUTE endpoint that the scope graph does not report as
`resolved` is never treated as resolved by the authoring layer, and the authoring layer
never re-derives a binding the scope graph withheld.

### 4.4. One Undo Transaction

For each user-visible action (create a ROUTE, rewire an endpoint, delete a ROUTE) the
planner collects and validates the complete WD1.2 edit set, obtains a WD1.4
`verifyTransaction` receipt (`src/vrml/document-transaction.js`, *existing*), and the
caller then calls `applyVerifiedEdits({ oldText, edits, newText, userEvent })`
(`src/editor/browser/editor-view.js`, *existing*) **exactly once**. That call already
dispatches the whole set as one CodeMirror transaction annotated with
`isolateHistory.of('full')`, refuses with `buffer-changed` if the buffer is no longer
`oldText`, and takes the change back on `post-dispatch-mismatch`. The ROUTE layer must
never dispatch one transaction per patch and must never call `view.dispatch` directly.
One Undo restores the exact prior source text (every byte, including line endings);
one Redo restores the exact post-action text.

The `userEvent` for ROUTE actions reuses the existing closed list (`input.model`).
Adding a new user-event tag is an owner decision (§10), because `applyVerifiedEdits`
deliberately accepts only `input.inspector` and `input.model`.

### 4.5. Proven, Half-Proven and Unresolved Edges

ROUTE legality comes only from WD1.5-P2C endpoint resolution in
`src/vrml/scope-graph.js` (*existing*: `routeVerdict`, `resolveRouteNode`,
`resolveRouteEndpoint`, `routeEndpointFor`, `routeReferences`, `routesFrom`,
`routesTo`). The verdict vocabulary is the house `{status, reason}` pair with the six
committed statuses of `STATUS` in `src/vrml/symbols.js` (*existing*): `resolved`,
`unresolved`, `ambiguous`, `invalid`, `unsupported`, `recovered`.

The authoring UI classifies every ROUTE statement in the document into exactly one
display class, derived from those verdicts and nothing else:

| Display class | Definition (from P2C verdicts) | Shown as |
| --- | --- | --- |
| **Proven** | `routeVerdict(...).status === 'resolved'`: both node bindings, both endpoints, both directions and an exact type match resolved. Exactly the edges P2C admits to `routesFrom` / `routesTo`. | A solid edge between the two bound nodes. |
| **Ambiguous endpoint** | Either node half or event half is `ambiguous` (e.g. more than one preceding DEF of that name in scope). | Not drawn as an edge. Listed as an unresolved ROUTE with its reason. |
| **Missing endpoint** | A node half is `unresolved` (`def-not-declared-in-scope`, `route-node-not-defined-before-route`, cross-PROTO visibility) or an event half is `unresolved`/`invalid` (`route-endpoint-unknown-field`, `route-endpoint-node-type-unresolved`). | Not drawn as an edge. Listed with reason. |
| **Type-incompatible** | Both endpoints resolve but the pair fails (`route-type-mismatch`, `route-type-unknown`, `route-source-not-an-event-out`, `route-dest-not-an-event-in`). | Not drawn as an edge. Listed with reason. |
| **Half-proven** | One side resolves and the other does not (any mix of the rows above), or the ROUTE is `recovered`/`unsupported`. | At most a stub on the resolved node, visibly distinct (dashed, labelled "unresolved") and never in proven styling. |

Hard rule: **only the Proven class is ever rendered or reported as a connection.** No
UI element, tooltip, count, export, or command may present an Ambiguous, Missing,
Type-incompatible or Half-proven ROUTE as a connection between two nodes. The graph
reads proven edges from `routesFrom` / `routesTo` and everything else from
`routeReferences`, so the P2C "whole ROUTE or nothing" index rule (WD1.5-P2C §15) is the
single source of truth.

## 5. Type-Checked Creation and Wiring

### 5.1. Endpoint Resolution Requirement

Before creating `ROUTE A.out TO B.in`, the planner (*proposed*: `planRouteInsert`)
must prove, through the existing scope graph:

- `A` and `B` each bind to exactly one DEF visible at the insertion point (the new
  ROUTE's position in the source, not the cursor). A binding that is resolved only at
  some other position does not count.
- `A.out` resolves to an endpoint whose effective access can supply an eventOut
  (`eventOut` or `exposedField`, including the `zzz_changed` alias).
- `B.in` resolves to an endpoint whose effective access can accept an eventIn
  (`eventIn` or `exposedField`, including the `set_zzz` alias).

The planner proves this by applying the candidate edit set to a copy of the text,
reparsing, building the scope graph, and requiring `routeVerdict` on the newly inserted
ROUTE to be `resolved`. Anything else refuses the whole action (§5.3).

### 5.2. Type Compatibility (ISO 4.10)

Created ROUTEs must type-check per ISO/IEC 14772-1 §4.10.2: the source and destination
field types match exactly — no coercion, no SF↔MF promotion. This is P2C's
`route-type-mismatch` / `route-type-unknown` check; the authoring layer does not
implement its own type comparison.

### 5.3. Refusal Reasons and the Diagnostic Path

An illegal connection is refused before any edit is dispatched, and the refusal carries
a stable reason. Two layers already exist and are reused, not duplicated:

- **Planner refusal (*proposed*).** The ROUTE planner returns
  `{status: 'refused', reason}` in the same shape as `structure-edit.js` /
  `field-edit.js` (*existing*). Where the cause is a P2C verdict, the planner copies the
  P2C `reason` verbatim (e.g. `route-type-mismatch`, `def-not-declared-in-scope`), so the
  UI and the analyzer never disagree on why.
- **Document findings (*existing*).** Existing ROUTE statements in the document keep
  being reported by `src/vrml/semantic-findings.js`, which already maps P2C verdicts to
  `FINDING_CODE.ROUTE_NODE_NOT_BOUND` (node half not bound),
  `FINDING_CODE.ROUTE_ENDPOINT_NOT_BOUND` (event half not bound), and
  `FINDING_CODE.ROUTE_CONNECTION_REJECTED` (both ends bound, pair rejected — direction or
  type). User-facing text comes from `src/vrml/messages.js` and the P4-B diagnostic
  catalog; grouping from `src/vrml/presentation.js`.

The authoring UI shows the refusal using the message the P4-B catalog associates with
the corresponding finding code; it never invents new wording for a P2C reason.

### 5.4. First-Slice Refusals

Independently of P2C, the planner refuses (reason ids *proposed*):

- the document has a syntax error, an incomplete parse, or a stale parse session
  (reuse `structure-edit.js` / `field-edit.js` document-gate reasons);
- insertion into a document whose line breaks are lone CR only (§6.1);
- either endpoint lies inside a PROTO body, or the insertion point would be inside one;
- either endpoint node is an EXTERNPROTO instance;
- the identical ROUTE (`A.out TO B.in`, same scope) already exists — no duplicate is
  created silently;
- the target insertion point cannot be determined uniquely (§6.1).

## 6. Minimal Source-Span Patches

All changes use the existing WD1.2 patch algebra (`src/vrml/edit.js`, *existing*:
`validateEdits`, `applyEdits`) and are verified by WD1.4 `verifyTransaction` before the
editor sees them. The ROUTE planner follows the five-step verification already used by
`structure-edit.js`: derive the new text through WD1.2, verify the receipt, reparse with
no blocking syntax error, check the token stream outside the edit is unchanged, then
check the operation's own post-condition.

### 6.1. Insertion

- Inserts exactly one `ROUTE A.out TO B.in` statement, as a pure insertion
  (`from === to`), on its own line.
- Placement (default, pending §10): after the last existing top-level statement of the
  root scene, and after both DEFs (ISO 4.10.2 requires the nodes to be defined before
  the ROUTE). If that position is not uniquely determined, the action refuses.
- Line ending: the document's own EOL convention (`detectEol` in `structure-edit.js`,
  *existing* — CRLF when the first LF is preceded by CR, else LF). A CRLF document gets
  CRLF, an LF document gets LF; mixed documents use that detected EOL and no other line
  is normalized. `detectEol` does not recognise lone-CR line endings, so a document
  whose line breaks are lone CR only is refused for insertion (*proposed*) rather than
  given a foreign LF. A missing final newline is not added except as the separator the
  inserted line itself needs.
- Indentation: none (root scene). Inserted text never contains a comment.
- Post-condition: exactly one new ROUTE exists, wholly inside the inserted text, and
  its `routeVerdict` is `resolved`.

### 6.2. Deletion

- Removes exactly the selected ROUTE statement's source span — the `ROUTE` keyword to
  the end of the `B.in` token — taken from the parsed node's range.
- Also removes the indentation and line ending of the line **only** when the ROUTE
  occupies that line alone (`lineAlone` rule in `structure-edit.js`, *existing*).
  Otherwise only the ROUTE span and the whitespace run directly after it are removed.
- Comments are never removed: a comment on the same line, before or after the ROUTE,
  stays byte-for-byte, and a ROUTE that has a trailing comment on its line is not
  treated as alone on that line.
- Deleting an unresolved (non-proven) ROUTE is allowed, because it is a source-level
  operation on a node whose span is proven; the UI must say it is removing an
  unresolved ROUTE.

### 6.3. Comments, whitespace and line endings

Every byte outside the edit spans is copied verbatim by `applyEdits` (CRLF, lone LF,
lone CR, tabs, trailing whitespace, commas-as-whitespace, comments, missing final
newline). The ROUTE planner never normalizes, re-indents, or re-prints any other text.

### 6.4. Rewiring

Rewiring an endpoint is one user-visible action expressed as one edit set: a
replacement of exactly the changed `node.event` token span(s) inside the existing ROUTE
statement. The new ROUTE must verify as `resolved` (§5.1); otherwise the whole action
refuses and the original ROUTE is untouched.

### 6.5. Invalid, ambiguous and overlapping spans; atomic refusal

- **Invalid spans** (out of bounds, `from > to`, non-integer) are refused by WD1.2
  (`EEDITSHAPE`, `EEDITBOUNDS`).
- **Overlapping patches** (two edits touching the same text, or an insertion strictly
  inside a replaced span) are refused by WD1.2 (`EEDITOVERLAP`).
- **Ambiguous patches** (two insertions at the same offset) are refused by WD1.2
  (`EEDITAMBIGUOUS`).
- **Ambiguous target spans** — a ROUTE whose span cannot be proven to belong to exactly
  one parsed ROUTE node (e.g. parser recovery, duplicate ranges) — are refused by the
  planner before any patch is built.
- **Stale source** — the buffer changed between planning and apply — is refused by
  `applyVerifiedEdits` (`buffer-changed`).

**Atomic refusal:** WD1.2 validates the complete set before producing any output,
WD1.4 refuses a set that does not exactly reproduce the planned new text, and
`applyVerifiedEdits` dispatches nothing unless the buffer still equals `oldText`. A
refused action therefore leaves the document, the CodeMirror history, and the selection
exactly as they were — no partial change is possible.

## 7. Dependencies

- No hard technical block recorded; planning order is expressed by Horizon.
- **Blocks:** WD2-I (Animation and timeline authoring).
- **Relies on (all *existing*):**
  - WD1.5-P2C — ROUTE endpoint resolution, direction and exact-type verdicts
    (`src/vrml/scope-graph.js`, `src/vrml/symbols.js`).
  - WD1.2 — span-patch algebra (`src/vrml/edit.js`).
  - WD1.4 — verified transactions (`src/vrml/document-transaction.js`).
  - WD2-C — structural planner pattern, EOL and whole-line rules
    (`src/vrml/structure-edit.js`).
  - WD2-B / WD2-C editor dispatch — `applyVerifiedEdits`
    (`src/editor/browser/editor-view.js`).
  - Diagnostic producer and catalog — `src/vrml/semantic-findings.js`,
    `src/vrml/messages.js`, P4-B (`P4_B_DIAGNOSTIC_MESSAGE_CATALOG.md`).

## 8. Evidence and Test Plan

Tests are grouped so each group proves one layer and does not rely on another group's
harness. All are `node:test` focused tests except group 4.

### 8.1. Endpoint and scope validation (P2C consumer)

Fixtures with expected display class (§4.5) for each ROUTE:

- clean ROUTE → Proven; appears in `routesFrom` / `routesTo`;
- ambiguous node (two preceding DEFs) → Ambiguous; not in the reverse index;
- undeclared node, node defined after the ROUTE, node across a PROTO boundary →
  Missing;
- unknown field; `eventIn` used as source; `eventOut` used as destination → Missing or
  Type-incompatible per P2C reason;
- SFColor → SFVec3f, SFFloat → MFFloat → Type-incompatible (`route-type-mismatch`);
- `exposedField` via `set_`/`_changed` alias and via bare name → Proven;
- EXTERNPROTO instance endpoint → not Proven; offered as no new endpoint;
- one side resolved, other not → Half-proven; never Proven.

Each case asserts the finding code (`ROUTE_NODE_NOT_BOUND`,
`ROUTE_ENDPOINT_NOT_BOUND`, `ROUTE_CONNECTION_REJECTED`) and its P4-B message id, and
asserts that the planner's refusal reason equals the P2C reason.

### 8.2. WD1.2 source-patch correctness (ROUTE planner)

Independent of the P2C corpus harness:

- insertion into an LF document, a CRLF document, a mixed-EOL document, and a document
  without a final newline — every byte outside the insertion
  is unchanged and the inserted line uses the detected EOL;
- deletion of a ROUTE alone on its line (line removed), of a ROUTE sharing a line with
  another statement (only its span removed), and of a ROUTE with a trailing and a
  leading comment (comments kept byte-for-byte);
- rewiring replaces only the changed `node.event` token(s);
- invalid span, overlapping patches, two insertions at one offset, stale session,
  lone-CR-only document (insertion),
  syntax-error document, PROTO-body target, duplicate ROUTE — each refuses, and the
  returned text equals the input text exactly (atomic refusal);
- the WD1.4 receipt binds exactly `oldText` → `newText`; the token stream outside the
  edit is unchanged.

### 8.3. CodeMirror undo behavior

Through the editor handle's existing `historyDepth()` / `undo()` / `redo()` seam in
`src/editor/browser/editor-view.js` (used today for WD2-C one-step evidence):

- one create / delete / rewire adds exactly one undo step (`historyDepth`);
- Undo restores the exact previous text (string equality, CRLF fixture included);
  Redo restores the exact post-action text;
- a refused action adds no history entry and leaves the buffer unchanged;
- a buffer change between plan and apply refuses with `buffer-changed`.

### 8.4. Visual display of proven and half-proven edges (Electron visual QA)

- a document with one Proven ROUTE: the edge is drawn with proven styling;
- a document with an existing Half-proven ROUTE (one endpoint undeclared): no proven
  edge is drawn, and any stub is visibly distinct and labelled unresolved;
- Ambiguous, Missing and Type-incompatible ROUTEs appear only in the unresolved list,
  with their reasons;
- one-transaction undo/redo after a visual create, checked by byte-exact text.

### 8.5. Refusal of illegal connection creation

- a type-mismatched connection attempt is refused with the
  `ROUTE_CONNECTION_REJECTED` message; the document and history are unchanged;
- connection attempts to an ambiguous, missing, PROTO-body or EXTERNPROTO endpoint are
  refused with their reasons;
- the UI never offers an illegal endpoint as a valid drop target once the planner has
  refused it.

`npm run check` must pass. Tests not run are recorded in the implementation PR.

## 9. Corpus Proof

The implementation lane re-runs the existing WD1.5-P2C ROUTE corpus to show no
regression in endpoint verdicts, and adds the fixtures in §8.1–§8.2. The corpus run
proves verdict stability only; it is not evidence for the source-patch invariants in
§8.2.

## 10. Unresolved Owner Decisions

1. **Placement of new ROUTEs.** End of the root scene (current default), immediately
   after the later of the two DEFs, or clustered with existing ROUTEs. Fallback if
   undecided: end of the root scene, refusing when that is not uniquely determined.
2. **ROUTEs inside PROTO bodies and IS-mapped endpoints.** Refused in the first slice;
   enabling them needs a separate design.
3. **Undo user-event tag.** Reuse `input.model`, or add a new tag (e.g. `input.route`)
   to `applyVerifiedEdits`' closed list.
4. **Half-proven presentation.** Dashed stub vs. list-only (no stub at all).
5. **Sensor / TimeSensor / Interpolator editing UX.** This document covers wiring
   between them; their field editing reuses WD2-B typed inspector editing, and any
   dedicated UI needs its own owner-approved design.

## 11. Limits

This design limits the system to visual authoring and type-checking of the ROUTE
topology through exact-source patches. Execution of behaviors (Scripts), event
propagation, and timeline animation authoring remain out of scope, and none of them is
implemented by this document.
