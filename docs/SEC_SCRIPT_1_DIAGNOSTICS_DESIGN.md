# SEC-SCRIPT-1: Protected-preview Script diagnostics design

**Issue:** [#76](https://github.com/Ascendance3D/wrlforge/issues/76)
**Parent:** [#75](https://github.com/Ascendance3D/wrlforge/issues/75)
**Status:** Draft — proposal for owner review; not approved

This document is a design proposal. Merging it does not authorize implementation;
the SEC-SCRIPT-1 lane starts only on an explicit owner GO.

## 1. Background (current, verified behavior)

WRL Forge previews VRML97 in an embedded X_ITE renderer behind a protected page.
The facts below are the current repository state; this design changes none of them.

- `renderer/editor.html`, `renderer/world.html` and `renderer/index.html` each carry a
  `Content-Security-Policy` meta tag whose `script-src` is
  `'self' file: 'wasm-unsafe-eval'`. There is no `unsafe-eval`, so string evaluation
  (`eval`, `new Function`) is blocked in the preview page.
- `docs/PREVIEW_ARCHITECTURE.md` and `docs/WORLD_PROJECT_ARCHITECTURE.md` record that
  inline `vrmlscript:` is never evaluated because the CSP has no `unsafe-eval`.
- `src/world-project/path-policy.js` (`INLINE_SCRIPT`), `src/world-project/url-fields.js`
  (`SCRIPT_SCHEMES` = `vrmlscript`, `javascript`, `ecmascript`) and
  `src/vrml/asset-refs.js` (`'inline-script'`) already classify Script code URLs. They
  are never treated as file dependencies.
- `src/vrml/parser.js` parses `Script` nodes. `src/vrml/semantic-findings.js` already
  reports grammar violations inside a Script interface (for example
  `EXPOSED_FIELD_IN_SCRIPT_INTERFACE`, cited as `GRAMMAR_SCRIPT_INTERFACE`, ISO A.3).
- `renderer/preview.js` wraps `console.error` only to collect texture-load warnings,
  and it tracks a preview `generation` for the scene it is displaying. The wrapper's
  regex is broad (`Failed to (load|fetch)`, `could not`, …), so a failed fetch of a
  local Script file could currently be filed as a "Texture:" warning. This design
  does not change the wrapper; visual QA case 5 (§9.2) records what actually happens.
- WD2-D picking (`src/preview/xite-pick-adapter.js`,
  `docs/white-dune-2026/WD2_D_XITE_PICKING_CLOSEOUT.md`) binds X_ITE runtime nodes to
  exact authored occurrences through parse-local provenance tied to one preview
  generation **and its exact source text**, with no heuristic fallback (DEF name,
  type, sibling index, offset). Today that provenance covers Shape selection only.
- `src/vrml/node-identity.js` provides Tier 0 (current-parse selection), Tier 1
  (transaction anchor) and Tier 2 (persistent DEF anchor) identity, and refuses when
  identity cannot be proven.

Today a valid Script node can leave the preview silently inert, or produce a generic
renderer error about blocked evaluation. The user cannot tell an expected security
limit from a real defect in their file.

## 2. Goal and non-goals

**Goal.** Show an accurate, non-fatal advisory for each valid `Script` node whose code
cannot run in the protected preview. Keep every unrelated error visible.

**Non-goals.**

- No Script execution of any kind (JavaScript, ECMAScript or VRMLScript).
- No CSP, `webPreferences`, IPC, filesystem-authorization or network-policy change.
- No source-text change. The advisory is read-only metadata; it never patches,
  instruments or annotates the document.
- No new parser. The advisory consumes the existing parse and the existing URL
  classifiers.

## 3. Diagnostic trigger (one deterministic rule)

### 3.1 The rule

> Emit `SCRIPT_PREVIEW_NOT_EXECUTED` for a node **N** if and only if all of these hold
> for the **current parse** of the document:
>
> 1. **N** is a node whose type is the built-in VRML97 `Script` node (not a PROTO
>    instance named `Script`, and not a node inside an unparsed EXTERNPROTO body).
>    **N** may sit in the main scene or inside a PROTO body in this document; it
>    must be in the document being edited, never in a file loaded through `Inline`
>    (§6);
> 2. the parse that produced **N** is complete. Concretely, for the result of
>    `parse()` in `src/vrml/parser.js`: `truncated` is false, `depthCapped` is false,
>    `diagnostics` contains no entry with severity `error` (`SEVERITY.ERROR` in
>    `src/vrml/diagnostics.js`), and no AST node in the tree has `incomplete: true`.
>    This is document-wide: **any** recoverable parser error anywhere in the file
>    withdraws **every** Script advisory in that document, not just those near the
>    error;
> 3. **N** has no finding of its own from `src/vrml/semantic-findings.js` with any
>    ISO result (`PROHIBITED`, `UNDEFINED`, `NOT_STATED` or any other). A finding
>    `f` is **N**'s own if and only if `f.subject.node === N`, or `f.subject.node` is
>    an element of `N.interfaces` (the Script's interface declarations), or
>    `f.subject.parent === N`. The middle case is required: for
>    `EXPOSED_FIELD_IN_SCRIPT_INTERFACE`, `interfaceDeclarationFindings` sets
>    `subject.node` to the interface declaration (`member.node = decl` in
>    `addMember`, `src/vrml/scope-graph.js`), not to the Script. A Script with any
>    own finding is reported by that existing finding instead — see §3.3;
> 4. **N**'s own `url` field contains at least one entry, and at least one entry is
>    in a form that receives the advisory (§3.2).

The rule is a pure function of `(parse session, node)`. It reads no preview state,
no filesystem, no project root and no referrer directory, so the same source text
always produces the same advisories.

### 3.2 Supported and unsupported forms

**Authoritative classifier:** the `kind` of each record returned by the exported
`extractAssetRefs(tree)` in `src/vrml/asset-refs.js`. (Its internal `classify()` is
not exported; it uses `isInlineScript` / `isRemote` from
`src/world-project/url-fields.js`.) The rule takes the records whose `range` lies
inside **N**'s own `url` field. `kind` reads only the string, so the rule stays
pure. It is one of `inline-script`, `remote` or `local`. `extractAssetRefs` already
skips empty strings. No second classifier is added.

`src/world-project/path-policy.js` (`classifyReference`) is **not** used by this rule,
because it needs a referrer directory and project root. Its separate
`traversal` / `absolute-local` diagnostics still appear through the World Project
view, unchanged.

| `url` entry form | Classification | Advisory? |
| --- | --- | --- |
| `"javascript: …"` | inline-script | Yes — the code cannot execute |
| `"vrmlscript: …"` | inline-script | Yes — the code cannot execute |
| `"ecmascript: …"` | inline-script | Yes — the code cannot execute |
| Local relative file (`"behavior.js"`) | local | Yes, **conditional on §4.3 / T15** — see the note below |
| Parent-relative path (`"../x.js"`) | local (path-policy separately reports `traversal` when it escapes the project) | Same as the row above |
| Absolute path (`"/x.js"`) | local (path-policy separately reports `absolute-local`) | Same as the row above |
| `file:///…` URL | local (`isRemote` treats `file:` as not remote; path-policy reports `absolute-local`) | Same as the row above |
| Network URL (`http:`, `https:`, `ws:`, `wss:`, `ftp:`, protocol-relative `//host/…`) | remote | No Script advisory. The preview blocks the request (`isBlockedPreviewUrl`) and `scanRemoteUrls` (`src/preview/url-policy.js`) reports it as a blocked remote URL |
| `data:`, `blob:`, `wrlworld:`, `about:` (also `devtools:`, `chrome:`, `chrome-extension:`) | remote (`isRemote` treats every non-`file:` scheme as remote) | **No Script advisory, and not reported anywhere today.** `scanRemoteUrls` reports only http(s)/ws(s)/ftp and `//`, and `isBlockedPreviewUrl` **allows** these schemes (`ALLOWED_SCHEMES`). Whether a Script `url` in one of these schemes executes is **unverified** — §4.3 item 3 and required test T15 must prove it does not. Open decision 6 |
| Other non-`file:` scheme (e.g. `mailto:`, `foo:`) | remote | No Script advisory. The preview blocks the request (not in `ALLOWED_SCHEMES`), but `scanRemoteUrls` does not report it |
| Windows drive path (`"C:\x.js"`, `"C:/x.js"`) | remote (`schemeOf` reads `c` as a scheme) | No Script advisory. This fails safe: the preview blocks the request (`c` is not in `ALLOWED_SCHEMES`). `path-policy.js` separately reports it as `absolute-local` |
| Empty string entry | skipped by `asset-refs.js` | Ignored |
| Empty `url` (`url []`) or no `url` field | — | No advisory: there is no code to run |

**Unverified assumption (local-file rows).** That a Script whose `url` names a local
file does **not** execute in the protected preview is an X_ITE property this design
has not verified. `script-src 'self' file:` permits loading script files from `file:`;
whether X_ITE loads a Script file through a path that CSP blocks (string evaluation)
or one it allows is unknown. Until the §4.3 spike and test T15 prove non-execution,
the advisory for local-file rows must not claim the code "does not run". If T15
shows the code **can** run, that is a separate security finding, reported to the
owner through `SECURITY.md`. It is not a reason to change the CSP in this lane.

The same applies to Script `url` values in the `data:`, `blob:`, `wrlworld:` and
`about:` schemes (and the other `ALLOWED_SCHEMES` entries): the preview's request
policy lets them through, and CSP `script-src 'self' file:` does not list `data:` or
`blob:`, but whether X_ITE can turn any of them into running code is unverified. T15
covers them; a positive result is a separate `SECURITY.md` finding, with no CSP
change in this lane.

A Script with several entries (MFString fallback list) gets **one** advisory if any
entry qualifies. The advisory names the qualifying entries by kind only. It never
copies script source text into the UI.

### 3.3 Valid but not executed vs. invalid declaration

- **Valid, not executed** (`SCRIPT_PREVIEW_NOT_EXECUTED`): severity *advisory*,
  non-fatal. It does not block save, export, packaging or preview.
- **Invalid declaration** (for example an `exposedField` in a Script interface): the
  existing semantic finding is kept and presented through the existing route,
  unchanged: `findingsForDocument(graph)` (`src/vrml/semantic-findings.js`) →
  `presentDocumentFindings(findings)` (`src/vrml/presentation.js`) →
  `messageForPresentation(result)` (`src/vrml/messages.js`). This lane adds nothing to
  that route for invalid Scripts. `SCRIPT_PREVIEW_NOT_EXECUTED` is **not** also shown
  for that node. The user must fix the finding first.
- **Incomplete parse** (half-typed source, recovery stop): no Script advisory is
  emitted for any node. Advisories from the last complete parse are withdrawn, not
  carried forward (§6).

**Proposed message:** "This Script node is saved unchanged, but its code does not run
in the WRL Forge protected preview (security policy). Events it would send are not
generated in the preview." The message makes no validity claim; clause 3 already
withholds it from any Script that has an ISO finding.

The final wording goes through the P4 message catalog
(`docs/white-dune-2026/P4_B_DIAGNOSTIC_MESSAGE_CATALOG.md`, `src/vrml/messages.js`,
`src/vrml/presentation.js`) and is an owner decision (§10).

### 3.4 Advisory data flow (proposed)

Every name marked *(proposed)* does not exist yet.

1. **Producer.** A new function next to `findingsForDocument` in
   `src/vrml/semantic-findings.js`, `scriptPreviewFindings(session, graph)`
   *(proposed)*. It takes the current parse session and its scope graph and applies
   §3.1 to each Script node. It is a separate function because `findingsForDocument`
   only reports ISO facts (`iso` is derived from `ISO_BY_REASON`), and this advisory
   is not one.
2. **Record.** One frozen record per qualifying node, built with the existing
   `createFinding` shape so the presentation chain accepts it unchanged:
   - `code`: `FINDING_CODE.SCRIPT_PREVIEW_NOT_EXECUTED` *(proposed)*;
   - `subject`: `createSubject({ node: N })`;
   - `range`: **N**'s parser range;
   - `iso`: `ISO_RESULT.NOT_STATED` (the advisory asserts nothing about the
     standard), `rule`: `null`;
   - `confidence`: `STATUS.RESOLVED`; `reason`: `REASON.SCRIPT_PREVIEW_NOT_EXECUTED`
     *(proposed)*;
   - `detail`: `{ urlKinds }` *(proposed)*, the sorted distinct `kind` values
     (`inline-script`, `local`) of **N**'s qualifying `url` entries from
     `extractAssetRefs`. The message uses it to word local-file advisories per §3.2
     until T15 passes. `evidence`: empty.
3. **Presentation.** The caller concatenates the result with `findingsForDocument`'s
   array and passes it through the existing chain: `presentDocumentFindings` →
   `messageForPresentation`. This needs a `GROUP_BY_FINDING_CODE` entry in
   `src/vrml/presentation.js` and a message entry in `src/vrml/messages.js` and the
   P4 catalog *(all proposed)*. No new presentation path is added.
4. **Recompute.** The producer runs on every new parse session. Its output replaces
   the previous set; nothing is accumulated or stored in undo history. Identity and
   dedup follow §5.

## 4. Follow-on preview errors: not suppressed in this lane

### 4.1 This lane ships the advisory only

The SEC-SCRIPT-1 implementation lane ships the §3 advisory **only**. It suppresses
no preview error. Every preview error stays visible, and nothing in this lane
depends on suppression.

Suppressing follow-on errors is a **later, separately approved step**. It needs its
own design proposal, and that proposal must define the full data flow before any
code: where the CSP refusal is captured (the producer), what record the X_ITE
adapter produces, what payload reaches the renderer and with which named fields,
how it reaches the advisory store, and where the filter runs. None of that is
defined here. Never suppress an error because the document contains a `Script` node.

### 4.2 Entry criteria for any future suppression proposal

A future proposal may hide a preview error from the user-facing panel only if it
proves that **all** of these hold for that error:

1. **Proven origin.** The error is the evaluation refusal caused by the protected
   preview's CSP: a `securitypolicyviolation` event with
   `violatedDirective`/`effectiveDirective` = `script-src` and
   `blockedURI` = `eval`, or the exact X_ITE error shape recorded in the spike for
   that same refusal. Text matching alone ("EvalError", "unsafe-eval", "CSP") is
   **not** sufficient.
2. **Same source text.** The displayed preview generation was built from source
   text byte-identical to the text of the current parse that holds the advisories.
   Nothing else substitutes for this equality: a Tier 1 transaction receipt
   re-anchors parse nodes across an edit but does not prove what the runtime scene
   was built from. When the preview is showing a last-good scene after a parse
   error, or the editor text has changed since the preview was built, the texts
   differ and suppression is refused.
3. **Same generation.** The error belongs to the preview `generation` that is
   currently displayed (`renderer/preview.js` already tracks it). An error from a
   previous generation is never matched against the current advisories.
4. **Proven node.** The error carries, or the X_ITE adapter can supply, the exact
   runtime Script node instance, and that instance maps to exactly one parse node
   through WD2-D-style parse-local provenance (runtime identity plus exact source
   span) captured while parsing that same byte-identical text for that same
   generation (criteria 2–3). The parse node is looked up in the current parse
   only because the texts are equal; no Tier 1 or Tier 2 anchor is used to bridge
   a difference. Position, document order, DEF name, node type and field values are
   **not** used as identity, in line with WD2-D. Extending that provenance to Script nodes would be part of that
   future proposal and needs §4.3 items 1–2. The mapped node must have an active
   `SCRIPT_PREVIEW_NOT_EXECUTED` advisory.
5. **Exactly one occurrence.** A Script inside a PROTO body exists once in source
   but once per instance at runtime. An error from such a Script cannot name one
   authored occurrence and stays visible.
6. **Counted, not lost.** The suppressed error is folded into the advisory (for
   example "1 blocked evaluation attempt"), so the event is still counted.

If any criterion is missing, unknown or ambiguous, the error stays visible. This
includes errors with no source, errors whose source cannot be mapped to one node,
errors from ROUTE targets downstream of a Script, and every non-CSP runtime error
(missing texture, failed Inline, X_ITE internal error, renderer crash).

### 4.3 Open spike

The spike runs against the pinned Electron X_ITE (`15.1.10`).

**Required for this lane:**

3. how X_ITE loads a Script `url` that is a local file (`"behavior.js"`,
   `"../x.js"`, `"/x.js"`, `file:///…`) or uses an allowed non-file scheme
   (`data:`, `blob:`, `wrlworld:`, `about:`), and whether any of them executes under
   the current CSP (the §3.2 unverified assumptions, proven by T15).

**Input to a future suppression proposal only** (not a dependency of this lane):

1. whether X_ITE exposes the runtime Script node behind a blocked evaluation, and
   whether WD2-D provenance can bind it to one exact source span (criteria 2–4);
2. the exact error/event shape of the CSP refusal (criterion 1).

If items 1–2 cannot be proven, no suppression proposal is made. Errors remain
visible. This is the fail-closed outcome, and it is also this lane's behavior.

## 5. Diagnostic identity and deduplication

- **Scope.** One advisory per proven Script node. A document-level summary line
  ("2 Script nodes do not run in the preview") may be derived from the per-node set,
  but the per-node advisory is the unit for presentation, dedup and tests.
- **Key within one parse.** The node's Tier 0 current-parse selection. Advisories are
  recomputed from the current parse; they are not accumulated. Re-running the rule on
  every editor update therefore produces a set of advisories, not a growing list.
- **Key across updates.** To keep UI state (such as a dismissed or expanded advisory)
  across reparses:
  - after an edit with a transaction receipt, the Tier 1 transaction anchor carries
    the advisory to the same node;
  - across reloads or unknown edits, only a Tier 2 persistent anchor (unique DEF name,
    node type, provable scope) carries it;
  - an anonymous Script, a duplicate DEF, a renamed DEF, a scope change or a refused
    anchor carries **nothing**: the old UI state is dropped and a fresh advisory is
    issued for whatever node now qualifies.
- **Never follow another node.** Position, document order, field values and source
  ranges are not identity. If identity is not proven, the advisory is re-issued, not
  re-attached.

## 6. Editing lifecycle

All advisories derive from the current complete parse, so each case below follows
from §3 and §5.

| Event | Behavior |
| --- | --- |
| Script node removed | Its advisory disappears on the next complete parse. Carried UI state is dropped. |
| `url` changed to another qualifying form | The advisory stays; identity follows §5. |
| `url` changed to only remote, empty or malformed entries | The advisory disappears; existing asset/remote diagnostics apply. |
| Script becomes invalid (grammar error) | The advisory is replaced by the existing error finding (§3.3). |
| Parse becomes incomplete | All Script advisories are withdrawn until a complete parse returns. No stale advisory is shown against unparsed text. |
| Undo / redo restores earlier source | The rule re-runs on the restored text and produces the same advisories as before for the same text. Undo restores source only; diagnostics are never stored in the undo stack. |
| Several Script nodes | Each qualifying node gets its own advisory. No preview error is suppressed (§4). |
| Unrelated runtime error while a Script is present | Shown normally. §4 never applies to it. |
| Script inside a PROTO body, several instances | One advisory on the single authored Script in the PROTO body. Runtime errors from its instances stay visible; this lane suppresses nothing, and any future suppression must treat them as ambiguous (§4.2 criterion 5). |
| Script in a file loaded through `Inline` | No advisory in the parent document; that file is not the parse being edited. Its runtime errors stay visible. Opening the Inline target as its own document gives it its own advisories. (Open decision 5.) |
| Document replaced, reopened or recovered | Advisories are recomputed; carried state follows only Tier 2 rules. |
| Feature reverted (code removed) | Source text, parsed Script nodes and preview security settings are unchanged, because the feature never wrote any of them. |

## 7. Security invariants

- `script-src` stays `'self' file: 'wasm-unsafe-eval'` in `editor.html`, `world.html`
  and `index.html`. No `unsafe-eval` and no `unsafe-inline` for scripts are added.
- No JavaScript, ECMAScript or VRMLScript execution path is added: no interpreter,
  sandbox, worker evaluator or WASM evaluator.
- `contextIsolation`, `nodeIntegration`, preload/IPC surface, `wrlworld:` authorization
  (`src/world-project/preview-source.js`) and remote-request blocking stay unchanged.
- The advisory reads parse data only. It does not fetch the Script `url`, read the
  referenced local file or copy script text into the UI.
- Suppression never hides a non-CSP error (§4.2).

## 8. Threat and failure cases (fail-closed)

| Case | Required outcome |
| --- | --- |
| A change relaxes the CSP to allow `unsafe-eval` | A focused test fails (§9.1, T10). |
| A local-file Script `url` executes under `script-src 'self' file:` | Unverified; T15 is required. If it executes, report a separate security finding; no CSP change in this lane. |
| A `data:`/`blob:`/`wrlworld:`/`about:` Script `url` executes (these schemes pass `isBlockedPreviewUrl` and get no advisory) | Unverified; T15 is required. If it executes, report a separate `SECURITY.md` finding; no CSP change in this lane. |
| Suppression hides an unrelated preview error | Prevented by §4.2; tested by T7–T9, T17 and T18. |
| An error cannot be mapped to one Script node | Error stays visible (T8). |
| An error from an older preview generation | Error stays visible, never matched (T9). |
| Advisory shown for an invalid Script | Prevented by §3.3 (T3). |
| Advisory attached to the wrong node after edits | Prevented by §5; identity refusal re-issues (T12). |
| Advisory spam on every keystroke | Advisories are a recomputed set per parse (T11). |
| Parser starts rejecting valid Scripts | Out of scope; parser and WD1.2 span patches are untouched. Existing parser tests stay green. |

## 9. Test plan

All proposed tests are deterministic and need no network.

### 9.1 Focused tests (proposed: `test/vrml/script-diagnostic.test.js`)

Tests marked *future* belong to a separately approved suppression proposal (§4) and
are not part of this lane. In this lane, the same inputs must leave every error
visible.

| ID | Input | Expected |
| --- | --- | --- |
| T1 | Valid Script, `url "javascript: function f(){}"` | One `SCRIPT_PREVIEW_NOT_EXECUTED`, non-fatal |
| T2 | Same with `vrmlscript:` and with `ecmascript:` | One advisory each |
| T3 | Script with an `exposedField` in its interface | Existing grammar error only; no advisory |
| T4 | Script with only `url "http://example.com/s.js"` | No advisory; remote refusal unchanged |
| T5 | Script with `url []` / no `url` | No advisory |
| T6 | Two Scripts, one inline and one local `"b.js"` | Two advisories, one per node. Gated on T15: until T15 passes, the local-file advisory's message must not claim the code does not run (§3.2); the test asserts that wording |
| T7 (future) | CSP eval violation correlated to flagged node, current generation | Folded into that node's advisory |
| T8 (future) | CSP eval violation with no node / ambiguous node | Visible |
| T9 (future) | Correlated violation from an older generation; unrelated texture error; generic error text containing "EvalError" | All visible |
| T10 | Read `editor.html`, `world.html`, `index.html` | `script-src` has no `unsafe-eval` |
| T11 | Apply ten edits inside one Script body | Advisory count stays 1 |
| T12 | Duplicate DEF Scripts; rename DEF; anonymous Script across reload | No UI state carried; fresh advisories only |
| T13 | Remove Script; undo; redo | Advisory gone, back, gone; source text byte-identical to before each step |
| T14 | Truncate document mid-Script (incomplete parse) | No Script advisories |
| T15 (required) | In the protected preview page with the production CSP, a Script whose `url` is (a) a local file — `"behavior.js"`, `"../x.js"`, `"/x.js"`, `file:///…` — or (b) a `data:` URL (e.g. `data:text/javascript,…`), a `blob:` URL, a `wrlworld:` URL to a `.js` file, or an `about:` URL, each carrying code with an observable side effect | The side effect never occurs in any case. If it occurs, file a separate `SECURITY.md` finding; the CSP is not changed in this lane |
| T16 | Script with an `UNDEFINED` or `NOT_STATED` finding | Existing finding only, presented via `presentDocumentFindings` / `messageForPresentation`; no advisory |
| T17 (future) | Correlated violation while the preview shows a last-good scene built from older text | Visible (criterion 2) |
| T18 | Script in a PROTO body with two instances | One advisory. *(Future:* a correlated violation stays visible, criterion 5*)* |
| T19 | Script with `EXPOSED_FIELD_IN_SCRIPT_INTERFACE` (finding subject is the interface declaration) | No advisory (ownership via `N.interfaces`); existing finding presented unchanged |
| T20 | Valid Script plus an unrelated recoverable parser error elsewhere in the file | No Script advisories anywhere in the document |
| T21 | Script `url "C:\\x.js"` | No advisory; preview request blocked |
| T22 | Parent document with `Inline { url "child.wrl" }`, where `child.wrl` contains a valid inline Script | No advisory in the parent's findings; the parent parse contains no Script node |
| T23 | The same `child.wrl` parsed as its own document | One advisory for its Script |

### 9.2 Electron visual QA (Linux first, macOS 15+ second)

Uses the existing `test/visual/` harness and `docs/VISUAL_QA_SAFETY.md` rules.

1. Open a fixture with one valid inline Script. The advisory is visible; the preview
   renders the rest of the scene; no unrelated error is hidden.
2. Fixture with a Script plus a missing texture. Both the advisory and the texture
   warning are visible.
3. Delete the Script, then undo and redo. The advisory follows each step; no
   duplicate entries.
4. Fixture with two Scripts. Two advisories, each focusing its own node.
5. Fixture with a Script whose `url` names a missing local `.js` file. Record whether
   the texture wrapper files the failure as a "Texture:" warning, and report it if it
   does.
6. World Project whose primary file Inlines a child `.wrl` containing a Script. The
   primary document shows no Script advisory. Opening the child as its own document
   shows its own advisory. Preview errors from either stay visible.

### 9.3 Independent security review criteria

- Zero diff to CSP meta tags, Electron `webPreferences`, preload/IPC, URL policy,
  `preview-source.js` and network blocking.
- No new remote origin, no `eval`/`new Function`, no script interpreter dependency.
- This lane adds no suppression code; any change that hides a preview error is out of
  scope and blocks the review. T15 is required before any local-file advisory ships.
- A future suppression proposal is reviewed against §4.2 with T7–T9, T17 and T18
  evidence.

## 10. Open owner decisions

1. Final advisory wording and whether it appears in the Problems panel, the preview
   warnings box (`previewWarnings`) or both.
2. Whether a document-level summary line is shown in addition to the per-node
   advisories.
3. Whether to open a separate suppression proposal (§4) after this lane. This
   lane ships the advisory alone either way.
4. Whether a local-file Script `url` should also be shown as an authorized asset in
   the World Project view (today's classifier decides; this design does not change it).
5. Whether Scripts inside `Inline`-loaded files should ever be summarized in the
   parent document, for example as a count with no per-node advisory.
6. Whether Script `url` entries in `data:`, `blob:`, `wrlworld:` or `about:` schemes
   should get a notice of their own. Today they are classified `remote`, receive no
   advisory and are not reported by `scanRemoteUrls`. This design adds no second
   classifier; any notice for them is a separate owner decision.

## 11. Dependencies

- P4 diagnostics pipeline: `src/vrml/messages.js`, `src/vrml/presentation.js`,
  `docs/white-dune-2026/P4_B_DIAGNOSTIC_MESSAGE_CATALOG.md`.
- Identity: `src/vrml/node-identity.js` (`docs/white-dune-2026/WD1_4_NODE_IDENTITY.md`).
- URL classification: `src/vrml/asset-refs.js`, `src/world-project/path-policy.js`,
  `src/world-project/url-fields.js`.
- Preview generation tracking: `renderer/preview.js`.

## 12. Limits and review status

- Design only. No production code, dependency, configuration, CI or CSP change.
- The local-file and allowed-scheme rows of §3.2 depend on the §4.3 spike (item 3)
  and test T15, which have not been run. Suppression (§4) is out of this lane.
- Owner review is **not complete**. Draft — proposal for owner review; not approved.
