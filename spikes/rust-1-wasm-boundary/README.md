# RUST-1 — WebAssembly boundary proof (isolated)

Proof that the Rust text core (`crates/wrlforge-text`) can cross the
JavaScript/WebAssembly boundary (`crates/wrlforge-wasm`) without silent text
changes. It has **no production caller**. It is not in `package.json`
scripts, `npm run check`, CI or packaging.
Contract: `docs/architecture/RUST_1_BOUNDARY_CONTRACT.md`. Results:
`RUST_1_WASM_QA.md`, `RUST_1_PERFORMANCE.md`.

## One-time tool setup (Linux)

```sh
rustup target add wasm32-unknown-unknown          # crates/rust-toolchain.toml pins 1.95.0
cargo install wasm-bindgen-cli --version 0.2.129 --locked --root crates/target/tools
npm ci                                            # existing lockfile; provides electron
```

## Run (from the repository root)

```sh
(cd crates && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked)
node spikes/rust-1-wasm-boundary/build.mjs --negative-controls   # out/pkg + out/neg/*/pkg + out/build-manifest.json
node --test spikes/rust-1-wasm-boundary/test/*.test.mjs          # Node 24 load proof
node spikes/rust-1-wasm-boundary/harness/run.mjs --negative-controls   # differential, exit 0 = pass
node --expose-gc spikes/rust-1-wasm-boundary/perf/node-bench.mjs # out/perf-node.json
node spikes/rust-1-wasm-boundary/electron/run.cjs                # renderer proof via VisualQaRunner
```

## Layout

| path | purpose |
|---|---|
| `build.mjs` | reproducible release build (+ mutant builds in separate target dirs) |
| `loaders/node.mjs` | Node loader (`initSync` from bytes) |
| `test/node-proof.test.mjs` | Node proof, adversarial inputs, sessions |
| `harness/framework.mjs` | reusable differential framework (stage adapter contract) |
| `harness/registry.json` | approved-difference registry (ids are NOT owner decisions D1–D9) |
| `harness/stages/edit-algebra.mjs` | implemented stage; regenerates the RUST-0 case set exactly |
| `harness/stages/planned.mjs` | interfaces for tokenize, parse, source-map, semantics, node-identity, transaction (not implemented) |
| `harness/run.mjs` | runner; release + negative controls |
| `perf/` | boundary benchmark shared by Node and the renderer |
| `electron/` | isolated Electron main + proof pages (production CSP copy and a no-`wasm-unsafe-eval` control) |

`out/` is gitignored and fully regenerable.
