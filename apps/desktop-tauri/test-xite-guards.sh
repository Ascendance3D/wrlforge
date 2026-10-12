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
    echo "PASS $name (rc=$rc)${XG_WHY:+: $(head -1 "$t/out")}"; pass=$((pass + 1))
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

# ---- STACK-REVIEW-1 follow-up: lexer-based declaration cases ---------------
# Each case is a whole synthetic pick adapter (the real gizmo adapter stays),
# so a case cannot pass because of the real file's own declaration.
J="$t/js.js"
js() { printf '%s\n' "$@" > "$J"; }
cj() { "$here/check-xite-adapters.sh" "$J" "$giz" "$ver"; }
iife() { js "'use strict';" '(function () {' "$@" '})();'; }
ok1="  const XITE_VERSIONS = Object.freeze(['15.1.10', '$ver']);"
iife "$ok1" '  return XITE_VERSIONS;'
expect ok     js-current-one-line-declaration  cj
iife '  // The pinned builds (see PIN).' "$ok1  // exact strings only" '  /* trailing block */'
expect ok     js-normal-surrounding-comments  cj
iife '/*' "$ok1" '*/' '  const XITE_VERSIONS = Object.freeze([' "    '15.1.10'" '  ]);'
expect refuse js-block-comment-then-wrong-real cj
iife '/*' "$ok1" '*/'
expect refuse js-block-comment-only            cj
iife "  /* $ok1 */"
expect refuse js-inline-block-comment-only     cj
iife "  const NOTE = \"$ok1\";" "  const XITE_VERSIONS = Object.freeze(['15.1.10']);"
expect refuse js-declaration-in-string        cj
iife '  const T = `' "$ok1" '`;' "  const XITE_VERSIONS = Object.freeze(['15.1.10']);"
expect refuse js-declaration-in-template      cj
iife '  const T = `${`' "$ok1" '`}`;'
expect refuse js-declaration-in-nested-template cj
iife '  const T = `${ "}" }' "$ok1" '`;' "  const XITE_VERSIONS = Object.freeze(['15.1.10']);"
expect refuse js-template-brace-in-string     cj
iife "  const OTHER = Object.freeze(['15.1.10', '$ver']);" "  const XITE_VERSIONS = Object.freeze(['15.1.10']);"
expect refuse js-version-in-unrelated-variable cj
iife "$ok1" "$ok1"
expect refuse js-duplicate-declaration         cj
iife "$ok1" "  let XITE_VERSIONS = ['$ver'];"
expect refuse js-duplicate-other-keyword       cj
iife "  const X = 1;"
expect refuse js-missing-declaration           cj
iife '  const XITE_VERSIONS = Object.freeze([' "    '15.1.10', '$ver'" '  ]);'
expect refuse js-multi-line-shape-unsupported  cj
iife "  const XITE_VERSIONS = Object.freeze([\"15.1.10\", \"$ver\"]);"
expect refuse js-double-quoted-unsupported     cj
iife "  const XITE_VERSIONS = Object.freeze(['15.1.10', '$ver', '1.0.0x']);"
expect refuse js-malformed-element             cj
iife '  if (false) {' "  $ok1" '  }'
expect refuse js-declaration-in-dead-block     cj
iife '  function unused() {' "  $ok1" '  }'
expect refuse js-declaration-in-inner-function cj
js "'use strict';" "$ok1" '(function () {' '})();'
expect refuse js-declaration-outside-iife      cj
js '(function () {' "$ok1" '})();' '(function () { })();'
expect refuse js-second-top-level-statement    cj
iife "$ok1" '  const s = /[/*]/;'
expect refuse js-regex-literal-unsupported     cj
iife "$ok1" "  const s = 'open"
expect refuse js-unterminated-string           cj
iife "$ok1" '  /* open'
expect refuse js-unterminated-block-comment    cj
iife "$ok1" '  const s = `open'
expect refuse js-unterminated-template         cj
iife "$ok1" "  const s = '\\" "';"
expect refuse js-string-line-continuation      cj
iife "$ok1" '  if (x) {'
expect refuse js-unbalanced-brace              cj
iife "$ok1" '  const half = a / 2; const q = (b) / c;'
expect ok     js-division-is-code              cj
# gizmo: the same lexer, through the var declaration
gj() { "$here/check-xite-adapters.sh" "$pick" "$J" "$ver"; }
okg="  var XITE_VERSION = '$ver';"
js '(function () {' "$okg" '})();'
expect ok     gizmo-js-one-line-declaration   gj
js '(function () {' '/*' "$okg" '*/' "  var XITE_VERSION = '0.0.0';" '})();'
expect refuse gizmo-js-block-comment-then-wrong gj
js '(function () {' "$okg" "  XITE_VERSION = '0.0.0';" '})();'
expect refuse gizmo-js-reassignment            gj
js '(function () {' "$okg" "  XITE_VERSION += '.1';" '})();'
expect refuse gizmo-js-compound-assignment     gj
js '(function () {' '  const T = `' "$okg" '`;' '})();'
expect refuse gizmo-js-declaration-in-template gj
js '(function () {' "$okg" '  if (v === XITE_VERSION) { return v == XITE_VERSION; }' '})();'
expect ok     gizmo-js-comparisons-are-reads   gj

