# RUST-1 — Boundary Contract (Rust text core ↔ JavaScript)

Status: **isolated implementation. No production caller. Not approved for
production.** Date: 2026-10-08. Branch: `architecture/rust-1-wasm-boundary`.
Companions: `RUST_1_WASM_QA.md`, `RUST_1_PERFORMANCE.md`, RUST-0 documents.

## 1. Scope and ownership

| owner | holds |
|---|---|
| JavaScript (unchanged) | CodeMirror document + undo, UI state, X_ITE, Electron IPC, filesystem authorization, safe save, gzip/repack, asset loading, preview authorization |
| Rust (`crates/wrlforge-text`) | pure computation over a text it is given: UTF-16 ↔ UTF-8 offsets, span validation, WD1.2 edit algebra, `mapOffset`/`mapRange`, line index |
| Rust (`crates/wrlforge-wasm`) | the wasm-bindgen boundary, the UTF-16 gate, one isolated session type |
| JS facade (`crates/wrlforge-wasm/js/wrlforge-text.mjs`) | argument shapes, error compatibility, session/snapshot branding |

Rust never writes a document, never touches a file, the network, a clock, the
environment or a process. `#![forbid(unsafe_code)]` in both crates.
`wrlforge-text` depends on `std` only.

## 2. JavaScript-facing API

`createTextEngine(glue)` takes an **initialized** wasm-bindgen module
namespace and returns a frozen engine. Loading is environment-specific
(§8); the facade is not.

| call | returns | notes |
|---|---|---|
| `engine.info` | string | includes `negative-controls=[]` for a release artifact |
| `engine.checkText(text)` | `true` | or `EENCODING` with `unit` |
| `engine.applyEdits(text, edits)` | string | stateless; full text in and out |
| `engine.mapOffset(offset, edits, affinity='before')` | number | text-free |
| `engine.mapRange(range, edits, {startAffinity, endAffinity})` | `{from,to}` | `{from,to}` or `{start:{offset},end:{offset}}` |
| `engine.openSession(text)` | `TextSession` | |
| `session.current()` | `Snapshot` | current revision |
| `session.update(base, text)` | `Snapshot` | the JS owner reports its new exact text; `base` must be current |
| `session.propose(base, edits)` | string | proposed text; session unchanged |
| `session.verifyTransaction(base, edits, after)` | `{verified, revision, length}` | full-text comparison |
| `session.dispose()` / `session.disposed` | | idempotent |
| `snapshot.revision` | number | informational label only |
| `snapshot.length`, `.text()` | | exact text |
| `snapshot.toUtf8(o)`, `.fromUtf8(b)` | number | |
| `snapshot.validateSpan(from,to)` | `{from,to,utf8From,utf8To}` | |
| `snapshot.lineCol(o)` / `.offsetAt(line,col)` | | 1-based line, 1-based **UTF-16** column |
| `snapshot.lineInfo()` | `{lines, lf, crlf, cr, mixed}` | detection only, never normalization |

Offsets are **UTF-16 code units**, half-open, exactly as `src/vrml/edit.js`,
the tokenizer, the source map and CodeMirror. UTF-8 offsets are exposed only
as explicitly named conversions.

The raw wasm exports (`RawSession`, `apply_edits`, …) are **not** API. They
are never returned by the facade. `probe_is_valid_utf16_by_units` exists only
for the performance comparison.

## 3. UTF-16 safety (the critical gate)

wasm-bindgen converts a JS string to a Rust `String` with `TextEncoder`, which
replaces an unpaired surrogate with U+FFFD. RUST-1 **proved** this by
experiment: the `negative-control-lossy-utf16` artifact turns input
`"\uDC00"` into `"�"` with no error (1,108 differential regressions).

The gate, in `wrlforge-wasm/src/lib.rs` `take_text` (**amended by RUST-1A**;
see `RUST_1A_BOUNDARY_HARDENING.md` §3 for the defect and the original design):

1. Every JS string arrives as a `JsString` (no automatic conversion).
2. Rust calls `String.prototype.isWellFormed()`, bound with `catch`. Missing
   or throwing → `EENGINE`. The facade also refuses to start without it. Its
   answer is a **claim, never proof**: RUST-1A reproduced 144 silent
   substitutions per lying mode when the gate trusted it.
3. The string is converted, then checked **independently of step 2**:
   `TextEncoder` replaces one unpaired surrogate with one U+FFFD and copies
   every other scalar, so UTF-16 positions are preserved. Each U+FFFD in the
   converted text must sit over a genuine `0xFFFD` source unit
   (`String.prototype.charCodeAt` at that index). No U+FFFD → no unit reads.
4. A U+FFFD over a surrogate unit → `EENCODING`, with `field`
   (`text`/`insert`/`after`), `unit` (index of the first unpaired surrogate)
   and `index` (edit index, for inserts). A U+FFFD over any other unit →
   `EENGINE`.
