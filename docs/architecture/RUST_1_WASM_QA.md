# RUST-1 — WebAssembly QA

Date: 2026-10-08. Host: Linux x86_64 (AMD Ryzen 9 5900X, kernel 7.0.0-34,
X11 session). Branch `architecture/rust-1-wasm-boundary` at base
`afb4158da39cdb328238c6f6259465947301dfe3`.
No independent external QA was run (owner routing rule; none chosen).

## 1. Platform coverage

| platform | result |
|---|---|
| Linux x86_64 | **tested** — everything below |
| macOS 15+ | **UNTESTED.** No authorized macOS host was available to this lane. No macOS result is claimed from Linux or cross-compilation. |
| Windows / mobile | out of scope; nothing added, nothing removed (see §8) |

The `.wasm` artifact is platform-neutral, but the Electron/Chromium loader,
CSP behaviour and timing must still be proven on macOS (§9).

## 2. Toolchain (changes recorded)

| item | value | change made by this lane |
|---|---|---|
| rustup | 1.29.0 | none |
| default toolchain | stable 1.95.0 | none |
| `wasm32-unknown-unknown` | added to stable 1.95.0 | **added** (authorized) |
| pinned toolchain `1.95.0` | installed by rustup from `crates/rust-toolchain.toml` (minimal profile + rustfmt, clippy, wasm32 target) | **installed** (same compiler build `59807616e`) |
| wasm-bindgen-cli | 0.2.129 | **installed lane-locally** under `crates/target/tools` (gitignored); not on `PATH`, not in `~/.cargo/bin` |
| Node | v24.21.0 | none |
| Electron | 41.7.1 (Chromium 146.0.7680.216) | none; `npm ci` in the new worktree from the existing lockfile |

No system compiler or package manager was changed.

## 3. Artifact identity

Build: `node spikes/rust-1-wasm-boundary/build.mjs` (cargo `--locked`,
`--release`, no features; then `wasm-bindgen --target web`). Rebuilt three
times; identical hashes each time.

| file | SHA-256 | bytes |
|---|---|---|
| `out/pkg/wrlforge_wasm_bg.wasm` (shipped artifact) | `a1e99690bf787e8542464f39fe1a1571cb05002e6912dd8ee3c4de0171910963` | 65,892 |
| `out/pkg/wrlforge_wasm.js` (glue) | `aabd73637dfc27d8d33377511d2839bf170b84f731a4f62c49deacd832b3760c` | |
| cargo output `crates/target/wasm32-unknown-unknown/release/wrlforge_wasm.wasm` | `2bcf05660d4ae8fafa297bec89c0c6d33c970db52d0b55237f16ae0f32ac59e2` | 456,526 |
| `crates/Cargo.lock` | `a2f504c5f5d14ee960da5eb06e1d5c0732323fb77fdccb135adb11cfcf7c6665` | |

`engine_info()` of the release artifact reports `negative-controls=[]`
(asserted by the Node test, the harness and the renderer proof). The glue
contains no `eval` / `new Function`.

## 4. Rust checks

| command (in `crates/`) | result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |
| `cargo clippy -p wrlforge-wasm --target wasm32-unknown-unknown --locked -- -D warnings` | clean |
| `cargo test --workspace --locked` | **27 passed** (24 `wrlforge-text`, 3 session) |
| `cargo test -p wrlforge-text --features negative-control-*` (3 mutants) | each **fails 1** hand-written test, as intended |

## 5. Node load proof (G6)

`node --test spikes/rust-1-wasm-boundary/test/*.test.mjs` — **18/18 pass.**
Covers module initialization; repeated initialization (idempotent; a fresh
instance is independent; cross-engine handles foreign); ASCII, BMP, astral,
surrogate pairs, BOM (leading and inner), CRLF, lone CR, mixed endings, NUL,
empty document; lone high/low, high at end, low at start, reversed pair, lone
surrogate after a real U+FFFD (all `EENCODING`, exact `unit`); a legitimate
U+FFFD round-trips; 9³ combination sweep with zero substitutions; offset
conversion incl. 2^31, 2^32, 2^53−1, 2^64, 2^70 and invalid values; line index;
empty inserts; same-offset insertion conflicts (also two empty inserts);
shape errors with `edit.js` codes and indexes; exact large `mapOffset` results
and precision refusals; sessions (stale, foreign, forged, prototype clone,
disposed, idempotent dispose, failed update leaves state unchanged); full-text
transaction verification; raw-layer refusal of numbers it did not mint;
fail-closed without `String.prototype.isWellFormed`; poisoning after an uncoded
failure.

## 6. Electron renderer load proof (G7)

`node spikes/rust-1-wasm-boundary/electron/run.cjs` — driven by the sanctioned
`qa/visual-qa/runner.js` (one Electron process, lock, launch cap 1, leak check:
exited gracefully, no survivors). Isolated `electron/main.cjs`; production
`main.js`, `preload.js`, HTML and IPC untouched.

| item | value |
|---|---|
| webPreferences (read back) | `contextIsolation: true`, `nodeIntegration: false`, `sandbox: true`, `webSecurity: true`, **no preload** |
| launch flags | none (no `--no-sandbox`, unlike the screenshot QA CLI) |
| page | `file:` page; no IPC, no protocol registration, window never shown |
| renderer globals | `require`, `process`, `module` all `undefined` |
| `String.prototype.isWellFormed` | present |

