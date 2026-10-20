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
  and it tracks a preview `generation` for the scene it is displaying.
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
> for the **current, complete parse** of the document:
>
> 1. **N** is a node whose type is the built-in VRML97 `Script` node (not a PROTO
>    instance named `Script`, and not a node inside an unparsed EXTERNPROTO body);
> 2. the parse that produced **N** is complete (no recovery stop, no node or depth
>    limit), so the node and its fields are proven;
> 3. **N** has no finding of its own from `src/vrml/semantic-findings.js` whose ISO
>    result is `PROHIBITED` (an invalid Script declaration is reported by that
>    existing finding instead — see §3.3);
> 4. **N**'s `url` field contains at least one entry, and at least one entry is in a
>    form the preview would attempt to run (§3.2).

The rule is a pure function of `(parse session, node)`. It reads no preview state, so
the same source text always produces the same advisories.

### 3.2 Supported and unsupported forms

Each `url` entry of a Script node is classified with the existing classifiers:
`isInlineScript` / `isRemote` in `src/world-project/url-fields.js` (as used by
`src/vrml/asset-refs.js`), and `src/world-project/path-policy.js` for malformed
references. No second classifier is added.

| `url` entry form | Classification | Advisory? |
| --- | --- | --- |
| `"javascript: …"` | inline-script | Yes — the code cannot execute |
| `"vrmlscript: …"` | inline-script | Yes — the code cannot execute |
| `"ecmascript: …"` | inline-script | Yes — the code cannot execute |
| Local relative file (e.g. `"behavior.js"`) | local | Yes — the file may be authorized as an asset, but its code still cannot execute |
| Remote URL (`http:`, `https:`, other schemes) | remote | No Script advisory. The existing remote-URL refusal already reports it |
| Malformed entry (path-policy `malformed`) | malformed | No Script advisory. Existing World Project reference diagnostics apply |
| Empty `url` (`url []`) or no `url` field | — | No advisory: there is no code to run |

A Script with several entries (MFString fallback list) gets **one** advisory if any
entry qualifies. The advisory names the qualifying entries by kind only. It never
copies script source text into the UI.

### 3.3 Valid but not executed vs. invalid declaration

- **Valid, not executed** (`SCRIPT_PREVIEW_NOT_EXECUTED`): severity *advisory*,
  non-fatal. It does not block save, export, packaging or preview.
- **Invalid declaration** (for example an `exposedField` in a Script interface): the
  existing error finding from `semantic-findings.js` is shown, unchanged.
  `SCRIPT_PREVIEW_NOT_EXECUTED` is **not** also shown for that node. The user must
  fix the grammar error first; an advisory that says the node is valid would be
  wrong.
- **Incomplete parse** (half-typed source, recovery stop): no Script advisory is
  emitted for any node. Advisories from the last complete parse are withdrawn, not
  carried forward (§6).

**Proposed message:** "This Script node is valid VRML97 and is saved unchanged, but
its code does not run in the WRL Forge protected preview (security policy). Events it
would send are not generated in the preview."

The final wording goes through the P4 message catalog
(`docs/white-dune-2026/P4_B_DIAGNOSTIC_MESSAGE_CATALOG.md`, `src/vrml/messages.js`,
`src/vrml/presentation.js`) and is an owner decision (§10).

## 4. Narrow suppression of follow-on preview errors

### 4.1 Default: nothing is suppressed

Implementation starts with **no** suppression. Every preview error stays visible
until each condition below is proven by a recorded spike against the bundled X_ITE
version. The design never suppresses an error because the document contains a
`Script` node.

### 4.2 Conditions for suppressing one error

An error from the preview may be hidden from the user-facing panel only if **all**
of these hold:

1. **Proven origin.** The error is the evaluation refusal caused by the protected
   preview's CSP: a `securitypolicyviolation` event with
   `violatedDirective`/`effectiveDirective` = `script-src` and
   `blockedURI` = `eval`, or the exact X_ITE error shape recorded in the spike for
   that same refusal. Text matching alone ("EvalError", "unsafe-eval", "CSP") is
   **not** sufficient.
2. **Proven node.** The error carries, or the X_ITE adapter can supply, a reference
   to the exact Script node instance in the displayed scene, and that instance maps
   to a node that currently has an active `SCRIPT_PREVIEW_NOT_EXECUTED` advisory.
3. **Same generation.** The error belongs to the preview `generation` that is
   currently displayed (`renderer/preview.js` already tracks it). An error from a
   previous generation is never matched against the current advisories.
4. **Not already the only report.** The suppressed error is folded into the advisory
   (for example "1 blocked evaluation attempt"), so the event is counted, not lost.

If any condition is missing, unknown or ambiguous, the error stays visible. This
includes errors with no source, errors whose source cannot be mapped to one node,
errors from ROUTE targets downstream of a Script, and every non-CSP runtime error
(missing texture, failed Inline, X_ITE internal error, renderer crash).

