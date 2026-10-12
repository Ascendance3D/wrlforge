#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Differential parity: Rust wrlforge-vrml vs JS src/vrml/parser.js.
# READ-ONLY over the given roots. Gzip is inflated to a temp pipe; files that
# are not valid UTF-8 are skipped (the Rust app refuses to open them).
# Usage: parity.sh <root>...    prints a summary; first mismatches in $OUT
set -u
here="$(cd "$(dirname "$0")" && pwd)"; repo="$here/../.."
rs="$repo/crates/target/release/examples/parity_dump"
OUT="${OUT:-/tmp/wrlforge-parity}"; mkdir -p "$OUT"; : > "$OUT/mismatch.txt"
n=0; same=0; diffc=0; skip=0
while IFS= read -r -d '' f; do
  t="$OUT/cur.txt"
  if [ "$(head -c2 "$f" | od -An -tx1 | tr -d ' \n')" = "1f8b" ]; then gzip -dc "$f" > "$t" 2>/dev/null || { skip=$((skip+1)); continue; }
  else cat "$f" > "$t"; fi
  iconv -f UTF-8 -t UTF-8 "$t" >/dev/null 2>&1 || { skip=$((skip+1)); continue; }
  n=$((n+1))
  if cmp -s <("$rs" < "$t") <(node "$here/parity-dump.js" < "$t"); then same=$((same+1))
  else diffc=$((diffc+1)); [ $diffc -le 20 ] && { echo "== $f" >> "$OUT/mismatch.txt"; diff <("$rs" < "$t") <(node "$here/parity-dump.js" < "$t") | head -8 >> "$OUT/mismatch.txt"; }
  fi
done < <(find "$@" -type f \( -iname '*.wrl' -o -iname '*.wrz' -o -iname '*.wrl.gz' \) -print0 | sort -z)
echo "files=$n identical=$same different=$diffc skipped_non_utf8_or_bad_gzip=$skip"