# ---- STACK-REVIEW-1 follow-up: the copied (embedded) files -----------------
# --install copies a verified tree and re-verifies the COPY, so a source that
# changes between the build check and the copy cannot ship.
inst() { rm -rf "$t/dst"; "$k/fetch-xite.sh" --install "$1" "$t/dst"; }
vc() { "$k/fetch-xite.sh" --verify-copy "$t/dst"; }
expect ok     copy-valid-install             inst "$t/good/dist"
expect ok     copy-valid-verify              vc
fresh; expect ok copy-source-checked-first   v "$t/c/dist"
printf 'x' >> "$t/c/dist/x_ite.min.js"   # changed after the check, before the copy
expect refuse copy-source-changed-after-check inst "$t/c/dist"
fresh; mv "$t/c/dist/assets/b.js" "$t/c/b.js"; ln -s ../../b.js "$t/c/dist/assets/b.js"
expect refuse copy-source-became-symlink     inst "$t/c/dist"
fresh; rm "$t/c/dist/assets/lib/a.js"
expect refuse copy-source-file-removed       inst "$t/c/dist"
inst "$t/good/dist" >/dev/null 2>&1; rm "$t/dst/assets/b.js"
expect refuse copy-missing-file              vc
inst "$t/good/dist" >/dev/null 2>&1; printf 'x' >> "$t/dst/assets/b.js"
expect refuse copy-changed-bytes             vc
inst "$t/good/dist" >/dev/null 2>&1; mv "$t/dst/x_ite.min.js" "$t/min.js"; ln -s "$t/min.js" "$t/dst/x_ite.min.js"
expect refuse copy-file-symlink              vc
inst "$t/good/dist" >/dev/null 2>&1; mv "$t/dst/assets/lib" "$t/lib2"; ln -s "$t/lib2" "$t/dst/assets/lib"
expect refuse copy-dir-symlink               vc
inst "$t/good/dist" >/dev/null 2>&1; printf 'extra' > "$t/dst/assets/extra.js"
expect refuse copy-extra-file                vc
inst "$t/good/dist" >/dev/null 2>&1; ln -s b.js "$t/dst/assets/alias.js"
expect refuse copy-extra-symlink             vc
inst "$t/good/dist" >/dev/null 2>&1
expect refuse copy-destination-exists        "$k/fetch-xite.sh" --install "$t/good/dist" "$t/dst"
rm -rf "$t/dst"; ln -s "$t/good/dist" "$t/dst"
expect refuse copy-destination-symlink       vc
rm -f "$t/dst"
# build-ui.sh must embed through --install and stop (removing dist/) on refusal
bu() { awk '/fetch-xite.sh" --install "\$xite" "\$here\/dist\/vendor\/x_ite" \|\| \{ rm -rf "\$here\/dist"; exit 4; \}/ { f = 1 } END { exit !f }' "$here/build-ui.sh"; }
expect ok     build-ui-installs-with-copy-check bu
nocp() { ! awk '/^[^#]*cp .*\$xite/ { f = 1 } END { exit !f }' "$here/build-ui.sh"; }
expect ok     build-ui-has-no-unchecked-copy nocp

