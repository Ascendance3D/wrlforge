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
# STACK-REVIEW-1 QA HOLD fix: any failure, in any step, removes dist/, so no
# later packaging step can use a partial, changed or refused build.
built=0
trap '[[ $built -eq 1 ]] || rm -rf "$here/dist"' EXIT
wb="$here/target/tools/bin/wasm-bindgen"
[[ -x "$wb" ]] || { echo "missing $wb: cargo install wasm-bindgen-cli --version =0.2.129 --locked --root $here/target/tools" >&2; exit 2; }
cargo build --manifest-path "$here/Cargo.toml" -p wrlforge-ui --target wasm32-unknown-unknown $flag
rm -rf "$here/dist"; mkdir -p "$here/dist/vendor"
"$wb" --target web --no-typescript --out-dir "$here/dist" "$here/target/wasm32-unknown-unknown/$profile/wrlforge_ui.wasm"
ver="$(sed -n 's/^version=//p' "$here/xite/PIN" | head -1)"
pick="$here/../../src/preview/xite-pick-adapter.js"; gizmo="$here/ui/static/xite-gizmo-adapter.js"
# The SHA-256 of each adapter source BEFORE its check: the staged copy must
# carry exactly these bytes, so a source that changes after the check (and
# before or during staging) cannot ship, even with a valid declaration.
sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }
pick_sha="$(sha256 "$pick")"; gizmo_sha="$(sha256 "$gizmo")"
xite="${X_ITE_DIST:-}"
if [[ -z "$xite" ]]; then
  xite="$here/target/xite/$ver/dist"
  [[ -d "$xite" ]] || "$here/fetch-xite.sh"
fi
"$here/fetch-xite.sh" --verify "$xite" || exit 4
# Both adapters must accept exactly this build (their own runtime gate stays).
# The check reads the real declarations, never a comment or another string.
"$here/check-xite-adapters.sh" "$pick" "$gizmo" "$ver" || exit 4
cp "$here/ui/static/"* "$here/dist/"
# VISUAL-2: the WD2-D X_ITE picking adapter, verbatim (one source of truth;
# every private X_ITE access stays in that one file).
cp "$pick" "$here/dist/xite-pick-adapter.js"
# Exactly the verified manifest files (x_ite.min.js, x_ite.css, LICENSE.md,
# assets/): nothing unlisted is embedded, even from a fuller X_ITE_DIST.
# --install verifies the COPY again after copying (STACK-REVIEW-1 follow-up),
# so a source that changes after the check above cannot ship. On refusal,
# dist/ is removed so no later step can package a partial or changed copy.
"$here/fetch-xite.sh" --install "$xite" "$here/dist/vendor/x_ite" || { rm -rf "$here/dist"; exit 4; }
# STACK-REVIEW-1 QA HOLD fix: check the FINAL staged adapters, after every
# copy: regular files, the exact bytes checked above, byte-identical to the
# repository sources, and the same declaration check on the staged files.
"$here/check-xite-adapters.sh" --staged "$here/dist" "$pick" "$pick_sha" "$gizmo" "$gizmo_sha" "$ver" || { rm -rf "$here/dist"; exit 4; }
built=1
echo "dist ready: $(du -sh "$here/dist" | cut -f1) (X_ITE $ver from $xite)"
