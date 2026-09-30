# WD.md — the lossless document core

The foundation the **model editor** stands on. Referenced from `CLAUDE.md`; read
this before touching anything under `src/vrml/`, and before designing any feature
that edits a document rather than merely reading it.

Lane docs live in `docs/white-dune-2026/`. Named "WD" after the White Dune
discovery lane that started it — **see §1 for what that name means now that WRL
Forge is GPL.**

---

## 1. White Dune and open-source reuse — read this first

**WRL Forge is `GPL-3.0-or-later`.** White Dune is `GPL-2.0-or-later`, whose "or
later" election makes it compatible with distributing a combined work under
GPLv3. The two are **no longer license-incompatible**, and the clean-room
prohibition that governed WD0–WD1.5 is **retired**.

White Dune — and any other GPL-compatible open-source implementation — **may** be
opened, searched, studied, compared, adapted, translated, ported, and
incorporated, provided upstream copyright, license notices, and provenance are
preserved.

The full current rules live in **`OPEN_SOURCE_PROVENANCE.md`** at the repository
root. The three that matter most here:

1. **Verify the specific artifact's license.** Do not assume every White Dune
   file, version, or packaged binary carries identical license metadata.
2. **Record what you took** — upstream project, version, component, authors,
   license, destination file, and whether it was copied, adapted, translated, or
   only conceptually informed. A reviewer cannot infer that from the diff.
3. **Never replace an upstream copyright header** with a WRL Forge one.

Still prohibited, and *not* changed by the license transition: proprietary source
without permission, leaked source, material with no usable license, and
reverse-engineering research artifacts of proprietary tools (`RE-ARTIFACTS`,
`blaxxun-cs-RE`). A GPL project license widens what is *compatible*; it widens
nothing about what is *lawfully obtained*.

The archive lives at `~/Projects/white-dune-archive/`, outside every Git
repository. Keeping GPL upstream trees out of this repo remains the rule — reuse
means an attributed, provenance-recorded port into WRL Forge's own files, not
committing somebody else's source tree wholesale.

The corpus guards in `spikes/*/corpus.js` that **throw** on a White Dune or
`RE-ARTIFACTS` path stay as they are. They are corpus hygiene — a VRML97
semantics sweep measures authored Cybertown content, not a modeling tool's C++
tree — and are unaffected by the licensing change.

**§9 still governs meaning.** Reuse teaches technique; it does not make any
implementation normative. ISO/IEC 14772-1 remains the authority on what VRML97
*means*.

## 2. The canonical document model

**The exact source-text buffer is the document.** Everything else is a derived,
disposable projection:

> tokens · AST · source map · symbol tables · scope graph · semantic indexes ·
> scene tree · typed inspector state · viewport state · diagnostics

Every semantic operation must ultimately produce **source-text patches** through
the accepted WD1.2 edit algebra. Nothing is regenerated wholesale from a tree.

This is what makes WRL Forge safe on the real Cybertown corpus: files keep their
formatting, their comments, their vendor quirks and their byte-level identity
through an edit, because the editor never re-prints them.

### Prohibited, permanently

Do **not** introduce any of these without an explicitly approved lane that
supersedes this file:

- a canonical scene graph (a tree that is the source of truth instead of the text)
- a CST, or an AST→text serializer used for whole-document regeneration
- hidden synthetic identifiers written into source, identity comments, or sidecar
  semantic state
- a second document buffer
- parser object identity relied on **across** reparses
- structural-path identity, nearest-match, fuzzy matching, or scoring (see §7)

## 3. Status at a glance

| lane | what it is | state |
|---|---|---|
| WD0 | discovery + GPL boundary | committed |
| WD1.1 | source mapping (`src/vrml/source-map.js`) | committed `4cf7398` |
| WD1.2 | span-patch edit algebra (`src/vrml/edit.js`) | committed `846800d` |
| WD1.3 | generated VRML97/X3D node schema (`src/vrml/node-schema.js`) | committed `94971a1` |
| WD1.4 | two-tier node identity (`node-identity.js`, `document-transaction.js`) | committed `5328262` |
| WD1.5 | scope semantics **design gate** | committed `bf4f8f9` (spike + plan only) |
| WD1.5-P1 | DEF/USE scope graph (`symbols.js`, `scope-graph.js`) | committed `66783c1` |
| WD1.5-P2A | PROTO/EXTERNPROTO type-name resolution | committed `5176d28` |
| WD1.5-P2B | interface members + `IS` (§8.1) | committed `51ff283` |
| WD1.5-P2C | ROUTE endpoints (§8.2) | **implemented, uncommitted** |
| WD2 | scene tree / inspector / viewport | **not started** |

## 4. WD1.1 — source mapping