# ---- PR #144 review r4244998855: the fetch path (tarball -> target/xite) ----
# cp -P follows a link in a PARENT directory, so a linked package/dist/assets
# used to be dereferenced into regular files that passed the final check.
# --from-tarball is the fetch without the download; the SHA pin still applies.
cp "$k/xite/PIN" "$t/PIN.bak"
pk() { rm -rf "$t/pk"; mkdir -p "$t/pk"; cp -a "$t/good" "$t/pk/package"; }   # package/{package.json,dist}
tgz() { # tgz: tar $t/pk into $t/x.tgz and pin its SHA-256
  rm -f "$t/x.tgz"; tar -czf "$t/x.tgz" -C "$t/pk" package
  sed "s/^tarball_sha256=.*/tarball_sha256=$(sha "$t/x.tgz")/" "$t/PIN.bak" > "$k/xite/PIN"
}
ft() { rm -rf "$k/target"; "$k/fetch-xite.sh" --from-tarball "$t/x.tgz"; }
fdest="$k/target/xite/9.9.9"
pk; tgz
expect ok     fetch-tarball-valid              ft
expect ok     fetch-tarball-result-verifies    "$k/fetch-xite.sh" --verify "$fdest/dist"
nolinks() { [[ -z "$(find "$fdest" -type l)" ]]; }
expect ok     fetch-tarball-result-has-no-link nolinks
pk; mkdir -p "$t/pk/real"; mv "$t/pk/package/dist/assets" "$t/pk/real/"; ln -s ../../real/assets "$t/pk/package/dist/assets"; tgz
expect refuse fetch-tarball-intermediate-dir-link ft
nodest() { [[ ! -e "$fdest" ]]; }
expect ok     fetch-tarball-refusal-writes-nothing nodest
pk; mkdir -p "$t/pk/real"; mv "$t/pk/package/dist/assets/lib" "$t/pk/real/"; ln -s ../../../real/lib "$t/pk/package/dist/assets/lib"; tgz
expect refuse fetch-tarball-deep-dir-link      ft
pk; mv "$t/pk/package/dist/x_ite.min.js" "$t/pk/package/min.js"; ln -s ../min.js "$t/pk/package/dist/x_ite.min.js"; tgz
expect refuse fetch-tarball-final-file-link    ft
pk; mv "$t/pk/package/dist" "$t/pk/package/dist-real"; ln -s dist-real "$t/pk/package/dist"; tgz
expect refuse fetch-tarball-dist-link          ft
pk; mv "$t/pk/package/package.json" "$t/pk/package/p.json"; ln -s p.json "$t/pk/package/package.json"; tgz
expect refuse fetch-tarball-package-json-link  ft
pk; printf 'x' >> "$t/pk/package/dist/assets/b.js"; tgz
expect refuse fetch-tarball-hash-mismatch      ft
pk; tgz; printf 'x' >> "$t/x.tgz"
expect refuse fetch-tarball-sha-not-pinned     ft
pk; printf 'extra' > "$t/pk/package/dist/assets/extra.js"; tgz
expect ok     fetch-tarball-extra-file-ignored ft
noextra() { [[ ! -e "$fdest/dist/assets/extra.js" ]]; }
expect ok     fetch-tarball-extra-not-copied   noextra
cp "$t/PIN.bak" "$k/xite/PIN"; rm -rf "$k/target"
# install_copy refuses a linked SOURCE before copying (cp would follow it)
fresh; mv "$t/c/dist/assets" "$t/c/assets-real"; ln -s ../assets-real "$t/c/dist/assets"
expect refuse install-source-intermediate-link inst "$t/c/dist"

# ---- STACK-REVIEW-1 QA HOLD: the FINAL staged adapters in dist/ ------------
# (a) check-xite-adapters.sh --staged on a staged dir built here from the
# real adapters. S is the stage; the SHAs are the sources' SHAs "at check".
S="$t/stage"; PS="$t/src-pick.js"; GS="$t/src-giz.js"
sh_() { sha256sum "$1" | cut -d' ' -f1; }
stage() { rm -rf "$S"; mkdir -p "$S"; cp "$pick" "$PS"; cp "$giz" "$GS"; cp "$PS" "$S/xite-pick-adapter.js"; cp "$GS" "$S/xite-gizmo-adapter.js"; PSHA="$(sh_ "$PS")"; GSHA="$(sh_ "$GS")"; }
st() { "$here/check-xite-adapters.sh" --staged "$S" "$PS" "$PSHA" "$GS" "$GSHA" "$ver"; }
edit() { python3 -I -c 'import sys; p,o,n=sys.argv[1:]; s=open(p).read(); assert o in s; open(p,"w").write(s.replace(o,n,1))' "$@"; }
gdecl="  var XITE_VERSION = '$ver';"
stage
expect ok     staged-01-both-valid             st
stage; rm "$S/xite-pick-adapter.js"
expect refuse staged-02-pick-missing           st
stage; rm "$S/xite-gizmo-adapter.js"
expect refuse staged-03-gizmo-missing          st
stage; printf 'x' >> "$S/xite-pick-adapter.js"
expect refuse staged-04-pick-changed           st
stage; printf 'x' >> "$S/xite-gizmo-adapter.js"
expect refuse staged-05-gizmo-changed          st
stage; edit "$S/xite-pick-adapter.js" "$decl" "$nover"
expect refuse staged-06-pick-version-removed   st
stage; edit "$S/xite-gizmo-adapter.js" "$gdecl" "  var XITE_VERSION = '0.0.1';"
expect refuse staged-07-gizmo-wrong-version    st
stage; edit "$S/xite-pick-adapter.js" "$decl" "$nover
  // const XITE_VERSIONS = Object.freeze(['15.1.10', '$ver']);"
