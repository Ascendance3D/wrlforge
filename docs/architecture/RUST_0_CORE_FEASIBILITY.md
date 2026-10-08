# RUST-0 — Rust Core Architecture and Migration Feasibility

Status: **research + isolated spike. Not approved. No production change.**
Date: 2026-10-08. Branch: `architecture/rust-core-feasibility`.
Companions: `RUST_0_MIGRATION_MATRIX.md`, `RUST_0_MIGRATION_ROADMAP.md`,
`spikes/rust-core-feasibility/`.

Labels used in this document:

- **[FACT]** — read from the repository or produced by a command in this lane.
- **[EXP]** — an experimental result of this lane's spike.
- **[REC]** — an engineering recommendation.
- **[UNVERIFIED]** — an assumption or an external claim this lane did not prove.

---

## 1. Executive verdict

**`RUST_0_PASS_WITH_CONDITIONS`.**

A Rust core is feasible **if and only if** it is a **pure computation core
behind the existing Electron shell**, reached through **WebAssembly** for the
document core, with **UTF-16 code units kept as the public offset unit**. The
exact-source-text contract can survive. The spike proves the highest-risk
boundary (offsets + span patches) with zero regressions.

The main conditions:

1. **The bridge must reach the renderer.** The whole VRML core — parser, scope
   graph, transactions, field edits, identity — runs **in the renderer**, bundled
   by esbuild (`src/editor/browser/editor-view.js:20,368-374`). A native N-API
   addon cannot run there (`contextIsolation: true`, `nodeIntegration: false`).
   An N-API or sidecar bridge would force every keystroke analysis over IPC.
   **[FACT]**
2. **The JavaScript baseline is not fully lossless today.** Three pre-existing
   text-fidelity gaps must be decided before Rust copies or changes them
   (§6.4). **[FACT]**
3. **The shell question is already decided.** APP-ARCH-0 O1 (owner-approved
   2026-10-07) keeps Electron and states that shell replacement is not a
   migration path. Nothing in this lane is new evidence against that. Tauri/Wry
   and Qt stay **not recommended** (§5). **[FACT]/[REC]**
4. **There is no measured performance problem that Rust solves.** The worst
   measured parse is 216 ms median for 1.6 MB, inside the 250 ms gate. The
   migration case is correctness, type safety and owner preference — not speed.
   Do not promise speed. **[FACT]**

The recommended first production lane is **RUST-1** (boundary foundation,
WebAssembly toolchain proof and differential harness), followed by a
JavaScript-side text-fidelity lane. No production Rust code before both.

---

## 2. Repository baseline **[FACT]**