Read-only offset→token/node lookup over a parse result. **Opt-in and lazy**:
`parse()` does not build one, so nothing existing pays for it.
`createSourceMap(parseResult)`.

The tokenizer is already **byte-lossless** — WD0 established this, which is why
no CST and no serializer exist or are needed.

## 5. WD1.2 — span-patch edit algebra

`src/vrml/edit.js`. Pure text-in/text-out edits anchored to the exact spans the
parser and source map report.

Owner-ratified invariants — **WD1.4+ must not reinterpret them**:

- same-offset inserts are **rejected**, not ordered by guesswork
- canonical order is `from` ascending, **insertion-first**

## 6. WD1.3 — node schema

`src/vrml/node-schema.js`, generated and **committed** (the product and the tests
consume the committed file; `npm install`/`test`/`start` never run the generator).

Two license-clean inputs only: the local **ISO/IEC 14772-1** mirror at
`~/Projects/cybertown/wb-ct-scrape/specs/iso-14772-vrml97` (normative authority)
and the MIT **`x_ite.d.ts`** (current X3D runtime shape). It extracts **facts** —
node names, field names, type tokens, access categories, defaults — and copies
**no prose**.

Counts: 312 ISO declarations across 54/54 VRML97 nodes; 541 x_ite fields; **232
X3D-only fields that must not leak into a VRML97 export**. Use
`isFieldAllowed(node, field, 'vrml97')` before emitting a field.

Regenerate with `node scripts/build-node-schema.js`; verify with `--check`.

## 7. WD1.4 — stable node identity

**The hard gate, which governs everything downstream:**

> A selection may be **lost**. A selection may be **ambiguous**. Identity may say
> it cannot be proven. It may **never** confidently return a different node.

Two tiers, both pure and browser-safe, with no callers in production yet:

- **Tier 1 — verified same-transaction re-anchoring.** Maps a node's old span
  forward through an edit set that has been *proven* to be exactly what turned
  one exact text into another (`verifyTransaction`, byte-for-byte). Not durable;
  never survives a reload, external edit, serialization or unknown edit chain.
- **Tier 2 — persistent DEF identity.** A uniquely named DEF plus node type plus
  the PROTO-lexical scope the parse tree itself proves. Order matters and *is*
  the safety property: **more than one match is ambiguous, decided on the name
  alone, before any type filtering.**

Sessions are bound by **object identity**, never by `sessionId` — the id is
diagnostic only. A restartable counter once let two documents mint the same id
and resolve a selection into the wrong tree.

Scope keys are **opaque and NUL-separated**: `/` is legal in a PROTO name, and a
`/`-joined key made `PROTO A/B` collide with `PROTO A { PROTO B }`.

**Rejected and absent from the source:** structural-path identity (1,020+ wrong
anchors), fingerprint identity, sibling-index identity, closest-range matching,
fuzzy scoring, retained source fingerprints, hidden ids, sidecar metadata.
`test/vrml/node-identity.test.js` asserts their absence by source scan *and* by
behaviour. Do not reintroduce them.

## 8. WD1.5 — scope semantics (design gate)

`spikes/wd1-scope-semantics/` + `docs/white-dune-2026/WD1_5_SCOPE_SEMANTICS_PLAN.md`.
A research gate. The spike and the plan are committed; **no production scope
module exists** — `src/vrml/symbols.js` and `src/vrml/scope-graph.js` are designed
here and built in **WD1.5-P1**, a separate approved lane.

**Outcome A: the current AST is sufficient** — a production scope resolver needs
no parser change, no AST change and no identity redesign.

VRML97 has **three namespaces**, and conflating them is the usual way to get this
wrong: node names (DEF) · node types (PROTO/EXTERNPROTO) · interface members.
Built-in node and field names are **schema lookups, not lexical symbols**.

The load-bearing rule is **ISO 4.8.4**: a PROTO establishes a DEF/USE scope
**separate** from the rest of the scene and from nested PROTOs, in *both*
directions. That is **disjointness, not shadowing** — a PROTO body's node-name
lookup has no parent and simply stops.

`src/vrml/analyze.js` is documented as flat and **non-authoritative**, and the
gate measured exactly how far that goes: against 59 independently authored
expected-truth cases it is wrong in **15** — cross-PROTO false duplicate-DEF,
USE-before-DEF accepted, PROTO DEF leakage both directions, ROUTE endpoints
treated as global. All sit inside the `VRML040`–`VRML044` **advisory** set that
never blocks a save, so nothing shipped is broken.

**Three rules that are easy to get wrong** (each cost real damage in the spike):

1. A node body's `ROUTE`/`PROTO`/`EXTERNPROTO` statements land in `node.fields` —
   only interface declarations get their own array. Dispatch on `type`.
