# WRL Forge desktop (Tauri 2 / Rust) — TAURI-RUST-MIGRATION-1

A replacement desktop application in progress. **No Electron, no Node.js
runtime.** Linux first; macOS 15+ is a target but has not been built yet.
Windows and mobile are out of scope.

Status and the module map: `docs/architecture/TAURI_RUST_MIGRATION_1.md`.

## Prerequisites (Linux)

* Rust 1.95 with the `wasm32-unknown-unknown` target.
* Tauri system libraries: `libwebkit2gtk-4.1-dev libsoup-3.0-dev libgtk-3-dev
  librsvg2-dev libxdo-dev libssl-dev` (already present on the dev host).
* `wasm-bindgen-cli` 0.2.129, installed locally (no sudo):
  `cargo install wasm-bindgen-cli --version =0.2.129 --locked --root target/tools`
* X_ITE 15.1.10 from the repository's `node_modules/x_ite/dist` (only copied;
  Node is not run). Override with `X_ITE_DIST=…`.

## Build and run

```sh
cd apps/desktop-tauri
./build-ui.sh                 # Leptos UI -> Wasm -> dist/   (--debug for a debug build)
cargo build -p wrl-forge-desktop          # or --release
./target/debug/wrl-forge [file.wrl]       # optional file to open at launch
```

`dist/` is embedded into the binary at compile time, so rebuild the binary
after `./build-ui.sh`.

## Tests

```sh
cargo test -p wrl-forge-desktop                       # file + session services
(cd ../../crates && cargo test -p wrlforge-vrml -p wrlforge-document)
./smoke.sh --headless file.wrl...                     # in-window end-to-end, on temp copies
./smoke.sh --headless --inspector file.wrl...         # also drives a real Inspector field edit
./smoke.sh --headless --create                        # VISUAL-1: New World → Create → edit → save → reopen
./smoke.sh --headless --pick                          # VISUAL-2: viewport picking (node: oracle plan; xdotool: real clicks in Xvfb)
./smoke.sh --headless --theme tokyo-night-light file.wrl...   # drives the theme selector (UI-THEME-1)
(cd ../.. && node scripts/check-rust-node-schema-parity.js)  # Rust/JS node schema equality
../../spikes/tauri-rust-migration-1/parity.sh <dir>   # JS-vs-Rust parser parity (needs node)
../../spikes/tauri-rust-migration-1/highlight-parity.sh <dir>  # Rust highlight vs language.js (needs node)
WRLFORGE_SMOKE_ALIGN_HOLD_MS=2500 ./smoke.sh ...      # pause at each syntax-layer registration state (screenshots)
```

Every smoke run also checks the syntax color layer (UI-SYNTAX-1): colors
from the current revision only, registration with the textarea at four scroll
positions, typing / paste / undo / redo / reload, stale-reply rejection,
diagnostic underlines and theme recoloring.

`smoke.sh` always copies its inputs to a fresh `/tmp` directory first. It never
touches the original files. Settings go to a temporary `--config-dir` per file,
never to the user's real preferences (`<app config dir>/settings.json`).
