# RUST-1 PR #130 review fix

Six unresolved Copilot findings on PR #130 (`architecture/rust-1-wasm-boundary`).
This lane fixes them on a separate stacked branch.

| | |
|---|---|
| branch | `review/rust1-pr130-findings` |
| base | `review/copilot-stack-hardening-1` at `a7301b8d4a79cf6c101d15f3afbce31ee6bb7d6e` (PR #144) |
| scope | RUST-1 spike harness, the isolated Electron proof and the wasm facade only |
| not changed | Rust crates, `renderer/editor.html`, the production CSP, X_ITE 16.5.0, the Tauri app, the registry, version `1.4.5 (Beta)` |

PR #130 and PRs #131–#144 are unchanged.

## 1. Findings, causes and corrections

Each defect was reproduced on the starting SHA before any change.
The script is `/tmp/rust1-pr130/repro.mjs` and its output is in §3.

### F1 — `TextSession.current()` answers on a poisoned instance

- **Thread:** `crates/wrlforge-wasm/js/wrlforge-text.mjs:255` (r4225786596).
- **Cause:** the poison check lived only inside `raw()`.
  `current()` never calls `raw()`, so it minted a fresh snapshot after a trap.
  Argument checks also ran before the poison check, so a poisoned engine
  answered `EEDITSHAPE` or `EARGUMENT` instead of `EENGINE`.
- **Correction:** a shared `alive()` check runs first in every session,
  snapshot and engine call, facade-only ones included.
  Three labels stay readable: `engine.poisoned`, `session.disposed`,
  `snapshot.revision`. A refused `dispose()` no longer marks the session
  disposed. Poison stays scoped to the wasm **instance**.
- **Commit:** `2e739b8`.

| call on a poisoned instance | before | after |
|---|---|---|
| `session.current()` | `NO-ERROR` | `EENGINE` |
| `session.propose(snap, 7)` | `EEDITSHAPE` | `EENGINE` |
| `snapshot.toUtf8(-1)` | `EARGUMENT` | `EENGINE` |
| `engine.applyEdits('a', 7)` | `EEDITSHAPE` | `EENGINE` |
| `engine.mapOffset(-1, [])` | `EEDITSHAPE` | `EENGINE` |
| `engine.checkText(7)` | `EARGUMENT` | `EENGINE` |
| `session.dispose()` | `EENGINE`, `disposed` = true | `EENGINE`, `disposed` = false |
| `update`, `text` | `EENGINE` | `EENGINE` |

### F2 — the production proof did not require a refused eval

- **Thread:** `spikes/rust-1-wasm-boundary/electron/run.cjs:78` (r4225786640).
- **Cause:** `prodOk` did not read `evalBlocked`.
- **Correction:** the production verdict requires `evalBlocked === 'EvalError'`.
  The CSP byte comparison with `renderer/editor.html` is unchanged.
- **Commit:** `be18cfe`.

### F3 — the negative control was vacuously true

- **Thread:** `spikes/rust-1-wasm-boundary/electron/run.cjs:79` (r4225786662).
- **Cause:** `Object.values(neg.loaders).every((l) => !l.ok)` is true for `{}`.
  It also ignored `neg.fatal`.
- **Correction:** the negative control passes only if the result is non-fatal,
  reports exactly `fetchThenInitSync`, `glueDefaultAsync` and
  `embeddedBytesAsync`, and each one has `ok === false`.
  The decision moves to a pure `electron/verdict.cjs`. The runner launch, lock,
  launch cap and leak check are unchanged.
- **Commit:** `be18cfe`.

### F4 — the precision review ignored oracle-refused Rust successes

- **Thread:** `spikes/rust-1-wasm-boundary/harness/precision-review.mjs:159` (r4225786698).
- **Cause:** the exit condition read `genuineRegressions` and `inexact` only.
- **Correction:** the gate also requires `rustSuccess.oracleRefused === 0`.
  The oracle moves unchanged to `harness/bigint-oracle.mjs` (function bodies
  diffed identical). `RUST1A_PKG` selects another artifact.
- **Commit:** `27aeb48`.

### F5 — `unsafeIntegerInvolved` trusted the JavaScript answer

- **Thread:** `spikes/rust-1-wasm-boundary/harness/stages/edit-algebra.mjs:359` (r4225786728).
- **Cause:** the check accepted a case when the JavaScript answer held an unsafe
  value. The answer under test justified its own classification.
  A rounded, safe-looking JavaScript answer could also hide a real precision case.
- **Correction:** the check reads unsafe **input** offsets from the case.
  It proves an unsafe **result** with the BigInt oracle, on well-formed
  mappings only. It reads neither answer.
- **Commit:** `7e0cfc2`.

### F6 — stale benchmark memory note

- **Thread:** `spikes/rust-1-wasm-boundary/perf/node-bench.mjs:80` (r4225786749).
- **Cause:** a hard-coded note ("four inputs, ~3.6 MB") outlived the fifth
  (U+FFFD) input.
- **Correction:** `describeInputs(inputs)` computes the count, UTF-16 units and
  UTF-8 bytes. A `unitsNote` states that the memory figures are MiB.
  The workload and the measurements are unchanged.
- **Commit:** `aeb2931`.

## 2. New tests

All are under `spikes/rust-1-wasm-boundary/test/`.

| file | tests | proves |
|---|---|---|
| `session-lifetime.test.mjs` test 8b | 1 | two engines over one poisoned instance: 16 session/snapshot operations and 7 engine operations each refuse `EENGINE`; labels readable; a separate instance works |
| `electron-verdict.test.mjs` | 11 | successful eval fails; empty, missing, unblocked, extra loader fail; fatal fails; missing page fails |
| `precision-review-gate.test.mjs` | 7 | oracle-refused success exits 1 (scripted ambiguity and overlap; the real `no-ambiguity` build); release exits 0 |
| `precision-check.test.mjs` | 5 | safe-in/unsafe-out, unsafe inputs, safe cases with a lying baseline, malformed cases, and a genuine regression through `runStage` + the real registry |
| `bench-metadata.test.mjs` | 3 | the note equals the real input count and size |

Against the starting code: test 8b fails (`current` → `NO-ERROR`).
`precision-check.test.mjs` fails 4 of 5. The other three files depend on the
new modules, so the reproduction script shows those defects on the base.

## 3. Evidence

Reproduction at `a7301b8`:

```
F1-poisoned-current REPRODUCED :: poisoned=true current()->NO-ERROR; fresh instance applyEdits=axb
F2-prod-eval-not-gated REPRODUCED :: prodOk(evalBlocked=false)=true
F3-neg-control-vacuous REPRODUCED :: negBlocked(empty)=true (missing loader)=true (fatal)=true
F4-oracle-refused-not-gated REPRODUCED :: exit line: process.exit(summary.genuineRegressions === 0 && rustSuccess.inexact.length === 0 ? 0 : 1);
F5-js-justifies-itself REPRODUCED :: check(safe inputs, truth 6, js=OKN 2^53)=true
F6-bench-note-stale REPRODUCED :: inputs=5 units=5237166 utf8Bytes=5237186; note="one session held per input (all four inputs at once, ~3.6 MB of text)"
```

F4 on a real Rust artifact: the base review run on the `no-ambiguity` build
exits **0** with `oracleRefused: 1049`. After the fix it exits **1** with the
same counts.

RUST-1 suite after the fixes:

| command | result |
|---|---|
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 |
| `cargo test --workspace --locked` | exit 0; 15 + 25 + 150 + 3 pass |
| `cargo fmt --all --check` | exit 1, **pre-existing** at `a7301b8`, `crates/wrlforge-document/src/lib.rs` only; no Rust file changed in this lane |
| `build.mjs --negative-controls` | exit 0 |
| `node --test test/*.test.mjs` | exit 0; 72 tests, 71 pass, 1 skip (the opt-in heavy test); base had 45 |
| `RUST1A_HEAVY=1 … session-lifetime.test.mjs` | exit 0; 12 pass, 0 skip |
| `RUST1A_PKG=…/lossy-utf16/pkg … utf16-gate-adversarial` | exit 1 as required (15 fail) |
| `harness/run.mjs --negative-controls` | exit 0; 177,582 cases; 169,938 exact, 7,419 approved, 225 pending, 0 regressions; RUST-0 digests match; all 4 negative controls detected. Identical to the base output except timing |
| `harness/precision-review.mjs` | exit 0; `oracleRefused` 0, `inexact` 0, `genuineRegressions` 0; output byte-identical to the base |
| same, `RUST1A_PKG=…/no-ambiguity/pkg` | exit 1 |
| `perf/node-bench.mjs` | exit 0; note: 5 inputs, 5,237,166 units = 5,237,186 bytes, ~5.2 MB |
| `electron/run.cjs` (VisualQaRunner, `xvfb-run -a`, one job) | exit 0; `prodOk` and `negBlocked` true; eval `EvalError`; 3/3 loaders load under the production CSP and 3/3 are blocked without `wasm-unsafe-eval`; 13/13 checks; no survivors |

The real Electron results were also replayed through `verdict.cjs` with one
mutation each. A successful eval, empty loaders, a missing loader, a fatal
error and an unblocked loader each fail the proof.

The full Linux gate (`apps/desktop-tauri/gate-linux.sh`) runs on the final
commit in a new clean worktree. Its result is recorded in the Draft PR body.
This document cannot hold the SHA of the commit that contains it.

## 4. Remaining risks

- The registry `inputCondition` text for `RUST1-PRECISION` still says "its exact
  JavaScript result". The check now uses the BigInt oracle. Changing the text
  changes `registrySha256` and needs owner review.
- The stage check and the precision review now share one oracle. A defect in
  the oracle would affect both, and the review's `G-genuine-regression` class is
  now reached through the harness `regression` tally instead.
- The `cargo fmt` failure in `wrlforge-document` is pre-existing and out of scope.
- Windows and macOS are untested in this lane.

## 5. Suggested thread replies (not posted)

**F1 (wrlforge-text.mjs:255).** Fixed in `2e739b8`. Every session, snapshot and engine call now runs the instance poison check first, `current()` included. Only `engine.poisoned`, `session.disposed` and `snapshot.revision` stay readable. Test 8b in `session-lifetime.test.mjs` drives two engines over one poisoned instance through every operation and proves a separate instance still works.

**F2 (run.cjs:78).** Fixed in `be18cfe`. The production verdict in `electron/verdict.cjs` now requires `evalBlocked === 'EvalError'`, and `electron-verdict.test.mjs` proves that a successful eval fails the proof. The CSP comparison with `renderer/editor.html` is unchanged.

**F3 (run.cjs:79).** Fixed in `be18cfe`. The negative control now requires a non-fatal result that reports `fetchThenInitSync`, `glueDefaultAsync` and `embeddedBytesAsync`, each blocked. Empty, missing, unblocked and extra loaders and a fatal error each fail in `electron-verdict.test.mjs`.

**F4 (precision-review.mjs:159).** Fixed in `27aeb48`. The exit gate now also requires zero oracle-refused Rust successes. On the `no-ambiguity` negative-control build the base command exited 0 with 1,049 such successes; it now exits 1 (`precision-review-gate.test.mjs`).

**F5 (edit-algebra.mjs:359).** Fixed in `7e0cfc2`. `unsafeIntegerInvolved` now reads unsafe input offsets from the case and proves an unsafe result with the BigInt oracle; it never reads either answer. The real harness classification is unchanged (225 pending, 0 regressions), and `precision-check.test.mjs` covers a lying baseline.

**F6 (node-bench.mjs:80).** Fixed in `aeb2931`. The note now comes from `describeInputs(inputs)`: 5 inputs, 5,237,166 UTF-16 units = 5,237,186 UTF-8 bytes, ~5.2 MB. The memory figures are unchanged and are labeled as MiB.
