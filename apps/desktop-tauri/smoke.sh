#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# In-window end-to-end smoke test on DISPOSABLE copies (never a user file).
# Usage: ./smoke.sh [--headless] --create      (VISUAL-1 New World → Create workflow)
#        ./smoke.sh [--headless] --pick        (VISUAL-2 viewport picking; needs node for the oracle plan)
#        ./smoke.sh [--headless] --move        (VISUAL-3A translation gizmo; real X input needs xdotool)
#        ./smoke.sh [--headless] --extrusion [--research DIR]
#                   (VISUAL-4A Extrusion create / Inspector / outline editor;
#                    --research renders copies of DIR/*.wrl or *.txt read-only)
#        ./smoke.sh [--headless] --rotscale  (VISUAL-4B Rotate / Scale tools; real X input needs xdotool)
#        ./smoke.sh [--headless] --recovery  (RUST-STAB-1: SIGKILL after a confirmed recovery
#                   write, then Restore / Discard / Later in two new processes)
#        ./smoke.sh [--headless] --dock      (UI-DOCK-1: docking, tabs, dividers, presets and
#                   source safety with real X input; then a restart and a damaged layout)
#        ./smoke.sh [--headless] --menu      (UI-MENU-1: menu bar, shortcuts, clipboard, Quit
#                   and menu / toolbar equivalence with real X input)
#        ./smoke.sh [--headless] [--no-preview] [--inspector]
#                   [--theme FINAL_ID [--theme-expect ID] [--theme-notice] [--theme-save-fails]]
#                   [--config-dir DIR] file.wrl...
# Settings (theme) go to a fresh TEMPORARY config dir per file unless
# --config-dir is given: a smoke run never touches the user's preferences.
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
bin="${WRLFORGE_BIN:-$here/target/debug/wrl-forge}"
xvfb=(); extra=(); cfg=""; create=0; pick=0; move=0; extrusion=0; rotscale=0; recovery=0; dock=0; menu=0; research=""
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
    --move) move=1;;
    --extrusion) extrusion=1;;
    --rotscale) rotscale=1;;
    --recovery) recovery=1;;
    --dock) dock=1;;
    --menu) menu=1;;
    --research) research="$2"; shift;;
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
if [[ $move -eq 1 ]]; then
  # Rust writes the source-format fixtures into this disposable directory.
  d="$work/move.d"; mkdir -p "$d/run" "$d/config"
  marker=(); [[ ${#xvfb[@]} -gt 0 ]] && marker=(env WRLFORGE_SMOKE_XVFB=1)
  timeout 400 "${xvfb[@]}" "${marker[@]}" "$bin" --smoke-move "$d/run" --smoke-report "$d/report.json" --config-dir "$d/config" >"$d/stdout.txt" 2>"$d/stderr.txt"
  code=$?
  pass=$(sed -n 's/.*"pass": *\(true\|false\).*/\1/p' "$d/report.json" 2>/dev/null | head -1)
  echo "move: exit=$code pass=${pass:-none} report=$d/report.json"
  [[ $code -eq 0 && "$pass" == "true" ]] || fail=1
fi
if [[ $extrusion -eq 1 ]]; then
  # Rust writes the source-format fixtures into this disposable directory;
  # research files are COPIED in first (the originals are never opened).
  d="$work/extrusion.d"; mkdir -p "$d/run" "$d/config"
  if [[ -n "$research" ]]; then
    for f in "$research"/*; do
      [[ -f "$f" ]] || continue
      cp "$f" "$d/run/research-$(basename "${f%.*}").wrl"
    done
  fi
  marker=(); [[ ${#xvfb[@]} -gt 0 ]] && marker=(env WRLFORGE_SMOKE_XVFB=1)
  timeout 400 "${xvfb[@]}" "${marker[@]}" "$bin" --smoke-extrusion "$d/run" --smoke-report "$d/report.json" --config-dir "$d/config" >"$d/stdout.txt" 2>"$d/stderr.txt"
  code=$?
  pass=$(sed -n 's/.*"pass": *\(true\|false\).*/\1/p' "$d/report.json" 2>/dev/null | head -1)
  echo "extrusion: exit=$code pass=${pass:-none} report=$d/report.json"
  [[ $code -eq 0 && "$pass" == "true" ]] || fail=1
fi
if [[ $rotscale -eq 1 ]]; then
  # Rust writes the source-format fixtures into this disposable directory.
  d="$work/rotscale.d"; mkdir -p "$d/run" "$d/config"
  marker=(); [[ ${#xvfb[@]} -gt 0 ]] && marker=(env WRLFORGE_SMOKE_XVFB=1)
  timeout 400 "${xvfb[@]}" "${marker[@]}" "$bin" --smoke-rotscale "$d/run" --smoke-report "$d/report.json" --config-dir "$d/config" >"$d/stdout.txt" 2>"$d/stderr.txt"
  code=$?
  pass=$(sed -n 's/.*"pass": *\(true\|false\).*/\1/p' "$d/report.json" 2>/dev/null | head -1)
  echo "rotscale: exit=$code pass=${pass:-none} report=$d/report.json"
  [[ $code -eq 0 && "$pass" == "true" ]] || fail=1
fi
if [[ $recovery -eq 1 ]]; then
  # One disposable run dir and ONE disposable config dir for all three
  # processes, so the second and third start see what the first left.
  d="$work/recovery.d"; mkdir -p "$d/config"
  # (1) Edit, wait for the background recovery writes, then die by SIGKILL.
  "${xvfb[@]}" "$bin" --smoke-recovery-crash "$d" --config-dir "$d/config" >"$d/crash-stdout.txt" 2>"$d/crash-stderr.txt" &
  wrapper=$!
  for _ in $(seq 1 600); do [[ -f "$d/ready.json" ]] && break; sleep 0.1; done
  pid=$(sed -n 's/.*"pid": *\([0-9]*\).*/\1/p' "$d/ready.json" 2>/dev/null | head -1)
  ready=$(sed -n 's/.*"pass": *\(true\|false\).*/\1/p' "$d/ready.json" 2>/dev/null | head -1)
  killed=no
  if [[ -n "$pid" && "$(cat /proc/$pid/comm 2>/dev/null)" == wrl-forge* ]]; then
    kill -KILL "$pid" && killed=yes
  fi
  wait "$wrapper"; crash_code=$?
  for _ in $(seq 1 50); do [[ -e "/proc/$pid" ]] || break; sleep 0.1; done
  gone=yes; [[ -n "$pid" && -e "/proc/$pid" ]] && gone=no
  recs=$(find "$d/config/recovery" -maxdepth 1 -type f -name 'rec-*.wfr' 2>/dev/null | wc -l)
  echo "recovery crash: ready=${ready:-none} pid=${pid:-none} sigkill=$killed exit=$crash_code gone=$gone records=$recs"
  [[ "$ready" == "true" && "$killed" == "yes" && "$gone" == "yes" && "$recs" -eq 4 ]] || fail=1
  # A damaged record next to the real ones (one flipped payload byte).
  first=$(find "$d/config/recovery" -maxdepth 1 -type f -name 'rec-*.wfr' -printf '%f\n' | sort | head -1)
  bad="rec-$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n').wfr"
  cp "$d/config/recovery/$first" "$d/config/recovery/$bad"
  size=$(stat -c %s "$d/config/recovery/$bad")
  printf 'X' | dd of="$d/config/recovery/$bad" bs=1 seek=$((size - 1)) conv=notrunc status=none
  # (2) Restore / edit + Undo / Save As / Discard / canceled close / Later.
  # (3) A third start: what Later and the canceled close kept is offered again.
  for phase in restore later; do
    timeout 180 "${xvfb[@]}" "$bin" --smoke-recovery-$phase "$d" --smoke-report "$d/$phase.json" --config-dir "$d/config" >"$d/$phase-stdout.txt" 2>"$d/$phase-stderr.txt"
    code=$?
    pass=$(sed -n 's/.*"pass": *\(true\|false\).*/\1/p' "$d/$phase.json" 2>/dev/null | head -1)
    echo "recovery $phase: exit=$code pass=${pass:-none} report=$d/$phase.json"
    [[ $code -eq 0 && "$pass" == "true" ]] || fail=1
  done
fi
if [[ $dock -eq 1 ]]; then
  # One disposable run dir and ONE config dir for the three processes: the
  # restart must see what the first run saved.
  d="$work/dock.d"; mkdir -p "$d/run" "$d/config"
  marker=(); [[ ${#xvfb[@]} -gt 0 ]] && marker=(env WRLFORGE_SMOKE_XVFB=1)
  for phase in main restore damaged; do
    flag=--smoke-dock; [[ $phase != main ]] && flag=--smoke-dock-$phase
    if [[ $phase == damaged ]]; then
      # A theme the run must keep, next to a layout with impossible sizes.
      cat > "$d/config/settings.json" <<'JSON'
{"schemaVersion":1,"themeId":"tokyo-night-storm","layout":{"version":1,"preset":"preset.source","root":{"type":"split","axis":"row","sizes":[1.5,-0.5],"children":[{"type":"tabs","panels":["panel.source"],"active":"panel.source"},{"type":"tabs","panels":["panel.viewport"],"active":"panel.viewport"}]}}}
JSON
    fi
    timeout 400 "${xvfb[@]}" "${marker[@]}" "$bin" $flag "$d/run" --smoke-report "$d/$phase.json" --config-dir "$d/config" >"$d/$phase-stdout.txt" 2>"$d/$phase-stderr.txt"
    code=$?
    pass=$(sed -n 's/.*"pass": *\(true\|false\).*/\1/p' "$d/$phase.json" 2>/dev/null | head -1)
    echo "dock $phase: exit=$code pass=${pass:-none} report=$d/$phase.json"
    [[ $code -eq 0 && "$pass" == "true" ]] || fail=1
    # The first run's edit left a recovery record (checked for layout data
    # above); keep it as evidence, out of the restart's way.
    [[ -d "$d/config/recovery" ]] && mv "$d/config/recovery" "$d/recovery-$phase"
  done
fi
if [[ $menu -eq 1 ]]; then
  # One disposable run dir and its own config dir (the recovery records the
  # canceled Quit must keep live there).
  d="$work/menu.d"; mkdir -p "$d/run" "$d/config"
  marker=(); [[ ${#xvfb[@]} -gt 0 ]] && marker=(env WRLFORGE_SMOKE_XVFB=1)
  timeout 400 "${xvfb[@]}" "${marker[@]}" "$bin" --smoke-menu "$d/run" --smoke-report "$d/report.json" --config-dir "$d/config" >"$d/stdout.txt" 2>"$d/stderr.txt"
  code=$?
  pass=$(sed -n 's/.*"pass": *\(true\|false\).*/\1/p' "$d/report.json" 2>/dev/null | head -1)
  echo "menu: exit=$code pass=${pass:-none} report=$d/report.json"
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