2. ISO **4.4.4** scopes the acyclicity rule to the *transformation hierarchy* and
   excludes `Script` descendants, so `DEF S Script { field SFNode me USE S }` is a
   standard idiom, not a cycle.
3. A damaged scope must withhold **every** lexical answer, positive included —
   parser recovery *moves scope boundaries*, so an unclosed PROTO can absorb
   statements and manufacture a unique binding out of an ambiguous one.

### 8.1 WD1.5-P2B — interface members and `IS`

`src/vrml/symbols.js` + `src/vrml/scope-graph.js`, internal and **consumer-free**
like P1/P2A. Doc: `docs/white-dune-2026/WD1_5_P2B_INTERFACE_IS_PLAN.md` (as
built). The **third namespace** is now populated, in its own tables: a `DEF Ball`,
a `PROTO Ball` and a `field SFBool Ball` never collide.

Three **interface scopes** (`proto-interface`, `externproto-interface`,
`script-interface`) carrying **neither `defParent` nor `typeParent`** — an
interface is an *ownership* scope, so an outward walk is structurally impossible
rather than forbidden. An `IS` reaches its interface through an owner **fixed on
descent**, never a containment search: 4.8.4 gives it the **innermost** enclosing
prototype and is silent on any outer one, and silence fails closed (corpus cost:
**0 of 23,246**).

Three rules worth not re-deriving:

1. **Alias expansion changes the effective access.** `exposedField zzz` also
   occupies `set_zzz` (eventIn) and `zzz_changed` (eventOut) — generated into the
   lookup index, never written anywhere. Table 4.4 applies to the **effective**
   access.
2. **An explicit/alias collision has no winner.** 4.3.5 prohibits
   `exposedField zzz` + `eventIn set_zzz` outright, so neither is "intended":
   `ambiguous`, binding nothing. Preferring the explicit one is candidate
   ranking — the §7 failure mode.
3. **EXTERNPROTO is asymmetric.** What it declares locally is authoritative
   (4.9.2) and is checked normally **without loading the URL**; what it omits is
   `unsupported`, never `unresolved`, because the declaration may be a strict
   subset. P2C inherits both halves unchanged.

Table 4.4 is one matrix in one place (rows = definition, columns = declaration; 7
legal cells of 16, all sixteen tested individually). Field types compare by
**exact token equality** — no coercion, no SF↔MF, no inner node-type check —
against **Annex A.2's `fieldType` production**, which is the authority for *which
tokens are field types* (the schema answers the different question of *what type
a node's field has*, and covers only 19 of the 20: no built-in field uses
`MFTime`).

**Corpus:** 0 wrong bindings and 0 confident answers from an unprovable scope
over 23,246 `IS` statements. One finding handed to P4: **1,481** are genuine
Table 4.4 violations — all the `exposedField` *column* — in authored Cybertown
content. Whether that surfaces as an error or a §9 compatibility warning is P4's
call.

**Deliberate strictness gap:** ISO 4.6.2 defines duplicate-name binding exactly
(*closest preceding*). The resolver returns `ambiguous` and does **not** implement
it, because its consumers are identity and rename, where the §7 hard gate forbids
ranking. If viewer fidelity ever needs the browser's answer it belongs in a
separately named `languageSemantics` query that never feeds identity.

### 8.2 WD1.5-P2C — ROUTE endpoints

`src/vrml/symbols.js` + `src/vrml/scope-graph.js`, internal and **consumer-free**
like P1/P2A/P2B. Doc: `docs/white-dune-2026/WD1_5_P2C_ROUTE_SEMANTICS_PLAN.md`
(as built). It resolves both ends of a `ROUTE` and judges whether the connection
is legal — and it adds **no fourth namespace**.

**ISO 4.6.2 is the decisive citation:** a DEF'd node "may be referenced by name
later in the same file with USE **or ROUTE** statements". ROUTE node names are
therefore P1's namespace under P1's rules — no second DEF table, and 4.8.4
disjointness in both directions for free. The event names are not a lexical
namespace at all: once the node binds, the event is answered by that node's
**public interface**, so P2B's endpoint acquisition was **extracted** into a
module-private `acquireEndpointOn` and shared rather than forked.

A ROUTE asks **six independently answerable questions** (two node bindings, two
endpoints, two directions, one type verdict), and keeping them six is the design:
a lost NODE must never come back as a missing EVENT, and a provable source
binding survives a damaged destination.

Three rules worth not re-deriving:

1. **ROUTE and `IS` have different exposedField rules, running in opposite
   directions.** 4.7 *expansion* means a declared `exposedField zzz` also
   occupies `set_zzz`/`zzz_changed` (P2B, shared). 4.10.2 *shorthand* means a
   written bare `zzz` falls back to them — **ROUTE only**, so it lives in a
   ROUTE-only wrapper. There is **no Table 4.4 for ROUTE**; do not import one.
   Order is normative: the written name is tried first.
