#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# STACK-REVIEW-1: check that both X_ITE adapters accept the pinned build in
# their REAL declarations -- not in a comment or an unrelated string.
#   ./check-xite-adapters.sh PICK_ADAPTER GIZMO_ADAPTER VERSION
# PICK_ADAPTER must hold exactly one line
#   const XITE_VERSIONS = Object.freeze(['a', 'b', ...]);
# whose list has VERSION as one exact element. GIZMO_ADAPTER must hold exactly
# one line  var XITE_VERSION = 'VERSION';  Each line must be only that
# statement (no trailing comment, no leading `//`). No Node.js is used.
set -euo pipefail
pick="${1:?pick adapter}"; gizmo="${2:?gizmo adapter}"; ver="${3:?version}"
[[ "$ver" =~ ^[0-9]+(\.[0-9]+)*$ ]] || { echo "bad version '$ver'" >&2; exit 2; }

decl='^[[:space:]]*const XITE_VERSIONS = Object\.freeze\(\[([^]]*)\]\);[[:space:]]*$'
lines="$(tr -d '\r' < "$pick" | sed -nE "\#$decl#p")"
count=0; [[ -z "$lines" ]] || count="$(printf '%s\n' "$lines" | wc -l)"
[[ "$count" -eq 1 ]] || { echo "$pick: expected exactly one XITE_VERSIONS declaration, found $count; refusing" >&2; exit 4; }
[[ "$lines" =~ $decl ]]; list="${BASH_REMATCH[1]}"
found=0
IFS=, read -ra items <<< "$list"
for item in "${items[@]}"; do
  item="$(printf '%s' "$item" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
  [[ "$item" =~ ^\'[0-9]+(\.[0-9]+)*\'$ ]] || { echo "$pick: XITE_VERSIONS element $item is not an exact version string; refusing" >&2; exit 4; }
  [[ "$item" == "'$ver'" ]] && found=1
done
[[ $found -eq 1 ]] || { echo "$pick: XITE_VERSIONS does not list X_ITE $ver; refusing" >&2; exit 4; }

gdecl="^[[:space:]]*var XITE_VERSION = '([0-9.]+)';[[:space:]]*\$"
glines="$(tr -d '\r' < "$gizmo" | sed -nE "\#$gdecl#p")"
count=0; [[ -z "$glines" ]] || count="$(printf '%s\n' "$glines" | wc -l)"
[[ "$count" -eq 1 ]] || { echo "$gizmo: expected exactly one XITE_VERSION declaration, found $count; refusing" >&2; exit 4; }
[[ "$glines" =~ $gdecl ]]
[[ "${BASH_REMATCH[1]}" == "$ver" ]] || { echo "$gizmo: XITE_VERSION is ${BASH_REMATCH[1]}, not $ver; refusing" >&2; exit 4; }
