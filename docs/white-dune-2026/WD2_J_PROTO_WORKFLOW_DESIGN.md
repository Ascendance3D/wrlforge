# WD2-J — PROTO and EXTERNPROTO Visual Workflow Design

**Lane:** WD2-J (Parent), WD2-J-R (Design sub-issue) · **Status:** **DESIGN / INVESTIGATION ONLY** — no production code changes authorized. Awaiting owner review.

## 1. Goal

Design the read-only inspection of PROTO interfaces, instances, and EXTERNPROTO declarations first, followed by narrowly gated edits. Establish the fail-closed identity rules and document contracts before any implementation begins.

## 2. Scope

### In Scope
- Read-only inspection of PROTO interfaces, PROTO instances, and EXTERNPROTO declarations.
- Separately gated safe instance-field editing (narrow edits).
- Narrow interface edits (once proven safe).

### Out of Scope
- Fetching EXTERNPROTO URLs (omissions remain unsupported, rather than resolving remotely).
- PROTO body editing (deferred work).

## 3. Architecture Constraints and Contracts

This design preserves the core WRL Forge contracts:

- **Exact Source Text (`WD.md` §2):** The canonical document is the source text. Every edit lands as WD1.2 span patches. No AST-to-text regeneration. No secondary document model or canonical scene graph.
- **No Hidden Instrumentation:** No hidden source instrumentation, synthetic identifiers, or sidecar semantic state.
- **Fail-Closed Identity (`WD.md` §7):** Unsupported or ambiguous identity operations fail closed. We never assume a confident wrong node.
- **One Undo Transaction:** One user-visible action maps to exactly one coherent undo transaction (CodeMirror undo).
- **One Selection Authority:** The UI selection model operates via a single shared selection authority.
- **ISO 4.8.4 Scope Disjointness:** PROTO scopes are properly disjoint from the enclosing document per WD1.5.
- **EXTERNPROTO Omissions:** EXTERNPROTO omissions are strictly unsupported rather than unresolved.
- **Profile Separation:** Strict isolation of features according to supported profiles.
- **Safety Boundaries:** X_ITE preview, IPC/CSP, and filesystem authorization contracts are maintained as applicable.

## 4. Design: Read-Only Inspection

Read-only inspection will expose the structure of PROTO and EXTERNPROTO declarations without modifying the buffer.
- **PROTO Interfaces:** Present the `IS` mapping, field types, access types (`eventIn`, `eventOut`, `field`, `exposedField`), and default values.
- **EXTERNPROTO Declarations:** Show the URL(s) and field declarations.
- **PROTO Instances:** Show the instantiated node and its overridden field values relative to the PROTO interface.

## 5. Design: Separately Gated Edits

Editing functionality will be separated into strictly gated implementation phases:
1. **Instance-Field Edits:** Modifying the fields of a PROTO instance. This requires the same WD1.2 patch algebra as built-in nodes.
2. **Narrow Interface Edits:** Modifying defaults or types of a PROTO declaration interface, *if* proven safe against existing instances.

Each phase will require its own fail-closed identity tests to ensure stability.

## 6. Fail-Closed Identity Cases

The identity resolution must fail rather than incorrectly anchoring:
- **Duplicate DEFs:** If multiple PROTOs or instances share a `DEF` name in the same scope, editing fails.
- **Complex Body Scopes:** Changes near `IS` statements that fail re-anchoring must reject the edit.
- **EXTERNPROTO URL Mismatches:** If the declaration shape cannot be securely tokenized and patched, fail closed.

## 7. QA and Test Plan

### Focused Corpus / Visual QA Cases
- **Visual Tests:** UI representation of `field` vs `exposedField` vs `eventIn` in the inspector.
- **Corpus Tests:** Correctly scoping and anchoring edits against the real-world Cybertown corpus, including hyphenated DEF names and complex EXTERNPROTO chains.
- **Fail-Closed Assertions:** Ensure that overlapping or ambiguous spans result in rejected transactions.

## 8. Dependencies and Unresolved Decisions

### Dependencies
- WD1.2 Source-span patches.
- WD1.4 Node identity.
- WD1.5 Scope disjointness.

### Unresolved Decisions
- Exact UI representation for unresolvable EXTERNPROTOs (i.e. network fetching is out of scope; UI must represent this clearly without appearing "broken").

## 9. Limits and State

- Owner review is **not complete**.
- Independent QA is **not complete**.
- No real-world fetching is performed.

---
*Draft — proposal for owner review; not approved.*
