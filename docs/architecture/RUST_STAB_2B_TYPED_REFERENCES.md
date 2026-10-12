# RUST-STAB-2B — typed ROUTE, IS, and PROTO reference analysis

Status: **built, read-only, advisory**. Branch `feature/rust-stab-2b-typed-references`,
stacked on RUST-STAB-2A (`feature/rust-stab-2a-scope`, PR #138).

This lane adds typed answers to the RUST-STAB-2A scope graph. It finds the type of each
node instance, the interface members of PROTO, EXTERNPROTO, and Script interfaces, and it
checks ROUTE events and `IS` connections.

It adds no modeling tool and no source-edit command.

## 1. Where it lives

| Part | File |
|---|---|
| Typed analysis | `crates/wrlforge-vrml/src/scope/typed.rs` |
| Builder hooks, reason and code constants | `crates/wrlforge-vrml/src/scope.rs` |
| Unit tests | `crates/wrlforge-vrml/src/scope/typed_tests.rs` |
| Scene Tree annotation | `crates/wrlforge-vrml/src/scene.rs` (`Item::typed`, `TypedSummary`) |
| IPC data | `apps/desktop-tauri/protocol/src/lib.rs` (`SceneItem::typed`, `TypedCheck`) |
| Service | `apps/desktop-tauri/src-tauri/src/service.rs` (`Service::analyze`) |
| UI | `apps/desktop-tauri/ui/src/panels.rs` (Scene Tree `data-typed`, tooltip; Diagnostics) |
| GUI smoke step | `apps/desktop-tauri/ui/src/smoke.rs` |
| Service tests | `apps/desktop-tauri/src-tauri/tests/typed_regression.rs` |
| Corrected fixture | `apps/desktop-tauri/src-tauri/tests/fixtures/flux-regression/corrected/08_proto_script_inline_valid.wrl` |

The analysis runs inside `scope::build`, on the same parse as the 2A graph. It is built for
each analyzed revision and thrown away with it. Nothing is cached across revisions.

Load, save, and crash recovery do not call it. A typed result cannot block Open, Save,
Save As, or recovery.

## 2. Standard clauses used

ISO/IEC 14772-1 is the authority. The JavaScript WD1.5 P2A/P2B/P2C research
(`docs/white-dune-2026/WD1_5_P2*.md`) was a cross-check. It was not translated line by line.

| Clause | Rule used here |
|---|---|
| 4.7 | An exposedField `zzz` can be used as eventIn `set_zzz` and as eventOut `zzz_changed`. |
| 4.8.1 | Node type names shall be unique in a file. A PROTO with the name of a built-in node, or a repeated name in one scope, gives undefined results. |
| 4.8.3 | `IS` appears inside a prototype definition and refers to a declaration in the prototype interface. The types must match exactly (SFColor to SFVec3f, and SFColor to MFColor, are illegal). An exposedField in a definition can be named by its `set_`/`_changed` event names. |
| Table 4.4 | Legal access mappings between the definition (node field) and the declaration (interface member). Seven of sixteen cells are legal. |
| 4.8.4 | A nested PROTO is local to the enclosing PROTO. `IS` in a nested PROTO refers to the **innermost** prototype. A prototype can be instantiated only **after** its definition is complete, and never inside its own implementation. |
| 4.9.2 | An EXTERNPROTO interface is a **subset** of the implementation. A declared name and type is authoritative; an undeclared name is unknown, not missing. |
| 4.10.2 | ROUTE nodes are defined before the ROUTE. A route goes from an eventOut to an eventIn. Types match exactly. When `zzz` is not found as the required event, the browser tries `set_zzz` (destination) or `zzz_changed` (source). |
| 4.4.4 | The Script self-reference exemption applies to the built-in Script node (2A rule, now gated on the proven type). |
| Annex A.2 | `fieldType` lists the 20 VRML97 field type tokens. Types compare by exact token. |
| Clause 6 | Built-in node interfaces come from the committed node schema (`node_schema`), VRML97 profile only. |

## 3. Type resolution rules

Each node instance gets one `TypeRef`. Built-in types are schema facts, never entries in the
`ScopeGraph::types` table.

Lookup walks the **type-scope chain**: the instance's scope, then the scope that each PROTO
body is written in, out to the document. This is separate from the DEF chain, which stops at
a PROTO body (4.8.4).

Steps, in this order:

1. No type name: `invalid` / `missing-name`.
2. Inside a PROTO interface default value: `unsupported` / `type-in-proto-interface-default`.
3. A damaged document, or **any** damaged scope in the chain: `recovered`. This also
   withholds the built-in answer, because "built-in" claims that no declaration is in scope.
4. A PROTO of this name contains the instance: `invalid` / `recursive-proto-instance` (4.8.4).
5. More than one declaration of the name in the chain: `ambiguous` /
   `duplicate-proto-declaration`. Decided on the name alone, before position or kind.
6. One declaration, completed before the instance: `resolved` / `ok`, target = that
   declaration. If the name is also a built-in, detail `proto-shadows-builtin`.
7. One declaration, not complete before the instance: `invalid` /
   `proto-instance-before-declaration`. This applies to built-in spellings too: the file's own
   declaration exists, so the instance is not silently the built-in.
8. No declaration and a VRML97 built-in name: `resolved` / `node-type-is-builtin`.
9. Otherwise: `unresolved` / `node-type-unknown`.

A type is never resolved through a DEF name. The type table and the DEF table are separate.

### Built-in Script

Script-only rules apply only to a node whose type is proven to be the built-in `Script`
(`TypeRef::is_builtin("Script")`). That covers the Script interface lookup, Script interface
`IS`, and the 2A self-reference exemption (4.4.4).

A `PROTO Script` instance therefore gets its PROTO's interface. A self-reference under it is
a cycle (`self-referential-use`), as for any other node. This closes the 2A §12 gap.

## 4. Interface ownership rules

Interface entries come from one owner only:

| Instance type | Entries |
|---|---|
| Built-in node | The schema fields with a VRML97 declaration. X3D-only fields are excluded. |
| Built-in Script | The schema fields (`url`, `directOutput`, `mustEvaluate`) plus the node's own declarations. |
| PROTO | The PROTO's declared interface. |
| EXTERNPROTO | The interface declared **in this document**. The URL is never read. |

Each exposedField adds two entries: `set_zzz` (eventIn) and `zzz_changed` (eventOut) (4.7).

Two entries with one name have no winner: `ambiguous` / `duplicate-interface-member`. This
covers a repeated member, an explicit/alias collision (`exposedField zzz` plus
`eventIn set_zzz`), and a Script declaration that repeats a schema field name. No
precedence or ranking is applied.

A test proves that no VRML97 built-in has an alias collision, so schema lookups are unique.

The interface is **not provable** (`recovered`) when the document is damaged, the owner's
declaring scope or body scope is damaged, or a member has no name or no type token.

Member lookup is separate from node-name and type-name lookup. When the owner is known, no
other interface is searched. When the owner is uncertain, no member is guessed.

## 5. ROUTE event rules

A ROUTE asks separate questions. Each endpoint has a node answer (2A) and an event answer
(`EventEnd`). The ROUTE has one verdict (`RouteCheck`).

An event is looked up **only after** its node is `resolved`:

| Node answer | Event answer |
|---|---|
| `resolved` | Looked up as below. |
| `recovered` | `recovered`, with the node's reason. |
| `ambiguous` | `ambiguous` / `route-node-not-proven`. |
| other | `unsupported` / `route-node-not-proven`. A lost node never becomes a missing event. |

The node's type must also be proven. Otherwise the event is withheld with
`node-type-not-proven`, or `recovered`.

Event lookup for one endpoint:

1. The written name first. One entry that can send (source: eventOut or exposedField) or
   receive (destination: eventIn or exposedField): bound.
2. The 4.10.2 fallback, `zzz_changed` (source) or `set_zzz` (destination). It runs only when
   the required event is **provably** absent: the written name is missing from a fully known
   interface, or it exists with another access (for example a `field`).
3. A missing name on an EXTERNPROTO interface stops the lookup: `unsupported` /
   `externproto-interface-not-locally-verifiable`. The fallback does not run past it.
4. Neither name exists: `unresolved` / `route-event-unknown`.
5. A name exists with the wrong access only: `invalid` / `route-source-not-an-event-out` or
   `route-destination-not-an-event-in`.

A Script `field` is never an event. Nothing is inferred from Script URL contents.

The ROUTE verdict:

1. A proven event defect on either side: that defect. A defect on one side is still reported
   when the other side is uncertain.
2. Else an unproven node: the node's status and reason (the 2A finding already covers it).
3. Else an uncertain event: that event's status.
4. Else compare the two field type tokens exactly. An unknown token: `unsupported` /
   `field-type-unknown`. Different tokens: `invalid` / `route-type-mismatch`. Equal:
   `resolved` / `ok`.

No implicit conversion: SFFloat to SFInt32 and SFFloat to MFFloat are mismatches.

ROUTE order is not changed and no ROUTE is edited.

Distinguished cases: proven valid connection (`ok`), missing event name
(`route-event-unknown`), wrong direction, type mismatch, unresolved node, ambiguous node,
unsupported node interface (EXTERNPROTO, type not proven), recovered source.

## 6. IS connection rules

Each `IS` gets one `IsBinding`. Two syntactic forms are covered:

- A node field: `size IS size` (definition = the node's field or event).
- A Script interface declaration: `eventIn SFFloat x IS y` (definition = that declaration).
  This applies only to a proven built-in Script; otherwise
  `unsupported` / `interface-declaration-on-non-script`.

Steps:

1. Inside a PROTO interface default value: `unsupported` / `is-in-proto-interface-default`.
2. A damaged scope: `recovered`.
3. Not inside any PROTO body: `invalid` / `is-outside-proto-definition` (4.8.3).
4. The **declaration**: look up the name in the innermost enclosing PROTO interface only
   (4.8.4). None: `unresolved` / `is-interface-member-not-declared`. Two:
   `ambiguous`. An outer or unrelated interface is never searched.
5. The **definition**: look up the node field in the node's proven interface (schema, Script,
   PROTO, or EXTERNPROTO). Not found: `unresolved` / `is-node-field-unknown`, or
   `unsupported` for an EXTERNPROTO. An unproven node type withholds the answer.
6. Table 4.4 on the effective access kinds. Illegal: `invalid` / `is-access-mismatch`.
7. Exact field type tokens. Unknown token: `unsupported` / `field-type-unknown`.
   Different: `invalid` / `is-type-mismatch`.
8. Otherwise `resolved` / `ok`.

Effective access: `set_translation` on a Transform is an eventIn; `translation` is an
exposedField. A connection is never accepted only because two names match.

Access is checked before type. A binding with both defects reports the access defect.

No `IS` is rewritten and no declaration is added.

## 7. Advisory codes

All typed findings are in `Analysis::scope_diagnostics`, next to `SCOPE001`–`SCOPE010`, in the
separate range `SCOPE1xx`. They use the same session, revision, and view mapping. They never
mark the editor, never change the parser count, and never block Save.

`SCOPE001`–`SCOPE010` keep their 2A meanings.

| Code | Severity | Reason | Range marked |
|---|---|---|---|
| `SCOPE101` | warning | `route-event-unknown` | The event name token |
| `SCOPE102` | warning | `route-source-not-an-event-out` | The source event name token |
| `SCOPE103` | warning | `route-destination-not-an-event-in` | The destination event name token |
| `SCOPE104` | warning | `route-type-mismatch` | The whole ROUTE statement |
| `SCOPE111` | warning | `is-interface-member-not-declared` | The name after `IS` |
| `SCOPE112` | warning | `is-node-field-unknown` | The node field name |
| `SCOPE113` | warning | `is-access-mismatch` | `field IS name` |
| `SCOPE114` | warning | `is-type-mismatch` | `field IS name` |
| `SCOPE115` | warning | `is-outside-proto-definition` | `field IS name` |
| `SCOPE121` | warning | `node-type-unknown` | The node type token |
| `SCOPE122` | warning | `proto-instance-before-declaration` | The node type token |
| `SCOPE123` | warning | `recursive-proto-instance` | The node type token |
| `SCOPE130` | warning | `duplicate-proto-declaration` (second and later declaration in **one** scope) | The PROTO/EXTERNPROTO name |
| `SCOPE131` | info | `proto-shadows-builtin` (detail) | The PROTO/EXTERNPROTO name |

A warning appears only for a proven defect. `SCOPE131` is info: the binding is proven (the
file's own declaration), and the clause gives the browser result as undefined rather than a
WRL Forge defect.

`recovered`, `unsupported`, and `ambiguous` typed answers never produce a finding. A
duplicate type name seen by one instance through nested scopes is an `ambiguous` status only.

Status values are the 2A set: `resolved`, `unresolved`, `invalid`, `ambiguous`, `recovered`,
`unsupported`. Detail values: `proto-shadows-builtin`, `event-via-shorthand`,
`event-via-exposedfield-alias`.

## 8. Scene Tree and UI

`SceneItem::typed` (`TypedCheck`) carries:

- for a ROUTE item: the whole-ROUTE verdict, with `SFFloat -> SFFloat` when both event types
  are known;
- for a node item whose type is not a proven built-in: the type answer, with `PROTO Pulse`
  when it resolves.

It is matched by exact statement span only. The UI shows it in the tooltip and as
`data-typed="subject:status:reason"`. An unresolved or uncertain typed answer uses the same
underline shapes as 2A.

Nothing is clickable as a target. Viewport picking, gizmo authorization, Inspector edits, and
selection identity did not change. The UI still ignores an analysis from an older revision.

## 9. Source safety

- The graph and the typed analysis are read-only. They take a parse result and return data.
- The canonical source text is the only document authority.
- No typed lookup has edit permission. `ScopeGraph::edit_target` is unchanged and still
  returns only a 2A `resolved` DEF.
- No nearest-name, nearest-span, or type-based guessing proves identity. Duplicate DEFs stay
  ambiguous; the event lookup never narrows them by type.
- Language meaning stays separate from edit identity.
- X_ITE remains the default renderer. The experimental Rust renderer stays disabled by
  default.

## 10. Fixture corrections

`08_proto_script_inline.wrl` is **unchanged**. It is not valid VRML97: it declares
`field SFFloat size 1` and binds it with `Box { size IS size }`, but `Box.size` is
`SFVec3f` (4.8.3). The analysis reports exactly one `SCOPE114` on `size IS size`.

It stays in the source-preservation and edit tests (`flux_regression.rs`), whose comment now
states that it is invalid. Source preservation also holds for invalid VRML.

The new original fixture `corrected/08_proto_script_inline_valid.wrl` declares
`field SFVec3f size 1 1 1`, uses `size 2 2 2` on the instance, and adds one `IS` on an eventIn
and one ROUTE to the PROTO instance. It has no scope finding.

`m2_route_no_such_eventout.wrl` now gets `SCOPE101` on `key_value`.

`m6_route_before_def.wrl` is non-conforming (4.10.2). It keeps its two `SCOPE003` warnings and
gets no typed finding, because its node names are unbound. Its semantic tests run without a
renderer.

### Known X_ITE preview failures

X_ITE 15.1.10 rejects two fixtures in the preview. The failure is the same on the 2A baseline
binary (`f508842`) and on this lane:

| Fixture | X_ITE parser error | Cause |
|---|---|---|
| `08_proto_script_inline.wrl` | line 15:32 | The `size IS size` type mismatch. |
| `m6_route_before_def.wrl` | line 5:9 | The ROUTE before its DEFs. |

The semantic warnings are not suppressed to make a preview test pass. X_ITE behavior is not
changed.

## 11. Tests

See §13 for the final-commit results.

Crate tests (`cargo test -p wrlforge-vrml`, 21 new tests in `scope/typed_tests.rs`):

1. Valid TimeSensor-to-interpolator ROUTE.
2. Valid interpolator-to-Transform ROUTE.
3. ROUTE with a missing event name (exact token marked).
4. ROUTE with the wrong direction, both sides; a built-in `field` as a source.
5. ROUTE with different types.
6. exposedField names: bare, explicit, and the 4.10.2 shorthand.
7. Script eventIn and eventOut; a Script `field`; nothing inferred from the URL.
8. PROTO instance with ROUTEs, incl. the shorthand and a missing name.
9. EXTERNPROTO: local interface used; undeclared name is `unsupported`; a declared type
   mismatch is still found.
10. Valid `IS`, incl. exposedField definitions to eventIn declarations.
11. `IS` type mismatch, incl. SFColor vs MFColor.
12. `IS` access mismatch, four illegal Table 4.4 cells and one legal one.
13. Nested PROTO interfaces with the same member name; an outer-only member is not found
    from the inner body.
14. A PROTO with a built-in name (`Box`, `Script`): before/after declaration, Script rules
    not applied, ROUTE uses the PROTO interface.
15. Repeated DEF names: no typed answer, no type narrowing, no target.
16. Missing, unsupported, and damaged declarations; every prefix of a document.
17. BOM, CRLF, lone CR, and Unicode (incl. astral) names.
18. Two documents with the same identifiers.
19. A stale revision (service test).
20. No source or parse change from analysis; deterministic output.

Also: the original and corrected Flux fixtures, and a schema alias-collision check.

Every crate test runs a gate check: an unproven node has no resolved event; a resolved
ROUTE has one type and legal directions; a resolved `IS` is legal under an independent
Table 4.4 oracle; a resolved user type names its declaration; no finding is an error.

Service tests (`typed_regression.rs`, 3 tests): findings and marked text across LF, CRLF,
BOM+CRLF, and gzip, with Save keeping every byte; Scene Tree typed answers; session and
revision binding, incl. a refused stale edit.

GUI smoke: a new step checks that every typed answer reaches the tree DOM and that the
Diagnostics scope list shows the analysis revision's findings.

## 12. Unsupported and uncertain cases, and known limits

- EXTERNPROTO URLs are not loaded. An undeclared EXTERNPROTO member is `unsupported`.
- Names in Inline files are not loaded.
- Types, `IS`, and DEFs inside a PROTO **interface default value** are `unsupported`.
- Recovery attribution is coarse (2A): one unclosed brace withholds the whole document.
- 4.6.2 closest-preceding binding for duplicate interface members is not implemented;
  duplicates are `ambiguous`.
- Not checked: multiple `IS` on one node field (4.8.3 undefined results), duplicate routes
  (4.10.2 redundant routing), fan-in, `exposedField` in a Script interface (6.40), field
  value syntax against the declared type, SFNode node-class constraints, and USE of a
  DEF'd node where the node type is not allowed.
- Script source and URLs are never read; Script behavior is not inferred.
- Not a full VRML97 or X3D conformance validator.
- VrmlPad scripting compatibility stays in the future backlog.

## 13. Final-commit results

Recorded in the PR description and the final report for the exact candidate SHA, built and
tested in a separate clean worktree.
