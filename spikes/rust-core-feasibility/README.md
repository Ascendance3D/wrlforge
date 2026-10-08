# RUST-0 spike — UTF-16 offset boundary + WD1.2 edit algebra

Isolated feasibility spike for `docs/architecture/RUST_0_CORE_FEASIBILITY.md`.
It has **no production caller**. It is not in the Electron app, the renderer,
`npm run check`, CI, or packaging. It uses the Rust standard library only.

## Question

Can a Rust core accept WRL Forge's existing offsets — UTF-16 code units from the
tokenizer, the source map, CodeMirror and `src/vrml/edit.js` — and apply exact
span patches over UTF-8 storage **without** any silent change to offsets or text?

This boundary was chosen first because it is the highest-risk boundary: every
later Rust module (parser, source map, identity, transactions) inherits it.

## Contract

- Public offsets stay **UTF-16 code units**, half-open `[from, to)`, exactly as
  in `src/vrml/edit.js`. Rust stores UTF-8 and converts at one place
  (`src/offsets.rs`).
- A UTF-16 offset strictly inside a surrogate pair is **refused**, never rounded.
  A UTF-8 offset inside a multi-byte scalar is **refused**.
- `apply_edits` / `map_offset` port `applyEdits` / `mapOffset` with the same
  error codes, the same canonical order, and the same caller indexes in errors.
- The canonical tie-break compares insert text by **UTF-16 code units**, as
  JavaScript `<` does. Rust byte order differs for U+E000..U+FFFF versus astral
  characters; the negative control below proves the harness sees that.
- Text outside the edited spans is copied verbatim (CRLF, lone CR, BOM, comments).

### Approved differences (additive refusals only)

| id | Rust | JavaScript | why it is safer |
|---|---|---|---|
| D1 `EEDITBOUNDARY` | refuses an edit endpoint inside a surrogate pair | applies it | JS can produce a lone surrogate; `Buffer.from(text,'utf8')` on save turns it into U+FFFD — silent corruption |
| D2 `EENCODING` | refuses ill-formed UTF-16 text or insert | applies it | UTF-8 cannot represent a lone surrogate |

Both checks run only after every JavaScript check passes, so Rust never reports
a *different* error than JavaScript; it only refuses some sets JS accepts. The
harness approves a difference only when an independent check (a direct surrogate
test on the original text, `String.prototype.isWellFormed`) proves the reason.

## Files

| file | purpose |
|---|---|
| `src/offsets.rs` | UTF-16 ↔ UTF-8 conversion, forward cursor, boundary errors |
| `src/edit.rs` | WD1.2 algebra port: validate, canonical order, apply, map |
| `src/wire.rs` | std-only line protocol; strings travel as UTF-16 hex so a lone surrogate reaches Rust unchanged |
| `src/main.rs` | stdin → stdout CLI for the harness |
| `differential.js` | identical inputs to `src/vrml/edit.js` and the Rust binary; classifies exact / approved / regression |

## Run

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings
cargo test                      # 17 hand-authored expectations
cargo build --release
node differential.js            # exit 0 = zero regressions
```

Output: `out/differential-summary.json` (gitignored).

## Result (2026-10-08, Linux x86_64, rustc 1.95.0, Node v24.21.0)

| measure | value |
|---|---|
| cases | 167,316 (A 64,869 · M 100,236 · C 2,211) |
| fixture files used | 65 (every `.wrl`/`.wrz`/`.x3dv`/gzip under `test/fixtures`, token spans from the production tokenizer) |
| exact match | 159,990 |
| D1 approved (boundary) | 3,120 — of which **2,723** JS outputs were ill-formed (would corrupt on save) |
| D2 approved (encoding) | 4,206 |
| **regressions** | **0** |
| input / baseline / Rust SHA-256 | `358ea723…` / `c8d70e04…` / `7e3d48f2…` — identical on re-run |

### Negative controls (each mutation applied, run, then reverted)

| mutation | regressions detected |
|---|---|
| tie-break by UTF-8 bytes instead of UTF-16 units | 646 |
| same-offset insertion ambiguity check removed | 1,509 |
| surrogate-pair boundary check removed | 2,727 |

## Limits

- Enumeration covers edit sets of size 1 and 2 over fixed small texts, plus
  sampled token spans of committed fixtures. It is not the private corpus.
- `createEdit` / `toRange` JavaScript shape checks (non-integer, negative,
  unknown keys) become type-system facts in Rust and are not compared here; a
  future FFI facade must re-check them at the JS boundary.
- `mapRange` is not ported; it composes two `mapOffset` calls.
- No performance claim. The edit algebra is not a measured bottleneck.