expect refuse staged-08-version-only-in-comment st
stage; edit "$S/xite-pick-adapter.js" "$decl" "$decl  // staged-only comment"
XG_WHY=1 expect refuse staged-09-changed-bytes-valid-declaration st
stage; mv "$S/xite-pick-adapter.js" "$t/linked-pick.js"; ln -s "$t/linked-pick.js" "$S/xite-pick-adapter.js"
expect refuse staged-10-pick-symlink           st
stage; mv "$S/xite-gizmo-adapter.js" "$t/linked-giz.js"; ln -s "$t/linked-giz.js" "$S/xite-gizmo-adapter.js"
expect refuse staged-10-gizmo-symlink          st
# 11: the source changes after its SHA was taken and checked; the stage is
# the changed source, so stage == source now, but not the checked bytes.
stage; edit "$PS" "$decl" "$decl  // changed after the source check"; cp "$PS" "$S/xite-pick-adapter.js"
XG_WHY=1 expect refuse staged-11-source-changed-after-check st
stage; edit "$GS" "$gdecl" "$gdecl  // changed after the source check"; cp "$GS" "$S/xite-gizmo-adapter.js"
expect refuse staged-11-gizmo-source-changed-after-check st
stage
same() { cmp -s "$pick" "$S/xite-pick-adapter.js" && cmp -s "$giz" "$S/xite-gizmo-adapter.js"; }
expect ok     staged-12-stage-is-repository-bytes same
expect ok     staged-12-exact-match-both-sources st
# the staged declaration check runs even when the bytes match: a source that
# itself lost the version (and so would have failed its own check) refuses.
stage; edit "$PS" "$decl" "$nover"; cp "$PS" "$S/xite-pick-adapter.js"; PSHA="$(sh_ "$PS")"
XG_WHY=1 expect refuse staged-declaration-check-on-stage st
stage; rm -rf "$S/xite-pick-adapter.js"; mkdir "$S/xite-pick-adapter.js"
expect refuse staged-directory-not-file        st
stage; mv "$S" "$t/stage-real"; ln -s "$t/stage-real" "$S"
expect refuse staged-dist-dir-symlink          st
rm -f "$S"; rm -rf "$t/stage-real"
stage; PSHA=deadbeef
expect refuse staged-bad-sha-argument          st

# (b) build-ui.sh itself, in a temp layout: fake cargo + wasm-bindgen, the
# synthetic X_ITE 9.9.9 kit above, adapters declaring 9.9.9, and a cp shim
# on PATH that tampers with ONE staged adapter (test-only; production code
# has no hook or delay). Every refusal must leave NO dist/.
bk="$t/bk"; ba="$bk/r/apps/desktop-tauri"
mkbk() {
  rm -rf "$bk"; mkdir -p "$ba/xite" "$ba/ui/static" "$ba/target/tools/bin" "$bk/r/src/preview" "$bk/bin"
  local f; for f in build-ui.sh check-xite-adapters.sh xite-js-mask.awk fetch-xite.sh; do cp "$here/$f" "$ba/"; done
  sed "s/'$ver'/'9.9.9'/" "$pick" > "$bk/r/src/preview/xite-pick-adapter.js"
  sed "s/'$ver'/'9.9.9'/" "$giz" > "$ba/ui/static/xite-gizmo-adapter.js"
  echo '<html></html>' > "$ba/ui/static/index.html"
  cp "$k/xite/PIN" "$k/xite/m.sha256" "$ba/xite/"
  printf '#!/bin/sh\nexit 0\n' > "$bk/bin/cargo"
  printf '#!/bin/sh\nfor x; do [ "$p" = --out-dir ] && d="$x"; p="$x"; done\necho js > "$d/wrlforge_ui.js"\n' > "$ba/target/tools/bin/wasm-bindgen"
  chmod +x "$bk/bin/cargo" "$ba/target/tools/bin/wasm-bindgen"
}
realcp="$(command -v cp)"
# bshim MODE TARGET [CONTENT]: install the tampering cp shim
bshim() {
  cat > "$bk/bin/cp" <<SH
#!/usr/bin/env bash
dest="\${@: -1}"; src=
for x in "\${@:1:\$#-1}"; do [[ "\${x##*/}" == "$2" ]] && src="\$x"; done
[[ -n "\$src" && "$1" == source-change ]] && printf '\n// changed after the source check\n' >> "\$src"
"$realcp" "\$@" || exit
[[ -n "\$src" ]] || exit 0
f="\$dest"; [[ -d "\$f" ]] && f="\$f/$2"
case "$1" in
  missing) rm -f "\$f" ;;
  change) printf 'x' >> "\$f" ;;
  replace) "$realcp" "${3:-}" "\$f" ;;
  symlink) "$realcp" "\$f" "$bk/outside.js"; rm -f "\$f"; ln -s "$bk/outside.js" "\$f" ;;