### CSP comparison

`electron/run.cjs` refuses to run unless `proof.html`'s CSP is **byte-for-byte**
`renderer/editor.html`'s CSP, and the negative page's CSP is exactly that minus
`'wasm-unsafe-eval'`.

| page | CSP | `eval('1')` | fetch + `initSync` | glue default async | embedded bytes async | checks |
|---|---|---|---|---|---|---|
| `proof.html` | production editor CSP, unchanged | blocked (`EvalError`) | **loads** (7.2 ms) | **loads** (4.4 ms) | **loads** (14.5 ms) | **12/12 pass** |
| `proof-no-wasm-eval.html` | same minus `'wasm-unsafe-eval'` | blocked | blocked (`CompileError`, CSP) | blocked | blocked | n/a |

Conclusion: the existing `'wasm-unsafe-eval'` source is **necessary and
sufficient** for this module. `connect-src 'self' file:` already allows the
`file:` fetch. No `unsafe-eval`, no remote origin, no new scheme, no web
security change. The only CSP violation report on the production page is the
deliberate `eval` probe.

Renderer checks (all PASS): release artifact; same engine identity from all
three loaders; repeated `initSync` idempotent; valid classes round-trip; lone
high/low/reversed refused; 11³ zero-substitution sweep; offset conversion and
boundary refusal; CRLF/CR/comment preservation; error code + caller indexes;
`EEDITBOUNDARY`; session create/update/stale/foreign/disposed; full-text
verify.

## 7. Differential (G4)

`node spikes/rust-1-wasm-boundary/harness/run.mjs --negative-controls` (after
`build.mjs --negative-controls`). Deterministic; digests identical on re-run.

| measure | value |
|---|---|
| total cases | **177,582** (RUST-0 subset 167,316 + RUST-1 additions 10,266) |
| RUST-0 equivalence | input SHA-256 `358ea723…066dd` and baseline SHA-256 `c8d70e04…85694` **identical to RUST-0** |
| exact | 169,938 |
| approved (`RUST0-BOUNDARY` 3,120; `RUST0-ENCODING` 4,299) | 7,419 |
| pending owner (`RUST1-PRECISION`) | 225 |
| **unexplained regressions** | **0** |
| boundary refusals whose JS output was ill-formed | 2,723 (unchanged from RUST-0) |
| fixtures used | 65 committed fixtures (production tokenizer spans) |
| registry SHA-256 | `b7af1f671429c07a6c2952dc3ac54292fa039a376396a02dc9d639db6e7cc681` |
| release digests | input `17fd4527…1d872`, baseline `86000a3a…c16`, candidate `eb44188d…7480` |

RUST-0 subset vs RUST-0's own run: 502 cases that RUST-0 classified as
encoding refusals are now **exact**, because the facade runs `edit.js` shape
checks before the UTF-16 gate (RUST-0's wire protocol refused first).

RUST-1 additions: argument shapes (S), `mapRange` enumeration incl. both range
shapes and all affinities (R), very large/overflow offsets (P), UTF-16 echo
11³ (U), line index against an independent oracle and against the production
tokenizer's own line/column for every token of the small texts and sampled
tokens of all 65 fixtures (L/LO/LT).

### Negative controls (mutant artifacts, never shipped)

| mutant | regressions | first example |
|---|---|---|
| `utf8-tiebreak` (Rust byte order) | 646 | ambiguity reported with swapped indexes |
| `no-ambiguity` | 1,597 | `OK` where JS refuses `EEDITAMBIGUOUS` |
| `no-boundary` | 2,723 | interior offset rounded silently |
| `lossy-utf16` (gate removed) | 1,108 | `"\uDC00"` became `"�"` — **silent corruption** |

### Registry changes

`RUST0-BOUNDARY` and `RUST0-ENCODING` are RUST-0's entries (RUST-0 labelled
them "D1"/"D2"; renamed to avoid confusion with owner decisions D1–D9;
`rust0Label` keeps the old label). **One new entry, `RUST1-PRECISION`,
status `proposed-pending-owner`**, counted separately and never as approved.
No entry approves a production change.

## 8. Production isolation (G10) and regression (G11)

`npm run check` in the RUST-1 worktree: **2,625 tests, 2,621 pass, 0 fail, 4
skipped** (pre-existing skips); 327 files parsed. No production file is
modified (`git status` shows only new paths: `crates/`,
`spikes/rust-1-wasm-boundary/`, `docs/architecture/RUST_1_*`, and the copied
RUST-0 files). `package.json`, lockfile, `main.js`, `preload.js`, `renderer/`,
`src/`, CI, packaging and release workflows are unchanged.

Platform-policy conflict (unchanged, for owner action): the RUST-1 platform
contract is Linux + macOS 15+, while the repository still builds and releases
Windows (CI, `release.yml`, `CLAUDE.md`). RUST-1 removes nothing. The wasm
artifact itself is platform-neutral.

## 9. Skipped / failed / outstanding

| item | status |
|---|---|
| macOS 15+ Node load, renderer load, CSP, perf | **not run** — no authorized host |
| Windows | out of scope; not run |
| Independent QA review | not run (owner routing; none selected) |
| Failures | none outstanding. During development one test assertion was wrong (`instanceof Object` on a null-prototype exports object) and was corrected; one harness load failed until the generated glue got an ESM `package.json` marker. |