5. A `false` claim in step 2 for text that step 3 proves well formed → `EENGINE`
   (the method lied). Fail closed; never accept a substitution.

Trust root: the intrinsics the wasm-bindgen glue itself uses to move a string
(`TextEncoder.prototype.encodeInto`, `String.prototype.charCodeAt`). Code that
replaces those already controls every byte the glue writes.

Rejected alternatives: `JsString::is_valid_utf16` (one `charCodeAt` call per
code unit — measured 1.3–11× slower than the native gate, see performance doc); a `Uint16Array` copy
plus `String::from_utf16` (an extra full copy built in JS). Both remain
correct; neither is smaller or faster. `decode_utf16_strict(&[u16])` exists in
`wrlforge-text` for native callers and tests.

Rust → JS strings are always valid UTF-8. The glue decodes with
`TextDecoder('utf-8', { fatal: true, ignoreBOM: true })`; `ignoreBOM: true`
keeps a leading U+FEFF (tested).

A legitimate U+FFFD in the input is ordinary text and round-trips (tested
separately from unpaired surrogates). Nothing is normalized: CRLF, lone CR,
BOM, NUL and trailing whitespace pass through unchanged.

## 4. Error taxonomy

Every refusal is a JS `Error` with a stable `code`. Codes that `edit.js` also
uses keep its meaning, its `index` (caller array position) and `otherIndex`.

| code | source | meaning |
|---|---|---|
| `EEDITSHAPE` `EEDITBOUNDS` `EEDITOVERLAP` `EEDITAMBIGUOUS` `EEDITRANGE` `EEDITAFFINITY` `EEDITINVERTED` | as `edit.js` | identical semantics, check order and indexes |
| `EEDITBOUNDARY` | Rust only | edit endpoint inside a surrogate pair (registry `RUST0-BOUNDARY`) |
| `EENCODING` | boundary | unpaired surrogate; `field`, `unit`, `index` |
| `EOFFSETPRECISION` | boundary | text-free offset or result above `Number.MAX_SAFE_INTEGER` (registry `RUST1-PRECISION`, pending owner) |
| `EOFFSETBOUNDS` `EOFFSETSURROGATE` `EOFFSETBYTE` | snapshot | past end / inside a pair / inside a UTF-8 scalar |
| `ESPANINVERTED` `ELINE` `ECOLUMN` | snapshot | |
| `ESESSIONFOREIGN` `ESESSIONSTALE` `ESESSIONDISPOSED` `ESESSIONREVISION` `ESESSIONEXHAUSTED` | session | §5 |
| `EVERIFYMISMATCH` | session | with `firstDivergence` (UTF-16 index) |
| `EARGUMENT` | facade/raw | wrong argument type for a non-`edit.js` call |
| `EENGINE` | facade/raw | missing export, missing or lying `isWellFormed`, a conversion that changed a non-surrogate unit, or a poisoned instance |

Check order for edit calls: facade shape checks in caller order (as
`edit.js`) → UTF-16 gate → bounds → set conflicts (canonical order) →
surrogate boundary. Therefore `EENCODING` can pre-empt a later `EEDITBOUNDS`
or `EEDITOVERLAP`; it never pre-empts a shape error.

An error **without** a `code` (a wasm trap) **poisons** the wasm **instance**
(RUST-1A; RUST-1 poisoned only the engine that saw the trap): every later call
through **any** engine on that instance refuses with `EENGINE`. A trapped
instance is never trusted again. A separately loaded instance is unaffected.

## 5. Session ownership and revisions

- Proof of a session or snapshot is **membership in a module-private
  `WeakMap`** inside one `createTextEngine` closure. A number is never
  authority. A session from engine A is foreign to engine B even on the same
  wasm instance.
- Facade objects are frozen and have **no own properties**
  (`Reflect.ownKeys(session)` is `[]`); `__wbg_ptr` is never reachable.
- Defence in depth in Rust: each session has a `serial`; each revision comes
  from one per-instance monotonic counter that is **never reset**, so no two
  sessions and no two revisions share a number. The raw layer refuses a serial
  it does not own (`ESESSIONFOREIGN`), a revision it issued earlier
  (`ESESSIONSTALE`), and any revision it never issued, including `NaN`,
  negatives, fractions and values ≥ 2^53 (`ESESSIONREVISION`).
- **Overflow:** the counter refuses to pass `Number.MAX_SAFE_INTEGER`
  (`ESESSIONEXHAUSTED`), leaving the session unchanged. Every revision a caller
  sees is an exact JS number. (Tested natively from a counter started near the
  limit.)
- Disposal marks the facade record dead **before** calling `dispose()` and
  `free()`; the text is released; every later call refuses with
  `ESESSIONDISPOSED`. Idempotent.
