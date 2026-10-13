# RUST-0 — Migration Roadmap (proposal)

Companion to `RUST_0_CORE_FEASIBILITY.md`. **Every stage is a proposal.** Each
needs its own owner GO; none advances automatically. Each ends with a
structured STOP report. Independent QA uses only owner-approved routing
(`CLAUDE.md` "Working with Ryan"); no QA tool is chosen here.

## Changes to the provisional order, and why

1. **New TEXT-1 before RUST-2.** The JavaScript baseline has text-fidelity
   gaps (T1–T4 in the feasibility report). A Rust port must not copy them
   silently or fix them silently. They are JS fixes and owner decisions.
2. **Schema moves into RUST-3**, beside the parser that consumes it.
3. **Identity (RUST-5) also takes the WD2 transforms and scene-tree
   projection**, because they share the handle/anchor design.
4. **RUST-6 shrinks** to pure project analysis. File I/O, authorization and
   deflate stay in main-process JS (classes D). SEC-SHELL-0 must land first.
5. **RUST-7 becomes optional.** APP-ARCH-0 O1 already keeps Electron. RUST-7
   runs only if the owner wants new evidence to reopen O1.

```text
RUST-0  architecture + spike                       (this lane)
RUST-1  boundary foundation + wasm proof + harness
TEXT-1  JS text-fidelity decisions (D1–D3)         (JS only; may run beside RUST-1)
RUST-2  wrlforge-text in production (edit algebra)
RUST-3  tokenizer + parser + source map + schema
RUST-4  semantics: scope graph, symbols, IS, ROUTE, findings
RUST-5  identity, transactions, WD2 transforms, scene-tree projection
RUST-6  pure project analysis (needs SEC-SHELL-0)
RUST-7  optional: WebKitGTK/WKWebView X_ITE shell spike
RUST-8  controlled production integration / JS retirement, per module
```

Common rollback for RUST-2..RUST-6: each module ships behind a per-module
switch with **JavaScript as the default** until its gate passes on Linux and
macOS. Rollback = flip the switch; the JS module is not deleted before RUST-8.

---

## RUST-1 — Boundary and Compatibility Foundation

- **Preconditions:** owner GO on D4 (WebAssembly, wasm-bindgen) and D5 (pure
  core). RUST-0 accepted.
- **Scope:**
  - Rust workspace (location to be approved — e.g. `crates/`, a new top-level
    directory needs approval), **not** in `package.json` scripts, not in
    packaging.
  - Adopt the spike as `wrlforge-text` (offsets, edit algebra, `mapRange`,
    `LineIndex` with UTF-16 columns).
  - `wrlforge-wasm` minimal facade over `wrlforge-text`.
  - **wasm load proof:** the same `.wasm` loaded (a) in Node 24 main-process
    context and (b) in an Electron renderer page under the **unchanged** CSP,
    in an isolated QA page or spike, never a production page.
  - Differential harness generalized: case generators, independent oracle
    hooks, approved-difference registry with written reasons.
  - Handle design (§4.4) implemented for one session type with stale /
    disposed / foreign tests.
- **Deliverables:** workspace, two crates, harness, wasm proof report (Linux
  **and** macOS 15 host), CI proposal (not enabled).
- **Acceptance:** 0 differential regressions vs `edit.js`; boundary
  round-trip assertions on every fixture; wasm loads with zero CSP change;
  boundary cost measured (copy + conversion) for 6 kB / 327 kB / 1.6 MB;
  stale/disposed/foreign handles fail closed in tests.
- **Regression risk:** none to product (no callers).
- **Rollback:** delete the workspace.
- **QA:** independent review of the offset contract and the harness.
- **Owner gate:** GO for TEXT-1/RUST-2; CI enablement decision.

## TEXT-1 — JavaScript text-fidelity decisions (JS only)

- **Preconditions:** owner decisions D1 (non-UTF-8 files), D2 (BOM),
  D3 (CodeMirror line endings).
- **Scope:** fatal UTF-8 decode with an explicit unsupported/read-only path;
  BOM as leading trivia; CodeMirror `lineSeparator` from the detected ending
  (or refusal for mixed endings); `edit.js` surrogate-boundary refusal
  (aligns JS with Rust D1).
