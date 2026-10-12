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

# safe_rel REL: a manifest path is relative, non-empty and has no empty, `.`
# or `..` component, so it cannot name anything outside the dist directory.
safe_rel() {
  local part; local -a parts
  [[ -n "$1" && "$1" != /* && "$1" != */ ]] || return 1
  IFS=/ read -ra parts <<< "$1"
  for part in "${parts[@]}"; do
    [[ -n "$part" && "$part" != . && "$part" != .. ]] || return 1
  done
}
# no_links ROOT REL: no component of ROOT/REL below ROOT is a symlink, so an
# intermediate directory link cannot redirect the check to another tree.
no_links() {
  local p="$1" part; local -a parts
  IFS=/ read -ra parts <<< "$2"
  for part in "${parts[@]}"; do
    p="$p/$part"
    [[ ! -L "$p" ]] || return 1
  done
}
# Every manifest line is "<64 hex> <safe relative path>"; anything else refuses.
while read -r want f; do
  [[ "$want" =~ ^[0-9a-f]{64}$ ]] && safe_rel "$f" || { echo "bad manifest line in $manifest: $want $f" >&2; exit 2; }
done < <(tr -d '\r' < "$manifest")

# verify DIR: DIR/../package.json names the pinned version, every manifest
# file exists with its exact hash, and neither DIR, the
# package.json nor ANY component of a manifest path is a symlink.
verify() {
  local dist="$1" ver want f got bad=0
  while [[ "$dist" == */ && "$dist" != / ]]; do dist="${dist%/}"; done
  [[ -d "$dist" && ! -L "$dist" ]] || { echo "dist $dist is not a real directory (symlink or missing); refusing" >&2; return 4; }
  [[ -f "$dist/../package.json" && ! -L "$dist/../package.json" ]] || { echo "no regular package.json beside $dist" >&2; return 4; }
  ver="$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$dist/../package.json" | head -1)"
  [[ "$ver" == "$version" ]] || { echo "X_ITE version $ver != pinned $version; refusing" >&2; return 4; }
  while read -r want f; do
    if ! no_links "$dist" "$f"; then echo "symlink in path: $f" >&2; bad=1; continue; fi
    if [[ ! -f "$dist/$f" ]]; then echo "missing or not a regular file: $f" >&2; bad=1; continue; fi
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
  cp -P "$tmp/ex/package/dist/$f" "$dest/dist/$f"   # a link stays a link, so verify refuses it
done < <(tr -d '\r' < "$manifest")
verify "$dest/dist"
echo "X_ITE $version fetched and verified at $dest"
