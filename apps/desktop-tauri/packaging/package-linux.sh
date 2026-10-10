#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# PKG-LINUX-1: build the Linux AppImage and Debian package.
#   1. Leptos UI -> release Wasm -> dist/ (with X_ITE 15.1.10), via build-ui.sh
#   2. Stage license texts into target/pkg-licenses/ (bundled as resources)
#   3. cargo tauri build --bundles deb,appimage (release backend; dist/ is
#      embedded in the binary, so the installed app needs no repository files)
# Needs: Rust 1.95 + wasm32 target, tauri-cli 2 (`cargo tauri`), the
# wasm-bindgen 0.2.129 in target/tools (see README), python3, and network
# access on the first AppImage build (Tauri downloads linuxdeploy).
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
repo="$(cd "$here/../.." && pwd)"
cd "$here"
# Keep the builder's home and checkout paths out of the shipped binaries
# (panic locations embed source paths). Appends to any caller RUSTFLAGS.
# rustc applies the LAST matching rule, so the catch-all $HOME rule is first.
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$HOME=/home/builder --remap-path-prefix=$cargo_home/registry/src=/cargo/registry/src --remap-path-prefix=$cargo_home/git/checkouts=/cargo/git/checkouts --remap-path-prefix=$repo=/wrlforge"
# Use every core: the workspace release profile (fat LTO, codegen-units = 1)
# serializes the final link on one core. Packaging builds use thin LTO split
# over one codegen unit per core. Override with the same variables.
export CARGO_PROFILE_RELEASE_LTO="${CARGO_PROFILE_RELEASE_LTO:-thin}"
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS="${CARGO_PROFILE_RELEASE_CODEGEN_UNITS:-$(nproc)}"
./build-ui.sh
lic="$here/target/pkg-licenses"
rm -rf "$lic"; mkdir -p "$lic"
cp "$repo/LICENSE" "$repo/COPYRIGHT.md" "$repo/THIRD_PARTY_NOTICES.md" \
   "$here/packaging/README-licenses.txt" "$lic/"
cp "$here/dist/vendor/x_ite/LICENSE.md" "$lic/X_ITE-LICENSE.md"
cp "$here/ui/static/LICENSE-tokyo-night.txt" "$lic/"
python3 "$here/packaging/collect-rust-licenses.py" "$here/Cargo.toml" "$lic/RUST-CRATES-LICENSES.txt"
cargo tauri build --bundles deb,appimage
# No private path may ship: fail the build if one does.
if rg -l -a --fixed-strings -e "$HOME" -e "$repo" "$lic" "$here/dist" "$here/target/release/wrl-forge"; then
  echo "private build path found in packaged files; refusing" >&2; exit 5
fi
ls -l "$here/target/release/bundle/deb/"*.deb "$here/target/release/bundle/appimage/"*.AppImage