| item | value |
|---|---|
| repository | `Ascendance3D/wrlforge` |
| research worktree | `~/Projects/cybertown/.worktrees/wrlforge/rust-0` |
| branch | `architecture/rust-core-feasibility` (new, local only) |
| base SHA | `afb4158da39cdb328238c6f6259465947301dfe3` (`origin/main`, merge of #129 SHELL-0) |
| primary checkout | `main` at `4e8fd35` — **stale**, 32 commits behind `origin/main`, 62 dirty/untracked entries; untouched |
| other active lanes | 33 registered worktrees incl. `ui0-i2` (#121, uncommitted), `wd2-*`; untouched |
| existing Rust code | none (`rg --files -g '*.rs' -g Cargo.toml` empty before this lane) |
| native runtime deps | none. Runtime dependency is `x_ite` 15.1.10 only. `@resvg/resvg-js` (napi-rs based) is a devDependency for icons |
| build/test | `npm test` (`scripts/run-tests.js`, explicit dir list, `node --test`), `npm run check` (`scripts/run-checks.js`: tests + `node --check`), `npm run build:editor` (esbuild) |
| test count | ~2,500 `test(`/`it(` sites; 1,043+ in `test/vrml` |
| CI | `ci.yml`: Linux, Windows, macOS on Node 24. `release.yml`: Linux (AppImage, tar.gz), Windows (NSIS, MSI, portable), macOS arm64 (signed, notarized) |
| toolchain on host | rustc/cargo 1.95.0, Node v24.21.0, Linux x86_64. `wasm32-unknown-unknown` **not installed** |
| WD2 status | WD2-A (scene tree/inspector), WD2-B (typed inspector edits), WD2-C (first object), WD2-D (source-proven picking) merged; UI-0, SHELL-0 merged; #121 in progress |

### 2.1 Platform-policy conflicts (for owner review, not changed)

- **P1.** RUST-0 excludes Windows. The repository ships Windows: CI and
  `release.yml` build it, `CLAUDE.md` describes a validated Windows beta, and
  `qa/phase-7c-windows/` exists. RUST-0 removes nothing. Any Rust stage that
  adds a native artifact would have to either build for Windows or explicitly
  break the current Windows release. **Owner decision D6.**
- **P2.** Electron 41 is past end-of-life (EOL 2026-08-24 per
  `releases.electronjs.org/schedule`, retrieved 2026-10-08 by a research
  agent). **[UNVERIFIED by this lane's own test]** This is independent of Rust
  and needs a separate maintenance lane.
- **P3.** `x_ite` 15.1.10 is pinned; 15.1.12 and 16.1.0 are published.
  **[UNVERIFIED]** whether 16.x breaks the picking adapter.

---

## 3. Existing engine inventory

Full per-subsystem table: `RUST_0_MIGRATION_MATRIX.md`. Summary:

| subsystem | LOC | runs in | privileges | class |
|---|---|---|---|---|
| `src/vrml/` text/edit leaves (`edit`, `source-map`, tokenizer) | ~1.3k | renderer **and** main | none | **A** |
| `src/vrml/` parser, scope graph, symbols, identity, transactions, WD2 transforms | ~21k | renderer **and** main | none | **B** (A after RUST-2/3 proof) |
| `src/vrml/node-schema.js` (generated) | 6.7k | both | none | **A** (regenerate as Rust data) |
| `src/world-project/` | 2.4k | main | fs read, one bundle write | **A** for pure parts; I/O stays JS |
| `src/mall/`, `src/files/`, `validator.js` | 0.6k | main | fs via `safeSave`, zlib | **A** for rules; **D** for deflate output |
| `src/external-proto/`, `proto-resolution/`, `proto-enrichment/` | 2.6k | main | fs read, realpath | **B** (moves with `src/vrml`) |
| `src/editor/` main side (`file-io`, `path-authorizer`, sessions, recovery) | ~1.5k | main | heavy fs, child_process | **D** (trust boundary; no Rust benefit yet) |
| `src/editor/` renderer side, `src/editor/browser/`, `renderer/` | ~6k | renderer | none | **C** |
| `src/preview/` main side | ~0.8k | main | fs read, gunzip | **B** |
| `src/preview/` renderer side, `xite-pick-adapter.js` | ~2k | renderer | none | **C** |
| `src/shell/` | 1.3k | not wired | none | **D** |
| `main.js`, `preload.js` | 1.7k | main | all IPC (43 handlers) | **C** |

Key execution-path facts **[FACT]**:

- `src/vrml/*` is CommonJS. It reaches the renderer **only** through the esbuild
  IIFE bundle `renderer/vendor/wrl-editor.bundle.js` (`editor.html:36`). The
  comment at `editor.html:~50` that says these modules go "through main-process
  IPC" is stale.
- In the main process, `src/vrml` is used by `world-project/externproto-deps.js`,
  `proto-resolution`, `proto-enrichment`, and `editor/language.js`.
- WD1.4 identity and WD1.5 symbol ownership use **module-private `WeakMap`s**
  keyed by JS objects (`node-identity.js:217,419,479`, `symbols.js:475`,
  `document-transaction.js:32`). Proof of a session is object identity.
- WD2 modules reach AST nodes by object identity (`astNodeForItem`).

Highest-risk migration areas: (1) the renderer boundary, (2) AST object
identity consumed by WD2, (3) UTF-16 offsets everywhere, (4) gzip/deflate byte
compatibility with the 81,920 B Mall limit, (5) the 22 kLOC semantic core with
its corpus-proven zero-wrong-binding gates.

---

## 4. Proposed Rust core architecture **[REC]**

### 4.1 Principles

1. **Rust is pure.** No filesystem, network, clock, environment, or process
   access in any core crate. I/O authority stays in the existing, reviewed
   main-process JS (`file-io.js`, `path-authorizer.js`) until a separately
   approved lane moves it. This keeps the trust boundary exactly where it is.
2. **The text is the document.** Rust receives the exact text and returns
   derived projections or edit sets. It never holds a second editable buffer.
   CodeMirror stays the text and undo authority (SHELL-0 `document-session.js`).
3. **UTF-16 code units are the public offset unit** at every JS boundary.
   UTF-8 byte offsets are an internal detail of one crate.
4. **Fail closed.** Every refusal is a typed error; nothing is clamped, merged,
   rounded, or guessed.
5. **Few crates.** Split only where a dependency direction must be enforced.

### 4.2 Workspace (five crates, not ten)

```text
wrlforge-text      offsets, SourceText, spans, edit algebra, line index
   ^
wrlforge-vrml      tokenizer, parser, AST, source map, node schema (data)
   ^
wrlforge-semantics scope graph, symbols, IS, ROUTE, identity, transactions,
                   containment, findings, field/structure edits
   ^
wrlforge-project   asset graph, path policy (lexical), package plan, zip
                   writer, image size, Mall rules (pure; host supplies bytes)
   ^
wrlforge-wasm      the only crate with a JS boundary: wasm-bindgen facade,
                   handle/capability checks, UTF-16 conversion at the edge
```

Merged candidates and why:

- `wrlforge-schema` → inside `wrlforge-vrml`. The schema is generated data with
  one consumer path; a crate adds a version boundary without a benefit.
- `wrlforge-identity` + `wrlforge-edit` → inside `wrlforge-semantics` and
  `wrlforge-text`. Identity depends on the scope graph (WD1.4 Tier 2 uses
  PROTO-lexical scope keys). Splitting them would create a cycle or a leaky API.
- `wrlforge-geometry` → **not proposed now.** The only geometry today is
  renderer-side fit/bounds math that reads X_ITE state (`bbox-traversal.js`,
  browser-only). Revisit with WD2-E manipulators.
- `wrlforge-platform` → **not proposed.** Principle 1 forbids I/O in the core.
- `wrlforge-bridge` → named `wrlforge-wasm`; an N-API crate is only added if
  RUST-6 proves a main-only need.

| crate | purpose | depends on | public API (sketch) | ownership | errors | JS equivalent | tests | priority |
|---|---|---|---|---|---|---|---|---|
| `wrlforge-text` | exact text + offset algebra | std | `SourceText::new(String)`; `utf16_len`; `utf16_to_byte`; `apply_edits(&SourceText,&[Edit])->Result<String>`; `map_offset`; `LineIndex` | owns an immutable `String` + lazy indexes | `EditError{code,index,other_index}`, `OffsetError` | `edit.js`, offset parts of `tokenizer.js`/`source-map.js` | spike tests + differential vs `edit.js` | 1 |
| `wrlforge-vrml` | lossless parse | text | `tokenize(&SourceText)->Tokens`; `parse(&SourceText,Limits)->Parse`; `SourceMap` | arena-owned AST, spans in UTF-8 internally, UTF-16 at export | diagnostics as data, never `panic` | `tokenizer/parser/ast/source-map/node-schema/diagnostics` | `test/vrml` fixtures + JSON AST differential | 2 |
| `wrlforge-semantics` | WD1.4/WD1.5 | vrml | `ScopeGraph::build(&Parse)`; `resolve_*` returning `Resolved/Ambiguous/Unresolved/Unsupported/Recovered`; `verify_transaction` | borrows the parse; no interior mutability | status enums, never `Option` for "not proven" | `scope-graph`, `symbols`, `node-identity`, `document-transaction`, `field-edit`, `structure-edit` | all WD1.5 oracles + corpus sweeps | 3 |
| `wrlforge-project` | pure project analysis | vrml | `AssetGraph::from_texts(host_reader)`; `package_plan`; `zip_bytes`; `mall_rules(text)` | host callback supplies bytes; no `std::fs` | typed findings | `world-project/*` pure parts, `validator.js` rules | `test/world-project` + byte-identical ZIP | 4 |
| `wrlforge-wasm` | JS boundary | all | opaque `DocumentHandle`; intention-based calls only | owns sessions; JS holds wrapper objects | JS `Error` with stable `code` | facade over `src/vrml/index.js` | boundary + stale-handle tests | with 1 |

### 4.3 Where state lives

| state | owner |
|---|---|
| source text, undo history | CodeMirror (renderer); main `EditorSession` for saved baseline |
| paths, authorization, file I/O, gzip I/O | main-process JS (unchanged) |
| derived parse/semantic products | Rust session, keyed to one exact text revision |
| selection | `sceneSelection` (renderer JS), holding Rust anchors as opaque values |
| presentation, layout, panels, theme, zoom | renderer JS |
| X_ITE scene | X_ITE (a projection, never authoritative) |

### 4.4 Document-session interface

A numeric handle is **never** proof. Each Rust parse session is exposed as a
wasm-bindgen object (a JS wrapper). The JS facade keeps today's rule: a session
is proven by **membership in a module-private `WeakSet` of facade objects**, not
by an id.

Every derived result and anchor carries a **binding** `{ session, revision,
textDigest }`:

- `revision` — a per-process monotonic `u64` that is **never reset** (WD.md §7:
  a restartable counter once minted duplicate ids).
- `textDigest` — a 128-bit hash of the exact UTF-16 text for fast mismatch
  detection. **Equality is never decided by the hash alone**: a call that
  accepts text compares the full text, exactly as `verifyTransaction` does
  byte-for-byte today.

Fail-closed rules:

| condition | result |
|---|---|
| stale revision (anchor from revision N used on N+1) | `refused: stale-revision`; Tier-1 re-anchoring only through a verified receipt |
| disposed session | `refused: disposed`; wasm memory freed; facade marks dead before free |
| foreign session (anchor from another document) | `refused: foreign-session` |
| ambiguous identity | `ambiguous`, binds nothing (WD1.4 hard gate) |
| parse recovery inside the relevant scope | `recovered`, withholds every lexical answer, positive included (WD.md §8 rule 3) |
| text mismatch on verify | `refused: text-mismatch` |

---

## 5. Shell comparison

The shell must host X_ITE (WebGL/WebGL2) reliably on Linux and macOS 15+.

| criterion | A Rust + Tauri/Wry | B Rust + Qt WebEngine | C Rust core + Electron |
|---|---|---|---|
| engine | WKWebView (macOS), **system WebKitGTK** (Linux) | Chromium via Qt; ~M134–M140 in Qt 6.10/6.11 | bundled Chromium (M146 in Electron 41; M150/M152 in 43/44) |
| X_ITE evidence | **none** found on WebKitGTK | none; Chromium family, likely | **proven** on all three OSes, incl. a 72-texture world in 847 ms (APP-ARCH-0 §9) |
| Linux WebGL risk | Tauri's own docs: DMABUF/NVIDIA blank windows; WebGL2 can silently fall back to software, undetectable from JS **[external, retrieved 2026-10-08]** | Chromium GPU stack; distro-independent if bundled | Chromium GPU stack, bundled, same on every distro |
| local asset scheme | `with_custom_protocol` exists; CSP interaction **[UNVERIFIED]** | `QWebEngineUrlSchemeHandler` | `wrlworld:` privileged scheme, reviewed |
| CodeMirror | runs, untested in WebKitGTK | runs | proven |
| docking | web docking (UI-C0) | native Qt ADS (LGPL-2.1; licence wording conflicts upstream vs distros) | web docking (UI-C0) |
| Rust interop | native | C++ boundary; `cxx-qt` 0.10 "early development", no WebEngine binding | wasm-bindgen (renderer + main); N-API optional |
| licence fit | MIT/Apache — clean | LGPLv3/GPL Qt + Chromium licences — plausible, **[UNVERIFIED legal]** | MIT — clean |
| packaging | new pipelines (deb/rpm/AppImage/DMG + notarize) | heaviest (Qt deploy + WebEngine) | exists, in release CI |
| QA harness | rewrite (~1,015 lines + evidence history) | rewrite | unchanged |
| migration cost | all 43 IPC handlers + preload + QA + packaging | everything above + UI toolkit | additive |

**Verdict [REC]:** Candidate C. Electron is a **legitimate long-term shell**, not
only a transition shell, because the renderer engine is the product-critical
dependency and only Electron bundles a uniform Chromium GPU stack today.
Candidate A fails the renderer-reliability criterion on Linux on current
evidence. Candidate B adds a C++ boundary and a lagging Chromium for no
measured gain. Reopening APP-ARCH-0 O1 needs **new** evidence: an X_ITE
conformance run on WebKitGTK across Mesa/NVIDIA/Wayland/X11 (optional RUST-7).

Binary size and modernity were not used as criteria.

---

## 6. Text and offset compatibility (mandatory gate)

### 6.1 Current conventions **[FACT]**

| module | unit | evidence |
|---|---|---|
| tokenizer offsets | UTF-16 code units, 0-based | `tokenizer.js:81-111` (`src.length`, `i += 1` per unit) |
| tokenizer line | 1-based; LF, CRLF and lone CR each count once | `tokenizer.js:92-111` |
| tokenizer column | 1-based, **UTF-16 code units** (an emoji advances it by 2) | measured: `DEF 😀x Group` → `Group` at column 9 |
| source map | UTF-16, half-open | `source-map.js`; `edit.js:27-40` |
| edit algebra | UTF-16, half-open, **no surrogate-boundary check** | `edit.js:27-35` |
| transactions | exact JS string equality, byte-for-byte via `firstDivergence` | `document-transaction.js:235,362` |
| CodeMirror | UTF-16 | CodeMirror 6 |
| identifiers | any non-control, non-delimiter code unit; non-ASCII accepted | `tokenizer.js:58-77`; `DEF café`, `DEF 😀x` parse clean |
| decode from disk | `Buffer.toString('utf8')`, **lossy** | `file-io.js:131,133`, `wrl-source.js:26,31` |
| encode to disk | `Buffer.from(text,'utf8')` | `file-io.js:120` |

### 6.2 Boundary contract **[REC]**

1. The **public** unit at every JS↔Rust boundary is the **UTF-16 code unit**.
   Rust converts at exactly one place (`wrlforge-text::offsets`).
2. Rust storage is a validated `String`. Ill-formed UTF-16 (a lone surrogate)
   is refused with `EENCODING` at the boundary, never replaced.
3. An offset inside a surrogate pair is refused (`EEDITBOUNDARY` /
   `EOFFSETSURROGATE`); a byte offset inside a scalar is refused. No rounding.
4. CRLF, lone CR, BOM, NUL-free control characters and trailing whitespace are
   ordinary text. Nothing is normalized.
5. Line/column stay JS-compatible: column in UTF-16 units. A future
   scalar-based column is a separate, named field, never a reinterpretation.
6. Every exported span is checked in debug builds: `utf16 → byte → utf16`
   must round-trip.

Alternative considered: a Rust core over `&[u16]`. It is lossless even for lone
surrogates and needs no conversion, but it gives up `str`, makes every Rust
string API unusable, and preserves a state the save path already corrupts.
Rejected unless D1 below chooses to preserve lone surrogates.

### 6.3 Spike evidence **[EXP]**

`spikes/rust-core-feasibility/` — std-only Rust port of the UTF-16 boundary and
the WD1.2 algebra, compared against `src/vrml/edit.js` on identical inputs:

- **167,316** cases (exhaustive 1- and 2-edit sets over 8 fixed texts incl.
  CRLF, BOM, BMP, astral and lone-surrogate, plus token spans of **65**
  committed fixtures from the production tokenizer).
- **0 regressions.** 159,990 exact; 3,120 approved D1 (boundary); 4,206
  approved D2 (encoding). Re-runs give identical SHA-256 digests.
- Of the 3,120 D1 cases, **2,723** JavaScript outputs were **ill-formed**
  strings that the save path would write as U+FFFD. Rust refuses them.
- Three mutation controls (UTF-8 tie-break, missing ambiguity check, missing
  boundary check) each produce hundreds to thousands of regressions, so the
  harness is sensitive.
- Found during porting: the canonical tie-break must compare inserts by
  **UTF-16 code units**. Rust byte order changes which edit an error names.
  This class of silent divergence is why every port needs a differential.

### 6.4 Pre-existing text-fidelity gaps in the JS baseline **[FACT]**

| id | gap | evidence | effect |
|---|---|---|---|
| T1 | Invalid UTF-8 on disk (e.g. Latin-1 bytes) decodes to U+FFFD; a save writes `EF BF BD` | `Buffer.from([0x23,0xE9,0x41]).toString('utf8')` → `#�A`; `file-io.js:131-133` | silent byte change on save of a non-UTF-8 file |
| T2 | A UTF-8 BOM is kept as U+FEFF but the tokenizer treats it as an identifier, so the header is "missing" | `parse('﻿#VRML V2.0 utf8\n…')` → `VRML001`, `VRML020` | false diagnostics; text itself preserved |
| T3 | CodeMirror 6 (no `lineSeparator` set) turns CRLF and lone CR into LF | `EditorState.create({doc:'a\r\nb\rc'}).doc.toString()` → `a\nb\nc`; documented in `WD2_C_FIRST_OBJECT.md:283` | every CRLF file becomes LF when saved from the native editor |
| T4 | `edit.js` can split a surrogate pair; the result is ill-formed and the save path turns it into U+FFFD | spike D1 | latent; no current caller is known to do this |

**These are owner decisions (D1–D3), not Rust work.** Rust must not "fix" them
silently and must not copy T1/T4 silently. Recommended policy: decode with a
fatal UTF-8 check and treat an invalid file as **unsupported-encoding,
read-only** until a byte-preserving design exists; keep the BOM bytes but
classify U+FEFF at offset 0 as trivia; set CodeMirror `EditorState.lineSeparator`
from the file's detected line ending (or refuse mixed endings for edits).

### 6.5 How exact preservation is tested

- Differential: identical input to JS and Rust; compare full output strings as
  UTF-16 unit sequences, error code, caller index and other index.
- Independent oracle checks decide approved differences
  (`String.prototype.isWellFormed`, a direct surrogate test), never the Rust
  answer.
- Round-trip: `decode(encode(text)) === text` for every fixture and twin.
- Byte level: gzip twins and CRLF fixtures in `test/fixtures` (`-text`).
- Negative controls: deliberate mutations must fail the harness.

---

## 7. IPC/FFI boundary recommendation **[REC]**

| option | reaches renderer synchronously | new native artifact | crash isolation | fit |
|---|---|---|---|---|
| **WebAssembly (wasm-bindgen)** | **yes** | no (a `.wasm` file, platform-neutral) | wasm traps become JS exceptions; no process crash | **recommended for the document core** |
| N-API addon (napi-rs 3.x) | no (main only) | yes, per OS/arch; `asarUnpack`; macOS signing | a Rust panic aborts main unless caught | optional later, main-only services |
| Sidecar process (stdio JSON) | no (IPC hop) | yes, a signed binary per OS/arch | best | rejected for the core: per-keystroke IPC and full-text copies |

Why WebAssembly:

- The same `.wasm` runs in the renderer (where analysis runs today) and in main
  (where World/PROTO code runs), keeping **one implementation**, as today.
- All three pages already allow `'wasm-unsafe-eval'` for X_ITE's own WASM
  decoders (`editor.html:19`, `index.html:13`, `world.html:14`). No CSP change.
- No per-platform native build, no ABI coupling to Electron, and no change to
  macOS signing. This also removes most of the Windows conflict P1, because the
  artifact is platform-neutral.
- WebAssembly has no filesystem access: principle 1 is enforced by the target.

Costs: strings are copied across the boundary (JS UTF-16 → wasm UTF-8);
wasm32 has a 4 GB address limit; the AST must be projected into JS objects for
existing consumers. **[UNVERIFIED]**: load and memory behaviour inside the
Electron renderer — the RUST-1 gate.

---

## 8. X_ITE integration risks

- X_ITE stays the only renderer. Rust never talks to X_ITE. The preview keeps
  receiving **source text** (saved file or the byte-exact buffer overlay) through
  today's bridges. One authority: the text. **[REC]**
- `xite-pick-adapter.js` stays the single module touching private X_ITE picking
  surfaces. WD2-D source-proven picking maps X_ITE hits back to source spans;
  those spans must stay UTF-16 when the parser moves to Rust.
- Rust-derived geometry (future manipulators) must emit **edit sets**, never
  mutate X_ITE nodes as an authority.
- A Rust migration has **no measured effect on rendering FPS**. None is claimed.

## 9. macOS / Linux support risks

| risk | macOS 15+ | Linux |
|---|---|---|
| WebAssembly in Electron renderer | Chromium, expected to work **[UNVERIFIED]** | same |
| Rust toolchain in CI | add `wasm32-unknown-unknown` target; one build for all OSes | same |
| native addon (if ever) | per-arch `.node`, signing + notarization of the binary | glibc floor per distro |
| host coverage | this lane ran on Linux x86_64 only; **macOS is untested** | tested (spike only) |

## 10. Security and trust boundary **[REC]**

- Rust core crates have **no I/O**. The renderer gains no new capability.
  `preload.js` and the 43 IPC handlers do not change shape.
- No generalized native bridge. If a Rust service ever needs files, main calls
  it with bytes it already authorized (`path-authorizer.js`: lexical + realpath,
  exact case, project-root confinement).
- Existing findings that Rust must **not** copy (APP-ARCH-0 SEC-SHELL-0):
  `mall:check` reads a renderer-supplied `editFile`; `mall:openPath` and
  `shell:revealInFolder` take renderer paths; `world:reveal` is lexical only.
  Fix these in SEC-SHELL-0 before any I/O moves.
- Backup-first safe save, atomic rename, unchanged-gzip preservation and the
  81,920 B refusal stay in `file-io.js` / `repack.js`. **Deflate output is
  encoder-specific**: a Rust deflate would not reproduce Node zlib level-9 bytes,
  and the Mall size verdict measures real bytes. Keep Node zlib (class **D**).
- Unsaved-buffer overlay stays byte-substitution-only and proof-gated.
- No telemetry, network, cloud, upload, or authentication code. wasm cannot
  open sockets.

## 11. Licensing and dependencies

| item | licence | status |
|---|---|---|
| spike | GPL-3.0-or-later, std only | added in spike only |
| wasm-bindgen 0.2.x | MIT OR Apache-2.0 | proposed for RUST-1 (needs approval) |
| napi-rs 3.x | MIT | not proposed now |
| flate2 / miniz_oxide | MIT OR Apache-2.0 | not proposed (deflate stays Node) |
| Tauri 2.x / Wry | Apache-2.0 OR MIT | not recommended |
| Qt 6 WebEngine, cxx-qt, Qt ADS | LGPLv3/GPL + Chromium; MIT/Apache; LGPL-2.1 | not recommended |

No White Dune or other upstream code was read or copied in this lane.
`OPEN_SOURCE_PROVENANCE.md` is unchanged. Versions above come from a research
agent's crates.io/npm lookups on 2026-10-08 **[UNVERIFIED by build]**.

## 12. Testing and benchmark strategy

### 12.1 Differential testing

```text
input ─┬─> src/vrml (JS baseline) ─┐
       └─> Rust (wasm or native)  ─┴─> normalized compare ─> exact | approved | regression
```

Must match exactly: token kind/lexeme/span/line/column; AST JSON shape and
spans; diagnostic code, severity and span (message text may be compared
separately); patch output strings; canonical edit order and error indexes;
WD1.4 verdicts; WD1.5 bindings and statuses; asset classification; package
manifests and ZIP bytes.

Normalization removes only object addresses and iteration order of unordered
maps. It never removes text, whitespace or line endings.

Hard gates, never an aggregate pass rate: **0** confidently wrong selections,
**0** wrong semantic bindings, **0** silent source changes, **0** unauthorized
writes, **0** unapproved fidelity loss. Independent oracles (`spikes/wd1-*`)
stay structurally unable to import either implementation. Corpus work keeps
decoded-text deduplication and input fingerprints; no private file enters the
repository.

### 12.2 Benchmarks

Existing facilities **[FACT]**: `qa/phase-7b-native-editor/perf.js` (parse +
analyze, gate median < 250 ms), `qa/shell-0-baseline/measure.js` (startup,
page switch, memory), `qa/phase-accessibility-perf/perf.js`,
`test/vrml/fixtures.test.js:92`. Current numbers: 0.2 ms (608 B), 1.4 ms
(6.9 kB), 49.3 ms (327 kB), 216.2 ms (1.6 MB) median; WD2-A pipeline 4.9 ms on a
representative World; renderer heap 45.2 MB stable.

**No Rust performance number is reported.** No comparable implementation of the
hot path (parse/analyze) exists, and the edit algebra is not a bottleneck.
Plan for RUST-3/4: same inputs (the six perf profiles + corpus sample), same
host, Node JIT warmed (≥ 20 warm-ups) vs Rust `--release` **through the wasm
boundary including string copy and AST projection**, ≥ 100 iterations, median,
p95, max, peak RSS / wasm memory, with host, CPU, rustc, Node and Electron
versions recorded. A Rust-internal number without boundary cost is not a fair
comparison and must not be reported alone.

## 13. Spike results

See §6.3 and `spikes/rust-core-feasibility/README.md`. Commands run:
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`
(17 passed), `cargo build --release`, `node differential.js` (0 regressions),
three mutation runs (all detected), re-run (identical digests).

## 14. Migration risks and mitigations

| risk | mitigation |
|---|---|
| renderer cannot load native code | WebAssembly bridge; RUST-1 load proof in Electron |
| AST object identity consumed by WD2 | Rust emits a fresh JS projection per parse (identity across reparses is already forbidden); WeakMap brands stay in the JS facade |
| offset reinterpretation | single conversion module; UTF-16 public unit; round-trip assertions; differential |
| baseline is itself lossy (T1–T4) | owner decisions D1–D3 before RUST-2 |
| 22 kLOC semantic core drift | migrate in dependency order with corpus-grade differential; JS stays the fallback until each gate passes |
| two implementations diverge during transition | one feature flag per module; JS remains default; no dual-writes |
| deflate bytes change Mall verdicts | keep Node zlib; do not migrate |
| wasm memory limits on large worlds | measure in RUST-3; streaming/limits from `DEFAULT_LIMITS` |
| Windows release breaks | wasm is platform-neutral; any native artifact needs D6 |
| toolchain/CI growth | one Rust target, std-first, dependency approval per crate |

## 15. Recommended first production lane

**RUST-1 — Boundary and Compatibility Foundation.** Details and gates in
`RUST_0_MIGRATION_ROADMAP.md`. In short: create the Rust workspace outside the
production build, adopt the spike as `wrlforge-text`, prove a wasm32 build loads
in Node **and** in an Electron renderer under the existing CSP, and turn the
differential harness into a reusable tool. Still no production caller.

## 16. Open decisions requiring owner approval

| id | decision |
|---|---|
| D1 | Policy for non-UTF-8 files (T1): read-only unsupported vs byte-preserving design |
| D2 | BOM classification (T2) |
| D3 | CodeMirror line-ending preservation (T3) — a JS fix, independent of Rust |
| D4 | Accept WebAssembly as the Rust bridge (and wasm-bindgen as the first Rust dependency) |
| D5 | Accept "Rust core is pure; I/O stays in main JS" as a standing rule |
| D6 | Windows: keep building it for any Rust artifact, or formally retire it (platform contract conflict P1) |
| D7 | Electron 41 EOL upgrade lane (P2), independent of Rust |
| D8 | Whether RUST-7 (WebKitGTK/X_ITE shell spike) is wanted at all, given APP-ARCH-0 O1 |
| D9 | Whether a migration with no measured performance gain is still wanted for correctness/maintainability reasons |
