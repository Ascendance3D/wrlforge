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
`oldText`, and takes the change back with `undo(view)` on `post-dispatch-mismatch`
(see §6.5 for what that leaves in history). The ROUTE layer must
never dispatch one transaction per patch and must never call `view.dispatch` directly.
One Undo restores the exact prior source text (every byte, including line endings);
one Redo restores the exact post-action text.

**Receipt binding (required before ROUTE actions ship).** Today
`applyVerifiedEdits` checks only that the buffer equals `oldText` before dispatch and
equals `newText` after it; it never sees the WD1.4 receipt, and the existing WD2-B/WD2-C
callers (`renderer/editor.js`) pass the planner's `edits` array alongside it. Two
different edit sets can turn the same `oldText` into the same `newText`, so those two
text checks alone do not prove that the dispatched spans are the spans that were
verified — and receipt-based identity mapping (WD1.4) would then describe spans that
were never dispatched. The ROUTE lane therefore requires:

1. The ROUTE planner returns, as its `edits`, exactly `receiptEdits(receipt)` — the
   frozen canonical set the WD1.4 receipt binds (`document-transaction.js`,
   *existing*, currently marked internal) — together with the receipt itself.
2. A dispatch-validation step — separate from the §5.4 scope gate, which runs before
   any patch exists — runs after planning and WD1.4 verification, immediately before
   dispatch (*proposed*: an optional `receipt` argument to `applyVerifiedEdits`, or a
   thin ROUTE-only wrapper around it). It refuses with a stable reason (*proposed*:
   `receipt-mismatch`, label "the edit could not be verified") unless
   `receiptBindsOldText(receipt, oldText)`, `receiptBindsNewText(receipt, newText)` and
   `edits === receiptEdits(receipt)` (the same frozen array, not an equal-looking copy)
   all hold, and then dispatches only that receipt-bound set.

Where that check lives (inside `applyVerifiedEdits` for every caller, or ROUTE-only) is
an owner decision (§10), because it changes an existing shared API and promotes
internal WD1.4 helpers. A ROUTE action without this check does not ship.

ROUTE actions pass `userEvent: 'input.model'`. `applyVerifiedEdits` maps
`'input.model'` to `input.model` and coerces every other value to `input.inspector`, so
no caller can introduce a third user-event tag through it. Adding a dedicated ROUTE tag
would change `applyVerifiedEdits` itself and is an owner decision (§10).

### 4.5. Proven, Half-Proven and Unresolved Edges

ROUTE legality comes only from WD1.5-P2C endpoint resolution in
`src/vrml/scope-graph.js` (*existing*: `routeVerdict`, `resolveRouteNode`,
`resolveRouteEndpoint`, `routeEndpointFor`, `routeReferences`, `routesFrom`,
`routesTo`). The verdict vocabulary is the house `{status, reason}` pair with the six
committed statuses of `STATUS` in `src/vrml/symbols.js` (*existing*): `resolved`,
`unresolved`, `ambiguous`, `invalid`, `unsupported`, `recovered`.

#### Reason class — exactly one per ROUTE

Every ROUTE statement gets **exactly one** reason class. The class is a pure function of
that ROUTE's `routeVerdict(graph, route)` result, which P2C already reduces to a single
`{status, reason, side}` by a fixed precedence: recovery of either node half first, then
source node, destination node, source event, destination event, then the exact-type
comparison (`computeRouteVerdict`, *existing*). The authoring layer applies the rows
below **top to bottom and stops at the first match**:

| # | Reason class | Matches when the verdict has… | P2C reasons covered |
| --- | --- | --- | --- |
| 1 | **Proven** | `status === 'resolved'` | `ok` (the endpoint resolutions may carry `route-endpoint-via-shorthand` / `route-endpoint-via-implicit-alias` as non-binding detail) |
| 2 | **Unsupported** | `status` is `recovered` or `unsupported` | e.g. `document-parse-incomplete` and other recovery reasons, `externproto-interface-not-locally-verifiable` |
| 3 | **Ambiguous** | `status === 'ambiguous'` | `duplicate-def-in-scope`, `duplicate-interface-member` |
| 4 | **Type-incompatible** | `reason` is one of the listed reasons | `route-source-not-an-event-out`, `route-dest-not-an-event-in`, `route-type-mismatch` |
| 5 | **Missing** | `reason` is one of the listed reasons | `def-not-declared-in-scope`, `route-node-not-defined-before-route`, `def-not-visible-across-proto-boundary`, `missing-name`, `route-endpoint-unknown-field` |
| 6 | **Unsupported** (fail-closed default) | anything else | `route-type-unknown`, `route-endpoint-node-type-unresolved`, and any reason not listed above, including reasons added to P2C later |

