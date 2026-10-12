#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build the Rust/Wasm UI into dist/ for the Tauri app. No Node.js is used.
#   ./build-ui.sh            release wasm (default)
#   ./build-ui.sh --debug    debug wasm
# X_ITE is the build pinned in xite/PIN (16.5.0), fetched and verified by
# fetch-xite.sh into target/xite/ -- NOT the root node_modules, which is the
# Electron app's 15.1.10. X_ITE_DIST may point at another copy, but every copy
# must pass the same version + per-file SHA-256 check. Nothing is changed.
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
ver="$(sed -n 's/^version=//p' "$here/xite/PIN" | head -1)"
xite="${X_ITE_DIST:-}"
if [[ -z "$xite" ]]; then
  xite="$here/target/xite/$ver/dist"
  [[ -d "$xite" ]] || "$here/fetch-xite.sh"
fi
"$here/fetch-xite.sh" --verify "$xite" || exit 4
# Both adapters must accept exactly this build (their own runtime gate stays).
# The check reads the real declarations, never a comment or another string.
"$here/check-xite-adapters.sh" "$here/../../src/preview/xite-pick-adapter.js" "$here/ui/static/xite-gizmo-adapter.js" "$ver" || exit 4
# Exactly the verified manifest files (x_ite.min.js, x_ite.css, LICENSE.md,
# assets/): nothing unlisted is embedded, even from a fuller X_ITE_DIST.
manifest="$here/xite/$(sed -n 's/^manifest=//p' "$here/xite/PIN" | head -1)"
while read -r _ f; do
  mkdir -p "$here/dist/vendor/x_ite/$(dirname "$f")"
  cp "$xite/$f" "$here/dist/vendor/x_ite/$f"
done < <(tr -d '\r' < "$manifest")
echo "dist ready: $(du -sh "$here/dist" | cut -f1) (X_ITE $ver from $xite)"
