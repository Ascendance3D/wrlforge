#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# STACK-REVIEW-1: check that both X_ITE adapters accept the pinned build in
# their REAL declarations -- not in a comment, a string, a template literal or
# a nested (possibly dead) block.
#   ./check-xite-adapters.sh PICK_ADAPTER GIZMO_ADAPTER VERSION
# Each file is first masked by xite-js-mask.awk (a small JavaScript lexer), so
# only executable code is matched. Then each file must:
#   - be exactly one top-level IIFE: ['use strict';] (function () { ... })();
#   - assign its name exactly once anywhere (no second declaration, no
#     reassignment, in any shape);
#   - hold that one assignment as a direct statement of the IIFE body (brace
#     depth 1), on one line, in exactly this shape:
#       PICK:  const XITE_VERSIONS = Object.freeze(['a', 'b', ...]);
#       GIZMO: var XITE_VERSION = 'VERSION';
#     with VERSION as one exact element. Comments around it are allowed.
# No Node.js is used: bash, tr and POSIX awk only.
#
#   ./check-xite-adapters.sh --staged DIST PICK_SRC PICK_SHA GIZMO_SRC GIZMO_SHA VERSION
# STACK-REVIEW-1 QA HOLD fix: check the FINAL copies that ship,
# DIST/xite-pick-adapter.js and DIST/xite-gizmo-adapter.js. Each must be a
# regular file (not a symlink, not missing), have the SHA-256 its source had
# when the build checked it (PICK_SHA / GIZMO_SHA, taken BEFORE that check),
# and be byte-identical to its repository source now. Then the declaration
# check below runs again, on the staged files themselves.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
if [[ "${1:-}" == "--staged" ]]; then
  dist="${2:?dist}"; ver="${7:?version}"
  [[ -d "$dist" && ! -L "$dist" ]] || { echo "staged dir $dist is not a real directory (symlink or missing); refusing" >&2; exit 4; }
  sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }
  staged() { # staged NAME SRC SHA
    local f="$dist/$1" src="$2" want="$3"
    [[ "$want" =~ ^[0-9a-f]{64}$ ]] || { echo "bad SHA-256 '$want' for $1; refusing" >&2; return 4; }
    [[ ! -L "$f" ]] || { echo "staged $f is a symlink; refusing" >&2; return 4; }
    [[ -f "$f" ]] || { echo "staged $f is missing or not a regular file; refusing" >&2; return 4; }
    [[ "$(sha256 "$f")" == "$want" ]] || { echo "staged $f is not the source the build checked (SHA-256 differs); refusing" >&2; return 4; }
    cmp -s "$src" "$f" || { echo "staged $f is not byte-identical to $src; refusing" >&2; return 4; }
  }
  staged xite-pick-adapter.js "${3:?pick source}" "${4:?pick sha}" || exit 4
  staged xite-gizmo-adapter.js "${5:?gizmo source}" "${6:?gizmo sha}" || exit 4
  exec "$0" "$dist/xite-pick-adapter.js" "$dist/xite-gizmo-adapter.js" "$ver"
fi
pick="${1:?pick adapter}"; gizmo="${2:?gizmo adapter}"; ver="${3:?version}"
[[ "$ver" =~ ^[0-9]+(\.[0-9]+)*$ ]] || { echo "bad version '$ver'" >&2; exit 2; }

# check FILE NAME DECL_REGEX -> prints the regex group 1 of the one declaration
check() {
  local f="$1" name="$2" decl="$3" masked top uses lines
  masked="$(tr -d '\r' < "$f" | F="$f" awk -f "$here/xite-js-mask.awk")" || { echo "$f: cannot lex; refusing" >&2; return 4; }
  # split each masked line at its FIRST tab only: code may hold tabs
  top="$(printf '%s\n' "$masked" | awk '{ t = index($0, "\t") } substr($0, 1, t - 1) == "TOP" { print substr($0, t + 1) }')"
  [[ "$top" == "(function(){})();" || "$top" == "'_';(function(){})();" ]] \
    || { echo "$f: top level is not one IIFE ($top); refusing" >&2; return 4; }
  # every write to NAME, in any shape: = (not == or =>), a compound
  # assignment (+= ... ??=) or ++/--. A <= or >= also counts: fail closed.
  uses="$(printf '%s\n' "$masked" | N="$name" awk '{ e = index($0, "\t"); n = ENVIRON["N"]; s = r = substr($0, e + 1) }
    substr($0, 1, e - 1) != "TOP" {
      while (match(s, "(^|[^A-Za-z0-9_$.])" n "[ \t]*([-+*/%&|^<>?]*=|\\+\\+|--)")) {
        c++; s = substr(s, RSTART + RLENGTH); if (s ~ /^[=>]/) c-- }
      while (match(r, "(\\+\\+|--)[ \t]*" n "($|[^A-Za-z0-9_$])")) { c++; r = substr(r, RSTART + RLENGTH) } }
    END { print c + 0 }')"
  [[ "$uses" -eq 1 ]] || { echo "$f: expected exactly one $name assignment, found $uses; refusing" >&2; return 4; }
  # ENVIRON, not -v: -v would rewrite the regex backslashes
  lines="$(printf '%s\n' "$masked" | RE="$decl" awk '{ t = index($0, "\t"); s = substr($0, t + 1) } substr($0, 1, t - 1) == "1" && s ~ ENVIRON["RE"] { print s }')"
  [[ -n "$lines" && "$lines" != *$'\n'* && "$lines" =~ $decl ]] \
    || { echo "$f: the $name assignment is not the one-line declaration at IIFE top level; refusing" >&2; return 4; }
  printf '%s\n' "${BASH_REMATCH[1]}"
}

list="$(check "$pick" XITE_VERSIONS '^[[:space:]]*const XITE_VERSIONS = Object\.freeze\(\[([^]]*)\]\);[[:space:]]*$')" || exit 4
found=0
IFS=, read -ra items <<< "$list"
for item in "${items[@]}"; do
  item="$(printf '%s' "$item" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
  [[ "$item" =~ ^\'[0-9]+(\.[0-9]+)*\'$ ]] || { echo "$pick: XITE_VERSIONS element $item is not an exact version string; refusing" >&2; exit 4; }
  [[ "$item" == "'$ver'" ]] && found=1
done
[[ $found -eq 1 ]] || { echo "$pick: XITE_VERSIONS does not list X_ITE $ver; refusing" >&2; exit 4; }

gver="$(check "$gizmo" XITE_VERSION "^[[:space:]]*var XITE_VERSION = '([0-9]+(\\.[0-9]+)*)';[[:space:]]*\$")" || exit 4
[[ "$gver" == "$ver" ]] || { echo "$gizmo: XITE_VERSION is $gver, not $ver; refusing" >&2; exit 4; }
