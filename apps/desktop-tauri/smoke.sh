#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# In-window end-to-end smoke test on DISPOSABLE copies (never a user file).
# Usage: ./smoke.sh [--headless] --create      (VISUAL-1 New World → Create workflow)
#        ./smoke.sh [--headless] --pick        (VISUAL-2 viewport picking; needs node for the oracle plan)
#        ./smoke.sh [--headless] [--no-preview] [--inspector]
#                   [--theme FINAL_ID [--theme-expect ID] [--theme-notice] [--theme-save-fails]]
#                   [--config-dir DIR] file.wrl...
# Settings (theme) go to a fresh TEMPORARY config dir per file unless
# --config-dir is given: a smoke run never touches the user's preferences.
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
bin="${WRLFORGE_BIN:-$here/target/debug/wrl-forge}"
xvfb=(); extra=(); cfg=""; create=0; pick=0
while [[ "${1:-}" == --* ]]; do
  case "$1" in
    --headless) xvfb=(xvfb-run -a -s "-screen 0 1600x1000x24");;
    --no-preview) extra+=(--smoke-no-preview);;
    --inspector) extra+=(--smoke-inspector);;
    --theme) extra+=(--smoke-theme "$2"); shift;;
    --theme-expect) extra+=(--smoke-theme-expect "$2"); shift;;
    --theme-notice) extra+=(--smoke-theme-notice);;
    --theme-save-fails) extra+=(--smoke-theme-save-fails);;
    --config-dir) cfg="$2"; shift;;
    --create) create=1;;
    --pick) pick=1;;
  esac; shift
done
work="$(mktemp -d /tmp/wrlforge-tauri-smoke.XXXXXX)"
case "${cfg:-/tmp/}" in /tmp/*) ;; *) echo "refusing non-/tmp --config-dir: $cfg" >&2; exit 2;; esac
fail=0
if [[ $create -eq 1 ]]; then
  # No input file: the run starts from New World and saves into its own
  # disposable directory, which Rust verifies afterwards.
  d="$work/create.d"; mkdir -p "$d/config"
  timeout 240 "${xvfb[@]}" "$bin" --smoke-create "$d" --smoke-report "$d/report.json" --config-dir "$d/config" >"$d/stdout.txt" 2>"$d/stderr.txt"
  code=$?
  pass=$(sed -n 's/.*"pass": *\(true\|false\).*/\1/p' "$d/report.json" 2>/dev/null | head -1)
  echo "create: exit=$code pass=${pass:-none} report=$d/report.json"
  [[ $code -eq 0 && "$pass" == "true" ]] || fail=1
fi
if [[ $pick -eq 1 ]]; then
  # The oracle plan (WD2-C0 fixtures, spans by authorship) is written into a
  # disposable directory; the app opens the fixtures by index from it.
  d="$work/pick.d"; mkdir -p "$d/plan" "$d/config"
  node "$here/smoke-pick-plan.cjs" "$d/plan" || exit 2
  # Real X pointer input only inside this run's own Xvfb (see smoke.rs).
  marker=(); [[ ${#xvfb[@]} -gt 0 ]] && marker=(env WRLFORGE_SMOKE_XVFB=1)
  timeout 300 "${xvfb[@]}" "${marker[@]}" "$bin" --smoke-pick "$d/plan" --smoke-report "$d/report.json" --config-dir "$d/config" >"$d/stdout.txt" 2>"$d/stderr.txt"
  code=$?
  pass=$(sed -n 's/.*"pass": *\(true\|false\).*/\1/p' "$d/report.json" 2>/dev/null | head -1)
  echo "pick: exit=$code pass=${pass:-none} report=$d/report.json"
  [[ $code -eq 0 && "$pass" == "true" ]] || fail=1
fi
for src in "$@"; do
  d="$work/$(basename "$src").d"; mkdir -p "$d"; cp "$src" "$d/"
  f="$d/$(basename "$src")"
  c="${cfg:-$d/config}"; mkdir -p "$c"
  timeout 120 "${xvfb[@]}" "$bin" --smoke "$f" --smoke-report "$d/report.json" --config-dir "$c" "${extra[@]}" >"$d/stdout.txt" 2>"$d/stderr.txt"
  code=$?
  pass=$(sed -n 's/.*"pass": *\(true\|false\).*/\1/p' "$d/report.json" 2>/dev/null | head -1)
  echo "$(basename "$src"): exit=$code pass=${pass:-none} report=$d/report.json"
  [[ $code -eq 0 && "$pass" == "true" ]] || fail=1
done
exit $fail