- Two separately loaded wasm instances have independent counters, so their
  numbers can coincide. This is safe: the numbers are not authority, and a
  facade never routes a handle to another instance.
- **Trust boundary (RUST-1A).** Session isolation holds for a caller that
  holds only an engine. The raw layer is not an isolation boundary: wasm-bindgen
  trusts `__wbg_ptr`, so code that holds the glue module can forge a raw handle
  and read another session's text (reproduced), or read linear memory directly.
  The renderer must not expose the glue module to untrusted code.

  The facade enforces session ownership for callers using the public facade
  API. Raw generated WebAssembly exports are trusted internal implementation
  details and do not independently enforce equivalent ownership. Before
  production integration, the application must prevent untrusted code from
  obtaining or invoking raw handles, or must strengthen the raw boundary.
- **Revision record (RUST-1A, measured).** `SessionCore.issued` keeps one `u64`
  per revision so a stale number is told apart from a never-issued one: about
  21.6 B of retained metadata per issued revision (200,000 revisions → 4.3 MB),
  released on `dispose`. This is live session state, not the wasm memory
  high-water mark (linear memory never shrinks, but freed memory is reused).
  It is bounded by session lifetime only. Before production integration, a
  bounded-history or equivalent memory strategy is required.
- No persistent document IDs; nothing is written into VRML source; WD1.4
  object-identity design is untouched.

## 6. Derived snapshot lifecycle

A `TextSnapshot` is an immutable copy of **one** exact source revision plus
its line index. It is not a second canonical document: `propose` returns a
string and changes nothing; only `update` (the JS owner's report) creates the
next revision, which releases the previous snapshot. A stale snapshot can
still report its `revision` label but refuses every query.

## 7. Transaction equivalence

`verifyTransaction` applies the edits to the snapshot text and compares the
result with `after` **in full**. No hash is computed or trusted. A mismatch
reports `firstDivergence`.

## 8. Serialization and initialization

- Strings cross the boundary as copies (JS UTF-16 ↔ wasm UTF-8). Edit offsets
  cross as `Float64Array`; inserts as a JS array of strings, each gated.
- Results cross as strings, numbers, or `Float64Array`s re-wrapped into frozen
  plain objects. No JSON, no `serde`.
- Glue: `wasm-bindgen --target web` (ES module). Node: `initSync({module:
  bytes})` (`spikes/rust-1-wasm-boundary/loaders/node.mjs`). Electron
  renderer: three loaders proven (fetch + `initSync`, glue default async, bytes
  embedded in a JS module). The embedded-bytes shape is what an esbuild
  `binary` loader would produce. Repeated initialization is idempotent.

## 9. Fallback

There is no production caller, so there is no fallback path yet. The RUST-0
roadmap rule stands for RUST-2: JavaScript stays the default per module behind
a switch; any `EENGINE` (load failure, missing `isWellFormed`, poisoning) must
route the caller back to `src/vrml/edit.js`, never to a degraded Rust answer.

## 10. Dependencies

Workspace: `crates/Cargo.toml`, `crates/Cargo.lock` (committed for
reproducibility), toolchain pinned in `crates/rust-toolchain.toml`
(1.95.0 + `wasm32-unknown-unknown`, `rustfmt`, `clippy`).

| crate | version | licence | why |
|---|---|---|---|
| wasm-bindgen | =0.2.129 | MIT OR Apache-2.0 | owner-approved bridge (D4) |
| js-sys | =0.3.106 | MIT OR Apache-2.0 | `JsString`, `Array`, `Reflect`, `Error`; **default features off** (drops `unsafe-eval` feature and `futures-util`) |
| transitive (11) | bumpalo 3.20.3, cfg-if 1.0.5, once_cell 1.21.4, proc-macro2 1.0.107, quote 1.0.47, rustversion 1.0.23, syn 3.0.6, unicode-ident 1.0.26, wasm-bindgen-macro(-support) 0.2.129, wasm-bindgen-shared 0.2.129 | MIT OR Apache-2.0 (unicode-ident adds Unicode-3.0) | build-time macros + runtime glue |

All are GPL-3.0-or-later compatible. 13 third-party packages in the lockfile;
all build-time except `wasm-bindgen`, `js-sys`, `cfg-if`, `once_cell`. MSRV of
the set is 1.81; toolchain is 1.95.0. Maintenance: wasm-bindgen/js-sys are the
`wasm-bindgen` organization's actively released crates (0.2.129 is current on
crates.io as of 2026-10-08). Build tool: `wasm-bindgen-cli` 0.2.129 (same
licence), installed lane-locally under `crates/target/tools` (gitignored).
`package.json` and the production dependency graph are unchanged. No upstream
(White Dune or other) code was used; `OPEN_SOURCE_PROVENANCE.md` is unchanged.
A suggested UTF transcoder crate (`xutf` 2.0.1) was considered and **not
added**: `std` provides every conversion needed.
