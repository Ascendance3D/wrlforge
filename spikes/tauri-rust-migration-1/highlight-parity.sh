#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Highlight parity (UI-SYNTAX-1): Rust wrlforge-vrml::highlight vs the JS
# editor's src/editor/language.js. READ-ONLY over the given roots; plain
# UTF-8 .wrl only (gzip inflated to a temp file).
# Usage: highlight-parity.sh <root>...
set -u
here="$(cd "$(dirname "$0")" && pwd)"; repo="$here/../.."
rs="$repo/crates/target/release/examples/highlight_dump"
OUT="${OUT:-/tmp/wrlforge-highlight-parity}"; mkdir -p "$OUT"; : > "$OUT/mismatch.txt"
n=0; same=0; diffc=0; skip=0
while IFS= read -r -d '' f; do
  t="$OUT/cur.txt"
  if [ "$(head -c2 "$f" | od -An -tx1 | tr -d ' \n')" = "1f8b" ]; then gzip -dc "$f" > "$t" 2>/dev/null || { skip=$((skip+1)); continue; }
  else cat "$f" > "$t"; fi
  iconv -f UTF-8 -t UTF-8 "$t" >/dev/null 2>&1 || { skip=$((skip+1)); continue; }
  n=$((n+1))
  "$rs" < "$t" > "$OUT/rust.txt"
  if node "$here/highlight-compare.js" "$OUT/rust.txt" < "$t" > "$OUT/cmp.txt"; then same=$((same+1))
  else diffc=$((diffc+1)); { echo "== $f"; head -5 "$OUT/cmp.txt"; } >> "$OUT/mismatch.txt"; fi
done < <(find "$@" -type f \( -iname '*.wrl' -o -iname '*.wrz' -o -iname '*.wrl.gz' \) -print0 | sort -z)
echo "files=$n consistent=$same different=$diffc skipped_non_utf8_or_bad_gzip=$skip"