Type-incompatible covers both direction incompatibility (an endpoint exists but cannot
serve that side of a ROUTE) and field-type incompatibility; in both cases the named
endpoints exist. Row 6 makes the table total: an unrecognised reason can never fall
through into Proven or into a class that implies more than P2C proved.

**Ambiguous is a deliberate house rule.** VRML97 (ISO/IEC 14772-1 §4.6.2) lets a USE
— and a ROUTE node name — bind to the closest preceding DEF of that name. WRL Forge's
scope graph instead reports more than one preceding DEF in the same scope as
`ambiguous` / `duplicate-def-in-scope` and refuses to choose. This is stricter than the
standard on purpose: it is the fail-closed identity gate (§4.3), so a ROUTE that a
browser would wire to the later DEF is never shown as a proven edge here.

#### Half-proven — a rendering attribute, not a class

Independently of the reason class, the UI computes, for each side, whether that
**endpoint is proven**: `resolveRouteEndpoint(graph, eventRef).status === 'resolved'`
for that side's event reference (*existing*; a resolved event half implies a resolved
node half and a usable direction). A ROUTE is **half-proven** when its reason class is
not Proven **and exactly one** of its two endpoints is proven. Half-proven is
therefore an attribute layered on top of rows 2–6, never a separate class:

- Proven ROUTE (row 1): drawn as a solid edge between the two bound nodes. Never
  half-proven.
- Non-proven ROUTE with exactly one proven endpoint: listed under its reason class and
  reason, **plus** a stub on the proven endpoint's node only — dashed, labelled
  "unresolved", never in proven styling, never reaching a second node.
- Non-proven ROUTE with zero proven endpoints, or with two proven endpoints whose pair
  fails (Type-incompatible by `route-type-mismatch`): listed under its reason class
  only. No stub and no edge. (A mismatched pair is not half-proven: both endpoints are
  proven and the connection between them is refused.)

Hard rule: **only Proven ROUTEs are ever rendered or reported as a connection.** No UI
element, tooltip, count, export, or command may present a ROUTE of rows 2–6 as a
connection between two nodes. The graph reads proven edges from `routesFrom` /
`routesTo` and everything else from `routeReferences`, so the P2C "whole ROUTE or
nothing" index rule (WD1.5-P2C §15) remains the single source of truth.

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
  `FINDING_CODE.ROUTE_NODE_NOT_BOUND` (a node half is not `resolved`),
  `FINDING_CODE.ROUTE_ENDPOINT_NOT_BOUND` (an event half is not `resolved` while its
  node half is — this includes the direction reasons `route-source-not-an-event-out` /
  `route-dest-not-an-event-in`), and `FINDING_CODE.ROUTE_CONNECTION_REJECTED` (all four
  halves resolved and the exact-type comparison failed). These de-duplication rules are
  the producer's own and are not re-derived by the authoring layer. User-facing text comes from `src/vrml/messages.js` and the P4-B diagnostic
  catalog; grouping from `src/vrml/presentation.js`.

The authoring UI shows a P2C-originated refusal using the message the P4-B catalog
associates with the corresponding finding code; it never invents new wording for a P2C
reason.

**Planner-only reasons are not P4-B findings.** They describe an edit attempt, not a
fact about the parsed document, so they have no P2C reason and no finding code. They
follow the existing planner convention instead: a stable reason id returned by the pure
planner, mapped to a short presentation label in the renderer (as
`renderer/scene-inspector.js` `REASON_TEXT` does today for `field-edit.js` reasons).
Every planner-only reason is listed in §5.4 with its id and label; ids and labels marked
*proposed* do not exist yet. Whether ROUTE planner reasons should instead get P4-B
catalog entries is an owner decision (§10).

### 5.4. First-Slice Refusals

