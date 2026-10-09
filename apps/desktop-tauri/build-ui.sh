#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build the Rust/Wasm UI into dist/ for the Tauri app. No Node.js is used.
#   ./build-ui.sh            release wasm (default)
#   ./build-ui.sh --debug    debug wasm
# X_ITE is copied verbatim from the repository's pinned node_modules (15.1.10);
# set X_ITE_DIST to point elsewhere. The version is checked, never changed.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
profile=release; flag=--release
[[ "${1:-}" == "--debug" ]] && { profile=debug; flag=; }
wb="$here/target/tools/bin/wasm-bindgen"
[[ -x "$wb" ]] || { echo "missing $wb: cargo install wasm-bindgen-cli --version =0.2.129 --locked --root $here/target/tools" >&2; exit 2; }
cargo build --manifest-path "$here/Cargo.toml" -p wrlforge-ui --target wasm32-unknown-unknown $flag
rm -rf "$here/dist"; mkdir -p "$here/dist/vendor"
"$wb" --target web --no-typescript --out-dir "$here/dist" "$here/target/wasm32-unknown-unknown/$profile/wrlforge_ui.wasm"
cp "$here/ui/static/"* "$here/dist/"
# VISUAL-2: the WD2-D X_ITE picking adapter, verbatim (one source of truth;
# every private X_ITE access stays in that one file).
cp "$here/../../src/preview/xite-pick-adapter.js" "$here/dist/xite-pick-adapter.js"
xite="${X_ITE_DIST:-}"
if [[ -z "$xite" ]]; then
  for c in "$here/../../node_modules/x_ite/dist" "$here/../../../../../wrlforge/node_modules/x_ite/dist"; do
    [[ -f "$c/x_ite.min.js" ]] && { xite="$c"; break; }
  done
fi
[[ -n "$xite" && -f "$xite/x_ite.min.js" ]] || { echo "X_ITE dist not found; set X_ITE_DIST" >&2; exit 3; }
ver="$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$xite/../package.json" | head -1)"
[[ "$ver" == "15.1.10" ]] || { echo "X_ITE version $ver != pinned 15.1.10; refusing" >&2; exit 4; }
mkdir -p "$here/dist/vendor/x_ite"
cp "$xite/x_ite.min.js" "$xite/x_ite.css" "$xite/LICENSE.md" "$here/dist/vendor/x_ite/"
cp -r "$xite/assets" "$here/dist/vendor/x_ite/"
echo "dist ready: $(du -sh "$here/dist" | cut -f1) (X_ITE $ver from $xite)"