- **Acceptance:** byte round-trip for every fixture incl. CRLF/gzip twins and
  new Latin-1, BOM, mixed-ending and astral fixtures; no file is ever written
  with a changed byte outside an edit.
- **Rollback:** revert the JS commits.
- **Owner gate:** behaviour visible to users (read-only notice), so owner sign-off.

## RUST-2 — Lossless text and edit algebra

- **Preconditions:** RUST-1 + TEXT-1 accepted.
- **Scope:** `document-transaction`, `field-edit`, `structure-edit` call the
  wasm `applyEdits`/`mapOffset` behind a switch; JS default.
- **Acceptance:** whole `test/vrml` suite passes in both modes; 0 differential
  regressions on enumeration + all fixtures + corpus sample; zero silent source
  changes; no measurable editor-latency regression (perf gate unchanged).
- **Risks:** string copy cost on every edit; wasm init failure path.
- **Rollback:** switch to JS.
- **QA:** independent; Linux + macOS.

## RUST-3 — Tokenizer, parser, source map, schema

- **Preconditions:** RUST-2 accepted.
- **Scope:** Rust tokenizer/parser/source map; schema regenerated as Rust data
  from the same ISO + `x_ite.d.ts` inputs; wasm exports a **JSON-compatible AST
  projection** that existing JS consumers read unchanged.
- **Acceptance:** token-by-token and AST-JSON equality on all fixtures and the
  corpus (decoded-text dedup, fingerprinted inputs); diagnostics equal by code
  and span; recovery shape identical; limits identical; fair benchmark per
  feasibility §12.2 reported **including** boundary and projection cost.
- **Risks:** the projection cost can erase any speed gain; recovery drift.
- **Rollback:** switch to JS.

## RUST-4 — Schema consumers and semantics

- **Preconditions:** RUST-3 accepted.
- **Scope:** scope graph, symbols, IS, ROUTE, containment, interface query,
  PROTO checks, findings, compatibility profile.
- **Acceptance:** every WD1.5 expected-truth oracle passes unchanged; corpus
  sweeps show **0 wrong bindings** and **0 confident answers from an unprovable
  scope** (IS 23,246; ROUTE 245,540 over 4,466 unique documents, or the current
  denominator with its fingerprint); identical status taxonomy.
- **Risks:** subtle ranking or recovery differences — the §7 failure mode.
- **Rollback:** switch to JS.

## RUST-5 — Identity, transactions, WD2 transforms, scene-tree projection

- **Preconditions:** RUST-4 accepted; handle design proven in RUST-1.
- **Scope:** WD1.4 Tier 1/2, `verifyTransaction`, field/structure edits,
  scene-tree projection, with the JS facade keeping `WeakSet`/`WeakMap` brands.
- **Acceptance:** all identity absence scans and adversarial sweeps report
  **0 confidently wrong selections**; WD2-D picking regression suite passes;
  stale/disposed/foreign/ambiguous/recovered cases fail closed.
- **Rollback:** switch to JS.

## RUST-6 — Pure project analysis

- **Preconditions:** RUST-4 accepted; **SEC-SHELL-0 landed**.
- **Scope:** asset graph, path policy (lexical), package plan, ZIP writer,
  Mall rules — host supplies bytes; all fs, realpath, gzip and writes stay JS.
- **Acceptance:** byte-identical ZIP and manifests; identical findings;
  0 unauthorized writes (Rust has no fs by construction).
- **Rollback:** switch to JS.

## RUST-7 — Optional shell spike

- **Preconditions:** owner wants to reopen APP-ARCH-0 O1 (D8).
- **Scope:** X_ITE conformance pages (Mall, 72-texture World, picking) in
  WebKitGTK (Mesa, NVIDIA; X11, Wayland) and WKWebView (macOS 15).
- **Acceptance:** renders identical to Electron within visual QA tolerance,
  hardware acceleration proven, not just "WebGL available".
- **Rollback:** none needed (spike only).

## RUST-8 — Controlled production integration

- **Preconditions:** each module's stage accepted on Linux and macOS.
- **Scope:** flip defaults module by module; retire a JS module only after a
  release cycle with Rust as default and no regression; build `.wasm` in CI on
  all release platforms (Windows per D6).
- **Acceptance:** release QA green; JS removal reviewed separately.
- **Rollback:** previous release; JS modules kept until removal is approved.