### 4.3 Open spike

Whether X_ITE exposes the Script node behind a blocked evaluation is **not yet
known**. If the spike cannot prove condition 2, suppression is dropped from the
lane and the advisory ships alone. Errors remain visible. This is the fail-closed
outcome.

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
| Several Script nodes | Each qualifying node gets its own advisory. Suppression (§4) matches each error to exactly one node or does not suppress it. |
| Unrelated runtime error while a Script is present | Shown normally. §4 never applies to it. |
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
| Suppression hides an unrelated preview error | Prevented by §4.2; tested by T7–T9. |
| An error cannot be mapped to one Script node | Error stays visible (T8). |
| An error from an older preview generation | Error stays visible, never matched (T9). |
| Advisory shown for an invalid Script | Prevented by §3.3 (T3). |
| Advisory attached to the wrong node after edits | Prevented by §5; identity refusal re-issues (T12). |
| Advisory spam on every keystroke | Advisories are a recomputed set per parse (T11). |
| Parser starts rejecting valid Scripts | Out of scope; parser and WD1.2 span patches are untouched. Existing parser tests stay green. |

## 9. Test plan

All proposed tests are deterministic and need no network.

### 9.1 Focused tests (proposed: `test/vrml/script-diagnostic.test.js`, plus a preview filter test under `test/preview/`)

| ID | Input | Expected |
| --- | --- | --- |
| T1 | Valid Script, `url "javascript: function f(){}"` | One `SCRIPT_PREVIEW_NOT_EXECUTED`, non-fatal |
| T2 | Same with `vrmlscript:` and with `ecmascript:` | One advisory each |
| T3 | Script with an `exposedField` in its interface | Existing grammar error only; no advisory |
| T4 | Script with only `url "http://example.com/s.js"` | No advisory; remote refusal unchanged |
| T5 | Script with `url []` / no `url` | No advisory |
| T6 | Two Scripts, one inline and one local `"b.js"` | Two advisories, one per node |
| T7 | CSP eval violation correlated to flagged node, current generation | Folded into that node's advisory |
| T8 | CSP eval violation with no node / ambiguous node | Visible |
| T9 | Correlated violation from an older generation; unrelated texture error; generic error text containing "EvalError" | All visible |
| T10 | Read `editor.html`, `world.html`, `index.html` | `script-src` has no `unsafe-eval` |
| T11 | Apply ten edits inside one Script body | Advisory count stays 1 |
| T12 | Duplicate DEF Scripts; rename DEF; anonymous Script across reload | No UI state carried; fresh advisories only |
| T13 | Remove Script; undo; redo | Advisory gone, back, gone; source text byte-identical to before each step |
| T14 | Truncate document mid-Script (incomplete parse) | No Script advisories |

### 9.2 Electron visual QA (Linux first, macOS 15+ second)

Uses the existing `test/visual/` harness and `docs/VISUAL_QA_SAFETY.md` rules.

1. Open a fixture with one valid inline Script. The advisory is visible; the preview
   renders the rest of the scene; no unrelated error is hidden.
2. Fixture with a Script plus a missing texture. Both the advisory and the texture
   warning are visible.
3. Delete the Script, then undo and redo. The advisory follows each step; no
   duplicate entries.
4. Fixture with two Scripts. Two advisories, each focusing its own node.

### 9.3 Independent security review criteria

- Zero diff to CSP meta tags, Electron `webPreferences`, preload/IPC, URL policy,
  `preview-source.js` and network blocking.
- No new remote origin, no `eval`/`new Function`, no script interpreter dependency.
- Suppression code (if any ships) is reviewed against §4.2 with T7–T9 evidence.

## 10. Open owner decisions

1. Final advisory wording and whether it appears in the Problems panel, the preview
   warnings box (`previewWarnings`) or both.
2. Whether a document-level summary line is shown in addition to the per-node
   advisories.
3. Whether follow-on suppression (§4) is in scope at all, or whether the lane ships
   the advisory alone. Suppression requires the §4.3 spike regardless.
4. Whether a local-file Script `url` should also be shown as an authorized asset in
   the World Project view (today's classifier decides; this design does not change it).

## 11. Dependencies

- P4 diagnostics pipeline: `src/vrml/messages.js`, `src/vrml/presentation.js`,
  `docs/white-dune-2026/P4_B_DIAGNOSTIC_MESSAGE_CATALOG.md`.
- Identity: `src/vrml/node-identity.js` (`docs/white-dune-2026/WD1_4_NODE_IDENTITY.md`).
- URL classification: `src/vrml/asset-refs.js`, `src/world-project/path-policy.js`,
  `src/world-project/url-fields.js`.
- Preview generation tracking: `renderer/preview.js`.

## 12. Limits and review status

- Design only. No production code, dependency, configuration, CI or CSP change.
- §4 depends on an X_ITE spike that has not been run.
- Owner review is **not complete**. Draft — proposal for owner review; not approved.