esac
SH
  chmod +x "$bk/bin/cp"
}
bb() { PATH="$bk/bin:$PATH" X_ITE_DIST="$t/good/dist" "$ba/build-ui.sh"; }
# brun ok|refuse NAME MODE TARGET [CONTENT]: build, then dist/ must exist
# (ok, with both staged adapters byte-identical to their sources) or not.
brun() {
  local want="$1" name="$2"; shift 2
  mkbk; [[ "$1" == none ]] || bshim "$@"
  expect "$want" "build-$name" bb
  if [[ "$want" == refuse ]]; then
    cp "$t/out" "$t/bout"   # refused by the STAGED check, not by anything else
    expect ok "build-$name-refused-by-staged-check" grep -q '^staged .*; refusing$\|/dist/xite-.*refusing$' "$t/bout"
    expect ok "build-$name-no-dist" test ! -e "$ba/dist"
  else
    bok() { cmp -s "$bk/r/src/preview/xite-pick-adapter.js" "$ba/dist/xite-pick-adapter.js" && cmp -s "$ba/ui/static/xite-gizmo-adapter.js" "$ba/dist/xite-gizmo-adapter.js" && [[ ! -L "$ba/dist/xite-pick-adapter.js" && ! -L "$ba/dist/xite-gizmo-adapter.js" ]]; }
    expect ok "build-$name-stage-matches-sources" bok
  fi
}
nov() { sed "s/, '9.9.9'//" "$bk/r/src/preview/xite-pick-adapter.js" > "$t/b-nover.js"; }
brun ok     01-both-valid           none
brun refuse 02-pick-missing         missing xite-pick-adapter.js
brun refuse 03-gizmo-missing        missing xite-gizmo-adapter.js
brun refuse 04-pick-changed         change  xite-pick-adapter.js
brun refuse 05-gizmo-changed        change  xite-gizmo-adapter.js
mkbk; nov; brun refuse 06-pick-version-removed replace xite-pick-adapter.js "$t/b-nover.js"
mkbk; sed "s/'9.9.9'/'0.0.1'/" "$ba/ui/static/xite-gizmo-adapter.js" > "$t/b-giz.js"
brun refuse 07-gizmo-wrong-version  replace xite-gizmo-adapter.js "$t/b-giz.js"
mkbk; nov; printf "// const XITE_VERSIONS = Object.freeze(['15.1.10', '9.9.9']);\n" >> "$t/b-nover.js"
brun refuse 08-version-only-in-comment replace xite-pick-adapter.js "$t/b-nover.js"
mkbk; { cat "$bk/r/src/preview/xite-pick-adapter.js"; echo '// staged-only comment'; } > "$t/b-cmt.js"
brun refuse 09-changed-bytes-valid-declaration replace xite-pick-adapter.js "$t/b-cmt.js"
brun refuse 10-pick-symlink         symlink xite-pick-adapter.js
brun refuse 10-gizmo-symlink        symlink xite-gizmo-adapter.js
brun refuse 11-pick-source-changed-during-staging  source-change xite-pick-adapter.js
brun refuse 11-gizmo-source-changed-during-staging source-change xite-gizmo-adapter.js
brun ok     12-exact-match-both-sources none
# the build-ui.sh order: source SHAs, then the source check, then the copies,
# then the staged check (which removes dist/ on refusal), then success.
order() { awk '/pick_sha="\$\(sha256/ { a = NR } /check-xite-adapters.sh" "\$pick" "\$gizmo"/ { b = NR } /^cp "\$pick" "\$here\/dist\/xite-pick-adapter.js"/ { c = NR }
  /check-xite-adapters.sh" --staged "\$here\/dist" .*\|\| \{ rm -rf "\$here\/dist"; exit 4; \}/ { d = NR } /^built=1$/ { e = NR }
  END { exit !(a && a < b && b < c && c < d && d < e) }' "$here/build-ui.sh"; }
expect ok     build-ui-staged-check-order     order
printf '\nSUMMARY pass=%d fail=%d\n' "$pass" "$fail"
[[ $fail -eq 0 ]]
