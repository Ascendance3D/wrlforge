# RUST-STAB-2A — Rust scope graph and safe reference analysis

Status: **built, read-only, advisory**. Branch `feature/rust-stab-2a-scope`, stacked on
RUST-STAB-1 (PR #137).

This lane adds a lexical scope graph for VRML97 node names to the Rust core. It replaces
the flat USE lookup in the Rust Scene Tree.

It adds no modeling feature and no source-edit operation.

## 1. Where it lives

| Part | File |
|---|---|
| Scope graph | `crates/wrlforge-vrml/src/scope.rs` |
| Unit tests | `crates/wrlforge-vrml/src/scope/tests.rs` |
| Scene Tree annotation | `crates/wrlforge-vrml/src/scene.rs` (`build_scene_tree_scoped`) |
| IPC data | `apps/desktop-tauri/protocol/src/lib.rs` (`SceneRef`, `Analysis::scope_diagnostics`) |
| Service | `apps/desktop-tauri/src-tauri/src/service.rs` (`Service::analyze`) |
| UI | `apps/desktop-tauri/ui/src/panels.rs` (Scene Tree, Diagnostics) |
| Service tests | `apps/desktop-tauri/src-tauri/tests/scope_regression.rs` |

The graph is built from one parse result. It does not change the source text, a source
span, or document identity.

The graph is disposable. `Service::analyze` builds a new graph for each analyzed revision and
caches nothing.

Load, save, and crash recovery do not call the graph. A scope result cannot block Save.

## 2. Standard rules used

All rules come from ISO/IEC 14772-1. The JavaScript WD1.5 research was a cross-check, not
the authority.

| Clause | Rule |
|---|---|
| 4.6.2 | A USE refers to a node that a DEF names **before** the USE. With repeated names, the standard uses the closest preceding DEF. |
| 4.6.2 | A USE inside the node it names refers to that node. |
| 4.4.4 | That self-reference is a cycle only in the transformation hierarchy. A Script's descendants are outside it. |
| 4.8.4 | A PROTO body is a DEF/USE scope that is separate from the scene and from nested PROTOs, in both directions. |
| 4.10.2 | Nodes in a ROUTE "shall be defined before the ROUTE statement". |
| Annex A | A PROTO body holds at least one node statement. |

## 3. Scopes and namespaces

There are two scope kinds. The **document** scope holds the top-level statements.

Each PROTO opens a **proto-body** scope. A body lookup has no parent scope, so it never sees
an outer DEF and an outer lookup never sees a body DEF.

Three namespaces use three separate tables:

| Namespace | Table | Contents |
|---|---|---|
| Node names | `ScopeGraph::defs` | `DEF name Type { }` |
| Node types | `ScopeGraph::types` | PROTO and EXTERNPROTO names only |
| Interface members | `ScopeGraph::members` | PROTO, EXTERNPROTO and Script interface declarations |

Built-in node types (`Group`, `Script`, ...) are schema facts. They never enter the type table.

A DEF, a PROTO, and an interface field can share one spelling. They do not collide.

This lane resolves node names only. The type and member tables exist for RUST-STAB-2B.

## 4. Two answers per reference

Each USE and each ROUTE node name gets two separate answers.

| Answer | Field | Use |
|---|---|---|
| Edit identity | `status`, `target` | `target` exists only for `resolved`: exactly one DEF is provable. |
| Language meaning | `language_target` | The 4.6.2 closest preceding DEF. Display only. |

The language meaning never feeds an edit, a selection, or node identity. This keeps the
WD.md §7 rule: an answer may be lost or ambiguous, but it must never name a different node.

Example: two `DEF Box1` nodes come before one `USE Box1`. The standard uses the second DEF.

WRL Forge reports `ambiguous` and gives no edit target. The Scene Tree tooltip names the
second DEF as the VRML97 meaning.

The lookup does not narrow duplicates by node type, by distance, or by any score.

## 5. Status codes

| Status | Class | Meaning |
|---|---|---|
| `resolved` | proven | Exactly one DEF. The only status with a target. |
| `unresolved` | unresolved | No visible DEF. A defect in the source. |
| `invalid` | unresolved | The reference breaks a rule (USE before DEF, a cycle). |
| `ambiguous` | uncertain | More than one visible DEF. Valid VRML97; no edit target. |
| `recovered` | uncertain | The parse is damaged where the answer depends on it. |
| `unsupported` | uncertain | A valid construct whose scope this lane does not decide. |

`unresolved` and `unsupported` stay separate. An unsupported case never becomes a
"missing node" finding.

## 6. Reason codes

| Reason | Status |
|---|---|
| `ok` | resolved |
| `self-reference-outside-transformation-hierarchy` | resolved (USE inside its own DEF, below a Script) |
| `missing-name` | invalid (the parse has no name token) |
| `use-before-def` | invalid |
| `self-referential-use` | invalid (cycle, 4.4.4) |
| `route-node-not-defined-before-route` | unresolved (4.10.2) |
| `def-not-declared-in-scope` | unresolved |
| `def-not-visible-across-proto-boundary` | unresolved (4.8.4) |
| `duplicate-def-in-scope` | ambiguous |
| `document-parse-incomplete` | recovered (node or depth limit reached) |
| `document-structure-unbalanced` | recovered (unclosed brace or bracket, unterminated string) |
| `scope-recovered` | recovered (a syntax error inside this scope) |
| `proto-scope-not-provable` | recovered (PROTO without a name) |
| `proto-body-not-provable` | recovered (PROTO body without a node statement) |
| `reference-in-proto-interface-default` | unsupported |
| `name-declared-in-proto-interface-default` | unsupported |

Detail code `route-in-mfnode-array` marks the Cybertown/Blaxxun ROUTE inside an MFNode
array. It is a classification, not a status.

## 7. Incomplete source

A parse limit, an unclosed brace or bracket, or an unterminated string marks **every**
scope as recovered. An unclosed brace absorbs the rest of the file, so it moves scope
boundaries.

Any other syntax error marks the innermost scope that contains it. An error that no scope
contains marks every scope.

A missing header changes no structure and marks nothing.

A recovered scope withholds every answer: positive, negative, and ambiguous. Example: an
unclosed PROTO can absorb a later `DEF Foo`, which would make one `Foo` look unique.

The graph never panics. A test builds it for every prefix of a document and for garbage input.

## 8. Advisory diagnostics

Scope findings are in `Analysis::scope_diagnostics`. They use the same session, revision, and
view mapping as the parser diagnostics.

They are a separate list. They do not mark the editor, change the parser count, or block Save.

| Code | Severity | When |
|---|---|---|
| `SCOPE001` | warning | `def-not-declared-in-scope` |
| `SCOPE002` | warning | `def-not-visible-across-proto-boundary` |
| `SCOPE003` | warning | `use-before-def` or `route-node-not-defined-before-route` |
| `SCOPE004` | warning | `self-referential-use` |
| `SCOPE010` | info | `duplicate-def-in-scope` (valid VRML97) |

A finding marks the reference name token. `recovered` and `unsupported` answers never
produce a finding.

The Diagnostics panel shows them under "Scope (advisory) · rev N". The UI already ignores an
analysis from an older revision.

## 9. Scene Tree

`resolution_scope` is now `"lexical"`. The old flat DEF table was removed from
`scene.rs`.

Each USE item has one `SceneRef`; each ROUTE item has two (source, destination).
`use_status` keeps its name and now carries the graph status.

A `SceneRef.target` is a Scene Tree item id of the same analysis. It is set only for a
proven reference whose DEF node is a tree item.

The tree shows the class with a shape, not only a color. Unresolved has a wavy underline;
uncertain has a dotted underline.

The tooltip names a proven target or the 4.6.2 meaning. Nothing is clickable as a target.

A click on a USE item still selects the USE's own source span. Viewport picking, gizmo
authorization, and Inspector edits did not change.

A proven scope answer is lexical. It is not proof of runtime object identity.

## 10. Standard meaning and safe editing identity

| Question | Standard (4.6.2) | WRL Forge edit identity |
|---|---|---|
| Two DEFs before a USE | The closest preceding one | `ambiguous`, no target |
| USE before its only DEF | Not bound | `invalid`, no target |
| USE inside its own DEF (not under Script) | That node, with undefined results | `invalid`, no target |
| USE inside its own Script DEF | That node | `resolved`, that node |
| Damaged scope | Unknown | `recovered`, no target |
| PROTO interface default | Not settled by the text | `unsupported`, no target |

## 11. Tests

Crate tests (`cargo test -p wrlforge-vrml`, 21 scope tests) cover the owner's list:

1. One DEF and one USE.
2. A USE before its DEF.
3. Repeated DEF names (language meaning against edit identity).
4. A PROTO body with its own DEF.
5. Nested PROTO declarations, both directions.
6. An outer DEF with the same name as a PROTO-local DEF.
7. A valid ROUTE, with exact name-token ranges for both endpoints.
8. ROUTE endpoints with unknown and missing names.
9. A ROUTE inside a PROTO body.
10. DEF, PROTO, and interface names with one spelling.
11. Malformed and incomplete source, including every prefix of a document.
12. BOM, CRLF, lone CR, and Unicode (astral) names.
13. A stale revision (service test).
14. Unsupported and unprovable scopes.
15. Two sessions with the same names (service test).

Most tests also run one gate check: a `resolved` answer points to a DEF with the same name,
and every other status has no target.

`tests/scope_regression.rs` runs the 16 RUST-STAB-1 Flux fixtures through `Service` in LF,
CRLF, BOM+CRLF, and gzip form. It proves that each proven target is the real DEF node in the
view text.

It also proves that the answers do not change between encodings, and that Save keeps every
byte.

The fixture text was not changed. Expected results were checked against the standard, not
the file names:

| Fixture | Result |
|---|---|
| `09_duplicate_def` | Valid. The second `USE Box1` is ambiguous (info only). `USE Floor` binds the outer Floor, not the PROTO's. |
| `08_proto_script_inline` | All four ROUTE endpoints proven. |
| `m2_route_no_such_eventout` | Node names proven. The bad event name is RUST-STAB-2B work. |
| `m6_route_before_def` | Really non-conforming (4.10.2): two `SCOPE003` warnings. X_ITE also rejects it. |
| `m4`, `m5` | Damaged parse: answers withheld, no finding. |

## 12. Limits and remaining gaps

- Event names, event direction, and event types on ROUTEs are not checked (RUST-STAB-2B).
- `IS` statements and interface-member lookups are not resolved. The member table exists only.
- PROTO type references (instances) are not resolved; the type table exists only.
- A DEF or USE inside a PROTO **interface default value** is `unsupported`.
- Recovery attribution is coarse: one unclosed brace withholds the whole document.
- Names in Inline files and EXTERNPROTO URLs are not loaded.
- A PROTO that takes a built-in name (for example `PROTO Script`) is not detected. The
  Script self-reference rule could then apply to it; the target node is still the same node.
- Not a full VRML97 semantic validator.

## 13. Finding for RUST-STAB-2B

The "positive" Flux fixture `08_proto_script_inline.wrl` binds `field SFFloat size` with
`size IS size` to `Box.size`, which is `SFVec3f`. That is a Table 4.4 type mismatch.

X_ITE rejects the file in the preview, also on the RUST-STAB-1 baseline. The fixture was not
changed; RUST-STAB-2B owns IS type checking.
