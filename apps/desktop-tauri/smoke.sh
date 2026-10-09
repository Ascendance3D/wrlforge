#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# In-window end-to-end smoke test on DISPOSABLE copies (never a user file).
# Usage: ./smoke.sh [--headless] [--no-preview] file.wrl...
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
bin="$here/target/debug/wrl-forge"
xvfb=(); extra=()
while [[ "${1:-}" == --* ]]; do
  case "$1" in --headless) xvfb=(xvfb-run -a -s "-screen 0 1600x1000x24");; --no-preview) extra+=(--smoke-no-preview);; esac; shift
done
work="$(mktemp -d /tmp/wrlforge-tauri-smoke.XXXXXX)"
fail=0
for src in "$@"; do
  d="$work/$(basename "$src").d"; mkdir -p "$d"; cp "$src" "$d/"
  f="$d/$(basename "$src")"
  timeout 120 "${xvfb[@]}" "$bin" --smoke "$f" --smoke-report "$d/report.json" "${extra[@]}" >"$d/stdout.txt" 2>"$d/stderr.txt"
  code=$?
  pass=$(sed -n 's/.*"pass": *\(true\|false\).*/\1/p' "$d/report.json" 2>/dev/null | head -1)
  echo "$(basename "$src"): exit=$code pass=${pass:-none} report=$d/report.json"
  [[ $code -eq 0 && "$pass" == "true" ]] || fail=1
done
exit $fail