Independently of P2C, the planner refuses with the reasons below. The first three rows
reuse existing `field-edit.js` reason ids that already have renderer labels; every other
id and label is *proposed*.

**Shared scope gate.** Rows marked "all" form one check (*proposed*:
`routeScopeGate`) that runs for **every** ROUTE action — create, delete and rewire —
on the parsed document **before any WD1.2 patch is built or dispatched**. Deleting an
unresolved ROUTE (§6.2) is allowed only after this gate passes. Rows are checked in
table order and the first match is the reported reason, so each refusal has exactly one
deterministic reason. `receipt-mismatch` is not part of this gate: it needs the WD1.4
receipt and the edit array, so it belongs to the dispatch-validation step (§4.4).

| Reason id | Applies to | Refuses when | Label |
| --- | --- | --- | --- |
| `parse-session-is-stale` (*existing*) | all | the parse session is not for the current text | "the document changed; waiting for analysis" (*existing*) |
| `document-parse-incomplete` (*existing*) | all | the parse hit a cap | "the document exceeded a parse limit" (*existing*) |
| `document-has-syntax-errors` (*existing*) | all | the document has a syntax error | "the document has syntax errors" (*existing*) |
| `route-in-proto-body` | all | the ROUTE (delete, rewire) or the insertion point (create) is inside a PROTO body | "ROUTEs inside a PROTO are not editable yet" |
| `route-endpoint-in-proto-body` | all | either endpoint's bound DEF is inside a PROTO body | "this node is inside a PROTO" |
| `route-endpoint-is-mapped` | all | either endpoint's node or event is connected to a PROTO interface by `IS` | "this event is mapped through IS" |
| `route-endpoint-externproto-instance` | create, rewire | an endpoint the action would newly bind is an EXTERNPROTO instance | "EXTERNPROTO events cannot be verified locally" |
| `route-duplicate` | create, rewire | the resulting ROUTE duplicates another ROUTE in the same scope (key below) | "this ROUTE already exists" |
| `route-insertion-point-not-unique` | create | the insertion point (§6.1) is not uniquely determined | "no single place to add this ROUTE" |
| `document-line-endings-lone-cr` | create | the document's line breaks are lone CR only (§6.1) | "unsupported line endings (CR only)" |
| `route-not-in-session` | delete, rewire | the selected ROUTE's span is not exactly one parsed ROUTE node of this session | "selection is from an older analysis" |

`IS` can only appear inside a PROTO body (ISO 4.8.3), so in this first slice every
IS-mapped endpoint is already caught by `route-in-proto-body` or
`route-endpoint-in-proto-body`. `route-endpoint-is-mapped` is kept as its own reason and
check so the IS rule survives a later decision to allow PROTO-body ROUTEs (§10), and so
a test can prove it independently (§8.5).

**Duplicate key.** Each side of a ROUTE is keyed by **(bound DEF symbol, endpoint
`effectiveName`)** as reported by P2C, and the scope is the ROUTE's enclosing scope. The
key uses the endpoint's `effectiveName` rather than its written name because 4.7 alias
expansion maps `set_x` to an `eventIn` whose `effectiveName` is `x`, and `x_changed` to
an `eventOut` whose `effectiveName` is `x`; so `A.x_changed TO B.set_x` and
`A.x TO B.x` (both `exposedField x`) are the same connection. The key deliberately does
not use an interface-member declaration: for built-in endpoints (`origin`
`builtin-schema`) the internal acquisition record carries `member: null`, and the
published ROUTE endpoint record (`createEndpoint` in `symbols.js`) has no member field
at all, so only the DEF symbol plus `effectiveName` identifies built-in and
PROTO-interface endpoints the same way.

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
- Placement (default, pending §10): in the **root scene only**. Both endpoint DEFs
  must be bound in the root scope — not inside any PROTO body (§5.4 shared scope gate)
  — so the root scene is the smallest scope in which both names are visible. The ROUTE
  goes after the last existing top-level statement of the root scene and after both
  DEFs (ISO 4.10.2 requires the nodes to be defined before the ROUTE). If that position
  is not uniquely determined, the action refuses (`route-insertion-point-not-unique`).
  Insertion into any nested scope, including a PROTO body, is out of this slice.
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
- The §5.4 shared scope gate runs first: a ROUTE inside a PROTO body, or with an
  endpoint inside a PROTO body or mapped through `IS`, is refused before any patch is
  built.
