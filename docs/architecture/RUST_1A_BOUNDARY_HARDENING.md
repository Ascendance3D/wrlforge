# RUST-1A — WebAssembly Boundary Hardening and Precision Review

Lane: RUST-1A, a focused follow-up to RUST-1 (Draft PR #130).
Date: 2026-10-08. Platform tested: Linux only.
Writing style: ASD-STE100 Simplified Technical English.

## 1. Executive findings

**Verdict recommendation: `RUST_1A_PASS_WITH_CONDITIONS`.**

1. **Defect A (confirmed, fixed). The UTF-16 gate trusted a replaceable
   method.** The Rust gate called `String.prototype.isWellFormed` through a
   glue shim that looks the method up at call time. A caller replaced the
   method with `() => true`. Then `"a\uD83Db"` became `"a�b"` with no
   error. The defect affected the facade and every raw string export:
   144 silent substitutions of 324 calls per lying mode.
2. **Fix A.** The gate now checks the converted text against the source code
   units. It does not trust the method. Result: 0 substitutions in all
   11 method modes. Cost: +0.03 ms to +0.2 ms median at 1.6 MB, and
   +1.1 ms to +1.3 ms for a 1.6 MB text that contains genuine U+FFFD.
3. **Defect B (confirmed, fixed). Poison was per engine, not per wasm
   instance.** A real wasm trap (allocation failure, `unreachable`) poisoned
   only the engine that saw it. A second engine on the same trapped instance
   continued to answer. Fix B keys poison to the instance.
4. **Precision (225 cases).** An independent BigInt oracle classifies all
   225 `RUST1-PRECISION` cases as safe refusals. 0 genuine regressions.
   0 error-taxonomy changes. 122 of 225 cases are inputs where JavaScript
   returns a **wrong** number. No valid real document can reach these values.
   The registry status stays `proposed-pending-owner`.
5. **Sessions.** No stale, foreign, forged or disposed facade object reaches
   a document. Memory does not grow on repeated open/dispose.
6. **Two documented limits (not fixed; owner decisions).** (a) The raw layer
   is not a session-isolation boundary: a forged `__wbg_ptr` reads another
   session's text. (b) The revision record grows by about 21.6 B per revision
   until `dispose`.
7. **Differential.** 177,582 cases. Candidate output digest is identical to
   RUST-1. 0 unexplained regressions. All 4 mutants detected.
8. **macOS 15+ is UNTESTED.** No authorized macOS host was available.
9. **`encoding_rs@=0.8.42` was evaluated and NOT added.** It is faster in
   isolation, but no RUST-1A operation needs it after Fix A (§3.6).

## 2. Verified candidate

| item | value |
|---|---|
| Authorized candidate SHA | `e92748f38d00fb4215531357b129783858996a04` |
| PR #130 head (checked with `gh pr view`) | `e92748f3…6a04`, Draft, OPEN, base `main` |
| Branch | `fix/rust-1a-boundary-hardening` (local only, not pushed) |
| Worktree | `~/Projects/cybertown/.worktrees/wrlforge/rust-1a` |
| Base reproduced before changes | release wasm `a1e99690…0963` (RUST-1 value), 18/18 Node, 27 Rust tests |
| Other worktrees | not modified |

## 3. UTF-16 gate adversarial results (Task A)

### 3.1 Root cause

wasm-bindgen generates this import shim:

```js
__wbg_isWellFormed_…: function () { return handleError(function (arg0) {
    const ret = getObject(arg0).isWellFormed(); …
```

The method is looked up on `String.prototype` at **every** call. A JS return
value goes to wasm as an `i32`, so `1` and `2` act as `true`; `'yes'`, `{}`
and `undefined` act as `false`.

### 3.2 Test design

`spikes/rust-1-wasm-boundary/test/utf16-gate-adversarial.test.mjs`.

- 18 entry points: 9 facade paths and 9 raw paths. They cover every
  parameter that receives a JS string (`text`, `insert`, `after`).
- 8 malformed inputs, 10 valid inputs (astral, pairs, BOM, CR, CRLF,
  genuine U+FFFD, empty).
- 11 method modes. The expected truth comes from an independent code-unit
  scan. It never calls the method under attack.
- Safety property: malformed input is refused with a code; valid input is
  returned with identical code units or refused with a code.

### 3.3 Results, before and after

| mode | RUST-1 artifact | RUST-1A artifact |
|---|---|---|
| 1 native | 0 violations | 0 |
| 2 missing before creation | engine refuses (`EENGINE`) | same |
| 3 deleted after creation | fail closed (`EENGINE`) | same |
| 4 always `true` | **144 / 324 silent substitutions** | 0 |
| 5 always `false` | fail closed, but valid text reported as `EENCODING` with a false `unit: 0` | 0; valid text refused as `EENGINE` (method lied) |
| 6 throws | fail closed | same |
| 7 returns `1` / `2` | **144 / 324 each** | 0 |
| 7 returns `'yes'` / `{}` / `undefined` | fail closed | same |
| 8 replaced after valid text accepted | **session accepted `"DEF � Shape {}"`** | refused; revision unchanged |
| 9 genuine U+FFFD + unpaired surrogate | substituted under modes 4/7 | `EENCODING`, `unit` = the surrogate |
| 10 astral and surrogate pairs | exact | exact |
| `inverted` (extra) | **80 / 324** | 0 |

Node: 16 tests. Before: 8 fail. After: 16 pass. Electron renderer: one new
check (three lying modes, facade and raw); passes.

Mutation proof: with `RUST1A_PKG` set to the `lossy-utf16` mutant, 15 of 16
tests fail (144 violations per mode).

### 3.4 Fix A — exact change

`crates/wrlforge-wasm/src/lib.rs` `take_text`, and a pure helper
`first_unconfirmed_replacement` in `crates/wrlforge-text/src/offsets.rs`
(with a native unit test).

1. Call `isWellFormed`. Missing or throwing → `EENGINE` (unchanged). Keep the
   answer as a claim only.
2. Convert the string (the existing wasm-bindgen path).
3. Check independently. The WHATWG UTF-8 encoder replaces one unpaired
   surrogate (one unit) with one U+FFFD (one unit) and copies every other
   scalar. So UTF-16 positions are preserved. Each U+FFFD in the converted
   text must sit over a source unit `0xFFFD` (`charCodeAt` at that index).
4. U+FFFD over a surrogate → `EENCODING` with `field`, `unit`, `index`.
   U+FFFD over any other unit → `EENGINE`.
5. A `false` claim for text that step 3 proves well formed → `EENGINE`.

No path replaces a code unit. The facade start-up check for `isWellFormed`
is unchanged.

### 3.5 Threat model

- **In scope and closed:** any replacement or deletion of
  `String.prototype.isWellFormed`, at any time.
- **Trust root (documented):** the intrinsics that the wasm-bindgen glue itself
  uses to move a string: `TextEncoder.prototype.encodeInto` and
  `String.prototype.charCodeAt`. The glue already writes ASCII bytes through
  `charCodeAt`. Code that replaces these controls every byte the glue writes;
  no check inside the same realm can detect it. Measured at publication: a
  lying `charCodeAt` alone turns well-formed `"hello"` into `"AAAAA"` with no
  error, in RUST-1 and RUST-1A alike. A lying `charCodeAt` alone does not
  pass a substitution (the native `false` claim is refused as `EENGINE`). A
  substitution under Fix A needs **both** a lying `isWellFormed` and a lying
  `charCodeAt` (reproduced: `"a\uD83Db"` → `"a�b"`). Both are inside this
  trust root.
- **Not a security boundary:** same-realm code that holds the glue module.
  See §5.3.

### 3.6 Runtime cost and the rejected interim design

An interim version re-read every code unit when the text held a U+FFFD. It
was correct but slow (Node, 1.6 MB with U+FFFD: `checkText` 27.1 ms vs
2.5 ms). The production read path decodes files as UTF-8, so a Latin-1 file
contains genuine U+FFFD. That cost was not acceptable. The final design reads
only the k units under the k U+FFFD characters.

Node 24, same process, same facade, RUST-1 artifact vs RUST-1A artifact,
median / p95 ms:

| input | operation | RUST-1 | RUST-1A |
|---|---|---|---|
| ~327 KB | `checkText` | 0.51 / 0.60 | 0.54 / 0.84 |
| ~1.6 MB | `checkText` | 3.45 / 3.65 | 3.52 / 4.37 |
| ~1.6 MB | stateless `applyEdits` | 5.89 / 6.21 | 5.93 / 6.31 |
| ~1.6 MB | `openSession` + `dispose` | 4.97 / 5.21 | 5.12 / 5.32 |
| ~1.6 MB two-byte | `checkText` | 2.50 / 2.61 | 2.68 / 2.90 |
| ~1.6 MB with U+FFFD | `checkText` | 2.50 / 2.73 | 3.81 / 4.08 |
| ~1.6 MB with U+FFFD | stateless `applyEdits` | 4.30 / 6.21 | 5.38 / 6.96 |
| ~1.6 MB with U+FFFD | `openSession` + `dispose` | 4.11 / 4.61 | 5.40 / 5.68 |

### 3.7 `encoding_rs@=0.8.42` evaluation (owner-approved, not added)

Scratch crate in `/tmp` (not committed). wasm32 release, Node 24,
1.6 MB, median ms:

| operation | ASCII | BMP mixed | astral |
|---|---|---|---|
| std `first_unpaired_surrogate` | 0.646 | 0.653 | 0.625 |
| `encoding_rs::mem::utf16_valid_up_to` | 0.301 | 0.242 | 0.617 |
| std strict decode | 3.284 | 2.783 | 2.710 |
| `encoding_rs` validate + `convert_utf16_to_utf8_partial` | 1.185 | 2.610 | 2.977 |

`encoding_rs` is 2–3× faster for ASCII-heavy UTF-16 validation in isolation.
But after Fix A, no RUST-1A path validates a `[u16]` buffer. The gate cost is
the `TextEncoder` copy, not a Rust scan. `encoding_rs@0.8.42` also brings six
transitive crates (`cfg-if`, `core_detect`, `multiversion_no_op`,
`scopeguard`, `simdutf8`, `rustversion`). The owner condition is "a
demonstrated benefit". There is none here, so the standard library stays.
Recommendation: re-evaluate in TEXT-1 for byte-level encoding analysis with
`decode_without_bom_handling_and_without_replacement`.

## 4. Precision adjudication (Task B)

Script: `spikes/rust-1-wasm-boundary/harness/precision-review.mjs`. It selects
cases with the framework's own rule. It grades each case with an exact BigInt
implementation of the WD1.2 mapping contract. The oracle imports neither
`edit.js` nor the engine, and does not read either answer.

### 4.1 Decision table

| class | count | kinds | JS result | exact truth | Rust | decision |
|---|---|---|---|---|---|---|
| P1 unsafe input, JS exact | 89 | PM 66, R 23 | correct | safe or unsafe | `EOFFSETPRECISION` | stricter refusal; safe |
| P2 unsafe input, JS wrong | 104 | PM 40, R 64 | **wrong** (rounded) | e.g. `1152921504606846978` | `EOFFSETPRECISION` | Rust prevents silent error |
| P3 safe input, unsafe result, JS exact | 14 | PM 4, R 10 | `2^53` (exact by chance) | `2^53` | `EOFFSETPRECISION` | stricter refusal; `2^53` is ambiguous in JS |
| P4 safe input, unsafe result, JS wrong | 18 | PM 7, R 11 | **wrong** (`…992` for `…993`) | above `2^53` | `EOFFSETPRECISION` | Rust prevents silent error |
| G genuine regression | **0** | | | | | |
| X error-code change | **0** | | | | | |

### 4.2 Answers

1. **Unsafe integer or unsafe result?** Yes, every case. The smallest
   largest-input of any case is `9007199254740990` (`MAX_SAFE_INTEGER − 1`).
2. **Can JS round before Rust sees the value?** Yes. A number above `2^53` is
   already rounded when the caller creates it. Rust cannot know the intended
   value. That is why it refuses every unsafe input.
3. **Does Rust refuse before an inaccurate result?** Yes. The facade
   (`checkPrecision`) and the raw layer (`safe_offset`, `safe_result`) both
   refuse. All 54,437 Rust mapping **successes** in the run equal the BigInt
   oracle (0 inexact). So a shared wrong answer does not hide as "exact".
4. **Can a valid real document reach these ranges?** No. V8 strings are
   below `2^30` code units. No case has all inputs below `2^30`.
5. **Ordinary valid UTF-16 offsets?** None. All 225 are in the synthetic
   `precision:map` and `precision:range` groups.
6. **Error taxonomy stable?** Yes. 0 cases where JS refused with a different
   code. `EOFFSETPRECISION` is the only new code.
7. **Classification and oracle correct?** Yes, with one note. The registered
   check `unsafeIntegerInvolved` reads the JS answer for the result-only
   cases (P3, P4). The framework contract says a check never trusts either
   answer. The BigInt review confirms the same 225 cases. Recommendation: in
   a later harness lane, replace that check with the BigInt oracle. RUST-1A
   did not change the registered check or the registry.
8. **Any genuine regression?** No.

**Recommendation:** approve `RUST1-PRECISION` as `approved-experimental`
(harness only). This is a recommendation. The registry still says
`proposed-pending-owner`; registry SHA-256 is unchanged
(`b7af1f67…c681`).

## 5. Session and memory analysis (Task C)

Test file: `spikes/rust-1-wasm-boundary/test/session-lifetime.test.mjs`
(10 tests + 1 opt-in heavy test).

### 5.1 Results

| # | test | result |
|---|---|---|
| 1 | 7,000 open/dispose cycles of a 21,000-unit document; 5,000 refused creations | linear memory flat on repeat (1,179,648 B high-water) |
| 2 | 200,000 revisions on one session | correct; +4,325,376 B (21.6 B/revision); reused after `dispose` |
| 3 | 50 stale snapshots × 11 operations | all `ESESSIONSTALE` |
| 4 | snapshots across 3 sessions | all `ESESSIONFOREIGN` |
| 5 | engines on separate instances | facade objects foreign both ways; raw serial/revision numbers collide (1/2 in each instance) |
| 6 | forgeries: `{}`, prototype clones, `Proxy`, `structuredClone`, JSON copies | all `ESESSIONFOREIGN`; frozen objects reject changes |
| 7 | every call after `dispose` | `ESESSIONDISPOSED`; idempotent; other sessions intact |
| 8 | trap seen through one engine | **every** engine on that instance refuses (fails before Fix B) |
| 9 | refused creations and coded refusals | no poison, no state change |
| 10 | 20 separately loaded instances | each works; mutually foreign |
| heavy | real allocation-failure trap (`RUST1A_HEAVY=1`, ~11 s) | `EENGINE`, cause `RuntimeError`; the second engine is poisoned |

### 5.2 Fix B — exact change

`crates/wrlforge-wasm/js/wrlforge-text.mjs`: a module-level
`WeakMap` keyed by `glue.RawSession` (shared by every view of one instance)
holds the poison record. Before: a per-closure variable.

Reproduction before the fix: two engines on one instance; a 20 × 256 Mi-unit
insert set exceeds wasm32 memory; engine 1 reports `EENGINE` (`unreachable`);
engine 2 still reports `poisoned: false` and answers.

### 5.3 Documented limits (no change made)

- **Raw-layer forgery.** wasm-bindgen trusts `__wbg_ptr`. A forged raw handle
  with a copied pointer returned `"secret document A"`. Calling instance B's
  method on instance A's handle trapped (`memory access out of bounds`).
  Only code that holds the glue module can do this, and that code can also
  read linear memory. So the facade, not the raw layer, is the isolation
  boundary. Condition for RUST-2: the renderer must not expose the glue module
  to untrusted code.

  The facade enforces session ownership for callers using the public facade
  API. Raw generated WebAssembly exports are trusted internal implementation
  details and do not independently enforce equivalent ownership. Before
  production integration, the application must prevent untrusted code from
  obtaining or invoking raw handles, or must strengthen the raw boundary.
- **Revision record growth.** `SessionCore.issued` keeps one `u64` per
  revision to tell `ESESSIONSTALE` from `ESESSIONREVISION`. Growth is linear
  until `dispose`. This is retained session metadata, not a leak. Options for
  the owner: keep it; store revision ranges (exact; O(1) for one active
  session); or drop the stale/invalid distinction at the raw layer (the facade
  already decides staleness). Any choice changes a Rust unit test, so RUST-1A
  did not decide it. This is retained metadata per issued revision, not the
  reusable wasm memory high-water mark. Before production integration, a
  bounded-history or equivalent memory strategy is required.
- **Instances cannot be unloaded.** ES module instances stay in the loader
  cache. A renderer must create one instance and reuse it.
- **Memory does not shrink.** Measured values are high-water marks. Test 1 and
  test 2 show reuse after free.

## 6. Reproduced defects

| id | defect | reproduction | status |
|---|---|---|---|
| A | UTF-16 gate trusts replaceable `isWellFormed` | 144/324 substitutions per lying mode; case 8 session corruption | fixed |
| A2 | `false` claim on valid text reported as `EENCODING unit 0` | adversarial test case 5 | fixed (now `EENGINE`) |
| B | poison scoped to engine, not instance | real OOM trap; simulated trap test 8 | fixed |

## 7. Exact fixes made

| file | change |
|---|---|
| `crates/wrlforge-wasm/src/lib.rs` | `take_text` rewritten (§3.4); `first_substitution`; header comment states the trust boundary |
| `crates/wrlforge-text/src/offsets.rs` | new pure `first_unconfirmed_replacement` + unit test |
| `crates/wrlforge-wasm/js/wrlforge-text.mjs` | instance-scoped poison (§5.2); header comment |

`#![forbid(unsafe_code)]` is unchanged. No dependency was added. No I/O was
added to the Rust core.

## 8. Rust and JavaScript test results

| gate | result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo test --workspace --locked` | 28 pass, 0 fail (25 `wrlforge-text` incl. 1 new, 3 `wrlforge-wasm`) |
| Node proof (`test/*.test.mjs`) | 45 tests: 44 pass, 0 fail, 1 skip (heavy, opt-in) |
| heavy test (`RUST1A_HEAVY=1`) | pass |
| Electron renderer proof | 13/13 (12 RUST-1 + 1 RUST-1A); 3/3 loaders; `eval` blocked; control page blocked as expected |
| `npm run check` | 2,625 tests: 2,621 pass, 0 fail, 4 skip (pre-existing); 327 files parsed |
| precision review | exit 0; 0 genuine regressions |

Original 18 Node tests are unchanged and pass.

## 9. Differential totals

| measure | RUST-1 | RUST-1A |
|---|---|---|
| cases | 177,582 | 177,582 |
| exact | 169,938 | 169,938 |
| approved (`RUST0-BOUNDARY` 3,120 + `RUST0-ENCODING` 4,299) | 7,419 | 7,419 |
| pending (`RUST1-PRECISION`) | 225 | 225 |
| unexplained regressions | 0 | **0** |
| input / baseline / candidate SHA-256 | `17fd4527…` / `86000a3a…` / `eb44188d…7480` | **identical** |
| RUST-0 input and baseline digests | match | match |
| registry SHA-256 | `b7af1f67…c681` | unchanged |
| mutants `utf8-tiebreak` / `no-ambiguity` / `no-boundary` / `lossy-utf16` | 646 / 1,597 / 2,723 / 1,108 | identical; all detected |

The fixes change no differential output. The harness corpus has no lying
`isWellFormed` and no trap, so the outputs are the same.

## 10. WebAssembly artifact identity

Toolchain: rustc 1.95.0, cargo 1.95.0, wasm-bindgen 0.2.129, Node v24.21.0.
Deterministic: two clean rebuilds gave the same hashes.

| artifact | SHA-256 | bytes |
|---|---|---|
| release `wrlforge_wasm_bg.wasm` (RUST-1A) | `56f678b0ac92af6f0c0698831e40280267613ae82ff2c7bb05ec475c8f609455` | 66,609 |
| release cargo `.wasm` | `91ae12f3995a2b679bf0cb5a5c1704d60819a0e9d14d46d430d2714f4e7e1481` | |
| release glue `wrlforge_wasm.js` | `aabd73637dfc27d8d33377511d2839bf170b84f731a4f62c49deacd832b3760c` (unchanged) | |
| RUST-1 release (for reference) | `a1e99690bf787e8542464f39fe1a1571cb05002e6912dd8ee3c4de0171910963` | |

`engine_info()` of the release artifact reports `negative-controls=[]`; the
build script and the differential runner assert it.

## 11. Performance comparison

Node 24 (`perf/node-bench.mjs`), median ms, RUST-1A artifact:

| input | Rust stateless edit | JS `edit.js` | Rust `checkText` | open+dispose |
|---|---|---|---|---|
| ~6 KB | 0.033 | 0.004 | 0.014 | 0.020 |
| ~327 KB | 1.42 | 0.13 | 0.54 | 0.87 |
| ~1.6 MB | 8.34 | 0.70 | 3.44 | 5.10 |
| ~1.6 MB two-byte | 6.59 | 1.33 | 2.65 | 4.25 |
| ~1.6 MB with U+FFFD (new input) | 7.65 | 1.33 | 3.73 | 5.34 |

Electron 41.7.1 renderer (Chrome 146), median / p95 ms, stateless edit:
~1.6 MB **16.6 / 17.3** (RUST-1: 16.4 / 16.8); U+FFFD variant 13.7 / 14.1.

Rust is **not** faster than the existing JavaScript edit path. Full-document
transfer dominates. A 1.6 MB stateless edit on the renderer main thread uses
the whole 16.7 ms frame budget. This is not acceptable for an
every-keystroke main-thread design.

### 11.1 RUST-2 architecture recommendation (no implementation)

| factor | A — resident text + verified deltas | B — Web Worker |
|---|---|---|
| implementation complexity | medium: delta protocol, revision handshake, resync path | medium-high: worker loader, CSP review for worker `wasm-unsafe-eval`, message protocol |
| main-thread latency | low: O(edit size) per keystroke | low for compute; still pays a full copy per message unless deltas are also used |
| source revision safety | strong if each delta is bound to base revision and verified; mismatch → full resync | results arrive late; every result must carry and check its revision |
| initialization cost | one instance, one full load per document | one more instance per worker; compile once, module transfer |
| error recovery | resync from CodeMirror text (authoritative) | restart worker; resync; main thread survives a trap |
| memory | one resident copy per open document | one copy per worker plus message copies |
| CodeMirror / X_ITE | fits CodeMirror `ChangeSet` directly; X_ITE untouched | async results need stale-result discard; X_ITE untouched |

**Recommendation:** run a separate performance/architecture spike before
RUST-2. Prototype **A first**, measured per keystroke on the 1.6 MB input in
the renderer. Add **B** only for whole-document analysis (parse, semantics)
that stays long even with deltas. They combine: deltas into a worker-resident
snapshot. Keep the RUST-1A UTF-16 gate on every delta insert.

## 12. Linux / macOS coverage

| platform | status |
|---|---|
| Linux x86_64 (kernel 7.0.0-34-generic), Node v24.21.0, Electron 41.7.1 | **TESTED** — all gates in §8 |
| macOS 15+ | **UNTESTED.** No authorized macOS host was available. No remote machine or paid service was used. Existing JS-app macOS CI is not evidence for these tests. |
| Windows, mobile | out of scope; not tested; no historical platform file changed |

## 13. Required independent QA scope

1. Review Fix A soundness: the position-preservation argument (§3.4) against
   the WHATWG Encoding spec, and the trust-root statement (§3.5).
2. Re-run `utf16-gate-adversarial.test.mjs` against the release artifact
   (must pass) and the `lossy-utf16` mutant (must fail).
3. Review Fix B and run the heavy real-trap test.
4. Re-run the differential and confirm candidate digest `eb44188d…7480`.
5. Independently audit `precision-review.mjs` and its BigInt oracle.
6. Confirm the release artifact hash `56f678b0…9455` from a clean rebuild.
7. Confirm no production file changed (`src/`, `renderer/`, `main.js`,
   `preload.js`, `package*.json`, CI).

Routing follows the repository rules; the owner selects the QA tool.

## 14. Outstanding owner decisions

1. `RUST1-PRECISION`: approve as `approved-experimental` (recommended) or not.
2. Revision record growth (§5.3): keep, range-encode, or drop the raw
   stale/invalid distinction.
3. Trust boundary (§5.3): accept "facade is the isolation boundary; the glue
   module stays private" as a RUST-2 requirement.
4. Replace the registered `unsafeIntegerInvolved` check with the BigInt
   oracle in a later harness lane.
5. `encoding_rs`: keep approval open for TEXT-1; not used in RUST-1A.
6. Authorize a macOS 15+ host for the RUST-1/1A proof.
7. Authorize the RUST-2 performance/architecture spike (§11.1).

Deferred UI work, kept visible and **not** implemented:

- `UX-LOADING-0` — the WRL Forge logo must appear during X_ITE scene loading.
- `APP-SPLASH-0` — optional application startup splash, near product completion.

## 15. Recommendation for PR #130

Add the RUST-1A changes to PR #130 **only after owner approval**, as a
separate commit, then request independent QA (§13). Keep PR #130 as a Draft
until macOS 15+ evidence exists and the `RUST1-PRECISION` decision is made.
Do not start RUST-2 production work before the performance/architecture spike.

Owner verdict recommendation: **GO WITH CONDITIONS** — conditions: owner
approval to commit, independent QA, macOS 15+ proof, and decisions 1–3.

## Files

New:

- `docs/architecture/RUST_1A_BOUNDARY_HARDENING.md`
- `spikes/rust-1-wasm-boundary/test/utf16-gate-adversarial.test.mjs`
- `spikes/rust-1-wasm-boundary/test/session-lifetime.test.mjs`
- `spikes/rust-1-wasm-boundary/harness/precision-review.mjs`

Modified:

- `crates/wrlforge-wasm/src/lib.rs`
- `crates/wrlforge-text/src/offsets.rs`
- `crates/wrlforge-wasm/js/wrlforge-text.mjs`
- `spikes/rust-1-wasm-boundary/electron/proof.mjs` (one new check)
- `spikes/rust-1-wasm-boundary/perf/inputs.mjs` (one new input)
- `spikes/rust-1-wasm-boundary/README.md`
- `docs/architecture/RUST_1_BOUNDARY_CONTRACT.md` (gate, poison, trust
  boundary, revision record)

Unchanged: all production code, the Electron CSP, `package.json`, the
lockfile, CI, the registry, `RUST_1_WASM_QA.md`, `RUST_1_PERFORMANCE.md`, and
all RUST-0 files.
