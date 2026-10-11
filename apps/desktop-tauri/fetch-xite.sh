#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# XITE-16-UPGRADE-2: obtain and verify the X_ITE build pinned in xite/PIN.
#   ./fetch-xite.sh               download (build time only) into target/xite/<version>/
#   ./fetch-xite.sh --verify DIR  check an X_ITE dist/ DIR against the pin; no network
# The tarball's SHA-256 and every copied file's SHA-256 must match the pin,
# and the package version must equal it. Nothing is ever rebuilt or patched.
# The app never downloads X_ITE at runtime: build-ui.sh embeds this copy.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
pin="$here/xite/PIN"
val() { sed -n "s/^$1=//p" "$pin" | head -1; }
version="$(val version)"; tarball="$(val tarball)"; tar_sha="$(val tarball_sha256)"
manifest="$here/xite/$(val manifest)"
[[ -n "$version" && -n "$tarball" && -n "$tar_sha" && -f "$manifest" ]] || { echo "incomplete $pin" >&2; exit 2; }
sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# verify DIR: DIR/../package.json names the pinned version, every manifest
# file exists with its exact hash, and none of them is a symlink.
verify() {
  local dist="$1" ver want f got bad=0
  [[ -f "$dist/../package.json" ]] || { echo "no package.json beside $dist" >&2; return 4; }
  ver="$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$dist/../package.json" | head -1)"
  [[ "$ver" == "$version" ]] || { echo "X_ITE version $ver != pinned $version; refusing" >&2; return 4; }
  while read -r want f; do
    if [[ -L "$dist/$f" || ! -f "$dist/$f" ]]; then echo "missing or not a regular file: $f" >&2; bad=1; continue; fi
    got="$(sha256 "$dist/$f")"
    [[ "$got" == "$want" ]] || { echo "hash mismatch: $f" >&2; bad=1; }
  done < <(tr -d '\r' < "$manifest")
  [[ $bad -eq 0 ]] || { echo "X_ITE dist $dist does not match $manifest; refusing" >&2; return 5; }
}

if [[ "${1:-}" == "--verify" ]]; then
  verify "${2:?--verify DIR}"
  exit
fi

dest="$here/target/xite/$version"
if [[ -d "$dest/dist" ]] && verify "$dest/dist" 2>/dev/null; then
  echo "X_ITE $version already verified at $dest"
  exit 0
fi
tmp="$(mktemp -d "${TMPDIR:-/tmp}/wrlforge-xite.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 -o "$tmp/x_ite.tgz" "$tarball"
got="$(sha256 "$tmp/x_ite.tgz")"
[[ "$got" == "$tar_sha" ]] || { echo "tarball SHA-256 $got != pinned $tar_sha; refusing" >&2; exit 5; }
mkdir "$tmp/ex"
tar -xzf "$tmp/x_ite.tgz" -C "$tmp/ex" package/package.json package/dist
# Copy ONLY the manifest's files (no extras), then verify the copy itself.
rm -rf "$dest"; mkdir -p "$dest/dist"
cp "$tmp/ex/package/package.json" "$dest/package.json"
while read -r _ f; do
  mkdir -p "$dest/dist/$(dirname "$f")"
  cp "$tmp/ex/package/dist/$f" "$dest/dist/$f"
done < <(tr -d '\r' < "$manifest")
verify "$dest/dist"
echo "X_ITE $version fetched and verified at $dest"