- Once the gate passes, deleting an unresolved (non-proven) ROUTE is allowed, because
  it is a source-level operation on a node whose span is proven; the UI must say it is
  removing an unresolved ROUTE.

### 6.3. Comments, whitespace and line endings

Every byte outside the edit spans is copied verbatim by `applyEdits` (CRLF, lone LF,
lone CR, tabs, trailing whitespace, commas-as-whitespace, comments, missing final
newline). The ROUTE planner never normalizes, re-indents, or re-prints any other text.

### 6.4. Rewiring

The §5.4 shared scope gate runs first, on both the existing ROUTE and the proposed new
endpoint, before any patch is built. Rewiring an endpoint is then one user-visible
action expressed as one edit set: a
replacement of exactly the changed `node.event` token span(s) inside the existing ROUTE
statement. The rewired ROUTE must verify as `resolved` (§5.1) and must not duplicate
another existing ROUTE (§5.4); otherwise the whole action refuses and the original
ROUTE is untouched.

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

**Atomic refusal.** WD1.2 validates the complete set before producing any output,
WD1.4 refuses a set that does not exactly reproduce the planned new text, the
dispatch-validation step (§4.4) refuses `receipt-mismatch`, and `applyVerifiedEdits`
dispatches nothing when it returns `editor-read-only`, `malformed-edit-set` or
`buffer-changed`. On every one of these reachable refusal paths — planner refusal,
WD1.2/WD1.4 rejection, `receipt-mismatch`, and those three `applyVerifiedEdits`
reasons — nothing is dispatched, so the document text, the CodeMirror undo/redo
history and the selection are exactly as they were.

**Edge case: `post-dispatch-mismatch`.** If a verified set ever dispatched and the
buffer then differed from `newText` (unreachable for a verified receipt), the existing
`applyVerifiedEdits` takes the change back with `undo(view)`. The document text is then
exactly the old text again, but history is **not** untouched: the undone transaction
remains as one redo entry, and the selection is whatever CodeMirror's undo restores.
The ROUTE layer does not paper over this (it does not clear history or dispatch a second
correcting transaction); it reports the refusal, and the implementation's test plan
covers the text restoration only (§8.3).

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

Fixtures with the exact expected reason class, P2C verdict and half-proven attribute
(§4.5). "Stub" means half-proven: exactly one endpoint proven. Finding codes follow the
`semantic-findings.js` de-duplication rules (§5.3).

