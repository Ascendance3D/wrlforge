#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# STACK-REVIEW-1: negative tests for the X_ITE build guards. No network: the
# symlink cases run fetch-xite.sh --verify against a synthetic pin + manifest
# in a temp copy, and the adapter cases run check-xite-adapters.sh against
# edited temp copies of the real adapters. Every forbidden case must refuse.
#   ./test-xite-guards.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
t="$(mktemp -d "${TMPDIR:-/tmp}/wrlforge-xite-guards.XXXXXX")"
trap 'rm -rf "$t"' EXIT
pass=0; fail=0
expect() { # expect ok|refuse NAME CMD...
  local want="$1" name="$2"; shift 2
  local rc=0; "$@" >"$t/out" 2>&1 || rc=$?
  if { [[ "$want" == ok && $rc -eq 0 ]] || [[ "$want" == refuse && $rc -ne 0 ]]; }; then
    echo "PASS $name (rc=$rc)"; pass=$((pass + 1))
  else
    echo "FAIL $name (want $want, rc=$rc)"; sed 's/^/    /' "$t/out"; fail=$((fail + 1))
  fi
}

# ---- symlink cases: a synthetic pinned build -------------------------------
mk() { mkdir -p "$(dirname "$1")"; printf '%s\n' "$2" > "$1"; }
sha() { sha256sum "$1" | cut -d' ' -f1; }
k="$t/kit"; mkdir -p "$k/xite"; cp "$here/fetch-xite.sh" "$k/"
mk "$t/good/dist/x_ite.min.js" 'min'
mk "$t/good/dist/assets/lib/a.js" 'deep'
mk "$t/good/dist/assets/b.js" 'shallow'
mk "$t/good/package.json" '{ "name": "x_ite", "version": "9.9.9" }'
{ for f in x_ite.min.js assets/lib/a.js assets/b.js; do echo "$(sha "$t/good/dist/$f")  $f"; done; } > "$k/xite/m.sha256"
printf 'version=9.9.9\ntarball=https://invalid.example/none.tgz\ntarball_sha256=00\nmanifest=m.sha256\n' > "$k/xite/PIN"
v() { "$k/fetch-xite.sh" --verify "$1"; }
fresh() { rm -rf "$t/c"; cp -a "$t/good" "$t/c"; }

expect ok     valid-regular-tree             v "$t/good/dist"
expect ok     valid-trailing-slash           v "$t/good/dist/"
fresh; mv "$t/c/dist/x_ite.min.js" "$t/c/real.js"; ln -s ../real.js "$t/c/dist/x_ite.min.js"
expect refuse final-file-symlink-correct-hash v "$t/c/dist"
fresh; mv "$t/c/dist/assets" "$t/c/assets-real"; ln -s ../assets-real "$t/c/dist/assets"
expect refuse intermediate-dir-symlink        v "$t/c/dist"
fresh; mv "$t/c/dist/assets/lib" "$t/c/lib-real"; ln -s ../../lib-real "$t/c/dist/assets/lib"
expect refuse deep-intermediate-dir-symlink   v "$t/c/dist"
fresh; mkdir -p "$t/outside"; cp "$t/c/dist/assets/b.js" "$t/outside/b.js"; rm "$t/c/dist/assets/b.js"; ln -s "$t/outside/b.js" "$t/c/dist/assets/b.js"
expect refuse symlink-outside-tree            v "$t/c/dist"
fresh; mkdir -p "$t/outside2"; cp -a "$t/c/dist/assets" "$t/outside2/"; rm -rf "$t/c/dist/assets"; ln -s "$t/outside2/assets" "$t/c/dist/assets"
expect refuse dir-symlink-outside-tree        v "$t/c/dist"
fresh; mv "$t/c/dist" "$t/c/dist-real"; ln -s dist-real "$t/c/dist"
expect refuse dist-dir-symlink                v "$t/c/dist"
fresh; mv "$t/c/package.json" "$t/c/pkg-real.json"; ln -s pkg-real.json "$t/c/package.json"
expect refuse package-json-symlink            v "$t/c/dist"
fresh; printf 'x' >> "$t/c/dist/assets/lib/a.js"
expect refuse hash-mismatch                   v "$t/c/dist"
fresh; sed -i 's/9.9.9/9.9.8/' "$t/c/package.json"
expect refuse version-mismatch                v "$t/c/dist"
cp "$k/xite/m.sha256" "$t/m.bak"
for bad in '../package.json' '/etc/hostname' 'assets/../x_ite.min.js' 'assets//b.js' './x_ite.min.js' 'assets/'; do
  cp "$t/m.bak" "$k/xite/m.sha256"; echo "$(printf '0%.0s' {1..64})  $bad" >> "$k/xite/m.sha256"
  expect refuse "manifest-path:$bad"          v "$t/good/dist"
done
cp "$t/m.bak" "$k/xite/m.sha256"
expect ok     valid-after-manifest-restore   v "$t/good/dist"

# ---- adapter declaration cases --------------------------------------------
pick="$here/../../src/preview/xite-pick-adapter.js"; giz="$here/ui/static/xite-gizmo-adapter.js"
ver="$(sed -n 's/^version=//p' "$here/xite/PIN" | head -1)"
c() { "$here/check-xite-adapters.sh" "$1" "$2" "$ver"; }
P="$t/pick.js"; G="$t/giz.js"
decl="  const XITE_VERSIONS = Object.freeze(['15.1.10', '$ver']);"
grep -qxF "$decl" "$pick" || { echo "FAIL pick adapter declaration changed; update this test"; exit 1; }
nover="  const XITE_VERSIONS = Object.freeze(['15.1.10']);"
put() { cp "$pick" "$P"; cp "$giz" "$G"; python3 -I -c 'import sys; p,o,n=sys.argv[1:]; s=open(p).read(); assert o in s; open(p,"w").write(s.replace(o,n,1))' "$@"; }

expect ok     adapter-valid-declaration      c "$pick" "$giz"
put "$P" "$decl" "$nover"
expect refuse adapter-version-removed        c "$P" "$G"
put "$P" "$decl" "$nover
  // supported: '$ver'
  // const XITE_VERSIONS = Object.freeze(['15.1.10', '$ver']);"
expect refuse adapter-version-only-in-comment c "$P" "$G"
put "$P" "$decl" "$nover
  const UNRELATED = '$ver'; const NOTE = \"XITE_VERSIONS '$ver'\";"
expect refuse adapter-version-in-other-string c "$P" "$G"
put "$P" "$decl" "$nover // '$ver'"
expect refuse adapter-trailing-comment       c "$P" "$G"
put "$P" "$decl" "$decl
$decl"
expect refuse adapter-duplicate-declaration  c "$P" "$G"
put "$P" "$decl" "  const XITE_VERSIONS = Object.freeze(['15.1.10', '$ver.1']);"
expect refuse adapter-prefix-version         c "$P" "$G"
put "$P" "$decl" "  const XITE_VERSIONS = Object.freeze(['15.1.10', '$ver', x]);"
expect refuse adapter-non-literal-element    c "$P" "$G"
put "$G" "  var XITE_VERSION = '$ver';" "  var XITE_VERSION = '0.0.0'; // var XITE_VERSION = '$ver';"
expect refuse gizmo-version-only-in-comment  c "$P" "$G"
put "$G" "  var XITE_VERSION = '$ver';" "  // var XITE_VERSION = '$ver';"
expect refuse gizmo-declaration-commented    c "$P" "$G"
printf '\nSUMMARY pass=%d fail=%d\n' "$pass" "$fail"
[[ $fail -eq 0 ]]
