# WD2-J — PROTO and EXTERNPROTO Visual Workflow Design

**Lane:** WD2-J (parent, #86), WD2-J-R (design sub-issue, #87) · **Status:** Draft — proposal for owner review; not approved. **DESIGN / INVESTIGATION ONLY** — no production code changes are authorized by this document, and merging it does not authorize implementing any phase below.

## 1. Goal

Design read-only inspection of PROTO interfaces, PROTO instances and EXTERNPROTO declarations first, followed by narrowly and separately gated edits. Establish the fail-closed identity rules, source-patch shape, undo contract and test plan before any implementation begins.

## 2. Scope

### In scope (design only)
- Phase J0: read-only inspection of PROTO interfaces, PROTO instances and EXTERNPROTO declarations.
- Phase J1: separately gated instance-field edits on PROTO instances.
- Phase J2: separately gated interface **default-value** edits on local PROTO declarations.

### Out of scope
- Fetching EXTERNPROTO URLs over the network. Remote definitions are never fetched (§7).
- Inventing interface data an EXTERNPROTO does not declare locally (§7).
- PROTO body editing (deferred; needs its own design).
- Interface **type, access-type or name** changes, and adding/removing interface declarations. These can invalidate existing instances, `IS` connections and ROUTEs; they refuse until a separately approved migration design exists (§5.4).
- Any change to preview security, IPC/CSP or filesystem authorization.

## 3. Architecture constraints and contracts

This design preserves the core WRL Forge contracts:

- **Exact source text** (`WD.md` §2; WD1.2 contract in [`src/vrml/edit.js`](../../src/vrml/edit.js)): the canonical document is the source text. Every edit is a WD1.2 `{from, to, insert}` patch in UTF-16 code units, half-open `[from, to)`. Edits follow WD1.2 validation (`validateEdits`): no overlap, no clamping, no repair; malformed sets throw rather than being massaged. No AST-to-text regeneration. No second editable document model or canonical scene graph.
- **Verified transaction:** each planned edit set is checked with `verifyTransaction({ oldText, edits, newText })` (`src/vrml/document-transaction.js`) before dispatch, exactly as WD2-B does (`WD2_B_TYPED_INSPECTOR_EDITING.md` §5).
- **One undo transaction:** one user-visible action is one `applyVerifiedEdits` call — one `view.dispatch` with `isolateHistory.of('full')` (`src/editor/browser/editor-view.js`). One Undo restores the exact prior text; one Redo restores the exact post-edit text. No semantic undo stack.
- **No hidden instrumentation:** no hidden source instrumentation, synthetic identifiers or sidecar semantic state.
- **Fail-closed identity** (`WD.md` §7): unsupported or ambiguous identity refuses with a stable reason id. The tool never picks a candidate.
- **One selection authority:** the inspector uses the existing shared selection authority (WD1.4 node identity).
- **ISO 4.8.4 scope disjointness:** PROTO body scopes stay disjoint from the enclosing scope, as built in WD1.5 (`src/vrml/scope-graph.js`).
- **Interface authority:** interfaces are read only through `effectiveInterfaceOf` (`src/vrml/interface-query.js`), which delegates every resolution decision to `scope-graph.js`. This design adds no second resolver.
- **Safety boundaries:** X_ITE preview, IPC/CSP and filesystem authorization contracts are unchanged.

## 4. Phase J0 — read-only inspection

Inspection never modifies the buffer. Every displayed value is either a source token span from the current parse or an explicit "not available" state; nothing is defaulted, synthesized or fetched.

### 4.1 Access types

All four VRML97 access types (`ACCESS` in `src/vrml/symbols.js`) are displayed. Per ISO/IEC 14772-1 §4.9, only `field` and `exposedField` carry a value in a PROTO declaration or on an instance; events carry none.

| Access | PROTO interface row | PROTO instance row | J1 instance edit | J2 default edit |
|---|---|---|---|---|
| `field` | name, type, default value (source span) | authored value; otherwise "default" with the declared default shown read-only (PROTO), or "default — not declared locally" (EXTERNPROTO) | Eligible (gated, §5.2) | Eligible (gated, §5.3; local PROTO only) |
| `exposedField` | name, type, default value (source span) | authored value; otherwise "default" with the declared default shown read-only (PROTO), or "default — not declared locally" (EXTERNPROTO) | Eligible (gated, §5.2) | Eligible (gated, §5.3; local PROTO only) |
| `eventIn` | name, type, "event — no value" | listed as an input event; no value cell | Never — no value exists | Never — no default exists |
| `eventOut` | name, type, "event — no value" | listed as an output event; no value cell | Never — no value exists | Never — no default exists |

An authored value on an instance for an `eventIn`/`eventOut` member is a document error. No diagnostic for this case exists in `src/vrml` today (the nearest ids, `is-access-incompatible` and `field-name-is-event-alias`, cover different cases); a dedicated diagnostic is **proposed** for the implementation lane. Until it exists, the row is displayed as authored and is read-only.

### 4.2 PROTO interfaces
Show each declared member (name, type, access, default where §4.1 allows) and, for a body node field bound with `IS`, the resolved interface member from `resolveIs`. An `IS` binding that does not resolve is shown with its scope-graph status and reason, never re-guessed.

### 4.3 PROTO instances
Show the effective interface from `effectiveInterfaceOf` (`sourceOrigin` = `proto-interface` or `externproto-interface`) with authored values and their spans. Interface status `ambiguous`, `unresolved`, `invalid` or `unsupported` makes the whole instance view read-only with that status shown.

For an EXTERNPROTO instance there is no local default: an omitted `field`/`exposedField` reads "default — not declared locally". The local declaration may not list every member the external definition has, so `effectiveInterfaceOf` reports `complete: false`. An authored member the local declaration does not list is shown read-only with reason `externproto-interface-not-locally-verifiable` and is **never** reported as an error.

### 4.4 EXTERNPROTO declarations
Show the written URL list (as source text) and the locally declared interface: name, type, access. EXTERNPROTO interface declarations carry **no default values** (ISO A.2 `externInterfaceDeclarations`), so the default column reads "not declared locally" — never a guessed or remotely obtained value.

## 5. Gated edit phases

Each phase needs its own explicit owner GO, its own fail-closed identity tests and its own QA. Until then the current behaviour stands: `planFieldEdit` refuses every PROTO-instance field with `node-type-is-a-proto-name` (`src/vrml/field-edit.js`).

### 5.1 Common gate (every edit)
An edit is planned only when all of these are proven from the current parse; otherwise it refuses with a stable reason id and nothing is dispatched:
1. The parse session's text equals the editor's current text (stale session refuses).
2. The parse saw the whole document and reported no syntax error (recovery can move boundaries).
3. The target node or declaration resolves through WD1.4 identity to exactly one item; `ambiguous` or lost refuses.
4. The PROTO type resolves to exactly one declaration in scope (`typeDeclIsUniqueInScope`); duplicate or shadowed declarations refuse.
5. The member resolves to exactly one interface declaration (`interfaceMemberIsUniqueInScope`).
6. The edit set is minimal (§6), passes `validateEdits`, and `verifyTransaction` accepts it.

### 5.2 J1 — instance-field edits (proposed)
- Only `field`/`exposedField` members, authored exactly once on the instance, not `IS`-bound, with a type one of the WD2-B typed editors supports.
- Type authority is the resolved interface declaration's type, never the lexical shape of the value.
- Instances of an EXTERNPROTO are eligible only if the locally declared interface resolves and the member is listed in it; unresolved or unsupported EXTERNPROTO declarations make their instances read-only (§7). Members not listed locally stay read-only (`externproto-interface-not-locally-verifiable`, §4.3).
- Instances inside a PROTO body are edited in that body's scope only; an instance whose scope cannot be proven refuses.

### 5.3 J2 — interface default-value edits (proposed)
- Local `PROTO` declarations only; EXTERNPROTO declarations have no defaults to edit.
- Only `field`/`exposedField` defaults; the replacement value must be valid for the **unchanged** declared type.
- Only the default-value token span changes. The name, type and access tokens are never rewritten.
- Changing a default changes the effective value of every instance that omits that field. The confirmation must state the number of affected instances in the current document; if any instance in the current document cannot be enumerated with proven identity, the edit refuses.
- **Cross-file reach:** other World Project files that reach this PROTO through an EXTERNPROTO also change behaviour, and the current-document count does not include them. Whether J2 must refuse, or may proceed with an explicit warning, when cross-file instances cannot be enumerated is an **open owner decision** (§11). Until it is decided, the confirmation must disclose that instances in other files are not counted.

### 5.4 Interface type, access and name changes — refused
Changing a member's type, access type or name, or adding/removing a member, can invalidate existing instance values, `IS` connections and ROUTEs. Renaming an `exposedField` also breaks ROUTEs that use its implicit `set_<name>` and `<name>_changed` events. This design does not permit them. They refuse with a stable reason (proposed id: `proto-interface-change-requires-migration`) until a separately approved migration design specifies instance compatibility checks and atomic multi-site patching.

## 6. Source-patch safety

- **Minimal spans:** each edit replaces only the token span(s) of the value being changed (one scalar token, one vector component, one string token, as WD2-B). Comments, whitespace, commas, CRLF/LF and number spelling outside those spans are never touched.
- **Line endings:** inserted text uses the document's existing line ending (`detectEol`, `src/vrml/structure-edit.js`); mixed documents keep every untouched line ending byte-for-byte.
- **Stale-source detection:** the plan records `oldText`. `applyVerifiedEdits` refuses with `buffer-changed` if the buffer no longer equals it. A plan built from an earlier inspection is never re-targeted; the user re-inspects.
- **Atomicity:** a multi-span edit is validated and dispatched as one set. If any span fails validation or verification, the whole set refuses and the buffer is unchanged. There is no partial apply.
- **Undo:** one authorized user action = one CodeMirror transaction = one Undo step (§3).

## 7. EXTERNPROTO boundaries

- **No remote fetching.** WD2-J adds no network access. The existing retrieval substrate enforces `network: false` (`src/external-proto/resolver-context.js`); this design does not change that.
- **No invented interface data.** Only what is written in the local EXTERNPROTO declaration is shown. Missing defaults, missing members and unresolved implementations are displayed as such.
- **Unsupported declarations** are shown read-only with their stable reason, and instances of them are read-only. The text "Defined in an external file that WRL Forge does not open here." is used **only** when the locally declared interface is valid and the implementation is external and not opened. An unparseable URL list, or an interface the scope graph reports as `unsupported`/`invalid`, shows its own local parse or interface reason instead.
- Local-file resolution through the World Project path (WD1.7-B/B2) is out of scope for this lane and needs its own decision (§9).

## 8. Fail-closed identity cases

Each refuses rather than anchoring:
- **Duplicate DEF names** in one scope (`duplicate-def-in-scope`): editing a DEF-named instance refuses.
- **Duplicate PROTO/EXTERNPROTO type names** in one scope: instance edits of that type refuse.
- **PROTO scope leakage:** a USE or `IS` that would cross a 4.8.4 boundary (`def-not-visible-across-proto-boundary`) refuses.
- **Invalid instance references:** an instance whose type does not resolve, or a USE of a missing/ambiguous DEF, is read-only.
- **IS re-anchoring:** an edit near an `IS` statement whose binding cannot be re-proven after the edit refuses.
- **Stale source:** buffer differs from the planned `oldText` → `buffer-changed`; no dispatch.
- **Unsupported EXTERNPROTO:** §7.

## 9. QA and test plan

### 9.1 Focused tests (pure, `node:test`)
1. **Access types:** a PROTO declaring one member of each of `field`, `exposedField`, `eventIn`, `eventOut`; assert display rows and that only `field`/`exposedField` are J1/J2 eligible.
2. **PROTO scope isolation:** same DEF name inside a PROTO body and at top level; edits stay in the correct scope; cross-boundary USE refuses.
3. **Duplicate DEF names:** two DEFs of one name in one scope → refusal with `duplicate-def-in-scope`.
4. **Unsupported interfaces:** EXTERNPROTO with an unsupported/invalid interface → read-only, stable reason, no edit planned.
5. **Stale edit spans:** plan against text A, mutate buffer to B → `buffer-changed`, buffer still B.
6. **Invalid instance references:** instance of an undeclared type; USE of a missing DEF → read-only.
7. **Undo restoration:** multi-span J1 edit → one transaction; one Undo restores the exact original source text, including CRLF line endings and comments; one Redo restores the exact edited source text, with the same preservation.
8. **Atomic refusal:** a set where one span is invalid → nothing dispatched, buffer unchanged, undo depth unchanged.
9. **Interface change refusal:** a type/access/name change request → `proto-interface-change-requires-migration`.
10. **Duplicate PROTO/EXTERNPROTO type names:** two declarations of one type name in one scope → instance edits of that type refuse.
11. **J2 enumeration refusal:** a default edit where an affected instance cannot be enumerated with proven identity → refusal, buffer unchanged.
12. **EXTERNPROTO local-only display:** an EXTERNPROTO instance with omitted fields shows "default — not declared locally"; an authored member not listed locally is read-only with `externproto-interface-not-locally-verifiable` and is not an error; the test asserts no network access is attempted.

### 9.2 Corpus tests
Real-world corpus documents (including hyphenated DEF names and EXTERNPROTO chains): inspection completes; every J1 eligible field round-trips; **0 wrong-target edits**.

### 9.3 Visual tests (Linux first, then macOS 15+)
- Inspector rows for all four access types, including `eventOut`, matching §4.1.
- EXTERNPROTO "not declared locally" and unsupported states are visibly distinct from errors.
- Refusal reasons are visible and readable.

## 10. Dependencies
- WD1.2 span patches; WD1.4 node identity; WD1.5 scope graph and disjointness; WD1.6 effective-interface query; WD2-B typed inspector editing and transaction gate.

## 11. Open owner decisions
1. Exact UI wording/presentation for unresolved and unsupported EXTERNPROTOs.
2. Whether J2 (default edits) is wanted at all, or only J1.
3. Whether local EXTERNPROTO files authorized by the World Project may later be opened read-only for inspection (separate lane).
4. Whether a migration design for interface type/access/name changes (§5.4) should ever be scheduled.
5. Whether J2 refuses, or warns and proceeds, when instances in other World Project files cannot be enumerated (§5.3).

## 12. Limits and state
- Owner review is **not complete**.
- Independent QA is **not complete**.
- No network or remote fetching is performed or proposed.

---
*Draft — proposal for owner review; not approved.*