| Fixture | Reason class | `routeVerdict` status / reason | Half-proven stub | Finding code |
| --- | --- | --- | --- | --- |
| `ROUTE T.fraction_changed TO I.set_fraction` (TimeSensor → interpolator) | Proven | `resolved` / `ok`; in `routesFrom`/`routesTo` | no | none |
| `exposedField` via `set_`/`_changed` alias (`A.translation_changed TO B.set_translation`) | Proven | `resolved` / `ok` (alias detail is carried on the endpoint resolution, not the verdict) | no | none |
| `exposedField` by bare name (`A.translation TO B.translation`) | Proven | `resolved` / `ok` | no | none |
| Two preceding `DEF S` in the same scope, used as source | Ambiguous | `ambiguous` / `duplicate-def-in-scope`, side `source` | yes, on destination | `ROUTE_NODE_NOT_BOUND` |
| Source node never declared | Missing | `unresolved` / `def-not-declared-in-scope`, side `source` | yes, on destination | `ROUTE_NODE_NOT_BOUND` |
| Source DEF appears only after the ROUTE | Missing | `unresolved` / `route-node-not-defined-before-route`, side `source` | yes, on destination | `ROUTE_NODE_NOT_BOUND` |
| Source DEF only inside a PROTO body, ROUTE outside it | Missing | `unresolved` / `def-not-visible-across-proto-boundary` | yes, on destination | `ROUTE_NODE_NOT_BOUND` |
| Source and destination nodes both undeclared | Missing | `unresolved` / `def-not-declared-in-scope`, side `source` | no (zero proven) | `ROUTE_NODE_NOT_BOUND` ×2 |
| Unknown field on a built-in source node (`T.nonesuch`) | Missing | `unresolved` / `route-endpoint-unknown-field`, side `source` | yes, on destination | `ROUTE_ENDPOINT_NOT_BOUND` |
| `eventIn` used as source (`V.set_bind TO X.set_on`, Viewpoint `set_bind`) | Type-incompatible | `invalid` / `route-source-not-an-event-out`, side `source` | yes, on destination | `ROUTE_ENDPOINT_NOT_BOUND` |
| `eventOut` used as destination (`X.isActive TO V.isBound`) | Type-incompatible | `invalid` / `route-dest-not-an-event-in`, side `destination` | yes, on source | `ROUTE_ENDPOINT_NOT_BOUND` |
| SFColor eventOut → SFVec3f eventIn | Type-incompatible | `invalid` / `route-type-mismatch` | no (both proven) | `ROUTE_CONNECTION_REJECTED` |
| SFFloat eventOut → MFFloat eventIn | Type-incompatible | `invalid` / `route-type-mismatch` | no (both proven) | `ROUTE_CONNECTION_REJECTED` |
| EXTERNPROTO instance destination, field not in its local interface | Unsupported | `unsupported` / `externproto-interface-not-locally-verifiable` | yes, on source | `ROUTE_ENDPOINT_NOT_BOUND` |
| Destination node of an undeclared type (`DEF A Foo {}`) | Unsupported | `unresolved` / `route-endpoint-node-type-unresolved`, side `destination` (row 6) | yes, on source | `ROUTE_ENDPOINT_NOT_BOUND` |
| Source PROTO interface `eventOut SFFoo o` (unknown field type) → `I.set_fraction` | Unsupported (row 6) | `unresolved` / `route-type-unknown` | no (both endpoints proven) | `ROUTE_CONNECTION_REJECTED` |
| Stray `}` after the ROUTE: `DEF T TimeSensor {} DEF I PositionInterpolator {} ROUTE T.fraction_changed TO I.set_fraction }` | Unsupported | `recovered` / `scope-recovered` | no (zero proven) | `ROUTE_NODE_NOT_BOUND` ×2 (both node halves, status `recovered`) |

Each case asserts the reason class, the verdict, the half-proven attribute, the finding
code and its P4-B message id. Note the `route-type-unknown` row: its reason class is
Unsupported (§4.5 row 6, because the type is unknown rather than proven different), but
`semantic-findings.js` reports it as `ROUTE_CONNECTION_REJECTED`, since all four halves
resolved and only the type comparison failed. The class and the finding code are
independent outputs; the test pins both so neither is "corrected" into the other. For each non-Proven fixture used as a creation attempt, it
also asserts that the planner's refusal reason equals the P2C reason in the table. A
further case feeds a verdict with an unrecognised reason to the classifier and asserts
row 6 (Unsupported), never Proven.

Every status / reason / side and per-endpoint status in this table was checked against
the current P2C code (`buildScopeGraph` → `routeVerdict` / `resolveRouteEndpoint`) when
this design was written. If a later P2C change moves a row, that is a design defect to
resolve before UI work, not a test to adjust.

### 8.2. WD1.2 source-patch correctness (ROUTE planner)

Independent of the P2C corpus harness:

- insertion into an LF document, a CRLF document, a mixed-EOL document, and a document
  without a final newline — every byte outside the insertion
  is unchanged and the inserted line uses the detected EOL;
- deletion of a ROUTE alone on its line (line removed), of a ROUTE sharing a line with
  another statement (only its span removed), and of a ROUTE with a trailing and a
  leading comment (comments kept byte-for-byte);
- rewiring replaces only the changed `node.event` token(s);
- planner-level refusals — stale session, lone-CR-only document (insertion),
  syntax-error document, PROTO-body target, duplicate ROUTE from a create, duplicate
  ROUTE from a rewire — each refuses, and the text is unchanged (atomic refusal);
- invalid span, overlapping patches and two insertions at one offset: the planner never
  produces these, so these tests **inject the malformed edit set directly** into WD1.2
  `validateEdits`/`applyEdits` and WD1.4 `verifyTransaction`, and assert `EEDITBOUNDS`
  / `EEDITSHAPE`, `EEDITOVERLAP`, `EEDITAMBIGUOUS` (and the WD1.4 `rejected` receipt)
  with no output text produced;