2. **The fallback's precondition is the safety property.** It fires only when
   the required event is *provably* absent. An `unsupported` EXTERNPROTO miss, an
   `ambiguous` interface or a `recovered` scope prove nothing, so the fallback
   does not fire — otherwise it could bind `set_zzz` in an implementation that
   also declares a real `eventIn zzz`, a wrong endpoint binding.
3. **R19 was settled by owner adjudication** (2026-08-07): the lookup is
   direction-specific, so a written `zzz` found only as a `field` has *not* found
   the required event and the fallback applies.

**Corpus:** 0 wrong node bindings, 0 wrong endpoint bindings and 0 confident
answers from an unprovable scope, against an independently authored oracle, over
**245,540 ROUTEs in 4,466 unique decoded documents** (14,226 discovered paths at
that measurement; the raw-path count drifts with the external roots, the
canonical denominator has not) through the production path. Direction violations: **0**. Type mismatches: **1**.
That is a striking contrast with P2B's 1,481 Table 4.4 violations in the same
corpus — the ROUTE rules were followed essentially universally, so P4 should not
assume P2B's remediation volume repeats here.

The denominator matters and is easy to get wrong twice over. De-duplicate by
**decoded** text, never raw bytes — a `.wrz` and its `.wrl` twin are one
document, and byte-dedup overcounted by ~32%. And the same ROUTEs counted per
discovered path rather than per unique document total 574,073, a 2.3× inflation.
A ROUTE figure quoted without its denominator is unusable.

The sweep and the oracle are **reproducible from the repository** —
`spikes/wd1-route-semantics/`, deterministic, read-only, boundary-guarded, with
the oracle structurally unable to load the resolver it grades. They were a
session scratchpad first, which cost the lane a
`BLOCKED — EVIDENCE INSUFFICIENT` QA verdict; a hard zero-wrong-bindings gate
whose measurement cannot be rerun is an assertion, not evidence.

`0 confident answers from an unprovable scope` is a **measured result for this
corpus**, not a universal proof. The audit keys that invariant on the ROUTE
statement's own enclosing scope, which is sound here because every observed
recovery-bearing endpoint is a case where that same scope is the recovered one.
A future corpus could hold the other shape — a clean ROUTE scope whose target's
*owning interface scope* is independently recovered — and would need a more
granular key **in the harness**. Production is unaffected either way: it already
withholds an endpoint whose required interface scope is unprovable.

**Reverse indexes are proven-only, and the bar is the WHOLE ROUTE** — both node
bindings, both endpoints, both directions and an exact type match. A half-proven
edge is indistinguishable from a proven one to a rename or scene-tree consumer.

`REFERENCE_KIND` is now **complete** for WD1.5: `route-node` and `route-event`
were the last two kinds the taxonomy declared but nothing built.

## 9. Standards-first, always

Core semantics derive from **ISO/IEC 14772-1**, not from any viewer's behaviour.

Cybertown and Blaxxun permissiveness is an **optional compatibility profile**,
classified and tagged — never promoted into the language rules. Observed in the
corpus and handled that way:

| construct | strict VRML97 | disposition |
|---|---|---|
| ROUTE/PROTO inside an MFNode array | non-conforming | parser recovery, already shipped |
| event bound to an `exposedField` declaration | violates Table 4.4 | compatibility profile |
| `exposedField` in a `Script` node | forbidden (6.40) | compatibility warning |
| hyphen/plus in identifiers | **conforming** | not a compatibility item at all |

When corpus behaviour differs from the standard: identify it, classify it,
preserve it where possible, and recommend whether it belongs in strict behaviour,
a compatibility warning, a profile, parser recovery, or unsupported. **Never
silently normalize vendor behaviour into standard behaviour.**

## 10. Working rules for this lane

- Research and spikes live under `spikes/<lane>/`; generated artifacts under
  `spikes/<lane>/out/`, which is gitignored and regenerable.
- Spike tests are **not** collected by `npm run check` (`scripts/run-tests.js`
  enumerates named directories under `test/`), so the production count is
  unaffected.
- Corpus work is **read-only**, boundary-guarded (a forbidden path **throws**,
  never silently skips), deterministic (fixed seed, codepoint ordering, no clock,
  no PRNG), and reports sanitized `group:relative/path` identifiers — never a
  private absolute path.
- The corpus roots are **external workspace trees that change independently**.
  Record a fingerprint over the discovered file set so drift is visibly an
  *input* change rather than an unstable analysis.
- An expected-truth model must be **independently authored** and structurally
  prevented from importing the thing it grades.