- the WD1.4 receipt binds exactly `oldText` → `newText`; the token stream outside the
  edit is unchanged.

### 8.3. CodeMirror undo behavior

Through the editor handle's existing `historyDepth()` / `undo()` / `redo()` seam in
`src/editor/browser/editor-view.js` (used today for WD2-C one-step evidence):

- one create / delete / rewire adds exactly one undo step (`historyDepth`);
- Undo restores the exact previous text (string equality, CRLF fixture included);
  Redo restores the exact post-action text;
- a **pre-dispatch** refusal (any planner reason in §5.4, a WD1.2/WD1.4 rejection,
  `receipt-mismatch`, or `applyVerifiedEdits` returning `editor-read-only`,
  `malformed-edit-set` or `buffer-changed`) adds no undo or redo entry and leaves the
  buffer unchanged;
- a buffer change between plan and apply refuses with `buffer-changed` and adds no
  history entry;
- these no-history assertions do not cover `post-dispatch-mismatch`: that path
  dispatches and then undoes, so it restores the text but leaves one redo entry (§6.5).
  It is unreachable for a receipt-bound set; if a test forces it, it asserts the text
  restoration and the single redo entry, not "no history".

### 8.4. Visual display of proven and half-proven edges (Electron visual QA)

- a document with one Proven ROUTE: the edge is drawn with proven styling;
- a document with an existing ROUTE whose source node is undeclared (reason class
  Missing, half-proven): no edge is drawn; a dashed stub labelled "unresolved" appears
  on the destination node only, never in proven styling; the ROUTE is listed as Missing
  with `def-not-declared-in-scope`;
- a ROUTE with both nodes undeclared (Missing, not half-proven): no edge and no stub;
  listed as Missing;
- an SFColor → SFVec3f ROUTE (Type-incompatible, both endpoints proven, not half-proven):
  no edge and no stub; listed as Type-incompatible with `route-type-mismatch`;
- Ambiguous and Unsupported ROUTEs appear only in the unresolved list with their
  reasons, with a stub only when exactly one endpoint is proven;
- one-transaction undo/redo after a visual create, checked by byte-exact text.

### 8.5. Refusal of illegal connection creation

- a type-mismatched connection attempt is refused with the
  `ROUTE_CONNECTION_REJECTED` message; the document and history are unchanged;
- connection attempts to an ambiguous, missing, PROTO-body or EXTERNPROTO endpoint are
  refused with their reasons;
- an IS-mapped endpoint, using the fixture `PROTO P [ eventIn SFFloat f ] { DEF S
  PositionInterpolator { set_fraction IS f } DEF T TimeSensor {} ROUTE
  T.fraction_changed TO S.set_fraction }` (parses cleanly; P2C reports the inner ROUTE
  `resolved`):
  - a create attempt `T.fraction_changed → S.set_fraction` is refused **before any patch
    is built** with exactly `route-in-proto-body`: `S` and `T` are visible only inside
    P's body, so that is the create's insertion point, and `route-in-proto-body` is
    checked before `route-endpoint-in-proto-body` (gate order, §5.4);
  - a focused unit test calls the scope gate's IS check directly on `S.set_fraction`
    and asserts `route-endpoint-is-mapped`, so the IS rule is proven independently of
    the PROTO-body rule;
  - delete and rewire of the inner ROUTE are refused by the same shared scope gate with
    `route-in-proto-body`, with no patch built (text and history unchanged);
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
5. **Receipt binding location (§4.4).** Add the receipt check to `applyVerifiedEdits`
   for all callers (also closing the same gap for WD2-B/WD2-C), or keep it ROUTE-only.
6. **Planner refusal text (§5.3).** Renderer-local labels (current convention) or new
   P4-B catalog entries for ROUTE planner reasons.
7. **Sensor / TimeSensor / Interpolator editing UX.** This document covers wiring
   between them; their field editing reuses WD2-B typed inspector editing, and any
   dedicated UI needs its own owner-approved design.

## 11. Limits

This design limits the system to visual authoring and type-checking of the ROUTE
topology through exact-source patches. Execution of behaviors (Scripts), event
propagation, and timeline animation authoring remain out of scope, and none of them is
implemented by this document.
